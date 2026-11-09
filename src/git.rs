use crate::model::{BranchStatus, Commit, FileStat, GraphLine, ReflogEntry, RemoteBranch};
use chrono::DateTime;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Hard ceiling per git invocation so one repo on a dead network mount or a
/// credential prompt can never wedge a sweep. kill_on_drop reaps the child.
const GIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// How long `git fetch` may go without a progress update before it is
/// killed (FZ-35a, #84). An inactivity limit, not a wall clock: the clock
/// restarts on every update, so a large download that is still moving is
/// never cut and its bytes are never wasted (a pack cannot be resumed),
/// while one that has gone quiet is cut sooner than FZ-34's 120 s ceiling
/// cut it. Under `SWEEP_STUCK_AFTER` (180 s in `main.rs`), which measures a
/// fetch's claim from its last update too. An unreachable ssh host still
/// fails in 10 s (`FETCH_SSH_COMMAND`).
pub(crate) const FETCH_STALL_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

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

/// The text of `path` as commit `sha` has it (`git show <sha>:<path>`),
/// bounded like every git call: the inspector's `script` body reads a run's
/// workflow file here before asking GitHub (FZ-30f). `None` when the clone
/// lacks the commit or the file, or `sha` is not a hex object name.
pub async fn show_file(repo_path: &Path, sha: &str, path: &str) -> Option<String> {
    if sha.is_empty() || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let spec = format!("{sha}:{path}");
    let output = run_git(repo_path, &["show", &spec]).await?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Per-day commit counts for one author over the contribution grid.
///
/// `--all` rather than the default branch: work on a feature branch is still
/// work, and git de-duplicates a commit reachable from several refs. `%ad`
/// with `--date=short` is the author date in local time, which is the day you
/// would say you did the work on.
///
/// Minus `refs/stash` and `refs/notes/*`, whose commits are not work: a
/// stash is a WIP and an index commit authored by you, and a note is
/// metadata about a commit (#43). Excluded rather than narrowed to
/// `--branches --remotes`, so everything else `--all` walks still counts:
/// tags, HEAD when detached, and linked worktrees' HEADs.
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
            // Each `--exclude` applies to the `--all` after it, and matches
            // full refnames by glob, where `*` also crosses `/`.
            "--exclude=refs/stash",
            "--exclude=refs/notes/*",
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

/// A cheap fingerprint of what the expensive probes (log, reflog, graph,
/// stash) read. A sweep that finds it equal to the last one reuses their
/// output instead of running them. The worktree list is not gated by it:
/// worktrees are not refs, so `linked_worktrees` reads it on every sweep.
///
/// Refs are keyed by content, never by file metadata: every ref's full
/// object id, as `for-each-ref` reports it, goes into `ref_signature`. File
/// times and sizes missed nested tags, remote-tracking refs and other
/// branches, and moved on `pack-refs` and `gc`, which change no ref.
///
/// Not seen: anything that changes a probe's answer without moving a ref,
/// HEAD, the index, the shallow boundary or a reflog size, such as config
/// that alters `git log` output. Working-tree state is never gated by this
/// key: `dirty_count` runs on every sweep.
///
/// In memory only; never persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkKey {
    /// Output of rev-parse HEAD: one cheap subprocess buys exact commit identity.
    pub head_sha: String,
    pub index_mtime: Option<std::time::SystemTime>,
    pub index_size: u64,
    /// Size of the stash reflog (common dir aware); 0 when absent.
    pub stash_size: u64,
    /// Size of this checkout's HEAD reflog (`<git_dir>/logs/HEAD`); 0 when
    /// absent. Catches `reflog expire`, which moves no ref.
    pub head_log_size: u64,
    /// The full refname of the branch HEAD names, `None` when detached. Per
    /// checkout: a linked worktree and its main clone differ here.
    pub head_ref: Option<String>,
    /// Hash of every ref's (full refname, full object id) pair (local
    /// branches, remote-tracking refs, tags at any depth, `refs/stash`,
    /// notes, replace refs and any other namespace), plus the bytes of
    /// `<common>/shallow` when it exists. Shared by every checkout of one
    /// clone. Nothing derived (no ahead or behind, no upstream, no short
    /// names) goes in.
    pub ref_signature: u64,
}

/// One ref as `for-each-ref` lists it: the full refname and the full object
/// id. The pair the key hashes.
pub type RefId = (String, String);

/// A cheap-tier ref query that did not answer: the process failed to run,
/// timed out, or exited non-zero. Distinct from "no refs", which is an empty
/// list, so the key can refuse to compare instead of matching another empty
/// answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefQueryFailed;

/// The local-branch query: the branch statuses the panes show, and the
/// `refs/heads/` content and HEAD symref the key hashes.
#[derive(Debug, Clone, Default)]
pub struct LocalBranches {
    pub statuses: Vec<BranchStatus>,
    /// Every `refs/heads/` ref with its full object id.
    pub refs: Vec<RefId>,
    /// The full refname HEAD names, `None` when detached.
    pub head: Option<String>,
}

/// The widened ref query: remote-tracking branches for the panes, and every
/// ref outside `refs/heads/` for the key.
#[derive(Debug, Clone, Default)]
pub struct OtherRefs {
    pub remotes: Vec<RemoteBranch>,
    /// Every ref outside `refs/heads/`, with its full object id.
    pub refs: Vec<RefId>,
}

