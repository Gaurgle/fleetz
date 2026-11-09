use crate::app::App;
use crate::engine::{
    FETCH_CONCURRENCY, FetchAsk, HISTORY_COUNT, SweepTarget, activity_position, disk_usage_of,
    fetch_under_claim_capped, probe_concurrency, size_held_rows,
};
use crate::event::AppEvent;
use crate::model::{RepoLocation, RepoStatus};
use crate::poll::repo_moved;
use crate::{app, contributions, git, model, ui};
use std::collections::HashSet;

/// A sweep that never reported back must not hold the watch lock forever.
pub(crate) const SWEEP_STUCK_AFTER: std::time::Duration = std::time::Duration::from_secs(180);

/// Land one repo's sweep result (`AppEvent::RepoUpdated`). The full sweep and
/// the single-repo sweep both arrive here, so they share one merge: only the
/// sweep-owned fields are taken (`RepoStatus::apply_local_sweep`), so a poll
/// that landed while the sweep ran keeps its CI and PR data.
///
/// A result whose path no longer has a row is dropped: the repo left the list
/// while its sweep was in flight, and a late result must not bring it back.
///
/// The fetch outcome lands separately from the sweep's fields, on the row's
/// `fetch` facts: only a sweep that fetched changes them, and a failure keeps
/// every local field and the remote refs it had (rules 2 and 4 of the fetch
/// contract). Any finished fetch settles a refresh waiting on it.
#[cfg(test)]
pub(crate) fn apply_repo_updated(
    app: &mut App,
    path: &str,
    status: RepoStatus,
    fetch: git::FetchOutcome,
) {
    land_repo_update(app, path, status, fetch, None);
}

/// `apply_repo_updated` for the sweep that held fetch claim `claim`: its
/// landing ends that claim only (`App::fetch_landed_under`), so a sweep
/// whose claim expired cannot end a newer one on the same path.
pub(crate) fn land_repo_update(
    app: &mut App,
    path: &str,
    status: RepoStatus,
    fetch: git::FetchOutcome,
    claim: Option<u64>,
) {
    app.settle_refresh_fetch(path, &fetch);
    app.fetch_landed_under(path, &fetch, claim);
    app.fetch_backoff.record(path, &fetch, chrono::Local::now());
    let mut name = None;
    if let Some(repo) = app
        .repos
        .iter_mut()
        .find(|r| r.path.as_deref() == Some(path))
    {
        if repo_moved(repo, &status) {
            app.push_detected = true;
        }
        repo.apply_local_sweep(status);
        repo.fetch.record(&fetch, chrono::Local::now());
        // The size this success brought the clone to: the pre-check's next
        // baseline (FZ-35b). Recorded clears a growth hold.
        if fetch == git::FetchOutcome::Succeeded {
            repo.fetch.disk_baseline = disk_usage_of(repo, &app.dataset);
        }
        name = Some(repo.name.clone());
    }
    let name = name.unwrap_or_else(|| path.to_string());
    if let Some(report) = app.manual_fetches.land(path, &name, &fetch) {
        app.set_status(ui::fetch_report_label(&report), true);
    }
    app.needs_redraw = true;
}

/// Snapshot the repos that have a local path, as sweep targets.
fn sweep_targets(app: &App, only: Option<usize>) -> Vec<SweepTarget> {
    let to_target = |r: &RepoStatus| {
        let display = r.path.clone()?;
        let expanded = shellexpand::tilde(&display).to_string();
        Some(SweepTarget {
            display_path: display,
            fs_path: std::path::PathBuf::from(expanded),
            old: Box::new(r.clone()),
        })
    };
    match only {
        Some(i) => app.repos.get(i).and_then(to_target).into_iter().collect(),
        None => app.repos.iter().filter_map(to_target).collect(),
    }
}

