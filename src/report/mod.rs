//! Print mode: the one-shot text report bash repoz prints, rebuilt on fleetz's
//! own discovery, git and GitHub code (`docs/repoz-print-design.md`).
//!
//! The time modes (default slot, `-a`, `-t`, `--since`, `--before`) are here
//! (FZ-47). Discovery is the TUI's assembly (`assemble_view_held`), so the
//! three passes, matching by origin and owner, `CheckoutRank` and the
//! `stale_remotes` fetch are fleetz's (D3, D4); the GitHub step is one poll
//! through fleetz's query that writes the keyed cache (D5). What is drawn,
//! and how each row's counts are read, follows bash repoz (D2): the branches
//! with an upstream, as its `check_repo` and pass 3 count them. Pass 3 is
//! bash's too (`with_bash_local_changes`): every unclaimed local repo with
//! news, whatever the window, under roots that include the working
//! directory's git toplevel.
//!
//! `-d` and the single-repo target (`/owner/repo`, `/repo`) are here too
//! (FZ-48). Neither asks GitHub anything, as in bash: `-d` is that pass 3
//! with nothing claimed (`print_local`), and a target is one clone found by
//! fleetz's own matching for that one repo (`print_target`).

pub mod render;

#[cfg(test)]
pub mod parity;

use std::collections::HashSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cli::{Args, Mode};
use crate::github::{self, GithubRepo};
use crate::model::{RepoLocation, RepoStatus};
use render::{Counts, Glyph, Layout, palette};

/// Write the report for the repoz-style arguments `args` (`-a`, `--since
/// 2026-03-01`, ...) to `out`, on a runtime of its own. The entry the parity
/// harness runs; the binary calls [`print`] from its own runtime. Returns
/// the exit status.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the binary enters through `print`")
)]
pub fn run(args: &[String], out: &mut dyn Write) -> io::Result<i32> {
    use clap::Parser;
    let argv = ["fleetz", "--print"]
        .into_iter()
        .map(str::to_string)
        .chain(args.iter().cloned());
    let args = Args::try_parse_from(argv).map_err(|e| invalid(e.to_string()))?;
    tokio::runtime::Runtime::new()?.block_on(print(&args, out))
}