/// Hash the ref content and the shallow file into `WorkKey::ref_signature`.
/// `DefaultHasher::new()` uses fixed keys, so equal input hashes equal for
/// the life of the process, which is as long as a key lives.
fn ref_signature(local: &[RefId], other: &[RefId], shallow: Option<&[u8]>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    local.hash(&mut hasher);
    other.hash(&mut hasher);
    shallow.hash(&mut hasher);
    hasher.finish()
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

/// The key for a checkout, running both cheap-tier ref queries itself. The
/// sweep already has their output and calls `work_key_from` instead.
#[cfg(test)]
pub async fn work_key(repo_path: &Path) -> Option<WorkKey> {
    let local = try_branch_statuses(repo_path).await;
    let other = try_ref_listing(repo_path).await;
    work_key_from(repo_path, local.as_ref().ok(), other.as_ref().ok()).await
}

/// Cheap change detector over the cheap tier's ref queries: when none of
/// HEAD's sha, the HEAD symref, any ref's content, the shallow boundary, the
/// index or the HEAD and stash reflog sizes moved, the expensive probes (log,
/// reflog, graph, stash) are reused. See `WorkKey` for what this
/// cannot see. Untracked files do not touch the index, so dirty_count is
/// always probed regardless.
///
/// `None` when a ref query failed (pass `None` for it), HEAD is unborn or
/// unreadable, or the git dir cannot be resolved. The sweep treats `None` as
/// changed, so two failures never compare as unchanged.
pub async fn work_key_from(
    repo_path: &Path,
    local: Option<&LocalBranches>,
    other: Option<&OtherRefs>,
) -> Option<WorkKey> {
    let (local, other) = (local?, other?);
    let head = run_git(repo_path, &["rev-parse", "HEAD"]).await?;
    if !head.status.success() {
        return None; // unborn HEAD etc: treat as always-changed
    }
    let head_sha = String::from_utf8_lossy(&head.stdout).trim().to_string();

    // Per checkout: index and HEAD reflog. Shared with the main clone, in
    // the common dir: the stash log and the shallow file.
    let RepoDirs {
        git_dir,
        common_dir,
    } = repo_dirs(repo_path)?;
    let index_meta = std::fs::metadata(git_dir.join("index")).ok();
    let file_size = |p: std::path::PathBuf| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let shallow = std::fs::read(common_dir.join("shallow")).ok();

    Some(WorkKey {
        head_sha,
        index_mtime: index_meta.as_ref().and_then(|m| m.modified().ok()),
        index_size: index_meta.map(|m| m.len()).unwrap_or(0),
        stash_size: file_size(common_dir.join("logs/refs/stash")),
        head_log_size: file_size(git_dir.join("logs/HEAD")),
        head_ref: local.head.clone(),
        ref_signature: ref_signature(&local.refs, &other.refs, shallow.as_deref()),
    })
}

/// Why a `git fetch` failed, grouped by what you would do about it. Read from
/// git's stderr under the C locale `git_cmd` sets (`classify_fetch_error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchFailureKind {
    /// No progress for `FETCH_STALL_LIMIT`.
    TimedOut,
    /// Credentials wanted or refused. Prompts are disabled, so a remote that
    /// asks for a password fails here instead of hanging.
    Auth,
    /// ssh refused the key (`Permission denied (publickey)`): the fix is the
    /// key or the agent, not a password. fleetz fetches with `BatchMode=yes`,
    /// so a passphrase key that is not in the agent lands here.
    Ssh,
    /// The network, DNS or the host itself.
    Unreachable,
    /// The remote is missing or is not a repository.
    NoRemote,
    /// Anything else, including git failing to start.
    Other,
    /// Received more than the automatic-fetch cap (`fetch_max_mb`, FZ-35b)
    /// and was cut. Not an error: the repo is held until `f` fetches it
    /// uncapped or a fetch succeeds. The detail is the bytes received
    /// (`FetchFailure::received_bytes`).
    TooBig,
}

/// A failed fetch: its kind, and at most git's first error line with any URL
/// userinfo removed (rule 8). Never the raw stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchFailure {
    pub kind: FetchFailureKind,
    pub detail: Option<String>,
}

/// The detail of a `TooBig` failure: `<bytes> bytes received`.
const TOO_BIG_SUFFIX: &str = " bytes received";

impl FetchFailure {
    /// A fetch cut at the cap after receiving `bytes` (FZ-35b).
    pub fn too_big(bytes: u64) -> Self {
        Self {
            kind: FetchFailureKind::TooBig,
            detail: Some(format!("{bytes}{TOO_BIG_SUFFIX}")),
        }
    }

    /// What a `TooBig` fetch had received when it was cut; `None` for any
    /// other kind.
    pub fn received_bytes(&self) -> Option<u64> {
        if self.kind != FetchFailureKind::TooBig {
            return None;
        }
        self.detail
            .as_deref()?
            .strip_suffix(TOO_BIG_SUFFIX)?
            .parse()
            .ok()
    }
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
    /// Cut at the cap after receiving this many bytes.
    TooBig(u64),
    NotStarted,
    Exited(std::process::Output),
}

/// `--progress` rather than `--quiet` (FZ-35a): git reports progress only to
/// a terminal unless asked, and `--quiet` turns off `index-pack`'s
/// `Receiving objects` even with `--progress` (git 2.50, run). What `--quiet`
/// used to keep off stderr is dropped from the kept text instead
/// (`quiet_would_hide`).
const FETCH_ARGS: &[&str] = &["fetch", "--progress", "--prune"];

/// The longest git error line kept as a failure's detail.
const FETCH_DETAIL_MAX: usize = 160;

/// Which part of a fetch a progress update is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchPhase {
    /// The server building the pack (`remote: Counting objects`,
    /// `remote: Compressing objects`, ...): no bytes yet.
    Remote,
    /// The pack arriving: `Receiving objects`, or `Unpacking objects` for a
    /// fetch small enough to be unpacked loose.
    Receiving,
    /// `Resolving deltas` after the pack has arrived.
    Resolving,
}

/// One progress update of a running fetch, read from git's stderr.
/// `objects_total` is 0 when git gave only a count (`Enumerating objects:
/// 1234`). `bytes` is what has been received so far: git prints it on the
/// `Receiving objects` lines only, so later updates carry the last value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchProgress {
    pub phase: FetchPhase,
    pub objects_done: u64,
    pub objects_total: u64,
    pub bytes: u64,
}

/// One `\r`- or `\n`-separated piece of git's stderr read as a progress
/// update, or `None` when it is anything else. The shapes, as git prints them
/// under `LC_ALL=C`, with padding and an optional `, done.`:
///
/// - `Receiving objects:  45% (123/273), 1.23 MiB | 1.20 MiB/s`
/// - `Resolving deltas:  12% (3/25)`
/// - `remote: Compressing objects: 100% (302/302)`, `remote: Enumerating
///   objects: 302`
///
/// Sizes are in `bytes`, `KiB`, `MiB` or `GiB` (also `B`). A local title git
/// prints for no fetch phase is not progress, and neither is a line whose
/// title does not start upper case (`remote: fatal: ...`), so an error line
/// is never swallowed.
pub fn parse_fetch_progress(segment: &str) -> Option<FetchProgress> {
    let line = segment.trim();
    let (remote, line) = match line.strip_prefix("remote:") {
        Some(rest) => (true, rest.trim_start()),
        None => (false, line),
    };
    let (title, rest) = line.split_once(": ")?;
    if !title.starts_with(|c: char| c.is_ascii_uppercase())
        || !title.chars().all(|c| c.is_ascii_alphabetic() || c == ' ')
    {
        return None;
    }
    let phase = match (remote, title) {
        (true, _) => FetchPhase::Remote,
        (false, "Receiving objects" | "Unpacking objects") => FetchPhase::Receiving,
        (false, "Resolving deltas") => FetchPhase::Resolving,
        (false, _) => return None,
    };
    let rest = rest.trim_start();
    let rest = rest.strip_suffix(", done.").unwrap_or(rest).trim_end();
    let (objects_done, objects_total, bytes) = match rest.split_once('%') {
        Some((percent, after)) => {
            percent.trim().parse::<u32>().ok()?;
            let counts = after.trim_start().strip_prefix('(')?;
            let (counts, tail) = counts.split_once(')')?;
            let (done, total) = counts.split_once('/')?;
            // `, 1.23 MiB | 1.20 MiB/s` on a receiving line; anything else
            // after the counts (`, completed with 76 local objects.`) has
            // no size.
            let bytes = tail
                .trim()
                .strip_prefix(',')
                .and_then(|sized| sized.split('|').next())
                .and_then(|size| parse_size(size.trim()))
                .unwrap_or(0);
            (done.parse().ok()?, total.parse().ok()?, bytes)
        }
        None => (rest.parse().ok()?, 0, 0),
    };
    Some(FetchProgress {
        phase,
        objects_done,
        objects_total,
        bytes,
    })
}