/// Kick off a background sweep of every repo with a local path. Each repo is
/// fetched (optionally), rebuilt in full, and streamed back as RepoUpdated;
/// SweepDone closes the books. No-op if a sweep is already running.
/// `manual` marks a sweep the user asked for by keypress. Only those announce
/// themselves in the header; a watch-timer sweep runs silently and simply
/// updates the rows when it lands.
/// What a sweep needs in order to fill the local contribution histogram.
/// `None` means the strip is not showing local data, so the year-long `git
/// log` per repo is skipped entirely: it is the most expensive probe in the
/// sweep and nothing on screen would read it.
#[derive(Clone)]
pub(crate) struct ContribProbe {
    /// Grid start, resolved once so every repo in one sweep shares a calendar.
    pub(crate) start: chrono::NaiveDate,
    /// Author regex from config, or None to resolve each repo's `user.email`.
    pub(crate) author: Option<String>,
}

/// Whether this sweep should pay for the per-repo commit histogram.
fn contrib_probe(app: &App) -> Option<ContribProbe> {
    (app.contributions == app::ContributionsMode::Local).then(|| ContribProbe {
        start: contributions::grid_start(chrono::Local::now().date_naive()),
        author: app.contrib_author.clone(),
    })
}

pub(crate) fn start_sweep(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    do_fetch: bool,
    author: Option<String>,
    manual: bool,
) {
    if app.sweep_started.is_some() {
        return;
    }
    let targets = sweep_targets(app, None);
    if targets.is_empty() {
        return;
    }
    app.sweep_started = Some(std::time::Instant::now());
    app.sweep_manual = manual;
    // `F`: report its fetches when the last one lands. A watch sweep fetches
    // silently, and `r` does not fetch here at all. A row the fetch cap
    // holds is only rebuilt and lands `NotAttempted`, so it is not awaited
    // (FZ-35b); a row whose fetch is already running is, since that fetch
    // lands with a real outcome.
    if do_fetch && manual {
        let held = size_held_paths(app, &targets);
        app.manual_fetches.expect(
            targets
                .iter()
                .map(|t| t.display_path.clone())
                .filter(|path| !held.contains(path)),
        );
    }
    let plan = plan_sweep_fetch(app, &targets, do_fetch, FetchAsk::for_full_sweep(manual));
    let probe = contrib_probe(app);
    spawn_sweep(
        tx.clone(),
        targets,
        do_fetch,
        &plan,
        author,
        true,
        probe,
        &app.fetches_in_flight,
    );
}

/// What a sweep hands `spawn_sweep` about fetching: the targets it rebuilds
/// without a fetch, and the byte cap the rest run under (FZ-35b).
struct SweepFetchPlan {
    no_fetch: HashSet<String>,
    max_bytes: Option<u64>,
}

/// The fetch half of a sweep asked for by `ask`: claims the targets it
/// fetches (`fetch_skips_for`) and picks the cap (`FetchAsk::cap`).
fn plan_sweep_fetch(
    app: &mut App,
    targets: &[SweepTarget],
    do_fetch: bool,
    ask: FetchAsk,
) -> SweepFetchPlan {
    SweepFetchPlan {
        no_fetch: fetch_skips_for(app, targets, do_fetch, ask),
        max_bytes: ask.cap(app.fetch_max_bytes),
    }
}

/// The targets an automatic sweep rebuilds without fetching at `now`: those
/// a recent fetch timeout holds (`App::fetch_held`, FZ-34).
fn held_fetch_paths(
    app: &App,
    targets: &[SweepTarget],
    now: chrono::DateTime<chrono::Local>,
) -> std::collections::HashSet<String> {
    targets
        .iter()
        .filter(|t| app.fetch_held(&t.display_path, now))
        .map(|t| t.display_path.clone())
        .collect()
}

