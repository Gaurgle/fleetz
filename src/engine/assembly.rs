//! The view assembly: GitHub's active repos matched to local checkouts in
//! three passes, ordered by activity, with the startup fetch baseline.

use super::Sink;
use super::carriers::{RemoteKey, ambiguity_line, mark_local_carriers, names_remote, remote_key};
use super::fetch::{FetchClaims, fetch_under_claim_capped};
use super::status::{build_repo_status, disk_usage_of, local_only_status, tilde_display};
use crate::cli::{Args, Mode};
use crate::model::{self, RepoLocation, RepoStatus};
use crate::{config, discovery, git, github, time};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Parallel git-status probes: enough to hide subprocess latency, capped so a
/// big fleet does not spawn hundreds of processes at once.
pub(crate) fn probe_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 12)
}

/// Network fetches are remote-bound, not CPU-bound: keep it polite.
pub(crate) const FETCH_CONCURRENCY: usize = 4;

/// What an assembly says besides its rows: the header, and the ambiguity
/// status line that `LoadResult::ambiguity` carries to the app.
pub(crate) struct ViewNotes {
    pub(crate) header: String,
    pub(crate) ambiguity: Option<String>,
}

/// Which of `targets` are worth spending a `git fetch` on before the first
/// paint.
///
/// Startup used to fetch every repo past the cutoff and await all of them
/// before statting anything -- around 0.8s of network per repo at
/// `FETCH_CONCURRENCY`, so a wide time window put six seconds of nothing in
/// front of the fleet while a narrow one cost under a second. The window was
/// never the real variable: it only decides how many repos reach this loop.
///
/// `remotes_that_moved` already answers this for every later poll by diffing
/// two GitHub datasets, but the first poll of a session has no previous
/// dataset to diff against, so it fell back to fetching everything. There is a
/// baseline on disk, though: `FETCH_HEAD`'s mtime is when this clone last
/// fetched. A remote GitHub says was pushed before then is already in hand.
///
/// Unknowns fetch. No `pushed_at` means no GitHub match (a local-only clone,
/// where the remote could be anywhere), and no `FETCH_HEAD` means this clone
/// has never fetched at all; both have to be read as stale. So does an empty
/// `FETCH_HEAD`, which is what a failed fetch leaves behind
/// (`git::last_successful_fetch_at`, rule 7 of the fetch contract).
///
/// Clock skew is survivable in the one direction it can bite: a local clock
/// running ahead can make a needed fetch look unnecessary, and the next poll
/// picks it up, because that comparison is GitHub-to-GitHub and immune.
pub(super) fn stale_remotes(
    targets: &[(discovery::FoundRepo, RepoLocation)],
    active: &[github::GithubRepo],
) -> Vec<std::path::PathBuf> {
    targets
        .iter()
        .filter(|(found, _)| {
            // The repo this clone claimed, by owner and name: a same-named
            // repo of another owner pushed at another time.
            let Some(pushed_at) = active
                .iter()
                .find(|r| names_remote(found.owner.as_deref(), &found.name, r))
                .map(|r| r.pushed_at)
            else {
                return true;
            };
            match git::last_successful_fetch_at(&found.path) {
                Some(fetched) => pushed_at > chrono::DateTime::<chrono::Utc>::from(fetched),
                None => true,
            }
        })
        .map(|(found, _)| found.path.clone())
        .collect()
}

