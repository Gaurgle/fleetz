use crate::git;
use crate::model::RepoStatus;
use std::path::Path;

/// Outcome of a single pull attempt.
#[derive(Debug)]
pub enum PullResult {
    Success { commits_pulled: u32 },
    SkippedDirty,
    SkippedDiverged,
    SkippedNoPath,
    SkippedSynced,
    Failed(String),
}

impl PullResult {
    pub fn message(&self, name: &str) -> String {
        match self {
            Self::Success { commits_pulled } => {
                format!("pulled {commits_pulled} commits from {name}")
            }
            Self::SkippedDirty => format!("{name}: uncommitted changes, skipping"),
            Self::SkippedDiverged => format!("{name}: branch diverged, manual merge needed"),
            Self::SkippedNoPath => format!("{name}: not cloned locally"),
            Self::SkippedSynced => format!("{name}: already synced"),
            Self::Failed(e) => format!("{name}: pull failed — {e}"),
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }
}

/// Check if a repo is a valid pull candidate.
pub fn can_pull(repo: &RepoStatus) -> PullCandidate {
    if repo.path.is_none() {
        return PullCandidate::No(PullResult::SkippedNoPath);
    }
    if repo.dirty > 0 {
        return PullCandidate::No(PullResult::SkippedDirty);
    }
    if repo.behind == 0 {
        return PullCandidate::No(PullResult::SkippedSynced);
    }
    // We check for divergence (ahead > 0 while also behind > 0)
    if repo.ahead > 0 && repo.behind > 0 {
        return PullCandidate::No(PullResult::SkippedDiverged);
    }
    PullCandidate::Yes
}

#[derive(Debug)]
pub enum PullCandidate {
    Yes,
    No(PullResult),
}

/// Pull a single repo (ff-only). Returns the result.
pub async fn pull_single(repo: &RepoStatus) -> PullResult {
    match can_pull(repo) {
        PullCandidate::No(result) => return result,
        PullCandidate::Yes => {}
    }

    let path = Path::new(repo.path.as_ref().unwrap());
    let behind_before = repo.behind;

    match git::pull_ff_only(path).await {
        Ok(_) => PullResult::Success {
            commits_pulled: behind_before,
        },
        Err(e) => PullResult::Failed(e),
    }
}

/// Build the list of candidates for bulk pull.
pub fn bulk_pull_candidates(repos: &[RepoStatus]) -> BulkPullPlan {
    let mut candidates = Vec::new();
    let mut skipped_dirty = 0u32;
    let mut skipped_diverged = 0u32;
    let mut skipped_synced = 0u32;
    let mut skipped_no_path = 0u32;

    for (i, repo) in repos.iter().enumerate() {
        match can_pull(repo) {
            PullCandidate::Yes => candidates.push(i),
            PullCandidate::No(PullResult::SkippedDirty) => skipped_dirty += 1,
            PullCandidate::No(PullResult::SkippedDiverged) => skipped_diverged += 1,
            PullCandidate::No(PullResult::SkippedSynced) => skipped_synced += 1,
            PullCandidate::No(PullResult::SkippedNoPath) => skipped_no_path += 1,
            PullCandidate::No(_) => {}
        }
    }

    BulkPullPlan {
        candidate_indices: candidates,
        skipped_dirty,
        skipped_diverged,
        skipped_synced,
        skipped_no_path,
        total: repos.len() as u32,
    }
}

#[derive(Debug)]
pub struct BulkPullPlan {
    pub candidate_indices: Vec<usize>,
    pub skipped_dirty: u32,
    pub skipped_diverged: u32,
    pub skipped_synced: u32,
    pub skipped_no_path: u32,
    pub total: u32,
}

impl BulkPullPlan {
    pub fn confirmation_message(&self) -> String {
        let n = self.candidate_indices.len();
        let mut skip_parts = Vec::new();
        if self.skipped_dirty > 0 {
            skip_parts.push(format!("{} dirty", self.skipped_dirty));
        }
        if self.skipped_diverged > 0 {
            skip_parts.push(format!("{} diverged", self.skipped_diverged));
        }
        if self.skipped_synced > 0 {
            skip_parts.push(format!("{} synced", self.skipped_synced));
        }
        if self.skipped_no_path > 0 {
            skip_parts.push(format!("{} not local", self.skipped_no_path));
        }
        let skipped = if skip_parts.is_empty() {
            String::new()
        } else {
            format!(" ({} — skipped)", skip_parts.join(", "))
        };
        format!(
            "Pull {} of {} repos?{skipped}",
            n, self.total
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RepoLocation;

    fn repo(name: &str, behind: u32, ahead: u32, dirty: u32, has_path: bool) -> RepoStatus {
        RepoStatus {
            name: name.to_string(),
            path: if has_path {
                Some(format!("~/repos/{name}"))
            } else {
                None
            },
            location: RepoLocation::Primary,
            branches: vec![],
            commits: vec![],
            behind,
            ahead,
            dirty,
            pr_count: 0,
            pushed_at: None,
        }
    }

    #[test]
    fn can_pull_clean_behind_repo() {
        let r = repo("good", 3, 0, 0, true);
        assert!(matches!(can_pull(&r), PullCandidate::Yes));
    }

    #[test]
    fn skips_dirty_repo() {
        let r = repo("dirty", 3, 0, 2, true);
        assert!(matches!(
            can_pull(&r),
            PullCandidate::No(PullResult::SkippedDirty)
        ));
    }

    #[test]
    fn skips_diverged_repo() {
        let r = repo("diverged", 3, 1, 0, true);
        assert!(matches!(
            can_pull(&r),
            PullCandidate::No(PullResult::SkippedDiverged)
        ));
    }

    #[test]
    fn skips_synced_repo() {
        let r = repo("synced", 0, 0, 0, true);
        assert!(matches!(
            can_pull(&r),
            PullCandidate::No(PullResult::SkippedSynced)
        ));
    }

    #[test]
    fn skips_no_path() {
        let r = repo("remote", 3, 0, 0, false);
        assert!(matches!(
            can_pull(&r),
            PullCandidate::No(PullResult::SkippedNoPath)
        ));
    }

    #[test]
    fn bulk_plan_categorizes_correctly() {
        let repos = vec![
            repo("pullable1", 2, 0, 0, true),
            repo("pullable2", 5, 0, 0, true),
            repo("dirty", 3, 0, 2, true),
            repo("diverged", 1, 1, 0, true),
            repo("synced", 0, 0, 0, true),
            repo("remote", 1, 0, 0, false),
        ];
        let plan = bulk_pull_candidates(&repos);
        assert_eq!(plan.candidate_indices, vec![0, 1]);
        assert_eq!(plan.skipped_dirty, 1);
        assert_eq!(plan.skipped_diverged, 1);
        assert_eq!(plan.skipped_synced, 1);
        assert_eq!(plan.skipped_no_path, 1);
    }

    #[test]
    fn confirmation_message_format() {
        let repos = vec![
            repo("pullable", 2, 0, 0, true),
            repo("dirty", 3, 0, 2, true),
            repo("synced", 0, 0, 0, true),
        ];
        let plan = bulk_pull_candidates(&repos);
        let msg = plan.confirmation_message();
        assert!(msg.contains("Pull 1 of 3 repos?"));
        assert!(msg.contains("1 dirty"));
        assert!(msg.contains("1 synced"));
    }

    #[test]
    fn pull_result_messages() {
        assert!(PullResult::Success { commits_pulled: 3 }
            .message("repo")
            .contains("pulled 3 commits"));
        assert!(PullResult::SkippedDirty
            .message("repo")
            .contains("uncommitted"));
        assert!(PullResult::SkippedDiverged
            .message("repo")
            .contains("diverged"));
    }
}