/// The targets whose row the fetch cap holds until `f` (FZ-35b).
fn size_held_paths(app: &App, targets: &[SweepTarget]) -> HashSet<String> {
    let held = size_held_rows(&app.repos);
    targets
        .iter()
        .filter(|t| held.contains(&t.display_path))
        .map(|t| t.display_path.clone())
        .collect()
}

/// `fetch_skips_for` as the FZ-34 tests ask it: `automatic`, or `f`.
#[cfg(test)]
fn fetch_skips(
    app: &mut App,
    targets: &[SweepTarget],
    do_fetch: bool,
    automatic: bool,
) -> std::collections::HashSet<String> {
    let ask = if automatic {
        FetchAsk::Automatic
    } else {
        FetchAsk::One
    };
    fetch_skips_for(app, targets, do_fetch, ask)
}

/// The targets a sweep rebuilds without fetching, claiming the rest for a
/// fetch (FZ-34). Every fetching sweep skips a path whose fetch is still
/// running (R5); an automatic one (the watch timer, a moved remote, the
/// recovery refetch) also skips a path held after a timeout (R3), where `f`
/// and `F` fetch it as asked. Every one but `f` skips a repo the fetch cap
/// holds (FZ-35b). A sweep that does not fetch claims nothing.
fn fetch_skips_for(
    app: &mut App,
    targets: &[SweepTarget],
    do_fetch: bool,
    ask: FetchAsk,
) -> std::collections::HashSet<String> {
    if !do_fetch {
        return std::collections::HashSet::new();
    }
    let mut skips = if ask == FetchAsk::Automatic {
        held_fetch_paths(app, targets, chrono::Local::now())
    } else {
        std::collections::HashSet::new()
    };
    if ask != FetchAsk::One {
        skips.extend(size_held_paths(app, targets));
    }
    let started = std::time::Instant::now();
    for target in targets {
        if !skips.contains(&target.display_path) && !app.claim_fetch(&target.display_path, started)
        {
            skips.insert(target.display_path.clone());
        }
    }
    skips
}

/// Sweep a single repo (the `f` fetch and the refresh after a branch switch
/// or delete).
/// Does not take the sweep lock: it is one repo, and it must work while a
/// full sweep is running.
pub(crate) fn start_single_sweep(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    index: usize,
    do_fetch: bool,
    automatic: bool,
    author: Option<String>,
) {
    let targets = sweep_targets(app, Some(index));
    if targets.is_empty() {
        return;
    }
    let plan = plan_sweep_fetch(
        app,
        &targets,
        do_fetch,
        FetchAsk::for_single_sweep(automatic),
    );
    spawn_sweep(
        tx.clone(),
        targets,
        do_fetch,
        &plan,
        author,
        false,
        contrib_probe(app),
        &app.fetches_in_flight,
    );
}

