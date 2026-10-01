use crate::model::{BranchStatus, Commit, FileStat, GraphLine, ReflogEntry, RemoteBranch};
use chrono::DateTime;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Hard ceiling per git invocation so one repo on a dead network mount or a
/// credential prompt can never wedge a sweep. kill_on_drop reaps the child.
const GIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Build a hardened git command: no optional locks (polling must not take
/// index.lock and fight editors), no terminal prompts (a fetch that wants
/// credentials fails instead of hanging), C locale (parsers match English
/// words like "insertion"), no color, stdin closed.
fn git_cmd(repo_path: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("git");
    c.arg("--no-optional-locks")
        .args(["-c", "color.ui=false"])
        .args(["-C", &repo_path.to_string_lossy()])
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .kill_on_drop(true);
    c
}

/// Run a git command with the hardening and timeout. None means the process
/// could not run or timed out; callers treat that like a failed status.
pub(crate) async fn run_git(repo_path: &Path, args: &[&str]) -> Option<std::process::Output> {
    match tokio::time::timeout(GIT_TIMEOUT, git_cmd(repo_path, args).output()).await {
        Ok(Ok(output)) => Some(output),
        _ => None,
    }
}

/// Per-day commit counts for one author over the contribution grid.
///
/// `--all` rather than the default branch: work on a feature branch is still
/// work, and git de-duplicates a commit reachable from several refs. `%ad`
/// with `--date=short` is the author date in local time, which is the day you
/// would say you did the work on.
pub async fn daily_commits(
    repo_path: &Path,
    start: chrono::NaiveDate,
    author: &str,
) -> Option<crate::contributions::DailyCounts> {
    let since = format!("--since={}", start.format("%Y-%m-%d"));
    let author_arg = format!("--author={author}");
    let output = run_git(
        repo_path,
        &[
            "log",
            "--all",
            "--date=short",
            "--pretty=%ad",
            &since,
            &author_arg,
        ],
    )
    .await?;
    if !output.status.success() {
        return None;
    }
    Some(parse_daily(&String::from_utf8_lossy(&output.stdout), start))
}

/// Bucket `git log --date=short --pretty=%ad` output into grid slots. Lines
/// that are not a date are skipped rather than treated as errors: a warning
/// on stdout must not cost a year of history.
pub fn parse_daily(stdout: &str, start: chrono::NaiveDate) -> crate::contributions::DailyCounts {
    let mut counts = crate::contributions::DailyCounts::default();
    for line in stdout.lines() {
        let Ok(date) = chrono::NaiveDate::parse_from_str(line.trim(), "%Y-%m-%d") else {
            continue;
        };
        let offset = (date - start).num_days();
        if offset >= 0 && (offset as usize) < crate::contributions::SLOTS {
            let slot = offset as usize;
            counts.0[slot] = counts.0[slot].saturating_add(1);
        }
    }
    counts
}

/// The identity git would stamp on a commit made here, honouring a
/// repo-local override. A clone configured with a work address must count
/// against that address, not against the global one.
pub async fn user_email(repo_path: &Path) -> Option<String> {
    let output = run_git(repo_path, &["config", "user.email"]).await?;
    if !output.status.success() {
        return None;
    }
    let email = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!email.is_empty()).then_some(email)
}

/// A cheap fingerprint of everything the expensive probes depend on. Equal
/// keys across two sweeps mean log, reflog, graph, stash, and worktree output
/// cannot have changed, so those subprocesses can be skipped entirely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkKey {
    /// Output of rev-parse HEAD: one cheap subprocess buys exact commit identity.
    pub head_sha: String,
    pub index_mtime: Option<std::time::SystemTime>,
    pub index_size: u64,
    /// Size of the stash reflog (common dir aware); 0 when absent.
    pub stash_size: u64,
    /// Fingerprint of the refs the graph decorates: the newer mtime of
    /// `packed-refs` and the loose `refs/tags` directory, that file's size,
    /// and how many loose tags there are.
    ///
    /// Tags are why this exists. Creating one moves no commit, no index and no
    /// stash, so without this the fingerprint was identical across the sweep
    /// and all six probes were skipped -- and the tag never reached the pane.
    /// That is precisely what the release flow does: the `chore: release`
    /// commit lands first, the tag is created after it, and nothing else
    /// moves. Every release tag was invisible until something unrelated
    /// happened to move HEAD.
    ///
    /// Three parts because one is not enough on a coarse filesystem: the size
    /// catches a `packed-refs` rewrite whose mtime did not tick, and the count
    /// catches a loose tag added or deleted inside the same mtime granule.
    ///
    /// Filesystem only, no subprocess: this is paid for every repo on every
    /// sweep, and the whole point of the key is to be cheaper than what it
    /// gates.
    ///
    /// Known limit: a tag nested under a subdirectory that already exists
    /// (`refs/tags/rc/2` when `refs/tags/rc/1` is there) moves neither the
    /// directory mtime nor the top-level count. Flat tag names, which is
    /// every convention in normal use, are covered.
    pub refs_mtime: Option<std::time::SystemTime>,
    pub packed_refs_size: u64,
    pub loose_tag_count: usize,
}

/// The ref fingerprint for `WorkKey`. Takes the *common* dir: a linked
/// worktree keeps its refs with the main checkout, not beside its own HEAD.
fn refs_fingerprint(common_dir: &Path) -> (Option<std::time::SystemTime>, u64, usize) {
    let packed = std::fs::metadata(common_dir.join("packed-refs")).ok();
    let packed_size = packed.as_ref().map(|m| m.len()).unwrap_or(0);
    let packed_mtime = packed.as_ref().and_then(|m| m.modified().ok());

    let tags_dir = common_dir.join("refs").join("tags");
    let tags_mtime = std::fs::metadata(&tags_dir)
        .ok()
        .and_then(|m| m.modified().ok());
    let loose_tags = std::fs::read_dir(&tags_dir).map(|d| d.count()).unwrap_or(0);

    (packed_mtime.max(tags_mtime), packed_size, loose_tags)
}

/// The repo's real git directory: `.git` itself, or the target of a
/// `gitdir:` pointer file (linked worktrees, submodule checkouts).
pub fn resolve_git_dir(repo_path: &Path) -> Option<std::path::PathBuf> {
    let dot = repo_path.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let target = text.strip_prefix("gitdir:")?.trim();
    let p = std::path::PathBuf::from(target);
    Some(if p.is_absolute() {
        p
    } else {
        repo_path.join(p)
    })
}

/// Where a checkout's git metadata lives on disk.
///
/// A linked worktree splits its metadata in two. Per worktree, in `git_dir`:
/// `HEAD`, `index`, `logs/HEAD` and `FETCH_HEAD`. Shared with the main clone,
/// in `common_dir`: `refs/`, `packed-refs`, `logs/refs/` (the stash log among
/// them), `config` and `worktrees/`. A plain clone and a submodule checkout
/// have no `commondir` file, so both fields name the same directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoDirs {
    pub git_dir: std::path::PathBuf,
    pub common_dir: std::path::PathBuf,
}

/// Resolve a checkout's git and common directories, read from disk without
/// spawning git. `None` when `.git` is neither a directory nor a readable
/// `gitdir:` file.
///
/// `<git_dir>/commondir` is trimmed of surrounding whitespace and joined onto
/// `git_dir`: a relative path stays unnormalized (`.../worktrees/lw/../..`),
/// and an absolute one replaces `git_dir` outright, as `Path::join` does.
pub fn repo_dirs(repo_path: &Path) -> Option<RepoDirs> {
    let git_dir = resolve_git_dir(repo_path)?;
    let common_dir = match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(rel) => git_dir.join(rel.trim()),
        Err(_) => git_dir.clone(),
    };
    Some(RepoDirs {
        git_dir,
        common_dir,
    })
}

/// When this clone last ran `git fetch`, from the mtime of `FETCH_HEAD`.
///
/// `None` means it has never fetched -- a fresh `git clone` writes no
/// `FETCH_HEAD` -- which the caller must read as "assume stale", not as
/// "nothing to do".
pub fn last_fetch_at(repo_path: &Path) -> Option<std::time::SystemTime> {
    let git_dir = repo_dirs(repo_path)?.git_dir;
    std::fs::metadata(git_dir.join("FETCH_HEAD"))
        .ok()?
        .modified()
        .ok()
}

/// When this clone last fetched *successfully*, as far as the disk can say:
/// the mtime of `FETCH_HEAD`, but only when that file is not empty.
///
/// A failed fetch truncates `FETCH_HEAD` to nothing and still moves its mtime
/// (pinned by `a_failed_fetch_empties_fetch_head`), so the bare mtime counts a
/// failure as a fetch. Rule 7 of `docs/fetch-outcome-contract.md`: an empty
/// file is no fetch at all, and the caller reads `None` as unknown.
///
/// A successful fetch that brought nothing (a clone with no remote) also
/// leaves it empty, and reads as unknown too. That costs a fetch, not wrong
/// data.
pub fn last_successful_fetch_at(repo_path: &Path) -> Option<std::time::SystemTime> {
    let at = last_fetch_at(repo_path)?;
    let git_dir = repo_dirs(repo_path)?.git_dir;
    let written = std::fs::metadata(git_dir.join("FETCH_HEAD")).ok()?.len() > 0;
    written.then_some(at)
}

/// Cheap change detector: if none of these moved, the expensive probes
/// (log, reflog, graph, stash, worktree) cannot have new answers. Untracked
/// files do not touch the index, so dirty_count is always probed regardless.
pub async fn work_key(repo_path: &Path) -> Option<WorkKey> {
    let head = run_git(repo_path, &["rev-parse", "HEAD"]).await?;
    if !head.status.success() {
        return None; // unborn HEAD etc: treat as always-changed
    }
    let head_sha = String::from_utf8_lossy(&head.stdout).trim().to_string();

    // Stash log lives in the COMMON dir for linked worktrees.
    let RepoDirs {
        git_dir,
        common_dir,
    } = repo_dirs(repo_path)?;
    let index_meta = std::fs::metadata(git_dir.join("index")).ok();
    let stash_size = std::fs::metadata(common_dir.join("logs/refs/stash"))
        .map(|m| m.len())
        .unwrap_or(0);

    let (refs_mtime, packed_refs_size, loose_tag_count) = refs_fingerprint(&common_dir);

    Some(WorkKey {
        head_sha,
        index_mtime: index_meta.as_ref().and_then(|m| m.modified().ok()),
        index_size: index_meta.map(|m| m.len()).unwrap_or(0),
        stash_size,
        refs_mtime,
        packed_refs_size,
        loose_tag_count,
    })
}

/// Why a `git fetch` failed, grouped by what you would do about it. Read from
/// git's stderr under the C locale `git_cmd` sets (`classify_fetch_error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchFailureKind {
    /// No answer within `GIT_TIMEOUT`.
    TimedOut,
    /// Credentials wanted or refused. Prompts are disabled, so a remote that
    /// asks for a password fails here instead of hanging.
    Auth,
    /// The network, DNS or the host itself.
    Unreachable,
    /// The remote is missing or is not a repository.
    NoRemote,
    /// Anything else, including git failing to start.
    Other,
}

/// A failed fetch: its kind, and at most git's first error line with any URL
/// userinfo removed (rule 8). Never the raw stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchFailure {
    pub kind: FetchFailureKind,
    pub detail: Option<String>,
}

/// How a fetch for one checkout ended. `NotAttempted` is also what a sweep
/// that did not fetch reports: requested is not successful (rule 1), and not
/// requested says nothing at all (rule 2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FetchOutcome {
    #[default]
    NotAttempted,
    Succeeded,
    Failed(FetchFailure),
}

/// How the fetch process ended, before it is read as an outcome. `run_git`
/// folds the first two into one `None`; a fetch has to tell them apart.
enum FetchRun {
    TimedOut,
    NotStarted,
    Exited(std::process::Output),
}

const FETCH_ARGS: &[&str] = &["fetch", "--quiet", "--prune"];

/// The longest git error line kept as a failure's detail.
const FETCH_DETAIL_MAX: usize = 160;

/// Run `git fetch` on a repo. Prunes deleted remote branches so
/// remote_only_branches does not accumulate stale entries.
///
/// Its own path rather than `run_git`, which every other caller keeps: a
/// fetch is the one probe whose failure the user has to be told about, and
/// `run_git` cannot say why one failed.
pub async fn fetch(repo_path: &Path) -> FetchOutcome {
    fetch_within(repo_path, GIT_TIMEOUT).await
}