/// Cutoff + header for the current view. An active TimeWindow (set by the
/// in-TUI selector or loaded from state) takes precedence over the launch
/// mode; Dirty mode never reaches here.
pub(crate) fn resolve_cutoff(
    window: Option<time::TimeWindow>,
    mode: &Mode,
    config: &config::Config,
) -> Result<(Option<chrono::DateTime<chrono::Utc>>, String), String> {
    if let Some(w) = window {
        return Ok((
            Some(w.cutoff(config.work_start, config.work_end)),
            w.label(config.work_start, config.work_end),
        ));
    }
    match mode {
        Mode::Auto => {
            let (c, h) = time::auto_cutoff(config.work_start, config.work_end);
            Ok((Some(c), h))
        }
        Mode::Since(s) => {
            let date = time::parse_relative_date(s)?;
            Ok((Some(time::date_to_utc_midnight(date)), format!("since {s}")))
        }
        Mode::Today => {
            let today = chrono::Local::now().date_naive();
            Ok((Some(time::date_to_utc_midnight(today)), "today".to_string()))
        }
        Mode::All => Ok((None, "all divergence".to_string())),
        Mode::Dirty => Err("dirty mode has no time cutoff".to_string()),
    }
}

/// Everything downstream of the GitHub fetch: time filtering, local
/// discovery, optional per-repo git fetch, statting, and header assembly.
/// `git_fetch: false` makes this purely local (used by the time-window
/// overlay so applying a window is instant).
/// Order discovered repos newest-pushed first, the way repoz lists them.
///
/// `active` arrives in that order already (the GraphQL `orderBy`, re-applied
/// by `merge_sources`), but the ordering never reached the screen:
/// `find_in_primary` reduces the names to a `HashSet` for a membership test,
/// and the walkers then push repos in `read_dir` order. On APFS that is hash
/// order, so the "github activity" section was in no order at all, and it
/// reshuffled whenever a repo was added or removed from the scan root.
///
/// Applied here rather than in `App::visible_repos` because the section sort
/// there is stable and simply carries this through, and *after* the pass-1 /
/// pass-2 dedupe because that is what makes a local clone beat the not-cloned
/// fallback -- reordering before it would break the load-bearing pass order.
///
/// Repos with no GitHub match sort last and keep discovery order, `sort_by_key`
/// being stable. In practice they are the local-only rows, which `section_rank`
/// puts in their own section anyway.
pub(super) fn sort_by_activity(
    targets: &mut [(discovery::FoundRepo, RepoLocation)],
    active: &[github::GithubRepo],
) {
    targets.sort_by_cached_key(|(f, _)| activity_position(f.owner.as_deref(), &f.name, active));
}

/// The activity rule both orderings share: a repo's place in `active`,
/// newest push first, by owner and name, so two same-named repos each sort
/// by their own push. `usize::MAX` for a repo `active` does not hold.
pub(crate) fn activity_position(
    owner: Option<&str>,
    name: &str,
    active: &[github::GithubRepo],
) -> usize {
    active
        .iter()
        .position(|r| names_remote(owner, name, r))
        .unwrap_or(usize::MAX)
}

/// Pass 3's candidates: the local repos passes 1-2 have not already shown,
/// each with the name it reports as (undotted, `discovery::repo_name_for_path`).
///
/// Deduped by canonical path only, as repoz does (D5 in
/// `docs/repository-identity-design.md`): a candidate is dropped when its
/// canonical path is the canonical path of a claimed row, or of an earlier
/// candidate, so one directory reached through two root spellings (a
/// symlink, `R/sub/..`, `/var` against `/private/var`) is listed once. The
/// raw path is the key when `canonicalize` fails. The key is for comparison
/// only: each candidate keeps the path as discovered, which is what the row
/// displays.
///
/// No name check: a dirty second checkout of a claimed repo, and two
/// same-named local-only repos, each get a row. Matching by path also keeps
/// out a clone passes 1-2 matched through its remote in a folder named
/// otherwise (`DevOps_Portfolio` for `portfolio-site`).
///
/// `checked_names` is ignored. It is the name set pass 3 used to filter by,
/// kept only so the signature existing callers use does not change.
pub(crate) fn unclaimed_local(
    all_local: Vec<PathBuf>,
    claimed_paths: &HashSet<&Path>,
    checked_names: &HashSet<String>,
) -> Vec<(PathBuf, String)> {
    let _ = checked_names;
    let key = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut seen: HashSet<PathBuf> = claimed_paths.iter().map(|path| key(path)).collect();
    all_local
        .into_iter()
        .filter(|path| seen.insert(key(path)))
        .map(|path| {
            let name = discovery::repo_name_for_path(&path);
            (path, name)
        })
        .collect()
}

