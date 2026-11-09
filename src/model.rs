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
    /// The latest progress of a fetch running now (FZ-35a), `None` when none
    /// is or it has not reported yet. On the facts rather than beside them
    /// because the row's badges read the facts and nothing else of the
    /// fetch, and because `carried_into` then keeps it across a reload that
    /// lands mid-fetch. In memory only; cleared when the fetch lands.
    pub progress: Option<crate::git::FetchProgress>,
    /// GitHub's `diskUsage` (kB) for this checkout's remote when a fetch of
    /// it last succeeded in this session (FZ-35b), the baseline the
    /// pre-check measures growth from. `None` is unknown. In memory only.
    pub disk_baseline: Option<u64>,
    /// How much the remote grew past the cap since `disk_baseline`, in
    /// bytes, while the pre-check holds the repo (`assess_growth`). Every
    /// automatic fetch and `F` skip it until `f` or a success.
    pub grown: Option<u64>,
}

/// The growth in bytes from `baseline_kb` to `current_kb`, when it is more
/// than `cap_bytes`. GitHub's `diskUsage` is in kilobytes.
fn growth_over_cap(
    baseline_kb: Option<u64>,
    current_kb: Option<u64>,
    cap_bytes: Option<u64>,
) -> Option<u64> {
    let cap = cap_bytes?;
    let growth = current_kb?.checked_sub(baseline_kb?)?.saturating_mul(1024);
    (growth > cap).then_some(growth)
}

/// Why a repo's automatic fetches wait for `f` (FZ-35b), with the size the
/// row names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeHold {
    /// GitHub's `diskUsage` grew by this many bytes, more than the cap,
    /// since the last successful fetch: `fetch ~N MB, press f`.
    Grown(u64),
    /// A fetch was cut at the cap after receiving this many bytes
    /// (`FetchFailureKind::TooBig`): `fetch over N MB, press f`.
    OverCap(u64),
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
    /// `last_success` (rule 3); `NotAttempted` changes nothing (rule 2), not
    /// even the progress of a fetch still running. A finished one ends it.
    pub fn record(&mut self, outcome: &crate::git::FetchOutcome, now: DateTime<chrono::Local>) {
        use crate::git::FetchOutcome;
        match outcome {
            FetchOutcome::NotAttempted => return,
            FetchOutcome::Succeeded => {
                self.last_success = Some(now);
                self.grown = None;
            }
            FetchOutcome::Failed(_) => {}
        }
        self.last_attempt = Some(now);
        self.outcome = outcome.clone();
        self.progress = None;
    }

    /// The failure, while the last fetch failed. Behind and ahead are then
    /// counted against the remote refs of `last_success` (rule 5).
    pub fn failure(&self) -> Option<&crate::git::FetchFailure> {
        match &self.outcome {
            crate::git::FetchOutcome::Failed(failure) => Some(failure),
            _ => None,
        }
    }

    /// The pre-check (FZ-35b): hold the repo when GitHub's `diskUsage` now
    /// (`current_kb`) exceeds `disk_baseline` by more than `cap_bytes`.
    /// A missing current value, an unknown baseline, an unchanged or
    /// shrunken size, or no cap decide nothing, and lift an earlier hold of
    /// this kind: the in-fetch cap still bounds the fetch.
    pub fn assess_growth(&mut self, current_kb: Option<u64>, cap_bytes: Option<u64>) {
        self.grown = growth_over_cap(self.disk_baseline, current_kb, cap_bytes);
    }

    /// Whether automatic fetches and `F` skip this repo, and the size to
    /// name. A fetch cut at the cap is the hold the row saw happen, so it is
    /// named over a growth GitHub reports.
    pub fn size_hold(&self) -> Option<SizeHold> {
        if let Some(failure) = self.failure()
            && failure.kind == crate::git::FetchFailureKind::TooBig
        {
            return Some(SizeHold::OverCap(failure.received_bytes().unwrap_or(0)));
        }
        self.grown.map(SizeHold::Grown)
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
            // The size baseline of that success: the load's own fetch knows
            // it, a `git fetch` run in a terminal does not.
            kept.disk_baseline = loaded.disk_baseline;
        }
        // Only when newer than what the session already has: an `R` whose
        // fetch finished before a sweep's fetch of the same repo landed must
        // not move `last_attempt` back or put its older outcome on top.
        if let Some(at) = loaded.last_attempt
            && newer(at, kept.last_attempt)
        {
            kept.record(&loaded.outcome, at);
            if loaded.outcome == crate::git::FetchOutcome::Succeeded {
                kept.disk_baseline = loaded.disk_baseline;
            }
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
/// - **Derived** (no writer): `authors`, rebuilt from `commits` on read.
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
    /// Owner login as reported by the GitHub API, set at load on the one row
    /// carrying this remote's GitHub fields. It is the carrier mark the poll
    /// follows. Every other row leaves it None and falls back to the remote
    /// URL via `owner_display`.
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
    /// The distinct authors of `commits`, for the `/` search (FZ-56).
    /// Derived, never written by a source: built on first read and dropped
    /// by `apply_local_sweep` when a new commit list lands, so a keystroke
    /// does not walk 200 commits per row. In memory only.
    pub authors: AuthorSet,
}

/// The distinct authors of a row's commit list, filled on first read.
///
/// Lazy rather than computed where the commits are built, so the rows
/// assembled at load (`main.rs`) need no code of their own: they start
/// empty-unfilled and the first search fills them. A `OnceLock` so the row
/// stays `Sync`.
#[derive(Debug, Clone, Default)]
pub struct AuthorSet(std::sync::OnceLock<Vec<String>>);