/// The shared sweep body: fetch (capped) then rebuild (capped), streaming
/// each repo back as it lands so the list updates progressively. A target
/// whose display path is in the plan's `no_fetch` is rebuilt without a
/// fetch; every other one fetches under the claim the plan reserved for it,
/// cut as `TooBig` past the plan's `max_bytes` (`plan_sweep_fetch`, FZ-35b).
#[allow(clippy::too_many_arguments)] // one call spine; a params struct would only rename the problem
fn spawn_sweep(
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    targets: Vec<SweepTarget>,
    do_fetch: bool,
    plan: &SweepFetchPlan,
    author: Option<String>,
    report_done: bool,
    probe: Option<ContribProbe>,
    claims: &app::FetchClaims,
) {
    let max_bytes = plan.max_bytes;
    let targets: Vec<(SweepTarget, Option<u64>)> = targets
        .into_iter()
        .map(|t| {
            let fetch = do_fetch && !plan.no_fetch.contains(&t.display_path);
            let claim = if fetch {
                claims.token(&t.display_path)
            } else {
                None
            };
            (t, claim)
        })
        .collect();
    let claims = claims.clone();
    tokio::spawn(async move {
        let probe_sem = std::sync::Arc::new(tokio::sync::Semaphore::new(probe_concurrency()));
        let fetch_sem = std::sync::Arc::new(tokio::sync::Semaphore::new(FETCH_CONCURRENCY));
        let mut set = tokio::task::JoinSet::new();
        for (target, claim) in targets {
            let probe_sem = probe_sem.clone();
            let fetch_sem = fetch_sem.clone();
            let tx = tx.clone();
            let author = author.clone();
            let probe = probe.clone();
            let claims = claims.clone();
            set.spawn(async move {
                let fetch = match claim {
                    Some(token) => {
                        let _permit = fetch_sem.acquire().await;
                        fetch_under_claim_capped(
                            &claims,
                            &target.display_path,
                            &target.fs_path,
                            token,
                            Some(&tx),
                            max_bytes,
                        )
                        .await
                    }
                    None => git::FetchOutcome::NotAttempted,
                };
                let _permit = probe_sem.acquire().await;
                let status = rebuild_repo_status(&target, author.as_deref(), probe.as_ref()).await;
                let _ = tx.send(AppEvent::RepoUpdated {
                    path: target.display_path.clone(),
                    status: Box::new(status),
                    fetch,
                    claim,
                });
            });
        }
        while set.join_next().await.is_some() {}
        if report_done {
            let _ = tx.send(AppEvent::SweepDone {
                at: chrono::Local::now(),
            });
        }
    });
}

/// Whether a sweep may reuse the gated probes: only when it has a key and the
/// key equals the snapshot's. A `None` key (a failed query, an unborn HEAD)
/// never counts as unchanged, not even against another `None`.
fn key_unchanged(new: &Option<git::WorkKey>, old: &Option<git::WorkKey>) -> bool {
    new.is_some() && new == old
}

