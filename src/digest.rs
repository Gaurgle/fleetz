//! The slot digest (FZ-45): what happened across one pane 1 section since a
//! window's cutoff. Standing on a section header draws it in pane 2.
//!
//! Numbers are local first: commit counts come from `git log --since` on a
//! clone fetched since the cutoff (`git::commits_since`), PRs and CI from the
//! poll's fields already on the row, the worklist from the sweep's counters.
//! Nothing here asks GitHub for anything. Spec:
//! `docs/superpowers/specs/2026-10-22-slot-digest-design.md`.

use chrono::{DateTime, Local, Utc};

use crate::model::{CiStatus, Commit, RepoLocation, RepoStatus};

/// A section of the repo list, in the order the list draws them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    GithubActivity,
    LocalChanges,
    AlsoActive,
}

impl Section {
    /// The section a repo is listed under. `app::section_rank` sorts by this,
    /// so the order and the header a repo sits under have one source.
    pub fn of(repo: &RepoStatus) -> Self {
        match repo.location {
            RepoLocation::Primary => Self::GithubActivity,
            RepoLocation::LocalOnly => Self::LocalChanges,
            RepoLocation::Elsewhere | RepoLocation::NotCloned => Self::AlsoActive,
        }
    }

    /// The header's words, as repoz's `section_div` prints them.
    pub fn label(self) -> &'static str {
        match self {
            Self::GithubActivity => "github activity",
            Self::LocalChanges => "local changes",
            Self::AlsoActive => "also active",
        }
    }
}

/// The window `t` cycles on the digest, the way repoz's flags pick one: the
/// default slot, `-t` (today), `--since yesterday` and `--since 1w`. Each is
/// a cutoff with no end, as repoz's are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DigestWindow {
    #[default]
    Slot,
    Today,
    Yesterday,
    Week,
}

impl DigestWindow {
    pub const ALL: [Self; 4] = [Self::Slot, Self::Today, Self::Yesterday, Self::Week];

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|w| *w == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// The short name the pane's window switcher draws.
    pub fn name(self) -> &'static str {
        match self {
            Self::Slot => "slot",
            Self::Today => "today",
            Self::Yesterday => "yesterday",
            Self::Week => "week",
        }
    }

    /// The cutoff and the header label at `now`. The slot comes from
    /// `time::slot_cutoff_at`, the function the time window's slot reads;
    /// the day windows from `time::date_to_utc_midnight`, as repoz's
    /// `--since` resolves a date to its local midnight.
    pub fn bounds(
        self,
        now: DateTime<Local>,
        work_start: u8,
        work_end: u8,
    ) -> (DateTime<Utc>, String) {
        let today = now.date_naive();
        let since_day = |days: i64| {
            let day = today - chrono::Duration::days(days);
            (
                crate::time::date_to_utc_midnight(day),
                format!("since {day}"),
            )
        };
        match self {
            Self::Slot => crate::time::slot_cutoff_at(now, work_start, work_end),
            Self::Today => (
                crate::time::date_to_utc_midnight(today),
                "today".to_string(),
            ),
            Self::Yesterday => since_day(1),
            Self::Week => since_day(7),
        }
    }
}

/// What a commit count was read under: the cutoff and the clone's refs. A
/// sweep that moves no ref keeps the key, so the log is not re-run; one that
/// moves a ref, or a new window, asks for a new read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogKey {
    pub cutoff: DateTime<Utc>,
    pub head_sha: String,
    pub ref_signature: u64,
}

/// A held `git log --since` read for one clone, `commits` None while it runs.
#[derive(Debug, Clone)]
pub struct HeldLog {
    pub key: LogKey,
    pub commits: Option<Vec<Commit>>,
}

/// A `git log --since` the app wants run off the UI thread.
#[derive(Debug, Clone)]
pub struct LogRequest {
    /// The row's display path, the key the result lands under.
    pub path: String,
    pub dir: std::path::PathBuf,
    pub key: LogKey,
}

