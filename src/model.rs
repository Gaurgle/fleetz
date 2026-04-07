use chrono::{DateTime, Utc};

/// Status of a single git branch relative to its upstream.
#[derive(Debug, Clone)]
pub struct BranchStatus {
    pub name: String,
    pub upstream: Option<String>,
    pub behind: u32,
    pub ahead: u32,
}

/// A single commit.
#[derive(Debug, Clone)]
pub struct Commit {
    pub hash: String,
    pub message: String,
    pub author: String,
    pub insertions: u32,
    pub deletions: u32,
}

/// Where a repo was discovered relative to the configured repo_dir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoLocation {
    /// Found under the configured repo directory (pass 1).
    Primary,
    /// Found elsewhere on the machine (pass 2).
    Elsewhere,
    /// Only has local changes, not active on GitHub (pass 3).
    LocalOnly,
    /// Active on GitHub but not cloned locally.
    NotCloned,
}

/// Aggregate status for a single repository.
#[derive(Debug, Clone)]
pub struct RepoStatus {
    pub name: String,
    pub path: Option<String>,
    pub location: RepoLocation,
    pub branches: Vec<BranchStatus>,
    pub commits: Vec<Commit>,
    pub behind: u32,
    pub ahead: u32,
    pub dirty: u32,
    pub pr_count: u32,
    pub pushed_at: Option<DateTime<Utc>>,
}

impl RepoStatus {
    pub fn is_synced(&self) -> bool {
        self.behind == 0 && self.ahead == 0 && self.dirty == 0
    }

    pub fn has_divergence(&self) -> bool {
        self.behind > 0 || self.ahead > 0
    }

    /// A short status string like "3↓", "2↑", "4!", or "synced".
    pub fn short_status(&self) -> String {
        let mut parts = Vec::new();
        if self.behind > 0 {
            parts.push(format!("{}↓", self.behind));
        }
        if self.ahead > 0 {
            parts.push(format!("{}↑", self.ahead));
        }
        if self.dirty > 0 {
            parts.push(format!("{}!", self.dirty));
        }
        if parts.is_empty() {
            "synced".to_string()
        } else {
            parts.join(" ")
        }
    }

    /// A longer status string like "3 behind, 2 ahead, 4 uncommitted".
    pub fn long_status(&self) -> String {
        let mut parts = Vec::new();
        if self.behind > 0 {
            parts.push(format!("{} behind", self.behind));
        }
        if self.ahead > 0 {
            parts.push(format!("{} ahead", self.ahead));
        }
        if self.dirty > 0 {
            parts.push(format!("{} uncommitted", self.dirty));
        }
        if parts.is_empty() {
            "synced".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// Aggregate summary across all repos.
#[derive(Debug, Default)]
pub struct Summary {
    pub behind_count: u32,
    pub ahead_count: u32,
    pub dirty_count: u32,
    pub synced_count: u32,
    pub not_local_count: u32,
    pub pr_count: u32,
}

impl Summary {
    pub fn from_repos(repos: &[RepoStatus]) -> Self {
        let mut s = Self::default();
        for r in repos {
            if r.behind > 0 {
                s.behind_count += 1;
            }
            if r.ahead > 0 {
                s.ahead_count += 1;
            }
            if r.dirty > 0 {
                s.dirty_count += 1;
            }
            if r.location == RepoLocation::NotCloned {
                s.not_local_count += 1;
            } else if r.is_synced() {
                s.synced_count += 1;
            }
            s.pr_count += r.pr_count;
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_repo(name: &str, behind: u32, ahead: u32, dirty: u32) -> RepoStatus {
        RepoStatus {
            name: name.to_string(),
            path: Some(format!("~/repos/{name}")),
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
    fn synced_repo_reports_synced() {
        let r = make_repo("clean", 0, 0, 0);
        assert!(r.is_synced());
        assert!(!r.has_divergence());
        assert_eq!(r.short_status(), "synced");
        assert_eq!(r.long_status(), "synced");
    }

    #[test]
    fn behind_repo_status_strings() {
        let r = make_repo("behind", 3, 0, 0);
        assert!(!r.is_synced());
        assert!(r.has_divergence());
        assert_eq!(r.short_status(), "3↓");
        assert_eq!(r.long_status(), "3 behind");
    }

    #[test]
    fn mixed_status_strings() {
        let r = make_repo("messy", 2, 1, 4);
        assert_eq!(r.short_status(), "2↓ 1↑ 4!");
        assert_eq!(r.long_status(), "2 behind, 1 ahead, 4 uncommitted");
    }

    #[test]
    fn summary_counts_correctly() {
        let repos = vec![
            make_repo("a", 3, 0, 0),
            make_repo("b", 0, 0, 0),
            make_repo("c", 0, 2, 1),
            {
                let mut r = make_repo("d", 0, 0, 0);
                r.location = RepoLocation::NotCloned;
                r.path = None;
                r
            },
        ];
        let s = Summary::from_repos(&repos);
        assert_eq!(s.behind_count, 1);
        assert_eq!(s.ahead_count, 1);
        assert_eq!(s.dirty_count, 1);
        assert_eq!(s.synced_count, 1);
        assert_eq!(s.not_local_count, 1);
    }
}
