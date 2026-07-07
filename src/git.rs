use crate::model::{BranchStatus, Commit, GraphLine, ReflogEntry};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Run `git fetch --quiet` on a repo. Errors are silently ignored (matches repoz behavior).
pub async fn fetch(repo_path: &Path) -> bool {
    Command::new("git")
        .args(["-C", &repo_path.to_string_lossy(), "fetch", "--quiet"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Get all branch statuses for a repo (behind/ahead relative to upstream).
pub async fn branch_statuses(repo_path: &Path) -> Vec<BranchStatus> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "for-each-ref",
            "--format=%(refname:short)\t%(upstream:short)\t%(upstream:track)",
            "refs/heads/",
        ])
        .output()
        .await;

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

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
    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "rev-list",
            "--count",
            branch,
            "--not",
            "--remotes",
        ])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
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
            let upstream = parts.get(1).filter(|s| !s.is_empty()).map(|s| s.to_string());
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

    for part in track.split(|c: char| c == '[' || c == ']' || c == ',') {
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
    let output = Command::new("git")
        .args(["-C", &repo_path.to_string_lossy(), "status", "--porcelain"])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .count() as u32
        }
        _ => 0,
    }
}

/// Get recent commits for a range (e.g. "main..origin/main" or "-5").
pub async fn commits(
    repo_path: &Path,
    range: &str,
    with_stat: bool,
    author_filter: Option<&str>,
) -> Vec<Commit> {
    let mut args = vec![
        "-C".to_string(),
        repo_path.to_string_lossy().to_string(),
        "log".to_string(),
        "--format=COMMIT|%h|%aN|%s".to_string(),
    ];
    if with_stat {
        args.push("--shortstat".to_string());
    }
    if let Some(author) = author_filter {
        args.push(format!("--author={author}"));
    }
    args.push(range.to_string());

    let output = Command::new("git").args(&args).output().await;

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_commit_output(&stdout)
}

/// Parse git log output with COMMIT| prefix format.
fn parse_commit_output(output: &str) -> Vec<Commit> {
    let mut result = Vec::new();
    let mut current: Option<Commit> = None;

    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("COMMIT|") {
            // Flush previous commit
            if let Some(c) = current.take() {
                result.push(c);
            }
            let parts: Vec<&str> = rest.splitn(3, '|').collect();
            if parts.len() == 3 {
                current = Some(Commit {
                    hash: parts[0].to_string(),
                    author: parts[1].to_string(),
                    message: parts[2].to_string(),
                    insertions: 0,
                    deletions: 0,
                });
            }
        } else if line.contains("changed") {
            // --shortstat line: " 3 files changed, 18 insertions(+), 4 deletions(-)"
            if let Some(ref mut c) = current {
                c.insertions = extract_stat_number(line, "insertion");
                c.deletions = extract_stat_number(line, "deletion");
            }
        }
    }

    if let Some(c) = current {
        result.push(c);
    }
    result
}

fn extract_stat_number(line: &str, keyword: &str) -> u32 {
    // Find " N insertion" or " N deletion" pattern
    for part in line.split(',') {
        let part = part.trim();
        if part.contains(keyword) {
            if let Some(n) = part.split_whitespace().next() {
                return n.parse().unwrap_or(0);
            }
        }
    }
    0
}

/// Get the last `limit` reflog entries -- HEAD movements, for a "what did I
/// just do" recovery glance.
pub async fn reflog(repo_path: &Path, limit: u32) -> Vec<ReflogEntry> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "reflog",
            "--format=REFLOG|%h|%gs",
            "-n",
            &limit.to_string(),
        ])
        .output()
        .await;

    let output = match output {
        Ok(o) if o.status.success() => o,
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
        "-C".to_string(),
        repo_path.to_string_lossy().to_string(),
        "log".to_string(),
        "--graph".to_string(),
        "--color=never".to_string(),
        "--format=GRAPH|%h|%d|%s".to_string(),
        "-n".to_string(),
        limit.to_string(),
    ];
    args.extend(branches.iter().cloned());

    let output = Command::new("git").args(&args).output().await;

    let output = match output {
        Ok(o) if o.status.success() => o,
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
                let parts: Vec<&str> = rest.splitn(3, '|').collect();
                GraphLine {
                    prefix,
                    hash: parts.first().unwrap_or(&"").to_string(),
                    refs: parts.get(1).unwrap_or(&"").trim().to_string(),
                    message: parts.get(2).unwrap_or(&"").to_string(),
                }
            }
            None => GraphLine {
                prefix: line.to_string(),
                hash: String::new(),
                refs: String::new(),
                message: String::new(),
            },
        })
        .collect()
}

