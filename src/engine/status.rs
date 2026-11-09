//! One row's status: probing a checkout into a `RepoStatus`, the local-only
//! and dirty-mode rows, and the display path a row is keyed by.

use super::carriers::{carries, names_remote};
use crate::cli::Args;
use crate::model::{self, RepoLocation, RepoStatus};
use crate::{discovery, git, github};
use std::path::Path;

/// How many commits/reflog entries to show in the detail pane's history
/// sections. Kept modest -- the pane has no scrolling, and these share it
/// with branches, CI, PRs, and stashes.
/// Rows fetched for the `commits`, `reflog` and `graph` views, which are
/// meant to be complete rather than a glance.
///
/// Affordable because these probes are already gated on `WorkKey`: a repo
/// whose refs, HEAD, index, shallow boundary and HEAD and stash reflogs have
/// not moved is skipped entirely, so the deeper log is paid once per actual
/// change rather than once per sweep.
pub(crate) const HISTORY_COUNT: u32 = 200;

/// Format a path for display, using `~/` shorthand when it's under the home dir.
/// Unlike a bare `~{suffix}` concatenation, this always produces a string
/// `shellexpand::tilde` can correctly re-expand back to the original path.
pub(crate) fn tilde_display(path: &Path) -> String {
    let home = dirs::home_dir().unwrap_or_default();
    match path.strip_prefix(&home) {
        Ok(rel) if !rel.as_os_str().is_empty() => format!("~/{}", rel.display()),
        _ => path.display().to_string(),
    }
}

/// GitHub's `diskUsage` for the remote `repo` carries, from `dataset`: the
/// size the fetch cap's pre-check reads (FZ-35b). `None` for a row that
/// carries no entry, or an entry from a payload without the field.
pub(crate) fn disk_usage_of(repo: &RepoStatus, dataset: &[github::GithubRepo]) -> Option<u64> {
    dataset
        .iter()
        .find(|g| carries(repo, g))
        .and_then(|g| g.disk_usage)
}

/// One repo's full local status for `-d` mode. Returns None for a repo with
/// nothing uncommitted and nothing unpushed: dirty mode only lists work in
/// progress.
pub(super) async fn dirty_mode_status(path: &Path, author: Option<&str>) -> Option<RepoStatus> {
    let dirty = git::dirty_count(path).await;
    let branches = git::branch_statuses(path).await;
    let total_ahead: u32 = branches.iter().map(|b| b.ahead).sum();

    if dirty == 0 && total_ahead == 0 {
        return None;
    }

    let name = discovery::repo_name_for_path(path);
    let display = tilde_display(path);

    // Always fetch the diffstat for recent commits so every commit
    // (not just those in a dirty repo) shows its +/- and file list.
    let branch_names: Vec<String> = branches.iter().map(|b| b.name.clone()).collect();
    let commits = git::commits(
        path,
        &format!("-{HISTORY_COUNT}"),
        true,
        author,
        &branch_names,
    )
    .await;
    let reflog = git::reflog(path, HISTORY_COUNT).await;
    let graph = git::graph(path, &branch_names, HISTORY_COUNT).await;
    let graph_expanded = git::graph_expanded(path, &branch_names, HISTORY_COUNT).await;
    let stash_entries = git::stash_list(path).await;
    let stash_count = stash_entries.len() as u32;
    let worktrees = git::worktree_list(path).await;
    let remote_url = git::remote_url(path).await;
    let remote = git::remote_branches(path).await;
    let remote_only_branches = git::unfetched_branches(&remote, &branches);

    Some(RepoStatus {
        // No local calendar until the strip asks for one.
        daily: None,
        daily_key: None,
        last_local: None,
        name,
        path: Some(display),
        location: RepoLocation::LocalOnly,
        branches,
        commits,
        behind: 0,
        ahead: total_ahead,
        dirty,
        pr_count: 0,
        prs: Vec::new(),
        ci_status: model::CiStatus::Unknown,
        ci_workflow: None,
        ci_duration: None,
        groups: vec![],
        stash_count,
        stash_entries,
        worktrees,
        reflog,
        graph,
        graph_expanded,
        repo_kind: model::RepoKind::Unknown,
        owner: None,
        remote_url,
        remote_only_branches,
        remote_branches: remote,
        work_key: None,
        fetch: model::FetchFacts::from_disk(git::last_successful_fetch_at(path)),
        authors: Default::default(),
    })
}

/// Probe a local repo that no GitHub match claimed. Returns None when it has
/// neither uncommitted nor unpushed work, which is the signal that it is not
/// worth a row: a clean local clone is not news.
/// Whether a local-only repo still belongs in "local changes" under `cutoff`.
///
/// The section cannot be gated on `pushed_at` the way "github activity" is:
/// its whole point is work the remote has never seen, and a repo that was
/// never pushed has no such timestamp. So it is gated on the local clock
/// instead, which is what `git::last_local_activity` reports.
///
/// Unpushed commits are exempt. Untracked files left lying around are noise
/// you chose to ignore, but a commit that exists only on this machine is one
/// `git push` from being lost with it, and a narrow window is not a reason to
/// stop saying so.
///
/// No cutoff means no filtering, which is how the "all" window and `--all`
/// keep showing everything the narrower windows evicted.
pub(super) fn local_repo_is_current(
    ahead: u32,
    last_local: Option<chrono::DateTime<chrono::Utc>>,
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
) -> bool {
    let Some(cutoff) = cutoff else { return true };
    if ahead > 0 {
        return true;
    }
    last_local.is_some_and(|at| at > cutoff)
}