/// Print mode for parsed `args`: the report goes to `out` and nowhere else,
/// and no terminal state is touched. Returns the exit status: 1 when GitHub
/// could not be reached, as bash repoz exits.
pub async fn print(args: &Args, out: &mut dyn Write) -> io::Result<i32> {
    let mode = args.mode();
    let config = crate::config::Config::load();
    let repo_dir = std::env::var("REPO_CHECK_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| config.resolved_repo_dir());
    let mut scan_roots = vec![repo_dir];
    scan_roots.extend(config.resolved_extra_dirs());
    add_cwd_toplevel(&mut scan_roots).await;
    let home = dirs::home_dir().unwrap_or_default();
    let plain = config.icons.as_deref().map(crate::app::Icons::from_config)
        == Some(crate::app::Icons::Plain);
    let layout = Layout {
        width: render::term_width(),
        glyphs: !plain && !args.no_glyphs,
    };

    // bash checks these two before it asks GitHub anything, `-d` first, and
    // neither asks at all.
    if mode == Mode::Dirty {
        return print_local(layout, &scan_roots, &home, out).await;
    }
    if let Some(target) = &args.target {
        return print_target(target, layout, &config, &scan_roots, &home, out).await;
    }

    let (cutoff, header) = window(args, &mode, &config).map_err(invalid)?;
    let Ok(dataset) = poll(config.repo_limit).await else {
        out.write_all(b"  Failed to reach GitHub. Run: gh auth login\n")?;
        return Ok(1);
    };
    let active = active_repos(&dataset, cutoff, args).map_err(invalid)?;
    let (rows, _notes) = crate::engine::assemble_view_held(
        &dataset,
        cutoff,
        header.clone(),
        args,
        &config,
        &home,
        &scan_roots,
        true,
        None,
        &crate::app::FetchClaims::default(),
        &HashSet::new(),
    )
    .await
    .map_err(invalid)?;
    let rows = with_bash_local_changes(rows, &dataset, &active, &scan_roots, &home).await;

    let mut report = Report {
        layout,
        home: &home,
        active: &active,
        author: args.author.as_deref(),
        stat: args.stat,
        releases: args.releases,
        prs: args.prs,
        counts: Counts::default(),
    };
    report.write(&header, rows, out).await?;
    Ok(0)
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// bash's `add_scan_dir "$_toplevel"`: the working directory's git toplevel
/// joins the scan roots unless it is one already, compared canonically as
/// bash `realpath`s each root. A toplevel under a root is still added, as
/// bash adds it; discovery dedupes the repos two roots both reach.
async fn add_cwd_toplevel(roots: &mut Vec<PathBuf>) {
    let Ok(cwd) = std::env::current_dir() else {
        return;
    };
    let Some(out) = crate::git::run_git(&cwd, &["rev-parse", "--show-toplevel"]).await else {
        return;
    };
    let toplevel = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || toplevel.is_empty() {
        return;
    }
    let toplevel = PathBuf::from(toplevel);
    let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let key = canonical(&toplevel);
    if !roots.iter().any(|root| canonical(root) == key) {
        roots.push(toplevel);
    }
}

/// Assembly's rows with its pass 3 replaced by bash's: every repo under the
/// scan roots that passes 1 and 2 did not show, listed when it is dirty,
/// ahead, mid-operation or stashed, whatever the window (`local_news`).
/// Assembly's own pass 3 (`local_only_status`) drops a dirty repo older than
/// the cutoff and a clean one whose only news is a stash or an operation.
///
/// The not-cloned rows are rebuilt to match: a repo is not cloned when
/// neither passes 1 and 2 nor one of these rows carries it, the carriers
/// chosen by assembly's own rule (`mark_local_carriers`).
async fn with_bash_local_changes(
    rows: Vec<RepoStatus>,
    dataset: &[GithubRepo],
    active: &[GithubRepo],
    scan_roots: &[PathBuf],
    home: &Path,
) -> Vec<RepoStatus> {
    let (mut shown, replaced): (Vec<_>, Vec<_>) = rows.into_iter().partition(|row| {
        matches!(
            row.location,
            RepoLocation::Primary | RepoLocation::Elsewhere
        )
    });
    // Assembly gave each active repo to exactly one of passes 1 and 2, its
    // pass 3 or a not-cloned row; the first are the ones neither replaced
    // row carries.
    let mut carried: HashSet<crate::engine::RemoteKey> = active
        .iter()
        .filter(|entry| {
            !replaced
                .iter()
                .any(|row| crate::engine::carries(row, entry))
        })
        .map(|entry| crate::engine::remote_key(entry.owner.as_deref(), &entry.name))
        .collect();

    let claimed: Vec<PathBuf> = shown.iter().map(|row| on_disk(home, row)).collect();
    let claimed: HashSet<&Path> = claimed.iter().map(PathBuf::as_path).collect();
    let probed = local_with_news(scan_roots, &claimed).await;
    let mut local: Vec<(&Path, RepoStatus)> = probed
        .iter()
        .map(|(path, row)| (path.as_path(), row.clone()))
        .collect();
    crate::engine::mark_local_carriers(&mut local, dataset, scan_roots, &mut carried);
    shown.extend(local.into_iter().map(|(_, row)| row));

    for entry in active {
        if carried.contains(&crate::engine::remote_key(
            entry.owner.as_deref(),
            &entry.name,
        )) {
            continue;
        }
        let mut row = bare_row(entry.name.clone(), None, RepoLocation::NotCloned);
        row.owner = entry.owner.clone();
        row.apply_github(entry);
        shown.push(row);
    }
    shown
}

/// bash's `scan_dirs_git_dirs` walk as its pass 3 and `-d` read it: every
/// repo under the scan roots not in `claimed` (compared canonically,
/// `unclaimed_local`) that has news (`local_news`), in walk order.
async fn local_with_news(
    scan_roots: &[PathBuf],
    claimed: &HashSet<&Path>,
) -> Vec<(PathBuf, RepoStatus)> {
    let candidates = crate::engine::unclaimed_local(
        crate::discovery::find_all_local(scan_roots),
        claimed,
        &HashSet::new(),
    );
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(
        crate::engine::probe_concurrency(),
    ));
    let mut set = tokio::task::JoinSet::new();
    for (i, (path, name)) in candidates.into_iter().enumerate() {
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire().await;
            let row = local_candidate(&path, name).await;
            (i, path, row)
        });
    }
    let mut probed = Vec::new();
    while let Some(joined) = set.join_next().await {
        if let Ok(done) = joined {
            probed.push(done);
        }
    }
    probed.sort_by_key(|(i, _, _)| *i);
    probed
        .into_iter()
        .filter(|(_, path, row)| local_news(path, row).is_some())
        .map(|(_, path, row)| (path, row))
        .collect()
}