/// `596.79 KiB` as bytes. git's units are binary (`strbuf_humanise_bytes`).
fn parse_size(text: &str) -> Option<u64> {
    let (number, unit) = text.split_once(' ')?;
    let scale: f64 = match unit {
        "B" | "byte" | "bytes" => 1.0,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    let number: f64 = number.parse().ok()?;
    (number >= 0.0).then(|| (number * scale).round() as u64)
}

/// Whether `--quiet`, which the fetch used to run with, would have kept this
/// stderr piece off the screen: a progress update, the server's `Total`
/// summary, and the `From <url>` header and ref update lines git prints for
/// a fetch that is not quiet. `classify_fetch_error` and `first_error_line`
/// read what is left, so they see what they saw before FZ-35a.
fn quiet_would_hide(segment: &str) -> bool {
    let ref_update = |s: &str| {
        let b = s.as_bytes();
        b.len() > 3 && b[0] == b' ' && b" +-t*!=".contains(&b[1]) && b[2] == b' '
    };
    parse_fetch_progress(segment).is_some()
        || segment.trim_start().starts_with("remote: Total ")
        || segment.starts_with("From ")
        || ref_update(segment)
}

/// git's stderr as it streams in: split on `\r` and `\n`, progress updates
/// handed out as they complete, everything else kept for classification.
#[derive(Debug, Default)]
struct FetchStderr {
    partial: Vec<u8>,
    kept: Vec<u8>,
    bytes: u64,
}

impl FetchStderr {
    /// Take in a chunk; returns the progress updates it completed.
    fn feed(&mut self, chunk: &[u8]) -> Vec<FetchProgress> {
        let mut updates = Vec::new();
        for &b in chunk {
            if b == b'\r' || b == b'\n' {
                updates.extend(self.end_segment());
            } else {
                self.partial.push(b);
            }
        }
        updates
    }

    /// The pipe closed: the last piece, if it had no line end.
    fn finish(&mut self) -> Option<FetchProgress> {
        self.end_segment()
    }

    fn end_segment(&mut self) -> Option<FetchProgress> {
        let segment = std::mem::take(&mut self.partial);
        let text = String::from_utf8_lossy(&segment);
        if let Some(mut progress) = parse_fetch_progress(&text) {
            self.bytes = self.bytes.max(progress.bytes);
            progress.bytes = self.bytes;
            return Some(progress);
        }
        if !text.trim().is_empty() && !quiet_would_hide(&text) {
            self.kept.extend_from_slice(&segment);
            self.kept.push(b'\n');
        }
        None
    }
}

/// Run `git fetch` on a repo. Prunes deleted remote branches so
/// remote_only_branches does not accumulate stale entries.
///
/// Its own path rather than `run_git`, which every other caller keeps: a
/// fetch is the one probe whose failure the user has to be told about, and
/// `run_git` cannot say why one failed.
#[cfg(test)]
pub async fn fetch(repo_path: &Path) -> FetchOutcome {
    fetch_reporting(repo_path, None, |_| {}).await
}

/// `fetch`, handing each progress update to `on_progress` as git prints it,
/// under `FETCH_STALL_LIMIT`, and cut as `TooBig` once more than `max_bytes`
/// have arrived (FZ-35b). `None` is no cap: what `f` asks for.
pub async fn fetch_reporting(
    repo_path: &Path,
    max_bytes: Option<u64>,
    on_progress: impl FnMut(FetchProgress),
) -> FetchOutcome {
    fetch_capped(repo_path, FETCH_STALL_LIMIT, max_bytes, on_progress).await
}

/// The ssh command a fetch hands git when the user chose none. ssh's own TCP
/// connect wait outlasts `GIT_TIMEOUT` (75 s measured on macOS), so an
/// unreachable host ended as `TimedOut` after the whole limit; with a 10 s
/// connect timeout it fails sooner, with ssh's reason. `BatchMode` stops ssh
/// prompting on the TUI's terminal (a passphrase, an unknown host key), which
/// `GIT_TERMINAL_PROMPT=0` does not cover. A slow but reachable remote is
/// held only to `FETCH_STALL_LIMIT`.
const FETCH_SSH_COMMAND: &str = "ssh -o ConnectTimeout=10 -o BatchMode=yes";

/// Whether a fetch adds `FETCH_SSH_COMMAND`: only when the user has set none
/// of `GIT_SSH_COMMAND`, `GIT_SSH` or `core.sshCommand`, so their own ssh
/// setup always wins. An empty string counts as set, as it does for git,
/// which tries to run the empty command rather than fall back to `ssh`.
fn fetch_ssh_command(
    git_ssh_command: Option<&str>,
    git_ssh: Option<&str>,
    core_ssh_command: Option<&str>,
) -> Option<&'static str> {
    let user_chose = git_ssh_command.is_some() || git_ssh.is_some() || core_ssh_command.is_some();
    (!user_chose).then_some(FETCH_SSH_COMMAND)
}

/// `core.sshCommand` as git sees it in this repo, from any config scope.
/// `None` when it is unset or cannot be read: a config git cannot read fails
/// the fetch itself before ssh runs.
async fn configured_ssh_command(repo_path: &Path) -> Option<String> {
    let output = run_git(repo_path, &["config", "--get", "core.sshCommand"]).await?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout);
    Some(value.strip_suffix('\n').unwrap_or(&value).to_string())
}

/// The fetch command, with `ssh_command` passed as `-c core.sshCommand=...`
/// when given. `-c` rather than `GIT_SSH_COMMAND` on the child: git ranks a
/// `GIT_SSH_COMMAND` in its environment above any `core.sshCommand`, so a
/// user's variable wins even if the decision above misread it, where a
/// variable set by fleetz would replace theirs.
fn fetch_cmd(repo_path: &Path, ssh_command: Option<&str>) -> Command {
    let ssh_config = ssh_command.map(|ssh| format!("core.sshCommand={ssh}"));
    let mut args: Vec<&str> = Vec::with_capacity(FETCH_ARGS.len() + 2);
    if let Some(config) = &ssh_config {
        args.extend(["-c", config.as_str()]);
    }
    args.extend(FETCH_ARGS);
    git_cmd(repo_path, &args)
}