/// Full per-repo rebuild. Fields the sweep does not probe are copied from the
/// snapshot only to complete the struct; `RepoStatus::apply_local_sweep`
/// discards them when the result lands.
/// Two-tier: the cheap probes always run, the expensive ones only when the
/// repo's WorkKey says something actually moved.
pub(crate) async fn rebuild_repo_status(
    target: &SweepTarget,
    author: Option<&str>,
    probe: Option<&ContribProbe>,
) -> RepoStatus {
    let path = &target.fs_path;
    let old = &target.old;

    // Cheap tier: always current. Untracked files never touch the WorkKey,
    // so dirty_count must not be skipped. Both ref queries feed the key, and
    // a failed one stays a failure there rather than an empty list.
    let local = git::try_branch_statuses(path).await;
    let dirty = git::dirty_count(path).await;
    let other = git::try_ref_listing(path).await;
    let key = git::work_key_from(path, local.as_ref().ok(), other.as_ref().ok()).await;
    let unchanged = key_unchanged(&key, &old.work_key);
    // Not gated: no ref moves when a worktree is added on an existing branch,
    // removed, or switched, so the key cannot see it. One stat, and a process
    // only for a clone that has linked worktrees.
    let worktrees = git::linked_worktrees(path).await;

    let branches = local.map(|l| l.statuses).unwrap_or_default();
    let remote = other.map(|o| o.remotes).unwrap_or_default();
    let branch_names: Vec<String> = branches.iter().map(|b| b.name.clone()).collect();
    let (total_behind, total_ahead) = branches
        .iter()
        .fold((0u32, 0u32), |(b, a), s| (b + s.behind, a + s.ahead));

    let (commits, reflog, graph, graph_expanded, stash_entries) = if unchanged {
        (
            old.commits.clone(),
            old.reflog.clone(),
            old.graph.clone(),
            old.graph_expanded.clone(),
            old.stash_entries.clone(),
        )
    } else {
        (
            git::commits(
                path,
                &format!("-{HISTORY_COUNT}"),
                true,
                author,
                &branch_names,
            )
            .await,
            git::reflog(path, HISTORY_COUNT).await,
            git::graph(path, &branch_names, HISTORY_COUNT).await,
            git::graph_expanded(path, &branch_names, HISTORY_COUNT).await,
            git::stash_list(path).await,
        )
    };

    // The local calendar is a year of log per repo, so it is gated twice:
    // only when the strip is actually showing it, and only when the
    // histogram's own key (grid start, configured author, ref signature,
    // HEAD) differs from the one the kept histogram was computed under. An
    // idle fleet costs nothing. Without the probe both are kept as they are,
    // so a later Local sweep still compares against what `daily` really saw.
    let wanted_daily_key = match (probe, &key) {
        (Some(probe), Some(key)) => Some(model::HistogramKey {
            start: probe.start,
            author: probe.author.clone(),
            ref_signature: key.ref_signature,
            head_sha: key.head_sha.clone(),
        }),
        _ => None,
    };
    let (daily, daily_key) = match probe {
        None => (old.daily.clone(), old.daily_key.clone()),
        Some(_)
            if old.daily.is_some()
                && wanted_daily_key.is_some()
                && wanted_daily_key == old.daily_key =>
        {
            (old.daily.clone(), old.daily_key.clone())
        }
        Some(probe) => {
            let author = match &probe.author {
                Some(a) => Some(a.clone()),
                None => git::user_email(path).await,
            };
            let daily = match author {
                Some(author) => git::daily_commits(path, probe.start, &author)
                    .await
                    .map(Box::new),
                // No identity configured: better to show nothing than to
                // count everyone's commits as if they were yours.
                None => None,
            };
            let daily_key = daily.as_ref().and(wanted_daily_key);
            (daily, daily_key)
        }
    };

    // Computed before the struct literal: the literal moves `branches` in, and
    // this borrows it.
    let remote_only_branches = git::unfetched_branches(&remote, &branches);

    RepoStatus {
        daily,
        daily_key,
        branches,
        dirty,
        behind: total_behind,
        ahead: total_ahead,
        remote_only_branches,
        remote_branches: remote,
        stash_count: stash_entries.len() as u32,
        stash_entries,
        commits,
        reflog,
        graph,
        graph_expanded,
        worktrees,
        work_key: key,
        ..(*old.clone())
    }
}

/// Re-order the rows by activity after a poll or a sweep lands (FZ-86), so
/// a repo pushed since launch rises within its section as a fresh launch
/// would put it. Before this the order was set once, by `sort_by_activity`
/// at load, and a refresh or a watch poll only updated rows in place.
///
/// The key restates how `assemble_view` lays the rows out, so a re-sort
/// moves nothing a launch on the same dataset would not: passes 1 and 2 by
/// `activity_position`, then pass 3's rows in discovery order (a stable
/// sort on an equal key keeps it), then the not-cloned rows by activity.
/// `App::visible_repos` groups that into sections, as at launch.
pub(crate) fn order_rows_by_activity(app: &mut App) {
    let keys: Vec<(u8, usize)> = app
        .repos
        .iter()
        .map(|repo| match repo.location {
            RepoLocation::Primary | RepoLocation::Elsewhere => (
                0,
                activity_position(repo.owner.as_deref(), &repo.name, &app.dataset),
            ),
            RepoLocation::LocalOnly => (1, 0),
            RepoLocation::NotCloned => (
                2,
                activity_position(repo.owner.as_deref(), &repo.name, &app.dataset),
            ),
        })
        .collect();
    app.reorder_repos(keys);
}

/// `AppEvent::SweepDone`: close the sweep's books, then apply the order. A
/// sweep changes no row's activity, but an order a search held back
/// (`App::reorder_repos`) lands here at the latest.
pub(crate) fn apply_sweep_done(app: &mut App, at: chrono::DateTime<chrono::Local>) {
    app.sweep_landed(at);
    order_rows_by_activity(app);
}

#[cfg(test)]
mod tests;