impl AuthorSet {
    /// The distinct authors of `commits`, sorted, computed once.
    fn get_or_build(&self, commits: &[Commit]) -> &[String] {
        self.0.get_or_init(|| {
            let mut authors: Vec<String> = commits
                .iter()
                .map(|c| c.author.as_str())
                .filter(|a| !a.is_empty())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(str::to_string)
                .collect();
            authors.shrink_to_fit();
            authors
        })
    }
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

/// An active linked git worktree and the state the Info view's `worktrees`
/// section draws for it.
#[derive(Debug, Clone, Default)]
pub struct Worktree {
    /// `~`-collapsed, as the pane draws it.
    pub path: String,
    /// The checked-out branch, empty when `detached`.
    pub branch: String,
    /// HEAD is detached: no branch, so no ahead or behind to compare.
    pub detached: bool,
    /// HEAD's short sha, what a detached row names.
    pub head: String,
    /// `git status --porcelain` lines in this worktree. `None` when the probe
    /// failed, so a broken worktree draws as unknown, never as clean.
    pub dirty: Option<u32>,
    /// When HEAD was committed. `None` when the probe failed.
    pub head_at: Option<chrono::DateTime<chrono::Utc>>,
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
/// Parsing folds no case; compare through `key`.
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

    /// The comparison key: host, owner and name, each lowercased, since GitHub
    /// treats owners and names case-insensitively. Compare keys, never the
    /// parsed spelling, which is kept for display.
    pub fn key(&self) -> Self {
        let (owner, name) = identity_key(Some(&self.owner), &self.name);
        Self {
            host: host_key(&self.host),
            owner: owner.unwrap_or_default(),
            name,
        }
    }
}

/// The comparison key of a host: lowercased. The one rule `RemoteRef::key`
/// and the cache file name (`cache::host_part`) both spell a host by.
pub fn host_key(host: &str) -> String {
    host.to_lowercase()
}

/// The key two remotes are the same repo by: lowercased owner and name, the
/// one rule every "same remote" check spells an owner and a name by
/// (`RemoteRef::key`, `GithubRepo::identity_key`, `main.rs::remote_key`,
/// `main.rs::names_remote`, discovery's `repo_key`). No owner is the legacy
/// key of a payload without `login`, so such an entry is the same as another
/// only by name, and never the same as an owned one. Logins and repo names
/// are ASCII, so lowercasing agrees with `eq_ignore_ascii_case`.
pub fn identity_key(owner: Option<&str>, name: &str) -> (Option<String>, String) {
    (owner.map(str::to_lowercase), name.to_lowercase())
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

/// Bare counts spelled as the repo list spells them: `N↓ N↑ N!` in that
/// order, the zeros left out, or `✓` when all are zero. The single source
/// `RepoStatus::short_status` delegates to, and what a linked worktree's
/// counters read in the Info view.
pub fn counters_text(behind: u32, ahead: u32, dirty: u32) -> String {
    let mut parts = Vec::new();
    if behind > 0 {
        parts.push(format!("{behind}↓"));
    }
    if ahead > 0 {
        parts.push(format!("{ahead}↑"));
    }
    if dirty > 0 {
        parts.push(format!("{dirty}!"));
    }
    if parts.is_empty() {
        "✓".to_string()
    } else {
        parts.join(" ")
    }
}

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

    /// The main checkout as a worktree row, for the `worktrees` view
    /// (FZ-77), which lists it first: `worktrees` carries the linked ones
    /// only. Built from what the sweep already holds: the checked-out
    /// branch, its tip, the dirty count, and the tip's commit time when the
    /// loaded commit list has it. `None` for a row with nothing on disk.
    pub fn main_worktree(&self) -> Option<Worktree> {
        if self.is_not_cloned() {
            return None;
        }
        let path = self.path.clone()?;
        let head_branch = self.branches.iter().find(|b| b.is_head);
        let head = match head_branch {
            Some(b) if !b.tip.is_empty() => b.tip.clone(),
            _ => self
                .work_key
                .as_ref()
                .map(|k| k.head_sha.chars().take(7).collect())
                .unwrap_or_default(),
        };
        let head_at = self
            .commits
            .iter()
            .find(|c| {
                !c.hash.is_empty()
                    && !head.is_empty()
                    && (c.hash.starts_with(&head) || head.starts_with(&c.hash))
            })
            .and_then(|c| c.date);
        Some(Worktree {
            path,
            branch: head_branch.map(|b| b.name.clone()).unwrap_or_default(),
            detached: head_branch.is_none(),
            head,
            dirty: Some(self.dirty),
            head_at,
        })
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

    /// The distinct authors of the loaded commit list, cached on the row
    /// (see `authors`). Empty for a row with no commits, a not-cloned one.
    pub fn commit_authors(&self) -> &[String] {
        self.authors.get_or_build(&self.commits)
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
            // Read by the fetch cap's pre-check from the dataset
            // (`main.rs::assess_size_holds`) onto the fetch facts, never here.
            disk_usage: _,
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
            // Derived from `commits`: reset below so the next read rebuilds
            // it from the list that just landed.
            authors: _,
        } = fresh;
        self.authors = AuthorSet::default();
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
    ///
    /// Not-cloned is said in words; everything else is `counters_text` on
    /// the repo's own counts.
    pub fn short_status(&self) -> String {
        if self.is_not_cloned() {
            return NOT_CLONED.to_string();
        }
        counters_text(self.behind, self.ahead, self.dirty)
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
mod tests;