/// The fetch command for this repo, with the ssh options unless the user's
/// environment or git config already names an ssh command.
async fn prepared_fetch_cmd(repo_path: &Path) -> Command {
    let env = |name: &str| std::env::var_os(name).map(|v| v.to_string_lossy().into_owned());
    let configured = configured_ssh_command(repo_path).await;
    let ssh_command = fetch_ssh_command(
        env("GIT_SSH_COMMAND").as_deref(),
        env("GIT_SSH").as_deref(),
        configured.as_deref(),
    );
    fetch_cmd(repo_path, ssh_command)
}

/// The names of the temp files an interrupted fetch leaves in
/// `<common_dir>/objects/pack`: `tmp_pack_*` (index-pack's pack being
/// written), `tmp_idx_*` (its index) and `tmp_rev_*` (its reverse index).
/// Empty when the directory cannot be read.
fn temp_pack_names(common_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(common_dir.join("objects").join("pack")) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| {
            ["tmp_pack_", "tmp_idx_", "tmp_rev_"]
                .iter()
                .any(|p| n.starts_with(p))
        })
        .collect()
}

/// Send `sig` to every process in the group `pgid`; true when the call
/// reached at least one. The one place that signals a group.
fn signal_process_group(pgid: u32, sig: i32) -> bool {
    // A pgid of 0 or 1 would address our own group or every process we may
    // signal: never.
    match i32::try_from(pgid) {
        // SAFETY: `kill` takes two integers and touches no memory of ours; a
        // negative pid addresses the group.
        Ok(pgid) if pgid > 1 => unsafe { libc::kill(-pgid, sig) == 0 },
        _ => false,
    }
}

/// SIGKILL the whole group. A group already gone is the outcome wanted.
fn kill_process_group(pgid: u32) {
    signal_process_group(pgid, libc::SIGKILL);
}

/// Whether any process of group `pgid` still exists (signal 0 only checks).
fn process_group_alive(pgid: u32) -> bool {
    signal_process_group(pgid, 0)
}

/// Read a child's pipe to its end; empty when there is none.
async fn read_pipe(pipe: Option<impl tokio::io::AsyncRead + Unpin>) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let mut buf = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_end(&mut buf).await;
    }
    buf
}

/// A running fetch, spawned as the leader of its own process group, so the
/// `git-remote-https`, `ssh`, `index-pack` and `upload-pack` it starts can be
/// killed with it. `kill_on_drop` alone killed only the `git` pid and left
/// the rest running, orphaned (#83). Dropping the guard before the fetch was
/// reaped (the sweep task cancelled, fleetz quitting) SIGKILLs the group and
/// removes the temp packs it created; `kill_on_drop` stays as the last resort
/// for the leader, which tokio then reaps.
struct FetchGroup {
    child: tokio::process::Child,
    pgid: Option<u32>,
    /// The common dir and the temp pack names in it before the fetch
    /// started, for `remove_new_temp_packs`. `None` when the dir is unknown.
    snapshot: Option<(std::path::PathBuf, Vec<String>)>,
}

impl FetchGroup {
    /// Kill the whole group and reap the leader. The group is killed before
    /// the leader is reaped, so its id cannot have been reused yet.
    async fn kill_and_reap(&mut self) {
        if let Some(pgid) = self.pgid.take() {
            kill_process_group(pgid);
            let _ = self.child.wait().await;
            // Orphans are reaped by init, not by us: wait briefly until the
            // group is gone, so the temp files are not removed under a
            // process still writing them. Bounded, since a process stuck in
            // the kernel (a dead network mount) cannot be waited out.
            let start = std::time::Instant::now();
            while process_group_alive(pgid) && start.elapsed() < GROUP_EXIT_WAIT {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        } else {
            let _ = self.child.kill().await;
        }
    }

    /// Remove the temp packs this fetch created (`remove_new_temp_packs`).
    fn remove_temp_packs(&self) {
        if let Some((dir, before)) = &self.snapshot {
            remove_new_temp_packs(dir, before);
        }
    }
}

impl Drop for FetchGroup {
    /// A fetch dropped mid-run. Best effort and never blocking: the group
    /// gets SIGKILL and the new temp packs are removed at once, without
    /// waiting for the group to die (`Drop` cannot await). A file a dying
    /// process still holds open is unlinked all the same and its space goes
    /// when the process does.
    fn drop(&mut self) {
        if let Some(pgid) = self.pgid {
            kill_process_group(pgid);
            self.remove_temp_packs();
        }
    }
}

/// How long a timed-out fetch waits for its killed group to disappear before
/// removing temp files anyway.
const GROUP_EXIT_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// `fetch_watched` with no one listening to its progress.
#[cfg(test)]
async fn fetch_within(repo_path: &Path, limit: std::time::Duration) -> FetchOutcome {
    fetch_watched(repo_path, limit, |_| {}).await
}

/// How a watched fetch ended, before its group is dealt with.
enum Watched {
    Exited(std::process::Output),
    Stalled,
    /// An update reported more bytes than the cap: this many.
    TooBig(u64),
    WaitFailed,
}

/// The next chunk of a pipe, `None` at its end, on an error, or when there
/// is no pipe. An `async fn` so `select!` builds it for a closed pipe
/// without touching the pipe.
async fn read_chunk(pipe: &mut Option<tokio::process::ChildStderr>) -> Option<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let pipe = pipe.as_mut()?;
    let mut buf = vec![0u8; 4096];
    match pipe.read(&mut buf).await {
        Ok(0) | Err(_) => None,
        Ok(n) => {
            buf.truncate(n);
            Some(buf)
        }
    }
}

/// Wait for the fetch while reading both pipes, so a chatty fetch never
/// blocks on a full stderr pipe, and hand each progress update to
/// `on_progress`. `Stalled` when `stall_limit` passes with no update; the
/// clock starts when the child does and restarts on every update. `TooBig`
/// as soon as an update reports more than `max_bytes` received.
async fn watch_fetch(
    child: &mut tokio::process::Child,
    stdout: Option<tokio::process::ChildStdout>,
    mut stderr: Option<tokio::process::ChildStderr>,
    stall_limit: std::time::Duration,
    max_bytes: Option<u64>,
    on_progress: &mut impl FnMut(FetchProgress),
) -> Watched {
    let wait = child.wait();
    tokio::pin!(wait);
    let stdout = read_pipe(stdout);
    tokio::pin!(stdout);
    let deadline = tokio::time::sleep(stall_limit);
    tokio::pin!(deadline);
    let mut log = FetchStderr::default();
    let (mut status, mut out) = (None, None);
    while status.is_none() || out.is_none() || stderr.is_some() {
        tokio::select! {
            exited = &mut wait, if status.is_none() => match exited {
                Ok(exited) => status = Some(exited),
                Err(_) => return Watched::WaitFailed,
            },
            read = &mut stdout, if out.is_none() => out = Some(read),
            chunk = read_chunk(&mut stderr), if stderr.is_some() => {
                let updates = match chunk {
                    Some(chunk) => log.feed(&chunk),
                    None => {
                        stderr = None;
                        log.finish().into_iter().collect()
                    }
                };
                for update in updates {
                    deadline
                        .as_mut()
                        .reset(tokio::time::Instant::now() + stall_limit);
                    on_progress(update);
                    if max_bytes.is_some_and(|max| update.bytes > max) {
                        return Watched::TooBig(update.bytes);
                    }
                }
            },
            () = &mut deadline => return Watched::Stalled,
        }
    }
    match (status, out) {
        (Some(status), Some(stdout)) => Watched::Exited(std::process::Output {
            status,
            stdout,
            stderr: log.kept,
        }),
        _ => Watched::WaitFailed,
    }
}

