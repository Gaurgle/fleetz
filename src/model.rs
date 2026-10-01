use chrono::{DateTime, Utc};

/// Status of a single git branch relative to its upstream.
#[derive(Debug, Clone)]
pub struct BranchStatus {
    pub name: String,
    pub upstream: Option<String>,
    pub behind: u32,
    pub ahead: u32,
    /// The branch HEAD is currently on, from `for-each-ref`'s `%(HEAD)`.
    ///
    /// Nothing used to record it, so every local branch drew identically and
    /// the pane could not answer the first question anyone asks it.
    pub is_head: bool,
    /// The branch's tip commit, short form, from `for-each-ref`'s
    /// `%(objectname:short)`.
    ///
    /// Rides the query that was already being made, the same way `is_head`
    /// does. It is what lets the branches view hand the inspector a commit.
    /// Empty in a repo with no commits, where there is no object to name.
    pub tip: String,
}

/// A remote-tracking branch: its short name (`origin/feature-x`) and the tip
/// it points at.
///
/// The tip rides the `for-each-ref` that was already listing these, the same
/// way `BranchStatus::tip` does. Note that a remote branch fleetz calls "not
/// fetched" still has a local object behind it: the phrase means no local
/// *branch* claims it, not that the ref is absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBranch {
    pub name: String,
    pub tip: String,
}

/// One file touched by a commit, from `git log --numstat`. `added`/`removed`
/// are line counts (both 0 for a binary file, which numstat reports as `-`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    pub path: String,
    pub added: u32,
    pub removed: u32,
}

/// A single commit. `message` is the subject line; `body` is the rest of the
/// commit message (empty for single-line commits). `date` is a relative
/// string like "3 days ago". `author` is the full author name -- the tight
/// lists show a short code (ui::author_code), but the inspector shows this.
/// `files` is the per-file diffstat, shown in the inspector (empty for merge
/// commits, which have no single diffstat).
#[derive(Debug, Clone)]
pub struct Commit {
    pub hash: String,
    pub message: String,
    pub body: String,
    pub author: String,
    /// Author timestamp, absolute. Stored absolute rather than as a
    /// pre-rendered "3 minutes ago" so the age is computed at draw time and
    /// stays current between sweeps. None when git gave an unparseable value.
    pub date: Option<DateTime<Utc>>,
    pub insertions: u32,
    pub deletions: u32,
    pub files: Vec<FileStat>,
}

/// CI/check status for the default branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiStatus {
    Passed,
    Failed,
    Pending,
    Unknown,
}

impl CiStatus {
    /// CI renders as a ringed disk `◉` in every state, distinguished by color:
    /// green passed, red failed, orange pending (which blinks under watch
    /// mode). One shape, so it can never be confused with the sync status's
    /// `✓` the way the old checkmark could.
    ///
    /// The ring reads as a status lamp, and carries further at one cell than
    /// a plain `●`.
    ///
    /// The cost is that passed and failed differ by color alone. The detail
    /// pane spells the state out in words next to the lamp, which is where
    /// that information is recoverable without color.
    pub fn symbol(&self) -> &str {
        match self {
            Self::Passed | Self::Failed | Self::Pending => "◉",
            Self::Unknown => "",
        }
    }
}

/// An open pull request.
#[derive(Debug, Clone)]
pub struct PullRequest {
    pub number: u32,
    pub title: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
    pub review_status: ReviewStatus,
    pub url: String,
}

/// Review status of a PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewStatus {
    Approved,
    ChangesRequested,
    ReviewRequired,
    Unknown,
}

impl ReviewStatus {
    pub fn label(&self) -> &str {
        match self {
            Self::Approved => "approved",
            Self::ChangesRequested => "changes requested",
            Self::ReviewRequired => "review needed",
            Self::Unknown => "",
        }
    }
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

/// The three fetch facts of `docs/fetch-outcome-contract.md` for one
/// checkout: when its remote refs were last actually updated, when a fetch
/// last finished, and how that ended. The same shape as
/// `app::GithubFreshness`, for the same reason: a fetch that was asked for is
/// not one that worked, and only a success may move the time the counts are
/// measured against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchFacts {
    /// When a fetch last succeeded. At load, from a non-empty `FETCH_HEAD`
    /// (`git::last_successful_fetch_at`); `None` is unknown and is never
    /// drawn as a time.
    pub last_success: Option<DateTime<chrono::Local>>,
    /// When a fetch last finished, however it ended.
    pub last_attempt: Option<DateTime<chrono::Local>>,
    pub outcome: crate::git::FetchOutcome,
}

impl FetchFacts {
    /// What a freshly loaded row knows: the disk's last success, no attempt.
    pub fn from_disk(last_success: Option<std::time::SystemTime>) -> Self {
        Self {
            last_success: last_success.map(DateTime::from),
            ..Self::default()
        }
    }

    /// A fetch for this checkout finished. Only a success advances
    /// `last_success` (rule 3); `NotAttempted` changes nothing (rule 2).
    pub fn record(&mut self, outcome: &crate::git::FetchOutcome, now: DateTime<chrono::Local>) {
        use crate::git::FetchOutcome;
        match outcome {
            FetchOutcome::NotAttempted => return,
            FetchOutcome::Succeeded => self.last_success = Some(now),
            FetchOutcome::Failed(_) => {}
        }
        self.last_attempt = Some(now);
        self.outcome = outcome.clone();
    }

    /// The failure, while the last fetch failed. Behind and ahead are then
    /// counted against the remote refs of `last_success` (rule 5).
    pub fn failure(&self) -> Option<&crate::git::FetchFailure> {
        match &self.outcome {
            crate::git::FetchOutcome::Failed(failure) => Some(failure),
            _ => None,
        }
    }