/// bash's `-d`: the scan roots' repos with news, as its pass 3 prints them
/// but with nothing claimed, then `All clean.` or `N dirty`. No GitHub
/// request, no fetch, no summary or version line.
async fn print_local(
    layout: Layout,
    scan_roots: &[PathBuf],
    home: &Path,
    out: &mut dyn Write,
) -> io::Result<i32> {
    let mut rows: Vec<RepoStatus> = local_with_news(scan_roots, &HashSet::new())
        .await
        .into_iter()
        .map(|(_, row)| row)
        .collect();
    sort_as_git_paths(home, &mut rows);
    let mut report = Report {
        layout,
        home,
        active: &[],
        author: None,
        stat: false,
        releases: false,
        prs: false,
        counts: Counts::default(),
    };
    let mut text = layout.header("local changes");
    text.push('\n');
    for row in &rows {
        text.push_str(&report.local_row(row));
    }
    text.push('\n');
    text.push_str(&layout.divider());
    text.push_str(&match report.counts.uncommitted {
        0 => format!("  {}All clean.{}\n", palette::GREEN, palette::RESET),
        n => format!("  {}{n} dirty{}\n", palette::YELLOW, palette::RESET),
    });
    text.push('\n');
    out.write_all(text.as_bytes())?;
    out.flush()?;
    Ok(0)
}

/// bash's target mode: `/owner/repo`, or `/repo` for one of the login's,
/// as one row with its path, or `not cloned`. The payload is never read, so
/// a repo GitHub does not list prints the same way. The clone is found by
/// fleetz's passes 1 and 2 for that one repo (D3, matching), fetched as an
/// unknown must be (D4), and counted as bash counts it: every branch with
/// an upstream and the clone's stashes, a linked worktree included.
async fn print_target(
    target: &str,
    layout: Layout,
    config: &crate::config::Config,
    scan_roots: &[PathBuf],
    home: &Path,
    out: &mut dyn Write,
) -> io::Result<i32> {
    let spec = target.strip_prefix('/').unwrap_or(target);
    // bash's `${spec%%/*}` and `${spec##*/}`.
    let (owner, name) = match (spec.split_once('/'), spec.rsplit_once('/')) {
        (Some((owner, _)), Some((_, name))) => (owner.to_string(), name),
        _ => (login().await.unwrap_or_else(|| "?".to_string()), spec),
    };
    let full = format!("{owner}/{name}");

    let mut text = layout.header(&full);
    text.push('\n');
    let line = match find_target(&owner, name, scan_roots, home) {
        Some(path) => target_row(layout, name, &path, config.fetch_cap_bytes()).await,
        None => layout.row(Glyph::None, &full, None, "not cloned", palette::RED),
    };
    text.push_str(&line);
    text.push('\n');
    out.write_all(text.as_bytes())?;
    out.flush()?;
    Ok(0)
}

/// The clone of `owner/name` fleetz's passes 1 and 2 would pick: under the
/// scan roots first, then elsewhere under `home`.
fn find_target(owner: &str, name: &str, scan_roots: &[PathBuf], home: &Path) -> Option<PathBuf> {
    use crate::discovery;
    let repo = [(Some(owner), name)];
    let wanted = discovery::Wanted::with_full_dataset(repo, repo);
    let (primary, _) = discovery::find_in_primary_with_ambiguities(scan_roots, &wanted);
    let found = match primary.into_iter().next() {
        Some(found) => Some(found),
        None => {
            discovery::find_elsewhere_with_ambiguities(home, scan_roots, &wanted, &HashSet::new())
                .0
                .into_iter()
                .next()
        }
    };
    found.map(|found| found.path)
}