/// Run the fetch in its own process group, cut when it goes `stall_limit`
/// without a progress update, handing each update to `on_progress`.
///
/// On a stall the group is killed and reaped, and the temp pack files
/// (`temp_pack_names`) that were not in the pack directory before the fetch
/// started are removed (`remove_new_temp_packs`): FZ-34's handling of a
/// timeout, and the outcome is `TimedOut` as it was. A fetch that exits by
/// itself removes nothing: git cleans up after its own exit. A cancelled
/// fetch (the guard dropped mid-run) kills the group and removes them
/// without waiting (`FetchGroup`'s `Drop`).
#[cfg(test)]
pub(crate) async fn fetch_watched(
    repo_path: &Path,
    stall_limit: std::time::Duration,
    on_progress: impl FnMut(FetchProgress),
) -> FetchOutcome {
    fetch_capped(repo_path, stall_limit, None, on_progress).await
}

/// `fetch_watched` with a cap (FZ-35b): once an update reports more than
/// `max_bytes` received, the fetch is killed and cleaned exactly as a stalled
/// one is, and reports `TooBig` with the bytes it had. A pack cannot be
/// resumed, so at most the cap is wasted.
async fn fetch_capped(
    repo_path: &Path,
    stall_limit: std::time::Duration,
    max_bytes: Option<u64>,
    mut on_progress: impl FnMut(FetchProgress),
) -> FetchOutcome {
    let snapshot = repo_dirs(repo_path).map(|d| {
        let before = temp_pack_names(&d.common_dir);
        (d.common_dir, before)
    });
    let mut cmd = prepared_fetch_cmd(repo_path).await;
    cmd.process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let Ok(mut child) = cmd.spawn() else {
        return fetch_outcome(FetchRun::NotStarted);
    };
    let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
    let pgid = child.id();
    let mut group = FetchGroup {
        child,
        pgid,
        snapshot,
    };
    let watched = watch_fetch(
        &mut group.child,
        stdout,
        stderr,
        stall_limit,
        max_bytes,
        &mut on_progress,
    )
    .await;
    let run = match watched {
        Watched::Stalled => {
            group.kill_and_reap().await;
            group.remove_temp_packs();
            FetchRun::TimedOut
        }
        Watched::TooBig(bytes) => {
            group.kill_and_reap().await;
            group.remove_temp_packs();
            FetchRun::TooBig(bytes)
        }
        Watched::WaitFailed => {
            group.kill_and_reap().await;
            FetchRun::NotStarted
        }
        Watched::Exited(output) => {
            // Reaped: the group id may be reused from here on, and whatever
            // the leader left running is no longer ours to kill.
            group.pgid = None;
            FetchRun::Exited(output)
        }
    };
    fetch_outcome(run)
}

/// Remove the temp packs in `common_dir` that are not in `before`, the names
/// taken before a timed-out fetch started, once its group is dead.
///
/// Accepted race (#83, R2): a `git fetch` the user started in a terminal
/// while ours ran also writes a new temp pack there, which this removes too;
/// that one fetch of theirs fails and is rerun. Nothing that was there
/// before ours started is touched, and no pack git references is.
fn remove_new_temp_packs(common_dir: &Path, before: &[String]) {
    let pack_dir = common_dir.join("objects").join("pack");
    for name in temp_pack_names(common_dir) {
        if !before.contains(&name) {
            let _ = std::fs::remove_file(pack_dir.join(name));
        }
    }
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
        FetchRun::TooBig(bytes) => FetchFailure::too_big(bytes),
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
        "host key verification failed",
        "invalid username or password",
        "access denied",
        "returned error: 401",
        "returned error: 403",
    ];
    // OpenSSH's refusal, `Permission denied (publickey)` or with more
    // methods in the parentheses, and the agent and identity lines it can
    // print before it.
    const SSH: &[&str] = &[
        "permission denied (",
        "agent refused operation",
        "no such identity",
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
    // ssh saying it never reached the host, whatever reason follows the last
    // colon: the reason is an errno text, and no list of those is complete
    // (macOS with the network down prints `Undefined error: 0`). The generic
    // `fatal: Could not read from remote repository.` after it is no kind on
    // its own: git prints it after auth failures and missing repositories too.
    let ssh_unreachable = text.lines().map(str::trim).any(|l| {
        l.starts_with("ssh: connect to host ") || l.starts_with("ssh: could not resolve hostname ")
    });
    // Auth first even then: a credential that was asked for or refused means
    // a server answered at some point (another address, a jump host), and the
    // credential is what the user has to fix. Within that, ssh refusing the
    // key comes first: its fix is the key or the agent, not a password.
    if has(SSH) {
        FetchFailureKind::Ssh
    } else if has(AUTH) {
        FetchFailureKind::Auth
    } else if ssh_unreachable || has(UNREACHABLE) {
        FetchFailureKind::Unreachable
    } else if missing_repo || has(NO_REMOTE) {
        FetchFailureKind::NoRemote
    } else {
        FetchFailureKind::Other
    }
}

