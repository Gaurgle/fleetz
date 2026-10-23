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
    let argv = std::iter::once("fleetz".to_string()).chain(args.iter().cloned());
    let args = Args::try_parse_from(argv).map_err(|e| invalid(e.to_string()))?;
    tokio::runtime::Runtime::new()?.block_on(print(&args, out))
}

/// Print mode for parsed `args`: the report goes to `out` and nowhere else,
/// and no terminal state is touched. Returns the exit status: 1 when GitHub
/// could not be reached, as bash repoz exits.
pub async fn print(args: &Args, out: &mut dyn Write) -> io::Result<i32> {
    let mode = args.mode();
    if mode == Mode::Dirty {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "print mode has no -d yet (FZ-48)",
        ));
    }
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
        glyphs: !plain,
    };

    let (cutoff, header) = window(args, &mode, &config).map_err(invalid)?;
    let Ok(dataset) = poll(config.repo_limit).await else {
        out.write_all(b"  Failed to reach GitHub. Run: gh auth login\n")?;
        return Ok(1);
    };
    let active = active_repos(&dataset, cutoff, args).map_err(invalid)?;
    let (rows, _notes) = crate::assemble_view_held(
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
    let mut carried: HashSet<crate::RemoteKey> = active
        .iter()
        .filter(|entry| !replaced.iter().any(|row| crate::carries(row, entry)))
        .map(|entry| crate::remote_key(entry.owner.as_deref(), &entry.name))
        .collect();

    let claimed: Vec<PathBuf> = shown.iter().map(|row| on_disk(home, row)).collect();
    let claimed: HashSet<&Path> = claimed.iter().map(PathBuf::as_path).collect();
    let candidates = crate::unclaimed_local(
        crate::discovery::find_all_local(scan_roots),
        &claimed,
        &HashSet::new(),
    );
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(crate::probe_concurrency()));
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
    let mut local: Vec<(&Path, RepoStatus)> = probed
        .iter()
        .filter(|(_, path, row)| local_news(path, row).is_some())
        .map(|(_, path, row)| (path.as_path(), row.clone()))
        .collect();
    crate::mark_local_carriers(&mut local, dataset, scan_roots, &mut carried);
    shown.extend(local.into_iter().map(|(_, row)| row));

    for entry in active {
        if carried.contains(&crate::remote_key(entry.owner.as_deref(), &entry.name)) {
            continue;
        }
        let mut row = bare_row(entry.name.clone(), None, RepoLocation::NotCloned);
        row.owner = entry.owner.clone();
        row.apply_github(entry);
        shown.push(row);
    }
    shown
}

