use crate::model::{BranchStatus, Commit, FileStat, GraphLine, ReflogEntry};
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

/// Cheap change detector: if none of these moved, the expensive probes
/// (log, reflog, graph, stash, worktree) cannot have new answers. Untracked
/// files do not touch the index, so dirty_count is always probed regardless.
pub async fn work_key(repo_path: &Path) -> Option<WorkKey> {
    let head = run_git(repo_path, &["rev-parse", "HEAD"]).await?;
    if !head.status.success() {
        return None; // unborn HEAD etc: treat as always-changed
    }
    let head_sha = String::from_utf8_lossy(&head.stdout).trim().to_string();

    let git_dir = resolve_git_dir(repo_path)?;
    // Stash log lives in the COMMON dir for linked worktrees.
    let common_dir = match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(rel) => git_dir.join(rel.trim()),
        Err(_) => git_dir.clone(),
    };
    let index_meta = std::fs::metadata(git_dir.join("index")).ok();
    let stash_size = std::fs::metadata(common_dir.join("logs/refs/stash"))
        .map(|m| m.len())
        .unwrap_or(0);

    Some(WorkKey {
        head_sha,
        index_mtime: index_meta.as_ref().and_then(|m| m.modified().ok()),
        index_size: index_meta.map(|m| m.len()).unwrap_or(0),
        stash_size,
    })
}