/// git's first error line, made safe to draw: an `ssh: ` line printed before
/// the first `fatal:` or `error:` line (it says why, where the fatal line
/// after it is generic), else that first `fatal:` or `error:` line, else the
/// first non-empty one, with URL userinfo removed, control characters dropped
/// and the length capped.
fn first_error_line(stderr: &str) -> Option<String> {
    let lines = || stderr.lines().map(str::trim).filter(|l| !l.is_empty());
    let is_error = |l: &str| {
        let lower = l.to_lowercase();
        lower.starts_with("fatal:") || lower.starts_with("error:")
    };
    let ssh_reason = lines()
        .take_while(|l| !is_error(l))
        .find(|l| l.to_lowercase().starts_with("ssh: "));
    let line = ssh_reason
        .or_else(|| lines().find(|l| is_error(l)))
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
/// Empty when the query fails; `try_branch_statuses` tells the two apart.
pub async fn branch_statuses(repo_path: &Path) -> Vec<BranchStatus> {
    try_branch_statuses(repo_path)
        .await
        .map(|local| local.statuses)
        .unwrap_or_default()
}

/// The branch query, keeping a failure distinct from "no branches" and the
/// full ids and HEAD symref `work_key_from` hashes.
pub async fn try_branch_statuses(repo_path: &Path) -> Result<LocalBranches, RefQueryFailed> {
    let output = run_git(
        repo_path,
        &[
            "for-each-ref",
            // `%(HEAD)` is `*` on the checked-out branch and a space on every
            // other, so which branch you are on rides the query that was
            // already being made rather than costing a second subprocess.
            // The full refname and object id at the end are for the key;
            // `tip` stays abbreviated to match the graph's `%h`.
            "--format=%(HEAD)\t%(refname:short)\t%(upstream:short)\t%(upstream:track)\t%(objectname:short)\t%(refname)\t%(objectname)",
            "refs/heads/",
        ],
    )
    .await
    .ok_or(RefQueryFailed)?;
    if !output.status.success() {
        return Err(RefQueryFailed);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let (refs, head) = parse_branch_refs(&stdout);
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

    Ok(LocalBranches {
        statuses,
        refs,
        head,
    })
}

/// The key's half of the branch query: each branch's (full refname, full
/// object id), from the last two fields, and the full refname marked `*`.
fn parse_branch_refs(output: &str) -> (Vec<RefId>, Option<String>) {
    let mut refs = Vec::new();
    let mut head = None;
    for line in output.lines().filter(|l| !l.is_empty()) {
        let parts: Vec<&str> = line.split('\t').collect();
        let (Some(full), Some(id)) = (parts.get(5), parts.get(6)) else {
            continue;
        };
        if parts.first().is_some_and(|h| h.trim() == "*") {
            head = Some(full.to_string());
        }
        refs.push((full.to_string(), id.trim().to_string()));
    }
    (refs, head)
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

/// Commits committed since `since` on every local and remote-tracking
/// branch, newest first: what the slot digest counts (FZ-45). Run after the
/// sweep's fetch, so the remote-tracking refs carry what was pushed; local
/// branches add what was committed here and not pushed yet. No stat and no
/// body: the digest names subjects, authors and times only, and the reads
/// are held for the session. No cap either, so the count never undercounts.
/// Empty on any failure.
pub async fn commits_since(repo_path: &Path, since: DateTime<chrono::Utc>) -> Vec<Commit> {
    let since = format!("--since={}", since.format("%Y-%m-%d %H:%M:%S +0000"));
    let output = run_git(
        repo_path,
        &[
            "log",
            // The trailing separator keeps the record's newline out of the
            // subject, where `%b` used to take it.
            "--format=%x1e%h%x1f%aN%x1f%at%x1f%s%x1f",
            &since,
            "--branches",
            "--remotes",
        ],
    )
    .await;
    match output {
        Some(o) if o.status.success() => parse_commit_output(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
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

/// One file in the `changes` view (FZ-60).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    /// Porcelain's two columns (`M `, ` M`, `??`) in the uncommitted section,
    /// `--name-status`'s one letter in the other.
    pub status: String,
    /// Repo-relative; a rename's destination, an untracked directory with
    /// its trailing `/`.
    pub path: String,
    /// A rename's or copy's source.
    pub from: Option<String>,
    /// `(added, removed)` lines; `None` for an untracked or binary file.
    pub churn: Option<(u32, u32)>,
}

/// What the `changes` view's second section compares `HEAD` with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChangesBase {
    /// The branch's upstream: what has not been pushed.
    #[default]
    Upstream,
    /// The default branch (`default_branch`): the branch's whole divergence.
    DefaultBranch,
}

/// The `changes` view's second section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outgoing {
    /// The files the commits ahead of `upstream` touch.
    Unpushed {
        upstream: String,
        ahead: u32,
        files: Vec<ChangedFile>,
    },
    /// No upstream to compare with: the branch is not tracking one, or HEAD
    /// is detached (`branch` is `None`).
    NoUpstream { branch: Option<String> },
    /// The files `HEAD` changed since it left `base`, the default branch.
    VsDefault {
        base: String,
        files: Vec<ChangedFile>,
    },
    /// The base toggle is on and no default branch resolves.
    NoDefault,
}

impl Outgoing {
    /// The ref the section's per-file diffs run against, if it lists files.
    pub fn base_ref(&self) -> Option<&str> {
        match self {
            Self::Unpushed { upstream, .. } => Some(upstream),
            Self::VsDefault { base, .. } => Some(base),
            Self::NoUpstream { .. } | Self::NoDefault => None,
        }
    }

    /// The files the section lists.
    pub fn files(&self) -> &[ChangedFile] {
        match self {
            Self::Unpushed { files, .. } | Self::VsDefault { files, .. } => files,
            Self::NoUpstream { .. } | Self::NoDefault => &[],
        }
    }
}

/// One read of the `changes` view: what has not left this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changes {
    pub uncommitted: Vec<ChangedFile>,
    pub outgoing: Outgoing,
}

/// Read a repo's `changes` (FZ-60): `git status --porcelain` with
/// `git diff HEAD --numstat` churn, then the files between `base` and
/// `HEAD`. NUL-separated throughout, so a path with spaces or non-ASCII
/// arrives as it is named. An error only when the status itself fails.
pub async fn changes(repo_path: &Path, base: ChangesBase) -> Result<Changes, String> {
    let status = git_ok(
        repo_path,
        &[
            "status",
            "--porcelain",
            "-z",
            "--untracked-files=normal",
            "--renames",
        ],
    )
    .await?;
    let churn = git_ok(repo_path, &["diff", "-M", "HEAD", "--numstat", "-z"])
        .await
        .map(|out| parse_numstat_z(&out))
        .unwrap_or_default();
    let mut uncommitted = parse_porcelain_z(&status);
    for file in &mut uncommitted {
        if file.status != "??" {
            file.churn = churn.get(&file.path).copied().flatten();
        }
    }
    let outgoing = match base {
        ChangesBase::Upstream => unpushed(repo_path).await,
        ChangesBase::DefaultBranch => vs_default(repo_path).await,
    };
    Ok(Changes {
        uncommitted,
        outgoing,
    })
}

/// The section against the upstream: the commits ahead of it, and the files
/// they touch since the merge base, so a branch that is also behind does not
/// list the upstream's own changes as unpushed.
async fn unpushed(repo_path: &Path) -> Outgoing {
    let branch = git_ok(repo_path, &["symbolic-ref", "-q", "--short", "HEAD"])
        .await
        .ok()
        .map(|b| b.trim().to_string());
    let upstream = match branch {
        None => None,
        Some(_) => git_ok(
            repo_path,
            &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
        )
        .await
        .ok()
        .map(|u| u.trim().to_string()),
    };
    let Some(upstream) = upstream.filter(|u| !u.is_empty()) else {
        return Outgoing::NoUpstream { branch };
    };
    let ahead = git_ok(
        repo_path,
        &["rev-list", "--count", &format!("{upstream}..HEAD")],
    )
    .await
    .ok()
    .and_then(|n| n.trim().parse().ok())
    .unwrap_or(0);
    let files = files_since(repo_path, &upstream).await;
    Outgoing::Unpushed {
        upstream,
        ahead,
        files,
    }
}

/// The section against the default branch: `origin/<default>` when the
/// clone has it, else the local one.
async fn vs_default(repo_path: &Path) -> Outgoing {
    let Some(name) = default_branch(repo_path).await else {
        return Outgoing::NoDefault;
    };
    let remote = format!("origin/{name}");
    let has_remote = git_ok(
        repo_path,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/remotes/{remote}"),
        ],
    )
    .await
    .is_ok();
    let base = if has_remote { remote } else { name };
    let files = files_since(repo_path, &base).await;
    Outgoing::VsDefault { base, files }
}

/// `git diff --name-status` joined with `--numstat`, `base...HEAD`.
async fn files_since(repo_path: &Path, base: &str) -> Vec<ChangedFile> {
    let range = format!("{base}...HEAD");
    let Ok(names) = git_ok(repo_path, &["diff", "-M", "--name-status", "-z", &range]).await else {
        return Vec::new();
    };
    let churn = git_ok(repo_path, &["diff", "-M", "--numstat", "-z", &range])
        .await
        .map(|out| parse_numstat_z(&out))
        .unwrap_or_default();
    let mut files = parse_name_status_z(&names);
    for file in &mut files {
        file.churn = churn.get(&file.path).copied().flatten();
    }
    files
}

/// The inspector's body for one `changes` file: `git diff HEAD` for an
/// uncommitted file (`base` `None`), `git diff <base>...HEAD` for the other
/// section, an untracked file's own text. Capped at `notes::BODY_CAP`.
pub async fn changed_file_diff(
    repo_path: &Path,
    base: Option<&str>,
    file: &ChangedFile,
) -> Result<String, String> {
    if base.is_none() && file.status == "??" {
        if file.path.ends_with('/') {
            return Ok(format!(
                "{} is an untracked directory; Enter opens it",
                file.path
            ));
        }
        let path = repo_path.join(&file.path);
        return tokio::task::spawn_blocking(move || crate::notes::read_body(&path))
            .await
            .map_err(|e| e.to_string())?;
    }
    let range = match base {
        Some(b) => format!("{b}...HEAD"),
        None => "HEAD".to_string(),
    };
    // Literal, so a name with `:` or glob characters is a path; unquoted, so
    // a non-ASCII name reads as itself in the diff's headers.
    let mut args = vec![
        "-c",
        "core.quotePath=false",
        "--literal-pathspecs",
        "diff",
        "--no-ext-diff",
        "-M",
        range.as_str(),
        "--",
        file.path.as_str(),
    ];
    if let Some(from) = &file.from {
        args.push(from.as_str());
    }
    let diff = git_ok(repo_path, &args).await?;
    Ok(cap_body(diff))
}

/// `text` cut at `notes::BODY_CAP` bytes on a character boundary, saying so.
fn cap_body(mut text: String) -> String {
    let cap = crate::notes::BODY_CAP;
    if text.len() <= cap {
        return text;
    }
    let mut cut = cap;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    text.truncate(cut);
    format!("{text}\n… cut at {} KiB", cap / 1024)
}

/// stdout of a git command that succeeded, else its stderr as the error.
async fn git_ok(repo_path: &Path, args: &[&str]) -> Result<String, String> {
    let output = run_git(repo_path, args)
        .await
        .ok_or_else(|| "git did not answer".to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `git status --porcelain -z`: `XY path`, a rename's or copy's source in
/// the field after it. An untracked directory stays one row.
fn parse_porcelain_z(out: &str) -> Vec<ChangedFile> {
    let mut fields = out.split('\0');
    let mut files = Vec::new();
    while let Some(entry) = fields.next() {
        let (Some(status), Some(path)) = (entry.get(..2), entry.get(3..)) else {
            continue;
        };
        let from = match status.contains(['R', 'C']) {
            true => fields.next().map(str::to_string),
            false => None,
        };
        files.push(ChangedFile {
            status: status.to_string(),
            path: path.to_string(),
            from,
            churn: None,
        });
    }
    files
}

/// `git diff --name-status -z`: a status field, then the path, or a
/// rename's or copy's source and destination.
fn parse_name_status_z(out: &str) -> Vec<ChangedFile> {
    let mut fields = out.split('\0');
    let mut files = Vec::new();
    while let Some(status) = fields.next() {
        let Some(letter) = status.chars().next() else {
            continue;
        };
        let Some(first) = fields.next() else { break };
        let (from, path) = match letter {
            'R' | 'C' => match fields.next() {
                Some(dest) => (Some(first.to_string()), dest.to_string()),
                None => break,
            },
            _ => (None, first.to_string()),
        };
        files.push(ChangedFile {
            status: letter.to_string(),
            path,
            from,
            churn: None,
        });
    }
    files
}

/// `git diff --numstat -z`, keyed by path (a rename's destination):
/// `added\tremoved\tpath`, or an empty path followed by the source and the
/// destination. A binary file's `-` counts are `None`.
fn parse_numstat_z(out: &str) -> std::collections::HashMap<String, Option<(u32, u32)>> {
    let mut fields = out.split('\0');
    let mut churn = std::collections::HashMap::new();
    while let Some(record) = fields.next() {
        let mut parts = record.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let path = match path.is_empty() {
            true => {
                let _source = fields.next();
                match fields.next() {
                    Some(dest) => dest,
                    None => break,
                }
            }
            false => path,
        };
        let counts = added.parse().ok().zip(removed.parse().ok());
        churn.insert(path.to_string(), counts);
    }
    churn
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
/// symbolic refs like `origin/HEAD`. Empty when the query fails.
pub async fn remote_branches(repo_path: &Path) -> Vec<RemoteBranch> {
    try_ref_listing(repo_path)
        .await
        .map(|other| other.remotes)
        .unwrap_or_default()
}

/// Every ref, in one query: the remote-tracking branches the panes show,
/// and every ref outside `refs/heads/` for the key.
///
/// No pattern, because `for-each-ref --exclude` is not in git 2.39, so the
/// local branches are listed here too and dropped by the parser: they come
/// from the branch query, whose list is the one the history probes walk.
/// That costs text, not a process.
pub async fn try_ref_listing(repo_path: &Path) -> Result<OtherRefs, RefQueryFailed> {
    let output = run_git(
        repo_path,
        &[
            "for-each-ref",
            // Both forms: the full refname to filter on, the short one to
            // show. Filtering on the short name alone missed the symref --
            // see `parse_remote_branch_output`. The full object id is for
            // the key; `tip` stays abbreviated.
            "--format=%(refname)\t%(refname:short)\t%(objectname:short)\t%(objectname)",
        ],
    )
    .await
    .ok_or(RefQueryFailed)?;
    if !output.status.success() {
        return Err(RefQueryFailed);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(OtherRefs {
        remotes: parse_remote_branch_output(&stdout),
        refs: parse_other_refs(&stdout),
    })
}

/// The key's half of the widened query: (full refname, full object id) for
/// every ref outside `refs/heads/`, the remotes' `HEAD` symrefs included.
fn parse_other_refs(stdout: &str) -> Vec<RefId> {
    stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            let full = *parts.first()?;
            if full.is_empty() || full.starts_with("refs/heads/") {
                return None;
            }
            let id = parts.get(3).map_or("", |s| s.trim());
            Some((full.to_string(), id.to_string()))
        })
        .collect()
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
///
/// The query lists every ref, so anything outside `refs/remotes/` (local
/// branches, tags, the stash, notes) is dropped here: it would otherwise
/// show up in the branches view.
fn parse_remote_branch_output(stdout: &str) -> Vec<RemoteBranch> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let full = parts.next()?;
            let short = parts.next()?.trim();
            if !full.starts_with("refs/remotes/") || full.ends_with("/HEAD") || short.is_empty() {
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

/// Whether `git worktree list` has anything to say for this checkout: one
/// `stat` of `<common>/worktrees`, which git creates with the first linked
/// worktree and deletes with the last. `true` when the directories cannot be
/// resolved, so an unreadable layout still asks git rather than assuming none.
fn worktree_probe_needed(repo_path: &Path) -> bool {
    repo_dirs(repo_path).is_none_or(|dirs| dirs.common_dir.join("worktrees").is_dir())
}

/// The linked worktrees, cheap enough for every sweep: no process at all when
/// `<common>/worktrees` is absent, which then means none, never "unknown".
/// Otherwise `worktree_list`. Worktrees are not refs, so `WorkKey` cannot gate
/// this list; it is read fresh instead.
pub async fn linked_worktrees(repo_path: &Path) -> Vec<crate::model::Worktree> {
    if !worktree_probe_needed(repo_path) {
        return Vec::new();
    }
    worktree_list(repo_path).await
}

/// List active worktrees (excluding the main one), each with its own dirty
/// count and HEAD time (`worktree_state`): two processes per linked worktree.
pub async fn worktree_list(repo_path: &Path) -> Vec<crate::model::Worktree> {
    let output = run_git(repo_path, &["worktree", "list", "--porcelain"]).await;

    let output = match output {
        Some(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut worktrees = parse_worktree_output(&stdout, repo_path);
    fill_worktree_state(&mut worktrees).await;
    worktrees
}

/// One linked worktree's own state: its `status --porcelain` line count and
/// its HEAD commit's time. Each is `None` when its probe fails, so a broken or
/// vanished worktree draws as unknown, never as clean. Both go through
/// `run_git` for its hardening and ceiling, and run concurrently.
///
/// The commit time, not the index's mtime: `status` itself may rewrite the
/// index, and it runs here every sweep, so that age would read "now" forever.
pub async fn worktree_state(worktree_path: &Path) -> (Option<u32>, Option<DateTime<chrono::Utc>>) {
    let (status, log) = tokio::join!(
        run_git(worktree_path, &["status", "--porcelain"]),
        run_git(worktree_path, &["log", "-1", "--format=%ct"]),
    );
    let dirty = status.filter(|o| o.status.success()).map(|o| {
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .count() as u32
    });
    let head_at = log
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<i64>()
                .ok()
        })
        .and_then(|secs| DateTime::from_timestamp(secs, 0));
    (dirty, head_at)
}

/// The filesystem path behind a `tilde_path` spelling, for probing it.
fn untilde_path(path: &str) -> std::path::PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None => std::path::PathBuf::from(path),
    }
}

/// `worktree_state` for every worktree in the list, concurrently, written
/// into each entry.
async fn fill_worktree_state(worktrees: &mut [crate::model::Worktree]) {
    let probes: Vec<_> = worktrees
        .iter()
        .map(|wt| {
            let path = untilde_path(&wt.path);
            tokio::spawn(async move { worktree_state(&path).await })
        })
        .collect();
    for (wt, probe) in worktrees.iter_mut().zip(probes) {
        if let Ok((dirty, head_at)) = probe.await {
            wt.dirty = dirty;
            wt.head_at = head_at;
        }
    }
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

/// The linked worktrees in `git worktree list --porcelain` output, with
/// their branch, or `detached` and no branch, and HEAD's short sha. The
/// state fields are left for `worktree_state`.
///
/// A detached entry is kept: it has a `detached` line where a branch entry
/// has `branch`, and it is a bisect or a review checkout, exactly the thing
/// that is forgotten. A bare entry (no branch, not detached) is skipped.
fn parse_worktree_output(output: &str, main_path: &Path) -> Vec<crate::model::Worktree> {
    let mut worktrees = Vec::new();
    let mut current = crate::model::Worktree::default();

    // Only LINKED worktrees are worth listing. The main one is just the repo,
    // whose path the pane header already prints.
    let push_linked = |entry: &mut crate::model::Worktree,
                       out: &mut Vec<crate::model::Worktree>| {
        let entry = std::mem::take(entry);
        let wt_path = std::path::Path::new(&entry.path);
        if (entry.branch.is_empty() && !entry.detached) || same_dir(wt_path, main_path) {
            return;
        }
        out.push(crate::model::Worktree {
            path: tilde_path(wt_path),
            ..entry
        });
    };

    for line in output.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            current = crate::model::Worktree {
                path: p.to_string(),
                ..Default::default()
            };
        } else if let Some(sha) = line.strip_prefix("HEAD ") {
            current.head = sha.chars().take(SHORT_SHA_LEN).collect();
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            current.branch = b.to_string();
        } else if line == "detached" {
            current.detached = true;
        } else if line.is_empty() && !current.path.is_empty() {
            push_linked(&mut current, &mut worktrees);
        }
    }

    // Last entry when the output has no trailing blank line.
    if !current.path.is_empty() {
        push_linked(&mut current, &mut worktrees);
    }

    worktrees
}

/// How many hex digits a worktree's HEAD is shown with: git's own default
/// abbreviation, so it reads like `%h` elsewhere in the pane.
const SHORT_SHA_LEN: usize = 7;

const PROTECTED_BRANCHES: &[&str] = &["main", "master", "develop", "dev"];

pub fn is_protected_branch(name: &str) -> bool {
    PROTECTED_BRANCHES.contains(&name)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod real_repo_tests;

#[cfg(test)]
mod fetch_outcome_tests;