async fn fetch_within(repo_path: &Path, limit: std::time::Duration) -> FetchOutcome {
    let run = match tokio::time::timeout(limit, git_cmd(repo_path, FETCH_ARGS).output()).await {
        Err(_) => FetchRun::TimedOut,
        Ok(Err(_)) => FetchRun::NotStarted,
        Ok(Ok(output)) => FetchRun::Exited(output),
    };
    fetch_outcome(run)
}

fn fetch_outcome(run: FetchRun) -> FetchOutcome {
    let failure = match run {
        FetchRun::Exited(output) if output.status.success() => return FetchOutcome::Succeeded,
        FetchRun::Exited(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            FetchFailure {
                kind: classify_fetch_error(&stderr),
                detail: first_error_line(&stderr),
            }
        }
        FetchRun::TimedOut => FetchFailure {
            kind: FetchFailureKind::TimedOut,
            detail: None,
        },
        FetchRun::NotStarted => FetchFailure {
            kind: FetchFailureKind::Other,
            detail: Some("git could not be started".to_string()),
        },
    };
    FetchOutcome::Failed(failure)
}

/// Classify a failed fetch by git's stderr. Substrings of the messages git,
/// curl and ssh print under the C locale, checked in this order so that a
/// refused credential is not mistaken for a missing repository: git prints
/// "Could not read from remote repository" after both.
fn classify_fetch_error(stderr: &str) -> FetchFailureKind {
    const AUTH: &[&str] = &[
        "authentication failed",
        "could not read username",
        "could not read password",
        "terminal prompts disabled",
        "permission denied (",
        "host key verification failed",
        "invalid username or password",
        "access denied",
        "returned error: 401",
        "returned error: 403",
    ];
    const UNREACHABLE: &[&str] = &[
        "could not resolve host",
        "temporary failure in name resolution",
        "name or service not known",
        "nodename nor servname",
        "connection refused",
        "connection timed out",
        "operation timed out",
        "connection reset",
        "network is unreachable",
        "no route to host",
        "failed to connect",
    ];
    const NO_REMOTE: &[&str] = &[
        "does not appear to be a git repository",
        "repository not found",
        "no such remote",
        "no remote repository specified",
        "returned error: 404",
    ];
    let text = stderr.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| text.contains(n));
    // git's own wording for an HTTP 404: `repository '<url>' not found`, with
    // the URL between the two halves.
    let missing_repo = text
        .lines()
        .any(|l| l.contains("repository '") && l.trim_end().ends_with("' not found"));
    if has(AUTH) {
        FetchFailureKind::Auth
    } else if has(UNREACHABLE) {
        FetchFailureKind::Unreachable
    } else if missing_repo || has(NO_REMOTE) {
        FetchFailureKind::NoRemote
    } else {
        FetchFailureKind::Other
    }
}

/// git's first error line, made safe to draw: the first `fatal:` or `error:`
/// line, else the first non-empty one, with URL userinfo removed, control
/// characters dropped and the length capped.
fn first_error_line(stderr: &str) -> Option<String> {
    let lines = || stderr.lines().map(str::trim).filter(|l| !l.is_empty());
    let line = lines()
        .find(|l| {
            let lower = l.to_lowercase();
            lower.starts_with("fatal:") || lower.starts_with("error:")
        })
        .or_else(|| lines().next())?;
    let clean: String = redact_userinfo(line)
        .chars()
        .filter(|c| !c.is_control())
        .take(FETCH_DETAIL_MAX)
        .collect();
    Some(clean)
}