/// The target's row after one fetch: its counts, a failed fetch in front,
/// or `synced`. Named as the target spelled it, as bash names it.
async fn target_row(layout: Layout, name: &str, path: &Path, max_bytes: Option<u64>) -> String {
    let fetch = crate::git::fetch_reporting(path, max_bytes, |_| {}).await;
    let fetch_failed = matches!(fetch, crate::git::FetchOutcome::Failed(_));
    let dirty = crate::git::dirty_count(path).await;
    let tracked: Vec<_> = crate::git::branch_statuses(path)
        .await
        .into_iter()
        .filter(|b| b.upstream.is_some())
        .collect();
    let behind: u32 = tracked.iter().map(|b| b.behind).sum();
    let ahead: u32 = tracked.iter().map(|b| b.ahead).sum();
    let op = operation(path);
    let stashes = stash_count(path);

    let symbols = render::status_symbols(behind, ahead, dirty, stashes);
    let noted = match fetch_failed {
        true => render::with_op("fetch failed", &symbols),
        false => symbols,
    };
    let status = render::with_op(op, &noted);
    let shown = crate::engine::tilde_display(path);
    match status.is_empty() {
        true => layout.row(Glyph::None, name, Some(&shown), "synced", palette::GREEN),
        false => layout.row(
            Glyph::None,
            name,
            Some(&shown),
            &status,
            row_color(&status, op, dirty, stashes),
        ),
    }
}

/// Names the login a one-segment target resolves against, in a test build
/// only (the parity harness sets it for bash repoz and print mode).
#[cfg(test)]
const GH_LOGIN_FIXTURE: &str = "REPOZ_GH_LOGIN";

/// How long the login lookup may take. It is a request to GitHub, unlike
/// the token lookup `LAUNCH_LOOKUP_BOUND` covers, so it gets more room; a
/// lookup that runs out reads as no login, as a failed one does.
const LOGIN_LOOKUP_BOUND: std::time::Duration = std::time::Duration::from_secs(10);

/// The authenticated user's login, as bash's `gh_login` asks for it (`gh
/// api user`), on the host every fleetz request goes to. None when `gh`
/// fails, answers nothing or runs past [`LOGIN_LOOKUP_BOUND`].
async fn login() -> Option<String> {
    // Test seam: with `REPOZ_GH_LOGIN` set, a test build takes it as the
    // answer. Compiled out of the binary; never a production flag.
    #[cfg(test)]
    if let Some(login) = std::env::var_os(GH_LOGIN_FIXTURE) {
        let login = login.to_string_lossy().trim().to_string();
        return (!login.is_empty()).then_some(login);
    }
    let lookup = tokio::process::Command::new("gh")
        .args(["api", "user", "--hostname", github::HOST, "--jq", ".login"])
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(LOGIN_LOOKUP_BOUND, lookup)
        .await
        .ok()?
        .ok()?;
    let login = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !login.is_empty()).then_some(login)
}

/// Pass 3 rows in bash's order: `LC_ALL=C sort` of their `.git` paths.
fn sort_as_git_paths(home: &Path, rows: &mut [RepoStatus]) {
    rows.sort_by_cached_key(|row| {
        let mut key = on_disk(home, row).into_os_string().into_encoded_bytes();
        key.extend_from_slice(b"/.git");
        key
    });
}

/// A pass 3 candidate with what bash's pass 3 reads: the uncommitted count,
/// the branches and their upstreams, and the origin carriers are chosen by.
async fn local_candidate(path: &Path, name: String) -> RepoStatus {
    let mut row = bare_row(
        name,
        Some(crate::engine::tilde_display(path)),
        RepoLocation::LocalOnly,
    );
    row.dirty = crate::git::dirty_count(path).await;
    row.branches = crate::git::branch_statuses(path).await;
    row.remote_url = crate::git::remote_url(path).await;
    row
}