/// Run `git fetch` on a repo. Prunes deleted remote branches so
/// remote_only_branches does not accumulate stale entries.
pub async fn fetch(repo_path: &Path) -> bool {
    run_git(repo_path, &["fetch", "--quiet", "--prune"])
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get all branch statuses for a repo (behind/ahead relative to upstream).
pub async fn branch_statuses(repo_path: &Path) -> Vec<BranchStatus> {
    let Some(output) = run_git(
        repo_path,
        &[
            "for-each-ref",
            "--format=%(refname:short)\t%(upstream:short)\t%(upstream:track)",
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

    // `upstream:track` only measures divergence from a branch's own upstream. A
    // branch whose upstream is stale (e.g. a leftover worktree branch already
    // merged into main via a different ref) still reports "ahead" there even
    // though the commits are safely on the remote. Recheck against ALL remotes
    // before reporting a branch as genuinely unpushed.
    for status in &mut statuses {
        if status.ahead > 0 {
            status.ahead = count_unreachable_from_remotes(repo_path, &status.name).await;
        }
    }

    statuses
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
            let name = parts.first().unwrap_or(&"").to_string();
            let upstream = parts
                .get(1)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let track = parts.get(2).unwrap_or(&"");

            let (ahead, behind) = parse_track_info(track);

            BranchStatus {
                name,
                upstream,
                behind,
                ahead,
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
            "--date=relative",
            "--format=%x1e%h%x1f%aN%x1f%ad%x1f%s%x1f%b",
            branch,
        ],
    )
    .await;

    match output {
        Some(o) if o.status.success() => parse_commit_output(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

/// Get recent commits for a range (e.g. "main..origin/main" or "-5").
pub async fn commits(
    repo_path: &Path,
    range: &str,
    with_stat: bool,
    author_filter: Option<&str>,
) -> Vec<Commit> {
    // Record-separated so the (possibly multi-line) body can't be confused
    // with field boundaries: each commit starts with RS (0x1e), fields are
    // split by US (0x1f). With --shortstat, a " N files changed..." line
    // trails the body inside the same record.
    let mut args = vec![
        "log".to_string(),
        "--date=relative".to_string(),
        "--format=%x1e%h%x1f%aN%x1f%ad%x1f%s%x1f%b".to_string(),
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
        let date = fields[2].to_string();
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

/// Render `git log --graph` over the given local branches (their union of
/// reachable commits), limited to `limit` commits. The graph's ASCII
/// topology is git's own -- we just capture and colorize it, rather than
/// recomputing branch layout ourselves.
pub async fn graph(repo_path: &Path, branches: &[String], limit: u32) -> Vec<GraphLine> {
    if branches.is_empty() {
        return Vec::new();
    }

    let mut args = vec![
        "log".to_string(),
        "--graph".to_string(),
        "--color=never".to_string(),
        "--format=GRAPH|%h|%d|%aN|%s".to_string(),
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

fn parse_graph_output(output: &str) -> Vec<GraphLine> {
    const MARKER: &str = "GRAPH|";
    output
        .lines()
        .map(|line| match line.find(MARKER) {
            Some(idx) => {
                let prefix = line[..idx].to_string();
                let rest = &line[idx + MARKER.len()..];
                let parts: Vec<&str> = rest.splitn(4, '|').collect();
                GraphLine {
                    prefix,
                    hash: parts.first().unwrap_or(&"").to_string(),
                    refs: parts.get(1).unwrap_or(&"").trim().to_string(),
                    author: parts.get(2).unwrap_or(&"").to_string(),
                    message: parts.get(3).unwrap_or(&"").to_string(),
                }
            }
            None => GraphLine {
                prefix: line.to_string(),
                hash: String::new(),
                refs: String::new(),
                author: String::new(),
                message: String::new(),
            },
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
pub async fn remote_branches(repo_path: &Path) -> Vec<String> {
    let output = run_git(
        repo_path,
        &["for-each-ref", "--format=%(refname:short)", "refs/remotes/"],
    )
    .await;

    match output {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty() && !l.ends_with("/HEAD"))
            .map(|l| l.trim().to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// Remote branches with no local branch of the same name -- never
/// fetched/checked out locally, or checked out once and deleted locally
/// after merging. `remote` entries are the "origin/name" short form;
/// `local` entries are bare branch names.
pub fn unfetched_branches(remote: &[String], local: &[String]) -> Vec<String> {
    remote
        .iter()
        .filter(|r| {
            let short = r.split_once('/').map_or(r.as_str(), |(_, name)| name);
            !local.iter().any(|l| l == short)
        })
        .cloned()
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

fn parse_worktree_output(output: &str, main_path: &Path) -> Vec<crate::model::Worktree> {
    let mut worktrees = Vec::new();
    let mut current_path = String::new();
    let mut current_branch = String::new();

    for line in output.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            current_path = p.to_string();
            current_branch.clear();
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            current_branch = b.to_string();
        } else if line.is_empty() && !current_path.is_empty() {
            // Skip the main worktree
            let wt_path = std::path::Path::new(&current_path);
            if wt_path != main_path && !current_branch.is_empty() {
                let home = dirs::home_dir().unwrap_or_default();
                let display = format!(
                    "~{}",
                    wt_path.strip_prefix(&home).unwrap_or(wt_path).display()
                );
                worktrees.push(crate::model::Worktree {
                    path: display,
                    branch: current_branch.clone(),
                });
            }
            current_path.clear();
            current_branch.clear();
        }
    }

    // Handle last entry without trailing newline
    if !current_path.is_empty() {
        let wt_path = std::path::Path::new(&current_path);
        if wt_path != main_path && !current_branch.is_empty() {
            let home = dirs::home_dir().unwrap_or_default();
            let display = format!(
                "~{}",
                wt_path.strip_prefix(&home).unwrap_or(wt_path).display()
            );
            worktrees.push(crate::model::Worktree {
                path: display,
                branch: current_branch,
            });
        }
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

    #[test]
    fn unfetched_branches_skips_ones_with_local_counterpart() {
        let remote = vec!["origin/main".to_string(), "origin/feature-x".to_string()];
        let local = vec!["main".to_string()];
        assert_eq!(
            unfetched_branches(&remote, &local),
            vec!["origin/feature-x"]
        );
    }

    #[test]
    fn unfetched_branches_empty_when_all_fetched() {
        let remote = vec!["origin/main".to_string()];
        let local = vec!["main".to_string()];
        assert!(unfetched_branches(&remote, &local).is_empty());
    }

    #[test]
    fn unfetched_branches_handles_nested_remote_names() {
        // A remote name with slashes in the branch itself (e.g. "origin/feat/x")
        // should still compare against the bare local branch "feat/x".
        let remote = vec!["origin/feat/x".to_string()];
        let local = vec!["feat/x".to_string()];
        assert!(unfetched_branches(&remote, &local).is_empty());
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
        let output = "main\torigin/main\t[behind 3]\nfeat/x\torigin/feat/x\t[ahead 1]\n";
        let branches = parse_branch_output(output);
        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].name, "main");
        assert_eq!(branches[0].behind, 3);
        assert_eq!(branches[0].ahead, 0);
        assert_eq!(branches[1].name, "feat/x");
        assert_eq!(branches[1].behind, 0);
        assert_eq!(branches[1].ahead, 1);
    }

    #[test]
    fn parse_branch_output_no_upstream() {
        let output = "local-only\t\t\n";
        let branches = parse_branch_output(output);
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].name, "local-only");
        assert!(branches[0].upstream.is_none());
    }

    #[test]
    fn parse_commit_output_without_stat() {
        // Records: RS h US author US date US subject US body
        let output = "\u{1e}a1b2c3d\u{1f}Alice\u{1f}2 days ago\u{1f}feat: add feature\u{1f}\u{1e}e4f5g6h\u{1f}Bob\u{1f}5 hours ago\u{1f}fix: bug\u{1f}";
        let commits = parse_commit_output(output);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "a1b2c3d");
        assert_eq!(commits[0].author, "Alice");
        assert_eq!(commits[0].date, "2 days ago");
        assert_eq!(commits[0].message, "feat: add feature");
        assert_eq!(commits[0].body, "");
        assert_eq!(commits[0].insertions, 0);
    }

    #[test]
    fn parse_commit_output_with_stat() {
        let output = "\u{1e}a1b2c3d\u{1f}Alice\u{1f}2 days ago\u{1f}feat: add feature\u{1f} 3 files changed, 18 insertions(+), 4 deletions(-)\n\u{1e}e4f5g6h\u{1f}Bob\u{1f}5 hours ago\u{1f}fix: bug\u{1f} 1 file changed, 2 insertions(+)\n";
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
        let output = "\u{1e}a1b2c3d\u{1f}Alice\u{1f}2 days ago\u{1f}feat: add feature\u{1f}Body text.\n12\t3\tsrc/main.rs\n0\t7\tREADME.md\n-\t-\tlogo.png\n 3 files changed, 12 insertions(+), 10 deletions(-)\n";
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
        let output = "* GRAPH|a1b2c3d| (HEAD -> main, origin/main)|Alice|feat: add feature";
        let lines = parse_graph_output(output);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].prefix, "* ");
        assert_eq!(lines[0].hash, "a1b2c3d");
        assert_eq!(lines[0].refs, "(HEAD -> main, origin/main)");
        assert_eq!(lines[0].author, "Alice");
        assert_eq!(lines[0].message, "feat: add feature");
    }

    #[test]
    fn parse_graph_output_connector_line_has_no_commit_data() {
        let output = "* GRAPH|a1b2c3d||Alice|feat: add feature\n|\\  \n| * GRAPH|d4e5f6g||Bob|other branch commit";
        let lines = parse_graph_output(output);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1].prefix, "|\\  ");
        assert!(lines[1].hash.is_empty());
        assert!(lines[1].message.is_empty());
        assert_eq!(lines[2].prefix, "| * ");
        assert_eq!(lines[2].hash, "d4e5f6g");
        assert_eq!(lines[2].author, "Bob");
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
}