#[allow(clippy::too_many_arguments)] // one call spine; a params struct would only rename the problem
pub(crate) async fn assemble_view(
    dataset: &[github::GithubRepo],
    coverage: Option<&github::Coverage>,
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
    header: String,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    git_fetch: bool,
    progress: Option<&(dyn Sink<ApplyProgress> + Sync)>,
    fetch_claims: &FetchClaims,
    size_held: &HashSet<String>,
) -> Result<(Vec<RepoStatus>, ViewNotes), String> {
    // Local scan roots: the primary repo_dir plus any configured extra dirs
    // (e.g. ~/.dotfiles) -- the fleetz equivalent of repoz's EXTRA_DIRS. Used
    // by all three passes, so a repo outside the main tree gets a full
    // GitHub-matched row in pass 1 rather than only surfacing under "local
    // changes" when it happens to be dirty.
    let mut scan_roots = vec![repo_dir.to_path_buf()];
    scan_roots.extend(config.resolved_extra_dirs());
    let home = dirs::home_dir().unwrap_or_default();

    let (repos, mut notes) = assemble_view_held(
        dataset,
        cutoff,
        header,
        args,
        config,
        &home,
        &scan_roots,
        git_fetch,
        progress,
        fetch_claims,
        size_held,
    )
    .await?;
    notes.header = with_coverage_note(notes.header, coverage, cutoff, config.repo_limit);
    Ok((repos, notes))
}

/// The header with the coverage note appended when the view can be missing
/// repos GitHub has (`github::coverage_note`), so an incomplete view never
/// masquerades as complete. No coverage (nothing fetched) adds nothing.
pub(crate) fn with_coverage_note(
    header: String,
    coverage: Option<&github::Coverage>,
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
    repo_limit: u32,
) -> String {
    match coverage.and_then(|coverage| github::coverage_note(coverage, cutoff, repo_limit)) {
        Some(note) => format!("{header} ({note})"),
        None => header,
    }
}

/// The body of `assemble_view`, with the two filesystem anchors it used to
/// read for itself passed in: `home` is where pass 2 sweeps, and `scan_roots`
/// (the repo dir first, then the configured extra dirs) is what passes 1 and 3
/// walk. This is the test seam: a test can run the whole assembly against a
/// temp home and temp roots instead of the real `$HOME`.
///
/// `_config` gives the fetch cap only (FZ-35b): the coverage note moved to
/// `assemble_view` (FZ-09b). The fetches run here under a claim table of
/// their own, with nothing held by size.
#[cfg(test)]
#[allow(clippy::too_many_arguments)] // one call spine; a params struct would only rename the problem
pub(crate) async fn assemble_view_in(
    dataset: &[github::GithubRepo],
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
    header: String,
    args: &Args,
    _config: &config::Config,
    home: &Path,
    scan_roots: &[PathBuf],
    git_fetch: bool,
    progress: Option<&(dyn Sink<ApplyProgress> + Sync)>,
) -> Result<(Vec<RepoStatus>, ViewNotes), String> {
    assemble_view_with(
        dataset,
        cutoff,
        header,
        args,
        _config,
        home,
        scan_roots,
        git_fetch,
        progress,
        &FetchClaims::default(),
    )
    .await
}