/// Check if a branch is fast-forward-able (not diverged).
pub async fn is_ff_possible(repo_path: &Path, branch: &str, upstream: &str) -> bool {
    // If the merge-base of branch and upstream equals branch, it's ff-able
    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "merge-base",
            "--is-ancestor",
            branch,
            upstream,
        ])
        .status()
        .await;

    output.map(|s| s.success()).unwrap_or(false)
}

/// Execute `git pull --ff-only` on a repo.
pub async fn pull_ff_only(repo_path: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", &repo_path.to_string_lossy(), "pull", "--ff-only"])
        .output()
        .await
        .map_err(|e| format!("failed to run git pull: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Get the current branch name.
pub async fn current_branch(repo_path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["-C", &repo_path.to_string_lossy(), "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .await
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Get list of branches that have been merged into the default branch.
pub async fn merged_branches(repo_path: &Path, default_branch: &str) -> Vec<String> {
    let output = Command::new("git")
        .args([
            "-C", &repo_path.to_string_lossy(),
            "branch", "--merged", default_branch, "--format=%(refname:short)",
        ])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.trim().to_string())
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Get all local branch names.
pub async fn all_branches(repo_path: &Path) -> Vec<String> {
    let output = Command::new("git")
        .args([
            "-C", &repo_path.to_string_lossy(),
            "branch", "--format=%(refname:short)",
        ])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.trim().to_string())
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Delete a local branch.
pub async fn delete_branch(repo_path: &Path, branch: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", &repo_path.to_string_lossy(), "branch", "-d", branch])
        .output()
        .await
        .map_err(|e| format!("failed to delete branch: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// List stash entries.
pub async fn stash_list(repo_path: &Path) -> Vec<String> {
    let output = Command::new("git")
        .args(["-C", &repo_path.to_string_lossy(), "stash", "list", "--format=%s"])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect()
        }
        _ => Vec::new(),
    }
}

/// List active worktrees (excluding the main one).
pub async fn worktree_list(repo_path: &Path) -> Vec<crate::model::Worktree> {
    let output = Command::new("git")
        .args(["-C", &repo_path.to_string_lossy(), "worktree", "list", "--porcelain"])
        .output()
        .await;

    let output = match output {
        Ok(o) if o.status.success() => o,
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
        let output = "COMMIT|a1b2c3d|Alice|feat: add feature\nCOMMIT|e4f5g6h|Bob|fix: bug\n";
        let commits = parse_commit_output(output);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "a1b2c3d");
        assert_eq!(commits[0].author, "Alice");
        assert_eq!(commits[0].message, "feat: add feature");
        assert_eq!(commits[0].insertions, 0);
    }

    #[test]
    fn parse_commit_output_with_stat() {
        let output = "COMMIT|a1b2c3d|Alice|feat: add feature\n 3 files changed, 18 insertions(+), 4 deletions(-)\nCOMMIT|e4f5g6h|Bob|fix: bug\n 1 file changed, 2 insertions(+)\n";
        let commits = parse_commit_output(output);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].insertions, 18);
        assert_eq!(commits[0].deletions, 4);
        assert_eq!(commits[1].insertions, 2);
        assert_eq!(commits[1].deletions, 0);
    }

    #[test]
    fn extract_stat_number_works() {
        assert_eq!(
            extract_stat_number(" 3 files changed, 18 insertions(+), 4 deletions(-)", "insertion"),
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
        let output = "* GRAPH|a1b2c3d| (HEAD -> main, origin/main)|feat: add feature";
        let lines = parse_graph_output(output);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].prefix, "* ");
        assert_eq!(lines[0].hash, "a1b2c3d");
        assert_eq!(lines[0].refs, "(HEAD -> main, origin/main)");
        assert_eq!(lines[0].message, "feat: add feature");
    }

    #[test]
    fn parse_graph_output_connector_line_has_no_commit_data() {
        let output = "* GRAPH|a1b2c3d||feat: add feature\n|\\  \n| * GRAPH|d4e5f6g||other branch commit";
        let lines = parse_graph_output(output);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1].prefix, "|\\  ");
        assert!(lines[1].hash.is_empty());
        assert!(lines[1].message.is_empty());
        assert_eq!(lines[2].prefix, "| * ");
        assert_eq!(lines[2].hash, "d4e5f6g");
    }
}