pub(super) async fn local_only_status(
    path: &Path,
    name: String,
    repo_kind: model::RepoKind,
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
) -> Option<RepoStatus> {
    let dirty = git::dirty_count(path).await;
    let branches = git::branch_statuses(path).await;
    let total_ahead: u32 = branches.iter().map(|b| b.ahead).sum();

    if dirty == 0 && total_ahead == 0 {
        return None;
    }

    // Dirty is not the same as current. A repo `git init`-ed and abandoned
    // reports dirty forever, and before this it held a row forever with it.
    // Kept rather than discarded: `info` draws the same value, so the date on
    // screen is the one the window judged the repo by.
    let last_local = git::last_local_activity(path).await;
    if !local_repo_is_current(total_ahead, last_local, cutoff) {
        return None;
    }

    let display = tilde_display(path);
    let remote_url = git::remote_url(path).await;
    let remote = git::remote_branches(path).await;
    let remote_only_branches = git::unfetched_branches(&remote, &branches);

    Some(RepoStatus {
        // No local calendar until the strip asks for one.
        daily: None,
        daily_key: None,
        last_local,
        name,
        path: Some(display),
        location: RepoLocation::LocalOnly,
        branches,
        commits: Vec::new(),
        behind: 0,
        ahead: total_ahead,
        dirty,
        pr_count: 0,
        prs: Vec::new(),
        ci_status: model::CiStatus::Unknown,
        ci_workflow: None,
        ci_duration: None,
        groups: vec![],
        stash_count: 0,
        stash_entries: vec![],
        worktrees: vec![],
        reflog: vec![],
        graph: vec![],
        graph_expanded: Vec::new(),
        repo_kind,
        // No GitHub match, so the remote URL is the only identity available;
        // `owner_display` parses it at render time.
        owner: None,
        remote_url,
        remote_only_branches,
        remote_branches: remote,
        work_key: None,
        fetch: model::FetchFacts::from_disk(git::last_successful_fetch_at(path)),
        authors: Default::default(),
    })
}

/// Build a full RepoStatus from a discovered local repo.
pub(super) async fn build_repo_status(
    found: &discovery::FoundRepo,
    gh_repos: &[github::GithubRepo],
    args: &Args,
    location: RepoLocation,
) -> RepoStatus {
    let path = &found.path;
    // Every key input is read before the probes the key gates, so a change
    // landing while they run is in the probes' output or moves the next
    // sweep's key, never recorded in the key alone.
    let local = git::try_branch_statuses(path).await;
    let dirty = git::dirty_count(path).await;
    let other = git::try_ref_listing(path).await;
    let work_key = git::work_key_from(path, local.as_ref().ok(), other.as_ref().ok()).await;
    let branches = local.map(|l| l.statuses).unwrap_or_default();
    let remote = other.map(|o| o.remotes).unwrap_or_default();

    let mut total_behind = 0u32;
    let mut total_ahead = 0u32;
    for b in &branches {
        total_behind += b.behind;
        total_ahead += b.ahead;
    }

    let author = args.author.as_deref();

    // Recent history across the local branches, not just the divergent commits
    // -- a glance at "what's been happening here" shouldn't depend on being
    // ahead/behind, and the graph beside it draws the same refs.
    // Always with the diffstat so every commit shows its +/- and file list.
    let branch_names: Vec<String> = branches.iter().map(|b| b.name.clone()).collect();
    let commits = git::commits(
        path,
        &format!("-{HISTORY_COUNT}"),
        true,
        author,
        &branch_names,
    )
    .await;
    let reflog = git::reflog(path, HISTORY_COUNT).await;

    let graph = git::graph(path, &branch_names, HISTORY_COUNT).await;
    let graph_expanded = git::graph_expanded(path, &branch_names, HISTORY_COUNT).await;

    // The repo discovery matched this clone to, by owner and name: the first
    // same-named entry could be another owner's.
    let gh_match = gh_repos
        .iter()
        .find(|r| names_remote(found.owner.as_deref(), &found.name, r));

    let pr_count = gh_match.map_or(0, |r| r.pr_count);
    let prs = gh_match.map_or_else(Vec::new, |r| r.prs.clone());
    let ci_status = gh_match.map_or(model::CiStatus::Unknown, |r| r.ci_status.clone());
    let ci_workflow = gh_match.and_then(|r| r.ci_workflow.clone());
    let ci_duration = gh_match.and_then(|r| r.ci_duration.clone());
    let repo_kind = gh_match.map_or(model::RepoKind::Unknown, |r| r.repo_kind);
    let owner = gh_match.and_then(|r| r.owner.clone());

    let display = tilde_display(path);

    // Stash + worktree info
    let stash_entries = git::stash_list(path).await;
    let stash_count = stash_entries.len() as u32;
    let worktrees = git::worktree_list(path).await;

    let remote_url = git::remote_url(path).await;
    let remote_only_branches = git::unfetched_branches(&remote, &branches);

    RepoStatus {
        // No local calendar until the strip asks for one.
        daily: None,
        daily_key: None,
        last_local: None,
        name: found.name.clone(),
        path: Some(display),
        location,
        branches,
        commits,
        behind: total_behind,
        ahead: total_ahead,
        dirty,
        pr_count,
        prs,
        ci_status,
        ci_workflow,
        ci_duration,
        groups: vec![],
        stash_count,
        stash_entries,
        worktrees,
        reflog,
        graph,
        graph_expanded,
        repo_kind,
        owner,
        remote_url,
        remote_only_branches,
        remote_branches: remote,
        work_key,
        fetch: model::FetchFacts::from_disk(git::last_successful_fetch_at(path)),
        authors: Default::default(),
    }
}