/// The body of `assemble_view` (see `assemble_view_in`, its test seam). Each
/// fetch it runs takes its claim in `fetch_claims` when its permit is
/// granted and gives it back when it ends (FZ-35a); a path a sweep is
/// fetching already is not fetched again, and its row records no fetch.
#[cfg(test)]
#[allow(clippy::too_many_arguments)] // one call spine; a params struct would only rename the problem
pub(super) async fn assemble_view_with(
    dataset: &[github::GithubRepo],
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
    header: String,
    args: &Args,
    config: &config::Config,
    home: &Path,
    scan_roots: &[PathBuf],
    git_fetch: bool,
    progress: Option<&(dyn Sink<ApplyProgress> + Sync)>,
    fetch_claims: &FetchClaims,
) -> Result<(Vec<RepoStatus>, ViewNotes), String> {
    assemble_view_held(
        dataset,
        cutoff,
        header,
        args,
        config,
        home,
        scan_roots,
        git_fetch,
        progress,
        fetch_claims,
        &HashSet::new(),
    )
    .await
}

/// `assemble_view_with`, whose fetches are capped at `config.fetch_max_mb`
/// and skip the display paths in `size_held`, which the fetch cap holds
/// (FZ-35b); a successful one records its row's size baseline.
#[allow(clippy::too_many_arguments)] // one call spine; a params struct would only rename the problem
pub(crate) async fn assemble_view_held(
    dataset: &[github::GithubRepo],
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
    header: String,
    args: &Args,
    config: &config::Config,
    home: &Path,
    scan_roots: &[PathBuf],
    git_fetch: bool,
    progress: Option<&(dyn Sink<ApplyProgress> + Sync)>,
    fetch_claims: &FetchClaims,
    size_held: &HashSet<String>,
) -> Result<(Vec<RepoStatus>, ViewNotes), String> {
    // Apply --before filter
    let before_cutoff = match &args.before {
        Some(b) => {
            let date = time::parse_relative_date(b)?;
            Some(time::date_to_utc_end_of_day(date))
        }
        None => None,
    };

    // Filter active repos
    let mut active: Vec<github::GithubRepo> = dataset.to_vec();
    if let Some(c) = cutoff {
        active = github::filter_since(&active, c);
    }
    if let Some(b) = before_cutoff {
        active = github::filter_before(&active, b);
    }

    // The full dataset rides along: whether a folder name is ambiguous, or an
    // origin names a known repo, must not depend on the window.
    let wanted = discovery::Wanted::with_full_dataset(
        active.iter().map(|r| (r.owner.as_deref(), r.name.as_str())),
        dataset
            .iter()
            .map(|r| (r.owner.as_deref(), r.name.as_str())),
    );

    // Pass 1: find under every scan root (repo_dir + extra_dirs)
    let (primary_found, mut ambiguities) =
        discovery::find_in_primary_with_ambiguities(scan_roots, &wanted);
    let found_set: HashSet<String> = primary_found
        .iter()
        .map(discovery::FoundRepo::key)
        .collect();

    // Pass 2: find elsewhere
    let (elsewhere_found, elsewhere_ambiguities) =
        discovery::find_elsewhere_with_ambiguities(home, scan_roots, &wanted, &found_set);
    ambiguities.extend(elsewhere_ambiguities);
    let ambiguity = ambiguity_line(&ambiguities, dataset);

    // Dedupe up front: a repo found in both passes is one stat target, so the
    // gauge total is the real amount of work rather than a count that has to
    // be padded as duplicates are skipped. Primary wins the location. Keyed
    // by owner and name, so `alice/api` and `company/api` are two targets.
    let mut targets: Vec<(discovery::FoundRepo, RepoLocation)> = Vec::new();
    let mut seen: HashSet<RemoteKey> = HashSet::new();
    for f in &primary_found {
        if seen.insert(remote_key(f.owner.as_deref(), &f.name)) {
            targets.push((f.clone(), RepoLocation::Primary));
        }
    }
    for f in &elsewhere_found {
        if seen.insert(remote_key(f.owner.as_deref(), &f.name)) {
            targets.push((f.clone(), RepoLocation::Elsewhere));
        }
    }
    sort_by_activity(&mut targets, &active);

    if let Some(tx) = progress {
        tx.send_event(ApplyProgress::Discovering);
        tx.send_event(ApplyProgress::Statting {
            done: 0,
            total: targets.len(),
        });
    }

    // How each fetch run here ended, by path, for the rows built below.
    let mut load_fetches: HashMap<PathBuf, git::FetchOutcome> = HashMap::new();
    if git_fetch {
        let stale = stale_remotes(&targets, &active)
            .into_iter()
            .filter(|path| !size_held.contains(&tilde_display(path)));
        let cap = config.fetch_cap_bytes();
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(FETCH_CONCURRENCY));
        let mut set = tokio::task::JoinSet::new();
        for path in stale {
            let sem = sem.clone();
            let claims = fetch_claims.clone();
            set.spawn(async move {
                let _permit = sem.acquire().await;
                let key = tilde_display(&path);
                let Some(token) = claims.reserve(&key, std::time::Instant::now()) else {
                    return (path, git::FetchOutcome::NotAttempted);
                };
                let outcome =
                    fetch_under_claim_capped(&claims, &key, &path, token, None, cap).await;
                claims.release(&key, Some(token));
                (path, outcome)
            });
        }
        while let Some(joined) = set.join_next().await {
            if let Ok((path, outcome)) = joined {
                load_fetches.insert(path, outcome);
            }
        }
    }
    let fetched_at = chrono::Local::now();

    // Stat every target in parallel behind a semaphore, keyed by index so the
    // output order matches the discovery order regardless of finish order.
    let active_shared = std::sync::Arc::new(active.clone());
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(probe_concurrency()));
    let mut set = tokio::task::JoinSet::new();
    for (i, (found, location)) in targets.iter().cloned().enumerate() {
        let sem = sem.clone();
        let active = active_shared.clone();
        let args = args.clone();
        set.spawn(async move {
            let _permit = sem.acquire().await;
            (i, build_repo_status(&found, &active, &args, location).await)
        });
    }

    let mut slots: Vec<Option<RepoStatus>> = vec![None; targets.len()];
    let mut stat_done = 0usize;
    while let Some(joined) = set.join_next().await {
        if let Ok((i, repo)) = joined {
            slots[i] = Some(repo);
            stat_done += 1;
            if let Some(tx) = progress {
                tx.send_event(ApplyProgress::Statting {
                    done: stat_done,
                    total: targets.len(),
                });
            }
        }
    }
    // Slots are in target order, so each row's fetch is found by its target.
    for (slot, (found, _)) in slots.iter_mut().zip(&targets) {
        if let (Some(repo), Some(outcome)) = (slot, load_fetches.get(&found.path)) {
            repo.fetch.record(outcome, fetched_at);
            if *outcome == git::FetchOutcome::Succeeded {
                repo.fetch.disk_baseline = disk_usage_of(repo, &active);
            }
        }
    }
    let mut repos: Vec<RepoStatus> = slots.into_iter().flatten().collect();

    if let Some(tx) = progress {
        tx.send_event(ApplyProgress::Finishing);
    }

    // Pass 3: local repos (under repo_dir or any configured extra_dir, e.g.
    // ~/.dotfiles) with uncommitted or unpushed changes, not already shown
    // above. Runs *before* the not-cloned loop so a dirty local clone whose
    // origin names an active repo carries it as LocalOnly rather than leaving
    // it mislabeled "not cloned" when it lives at a path passes 1/2 don't
    // reach.
    let all_local = discovery::find_all_local(scan_roots);
    let claimed_paths: HashSet<&Path> = targets.iter().map(|(f, _)| f.path.as_path()).collect();
    let candidates = unclaimed_local(all_local, &claimed_paths, &HashSet::new());
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(probe_concurrency()));
    let mut set = tokio::task::JoinSet::new();
    for (i, (path, name)) in candidates.iter().cloned().enumerate() {
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire().await;
            // No glyph by name: a LocalOnly row gets one only as a carrier,
            // from the repo its origin names, below.
            let status = local_only_status(&path, name, model::RepoKind::Unknown, cutoff).await;
            (i, status)
        });
    }

    // Back into discovery order. A clean repo gets no row, and every dirty
    // one does: `unclaimed_local` already deduped the candidates by path.
    let mut local_results: Vec<(usize, Option<RepoStatus>)> = Vec::new();
    while let Some(joined) = set.join_next().await {
        if let Ok(pair) = joined {
            local_results.push(pair);
        }
    }
    local_results.sort_by_key(|(i, _)| *i);
    let mut local_rows: Vec<(&Path, RepoStatus)> = Vec::new();
    for (i, status) in local_results {
        let Some(repo) = status else { continue };
        local_rows.push((candidates[i].0.as_path(), repo));
    }
    mark_local_carriers(&mut local_rows, dataset, scan_roots, &mut seen);
    repos.extend(local_rows.into_iter().map(|(_, repo)| repo));

    // Not-cloned repos: active on GitHub but carried by no row above. Last,
    // so a local clone always wins over this, but only through the remote it
    // carries: a pass 3 clone without an origin no longer hides the repo.
    for gh_repo in &active {
        if seen.contains(&remote_key(gh_repo.owner.as_deref(), &gh_repo.name)) {
            continue;
        }
        repos.push(RepoStatus {
            // No local calendar until the strip asks for one.
            daily: None,
            daily_key: None,
            last_local: None,
            name: gh_repo.name.clone(),
            path: None,
            location: RepoLocation::NotCloned,
            branches: Vec::new(),
            commits: Vec::new(),
            behind: 0,
            ahead: 0,
            dirty: 0,
            pr_count: gh_repo.pr_count,
            prs: gh_repo.prs.clone(),
            ci_status: gh_repo.ci_status.clone(),
            ci_workflow: gh_repo.ci_workflow.clone(),
            ci_duration: gh_repo.ci_duration.clone(),
            groups: vec![],
            stash_count: 0,
            stash_entries: vec![],
            worktrees: vec![],
            reflog: vec![],
            graph: vec![],
            graph_expanded: Vec::new(),
            repo_kind: gh_repo.repo_kind,
            owner: gh_repo.owner.clone(),
            remote_url: None,
            remote_only_branches: vec![],
            remote_branches: vec![],
            work_key: None,
            // Nothing on disk to fetch into.
            fetch: model::FetchFacts::default(),
            authors: Default::default(),
        });
    }

    // Build header with --before and --author if present
    let mut full_header = header;
    if let Some(ref b) = args.before {
        full_header = format!("{full_header} → before {b}");
    }
    if let Some(ref a) = args.author {
        full_header = format!("{full_header} (author: {a})");
    }
    // The coverage note goes last, in `assemble_view`, which has the payload's
    // coverage; the merged dataset here cannot say what was cut off.

    let notes = ViewNotes {
        header: full_header,
        ambiguity,
    };
    Ok((repos, notes))
}

/// Progress of an in-flight time-window apply, shown as a footer gauge.
/// `Statting` is the determinate phase over the GitHub-matched repos;
/// `Discovering` and `Finishing` bracket it (their counts are not known).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyProgress {
    Discovering,
    Statting { done: usize, total: usize },
    Finishing,
}

impl ApplyProgress {
    /// Fill fraction for the gauge, in 0.0..=1.0.
    pub fn ratio(&self) -> f64 {
        match self {
            ApplyProgress::Discovering => 0.0,
            ApplyProgress::Statting { done, total } => {
                if *total == 0 {
                    0.0
                } else {
                    (*done as f64 / *total as f64).clamp(0.0, 1.0)
                }
            }
            ApplyProgress::Finishing => 1.0,
        }
    }

    /// Gauge label text.
    pub fn label(&self) -> String {
        match self {
            ApplyProgress::Discovering => "discovering…".to_string(),
            ApplyProgress::Statting { done, total } => format!("{done}/{total} repos"),
            ApplyProgress::Finishing => "finishing…".to_string(),
        }
    }
}