    /// Carry what this session learned about fetching across a load that
    /// rebuilt the row (`R`, a time-window apply). Without it a window apply,
    /// which fetches nothing, wiped a failure off the row and drew stale
    /// counts as current, and an `R` whose own fetch failed forgot the last
    /// success the session had seen.
    ///
    /// `self` is the replaced row's facts, `loaded` the new row's: the disk's
    /// last success plus any fetch the load ran. A disk success newer than
    /// anything this session saw (a `git fetch` run in a terminal) is a
    /// success all the same; the load's own fetch is recorded last, and only
    /// when it is newer than the session's last attempt.
    pub fn carried_into(&self, loaded: &FetchFacts) -> FetchFacts {
        let mut kept = self.clone();
        let newer = |at: DateTime<chrono::Local>, than: Option<DateTime<chrono::Local>>| {
            than.is_none_or(|t| at > t)
        };
        if let Some(at) = loaded.last_success
            && newer(at, kept.last_success)
            && newer(at, kept.last_attempt)
        {
            kept.record(&crate::git::FetchOutcome::Succeeded, at);
        }
        // Only when newer than what the session already has: an `R` whose
        // fetch finished before a sweep's fetch of the same repo landed must
        // not move `last_attempt` back or put its older outcome on top.
        if let Some(at) = loaded.last_attempt
            && newer(at, kept.last_attempt)
        {
            kept.record(&loaded.outcome, at);
        }
        kept
    }
}

/// Aggregate status for a single repository.
///
/// Four writers own disjoint sets of fields, and each writes only its own:
///
/// - **GitHub poll** (`apply_github`): `pr_count`, `prs`, `ci_status`,
///   `ci_workflow`, `ci_duration`, `repo_kind`.
/// - **Local sweep** (`apply_local_sweep`): `branches`, `commits`, `behind`,
///   `ahead`, `dirty`, `stash_count`, `stash_entries`, `worktrees`, `reflog`,
///   `graph`, `graph_expanded`, `remote_branches`, `remote_only_branches`,
///   `daily`, `daily_key`, `work_key`.
/// - **Fetch** (`FetchFacts::record`, when a fetch lands): `fetch`. A sweep
///   that did not fetch leaves it alone (rule 2 of the fetch contract).
/// - **Load** (discovery and `assemble_view`, which rebuild the whole list):
///   everything else, including `owner`, which comes from GitHub data but is
///   only ever set at load.
///
/// Background results land on the row as it is when they arrive, so a slow
/// writer never restores another writer's fields from an older snapshot.
#[derive(Debug, Clone)]
pub struct RepoStatus {
    /// When anything last moved in the local clone: the newer of the last
    /// commit on any ref and the newest change in the working tree
    /// (`git::last_local_activity`).
    ///
    /// Only filled for local-only repos, the ones with no `pushedAt` to be
    /// dated by. It is what the time window tests to decide whether such a
    /// repo keeps its row, and `info` draws the same value so an eviction is
    /// legible rather than mysterious. `None` where nothing computed it, or
    /// in a repo with no commits and a clean tree.
    pub last_local: Option<chrono::DateTime<chrono::Utc>>,
    pub name: String,
    pub path: Option<String>,
    pub location: RepoLocation,
    pub branches: Vec<BranchStatus>,
    pub commits: Vec<Commit>,
    pub behind: u32,
    pub ahead: u32,
    pub dirty: u32,
    pub pr_count: u32,
    pub prs: Vec<PullRequest>,
    pub ci_status: CiStatus,
    pub ci_workflow: Option<String>,
    pub ci_duration: Option<String>,
    pub groups: Vec<String>,
    pub stash_count: u32,
    pub stash_entries: Vec<String>,
    pub worktrees: Vec<Worktree>,
    pub reflog: Vec<ReflogEntry>,
    pub graph: Vec<GraphLine>,
    /// `git log --graph` in its expanded form, as raw lines. See
    /// `git::graph_expanded`.
    pub graph_expanded: Vec<String>,
    pub repo_kind: RepoKind,
    /// Owner login as reported by the GitHub API, when a fetch matched this
    /// repo. Local-only clones leave it None and fall back to the remote URL
    /// via `owner_display`.
    pub owner: Option<String>,
    /// URL of the `origin` remote, if configured.
    pub remote_url: Option<String>,
    /// Every remote-tracking branch, fetched or not.
    ///
    /// Free: `git::remote_branches` was already run during the sweep and its
    /// result reduced to `remote_only_branches` and discarded. The branches
    /// view lists all of them, so the whole list is kept instead.
    pub remote_branches: Vec<RemoteBranch>,
    /// Remote-tracking branches (e.g. "origin/feature-x") with no local
    /// branch of the same name -- never fetched/checked out locally, or
    /// checked out once and deleted locally after merging.
    pub remote_only_branches: Vec<String>,
    /// Per-day commit counts for this repo over the contribution grid, or
    /// None when the local calendar has never been asked for. Boxed so a
    /// repo that will never need it costs one pointer rather than 742 bytes.
    pub daily: Option<Box<crate::contributions::DailyCounts>>,
    /// What `daily` was computed under, set only alongside it. A sweep reuses
    /// `daily` only when its own key equals this one; a sweep without the
    /// histogram probe keeps both as they are. In memory only.
    pub daily_key: Option<HistogramKey>,
    /// Fingerprint of the repo state the expensive probes were taken from.
    /// A sweep that finds the same key reuses commits/reflog/graph/stash
    /// instead of re-running those subprocesses; `worktrees` is re-read on
    /// every sweep, since the key cannot see it. None means "unknown", which
    /// always counts as changed.
    pub work_key: Option<crate::git::WorkKey>,
    /// Whether the remote refs behind `behind` and `ahead` are current: the
    /// fetch facts for this checkout.
    pub fetch: FetchFacts,
}