/// A pass 3 candidate with what bash's pass 3 reads: the uncommitted count,
/// the branches and their upstreams, and the origin carriers are chosen by.
async fn local_candidate(path: &Path, name: String) -> RepoStatus {
    let mut row = bare_row(
        name,
        Some(crate::tilde_display(path)),
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
    let (cutoff, mut header) = crate::resolve_cutoff(None, mode, config)?;
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
        local.sort_by_cached_key(|row| {
            let mut key = self.absolute(row).into_os_string().into_encoded_bytes();
            key.extend_from_slice(b"/.git");
            key
        });

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
            .position(|r| crate::names_remote(row.owner.as_deref(), &row.name, r))
            .unwrap_or(usize::MAX)
    }

    /// The GitHub repo's own spelling of a matched row's name, as bash
    /// prints it; a folder match reports the folder's spelling otherwise.
    fn github_name<'r>(&'r self, row: &'r RepoStatus) -> &'r str {
        self.active
            .iter()
            .find(|r| crate::names_remote(row.owner.as_deref(), &row.name, r))
            .map_or(row.name.as_str(), |r| r.name.as_str())
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
        let status = render::with_op(op, &noted);

        let counts = &mut self.counts;
        counts.in_progress += u32::from(!op.is_empty());
        counts.fetch_failed += u32::from(fetch_failed);
        counts.behind += u32::from(behind > 0);
        counts.ahead += u32::from(ahead > 0);
        counts.uncommitted += u32::from(row.dirty > 0);
        counts.stashed += stashes;

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

        let diverged: Vec<_> = tracked
            .iter()
            .filter(|b| b.behind > 0 || b.ahead > 0)
            .collect();
        if diverged.is_empty() {
            text.push_str(&self.commit_lines(&path, "-1", changed).await);
        }
        for branch in diverged {
            let upstream = branch.upstream.as_deref().unwrap_or_default();
            if branch.behind > 0 {
                let range = format!("{}..{upstream}", branch.name);
                text.push_str(&self.commit_lines(&path, &range, changed).await);
            }
            if branch.ahead > 0 {
                let range = format!("{upstream}..{}", branch.name);
                text.push_str(&self.commit_lines(&path, &range, changed).await);
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
        self.layout.row(
            Glyph::of(row.repo_kind),
            &row.name,
            None,
            "not cloned",
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
mod tests {
    use super::parity::{Fixture, strip_ansi};
    use super::*;

    fn text(bytes: &[u8]) -> String {
        strip_ansi(&String::from_utf8_lossy(bytes))
    }

    fn parse(args: &[&str]) -> Args {
        use clap::Parser;
        Args::parse_from(std::iter::once("fleetz").chain(args.iter().copied()))
    }

    /// The line of `report` naming `name`, without its escapes.
    fn line_of<'a>(report: &'a str, name: &str) -> &'a str {
        report
            .lines()
            .find(|line| line.split_whitespace().any(|word| word == name))
            .unwrap_or_else(|| panic!("no row for {name} in\n{report}"))
    }

    /// D9: the default mode's cutoff and header are `time.rs`'s slot.
    #[test]
    fn the_default_mode_cuts_off_at_the_slot_time_rs_gives() {
        let config = crate::config::Config::default();
        let before = crate::time::auto_cutoff(config.work_start, config.work_end);
        let (cutoff, header) = window(&parse(&["--print"]), &Mode::Auto, &config).unwrap();
        let after = crate::time::auto_cutoff(config.work_start, config.work_end);
        // Read on both sides, so a slot boundary crossed mid-test still has
        // one side to agree with.
        let ours = (cutoff.unwrap(), header);
        assert!(ours == before || ours == after, "{ours:?} vs {before:?}");
    }

    #[test]
    fn today_cuts_off_at_local_midnight() {
        let config = crate::config::Config::default();
        let (cutoff, header) = window(&parse(&["-t"]), &Mode::Today, &config).unwrap();
        let midnight = crate::time::date_to_utc_midnight(chrono::Local::now().date_naive());
        assert_eq!((cutoff, header.as_str()), (Some(midnight), "today"));
    }

    /// bash names a relative date by the date it resolved to.
    #[test]
    fn a_relative_since_and_before_read_as_their_dates() {
        let config = crate::config::Config::default();
        let args = parse(&["--since", "3d", "--before", "yesterday"]);
        let (_, header) = window(&args, &args.mode(), &config).unwrap();
        let today = chrono::Local::now().date_naive();
        let since = today - chrono::Duration::days(3);
        let before = today - chrono::Duration::days(1);
        assert_eq!(header, format!("since {since} \u{2192} before {before}"));
    }

    /// No terminal state, structurally: the `--print` branch in `main` sits
    /// before the first terminal call.
    #[test]
    fn main_branches_to_print_mode_before_any_terminal_call() {
        let main = include_str!("../main.rs");
        let body = &main[main.find("async fn main()").expect("no main")..];
        let branch = body.find("if args.print").expect("no --print branch");
        for call in [
            "crossterm::terminal::size()",
            "enable_raw_mode()?",
            "EnterAlternateScreen",
            "spawn_input_thread(",
        ] {
            let at = body.find(call).unwrap_or_else(|| panic!("no {call}"));
            assert!(branch < at, "--print branches after {call}");
        }
    }

    /// No terminal state, at run time: the report goes to the `Write` it is
    /// handed, and nothing of it reaches the process's own stdout or stderr.
    #[test]
    fn print_mode_writes_only_to_the_writer_it_is_given() {
        let fixture = Fixture::build("writer");
        let (report, child) = fixture.run_print_mode_full(&["-a"]);
        assert!(text(&report).contains("alpha-tool"));
        for (stream, bytes) in [("stdout", &child.stdout), ("stderr", &child.stderr)] {
            let bytes = String::from_utf8_lossy(bytes);
            assert!(
                !bytes.contains('\u{1b}') && !bytes.contains("alpha-tool"),
                "print mode wrote to {stream}: {bytes}"
            );
        }
    }

    /// D5: a poll through the fixture seam leaves this credential's keyed
    /// cache behind for the next TUI launch.
    #[test]
    fn a_print_run_writes_the_keyed_cache() {
        let fixture = Fixture::build("keyed");
        fixture.run_print_mode(&["-a"]);
        let written: Vec<PathBuf> = ["Library/Caches/fleetz", ".cache/fleetz"]
            .iter()
            .filter_map(|dir| std::fs::read_dir(fixture.home.join(dir)).ok())
            .flatten()
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .collect();
        let account = github::token_fingerprint("fixture");
        assert!(
            written.iter().any(|path| path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().contains(&account))),
            "no keyed cache file for the account in {written:?}"
        );
    }

    /// D3, matching: a clone in a folder named for a payload repo whose
    /// origin names another owner's repo does not claim the row. bash repoz
    /// matches by folder name and would list it under the roots.
    #[test]
    fn a_folder_name_does_not_claim_a_repo_its_origin_disowns() {
        let fixture = Fixture::build("d3_owner");
        fixture.publish(
            "someone-else",
            super::parity::NOT_CLONED,
            "2026-02-01T09:00:00Z",
        );
        let stranger = fixture.repo_dir.join(super::parity::NOT_CLONED);
        fixture.clone_into("someone-else", super::parity::NOT_CLONED, &stranger);

        let report = text(&fixture.run_print_mode(&["-a"]));
        let row = line_of(&report, super::parity::NOT_CLONED);
        assert!(row.ends_with("not cloned"), "{row}");
        assert!(!report.contains("~/repos/ghost-repo"), "{report}");
    }

    /// D3, several clones: the exact origin outranks a folder match in an
    /// earlier root (`CheckoutRank`). bash repoz takes the first folder
    /// match under `REPO_CHECK_DIR`.
    #[test]
    fn of_two_clones_the_ranked_one_is_the_row() {
        let fixture = Fixture::build("d3_rank");
        let unlinked = fixture.repo_dir.join("orgproj");
        fixture.clone_into(super::parity::ORG, "orgproj", &unlinked);
        fixture.git(&unlinked, &["remote", "remove", "origin"]);

        let report = text(&fixture.run_print_mode(&["-a"]));
        let row = line_of(&report, "orgproj");
        assert!(row.contains("~/extra/orgproj"), "{row}");
        assert!(!report.contains("~/repos/orgproj"), "{report}");
    }

    /// D3, which clones fetch: only a remote GitHub says moved since the
    /// clone's last fetch, and a clone that never fetched. bash repoz
    /// fetches every matched clone.
    #[test]
    fn only_stale_remotes_are_fetched() {
        let fixture = Fixture::build("d3_stale");
        fixture.push_upstream(super::parity::LOGIN, "alpha-tool", "Second parser pass");
        fixture.push_upstream(super::parity::ORG, "orgproj", "Org follow-up");
        // Pushed before its last fetch by the payload's clock: not fetched,
        // so the new commit stays unseen and the row stays at 1 behind.
        // Never fetched: fetched, and the new commit shows.
        std::fs::remove_file(fixture.org_clone.join(".git/FETCH_HEAD")).unwrap();

        let report = text(&fixture.run_print_mode(&["-a"]));
        assert!(
            line_of(&report, "alpha-tool").ends_with(" 1\u{2193}"),
            "{report}"
        );
        assert!(
            line_of(&report, "orgproj").ends_with(" 1\u{2193}"),
            "{report}"
        );
        assert!(report.contains("Org follow-up"), "{report}");
        assert!(!report.contains("Second parser pass"), "{report}");
    }

    /// D3, archived repos: print mode polls with fleetz's query, which asks
    /// GitHub to leave archived repos out of both connections (O3).
    #[tokio::test]
    async fn the_poll_leaves_archived_repos_out() {
        let dir =
            std::env::temp_dir().join(format!("fleetz_report_archived_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let payload = include_str!("fixtures/graphql.json").to_string();
        let (url, request) = serve_capturing(payload).await;
        let cache = crate::cache::KeyedCache::at(dir.clone(), 50, github::HOST);

        let fetch = poll_at(50, &cache, || async { Ok("tok".to_string()) }, &url).await;
        let request = request.await.unwrap();
        let keyed = cache.for_account(&github::token_fingerprint("tok"));
        let cached = keyed.read();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(fetch.expect("the poll failed").repos.len(), 4);
        assert_eq!(request.matches("isArchived: false").count(), 2, "{request}");
        assert!(cached.is_some(), "the poll wrote no keyed cache");
    }

    /// O4: the poll goes to GitHub even when this credential's keyed cache
    /// holds a payload (`CacheMode::Bypass`); a cache-served answer would
    /// never reach the listener.
    #[tokio::test]
    async fn the_poll_asks_github_past_a_cached_payload() {
        let dir = std::env::temp_dir().join(format!("fleetz_report_bypass_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let payload = include_str!("fixtures/graphql.json").to_string();
        let cache = crate::cache::KeyedCache::at(dir.clone(), 50, github::HOST);
        cache
            .for_account(&github::token_fingerprint("tok"))
            .write(&payload);
        let (url, request) = serve_capturing(payload).await;

        let fetch = poll_at(50, &cache, || async { Ok("tok".to_string()) }, &url).await;
        let request = tokio::time::timeout(std::time::Duration::from_secs(5), request).await;
        let _ = std::fs::remove_dir_all(&dir);

        assert!(fetch.is_ok(), "the poll failed");
        assert!(
            request.is_ok_and(|r| r.is_ok()),
            "the poll was answered from the keyed cache, not GitHub"
        );
    }

    /// bash adds the working directory's git toplevel to its scan roots
    /// (`add_scan_dir "$_toplevel"`), so a dirty repo outside every root is
    /// in `local changes` when print mode runs from inside it.
    #[test]
    fn the_working_directorys_repo_is_scanned() {
        let fixture = Fixture::build("cwd_root");
        let loose = fixture.home.join("loose/scratch");
        let inside = loose.join("src");
        std::fs::create_dir_all(&inside).unwrap();
        fixture.git(&loose, &["init", "-q"]);
        std::fs::write(inside.join("wip.txt"), "wip\n").unwrap();

        let elsewhere = text(&fixture.run_print_mode(&["-a"]));
        assert!(!elsewhere.contains("scratch"), "{elsewhere}");

        let (report, _) = fixture.run_print_mode_from(&inside, &["-a"]);
        let report = text(&report);
        let (_, local) = report
            .split_once("local changes")
            .unwrap_or_else(|| panic!("no local changes section\n{report}"));
        let row = line_of(local, "scratch");
        assert!(
            row.contains("~/loose/scratch") && row.ends_with(" 1!"),
            "{row}"
        );
    }

    /// A loopback listener answering one request with `body`, handing back
    /// the request it read.
    async fn serve_capturing(body: String) -> (String, tokio::sync::oneshot::Receiver<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (sent, received) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut seen = Vec::new();
            let mut chunk = [0u8; 16 * 1024];
            loop {
                let text = String::from_utf8_lossy(&seen).to_lowercase();
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if seen.len() >= end + 4 + length {
                        break;
                    }
                }
                match socket.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => seen.extend_from_slice(&chunk[..n]),
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = sent.send(String::from_utf8_lossy(&seen).into_owned());
        });
        (format!("http://{addr}/graphql"), received)
    }
}