/// A row holding only its name, path and location; print mode reads no
/// other field of a pass 3 or not-cloned row before `apply_github`.
fn bare_row(name: String, path: Option<String>, location: RepoLocation) -> RepoStatus {
    RepoStatus {
        last_local: None,
        name,
        path,
        location,
        branches: Vec::new(),
        commits: Vec::new(),
        behind: 0,
        ahead: 0,
        dirty: 0,
        pr_count: 0,
        prs: Vec::new(),
        ci_status: crate::model::CiStatus::Unknown,
        ci_workflow: None,
        ci_duration: None,
        groups: Vec::new(),
        stash_count: 0,
        stash_entries: Vec::new(),
        worktrees: Vec::new(),
        reflog: Vec::new(),
        graph: Vec::new(),
        graph_expanded: Vec::new(),
        repo_kind: crate::model::RepoKind::Unknown,
        owner: None,
        remote_url: None,
        remote_branches: Vec::new(),
        remote_only_branches: Vec::new(),
        daily: None,
        daily_key: None,
        work_key: None,
        fetch: crate::model::FetchFacts::default(),
        authors: Default::default(),
    }
}

/// What bash's pass 3 says about a local repo, or None when it says
/// nothing: behind is not counted, and a linked worktree counts its own
/// branch and no stashes (`own_branches`, `own_stash_count`).
struct LocalNews {
    ahead: u32,
    op: &'static str,
    stashes: u32,
}

fn local_news(path: &Path, row: &RepoStatus) -> Option<LocalNews> {
    let worktree = path.join(".git").is_file();
    let ahead: u32 = row
        .branches
        .iter()
        .filter(|b| b.upstream.is_some() && (!worktree || b.is_head))
        .map(|b| b.ahead)
        .sum();
    let op = operation(path);
    let stashes = if worktree { 0 } else { stash_count(path) };
    let quiet = row.dirty == 0 && ahead == 0 && op.is_empty() && stashes == 0;
    (!quiet).then_some(LocalNews { ahead, op, stashes })
}

/// The row's checkout on disk, from the path it displays.
fn on_disk(home: &Path, row: &RepoStatus) -> PathBuf {
    let shown = row.path.as_deref().unwrap_or_default();
    match shown.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(shown),
    }
}

/// The cutoff and the header's mode text. The cutoff is the TUI's
/// (`resolve_cutoff`, which takes the default slot from `time.rs`, D9); the
/// text is bash's, which names a relative `--since` or `--before` by the date
/// it resolved to.
fn window(
    args: &Args,
    mode: &Mode,
    config: &crate::config::Config,
) -> Result<(Option<chrono::DateTime<chrono::Utc>>, String), String> {
    let (cutoff, mut header) = crate::engine::resolve_cutoff(None, mode, config)?;
    if let Mode::Since(since) = mode {
        header = format!("since {}", crate::time::parse_relative_date(since)?);
    }
    if let Some(before) = &args.before {
        let date = crate::time::parse_relative_date(before)?;
        header = format!("{header} \u{2192} before {date}");
    }
    if let Some(author) = &args.author {
        header = format!("{header} (author: {author})");
    }
    Ok((cutoff, header))
}

/// The repos in the window, newest push first, as assembly filters them.
fn active_repos(
    dataset: &[GithubRepo],
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
    args: &Args,
) -> Result<Vec<GithubRepo>, String> {
    let mut active = dataset.to_vec();
    if let Some(cutoff) = cutoff {
        active = github::filter_since(&active, cutoff);
    }
    if let Some(before) = &args.before {
        let date = crate::time::parse_relative_date(before)?;
        active = github::filter_before(&active, crate::time::date_to_utc_end_of_day(date));
    }
    Ok(active)
}

/// Names the file the GitHub step reads instead of asking GitHub, in a test
/// build only (the parity harness sets it for bash repoz and print mode).
#[cfg(test)]
const GRAPHQL_FIXTURE: &str = "REPOZ_GRAPHQL_FIXTURE";