/// The inputs a local contribution histogram (`RepoStatus::daily`) depends
/// on, so a sweep can tell whether the one it has is still the answer.
///
/// Separate from `WorkKey`: the histogram's slots are offsets from the grid
/// start, so a week rollover changes the answer with nothing in the repo
/// moving, and a sweep without the probe must not record a new `WorkKey`
/// against an old histogram. When `contributions_author` is unset the
/// resolved `user.email` is deliberately not part of the key: it is
/// re-resolved on every re-probe, and an email change alone is an accepted
/// limitation that `R` clears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistogramKey {
    /// Grid start the slots are offsets from.
    pub start: chrono::NaiveDate,
    /// `contributions_author` as configured, `None` when unset.
    pub author: Option<String>,
    /// `WorkKey::ref_signature` from the same sweep.
    pub ref_signature: u64,
    /// `WorkKey::head_sha` from the same sweep. `--all` walks HEAD, and the
    /// signature only records that HEAD is detached, not where.
    pub head_sha: String,
}

/// An active git worktree.
#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: String,
    pub branch: String,
}

/// One entry from `git reflog` -- a HEAD movement, for "what did I just do"
/// recovery glances.
#[derive(Debug, Clone)]
pub struct ReflogEntry {
    pub hash: String,
    pub action: String,
    pub description: String,
}

/// What a decoration on a commit points at. Kept apart because the kinds read
/// as different things at a glance -- where you are, what you have, what the
/// remote has, what is pinned -- and one flat colour for the whole
/// `(HEAD -> main, origin/main, tag: v1)` blob threw that away.
///
/// Classified from `--decorate=full`'s `refs/heads/`, `refs/remotes/` and
/// `refs/tags/` prefixes rather than guessed from the short names, so a local
/// branch called `origin/thing` cannot be mistaken for a remote one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// A detached HEAD, which git decorates as a bare `HEAD`. An *attached*
    /// HEAD is not one of these: it rides on the branch it points at, as
    /// `GitRef::is_head`.
    Head,
    /// A local branch (`refs/heads/`).
    Local,
    /// A remote-tracking branch (`refs/remotes/`).
    Remote,
    /// A tag (`refs/tags/`).
    Tag,
    /// Anything else git decorated with: `refs/stash`, notes, replace refs.
    Other,
}

/// The arrow git draws between an attached HEAD and its branch, and what the
/// renderer measures the decoration by.
pub const HEAD_ARROW: &str = "HEAD -> ";

/// One decoration on a commit: its kind and the name to print, with the
/// `refs/...` prefix already stripped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRef {
    pub kind: RefKind,
    pub name: String,
    /// True for the branch HEAD is attached to, drawn as `HEAD -> name`.
    ///
    /// A flag on the branch rather than a marker of its own, because git
    /// separates the two with an arrow and every other pair with a comma. As
    /// its own list entry it would need the renderer to special-case which
    /// separator follows it, and to get that wrong for a detached HEAD
    /// sitting on a tag (`HEAD, tag: v1`), where the comma is correct.
    pub is_head: bool,
}

impl GitRef {
    /// Classify one comma-separated entry of `%D` under `--decorate=full` and
    /// strip its prefix. Returns None for an empty entry.
    pub fn parse(entry: &str) -> Option<Self> {
        let entry = entry.trim();
        if entry.is_empty() {
            return None;
        }
        // Detached: git decorates it as a bare "HEAD", with nothing to strip.
        if entry == "HEAD" {
            return Some(Self {
                kind: RefKind::Head,
                name: "HEAD".to_string(),
                is_head: false,
            });
        }
        let (is_head, entry) = match entry.strip_prefix(HEAD_ARROW) {
            Some(rest) => (true, rest),
            None => (false, entry),
        };
        // `--decorate=full` spells the ref out but keeps git's own "tag: "
        // marker in front of it, so a tag arrives as "tag: refs/tags/v1.0".
        // Verified against a real tagged commit: assuming the full form
        // dropped the marker left the raw prefix on screen.
        let entry = entry.strip_prefix("tag: ").unwrap_or(entry);
        for (prefix, kind) in [
            ("refs/heads/", RefKind::Local),
            ("refs/remotes/", RefKind::Remote),
            ("refs/tags/", RefKind::Tag),
        ] {
            if let Some(name) = entry.strip_prefix(prefix) {
                return Some(Self {
                    kind,
                    name: name.to_string(),
                    is_head,
                });
            }
        }
        Some(Self {
            kind: RefKind::Other,
            name: entry.trim_start_matches("refs/").to_string(),
            is_head,
        })
    }

    /// Printed width, the `HEAD -> ` arrow included.
    pub fn width(&self) -> usize {
        self.name.chars().count() + if self.is_head { HEAD_ARROW.len() } else { 0 }
    }
}

/// One rendered line of `git log --graph`: the ASCII graph prefix plus the
/// commit it points at. Pure connector lines (no commit on that row) carry
/// an empty hash/refs/author/message and only the prefix.
#[derive(Debug, Clone)]
pub struct GraphLine {
    /// The graph column for this row, one cell per lane, computed from parent
    /// hashes by `crate::graph`. Not git's own `--graph` art: see that
    /// module's header for why.
    pub lanes: Vec<crate::graph::Cell>,
    pub hash: String,
    /// Two or more parents, which is what makes the row draw a merge node.
    /// Comes from `%p`, the only field that can tell a merge from an ordinary
    /// commit.
    pub is_merge: bool,
    pub refs: Vec<GitRef>,
    pub author: String,
    pub message: String,
}