/// What one digest line says about its repo.
#[derive(Debug, Clone)]
pub enum LineBody {
    /// Commits in the window, newest first; never empty.
    Counted { commits: Vec<Commit> },
    /// Pushed in the window, but not cloned or not fetched since the
    /// cutoff: fleetz knows it moved, not what moved.
    PushedOnly,
    /// The commit count is being read.
    Reading,
    /// The local changes worklist.
    Worklist {
        dirty: u32,
        stashes: u32,
        unpushed: u32,
    },
}

/// One touched repo of the digest.
#[derive(Debug, Clone)]
pub struct DigestLine {
    /// Index into `App::repos`.
    pub repo: usize,
    pub name: String,
    pub body: LineBody,
    pub ci: CiStatus,
    pub prs: u32,
    pub pushed_at: Option<DateTime<Utc>>,
}

impl DigestLine {
    /// The distinct authors of the line's commits, newest first.
    pub fn authors(&self) -> Vec<&str> {
        let LineBody::Counted { commits } = &self.body else {
            return Vec::new();
        };
        let mut out: Vec<&str> = Vec::new();
        for c in commits {
            if !out.contains(&c.author.as_str()) {
                out.push(&c.author);
            }
        }
        out
    }
}

/// The digest of one section in one window.
#[derive(Debug, Clone)]
pub struct Digest {
    pub section: Section,
    pub window: DigestWindow,
    pub label: String,
    pub lines: Vec<DigestLine>,
}

/// The sums the totals line draws.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub repos: usize,
    pub commits: usize,
    pub prs: u32,
    pub red_ci: usize,
    pub dirty: u32,
    pub stashes: u32,
    pub unpushed: u32,
}

impl Digest {
    pub fn totals(&self) -> Totals {
        let mut t = Totals {
            repos: self.lines.len(),
            ..Totals::default()
        };
        for line in &self.lines {
            t.prs += line.prs;
            if line.ci == CiStatus::Failed {
                t.red_ci += 1;
            }
            match &line.body {
                LineBody::Counted { commits } => t.commits += commits.len(),
                LineBody::Worklist {
                    dirty,
                    stashes,
                    unpushed,
                } => {
                    t.dirty += dirty;
                    t.stashes += stashes;
                    t.unpushed += unpushed;
                }
                LineBody::PushedOnly | LineBody::Reading => {}
            }
        }
        t
    }
}

/// One row of a section as the digest sees it.
pub struct Row<'a> {
    /// Index into `App::repos`.
    pub index: usize,
    pub repo: &'a RepoStatus,
    /// The carried dataset entry's `pushed_at`, None when the row carries none.
    pub pushed_at: Option<DateTime<Utc>>,
    /// The held log read under the row's current `LogKey`, None when there
    /// is none yet.
    pub log: Option<&'a [Commit]>,
}

/// Whether the clone's last successful fetch is at or after `cutoff`
/// (`FetchFacts::last_success`, from `FETCH_HEAD`). Unknown is not fetched.
fn fetched_since(repo: &RepoStatus, cutoff: DateTime<Utc>) -> bool {
    repo.fetch
        .last_success
        .is_some_and(|t| t.with_timezone(&Utc) >= cutoff)
}

/// Whether a remote-backed row moved in the window: GitHub's push time.
fn pushed_since(pushed_at: Option<DateTime<Utc>>, cutoff: DateTime<Utc>) -> bool {
    pushed_at.is_some_and(|t| t >= cutoff)
}

/// The key a row's commit count must be read under, or None when its line
/// is drawn without one: a local changes row (the worklist), an untouched
/// row, a row with no clone, one not fetched since the cutoff, or one whose
/// refs the sweep could not key.
pub fn needs_log(
    section: Section,
    repo: &RepoStatus,
    pushed_at: Option<DateTime<Utc>>,
    cutoff: DateTime<Utc>,
) -> Option<LogKey> {
    if section == Section::LocalChanges
        || !pushed_since(pushed_at, cutoff)
        || repo.location == RepoLocation::NotCloned
        || repo.path.is_none()
        || !fetched_since(repo, cutoff)
    {
        return None;
    }
    let work = repo.work_key.as_ref()?;
    Some(LogKey {
        cutoff,
        head_sha: work.head_sha.clone(),
        ref_signature: work.ref_signature,
    })
}