/// One poll with fleetz's own query, accepted through `accept_response` and
/// written to this credential's keyed cache (D5). A failure is a failure:
/// no cached fallback (O4).
async fn poll(repo_limit: u32) -> Result<Vec<GithubRepo>, String> {
    // Test seam: with `REPOZ_GRAPHQL_FIXTURE` set, a test build reads that
    // file as the response body and takes it through the same parser,
    // `accept_response` and keyed cache write a real answer takes, under a
    // fixed account. Compiled out of the binary; never a production flag.
    #[cfg(test)]
    if let Some(path) = std::env::var_os(GRAPHQL_FIXTURE) {
        let body = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let account = github::token_fingerprint("fixture");
        let own = crate::cache::KeyedCache::new(repo_limit, github::HOST).for_account(&account);
        return github::accept_response(&body, &own, chrono::Local::now().date_naive())
            .map(|fetch| fetch.repos)
            .map_err(|failure| failure.message);
    }
    let cache = crate::cache::KeyedCache::new(repo_limit, github::HOST);
    poll_at(
        repo_limit,
        &cache,
        github::get_gh_token,
        github::GRAPHQL_URL,
    )
    .await
    .map(|fetch| fetch.repos)
    .map_err(|error| error.failure.message)
}

/// [`poll`] with the token source and endpoint passed in, so a test can
/// answer it from a loopback listener.
async fn poll_at<S, F>(
    repo_limit: u32,
    cache: &crate::cache::KeyedCache,
    token_source: S,
    url: &str,
) -> Result<github::GithubFetch, github::FetchError>
where
    S: FnOnce() -> F,
    F: std::future::Future<Output = Result<String, String>>,
{
    github::fetch_keyed(
        repo_limit,
        cache,
        github::CacheMode::Bypass,
        token_source,
        github::client(),
        url,
        github::LAUNCH_LOOKUP_BOUND,
    )
    .await
}

/// One report being drawn: what every section needs, and the tallies the
/// summary prints.
struct Report<'a> {
    layout: Layout,
    home: &'a Path,
    active: &'a [GithubRepo],
    author: Option<&'a str>,
    /// `-s`: the diff stat on every commit line, not only a changed row's.
    stat: bool,
    /// `-r`: a release line under each pass 1 and pass 2 row.
    releases: bool,
    /// `-p` as repoz reads it: each row's open-PR count, and their total.
    prs: bool,
    counts: Counts,
}