impl GraphLine {
    /// Printed width of the decoration list, parens and separators included,
    /// so the detail pane can budget the message column without rendering the
    /// refs twice and measuring the result.
    pub fn refs_width(&self) -> usize {
        if self.refs.is_empty() {
            return 0;
        }
        // "(a, b)": two parens, plus ", " between every pair.
        let names: usize = self.refs.iter().map(GitRef::width).sum();
        names + 2 + 2 * (self.refs.len() - 1)
    }
}

/// Which kind of GitHub owner a repo belongs to, for the org/private/public
/// glyph shown next to its name (mirrors repoz's glyph logic). `Unknown`
/// means there's no GitHub metadata to go on (local-only repos), so no
/// glyph is shown at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoKind {
    Org,
    Private,
    Public,
    Unknown,
}

/// Pull `(owner, repo)` out of a git remote URL, covering the forms git
/// actually writes: scp-style (`git@host:owner/repo.git`), HTTPS/SSH URLs
/// (`https://host/owner/repo.git`, `ssh://git@host/owner/repo.git`), with or
/// without the `.git` suffix, which is stripped from the repo. Anything that
/// doesn't yield an `owner/repo` pair returns None rather than a guess.
///
/// Self-hosted hosts work too: only the path shape matters, not the host. So
/// does an SSH host alias (`git@github-work:owner/repo.git`), which is what
/// lets discovery match on this without knowing which host a clone uses.
pub fn repo_from_remote_url(url: &str) -> Option<(String, String)> {
    RemoteRef::parse(url).map(|remote| (remote.owner, remote.name))
}

/// What a git remote URL points at: `(host, owner, name)`, each in the
/// spelling the URL used, so it can be displayed as written. Evidence for
/// matching a clone to a GitHub repo, not an identity in itself.
///
/// The host is parsed and carried but nothing matches on it yet: an SSH host
/// alias (`git@github-work:owner/repo.git`) is a real GitHub remote that a
/// strict host check would reject, and resolving it through `~/.ssh/config`
/// costs a parse of `Include` and `Match` or a process per candidate.
/// Carrying it lets a later caller prefer `github.com` among several clones,
/// or scope by host, without parsing the URL again.
///
/// No case folding happens here. A lowercased comparison key arrives in
/// FZ-05b, with its first caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRef {
    /// Without user or port: `github.com` for `ssh://git@github.com:22/...`,
    /// the alias itself for an SSH host alias, empty for `file:///...`. Case
    /// is kept as written (`GitHub.com` stays `GitHub.com`). A bracketed IPv6
    /// host is not understood: `ssh://git@[::1]:22/o/r` yields host `[`,
    /// though owner and name still parse.
    pub host: String,
    pub owner: String,
    /// The repo, with a lowercase `.git` suffix stripped.
    pub name: String,
}

impl RemoteRef {
    /// Parses the forms git writes: scp-style (`git@host:owner/repo.git`) and
    /// URLs with a scheme (`https://host/owner/repo.git`,
    /// `ssh://git@host:port/owner/repo.git`), with or without the `.git`
    /// suffix and a trailing slash. Anything without an `owner/repo` pair is
    /// None rather than a guess. Only the path shape decides; the host can be
    /// anything, so self-hosted servers and aliases parse too.
    pub fn parse(url: &str) -> Option<Self> {
        let url = url.trim().trim_end_matches('/');
        // scp-style has no scheme and puts the path after a colon. Checking
        // for "://" first keeps a URL's own "host:port" colon from matching.
        let (host, path) = if let Some((_, after)) = url.split_once("://") {
            let (authority, path) = after.split_once('/')?;
            let host = without_user(authority);
            (host.split_once(':').map_or(host, |(h, _)| h), path)
        } else {
            let (before, path) = url.split_once(':')?;
            (without_user(before), path)
        };

        let mut segments = path.trim_start_matches('/').split('/');
        let owner = segments.next().filter(|s| !s.is_empty())?;
        // Require a repo segment as well, so a bare host never reads as an
        // owner.
        let name = segments
            .next()
            .map(|s| s.strip_suffix(".git").unwrap_or(s))
            .filter(|s| !s.is_empty())?;
        Some(Self {
            host: host.to_string(),
            owner: owner.to_string(),
            name: name.to_string(),
        })
    }
}

/// `host` from `user@host`, or the input when there is no user.
fn without_user(authority: &str) -> &str {
    authority.rsplit_once('@').map_or(authority, |(_, h)| h)
}

/// The owner half of `repo_from_remote_url`.
pub fn owner_from_remote_url(url: &str) -> Option<String> {
    repo_from_remote_url(url).map(|(owner, _)| owner)
}

/// What a not-cloned repo reports instead of a counter summary, in both the
/// short and long status forms (repoz prints the same words). The footer
/// summary reads it too, so the count and the badge can never disagree about
/// what the state is called.
pub const NOT_CLONED: &str = "not cloned";

impl RepoStatus {
    /// Active on GitHub with no local clone, so its counters are all zero for
    /// want of a working tree rather than because it's up to date.
    pub fn is_not_cloned(&self) -> bool {
        self.location == RepoLocation::NotCloned
    }

    /// True only for a repo that actually exists locally and is level with its
    /// remote. A not-cloned repo is excluded deliberately: every counter reads
    /// zero, which would otherwise render as a green "✓ synced" for a repo
    /// that isn't on disk at all.
    pub fn is_synced(&self) -> bool {
        !self.is_not_cloned() && self.behind == 0 && self.ahead == 0 && self.dirty == 0
    }