/// The digest line for `row`, or None when it did not move in the window.
pub fn line_for(section: Section, row: &Row<'_>, cutoff: DateTime<Utc>) -> Option<DigestLine> {
    let repo = row.repo;
    let body = if section == Section::LocalChanges {
        let worklist = repo.dirty > 0 || repo.stash_count > 0 || repo.ahead > 0;
        if !worklist && !repo.last_local.is_some_and(|t| t >= cutoff) {
            return None;
        }
        LineBody::Worklist {
            dirty: repo.dirty,
            stashes: repo.stash_count,
            unpushed: repo.ahead,
        }
    } else {
        if !pushed_since(row.pushed_at, cutoff) {
            return None;
        }
        match needs_log(section, repo, row.pushed_at, cutoff) {
            None => LineBody::PushedOnly,
            Some(_) => match row.log {
                None => LineBody::Reading,
                Some([]) => LineBody::PushedOnly,
                Some(commits) => LineBody::Counted {
                    commits: commits.to_vec(),
                },
            },
        }
    };
    Some(DigestLine {
        repo: row.index,
        name: repo.name.clone(),
        body,
        ci: repo.ci_status.clone(),
        prs: repo.pr_count,
        pushed_at: row.pushed_at,
    })
}

/// The digest of `section` from its rows in the list's order, which is the
/// section's own order: most recent push first.
pub fn build(
    section: Section,
    window: DigestWindow,
    cutoff: DateTime<Utc>,
    label: String,
    rows: &[Row<'_>],
) -> Digest {
    Digest {
        section,
        window,
        label,
        lines: rows
            .iter()
            .filter_map(|row| line_for(section, row, cutoff))
            .collect(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use chrono::TimeZone;

    pub(crate) fn repo(name: &str, location: RepoLocation) -> RepoStatus {
        RepoStatus {
            daily: None,
            daily_key: None,
            last_local: None,
            name: name.to_string(),
            path: Some(format!("~/repos/{name}")),
            location,
            branches: vec![],
            commits: vec![],
            behind: 0,
            ahead: 0,
            dirty: 0,
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
            repo_kind: crate::model::RepoKind::Unknown,
            owner: None,
            remote_url: None,
            remote_only_branches: vec![],
            remote_branches: vec![],
            work_key: None,
            fetch: crate::model::FetchFacts::default(),
            authors: Default::default(),
        }
    }

    pub(crate) fn work_key(head: &str) -> crate::git::WorkKey {
        crate::git::WorkKey {
            head_sha: head.to_string(),
            index_mtime: None,
            index_size: 0,
            stash_size: 0,
            head_log_size: 0,
            head_ref: None,
            ref_signature: 7,
        }
    }

    pub(crate) fn commit(hash: &str, author: &str, subject: &str) -> Commit {
        Commit {
            hash: hash.to_string(),
            message: subject.to_string(),
            body: String::new(),
            author: author.to_string(),
            date: None,
            insertions: 0,
            deletions: 0,
            files: vec![],
        }
    }

    /// A cloned row fetched after `cutoff`, with refs the sweep keyed.
    pub(crate) fn fetched(name: &str, cutoff: DateTime<Utc>) -> RepoStatus {
        let mut r = repo(name, RepoLocation::Primary);
        r.work_key = Some(work_key("abc"));
        r.fetch.last_success = Some((cutoff + chrono::Duration::minutes(5)).with_timezone(&Local));
        r
    }

    fn cutoff() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 27, 7, 0, 0).unwrap()
    }

    fn after() -> Option<DateTime<Utc>> {
        Some(cutoff() + chrono::Duration::hours(1))
    }

    fn row<'a>(
        repo: &'a RepoStatus,
        pushed_at: Option<DateTime<Utc>>,
        log: Option<&'a [Commit]>,
    ) -> Row<'a> {
        Row {
            index: 0,
            repo,
            pushed_at,
            log,
        }
    }

    #[test]
    fn a_fetched_clone_pushed_in_the_window_is_counted_from_its_log() {
        let r = fetched("fleetz", cutoff());
        let log = vec![
            commit("a1", "Andreas Roos", "feat: two"),
            commit("b2", "Andreas Roos", "feat: one"),
        ];
        let line = line_for(
            Section::GithubActivity,
            &row(&r, after(), Some(&log)),
            cutoff(),
        )
        .unwrap();
        let LineBody::Counted { commits } = &line.body else {
            panic!("{:?}", line.body);
        };
        assert_eq!(commits.len(), 2);
        assert_eq!(line.authors(), vec!["Andreas Roos"]);
    }

    #[test]
    fn two_authors_are_both_named() {
        let r = fetched("bonsai", cutoff());
        let log = vec![
            commit("a1", "Ann Bee", "x"),
            commit("b2", "Cid Dee", "y"),
            commit("c3", "Ann Bee", "z"),
        ];
        let line = line_for(
            Section::GithubActivity,
            &row(&r, after(), Some(&log)),
            cutoff(),
        )
        .unwrap();
        assert_eq!(line.authors(), vec!["Ann Bee", "Cid Dee"]);
    }

    #[test]
    fn a_clone_not_fetched_since_the_cutoff_shows_its_push_only() {
        let mut r = fetched("repoz", cutoff());
        r.fetch.last_success = Some((cutoff() - chrono::Duration::hours(1)).with_timezone(&Local));
        assert_eq!(
            needs_log(Section::GithubActivity, &r, after(), cutoff()),
            None
        );
        let log = vec![commit("a1", "A", "stale read")];
        let line = line_for(
            Section::GithubActivity,
            &row(&r, after(), Some(&log)),
            cutoff(),
        )
        .unwrap();
        assert!(matches!(line.body, LineBody::PushedOnly), "{:?}", line.body);
    }

    #[test]
    fn an_unknown_fetch_time_and_a_not_cloned_row_show_the_push_only() {
        let mut unknown = fetched("a", cutoff());
        unknown.fetch.last_success = None;
        assert_eq!(
            needs_log(Section::GithubActivity, &unknown, after(), cutoff()),
            None
        );
        let mut absent = fetched("b", cutoff());
        absent.location = RepoLocation::NotCloned;
        assert_eq!(
            needs_log(Section::AlsoActive, &absent, after(), cutoff()),
            None
        );
        let line = line_for(Section::AlsoActive, &row(&absent, after(), None), cutoff()).unwrap();
        assert!(matches!(line.body, LineBody::PushedOnly));
    }

    #[test]
    fn a_row_not_pushed_in_the_window_is_not_in_the_digest() {
        let r = fetched("quiet", cutoff());
        let before = Some(cutoff() - chrono::Duration::minutes(1));
        assert!(line_for(Section::GithubActivity, &row(&r, before, None), cutoff()).is_none());
        assert!(line_for(Section::GithubActivity, &row(&r, None, None), cutoff()).is_none());
        assert_eq!(
            needs_log(Section::GithubActivity, &r, before, cutoff()),
            None
        );
    }

    #[test]
    fn a_log_still_running_reads_as_reading_and_an_empty_one_as_pushed() {
        let r = fetched("fleetz", cutoff());
        let line = line_for(Section::GithubActivity, &row(&r, after(), None), cutoff()).unwrap();
        assert!(matches!(line.body, LineBody::Reading));
        let line = line_for(
            Section::GithubActivity,
            &row(&r, after(), Some(&[])),
            cutoff(),
        )
        .unwrap();
        assert!(matches!(line.body, LineBody::PushedOnly));
    }

    #[test]
    fn the_log_key_is_the_cutoff_and_the_refs() {
        let r = fetched("fleetz", cutoff());
        let key = needs_log(Section::GithubActivity, &r, after(), cutoff()).unwrap();
        assert_eq!(
            key,
            LogKey {
                cutoff: cutoff(),
                head_sha: "abc".to_string(),
                ref_signature: 7
            }
        );
        let mut moved = r.clone();
        moved.work_key = Some(work_key("def"));
        assert_ne!(
            needs_log(Section::GithubActivity, &moved, after(), cutoff()),
            Some(key)
        );
    }

    #[test]
    fn the_local_digest_is_the_worklist() {
        let mut r = repo("scratch", RepoLocation::LocalOnly);
        r.dirty = 4;
        r.stash_count = 1;
        r.ahead = 2;
        assert_eq!(
            needs_log(Section::LocalChanges, &r, after(), cutoff()),
            None
        );
        let line = line_for(Section::LocalChanges, &row(&r, None, None), cutoff()).unwrap();
        assert!(matches!(
            line.body,
            LineBody::Worklist {
                dirty: 4,
                stashes: 1,
                unpushed: 2
            }
        ));
        let clean = repo("idle", RepoLocation::LocalOnly);
        assert!(line_for(Section::LocalChanges, &row(&clean, None, None), cutoff()).is_none());
    }

    #[test]
    fn totals_sum_the_touched_lines() {
        let mut a = fetched("a", cutoff());
        a.pr_count = 2;
        a.ci_status = CiStatus::Failed;
        let mut b = fetched("b", cutoff());
        b.fetch.last_success = None;
        b.pr_count = 1;
        let quiet = fetched("quiet", cutoff());
        let log = vec![
            commit("1", "A", "x"),
            commit("2", "A", "y"),
            commit("3", "A", "z"),
        ];
        let rows = [
            row(&a, after(), Some(&log)),
            row(&b, after(), None),
            row(&quiet, None, None),
        ];
        let d = build(
            Section::GithubActivity,
            DigestWindow::Slot,
            cutoff(),
            "since x".into(),
            &rows,
        );
        let t = d.totals();
        assert_eq!((t.repos, t.commits, t.prs, t.red_ci), (2, 3, 3, 1));
    }

    #[test]
    fn t_cycles_the_four_windows_and_wraps() {
        let mut w = DigestWindow::default();
        let mut seen = vec![w];
        for _ in 0..4 {
            w = w.next();
            seen.push(w);
        }
        assert_eq!(
            seen,
            vec![
                DigestWindow::Slot,
                DigestWindow::Today,
                DigestWindow::Yesterday,
                DigestWindow::Week,
                DigestWindow::Slot
            ]
        );
    }

    /// The cutoffs come from `time.rs`: the slot from the function the time
    /// window's slot reads, the days as repoz's `-t` and `--since` resolve.
    #[test]
    fn the_window_cutoffs_match_time_rs() {
        let now = Local::now();
        let (slot, slot_label) = DigestWindow::Slot.bounds(now, 9, 18);
        assert_eq!((slot, slot_label), crate::time::slot_cutoff_at(now, 9, 18));
        let (auto, _) = crate::time::auto_cutoff(9, 18);
        assert!(
            (auto - slot).num_seconds().abs() < 2,
            "auto_cutoff reads the same function"
        );

        let today = now.date_naive();
        let midnight = crate::time::date_to_utc_midnight;
        assert_eq!(
            DigestWindow::Today.bounds(now, 9, 18),
            (midnight(today), "today".to_string())
        );
        let yesterday = crate::time::parse_relative_date("yesterday").unwrap();
        assert_eq!(
            DigestWindow::Yesterday.bounds(now, 9, 18),
            (midnight(yesterday), format!("since {yesterday}"))
        );
        let week = crate::time::parse_relative_date("1w").unwrap();
        assert_eq!(
            DigestWindow::Week.bounds(now, 9, 18),
            (midnight(week), format!("since {week}"))
        );
    }

    #[test]
    fn the_slot_boundary_follows_the_work_hours() {
        let at = |h: u32| Local.with_ymd_and_hms(2026, 10, 27, h, 30, 0).unwrap();
        let local = |c: DateTime<Utc>| c.with_timezone(&Local).format("%m-%d %H:%M").to_string();
        // Evening: since work start today.
        assert_eq!(
            local(DigestWindow::Slot.bounds(at(20), 9, 18).0),
            "10-27 09:00"
        );
        // Work hours and night: since yesterday's work end.
        assert_eq!(
            local(DigestWindow::Slot.bounds(at(11), 9, 18).0),
            "10-26 18:00"
        );
        assert_eq!(
            local(DigestWindow::Slot.bounds(at(3), 9, 18).0),
            "10-26 18:00"
        );
    }
}