impl Report<'_> {
    /// The header, the three sections and the summary, each written whole.
    async fn write(
        &mut self,
        header: &str,
        rows: Vec<RepoStatus>,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        let mut primary = Vec::new();
        let mut also = Vec::new();
        let mut local = Vec::new();
        for row in rows {
            match row.location {
                RepoLocation::Primary => primary.push(row),
                RepoLocation::Elsewhere | RepoLocation::NotCloned => also.push(row),
                RepoLocation::LocalOnly => local.push(row),
            }
        }
        // bash lists pass 2 in push order, found and not cloned interleaved,
        // and pass 3 in `LC_ALL=C sort` order of the `.git` paths.
        also.sort_by_key(|row| self.activity_rank(row));
        sort_as_git_paths(self.home, &mut local);

        let mut text = self.layout.header(header);
        if self.active.is_empty() {
            text.push_str(&format!(
                "\n  {}No GitHub activity.{}\n",
                palette::GREEN,
                palette::RESET
            ));
        }
        text.push('\n');
        for row in &primary {
            text.push_str(&self.checked_row(row).await);
        }
        out.write_all(text.as_bytes())?;

        if !also.is_empty() {
            let mut text = format!("\n{}\n", self.layout.section("also active"));
            for row in &also {
                let line = match row.location {
                    RepoLocation::NotCloned => self.not_cloned_row(row),
                    _ => self.checked_row(row).await,
                };
                text.push_str(&line);
            }
            out.write_all(text.as_bytes())?;
        }

        let mut lines = String::new();
        for row in &local {
            lines.push_str(&self.local_row(row));
        }
        if !lines.is_empty() {
            let text = format!("\n{}\n{lines}", self.layout.section("local changes"));
            out.write_all(text.as_bytes())?;
        }

        let version = format!("fleetz {}", env!("CARGO_PKG_VERSION"));
        out.write_all(self.layout.summary(&self.counts, &version).as_bytes())?;
        out.flush()
    }

    /// The row's place in the window's push order; rows the window does not
    /// hold sort last.
    fn activity_rank(&self, row: &RepoStatus) -> usize {
        self.active
            .iter()
            .position(|r| crate::engine::names_remote(row.owner.as_deref(), &row.name, r))
            .unwrap_or(usize::MAX)
    }

    /// The GitHub repo's own spelling of a matched row's name, as bash
    /// prints it; a folder match reports the folder's spelling otherwise.
    fn github_name<'r>(&'r self, row: &'r RepoStatus) -> &'r str {
        self.active
            .iter()
            .find(|r| crate::engine::names_remote(row.owner.as_deref(), &row.name, r))
            .map_or(row.name.as_str(), |r| r.name.as_str())
    }

    /// The row's open pull requests under `-p`, as the payload counts them;
    /// 0 without `-p`. Added to the summary's total.
    fn open_prs(&mut self, row: &RepoStatus) -> u32 {
        if !self.prs {
            return 0;
        }
        let count = self
            .active
            .iter()
            .find(|r| crate::engine::names_remote(row.owner.as_deref(), &row.name, r))
            .map_or(0, |r| r.pr_count);
        self.counts.open_prs += count;
        count
    }

    /// The row's checkout on disk, from the path it displays.
    fn absolute(&self, row: &RepoStatus) -> PathBuf {
        on_disk(self.home, row)
    }

    /// bash's `check_repo` for a pass 1 or pass 2 row: the status row, then
    /// its commit lines.
    async fn checked_row(&mut self, row: &RepoStatus) -> String {
        let path = self.absolute(row);
        let tracked: Vec<_> = row
            .branches
            .iter()
            .filter(|b| b.upstream.is_some())
            .collect();
        let behind: u32 = tracked.iter().map(|b| b.behind).sum();
        let ahead: u32 = tracked.iter().map(|b| b.ahead).sum();
        let op = operation(&path);
        let stashes = stash_count(&path);
        let fetch_failed = matches!(row.fetch.outcome, crate::git::FetchOutcome::Failed(_));

        let symbols = render::status_symbols(behind, ahead, row.dirty, stashes);
        let noted = match fetch_failed {
            true => render::with_op("fetch failed", &symbols),
            false => symbols,
        };
        let mut status = render::with_op(op, &noted);

        let counts = &mut self.counts;
        counts.in_progress += u32::from(!op.is_empty());
        counts.fetch_failed += u32::from(fetch_failed);
        counts.behind += u32::from(behind > 0);
        counts.ahead += u32::from(ahead > 0);
        counts.uncommitted += u32::from(row.dirty > 0);
        counts.stashed += stashes;

        // bash appends the count after the counters, and a row with only
        // open PRs is not synced.
        let prs = self.open_prs(row);
        if prs > 0 {
            if !status.is_empty() {
                status.push(' ');
            }
            status.push_str(&format!("{prs}PR"));
        }

        let changed = !status.is_empty();
        let mut text = match changed {
            true => {
                let color = row_color(&status, op, row.dirty, stashes);
                self.status_row(row, &status, color)
            }
            false => {
                self.counts.synced += 1;
                self.status_row(row, "synced", palette::GREEN)
            }
        };
        if self.releases {
            text.push_str(&self.layout.release(&release_status(&path).await));
        }
        let with_stat = changed || self.stat;

        let diverged: Vec<_> = tracked
            .iter()
            .filter(|b| b.behind > 0 || b.ahead > 0)
            .collect();
        if diverged.is_empty() {
            text.push_str(&self.commit_lines(&path, "-1", with_stat).await);
        }
        for branch in diverged {
            let upstream = branch.upstream.as_deref().unwrap_or_default();
            if branch.behind > 0 {
                let range = format!("{}..{upstream}", branch.name);
                text.push_str(&self.commit_lines(&path, &range, with_stat).await);
            }
            if branch.ahead > 0 {
                let range = format!("{upstream}..{}", branch.name);
                text.push_str(&self.commit_lines(&path, &range, with_stat).await);
            }
        }
        text
    }

    fn status_row(&self, row: &RepoStatus, status: &str, color: &str) -> String {
        self.layout.row(
            Glyph::of(row.repo_kind),
            self.github_name(row),
            row.path.as_deref(),
            status,
            color,
        )
    }

    fn not_cloned_row(&mut self, row: &RepoStatus) -> String {
        self.counts.not_local += 1;
        let status = match self.open_prs(row) {
            0 => "not cloned".to_string(),
            prs => format!("not cloned {prs}PR"),
        };
        self.layout.row(
            Glyph::of(row.repo_kind),
            &row.name,
            None,
            &status,
            palette::RED,
        )
    }

    /// bash's pass 3 row (`local_news`); a row with nothing to say is
    /// skipped.
    fn local_row(&mut self, row: &RepoStatus) -> String {
        let path = self.absolute(row);
        let Some(LocalNews { ahead, op, stashes }) = local_news(&path, row) else {
            return String::new();
        };
        let status = render::with_op(op, &render::status_symbols(0, ahead, row.dirty, stashes));
        self.counts.in_progress += u32::from(!op.is_empty());
        self.counts.stashed += stashes;
        self.counts.uncommitted += 1;
        // The folder's own spelling, dot and all, as bash's `basename`.
        let name = path
            .file_name()
            .map_or_else(|| row.name.clone(), |n| n.to_string_lossy().into_owned());
        self.layout.row(
            Glyph::of(row.repo_kind),
            &name,
            row.path.as_deref(),
            &status,
            row_color(&status, op, row.dirty, stashes),
        )
    }

    /// bash's `print_commits` for one range, the diff stat only on a row
    /// that has something to say.
    async fn commit_lines(&self, path: &Path, range: &str, with_stat: bool) -> String {
        crate::git::commits(path, range, with_stat, self.author, &[])
            .await
            .iter()
            .map(|c| {
                self.layout
                    .commit(&c.hash, &c.message, &c.author, c.insertions, c.deletions)
            })
            .collect()
    }
}