    /// The owner to show in the UI: the API login when a GitHub fetch matched
    /// this repo, otherwise whatever the `origin` remote reveals. None when
    /// the repo has neither, which is the "no identity to show" case.
    pub fn owner_display(&self) -> Option<String> {
        if let Some(owner) = &self.owner {
            return Some(owner.clone());
        }
        owner_from_remote_url(self.remote_url.as_deref()?)
    }

    /// Write a GitHub poll's answer for this repo: the GitHub-owned fields
    /// and nothing else (see `RepoStatus` for the split).
    ///
    /// The destructure is exhaustive on purpose, so a field added to
    /// `GithubRepo` does not compile until someone decides whether the poll
    /// writes it.
    pub fn apply_github(&mut self, gh: &crate::github::GithubRepo) {
        let crate::github::GithubRepo {
            // The match key, not data.
            name: _,
            // Read by `remotes_that_moved` from the dataset, never stored here.
            pushed_at: _,
            pr_count,
            prs,
            ci_status,
            ci_workflow,
            ci_duration,
            repo_kind,
            // Filled at load (`assemble_view`); the poll has never rewritten it.
            owner: _,
        } = gh;
        self.pr_count = *pr_count;
        self.prs = prs.clone();
        self.ci_status = ci_status.clone();
        self.ci_workflow = ci_workflow.clone();
        self.ci_duration = ci_duration.clone();
        self.repo_kind = *repo_kind;
    }

    /// Land a local sweep's rebuild on this row: take the sweep-owned fields
    /// from `fresh`, keep every other field as the row has it *now*.
    ///
    /// `fresh` was built from a snapshot taken when the sweep started
    /// (`SweepTarget::old`), so its non-local fields are as old as the sweep.
    /// Replacing the row wholesale let a sweep that started while CI was
    /// pending restore `Pending` over a poll that had since said passed.
    /// Deciding ownership here, against the row as it is when the result
    /// lands, makes the completion order of sweep and poll irrelevant.
    ///
    /// The destructure is exhaustive on purpose: a new `RepoStatus` field does
    /// not compile until it is classified here as sweep-owned or not.
    pub fn apply_local_sweep(&mut self, fresh: RepoStatus) {
        let RepoStatus {
            // Sweep-owned: probed by `rebuild_repo_status` every sweep.
            branches,
            commits,
            behind,
            ahead,
            dirty,
            stash_count,
            stash_entries,
            worktrees,
            reflog,
            graph,
            graph_expanded,
            remote_branches,
            remote_only_branches,
            daily,
            daily_key,
            work_key,
            // GitHub-owned: written by the poll (`apply_github`).
            pr_count: _,
            prs: _,
            ci_status: _,
            ci_workflow: _,
            ci_duration: _,
            repo_kind: _,
            // Load-owned: set by discovery and `assemble_view`, which replaces
            // the whole list; a sweep only carries the snapshot's copy.
            last_local: _,
            name: _,
            path: _,
            location: _,
            groups: _,
            owner: _,
            remote_url: _,
            // Fetch-owned: written only when a fetch lands
            // (`FetchFacts::record`). The sweep's copy is the snapshot's.
            fetch: _,
        } = fresh;
        self.branches = branches;
        self.commits = commits;
        self.behind = behind;
        self.ahead = ahead;
        self.dirty = dirty;
        self.stash_count = stash_count;
        self.stash_entries = stash_entries;
        self.worktrees = worktrees;
        self.reflog = reflog;
        self.graph = graph;
        self.graph_expanded = graph_expanded;
        self.remote_branches = remote_branches;
        self.remote_only_branches = remote_only_branches;
        self.daily = daily;
        self.daily_key = daily_key;
        self.work_key = work_key;
    }