/// `text` with the credentials removed from every `scheme://user:pass@host`
/// URL in it, so a credential embedded in a remote cannot reach the screen.
///
/// The authority is found by the RFC 3986 delimiters only: everything after
/// `://` up to the first `/`, `?`, `#`, whitespace or the end of the text.
/// If it holds an `@`, everything before the *last* one is userinfo. Quotes,
/// commas and parentheses are legal unencoded in userinfo, so they get no
/// special handling: ending the authority at one cut a password in half and
/// let the whole URL through. Over-redacting odd text is acceptable; a
/// credential on screen is not.
///
/// For the ssh family (`ssh`, and any scheme containing `ssh`, such as
/// `git+ssh`) only `:password` goes and the user stays: an ssh user is a
/// login name, and dropping it would draw a URL that differs from
/// `git remote -v`. Every other scheme loses all userinfo, since a token is
/// commonly written as the user alone (`https://<token>@host/...`) and no
/// list of http-like schemes can be complete. scp-style remotes
/// (`git@host:owner/repo`) have no `://` and cannot carry a password, so they
/// are left as written.
///
/// A password must be percent-encoded to be stripped: one with an unencoded
/// `/`, `?`, `#` or whitespace is passed through as git prints it, and git
/// rejects such a URL, so it can never have authenticated.
pub fn redact_userinfo(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("://") {
        // By characters, never `rfind` plus one: the character found can be
        // several bytes long (a smart quote before a pasted URL), and slicing
        // one byte past its start panicked inside it.
        let scheme_start = rest[..i]
            .char_indices()
            .rev()
            .find(|(_, c)| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
            .map_or(0, |(p, c)| p + c.len_utf8());
        let scheme = rest[scheme_start..i].to_ascii_lowercase();
        let (head, tail) = rest.split_at(i + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| matches!(c, '/' | '?' | '#') || c.is_whitespace())
            .unwrap_or(tail.len());
        let authority = &tail[..end];
        match authority.rsplit_once('@') {
            None => out.push_str(authority),
            Some((userinfo, host)) if scheme.contains("ssh") => {
                let user = userinfo.split_once(':').map_or(userinfo, |(u, _)| u);
                out.push_str(user);
                out.push('@');
                out.push_str(host);
            }
            Some((_, host)) => out.push_str(host),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// Get all branch statuses for a repo (behind/ahead relative to upstream).
pub async fn branch_statuses(repo_path: &Path) -> Vec<BranchStatus> {
    let Some(output) = run_git(
        repo_path,
        &[
            "for-each-ref",
            // `%(HEAD)` is `*` on the checked-out branch and a space on every
            // other, so which branch you are on rides the query that was
            // already being made rather than costing a second subprocess.
            "--format=%(HEAD)\t%(refname:short)\t%(upstream:short)\t%(upstream:track)\t%(objectname:short)",
            "refs/heads/",
        ],
    )
    .await
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut statuses = parse_branch_output(&stdout);

    // `upstream:track` only measures divergence from a branch's own upstream,
    // which is wrong in both directions:
    //
    // - It over-reports when the upstream is stale (a leftover worktree branch
    //   already merged into main via a different ref still reads "ahead" even
    //   though its commits are safely on the remote).
    // - It says nothing at all when a branch has no upstream, which is the
    //   most common shape of work in progress: a feature branch created
    //   locally and not pushed yet. That hid whole repos, because a repo whose
    //   only news is unpushed branch commits reports dirty=0 ahead=0 and so
    //   never reaches the "local changes" section.
    //
    // Counting commits unreachable from ALL remotes answers both. It is only
    // meaningful when the repo has a remote at all: without one every commit
    // is unreachable, and a purely local repo would show its whole history as
    // unpushed forever. Resolved lazily so the all-tracked case pays nothing.
    let has_remote = if statuses.iter().any(|s| s.upstream.is_none()) {
        has_remotes(repo_path).await
    } else {
        false
    };
    for status in &mut statuses {
        if status.ahead > 0 || (status.upstream.is_none() && has_remote) {
            status.ahead = count_unreachable_from_remotes(repo_path, &status.name).await;
        }
    }

    statuses
}

/// Whether the repo has at least one remote configured.
async fn has_remotes(repo_path: &Path) -> bool {
    run_git(repo_path, &["remote"]).await.is_some_and(|o| {
        o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty()
    })
}

/// Count commits on `branch` not reachable from any remote-tracking ref.
async fn count_unreachable_from_remotes(repo_path: &Path, branch: &str) -> u32 {
    let output = run_git(
        repo_path,
        &["rev-list", "--count", branch, "--not", "--remotes"],
    )
    .await;

    match output {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .trim()
            .parse()
            .unwrap_or(0),
        _ => 0,
    }
}

/// Parse the output of `git for-each-ref` into BranchStatus structs.
fn parse_branch_output(output: &str) -> Vec<BranchStatus> {
    output
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            let is_head = parts.first().is_some_and(|h| h.trim() == "*");
            let name = parts.get(1).unwrap_or(&"").to_string();
            let upstream = parts
                .get(2)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let track = parts.get(3).unwrap_or(&"");

            let (ahead, behind) = parse_track_info(track);
            let tip = parts.get(4).unwrap_or(&"").trim().to_string();

            BranchStatus {
                name,
                upstream,
                behind,
                ahead,
                is_head,
                tip,
            }
        })
        .collect()
}

/// Parse git tracking info like "[ahead 2, behind 3]" or "[ahead 1]".
fn parse_track_info(track: &str) -> (u32, u32) {
    let mut ahead = 0u32;
    let mut behind = 0u32;

    if track.is_empty() {
        return (ahead, behind);
    }

    for part in track.split(['[', ']', ',']) {
        let part = part.trim();
        if let Some(n) = part.strip_prefix("ahead ") {
            ahead = n.trim().parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            behind = n.trim().parse().unwrap_or(0);
        }
    }

    (ahead, behind)
}

/// Paths `git status --porcelain` reports as changed, relative to the repo
/// root.
///
/// Untracked directories arrive collapsed (`blue-tui/` rather than every file
/// beneath it), which is what keeps `last_local_activity` from descending a
/// `target/` to date a repo.
async fn dirty_paths(repo_path: &Path) -> Vec<String> {
    let Some(output) = run_git(repo_path, &["status", "--porcelain"]).await else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_porcelain_path)
        .collect()
}

/// The path out of one `git status --porcelain` line: two status columns, a
/// space, then the path. A rename reads `R  old -> new`, and only the
/// destination exists to be stat'd.
fn parse_porcelain_path(line: &str) -> Option<String> {
    let rest = line.get(3..)?.trim();
    if rest.is_empty() {
        return None;
    }
    let path = match rest.split_once(" -> ") {
        Some((_, dest)) => dest,
        None => rest,
    };
    // Paths with spaces or non-ASCII arrive quoted; the quotes are not part
    // of the name and a stat through them finds nothing.
    Some(path.trim_matches('"').to_string())
}

/// The newest moment anything in this repo moved locally: the last commit on
/// any ref, or the newest mtime among the paths git reports as changed,
/// whichever is later.
///
/// Both halves are needed. A repo that was `git init`-ed and never committed
/// has no commit to date it by, and dating a repo by its last commit alone
/// calls it stale while you are still editing files in it. `None` means
/// nothing has ever happened here: no commits and a clean tree.
pub async fn last_local_activity(repo_path: &Path) -> Option<DateTime<chrono::Utc>> {
    let committed = last_commit_time(repo_path).await;

    let touched = {
        let mut newest = None;
        for rel in dirty_paths(repo_path).await {
            let Ok(meta) = std::fs::metadata(repo_path.join(&rel)) else {
                continue;
            };
            let Ok(mtime) = meta.modified() else { continue };
            let at: DateTime<chrono::Utc> = mtime.into();
            newest = Some(newest.map_or(at, |n: DateTime<chrono::Utc>| n.max(at)));
        }
        newest
    };

    match (committed, touched) {
        (Some(c), Some(t)) => Some(c.max(t)),
        (c, t) => c.or(t),
    }
}

/// Commit time of the newest commit on any ref, or None in a repo with no
/// commits at all -- where `git log` exits non-zero rather than empty.
async fn last_commit_time(repo_path: &Path) -> Option<DateTime<chrono::Utc>> {
    let output = run_git(repo_path, &["log", "-1", "--all", "--format=%ct"]).await?;
    if !output.status.success() {
        return None;
    }
    let secs: i64 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .ok()?;
    DateTime::from_timestamp(secs, 0)
}

/// Count uncommitted/untracked changes (like `git status --porcelain | wc -l`).
pub async fn dirty_count(repo_path: &Path) -> u32 {
    let output = run_git(repo_path, &["status", "--porcelain"]).await;

    match output {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .count() as u32,
        _ => 0,
    }
}

/// Recent commits on a specific branch (local or "origin/x"), for the
/// read-only preview of what's happened on a branch without checking it out.
pub async fn branch_commits(repo_path: &Path, branch: &str, limit: u32) -> Vec<Commit> {
    let output = run_git(
        repo_path,
        &[
            "log",
            &format!("-{limit}"),
            "--format=%x1e%h%x1f%aN%x1f%at%x1f%s%x1f%b",
            branch,
        ],
    )
    .await;

    match output {
        Some(o) if o.status.success() => parse_commit_output(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

/// Get recent commits for a range (e.g. "main..origin/main" or "-5"), read
/// from `branches` -- the same refs `graph` walks -- or from HEAD when that
/// list is empty.
///
/// The refs are not decoration. The detail pane looks a graph row up in this
/// list by hash, for the inspector's body and file list and for the churn
/// column beside the subject, and this list used to be HEAD's alone while the
/// graph was the union of the local branches. A commit reachable only from a
/// side branch was therefore drawn and then not found: no `+N -M` on its row,
/// and "outside the loaded commit range" in the pane below.
pub async fn commits(
    repo_path: &Path,
    range: &str,
    with_stat: bool,
    author_filter: Option<&str>,
    branches: &[String],
) -> Vec<Commit> {
    // Record-separated so the (possibly multi-line) body can't be confused
    // with field boundaries: each commit starts with RS (0x1e), fields are
    // split by US (0x1f). With --shortstat, a " N files changed..." line
    // trails the body inside the same record.
    let mut args = vec![
        "log".to_string(),
        "--format=%x1e%h%x1f%aN%x1f%at%x1f%s%x1f%b".to_string(),
    ];
    if with_stat {
        // --numstat gives the per-file breakdown (for the inspector's file
        // list); --shortstat adds the "N files changed, ..." summary line the
        // totals are parsed from. Combined, both trail the body in the record.
        args.push("--numstat".to_string());
        args.push("--shortstat".to_string());
    }
    if let Some(author) = author_filter {
        args.push(format!("--author={author}"));
    }
    args.push(range.to_string());
    // `-n` caps the union, not each ref, so widening from HEAD to every local
    // branch returns the same number of commits and costs about the same.
    // Empty means the repo has no local branches to name -- a detached HEAD,
    // a clone caught mid-checkout -- where git's own default of HEAD is right.
    args.extend(branches.iter().cloned());

    let output = run_git(
        repo_path,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )
    .await;

    let output = match output {
        Some(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_commit_output(&stdout)
}

/// Parse the record-separated git log output (see `commits`). Each record is
/// `hash US author US date US subject US body-and-maybe-shortstat`.
fn parse_commit_output(output: &str) -> Vec<Commit> {
    let mut result = Vec::new();

    for record in output.split('\u{1e}') {
        if record.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = record.splitn(5, '\u{1f}').collect();
        if fields.len() < 4 {
            continue;
        }
        let hash = fields[0].trim().to_string();
        let author = fields[1].to_string();
        // `%at` is unix epoch seconds; anything else means a git format we
        // don't understand, which drops the age rather than the whole commit.
        let date = fields[2]
            .trim()
            .parse::<i64>()
            .ok()
            .and_then(|secs| DateTime::from_timestamp(secs, 0));
        let message = fields[3].to_string();

        // The 5th field is the body, possibly followed by --numstat file
        // lines and a --shortstat summary. Pull the stats out (per-file into
        // `files`, totals into insertions/deletions) and keep the rest as body.
        let mut body_lines: Vec<&str> = Vec::new();
        let mut insertions = 0;
        let mut deletions = 0;
        let mut files: Vec<FileStat> = Vec::new();
        if let Some(tail) = fields.get(4) {
            for line in tail.lines() {
                if line.contains("changed")
                    && (line.contains("insertion")
                        || line.contains("deletion")
                        || line.contains("file"))
                {
                    insertions = extract_stat_number(line, "insertion");
                    deletions = extract_stat_number(line, "deletion");
                } else if let Some(file) = parse_numstat_line(line) {
                    files.push(file);
                } else {
                    body_lines.push(line);
                }
            }
        }
        let body = body_lines.join("\n").trim().to_string();

        result.push(Commit {
            hash,
            message,
            body,
            author,
            date,
            insertions,
            deletions,
            files,
        });
    }

    result
}

/// Parse one `git log --numstat` line: `<added>\t<removed>\t<path>`. Binary
/// files report `-` for both counts, which we store as 0. Returns None for
/// any line that isn't numstat-shaped (body text, the shortstat summary, etc.)
/// so the caller can fall through to treating it as body.
fn parse_numstat_line(line: &str) -> Option<FileStat> {
    let mut parts = line.splitn(3, '\t');
    let added_raw = parts.next()?;
    let removed_raw = parts.next()?;
    let path = parts.next()?;
    if path.is_empty() {
        return None;
    }
    // Both count fields must be a number or the binary `-`, else this isn't a
    // numstat line (e.g. a body line that merely contains a tab).
    let parse_count = |s: &str| -> Option<u32> {
        if s == "-" {
            Some(0)
        } else {
            s.parse::<u32>().ok()
        }
    };
    Some(FileStat {
        added: parse_count(added_raw)?,
        removed: parse_count(removed_raw)?,
        path: path.to_string(),
    })
}

fn extract_stat_number(line: &str, keyword: &str) -> u32 {
    // Find " N insertion" or " N deletion" pattern
    for part in line.split(',') {
        let part = part.trim();
        if part.contains(keyword)
            && let Some(n) = part.split_whitespace().next()
        {
            return n.parse().unwrap_or(0);
        }
    }
    0
}

/// Get the last `limit` reflog entries -- HEAD movements, for a "what did I
/// just do" recovery glance.
pub async fn reflog(repo_path: &Path, limit: u32) -> Vec<ReflogEntry> {
    let output = run_git(
        repo_path,
        &["reflog", "--format=REFLOG|%h|%gs", "-n", &limit.to_string()],
    )
    .await;

    let output = match output {
        Some(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_reflog_output(&stdout)
}

fn parse_reflog_output(output: &str) -> Vec<ReflogEntry> {
    output
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("REFLOG|")?;
            let parts: Vec<&str> = rest.splitn(2, '|').collect();
            if parts.len() != 2 {
                return None;
            }
            let (action, description) = match parts[1].split_once(": ") {
                Some((a, d)) => (a.to_string(), d.to_string()),
                None => (String::new(), parts[1].to_string()),
            };
            Some(ReflogEntry {
                hash: parts[0].to_string(),
                action,
                description,
            })
        })
        .collect()
}

/// Read the given local branches' history (their union of reachable commits),
/// limited to `limit` commits, and lay out its graph column.
///
/// Deliberately *not* `--graph`. Git's own graph is a picture with filler rows
/// in it, and translating it to box drawing loses more than it keeps -- see
/// `crate::graph` for the argument. All that is needed from git is `%p`; the
/// topology falls out of that.
pub async fn graph(repo_path: &Path, branches: &[String], limit: u32) -> Vec<GraphLine> {
    if branches.is_empty() {
        return Vec::new();
    }

    let mut args = vec![
        "log".to_string(),
        "--color=never".to_string(),
        // `--decorate=full` with `%D` rather than `%d`: the full form spells
        // out refs/heads/, refs/remotes/ and refs/tags/, which is what lets
        // the pane colour a decoration by what it actually is instead of
        // guessing from the short name. `%D` drops the parens and the leading
        // space `%d` adds, so the entries split cleanly on ", ".
        "--decorate=full".to_string(),
        // %p, not %P: the abbreviated form, so a parent hash is spelled the
        // same here as the child's own %h is one row down. The layout matches
        // the two against each other, so a mismatch would leave every pipe
        // dangling and every commit in a lane of its own.
        "--format=%h|%p|%D|%aN|%s".to_string(),
        "-n".to_string(),
        limit.to_string(),
    ];
    args.extend(branches.iter().cloned());

    let output = run_git(
        repo_path,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )
    .await;

    let output = match output {
        Some(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_graph_output(&stdout)
}

/// Split one `%D` field into classified refs. Splitting on "," rather than
/// ", " because git omits neither, but a ref name cannot contain a comma
/// while `GitRef::parse` trims the space anyway.
fn parse_refs(field: &str) -> Vec<crate::model::GitRef> {
    field
        .split(',')
        .filter_map(crate::model::GitRef::parse)
        .collect()
}

/// `git log --graph` in its expanded form: one block per commit carrying
/// Merge/Author/Date and the full message, with the graph drawn down the
/// left of every line.
///
/// Returned as raw lines rather than parsed. The compact `graph` above is
/// parsed because its fields are rendered into columns; this one is git's own
/// layout and the value is in reproducing it, so the renderer colours the
/// lines and changes nothing else. A second call rather than one shared with
/// `graph`, because deriving a column layout from this format is more work
/// than asking git twice -- and both are gated on `WorkKey`, so an unchanged
/// repo pays for neither.
pub async fn graph_expanded(repo_path: &Path, branches: &[String], limit: u32) -> Vec<String> {
    if branches.is_empty() {
        return Vec::new();
    }
    let mut args = vec![
        "log".to_string(),
        "--graph".to_string(),
        "--color=never".to_string(),
        "--decorate".to_string(),
        "--abbrev-commit".to_string(),
        // Absolute, not `--date=relative`: git renders a relative date once,
        // at fetch time, and this output is only refetched when the repo's
        // WorkKey moves. An idle repo therefore sat on "3 minutes ago" all
        // night while the commit list beside it correctly said "16h ago".
        // The renderer turns this back into an age on every frame.
        "--date=iso-strict".to_string(),
        "-n".to_string(),
        limit.to_string(),
    ];
    args.extend(branches.iter().cloned());

    let output = run_git(
        repo_path,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )
    .await;
    match output {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|l| l.trim_end().to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// One row per commit, in the order git listed them, with the graph column
/// computed rather than parsed.
fn parse_graph_output(output: &str) -> Vec<GraphLine> {
    struct Row {
        hash: String,
        parents: Vec<String>,
        refs: Vec<crate::model::GitRef>,
        author: String,
        message: String,
    }

    let rows: Vec<Row> = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            // The subject stays last: it is the only field that can itself
            // contain a `|`, so it has to be whatever splitn leaves rather
            // than a field with a bound.
            let parts: Vec<&str> = line.splitn(5, '|').collect();
            Row {
                hash: parts.first().unwrap_or(&"").to_string(),
                parents: parts
                    .get(1)
                    .unwrap_or(&"")
                    .split_whitespace()
                    .map(str::to_string)
                    .collect(),
                refs: parse_refs(parts.get(2).unwrap_or(&"")),
                author: parts.get(3).unwrap_or(&"").to_string(),
                message: parts.get(4).unwrap_or(&"").to_string(),
            }
        })
        .collect();

    let commits: Vec<crate::graph::Commit<'_>> = rows
        .iter()
        .map(|r| crate::graph::Commit {
            hash: &r.hash,
            parents: &r.parents,
        })
        .collect();
    let mut lanes = crate::graph::layout(&commits);

    rows.into_iter()
        .zip(lanes.drain(..))
        .map(|(r, lanes)| GraphLine {
            lanes,
            is_merge: r.parents.len() > 1,
            hash: r.hash,
            refs: r.refs,
            author: r.author,
            message: r.message,
        })
        .collect()
}

/// Get the current branch name.
pub async fn current_branch(repo_path: &Path) -> Option<String> {
    let output = run_git(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// The repo's default branch: origin/HEAD if known, else main/master if they
/// exist locally. None when neither is resolvable (offline bare-ish repos).
pub async fn default_branch(repo_path: &Path) -> Option<String> {
    if let Some(o) = run_git(
        repo_path,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .await
        && o.status.success()
    {
        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if let Some(name) = s.strip_prefix("origin/") {
            return Some(name.to_string());
        }
    }
    for cand in ["main", "master"] {
        if let Some(o) = run_git(
            repo_path,
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{cand}"),
            ],
        )
        .await
            && o.status.success()
        {
            return Some(cand.to_string());
        }
    }
    None
}

/// Get list of branches that have been merged into the given base branch.
pub async fn merged_branches(repo_path: &Path, base_branch: &str) -> Vec<String> {
    let output = run_git(
        repo_path,
        &[
            "branch",
            "--merged",
            base_branch,
            "--format=%(refname:short)",
        ],
    )
    .await;

    match output {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.trim().to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// Get all local branch names.
pub async fn all_branches(repo_path: &Path) -> Vec<String> {
    let output = run_git(repo_path, &["branch", "--format=%(refname:short)"]).await;

    match output {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.trim().to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// Get all remote-tracking branch names (e.g. "origin/feature-x"), excluding
/// symbolic refs like `origin/HEAD`.
pub async fn remote_branches(repo_path: &Path) -> Vec<RemoteBranch> {
    let output = run_git(
        repo_path,
        &[
            "for-each-ref",
            // Both forms: the full refname to filter on, the short one to
            // show. Filtering on the short name alone missed the symref --
            // see `parse_remote_branch_output`.
            "--format=%(refname)\t%(refname:short)\t%(objectname:short)",
            "refs/remotes/",
        ],
    )
    .await;

    match output {
        Some(o) if o.status.success() => {
            parse_remote_branch_output(&String::from_utf8_lossy(&o.stdout))
        }
        _ => Vec::new(),
    }
}

/// Short names of the remote-tracking branches, dropping each remote's
/// default-branch symref.
///
/// The symref has to be matched on its FULL refname. `refs/remotes/origin/HEAD`
/// shortens to bare `origin`, because that resolves to it unambiguously, so a
/// `ends_with("/HEAD")` test against the short form never fires. The result
/// was a phantom branch called `origin` sitting in the branches list marked
/// "not fetched", when it is not a branch at all and points at `origin/main`,
/// which is fetched.
fn parse_remote_branch_output(stdout: &str) -> Vec<RemoteBranch> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let full = parts.next()?;
            let short = parts.next()?.trim();
            if full.ends_with("/HEAD") || short.is_empty() {
                return None;
            }
            Some(RemoteBranch {
                name: short.to_string(),
                tip: parts.next().unwrap_or_default().trim().to_string(),
            })
        })
        .collect()
}

/// Remote branches nothing local corresponds to -- never fetched/checked out
/// locally, or checked out once and deleted locally after merging. `remote`
/// entries are the "origin/name" short form.
///
/// A remote counts as fetched when a local branch either shares its short name
/// or names it as upstream. The upstream half matters because a local branch is
/// free to be called something other than the ref it tracks: with only the name
/// test, `feature/who-goes-first` tracking `origin/wip` left `origin/wip`
/// reported as never fetched, which is both false and the opposite of the
/// truth -- that branch is the reason the repo is behind.
pub fn unfetched_branches(remote: &[RemoteBranch], local: &[BranchStatus]) -> Vec<String> {
    remote
        .iter()
        .filter(|r| {
            let short = r.name.split_once('/').map_or(r.name.as_str(), |(_, n)| n);
            !local
                .iter()
                .any(|l| l.name == short || l.upstream.as_deref() == Some(r.name.as_str()))
        })
        .map(|r| r.name.clone())
        .collect()
}

/// Get the URL of the "origin" remote, if configured.
pub async fn remote_url(repo_path: &Path) -> Option<String> {
    let output = run_git(repo_path, &["remote", "get-url", "origin"]).await?;

    if output.status.success() {
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (!url.is_empty()).then_some(url)
    } else {
        None
    }
}

/// Switch to a branch (`git switch`). For a `refs/remotes` short name like
/// "origin/feature-x", switches to the local "feature-x" (git auto-creates a
/// tracking branch), so the same overlay can check out an unfetched remote
/// branch and a local one alike.
pub async fn switch_branch(repo_path: &Path, branch: &str) -> Result<(), String> {
    let local = branch
        .split_once('/')
        .filter(|(remote, _)| !remote.is_empty())
        .map_or(branch, |(_, name)| name);

    let output = run_git(repo_path, &["switch", local])
        .await
        .ok_or_else(|| "failed to switch branch: git did not run".to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Delete a local branch.
pub async fn delete_branch(repo_path: &Path, branch: &str) -> Result<(), String> {
    let output = run_git(repo_path, &["branch", "-d", branch])
        .await
        .ok_or_else(|| "failed to delete branch: git did not run".to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// List stash entries.
pub async fn stash_list(repo_path: &Path) -> Vec<String> {
    let output = run_git(repo_path, &["stash", "list", "--format=%s"]).await;

    match output {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// List active worktrees (excluding the main one).
pub async fn worktree_list(repo_path: &Path) -> Vec<crate::model::Worktree> {
    let output = run_git(repo_path, &["worktree", "list", "--porcelain"]).await;

    let output = match output {
        Some(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_worktree_output(&stdout, repo_path)
}

/// Whether two paths name the same directory, tolerating spellings that
/// differ without the target differing: case on a case-insensitive
/// filesystem, and symlinked parents.
///
/// The raw `==` this replaces let the MAIN worktree through whenever
/// discovery and git disagreed about spelling. On macOS a configured
/// `repo_dir` of `~/repos` resolves happily against a `~/Repos` on disk, so
/// discovery carries the lowercase form while `git worktree list` reports the
/// real casing -- and the repo root then rendered as though it were a linked
/// worktree of itself, duplicating the path already shown in the pane header
/// under a second spelling.
fn same_dir(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        // Unreadable either way: fall back to having already compared them.
        _ => false,
    }
}

/// A path with `$HOME` collapsed to `~`, separator included. Without it this
/// printed `~Repos/pinz`, which is not a path anything would accept.
fn tilde_path(path: &Path) -> String {
    let home = dirs::home_dir().unwrap_or_default();
    match path.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn parse_worktree_output(output: &str, main_path: &Path) -> Vec<crate::model::Worktree> {
    let mut worktrees = Vec::new();
    let mut current_path = String::new();
    let mut current_branch = String::new();

    // Only LINKED worktrees are worth listing. The main one is just the repo,
    // whose path the pane header already prints.
    let push_linked = |path: &str, branch: &str, out: &mut Vec<crate::model::Worktree>| {
        let wt_path = std::path::Path::new(path);
        if branch.is_empty() || same_dir(wt_path, main_path) {
            return;
        }
        out.push(crate::model::Worktree {
            path: tilde_path(wt_path),
            branch: branch.to_string(),
        });
    };

    for line in output.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            current_path = p.to_string();
            current_branch.clear();
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            current_branch = b.to_string();
        } else if line.is_empty() && !current_path.is_empty() {
            push_linked(&current_path, &current_branch, &mut worktrees);
            current_path.clear();
            current_branch.clear();
        }
    }

    // Last entry when the output has no trailing blank line.
    if !current_path.is_empty() {
        push_linked(&current_path, &current_branch, &mut worktrees);
    }

    worktrees
}

const PROTECTED_BRANCHES: &[&str] = &["main", "master", "develop", "dev"];

pub fn is_protected_branch(name: &str) -> bool {
    PROTECTED_BRANCHES.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RefKind;

    /// A local branch for the `unfetched_branches` tests. Only the name and
    /// the upstream take part in that decision.
    fn local_branch(name: &str, upstream: Option<&str>) -> BranchStatus {
        BranchStatus {
            name: name.to_string(),
            upstream: upstream.map(|u| u.to_string()),
            behind: 0,
            ahead: 0,
            is_head: false,
            tip: String::new(),
        }
    }

    /// The failure this closes: a branch row had no way to reach its own
    /// commit, so the branches view could not feed the inspector. The hash
    /// rides the `for-each-ref` that was already asking for `%(HEAD)` rather
    /// than costing a second subprocess.
    #[test]
    fn branch_output_carries_the_tip_hash() {
        let out = "*\tmain\torigin/main\t[ahead 2]\ta1b2c3d\n \tbeta\t\t\te4f5a6b\n";
        let parsed = parse_branch_output(out);
        assert_eq!(parsed[0].name, "main");
        assert_eq!(parsed[0].tip, "a1b2c3d");
        assert_eq!(parsed[1].name, "beta");
        assert_eq!(parsed[1].tip, "e4f5a6b");
    }

    /// A repo with no commits yields no object name. The field is empty rather
    /// than the row being dropped: the branch exists and must still be listed.
    #[test]
    fn a_branch_with_no_tip_parses_with_an_empty_hash() {
        let parsed = parse_branch_output("*\tmain\t\t\t\n");
        assert_eq!(parsed[0].name, "main");
        assert_eq!(parsed[0].tip, "");
    }

    /// Remote branches for the `unfetched_branches` tests. Only the name takes
    /// part in that decision; the tip rides along for the branches view.
    fn remotes(names: &[&str]) -> Vec<RemoteBranch> {
        names
            .iter()
            .map(|n| RemoteBranch {
                name: (*n).to_string(),
                tip: String::new(),
            })
            .collect()
    }

    /// The failure this closes: only the *unfetched* subset survived the
    /// sweep, so a branches view had no way to list the remotes that are
    /// fetched. The full list was already being computed and thrown away.
    #[test]
    fn remote_branch_output_carries_names_and_tips() {
        let out = "refs/remotes/origin/HEAD\torigin\tdeadbee\n\
                   refs/remotes/origin/main\torigin/main\ta1b2c3d\n\
                   refs/remotes/origin/wip\torigin/wip\te4f5a6b\n";
        let parsed = parse_remote_branch_output(out);
        assert_eq!(parsed.len(), 2, "the HEAD symref must still be dropped");
        assert_eq!(parsed[0].name, "origin/main");
        assert_eq!(parsed[0].tip, "a1b2c3d");
        assert_eq!(parsed[1].name, "origin/wip");
        assert_eq!(parsed[1].tip, "e4f5a6b");
    }

    #[test]
    fn unfetched_branches_skips_ones_with_local_counterpart() {
        let remote = remotes(&["origin/main", "origin/feature-x"]);
        let local = vec![local_branch("main", Some("origin/main"))];
        assert_eq!(
            unfetched_branches(&remote, &local),
            vec!["origin/feature-x"]
        );
    }

    #[test]
    fn unfetched_branches_empty_when_all_fetched() {
        let remote = remotes(&["origin/main"]);
        let local = vec![local_branch("main", Some("origin/main"))];
        assert!(unfetched_branches(&remote, &local).is_empty());
    }

    #[test]
    fn unfetched_branches_handles_nested_remote_names() {
        // A remote name with slashes in the branch itself (e.g. "origin/feat/x")
        // should still compare against the bare local branch "feat/x".
        let remote = remotes(&["origin/feat/x"]);
        let local = vec![local_branch("feat/x", None)];
        assert!(unfetched_branches(&remote, &local).is_empty());
    }

    #[test]
    fn unfetched_branches_counts_a_tracked_remote_as_fetched_under_any_name() {
        // The regression: a local branch may be named nothing like the ref it
        // tracks. `origin/wip` is fetched and tracked, so it is not unfetched,
        // even though no local branch is called "wip".
        let remote = remotes(&["origin/wip", "origin/stale"]);
        let local = vec![local_branch("feature/who-goes-first", Some("origin/wip"))];
        assert_eq!(unfetched_branches(&remote, &local), vec!["origin/stale"]);
    }

    #[test]
    fn unfetched_branches_matches_upstream_on_the_full_remote_name() {
        // The upstream test compares against the "origin/x" form, not the bare
        // branch, so a same-named branch on a *different* remote stays
        // unfetched rather than being covered by this one.
        let remote = remotes(&["upstream/wip"]);
        let local = vec![local_branch("mine", Some("origin/wip"))];
        assert_eq!(unfetched_branches(&remote, &local), vec!["upstream/wip"]);
    }

    #[test]
    fn parse_track_info_ahead_and_behind() {
        assert_eq!(parse_track_info("[ahead 2, behind 3]"), (2, 3));
    }

    #[test]
    fn parse_track_info_ahead_only() {
        assert_eq!(parse_track_info("[ahead 5]"), (5, 0));
    }

    #[test]
    fn parse_track_info_behind_only() {
        assert_eq!(parse_track_info("[behind 1]"), (0, 1));
    }

    #[test]
    fn parse_track_info_empty() {
        assert_eq!(parse_track_info(""), (0, 0));
    }

    #[test]
    fn parse_branch_output_basic() {
        // `%(HEAD)` leads each row: "*" on the checked-out branch, a single
        // space on the others. Taken from real `for-each-ref` output.
        let output = " \tmain\torigin/main\t[behind 3]\n*\tfeat/x\torigin/feat/x\t[ahead 1]\n";
        let branches = parse_branch_output(output);
        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].name, "main");
        assert_eq!(branches[0].behind, 3);
        assert_eq!(branches[0].ahead, 0);
        assert_eq!(branches[1].name, "feat/x");
        assert_eq!(branches[1].behind, 0);
        assert_eq!(branches[1].ahead, 1);
    }

    /// Which branch you are on. Nothing carried it before, so the detail pane
    /// drew two level branches identically and could not say which was HEAD.
    #[test]
    fn parse_branch_output_marks_the_checked_out_branch() {
        let output = " \tmain\torigin/main\t\n*\tfeat/x\torigin/feat/x\t\n";
        let branches = parse_branch_output(output);
        assert!(!branches[0].is_head, "main is not checked out");
        assert!(branches[1].is_head, "feat/x is");
        assert_eq!(
            branches.iter().filter(|b| b.is_head).count(),
            1,
            "exactly one branch can be HEAD"
        );
    }

    #[test]
    fn parse_branch_output_no_upstream() {
        let output = "*\tlocal-only\t\t\n";
        let branches = parse_branch_output(output);
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].name, "local-only");
        assert!(branches[0].upstream.is_none());
        assert!(branches[0].is_head);
    }

    /// Regression: `refs/remotes/origin/HEAD` shortens to bare `origin`, so
    /// filtering the short name for a "/HEAD" suffix never fired and the
    /// remote's default-branch symref rendered as a branch you had never
    /// fetched. It is not a branch, and it points at `origin/main`, which is
    /// fetched. Taken verbatim from a live repo.
    #[test]
    fn remote_branches_drop_the_default_branch_symref() {
        let stdout = "refs/remotes/origin/HEAD\torigin\n\
                      refs/remotes/origin/main\torigin/main\n";
        let remotes = parse_remote_branch_output(stdout);
        assert!(
            !remotes.iter().any(|r| r.name == "origin"),
            "the symref leaked: {remotes:?}"
        );
        let names: Vec<&str> = remotes.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["origin/main"]);
    }

    /// A branch legitimately called `.../HEAD-something` must survive: the
    /// filter matches the suffix, not the substring.
    #[test]
    fn a_branch_whose_name_merely_contains_head_survives() {
        let stdout = "refs/remotes/origin/HEADroom\torigin/HEADroom\n\
                      refs/remotes/origin/fix/HEAD-parsing\torigin/fix/HEAD-parsing\n";
        let remotes = parse_remote_branch_output(stdout);
        assert_eq!(remotes.len(), 2, "got {remotes:?}");
    }

    /// Real `git worktree list --porcelain` output, one entry per blank-line
    /// separated block. Built with `concat!` rather than a continued string
    /// literal: rustfmt bakes a continuation's indentation into the value,
    /// which silently stops every line matching its prefix.
    fn worktree_block(path: &str, sha: &str, branch: &str) -> String {
        format!("worktree {path}\nHEAD {sha}\nbranch refs/heads/{branch}\n")
    }

    /// `git worktree list` always reports the main worktree first. It is just
    /// the repo, whose path the pane header already prints, so listing it
    /// again under a branch duplicated that line in a second spelling.
    #[test]
    fn parse_worktree_output_skips_the_main_worktree() {
        let main = std::path::Path::new("/Users/me/repos/pinz");
        let output = worktree_block("/Users/me/repos/pinz", "16d99c6", "feat/header-version");
        assert!(parse_worktree_output(&output, main).is_empty());
    }

    #[test]
    fn parse_worktree_output_keeps_linked_worktrees() {
        let main = std::path::Path::new("/Users/me/repos/pinz");
        let output = format!(
            "{}\n{}",
            worktree_block("/Users/me/repos/pinz", "16d99c6", "main"),
            worktree_block("/Users/me/repos/pinz-hotfix", "abc1234", "hotfix"),
        );
        let wts = parse_worktree_output(&output, main);
        assert_eq!(wts.len(), 1, "only the linked one, got {wts:?}");
        assert_eq!(wts[0].branch, "hotfix");
        // Trailing entry with no blank line after it still lands.
        assert!(wts[0].path.ends_with("pinz-hotfix"));
    }

    /// Regression: `format!("~{}", ..)` dropped the separator and printed
    /// `~Repos/pinz`, which is not a path anything would accept.
    #[test]
    fn a_worktree_under_home_keeps_its_path_separator() {
        let home = dirs::home_dir().expect("home");
        let linked = home.join("work").join("side");
        let output = worktree_block(&linked.display().to_string(), "abc1234", "side");
        let wts = parse_worktree_output(&output, std::path::Path::new("/nowhere"));
        assert_eq!(wts[0].path, "~/work/side", "got {}", wts[0].path);
    }

    /// The main-worktree filter compares paths that reach the same directory
    /// by different spellings: git reports the real casing while a configured
    /// `repo_dir` may carry another, and on a case-insensitive filesystem
    /// both resolve. A raw `==` missed that and leaked the repo root.
    #[test]
    fn same_dir_sees_through_a_symlinked_spelling() {
        let base = std::env::temp_dir().join("fleetz_same_dir_test");
        let _ = std::fs::remove_dir_all(&base);
        let real = base.join("real");
        std::fs::create_dir_all(&real).expect("create");
        let link = base.join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        assert!(same_dir(&real, &real), "a path matches itself");
        #[cfg(unix)]
        assert!(same_dir(&link, &real), "a symlink reaches the same dir");
        assert!(
            !same_dir(&real, &base.join("other")),
            "different dirs stay different"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn parse_commit_output_without_stat() {
        // Records: RS h US author US epoch US subject US body
        let output = "\u{1e}a1b2c3d\u{1f}Alice\u{1f}1712000000\u{1f}feat: add feature\u{1f}\u{1e}e4f5g6h\u{1f}Bob\u{1f}1712080000\u{1f}fix: bug\u{1f}";
        let commits = parse_commit_output(output);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "a1b2c3d");
        assert_eq!(commits[0].author, "Alice");
        assert_eq!(
            commits[0].date,
            DateTime::from_timestamp(1712000000, 0),
            "%at is parsed into an absolute timestamp, not a frozen string"
        );
        assert_eq!(commits[0].message, "feat: add feature");
        assert_eq!(commits[0].body, "");
        assert_eq!(commits[0].insertions, 0);
    }

    #[test]
    fn parse_commit_output_survives_an_unparseable_date() {
        // A git that hands back something other than %at must cost the age,
        // not the whole commit row.
        let output = "\u{1e}a1b2c3d\u{1f}Alice\u{1f}not-a-timestamp\u{1f}feat: x\u{1f}";
        let commits = parse_commit_output(output);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, "a1b2c3d");
        assert_eq!(commits[0].date, None);
    }

    #[test]
    fn parse_commit_output_with_stat() {
        let output = "\u{1e}a1b2c3d\u{1f}Alice\u{1f}1712000000\u{1f}feat: add feature\u{1f} 3 files changed, 18 insertions(+), 4 deletions(-)\n\u{1e}e4f5g6h\u{1f}Bob\u{1f}1712080000\u{1f}fix: bug\u{1f} 1 file changed, 2 insertions(+)\n";
        let commits = parse_commit_output(output);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].insertions, 18);
        assert_eq!(commits[0].deletions, 4);
        assert_eq!(commits[1].insertions, 2);
        assert_eq!(commits[1].deletions, 0);
    }

    #[test]
    fn parse_commit_output_multiline_body_and_stat() {
        // A body with its own newlines, then a shortstat line -- the body is
        // preserved and the stat is pulled out, not treated as body text.
        let output = "\u{1e}a1b2c3d\u{1f}Alice\u{1f}1 day ago\u{1f}feat: thing\u{1f}Longer explanation.\n\nSecond paragraph.\n 2 files changed, 9 insertions(+)\n";
        let commits = parse_commit_output(output);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].message, "feat: thing");
        assert_eq!(commits[0].body, "Longer explanation.\n\nSecond paragraph.");
        assert_eq!(commits[0].insertions, 9);
    }

    #[test]
    fn parse_commit_output_collects_numstat_files() {
        // Body, then numstat lines, then the shortstat summary -- as emitted
        // by `git log --numstat --shortstat`. Files land in `files`, totals in
        // insertions/deletions, and neither leaks into the body.
        let output = "\u{1e}a1b2c3d\u{1f}Alice\u{1f}1712000000\u{1f}feat: add feature\u{1f}Body text.\n12\t3\tsrc/main.rs\n0\t7\tREADME.md\n-\t-\tlogo.png\n 3 files changed, 12 insertions(+), 10 deletions(-)\n";
        let commits = parse_commit_output(output);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].body, "Body text.");
        assert_eq!(commits[0].insertions, 12);
        assert_eq!(commits[0].deletions, 10);
        assert_eq!(commits[0].files.len(), 3);
        assert_eq!(
            commits[0].files[0],
            FileStat {
                path: "src/main.rs".into(),
                added: 12,
                removed: 3
            }
        );
        assert_eq!(
            commits[0].files[1],
            FileStat {
                path: "README.md".into(),
                added: 0,
                removed: 7
            }
        );
        // Binary file: numstat reports `-` for both counts, stored as 0/0.
        assert_eq!(
            commits[0].files[2],
            FileStat {
                path: "logo.png".into(),
                added: 0,
                removed: 0
            }
        );
    }

    #[test]
    fn parse_numstat_line_rejects_non_numstat() {
        // A body line that merely contains a tab is not a numstat line.
        assert!(parse_numstat_line("just some\tprose here").is_none());
        assert!(parse_numstat_line("no tabs at all").is_none());
        assert!(parse_numstat_line("12\t3").is_none()); // missing path
        assert_eq!(
            parse_numstat_line("5\t2\tsrc/app.rs"),
            Some(FileStat {
                path: "src/app.rs".into(),
                added: 5,
                removed: 2
            })
        );
    }

    #[test]
    fn extract_stat_number_works() {
        assert_eq!(
            extract_stat_number(
                " 3 files changed, 18 insertions(+), 4 deletions(-)",
                "insertion"
            ),
            18
        );
        assert_eq!(
            extract_stat_number(" 1 file changed, 2 insertions(+)", "deletion"),
            0
        );
    }

    #[test]
    fn parse_reflog_output_splits_action_from_description() {
        let output = "REFLOG|a1b2c3d|commit: add feature\nREFLOG|e4f5g6h|checkout: moving from main to feat\n";
        let entries = parse_reflog_output(output);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].hash, "a1b2c3d");
        assert_eq!(entries[0].action, "commit");
        assert_eq!(entries[0].description, "add feature");
        assert_eq!(entries[1].action, "checkout");
        assert_eq!(entries[1].description, "moving from main to feat");
    }

    #[test]
    fn parse_reflog_output_no_colon_keeps_whole_subject_as_description() {
        let output = "REFLOG|a1b2c3d|pull origin main\n";
        let entries = parse_reflog_output(output);
        assert_eq!(entries[0].action, "");
        assert_eq!(entries[0].description, "pull origin main");
    }

    #[test]
    fn parse_graph_output_commit_line() {
        // `%D` under `--decorate=full`: no parens, no leading space, and every
        // ref spelled out to its refs/ prefix.
        let output = "a1b2c3d|9999999|HEAD -> refs/heads/main, refs/remotes/origin/main|Alice|feat: add feature";
        let lines = parse_graph_output(output);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].hash, "a1b2c3d");
        assert_eq!(lines[0].author, "Alice");
        assert_eq!(lines[0].message, "feat: add feature");

        let refs = &lines[0].refs;
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].kind, RefKind::Local);
        assert_eq!(refs[0].name, "main");
        assert!(refs[0].is_head, "HEAD is attached to main");
        assert_eq!(refs[1].kind, RefKind::Remote);
        assert_eq!(refs[1].name, "origin/main");
        assert!(!refs[1].is_head);
    }

    /// The prefixes are what make the classification safe. A local branch
    /// literally named `origin/thing` is legal, and the short `%d` form draws
    /// it identically to the remote-tracking ref of the same name -- which is
    /// why this reads `--decorate=full` rather than guessing from the name.
    #[test]
    fn a_local_branch_named_like_a_remote_is_not_mistaken_for_one() {
        let output = "a1b2c3d|9999999|refs/heads/origin/thing|Alice|wip";
        let refs = &parse_graph_output(output)[0].refs;
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].kind, RefKind::Local);
        assert_eq!(refs[0].name, "origin/thing");
    }

    #[test]
    fn parse_graph_output_classifies_tags_and_a_detached_head() {
        // git keeps its "tag: " marker under --decorate=full; the ref is
        // spelled out behind it rather than in place of it.
        let output = "a1b2c3d|9999999|HEAD, tag: refs/tags/v1.0|Alice|release";
        let refs = &parse_graph_output(output)[0].refs;
        assert_eq!(refs.len(), 2);
        // Detached: HEAD stands alone rather than riding a branch, so it must
        // not pick up the arrow that would claim it points at the tag.
        assert_eq!(refs[0].kind, RefKind::Head);
        assert_eq!(refs[0].name, "HEAD");
        assert!(!refs[0].is_head);
        assert_eq!(refs[1].kind, RefKind::Tag);
        assert_eq!(refs[1].name, "v1.0");
    }

    /// The real shape of a tagged release commit, taken from a live repo
    /// rather than written from memory: HEAD attached to a branch, a tag, and
    /// two remote refs, all in one decoration list.
    #[test]
    fn parse_graph_output_handles_a_tagged_release_commit() {
        let output = "78cdfac|9999999 8888888|HEAD -> refs/heads/main, tag: refs/tags/v0.4.0, refs/remotes/origin/main, refs/remotes/origin/HEAD|AR|release";
        let refs = &parse_graph_output(output)[0].refs;
        let got: Vec<_> = refs.iter().map(|r| (r.kind, r.name.as_str())).collect();
        assert_eq!(
            got,
            vec![
                (RefKind::Local, "main"),
                (RefKind::Tag, "v0.4.0"),
                (RefKind::Remote, "origin/main"),
                (RefKind::Remote, "origin/HEAD"),
            ]
        );
        assert!(refs[0].is_head);
        // The regression: "tag: " survived the strip and the raw
        // "tag: refs/tags/v0.4.0" was drawn in the pane.
        assert!(
            !refs[1].name.contains("refs/"),
            "prefix leaked into the drawn name: {}",
            refs[1].name
        );
    }

    /// Git's `--graph` draws every commit as `*`, so a merge is only
    /// distinguishable by its parent count. The root commit has none, which
    /// must not be mistaken for a merge by counting anything other than the
    /// number of entries.
    #[test]
    fn a_merge_is_recognised_by_its_parent_count() {
        let cases = [
            ("9999999", false, "one parent, ordinary"),
            ("9999999 8888888", true, "two parents, a merge"),
            ("9999999 8888888 7777777", true, "an octopus merge"),
            ("", false, "the root commit has no parents"),
        ];
        for (parents, expected, why) in cases {
            let output = format!("a1b2c3d|{parents}||Alice|subject");
            let lines = parse_graph_output(&output);
            assert_eq!(lines[0].is_merge, expected, "{why}");
        }
    }

    #[test]
    fn a_commit_with_no_decorations_has_no_refs() {
        let output = "a1b2c3d|9999999||Alice|feat: add feature";
        assert!(parse_graph_output(output)[0].refs.is_empty());
    }

    /// The property that replaced the connector-row handling this parser used
    /// to need. `git log --graph` interleaves rows carrying no commit, which
    /// forced every consumer to check for an empty hash; dropping `--graph`
    /// means a row and a commit are now the same thing.
    #[test]
    fn every_row_carries_a_commit() {
        let output = "a1b2c3d|9999999||Alice|feat: add feature\n\nd4e5f6g|a1b2c3d||Bob|other branch commit\n";
        let lines = parse_graph_output(output);
        assert_eq!(lines.len(), 2, "the blank line is not a row");
        assert!(
            lines.iter().all(|l| !l.hash.is_empty()),
            "a row with no commit on it got through"
        );
        assert_eq!(lines[1].hash, "d4e5f6g");
        assert_eq!(lines[1].author, "Bob");
    }

    /// The graph column is computed here, not read out of git, so the parser
    /// owes each row its lanes. A child and its parent share a lane.
    #[test]
    fn parsing_lays_out_the_graph_column() {
        let output = "a1b2c3d|d4e5f6g||Alice|child\nd4e5f6g|7777777||Alice|parent";
        let lines = parse_graph_output(output);
        let drawn: Vec<String> = lines
            .iter()
            .map(|l| l.lanes.iter().map(|c| c.glyph).collect())
            .collect();
        assert_eq!(drawn, vec![crate::graph::COMMIT_NODE.to_string(); 2]);
    }

    fn start_day() -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str("2025-08-17", "%Y-%m-%d").unwrap()
    }

    #[test]
    fn parse_daily_accumulates_repeated_days() {
        let out = "2025-08-17\n2025-08-17\n2025-08-18\n";
        let counts = parse_daily(out, start_day());
        assert_eq!(counts.0[0], 2);
        assert_eq!(counts.0[1], 1);
        assert_eq!(counts.0[2], 0, "a day with no commits stays zero");
    }

    #[test]
    fn parse_daily_ignores_days_outside_the_grid() {
        let out = "2020-01-01\n2025-08-16\n2025-08-17\n";
        let counts = parse_daily(out, start_day());
        assert_eq!(counts.0.iter().map(|n| *n as u32).sum::<u32>(), 1);
    }

    #[test]
    fn parse_daily_survives_junk_lines() {
        // A git that printed a warning to stdout must not poison the year.
        let out = "warning: something\n\n2025-08-17\n";
        let counts = parse_daily(out, start_day());
        assert_eq!(counts.0[0], 1);
    }

    #[test]
    fn parse_daily_fills_the_last_slot() {
        let last = start_day() + chrono::Duration::days((crate::contributions::SLOTS - 1) as i64);
        let out = format!("{}\n", last.format("%Y-%m-%d"));
        let counts = parse_daily(&out, start_day());
        assert_eq!(counts.0[crate::contributions::SLOTS - 1], 1);
    }
}