/// A row's fallback colour, by bash's ladder: peach, yellow when files are
/// uncommitted, mauve when only stashes speak, red during an operation.
fn row_color(status: &str, op: &str, dirty: u32, stashes: u32) -> &'static str {
    if !op.is_empty() {
        palette::RED
    } else if status == format!("{stashes}{}", render::SYM_STASH) {
        palette::MAUVE
    } else if dirty > 0 {
        palette::YELLOW
    } else {
        palette::PEACH
    }
}

/// bash's `release_status`: commits on HEAD since the newest reachable tag
/// with a digit in it (`--no-merges`), `released <tag>` when none, or
/// `unreleased` when no such tag is reachable.
async fn release_status(path: &Path) -> String {
    let describe = ["describe", "--tags", "--abbrev=0", "--match", "*[0-9]*"];
    let tag = match crate::git::run_git(path, &describe).await {
        Some(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        _ => return "unreleased".to_string(),
    };
    let range = format!("{tag}..HEAD");
    let since = crate::git::run_git(path, &["rev-list", "--count", "--no-merges", &range])
        .await
        .filter(|out| out.status.success())
        .and_then(|out| {
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse::<u64>()
                .ok()
        })
        .unwrap_or(0);
    match since {
        0 => format!("released {tag}"),
        n => format!("{n} since {tag}"),
    }
}

/// The operation a checkout is in the middle of, from git's marker files,
/// as bash's `repo_operation` reads them. Empty when none.
fn operation(path: &Path) -> &'static str {
    let Some(dirs) = crate::git::repo_dirs(path) else {
        return "";
    };
    let git = dirs.git_dir;
    if git.join("MERGE_HEAD").is_file() {
        "merging"
    } else if git.join("rebase-merge").is_dir() || git.join("rebase-apply").is_dir() {
        "rebasing"
    } else if git.join("CHERRY_PICK_HEAD").is_file() {
        "cherry-picking"
    } else if git.join("REVERT_HEAD").is_file() {
        "reverting"
    } else if git.join("BISECT_LOG").is_file() {
        "bisecting"
    } else {
        ""
    }
}

/// Stashes, as lines of the stash reflog in the common dir (bash's
/// `stash_count`): no subprocess, and a linked worktree sees its clone's.
fn stash_count(path: &Path) -> u32 {
    crate::git::repo_dirs(path)
        .and_then(|dirs| std::fs::read(dirs.common_dir.join("logs/refs/stash")).ok())
        .map_or(0, |log| log.iter().filter(|&&b| b == b'\n').count() as u32)
}

#[cfg(test)]
mod tests;