    /// A short status string like "3↓", "2↑", "4!", or "✓" (synced). The
    /// synced case is a bare checkmark rather than the word -- in a list
    /// that's mostly synced repos, the glyph reads as "all good" at a glance
    /// without a column of repeated "synced" text.
    pub fn short_status(&self) -> String {
        if self.is_not_cloned() {
            return NOT_CLONED.to_string();
        }
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
            "✓".to_string()
        } else {
            parts.join(" ")
        }
    }

    /// A longer status string like "3 behind, 2 ahead, 4 uncommitted".
    ///
    /// `behind` is summed over every local branch, so on a repo whose branches
    /// are behind by different amounts the bare number belongs to no single
    /// one of them. Read as "3 behind" it says a pull away; standing in the
    /// repo on a current branch, git then says there is nothing to pull and
    /// the two flatly disagree. Naming the spread is what makes the number
    /// mean the same thing here and there, and points at the branches block
    /// for the split. Only when more than one branch is behind: on the
    /// ordinary single-branch repo the qualifier would be noise.
    pub fn long_status(&self) -> String {
        if self.is_not_cloned() {
            return NOT_CLONED.to_string();
        }
        let mut parts = Vec::new();
        if self.behind > 0 {
            match self.branches.iter().filter(|b| b.behind > 0).count() {
                spread if spread > 1 => {
                    parts.push(format!("{} behind on {spread} branches", self.behind))
                }
                _ => parts.push(format!("{} behind", self.behind)),
            }
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
    pub not_cloned_count: u32,
    pub pr_count: u32,
}

impl Summary {
    /// Count over borrowed repos rather than a slice, so the footer can
    /// summarize the filtered view the repo list is drawing without cloning
    /// the repos back out of it.
    pub fn from_refs<'a>(repos: impl Iterator<Item = &'a RepoStatus>) -> Self {
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
                s.not_cloned_count += 1;
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

    #[test]
    fn owner_parsed_from_every_remote_url_form() {
        for (url, want) in [
            ("git@github.com:Gaurgle/fleetz.git", "Gaurgle"),
            ("git@github.com:Gaurgle/fleetz", "Gaurgle"),
            ("https://github.com/Gaurgle/fleetz.git", "Gaurgle"),
            ("https://github.com/Gaurgle/fleetz/", "Gaurgle"),
            ("ssh://git@github.com/acme/service.git", "acme"),
            // Self-hosted, and a non-default port, which must not be read as
            // the scp-style path separator.
            ("ssh://git@git.internal:2222/acme/service.git", "acme"),
            ("https://gitlab.example.com/team/tool.git", "team"),
        ] {
            assert_eq!(
                owner_from_remote_url(url).as_deref(),
                Some(want),
                "parsing {url}"
            );
        }
    }

    #[test]
    fn owner_is_none_for_urls_without_an_owner_repo_pair() {
        for url in [
            "",
            "not a url",
            "https://github.com",
            "https://github.com/",
            // An owner with no repo segment is not an owner.
            "https://github.com/Gaurgle",
            "git@github.com:",
            "/local/path/repo",
        ] {
            assert_eq!(owner_from_remote_url(url), None, "parsing {url:?}");
        }
    }

    #[test]
    fn repo_parsed_with_the_git_suffix_stripped() {
        for url in [
            "git@github.com:Gaurgle/portfolio-site.git",
            "git@github.com:Gaurgle/portfolio-site",
            "https://github.com/Gaurgle/portfolio-site/",
            "ssh://git@github.com/Gaurgle/portfolio-site.git",
            // An SSH host alias from ~/.ssh/config: the host is never checked.
            "git@github-work:Gaurgle/portfolio-site.git",
        ] {
            assert_eq!(
                repo_from_remote_url(url),
                Some(("Gaurgle".to_string(), "portfolio-site".to_string())),
                "parsing {url}"
            );
        }
    }

    /// The S10 forms of the identity design, each pinned to the exact
    /// `(owner, repo)` it yields, spelling included: case is kept here and
    /// only folded by whoever compares.
    #[test]
    fn repo_parsed_from_every_s10_remote_form() {
        for (url, owner, repo) in [
            ("git@github.com:Gaurgle/fleetz.git", "Gaurgle", "fleetz"),
            ("git@github.com:Gaurgle/fleetz", "Gaurgle", "fleetz"),
            ("https://github.com/Gaurgle/fleetz.git", "Gaurgle", "fleetz"),
            ("https://github.com/Gaurgle/fleetz", "Gaurgle", "fleetz"),
            ("https://github.com/gaurgle/FLEETZ/", "gaurgle", "FLEETZ"),
            (
                "ssh://git@github.com:22/Gaurgle/fleetz.git",
                "Gaurgle",
                "fleetz",
            ),
            ("git@github-work:Gaurgle/fleetz.git", "Gaurgle", "fleetz"),
        ] {
            assert_eq!(
                repo_from_remote_url(url),
                Some((owner.to_string(), repo.to_string())),
                "parsing {url}"
            );
        }
    }

    /// Two edges the identity design could only reason about, pinned as they
    /// behave: a `file://` URL reads its first two path segments as owner and
    /// repo, and only a lowercase `.git` suffix is stripped. Neither is a
    /// claim that this is right, only that it is what discovery sees today.
    #[test]
    fn file_url_and_uppercase_git_suffix_parse_as_they_always_have() {
        assert_eq!(
            repo_from_remote_url("file:///Users/me/repo"),
            Some(("Users".to_string(), "me".to_string()))
        );
        assert_eq!(
            repo_from_remote_url("git@github.com:Gaurgle/fleetz.GIT"),
            Some(("Gaurgle".to_string(), "fleetz.GIT".to_string()))
        );
    }

    /// The host comes out of every S10 form without user or port, and an
    /// SSH alias is carried as written: nothing resolves or checks it.
    #[test]
    fn remote_ref_carries_the_host_of_every_s10_form() {
        for (url, host, owner, name) in [
            (
                "git@github.com:Gaurgle/fleetz.git",
                "github.com",
                "Gaurgle",
                "fleetz",
            ),
            (
                "git@github.com:Gaurgle/fleetz",
                "github.com",
                "Gaurgle",
                "fleetz",
            ),
            (
                "https://github.com/Gaurgle/fleetz.git",
                "github.com",
                "Gaurgle",
                "fleetz",
            ),
            (
                "https://github.com/Gaurgle/fleetz",
                "github.com",
                "Gaurgle",
                "fleetz",
            ),
            (
                "https://github.com/gaurgle/FLEETZ/",
                "github.com",
                "gaurgle",
                "FLEETZ",
            ),
            (
                "ssh://git@github.com:22/Gaurgle/fleetz.git",
                "github.com",
                "Gaurgle",
                "fleetz",
            ),
            (
                "git@github-work:Gaurgle/fleetz.git",
                "github-work",
                "Gaurgle",
                "fleetz",
            ),
            ("file:///Users/me/repo", "", "Users", "me"),
        ] {
            assert_eq!(
                RemoteRef::parse(url),
                Some(RemoteRef {
                    host: host.to_string(),
                    owner: owner.to_string(),
                    name: name.to_string(),
                }),
                "parsing {url}"
            );
        }
    }

    #[test]
    fn remote_ref_keeps_the_url_spelling_for_display() {
        let mixed = RemoteRef::parse("https://GitHub.com/gaurgle/FLEETZ/").unwrap();
        assert_eq!(
            (
                mixed.host.as_str(),
                mixed.owner.as_str(),
                mixed.name.as_str()
            ),
            ("GitHub.com", "gaurgle", "FLEETZ")
        );
    }

    #[test]
    fn remote_ref_is_none_where_repo_from_remote_url_is() {
        for url in [
            "",
            "not a url",
            "https://github.com",
            "https://github.com/Gaurgle",
            "git@github.com:",
            "/local/path/repo",
        ] {
            assert_eq!(RemoteRef::parse(url), None, "parsing {url:?}");
        }
    }

    #[test]
    fn owner_display_prefers_the_api_over_the_remote_url() {
        // The API login is authoritative: a remote can point at a fork or a
        // mirror whose path no longer matches the owner GitHub reports.
        let mut repo = make_repo("fleetz", 0, 0, 0);
        repo.remote_url = Some("git@github.com:someone-else/fleetz.git".to_string());
        assert_eq!(repo.owner_display().as_deref(), Some("someone-else"));

        repo.owner = Some("Gaurgle".to_string());
        assert_eq!(repo.owner_display().as_deref(), Some("Gaurgle"));
    }

    #[test]
    fn owner_display_is_none_without_api_or_remote() {
        let repo = make_repo("orphan", 0, 0, 0);
        assert_eq!(repo.owner_display(), None);
    }

    /// A repo whose behind count is spread over several branches, as
    /// `long_status` sees it.
    fn repo_behind_on(branches: &[(&str, u32)]) -> RepoStatus {
        let total = branches.iter().map(|(_, n)| n).sum();
        let mut repo = make_repo("spread", total, 0, 0);
        repo.branches = branches
            .iter()
            .map(|(name, behind)| BranchStatus {
                name: (*name).to_string(),
                upstream: Some(format!("origin/{name}")),
                behind: *behind,
                ahead: 0,
                is_head: false,
                tip: String::new(),
            })
            .collect();
        repo
    }

    #[test]
    fn long_status_names_the_spread_when_several_branches_are_behind() {
        // The bare "40 behind" reads as one pull away, and on a repo whose
        // current branch is level git says there is nothing to pull.
        let r = repo_behind_on(&[("beta", 21), ("feature/who-goes-first", 19)]);
        assert_eq!(r.long_status(), "40 behind on 2 branches");
    }

    #[test]
    fn long_status_leaves_a_single_behind_branch_unqualified() {
        let r = repo_behind_on(&[("main", 3), ("beta", 0)]);
        assert_eq!(r.long_status(), "3 behind");
    }

    fn make_repo(name: &str, behind: u32, ahead: u32, dirty: u32) -> RepoStatus {
        RepoStatus {
            last_local: None,
            daily: None,
            daily_key: None,
            name: name.to_string(),
            path: Some(format!("~/repos/{name}")),
            location: RepoLocation::Primary,
            branches: vec![],
            commits: vec![],
            behind,
            ahead,
            dirty,
            pr_count: 0,
            prs: vec![],
            ci_status: CiStatus::Unknown,
            ci_workflow: None,
            ci_duration: None,
            groups: vec![],
            stash_count: 0,
            stash_entries: vec![],
            worktrees: vec![],
            reflog: vec![],
            graph: vec![],
            graph_expanded: Vec::new(),
            repo_kind: RepoKind::Unknown,
            owner: None,
            remote_url: None,
            remote_only_branches: vec![],
            remote_branches: vec![],
            work_key: None,
            fetch: FetchFacts::default(),
        }
    }

    #[test]
    fn synced_repo_reports_synced() {
        let r = make_repo("clean", 0, 0, 0);
        assert!(r.is_synced());
        assert_eq!(r.short_status(), "✓");
        assert_eq!(r.long_status(), "synced");
    }

    #[test]
    fn behind_repo_status_strings() {
        let r = make_repo("behind", 3, 0, 0);
        assert!(!r.is_synced());
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
    fn ci_and_sync_never_share_a_glyph() {
        // The original double-checkmark bug: a clean repo with green CI drew
        // two identical green ✓. CI owns the ringed disk, sync owns the check.
        let mut r = make_repo("clean", 0, 0, 0);
        r.ci_status = CiStatus::Passed;
        assert_eq!(r.short_status(), "✓");
        assert_eq!(r.ci_status.symbol(), "◉");
        assert_ne!(r.short_status(), r.ci_status.symbol());
    }

    #[test]
    fn every_known_ci_state_is_the_same_shape() {
        // Deliberate: the states differ by color, not shape, so the disk can
        // never be mistaken for a sync status. Unknown stays empty so a repo
        // with no CI reserves no column.
        assert_eq!(CiStatus::Passed.symbol(), "◉");
        assert_eq!(CiStatus::Failed.symbol(), "◉");
        assert_eq!(CiStatus::Pending.symbol(), "◉");
        assert_eq!(CiStatus::Unknown.symbol(), "");
    }

    #[test]
    fn not_cloned_repo_never_reads_as_synced() {
        // Regression: all three counters are zero for a repo with no working
        // tree, which used to render as a green "✓ / synced" -- visually
        // identical to a repo that really was up to date.
        let mut r = make_repo("ghost", 0, 0, 0);
        r.location = RepoLocation::NotCloned;
        r.path = None;

        assert!(r.is_not_cloned());
        assert!(!r.is_synced());
        assert_eq!(r.short_status(), "not cloned");
        assert_eq!(r.long_status(), "not cloned");
    }

    #[test]
    fn summary_counts_correctly() {
        let repos = [
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
        let s = Summary::from_refs(repos.iter());
        assert_eq!(s.behind_count, 1);
        assert_eq!(s.ahead_count, 1);
        assert_eq!(s.dirty_count, 1);
        assert_eq!(s.synced_count, 1);
        assert_eq!(s.not_cloned_count, 1);
    }

    // --- FZ-04: the fetch facts (docs/fetch-outcome-contract.md) ---

    fn at(h: u32) -> DateTime<chrono::Local> {
        use chrono::TimeZone;
        chrono::Local
            .with_ymd_and_hms(2026, 10, 1, h, 0, 0)
            .unwrap()
    }

    fn failed(kind: crate::git::FetchFailureKind) -> crate::git::FetchOutcome {
        crate::git::FetchOutcome::Failed(crate::git::FetchFailure { kind, detail: None })
    }

    /// Rule 3: a failure records the attempt and leaves the last success
    /// where it was.
    #[test]
    fn only_a_successful_fetch_advances_the_last_success() {
        let mut facts = FetchFacts::default();
        facts.record(&crate::git::FetchOutcome::Succeeded, at(9));
        facts.record(&failed(crate::git::FetchFailureKind::Unreachable), at(10));
        assert_eq!(facts.last_success, Some(at(9)));
        assert_eq!(facts.last_attempt, Some(at(10)));
        assert!(facts.failure().is_some());
    }

    /// Rule 3: unknown stays unknown through a failure.
    #[test]
    fn a_failure_with_no_known_success_leaves_it_unknown() {
        let mut facts = FetchFacts::from_disk(None);
        facts.record(&failed(crate::git::FetchFailureKind::NoRemote), at(10));
        assert_eq!(facts.last_success, None);
    }

    /// Recovery: the next fetch that works clears the failure.
    #[test]
    fn a_success_after_a_failure_recovers() {
        let mut facts = FetchFacts::default();
        facts.record(&failed(crate::git::FetchFailureKind::Auth), at(9));
        facts.record(&crate::git::FetchOutcome::Succeeded, at(10));
        assert_eq!(facts.failure(), None);
        assert_eq!(facts.last_success, Some(at(10)));
    }

    /// Rule 2: an update that did not fetch says nothing about fetching.
    #[test]
    fn a_sweep_that_did_not_fetch_changes_no_fetch_fact() {
        let mut facts = FetchFacts::default();
        facts.record(&failed(crate::git::FetchFailureKind::TimedOut), at(9));
        let before = facts.clone();
        facts.record(&crate::git::FetchOutcome::NotAttempted, at(10));
        assert_eq!(facts, before);
    }

    /// Rule 2 under the FZ-02 ownership split: the fetch facts are their own
    /// owner, so a sweep result never carries its snapshot's copy onto the
    /// row, in either direction.
    #[test]
    fn a_local_sweep_does_not_carry_the_fetch_facts() {
        let mut row = make_repo("r", 0, 0, 0);
        row.fetch
            .record(&failed(crate::git::FetchFailureKind::Unreachable), at(10));
        let mut fresh = make_repo("r", 2, 0, 0);
        fresh.fetch = FetchFacts::from_disk(Some(std::time::SystemTime::now()));
        let kept = row.fetch.clone();
        row.apply_local_sweep(fresh);
        assert_eq!(row.behind, 2, "the sweep's own fields still land");
        assert_eq!(row.fetch, kept);
    }

    /// A time-window apply rebuilds every row and fetches nothing. The
    /// failure has to survive it, or the counts read as current again.
    #[test]
    fn a_load_that_did_not_fetch_keeps_the_sessions_failure() {
        let mut session = FetchFacts::from_disk(None);
        session.record(&crate::git::FetchOutcome::Succeeded, at(9));
        session.record(&failed(crate::git::FetchFailureKind::Auth), at(10));
        // The failure emptied FETCH_HEAD, so the disk knows nothing.
        let loaded = FetchFacts::from_disk(None);
        assert_eq!(session.carried_into(&loaded), session);
    }

    /// An `R` whose own fetch worked clears the failure; one whose fetch
    /// failed keeps the last success the session saw.
    #[test]
    fn a_load_that_fetched_records_its_fetch_on_the_sessions_facts() {
        let mut session = FetchFacts::default();
        session.record(&crate::git::FetchOutcome::Succeeded, at(9));
        session.record(&failed(crate::git::FetchFailureKind::Auth), at(10));

        let mut ok = FetchFacts::from_disk(None);
        ok.record(&crate::git::FetchOutcome::Succeeded, at(11));
        let carried = session.carried_into(&ok);
        assert_eq!(carried.failure(), None);
        assert_eq!(carried.last_success, Some(at(11)));

        let mut bad = FetchFacts::from_disk(None);
        bad.record(&failed(crate::git::FetchFailureKind::Unreachable), at(11));
        let carried = session.carried_into(&bad);
        assert_eq!(
            carried.failure().map(|f| f.kind),
            Some(crate::git::FetchFailureKind::Unreachable)
        );
        assert_eq!(
            carried.last_success,
            Some(at(9)),
            "the session's success was lost"
        );
    }

    /// An `R` whose fetch finished before a sweep's fetch of the same repo
    /// landed: the newer outcome and attempt time stay.
    #[test]
    fn a_load_fetch_older_than_the_sessions_attempt_is_not_recorded() {
        let mut session = FetchFacts::default();
        session.record(&crate::git::FetchOutcome::Succeeded, at(11));
        let mut loaded = FetchFacts::from_disk(None);
        loaded.record(&failed(crate::git::FetchFailureKind::Unreachable), at(10));
        assert_eq!(session.carried_into(&loaded), session);
    }

    /// A fetch run in a terminal after the failure fixed it; the disk says so.
    #[test]
    fn a_newer_success_on_disk_clears_an_older_failure() {
        let mut session = FetchFacts::default();
        session.record(&failed(crate::git::FetchFailureKind::Auth), at(10));
        let loaded = FetchFacts {
            last_success: Some(at(11)),
            ..FetchFacts::default()
        };
        let carried = session.carried_into(&loaded);
        assert_eq!(carried.failure(), None);
        assert_eq!(carried.last_success, Some(at(11)));
    }
}