#[cfg(test)]
mod real_repo_tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command as StdCommand;

    fn sh(dir: &Path, args: &[&str]) {
        let status = StdCommand::new(args[0])
            .args(&args[1..])
            .current_dir(dir)
            .status()
            .expect("spawn");
        assert!(status.success(), "command failed: {args:?}");
    }

    fn temp_repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fleetz_it_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        sh(&dir, &["git", "init", "-q", "-b", "main"]);
        sh(&dir, &["git", "config", "user.email", "t@example.com"]);
        sh(&dir, &["git", "config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        sh(&dir, &["git", "add", "."]);
        sh(&dir, &["git", "commit", "-q", "-m", "init"]);
        dir
    }

    /// The failure this closes: creating a tag moves no commit, no index and
    /// no stash, so `WorkKey` was identical across the sweep and fleetz skipped
    /// the log and graph probes entirely. The tag never reached the pane, which
    /// is exactly what the release flow produces: the `chore: release` commit
    /// lands first and the tag is created after it, with nothing else moving.
    #[tokio::test]
    async fn a_new_tag_changes_the_work_key() {
        let dir = temp_repo("tagkey");
        let before = work_key(&dir).await.expect("a key for a real repo");
        sh(&dir, &["git", "tag", "v9.9.9"]);
        let after = work_key(&dir).await.expect("a key after tagging");
        assert_ne!(
            before, after,
            "a new tag left the fingerprint unchanged, so the graph is skipped"
        );
    }

    /// The other half: the fingerprint must not churn on its own, or every
    /// sweep pays for all six probes in every repo and the gate is pointless.
    #[tokio::test]
    async fn an_idle_repo_keeps_the_same_work_key() {
        let dir = temp_repo("idlekey");
        let first = work_key(&dir).await.expect("a key for a real repo");
        let second = work_key(&dir).await.expect("a second key");
        assert_eq!(
            first, second,
            "the fingerprint moved with nothing to move it"
        );
    }

    /// Deleting a tag has to count too: the decoration has to come off the
    /// pane as readily as it went on.
    #[tokio::test]
    async fn deleting_a_tag_changes_the_work_key() {
        let dir = temp_repo("untagkey");
        sh(&dir, &["git", "tag", "v9.9.9"]);
        let before = work_key(&dir).await.expect("a key");
        sh(&dir, &["git", "tag", "-d", "v9.9.9"]);
        let after = work_key(&dir).await.expect("a key");
        assert_ne!(
            before, after,
            "a deleted tag left the fingerprint unchanged"
        );
    }

    /// A linked worktree of `main_dir`, checked out on a new branch.
    fn linked_worktree(main_dir: &Path, tag: &str) -> PathBuf {
        let wt = std::env::temp_dir().join(format!("fleetz_it_{tag}_wt_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&wt);
        let wt_arg = wt.to_string_lossy().into_owned();
        sh(
            main_dir,
            &["git", "worktree", "add", "-q", "-b", tag, &wt_arg],
        );
        wt
    }

    fn canonical(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).expect("path exists")
    }

    /// The parts of `WorkKey` read from the common dir.
    fn shared_fields(key: &WorkKey) -> (u64, Option<std::time::SystemTime>, u64, usize) {
        (
            key.stash_size,
            key.refs_mtime,
            key.packed_refs_size,
            key.loose_tag_count,
        )
    }

    #[tokio::test]
    async fn a_plain_clone_keeps_both_dirs_in_dot_git() {
        let dir = temp_repo("dirs_plain");
        let dot = dir.join(".git");

        let dirs = repo_dirs(&dir).expect("a plain clone resolves");
        assert_eq!(dirs.git_dir, dot);
        assert_eq!(dirs.common_dir, dot);
        assert_eq!(resolve_git_dir(&dir), Some(dot.clone()));

        assert_eq!(last_fetch_at(&dir), None, "a fresh repo never fetched");
        std::fs::write(dot.join("FETCH_HEAD"), "").unwrap();
        assert!(last_fetch_at(&dir).is_some());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// git writes a linked worktree's `commondir` as the relative `../..`,
    /// and its `FETCH_HEAD` beside its own `HEAD`, not in the main clone.
    #[tokio::test]
    async fn a_linked_worktree_shares_the_main_clones_common_dir() {
        let main_dir = temp_repo("dirs_linked");
        let wt = linked_worktree(&main_dir, "dirs_linked");

        let dirs = repo_dirs(&wt).expect("a linked worktree resolves");
        assert_eq!(Some(dirs.git_dir.clone()), resolve_git_dir(&wt));
        // git names the admin dir after the worktree's folder.
        let admin = main_dir
            .join(".git/worktrees")
            .join(wt.file_name().expect("named"));
        assert_eq!(canonical(&dirs.git_dir), canonical(&admin));
        assert_eq!(dirs.common_dir, dirs.git_dir.join("../.."));
        assert_eq!(
            canonical(&dirs.common_dir),
            canonical(&main_dir.join(".git"))
        );

        // The key reads refs and the stash log from the common dir.
        sh(&main_dir, &["git", "tag", "v1"]);
        let main_key = work_key(&main_dir).await.expect("main key");
        let wt_key = work_key(&wt).await.expect("worktree key");
        assert_eq!(shared_fields(&main_key), shared_fields(&wt_key));
        assert_eq!(wt_key.loose_tag_count, 1);

        // FETCH_HEAD is per worktree.
        std::fs::write(main_dir.join(".git/FETCH_HEAD"), "").unwrap();
        assert_eq!(last_fetch_at(&wt), None, "main clone's fetch is not ours");
        std::fs::write(dirs.git_dir.join("FETCH_HEAD"), "").unwrap();
        assert!(last_fetch_at(&wt).is_some());

        std::fs::remove_dir_all(&wt).ok();
        std::fs::remove_dir_all(&main_dir).ok();
    }

    /// An absolute `commondir` replaces the git dir outright, and the
    /// trailing newline git writes is not part of the path.
    #[tokio::test]
    async fn an_absolute_commondir_is_used_as_is() {
        let main_dir = temp_repo("dirs_abs");
        let wt = linked_worktree(&main_dir, "dirs_abs");
        let git_dir = resolve_git_dir(&wt).expect("worktree git dir");
        let common = canonical(&main_dir.join(".git"));
        std::fs::write(git_dir.join("commondir"), format!("{}\n", common.display())).unwrap();

        let dirs = repo_dirs(&wt).expect("resolves");
        assert_eq!(dirs.git_dir, git_dir);
        assert_eq!(dirs.common_dir, common);

        sh(&main_dir, &["git", "tag", "v1"]);
        let main_key = work_key(&main_dir).await.expect("main key");
        let wt_key = work_key(&wt).await.expect("worktree key");
        assert_eq!(shared_fields(&main_key), shared_fields(&wt_key));
        assert_eq!(wt_key.loose_tag_count, 1);

        std::fs::remove_dir_all(&wt).ok();
        std::fs::remove_dir_all(&main_dir).ok();
    }

    /// A hand-built layout: a relative `gitdir:` pointer and a relative
    /// `commondir` padded with whitespace. Both joins stay unnormalized.
    #[test]
    fn a_relative_commondir_is_trimmed_and_joined_onto_the_git_dir() {
        let root = std::env::temp_dir().join(format!("fleetz_it_dirs_rel_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let checkout = root.join("checkout");
        let store = root.join("store");
        std::fs::create_dir_all(&checkout).unwrap();
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(checkout.join(".git"), "gitdir: ../store\n").unwrap();
        std::fs::write(store.join("commondir"), "  ../common \n").unwrap();

        let git_dir = checkout.join("../store");
        let dirs = repo_dirs(&checkout).expect("resolves");
        assert_eq!(dirs.git_dir, git_dir);
        assert_eq!(dirs.common_dir, git_dir.join("../common"));
        assert_eq!(resolve_git_dir(&checkout), Some(git_dir));

        std::fs::remove_dir_all(&root).ok();
    }

    /// A submodule checkout (here `git init --separate-git-dir`, the same
    /// shape) has a `gitdir:` file and no `commondir`: one directory.
    #[tokio::test]
    async fn a_gitdir_file_without_commondir_has_one_dir() {
        let root = std::env::temp_dir().join(format!("fleetz_it_dirs_sub_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let checkout = root.join("sub");
        let store = root.join("modules").join("sub");
        std::fs::create_dir_all(&checkout).unwrap();
        std::fs::create_dir_all(root.join("modules")).unwrap();
        let store_arg = store.to_string_lossy().into_owned();
        sh(
            &checkout,
            &[
                "git",
                "init",
                "-q",
                "-b",
                "main",
                "--separate-git-dir",
                &store_arg,
            ],
        );
        sh(&checkout, &["git", "config", "user.email", "t@example.com"]);
        sh(&checkout, &["git", "config", "user.name", "t"]);
        std::fs::write(checkout.join("a.txt"), "one\n").unwrap();
        sh(&checkout, &["git", "add", "."]);
        sh(&checkout, &["git", "commit", "-q", "-m", "init"]);
        assert!(checkout.join(".git").is_file(), "a gitdir: pointer file");
        assert!(!store.join("commondir").exists());

        let dirs = repo_dirs(&checkout).expect("resolves");
        assert_eq!(Some(dirs.git_dir.clone()), resolve_git_dir(&checkout));
        assert_eq!(canonical(&dirs.git_dir), canonical(&store));
        assert_eq!(dirs.common_dir, dirs.git_dir);

        sh(&checkout, &["git", "tag", "v1"]);
        let key = work_key(&checkout).await.expect("a key");
        assert_eq!(key.loose_tag_count, 1, "refs read from the one dir");
        assert!(key.index_size > 0, "index read from the one dir");

        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn branch_statuses_and_dirty_count_on_real_repo() {
        let dir = temp_repo("basic");

        let branches = branch_statuses(&dir).await;
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].name, "main");
        assert_eq!(branches[0].upstream, None);

        assert_eq!(dirty_count(&dir).await, 0);
        std::fs::write(dir.join("b.txt"), "two\n").unwrap();
        assert_eq!(dirty_count(&dir).await, 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn merged_branches_resolve_against_default_not_current() {
        let dir = temp_repo("merged");

        // Land a feature on main, then park on an unrelated branch. Asking
        // about the current branch would miss the merge; the default branch
        // is the answer the overlay actually wants.
        sh(&dir, &["git", "switch", "-q", "-c", "feature"]);
        std::fs::write(dir.join("f.txt"), "feat\n").unwrap();
        sh(&dir, &["git", "add", "."]);
        sh(&dir, &["git", "commit", "-q", "-m", "feature work"]);
        sh(&dir, &["git", "switch", "-q", "main"]);
        sh(
            &dir,
            &["git", "merge", "-q", "--no-ff", "-m", "merge", "feature"],
        );
        sh(&dir, &["git", "switch", "-q", "-c", "elsewhere"]);

        let base = default_branch(&dir).await;
        assert_eq!(base.as_deref(), Some("main"), "no origin, so main fallback");

        let merged = merged_branches(&dir, "main").await;
        assert!(
            merged.iter().any(|b| b == "feature"),
            "feature merged into main, got {merged:?}"
        );

        let from_current = merged_branches(&dir, "elsewhere").await;
        assert!(
            from_current.iter().any(|b| b == "feature"),
            "sanity: elsewhere descends from the merge too"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Regression: a feature branch created locally and never pushed has an
    /// empty `upstream:track`, so fleetz reported ahead=0 and the repo
    /// vanished from "local changes" the moment the work was committed. The
    /// unpushed commits are the whole point of that section.
    #[tokio::test]
    async fn unpushed_branch_without_upstream_is_ahead() {
        let dir = temp_repo("noupstream");
        let bare = std::env::temp_dir().join(format!(
            "fleetz_it_noupstream_remote_{}.git",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&bare);
        std::fs::create_dir_all(&bare).unwrap();
        sh(&bare, &["git", "init", "-q", "--bare"]);
        sh(
            &dir,
            &["git", "remote", "add", "origin", &bare.to_string_lossy()],
        );
        sh(&dir, &["git", "push", "-q", "-u", "origin", "main"]);

        sh(&dir, &["git", "switch", "-q", "-c", "feat/x"]);
        std::fs::write(dir.join("b.txt"), "two\n").unwrap();
        sh(&dir, &["git", "add", "."]);
        sh(&dir, &["git", "commit", "-q", "-m", "work"]);

        let branches = branch_statuses(&dir).await;
        let feat = branches
            .iter()
            .find(|b| b.name == "feat/x")
            .expect("feat/x");
        assert_eq!(feat.upstream, None, "branch was never pushed");
        assert_eq!(feat.ahead, 1, "the unpushed commit must be counted");

        let main = branches.iter().find(|b| b.name == "main").expect("main");
        assert_eq!(main.ahead, 0, "main is level with its upstream");

        // Pushing it settles the count back to zero.
        sh(&dir, &["git", "push", "-q", "-u", "origin", "feat/x"]);
        let after = branch_statuses(&dir).await;
        let feat = after.iter().find(|b| b.name == "feat/x").expect("feat/x");
        assert_eq!(feat.ahead, 0, "pushed branch is not ahead");

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&bare).ok();
    }

    /// A repo with no remote at all has nowhere to push, so every commit is
    /// technically "unreachable from remotes". Reporting that would make every
    /// scratch repo shout an ahead count forever.
    #[tokio::test]
    async fn local_repo_without_remotes_is_never_ahead() {
        let dir = temp_repo("noremote");

        let branches = branch_statuses(&dir).await;
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].upstream, None);
        assert_eq!(
            branches[0].ahead, 0,
            "no remote means nothing to be ahead of"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The expanded graph must carry an absolute timestamp, not the relative
    /// one git renders once and freezes. The renderer turns it back into an
    /// age per frame; with `--date=relative` an idle repo showed "3 minutes
    /// ago" for as long as it stayed idle.
    #[tokio::test]
    async fn expanded_graph_dates_are_absolute_and_parseable() {
        let dir = temp_repo("expdate");

        let lines = graph_expanded(&dir, &["main".to_string()], 5).await;
        let date = lines
            .iter()
            .find_map(|l| l.split_once("Date:"))
            .map(|(_, rest)| rest.trim().to_string())
            .expect("the block has a Date row");
        assert!(
            chrono::DateTime::parse_from_rfc3339(&date).is_ok(),
            "not an absolute timestamp the renderer can re-age: {date:?}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The regression: the detail pane draws the graph, which is the union of
    /// the local branches, and then looks each row up in this list by hash for
    /// its churn and for the inspector's body. Reading HEAD alone here left a
    /// side-branch commit drawn but unfindable -- no `+N -M` on the row, and
    /// "outside the loaded commit range" in the pane below it.
    #[tokio::test]
    async fn commits_cover_every_branch_the_graph_draws() {
        let dir = temp_repo("branchunion");
        sh(&dir, &["git", "checkout", "-q", "-b", "side"]);
        std::fs::write(dir.join("b.txt"), "on the side\n").unwrap();
        sh(&dir, &["git", "add", "."]);
        sh(
            &dir,
            &["git", "commit", "-q", "-m", "only on the side branch"],
        );
        sh(&dir, &["git", "checkout", "-q", "main"]);

        let branches = vec!["main".to_string(), "side".to_string()];
        let commits = commits(&dir, "-20", true, None, &branches).await;
        let graph = graph(&dir, &branches, 20).await;

        assert!(
            commits
                .iter()
                .any(|c| c.message == "only on the side branch"),
            "the side branch commit is missing: {:?}",
            commits.iter().map(|c| &c.message).collect::<Vec<_>>()
        );
        // Every drawn row is findable, which is the property the pane needs.
        for row in &graph {
            assert!(
                commits.iter().any(|c| c.hash == row.hash),
                "graph draws {} but the commit list has no such hash",
                row.hash
            );
        }

        // HEAD alone does not, which is what makes the assertion above worth
        // making: the two lists really do differ on this repo.
        let head_only = super::commits(&dir, "-20", true, None, &[]).await;
        assert!(
            !head_only
                .iter()
                .any(|c| c.message == "only on the side branch"),
            "the side branch is not reachable from HEAD, so this is not a test"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn work_key_stable_then_changes_on_commit_and_stash() {
        let dir = temp_repo("workkey");

        let k1 = work_key(&dir).await.expect("key");
        let k2 = work_key(&dir).await.expect("key");
        assert_eq!(k1, k2, "key must be stable when nothing changed");

        std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
        sh(&dir, &["git", "add", "."]);
        sh(&dir, &["git", "commit", "-q", "-m", "second"]);
        let k3 = work_key(&dir).await.expect("key");
        assert_ne!(k1.head_sha, k3.head_sha, "commit must change the key");

        std::fs::write(dir.join("a.txt"), "stash me\n").unwrap();
        sh(&dir, &["git", "stash", "-q"]);
        let k4 = work_key(&dir).await.expect("key");
        assert_ne!(k3.stash_size, k4.stash_size, "stash must change the key");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Commit at a fixed old date so the assertion is exact rather than
    /// "somewhere near now".
    fn sh_at(dir: &Path, iso: &str, args: &[&str]) {
        let status = StdCommand::new(args[0])
            .args(&args[1..])
            .current_dir(dir)
            .env("GIT_AUTHOR_DATE", iso)
            .env("GIT_COMMITTER_DATE", iso)
            .status()
            .expect("spawn");
        assert!(status.success(), "command failed: {args:?}");
    }

    /// A bare `git init` with nothing in it, which is where the three repos
    /// that prompted this all started.
    fn empty_repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fleetz_lla_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        sh(&dir, &["git", "init", "-q", "-b", "main"]);
        sh(&dir, &["git", "config", "user.email", "t@example.com"]);
        sh(&dir, &["git", "config", "user.name", "t"]);
        dir
    }

    #[tokio::test]
    async fn last_local_activity_reports_the_last_commit() {
        let dir = empty_repo("commit");
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        sh(&dir, &["git", "add", "."]);
        sh_at(
            &dir,
            "2026-03-21T19:12:00+00:00",
            &["git", "commit", "-q", "-m", "init"],
        );

        let at = last_local_activity(&dir).await.expect("repo has a commit");
        assert_eq!(at.to_rfc3339(), "2026-03-21T19:12:00+00:00");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `~/Repos/Rust`, `bonsai` and `AoC-2025` are all `git init` with
    /// untracked files and zero commits, so `git log` reports nothing at all.
    /// Falling back to the dirty paths is the only way they get a timestamp,
    /// and without one the time window can never evict them.
    #[tokio::test]
    async fn a_repo_with_no_commits_falls_back_to_its_dirty_files() {
        let dir = empty_repo("nocommits");
        std::fs::write(dir.join("junk.txt"), "scratch\n").unwrap();

        let at = last_local_activity(&dir)
            .await
            .expect("untracked files are activity");
        let age = (chrono::Utc::now() - at).num_seconds();
        assert!(age < 60, "file written just now, got {at} ({age}s ago)");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_clean_repo_with_no_commits_has_no_local_activity() {
        let dir = empty_repo("clean");
        assert_eq!(last_local_activity(&dir).await, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The AoC-2025 shape: committed (or staged) long ago, touched since.
    /// Taking the commit alone would date the repo to March and evict work
    /// that is actually current.
    #[tokio::test]
    async fn a_dirty_file_newer_than_the_last_commit_wins() {
        let dir = empty_repo("newer");
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        sh(&dir, &["git", "add", "."]);
        sh_at(
            &dir,
            "2026-03-21T19:12:00+00:00",
            &["git", "commit", "-q", "-m", "init"],
        );
        std::fs::write(dir.join("b.txt"), "touched today\n").unwrap();

        let at = last_local_activity(&dir).await.expect("dirty file");
        let age = (chrono::Utc::now() - at).num_seconds();
        assert!(age < 60, "newest change is the untracked file, got {at}");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A clone of a local upstream, fetched once so `FETCH_HEAD` holds
    /// something. No network: the remote is a path on this disk. Returns the
    /// upstream and the clone.
    fn fetched_clone(tag: &str) -> (PathBuf, PathBuf) {
        let upstream = temp_repo(&format!("{tag}_up"));
        let dir = std::env::temp_dir().join(format!("fleetz_it_{tag}_down_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (from, to) = (
            upstream.to_string_lossy().into_owned(),
            dir.to_string_lossy().into_owned(),
        );
        sh(&std::env::temp_dir(), &["git", "clone", "-q", &from, &to]);
        sh(&dir, &["git", "fetch", "-q"]);
        (upstream, dir)
    }

    /// Point `origin` somewhere that is not a repository.
    fn break_origin(dir: &Path) {
        let missing = dir.join("no-such-remote").to_string_lossy().into_owned();
        sh(dir, &["git", "remote", "set-url", "origin", &missing]);
    }

    /// Pins the git behaviour rule 7 of `docs/fetch-outcome-contract.md`
    /// rests on: a failed fetch truncates `FETCH_HEAD` to nothing and moves
    /// its mtime, so the mtime alone counts a failure as a fetch.
    #[tokio::test]
    async fn a_failed_fetch_empties_fetch_head() {
        let (upstream, dir) = fetched_clone("emptyhead");
        let fetch_head = repo_dirs(&dir).unwrap().git_dir.join("FETCH_HEAD");
        assert!(std::fs::metadata(&fetch_head).unwrap().len() > 0);
        break_origin(&dir);
        fetch(&dir).await;
        assert_eq!(
            std::fs::metadata(&fetch_head).unwrap().len(),
            0,
            "git no longer truncates FETCH_HEAD on a failed fetch"
        );
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&upstream).ok();
    }

    /// Rule 1, the success half: a fetch that worked says so, and the disk
    /// baseline then has a time to give.
    #[tokio::test]
    async fn a_fetch_from_a_reachable_remote_succeeds() {
        let (upstream, dir) = fetched_clone("fetchok");
        sh(
            &upstream,
            &["git", "commit", "-q", "--allow-empty", "-m", "two"],
        );
        assert_eq!(fetch(&dir).await, FetchOutcome::Succeeded);
        assert!(last_successful_fetch_at(&dir).is_some());
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&upstream).ok();
    }

    /// Rule 1, the failure half, and rule 7 on a real clone: a remote that is
    /// not a repository fails with `NoRemote`, git's own line as the detail,
    /// and leaves no successful-fetch time on disk.
    #[tokio::test]
    async fn a_fetch_from_a_missing_remote_fails_as_no_remote() {
        let (upstream, dir) = fetched_clone("fetchgone");
        assert!(last_successful_fetch_at(&dir).is_some());
        break_origin(&dir);
        let FetchOutcome::Failed(failure) = fetch(&dir).await else {
            panic!("a fetch from a missing path did not fail");
        };
        assert_eq!(failure.kind, FetchFailureKind::NoRemote);
        let detail = failure.detail.expect("git said why");
        assert!(
            detail.contains("does not appear to be a git repository"),
            "{detail:?}"
        );
        assert_eq!(
            last_successful_fetch_at(&dir),
            None,
            "a failed fetch was read as a fetch"
        );
        // The raw mtime still moves, which is why it cannot be the baseline.
        assert!(last_fetch_at(&dir).is_some());
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&upstream).ok();
    }

    /// A remote that never answers is given up on as `TimedOut`, not lumped
    /// in with a failure to start. The upload-pack here waits on its input
    /// forever, so the limit is what ends it, deterministically.
    #[tokio::test]
    async fn a_fetch_that_never_answers_times_out() {
        let (upstream, dir) = fetched_clone("fetchhang");
        sh(
            &dir,
            &[
                "git",
                "config",
                "remote.origin.uploadpack",
                "cat >/dev/null; :",
            ],
        );
        let outcome = fetch_within(&dir, std::time::Duration::from_millis(300)).await;
        assert_eq!(
            outcome,
            FetchOutcome::Failed(FetchFailure {
                kind: FetchFailureKind::TimedOut,
                detail: None,
            })
        );
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&upstream).ok();
    }
}

#[cfg(test)]
mod fetch_outcome_tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    fn exited(code: i32, stderr: &str) -> FetchRun {
        FetchRun::Exited(std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        })
    }

    fn kind_of(stderr: &str) -> FetchFailureKind {
        match fetch_outcome(exited(128, stderr)) {
            FetchOutcome::Failed(f) => f.kind,
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_zero_exit_is_a_success() {
        assert_eq!(fetch_outcome(exited(0, "")), FetchOutcome::Succeeded);
    }

    /// `run_git` returns `None` for both; a fetch keeps them apart.
    #[test]
    fn a_timeout_and_a_failure_to_start_are_told_apart() {
        let FetchOutcome::Failed(timed_out) = fetch_outcome(FetchRun::TimedOut) else {
            panic!("a timeout is a failure");
        };
        let FetchOutcome::Failed(not_started) = fetch_outcome(FetchRun::NotStarted) else {
            panic!("a failure to start is a failure");
        };
        assert_eq!(timed_out.kind, FetchFailureKind::TimedOut);
        assert_eq!(not_started.kind, FetchFailureKind::Other);
        assert_ne!(timed_out, not_started);
    }

    /// Messages as git 2.39, curl and OpenSSH print them under `LC_ALL=C`.
    #[test]
    fn git_error_messages_map_to_their_kind() {
        use FetchFailureKind::*;
        for (stderr, want) in [
            (
                "fatal: Authentication failed for 'https://github.com/o/r.git/'",
                Auth,
            ),
            (
                "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
                Auth,
            ),
            (
                "git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.",
                Auth,
            ),
            (
                "Host key verification failed.\nfatal: Could not read from remote repository.",
                Auth,
            ),
            (
                "fatal: unable to access 'https://nohost.invalid/o/r.git/': Could not resolve host: nohost.invalid",
                Unreachable,
            ),
            (
                "ssh: Could not resolve hostname nohost: nodename nor servname provided, or not known",
                Unreachable,
            ),
            (
                "ssh: connect to host h port 22: Connection refused",
                Unreachable,
            ),
            (
                "fatal: '/x' does not appear to be a git repository\nfatal: Could not read from remote repository.",
                NoRemote,
            ),
            (
                "ERROR: Repository not found.\nfatal: Could not read from remote repository.",
                NoRemote,
            ),
            // git 2.39.3 on an HTTP 404 (a removed or renamed remote).
            (
                "fatal: repository 'http://127.0.0.1:18765/nope.git/' not found",
                NoRemote,
            ),
            (
                "fatal: refusing to fetch into branch 'refs/heads/main'",
                Other,
            ),
            ("", Other),
        ] {
            assert_eq!(kind_of(stderr), want, "{stderr:?}");
        }
    }

    #[test]
    fn the_detail_is_the_first_error_line_only() {
        let FetchOutcome::Failed(failure) = fetch_outcome(exited(
            128,
            "warning: something first\nfatal: '/x' does not appear to be a git repository\nfatal: Could not read from remote repository.\n",
        )) else {
            panic!("a failure");
        };
        assert_eq!(
            failure.detail.as_deref(),
            Some("fatal: '/x' does not appear to be a git repository")
        );
    }

    /// Rule 8: whatever git prints, a credential in a URL never reaches the
    /// detail, and nothing in it can drive the terminal.
    #[test]
    fn the_detail_carries_no_userinfo_or_control_characters() {
        let FetchOutcome::Failed(failure) = fetch_outcome(exited(
            128,
            "fatal: unable to access 'https://user:secret@host.invalid/o/r.git/': \u{1b}[31mCould not resolve host",
        )) else {
            panic!("a failure");
        };
        let detail = failure.detail.unwrap();
        assert!(
            !detail.contains("secret") && !detail.contains("user"),
            "{detail}"
        );
        assert!(!detail.contains('\u{1b}'), "{detail:?}");
        assert!(detail.contains("https://host.invalid/o/r.git/"), "{detail}");
    }

    /// The detail is built inside the spawned fetch task: a non-ASCII
    /// character before `scheme://` in git's line must not panic there.
    #[test]
    fn a_detail_with_a_multibyte_character_before_the_url_does_not_panic() {
        let FetchOutcome::Failed(failure) = fetch_outcome(exited(
            128,
            "fatal: unable to access \u{2018}https://u:secret@host.invalid/o/r.git/\u{2019}: Could not resolve host",
        )) else {
            panic!("a failure");
        };
        let detail = failure.detail.unwrap();
        assert!(!detail.contains("secret"), "{detail}");
        assert!(
            detail.contains("\u{2018}https://host.invalid/o/r.git/"),
            "{detail}"
        );
    }

    #[test]
    fn a_long_error_line_is_capped() {
        let FetchOutcome::Failed(failure) =
            fetch_outcome(exited(1, &format!("fatal: {}", "x".repeat(500))))
        else {
            panic!("a failure");
        };
        assert_eq!(failure.detail.unwrap().chars().count(), FETCH_DETAIL_MAX);
    }

    #[test]
    fn userinfo_is_removed_from_every_url_form_that_can_carry_it() {
        for (url, want) in [
            (
                "https://user:secret@github.com/Gaurgle/fleetz.git",
                "https://github.com/Gaurgle/fleetz.git",
            ),
            (
                "https://ghp_token@github.com/o/r.git",
                "https://github.com/o/r.git",
            ),
            // An ssh user is a login name, not a credential: only a
            // password goes.
            (
                "ssh://git@github.com:22/o/r.git",
                "ssh://git@github.com:22/o/r.git",
            ),
            ("ssh://u:pw@host/x", "ssh://u@host/x"),
            ("HTTPS://tok@host/x", "HTTPS://host/x"),
            ("git+ssh://u:pw@host/x", "git+ssh://u@host/x"),
            // Every scheme outside the ssh family loses all userinfo.
            ("ftp://tok@host/x", "ftp://host/x"),
            ("persistent-https://tok@host/x", "persistent-https://host/x"),
            // `,` `)` and `'` are legal in userinfo: they must not end it.
            ("https://user:abc,def@host/r.git", "https://host/r.git"),
            ("https://user:p)ss@host/r.git", "https://host/r.git"),
            ("https://user:it's@host/r.git", "https://host/r.git"),
            ("https://u:a:b:c@host/x", "https://host/x"),
            ("https://u:p@[::1]:8080/x", "https://[::1]:8080/x"),
            // An `@` after the authority is not userinfo.
            (
                "https://host/o/r@v?x=a@b#c@d",
                "https://host/o/r@v?x=a@b#c@d",
            ),
            ("(https://u:p@a), next", "(https://a), next"),
            // Multi-byte characters right before the scheme must not land a
            // slice inside them: the scan used to panic on all three.
            (
                "\u{2018}https://u:p@h/x\u{2019}",
                "\u{2018}https://h/x\u{2019}",
            ),
            ("日本https://u:p@h/x", "日本https://h/x"),
            // `é` is not a scheme character, so the scheme is empty and,
            // being outside the ssh family, all userinfo goes.
            ("é://u:p@h/x", "é://h/x"),
            (
                "https://u:p@a?x=1 and https://v@b#frag",
                "https://a?x=1 and https://b#frag",
            ),
            (
                "https://github.com/Gaurgle/fleetz.git",
                "https://github.com/Gaurgle/fleetz.git",
            ),
            // scp-style has no `://` and no password to leak.
            (
                "git@github.com:Gaurgle/fleetz.git",
                "git@github.com:Gaurgle/fleetz.git",
            ),
            (
                "a 'https://u:p@a/x' and https://v@b/y",
                "a 'https://a/x' and https://b/y",
            ),
        ] {
            assert_eq!(redact_userinfo(url), want, "{url}");
        }
    }
}
