//! The `ci` view: its data per repo, the `core` budget its calls bill, the
//! inspector's run bodies, and the `App` methods that poll, land and read
//! them (`app.ci`).
//!
//! Moved out of `app.rs` unchanged but for the field paths (FZ-118). The
//! methods stay on `App`: each reads the repo list, the GitHub sync, the
//! focused panel or the navigation. Design: `docs/app-split-design.md`,
//! "Owner types" (the per-view row), D5 and D7, owner item O-E.

use super::{App, FocusedPanel, HistoryView, View};
use crate::engine::{
    ApiBudget, DataTime, GithubBackoff, GithubFreshness, Observed, Outcome, Pace, RemoteKey,
};
use crate::model::{CiStatus, RepoStatus};
use chrono::{DateTime, Local};

/// The `ci` view's state, moved off `App` unchanged (FZ-118).
pub(crate) struct CiView {
    /// The `ci` view's data per remote identity, held for the session and
    /// never written to disk (FZ-30b). See `ci.rs`. Polled only while the
    /// view is on screen (`ci_wanted`); what an earlier visit read stays.
    pub(crate) states: std::collections::HashMap<RemoteKey, CiState>,
    /// The `core` budget the CI calls bill, apart from `github`'s, which
    /// is the GraphQL one the fleet poll paces on. One per session: the
    /// allowance is the account's, whichever repo spends it.
    pub(crate) budget: ApiBudget,
    /// The CI target the cursor rests on (`None` for a row without one) and
    /// since when, for the dwell (`ci::DWELL`).
    pub(crate) cursor: Option<(Option<RemoteKey>, std::time::Instant)>,
    /// The inspector body shown for the highlighted run (FZ-30f).
    pub(crate) body: CiBody,
    /// Scroll offset per body, indexed by `CiBody::index`; reset when the
    /// highlighted run changes.
    pub(crate) body_scrolls: [u16; 3],
    /// The job the `jobs` body marks, which the `log` body shows, once `j`
    /// or `k` moved it; `None` is the first failed job, else the first.
    pub(crate) job_mark: Option<usize>,
    /// The run the scrolls and the mark belong to, and since when it is
    /// highlighted, for the dwell before the inspector fetches anything.
    pub(crate) inspected: Option<(u64, std::time::Instant)>,
}

impl CiView {
    pub(crate) fn new() -> Self {
        Self {
            states: std::collections::HashMap::new(),
            budget: ApiBudget::default(),
            cursor: None,
            body: CiBody::default(),
            body_scrolls: [0; 3],
            job_mark: None,
            inspected: None,
        }
    }
}

/// A whole job log wrapped for one pane width, so a 5000-line log is not
/// re-wrapped on every redraw (FZ-30f).
#[derive(Debug, Clone)]
pub struct CiLogWrap {
    pub job_id: u64,
    pub width: u16,
    pub lines: Vec<ratatui::text::Line<'static>>,
}

/// The `ci` view's data for one repo (FZ-30b).
#[derive(Debug, Clone, Default)]
pub struct CiState {
    /// The newest list, newest run first.
    pub runs: Vec<crate::ci::WorkflowRun>,
    /// The list's `ETag`, sent back as `If-None-Match`.
    pub runs_etag: Option<String>,
    /// The run whose jobs are read on each poll: the highlighted run while
    /// the `ci` view shows and the inspector is open
    /// (`App::sync_ci_inspector`).
    pub expanded: Option<u64>,
    /// Each read run's jobs with their `ETag`, by run id.
    pub jobs: std::collections::HashMap<u64, CiJobs>,
    /// Runs whose `/jobs` answered 404, dropped by the next list.
    pub gone: std::collections::HashSet<u64>,
    /// Failed jobs' log tails by job id, each read once.
    pub logs: std::collections::HashMap<u64, crate::ci::LogTail>,
    /// Whole job logs by job id for the inspector's `log` body, each read
    /// once, by the poll that reads a failed job's tail or by the body
    /// (FZ-30f).
    pub full_logs: std::collections::HashMap<u64, crate::ci::Detail<Vec<String>>>,
    /// Workflow files by commit and path for the `script` body, each read
    /// once.
    pub scripts: std::collections::HashMap<(String, String), crate::ci::Detail<String>>,
    /// The inspector's own `/jobs` fetch per run, for a highlighted run no
    /// poll has read yet. The jobs land in `jobs`; this holds the fetch's
    /// state until they do, and its failure after.
    pub jobs_fetch: std::collections::HashMap<u64, crate::ci::Detail<()>>,
    /// The three freshness facts, for this repo's CI alone. A 304 is a
    /// successful attempt; the fleet header never reads this.
    pub freshness: GithubFreshness,
    /// A poll for this repo is out.
    pub inflight: bool,
    /// When the poll that holds `inflight` was claimed, so one that never
    /// answers can be given up on (`App::expire_stuck_ci_polls`).
    pub started: Option<std::time::Instant>,
    /// This repo's retry ladder and not-before, apart from the fleet's and
    /// from every other repo's, so one repo's failures never delay another
    /// repo's first poll.
    pub backoff: GithubBackoff,
    /// The default branch's tip and its contexts (FZ-83), read with the runs
    /// while the `ci` view shows this repo. Memory only.
    pub tip: Option<CiTip>,
}

/// The default branch's tip as the `ci` poll last read it (FZ-83).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CiTip {
    pub sha: String,
    pub status_etag: Option<String>,
    pub statuses: Vec<crate::ci::Context>,
    pub statuses_complete: bool,
    pub checks_etag: Option<String>,
    /// `None` until the check runs of this sha answered.
    pub checks: Option<crate::ci::TipChecks>,
    /// When both reads last answered for this sha, fresh or 304: the data
    /// time `App::ci_shown` weighs against the fleet poll's.
    pub confirmed: Option<DateTime<Local>>,
    /// Every state change of the tip's commit statuses, which dates their
    /// rows in the `ci` list.
    pub history: Vec<crate::ci::StatusEvent>,
    pub history_etag: Option<String>,
}

impl CiTip {
    /// Every context of the tip is held: all its statuses and check runs.
    pub fn complete(&self) -> bool {
        self.statuses_complete && self.checks.as_ref().is_some_and(|c| c.complete)
    }

    /// The statuses, then the check runs.
    pub fn contexts(&self) -> impl Iterator<Item = &crate::ci::Context> {
        self.statuses
            .iter()
            .chain(self.checks.iter().flat_map(|c| c.runs.iter()))
    }

    /// Land the tip reads' answer at `now`. A new sha starts over: the old
    /// tip's contexts say nothing about it.
    fn land(held: &mut Option<Self>, answer: crate::ci::TipAnswer, now: DateTime<Local>) {
        use crate::ci::Fetched;
        if let Fetched::Fresh { data, etag } = answer.status {
            if held.as_ref().is_none_or(|t| t.sha != data.sha) {
                *held = Some(Self {
                    sha: data.sha.clone(),
                    ..Self::default()
                });
            }
            if let Some(tip) = held.as_mut() {
                tip.status_etag = etag;
                tip.statuses = data.statuses;
                tip.statuses_complete = data.complete;
            }
        }
        let Some(tip) = held.as_mut() else {
            return;
        };
        if let Some((sha, checks)) = answer.checks
            && tip.sha == sha
        {
            if let Fetched::Fresh { data, etag } = checks {
                tip.checks = Some(data);
                tip.checks_etag = etag;
            }
            if tip.checks.is_some() {
                tip.confirmed = Some(now);
            }
        }
        if let Some((sha, Fetched::Fresh { data, etag })) = answer.history
            && tip.sha == sha
        {
            tip.history = data;
            tip.history_etag = etag;
        }
    }

    /// The tip's contexts the run list does not already show, as rows of
    /// it: every commit status, from its history, and every check run of
    /// an app other than GitHub Actions.
    pub fn context_runs(&self) -> Vec<crate::ci::ContextRun> {
        let statuses = self
            .statuses
            .iter()
            .map(|s| crate::ci::status_run(s, &self.history));
        let checks = self
            .checks
            .iter()
            .flat_map(|c| c.runs.iter())
            .filter(|c| !c.actions)
            .map(crate::ci::check_run);
        statuses.chain(checks).collect()
    }
}

/// One row of the `ci` view's list (FZ-83): a workflow run, or another
/// context of the default branch's tip, sorted together by time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiEntry<'a> {
    Run(&'a crate::ci::WorkflowRun),
    Context(crate::ci::ContextRun),
}

/// The CI state to show for a repo (FZ-83): what the row lamp, pane 2's
/// header and the `info` line all draw, from `App::ci_shown`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiShown {
    pub status: CiStatus,
    /// The default branch's name, once a held run on its tip names it.
    pub branch: Option<String>,
    /// The context to blame while not passed (`Vercel`, `Vercel +2`), known
    /// only when the verdict came from the tip's own contexts.
    pub culprit: Option<String>,
}

/// One run's jobs as last read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CiJobs {
    pub etag: Option<String>,
    pub jobs: Vec<crate::ci::Job>,
}

impl CiState {
    /// What one poll of this repo costs on `core`: the list, the tip's
    /// status and check runs (FZ-83), its status history while it has
    /// commit statuses, plus the expanded run's jobs.
    pub fn poll_cost(&self) -> u32 {
        let history = self.tip.as_ref().is_some_and(|t| !t.statuses.is_empty());
        crate::ci::CALL_COST * (3 + u32::from(history) + u32::from(self.expanded.is_some()))
    }

    /// Land an applied answer's data at `now`. The outcome is
    /// `App::apply_ci`'s.
    fn land(&mut self, mut answer: crate::ci::CiAnswer, now: DateTime<Local>) {
        use crate::ci::{Fetched, JobsAnswer};
        if let Some(tip) = answer.tip.take() {
            CiTip::land(&mut self.tip, tip, now);
        }
        if let Some(Fetched::Fresh { data, etag }) = answer.runs {
            let gone = std::mem::take(&mut self.gone);
            self.runs = data.into_iter().filter(|r| !gone.contains(&r.id)).collect();
            self.runs_etag = etag;
            let listed: std::collections::HashSet<u64> = self.runs.iter().map(|r| r.id).collect();
            self.jobs.retain(|id, _| listed.contains(id));
            if self.expanded.is_some_and(|id| !listed.contains(&id)) {
                self.expanded = None;
            }
        }
        match answer.jobs {
            Some((run_id, JobsAnswer::Fetched(Fetched::Fresh { data, etag }))) => {
                self.jobs.insert(run_id, CiJobs { etag, jobs: data });
            }
            Some((run_id, JobsAnswer::Gone)) => {
                self.jobs.remove(&run_id);
                self.gone.insert(run_id);
            }
            Some((_, JobsAnswer::Fetched(Fetched::NotModified))) | None => {}
        }
        if let Some((job_id, tail)) = answer.log {
            self.logs.insert(job_id, tail);
        }
        if let Some((job_id, lines)) = answer.full_log {
            self.full_logs
                .insert(job_id, crate::ci::Detail::Ready(lines));
        }
    }

    /// Land an inspector fetch's answer at `now` (FZ-30f).
    fn land_detail(&mut self, got: crate::ci::DetailGot, now: DateTime<Local>) {
        use crate::ci::{Detail, DetailGot, Fetched, JobsAnswer};
        match got {
            DetailGot::Jobs { run_id, result } => match result {
                Ok(JobsAnswer::Fetched(Fetched::Fresh { data, etag })) => {
                    self.jobs_fetch.remove(&run_id);
                    self.jobs.insert(run_id, CiJobs { etag, jobs: data });
                }
                Ok(JobsAnswer::Fetched(Fetched::NotModified)) => {
                    self.jobs_fetch.remove(&run_id);
                }
                Ok(JobsAnswer::Gone) => {
                    self.jobs_fetch.remove(&run_id);
                    self.jobs.remove(&run_id);
                    self.gone.insert(run_id);
                }
                Err(failure) => {
                    let held = Detail::from_result(Err::<(), _>(failure), now);
                    self.jobs_fetch.insert(run_id, held);
                }
            },
            DetailGot::Log {
                job_id,
                result,
                tail,
            } => {
                if let Some(tail) = tail {
                    self.logs.entry(job_id).or_insert(tail);
                }
                self.full_logs
                    .insert(job_id, Detail::from_result(result, now));
            }
            DetailGot::Script { sha, path, result } => {
                self.scripts
                    .insert((sha, path), Detail::from_result(result, now));
            }
        }
    }
}

/// The three bodies the inspector shows for the highlighted run while pane 2
/// shows `ci` (FZ-30f), cycled with `[` and `]` while the inspector has
/// focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CiBody {
    /// Each job with its lamp, duration and steps; a failed job's log tail.
    #[default]
    Jobs,
    /// The marked job's whole log.
    Log,
    /// The workflow file the run ran, the failing step highlighted.
    Script,
}

impl CiBody {
    pub const ALL: [Self; 3] = [Self::Jobs, Self::Log, Self::Script];

    /// Index into `CiView::body_scrolls`.
    pub fn index(self) -> usize {
        match self {
            Self::Jobs => 0,
            Self::Log => 1,
            Self::Script => 2,
        }
    }

    /// The word the inspector's title names it by.
    pub fn label(self) -> &'static str {
        match self {
            Self::Jobs => "jobs",
            Self::Log => "log",
            Self::Script => "script",
        }
    }
}

/// The `jobs` body's scroll against its job cursor: `j`/`k` and a new run
/// ask the next frame to bring the marked job into view (`Due`), the frame
/// records the offset it chose (`At`), and a scroll by the wheel or a page
/// key goes on from there and leaves the cursor behind (`Free`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobFollow {
    Free,
    Due,
    At(u16),
}

impl App {
    /// How long the cursor has rested on `target` at `now`, restarting the
    /// clock when it moved to another one.
    pub fn ci_settled_for(
        &mut self,
        target: Option<&RemoteKey>,
        now: std::time::Instant,
    ) -> std::time::Duration {
        match &self.ci.cursor {
            Some((held, since)) if held.as_ref() == target => now.duration_since(*since),
            _ => {
                self.ci.cursor = Some((target.cloned(), now));
                std::time::Duration::ZERO
            }
        }
    }

    /// The cadence inputs for the repo at `key`, judged at `now`.
    pub fn ci_cadence(
        &self,
        key: &RemoteKey,
        settled_for: std::time::Duration,
        now: DateTime<Local>,
    ) -> crate::ci::Cadence<'_> {
        let state = self.ci.states.get(key);
        let runs = state.map_or(&[][..], |s| &s.runs);
        let last_attempt = state.and_then(|s| s.freshness.last_attempt);
        let cost = state.map_or(crate::ci::CALL_COST, CiState::poll_cost);
        crate::ci::Cadence {
            folded: !self.ci_wanted(),
            has_identity: true,
            settled_for,
            inflight: state.is_some_and(|s| s.inflight),
            runs,
            last_attempt,
            pace: self.ci.budget.pace(cost, last_attempt.unwrap_or(now)),
            not_before: state
                .and_then(|s| s.backoff.not_before(crate::ci::base_interval_secs(runs))),
            now,
        }
    }

    /// When the CI rate limit on `key`'s repo lifts, if one holds it at
    /// `now`: its backoff's limit hold, or the `core` budget's reset once
    /// it is spent. The pane's `rate limited until 12:40`.
    pub fn ci_limit_until(&self, key: &RemoteKey, now: DateTime<Local>) -> Option<DateTime<Local>> {
        let state = self.ci.states.get(key)?;
        let interval = crate::ci::base_interval_secs(&state.runs);
        let limit = state.backoff.limit_until(interval).filter(|t| *t > now);
        let exhausted = self
            .ci
            .budget
            .exhausted(state.poll_cost(), now)
            .then(|| self.ci.budget.resume_at())
            .flatten();
        limit.max(exhausted)
    }

    /// The selected repo's CI target and state, as the `ci` view draws them:
    /// `None` when the row has no GitHub identity, and a `None` state while
    /// nothing was ever claimed for it.
    pub fn selected_ci(&self) -> Option<(RemoteKey, Option<&CiState>)> {
        let (key, ..) = self.selected_repo().and_then(crate::ci::target)?;
        let state = self.ci.states.get(&key);
        Some((key, state))
    }

    /// The CI state to show for `repo` (FZ-83), the one answer the row
    /// lamp, pane 2's header and the `info` line draw: the fleet poll's
    /// `ci_status`, unless every context of the default branch's tip is held
    /// from the `ci` view and was confirmed after the fleet poll's data time
    /// (`GithubFreshness::data_time`, `docs/freshness-contract.md`); then the
    /// verdict of those contexts, with its culprit. The branch is named
    /// whenever a held run ran on the held tip.
    pub fn ci_shown(&self, repo: &RepoStatus) -> CiShown {
        let state = crate::ci::target(repo).and_then(|(key, ..)| self.ci.states.get(&key));
        let tip = state.and_then(|s| Some((s, s.tip.as_ref()?)));
        let branch = tip.and_then(|(s, t)| {
            s.runs
                .iter()
                .find(|r| r.head_sha.as_deref() == Some(t.sha.as_str()))
                .and_then(|r| r.branch.clone())
        });
        let fresher = tip
            .map(|(_, t)| t)
            .filter(|t| t.complete() && self.newer_than_fleet(t.confirmed));
        match fresher {
            Some(t) => {
                let (status, culprit) = crate::ci::tip_verdict(t.contexts());
                CiShown {
                    status,
                    branch,
                    culprit,
                }
            }
            None => CiShown {
                status: repo.ci_status.clone(),
                branch,
                culprit: None,
            },
        }
    }

    /// Whether data confirmed at `at` is newer than the fleet poll's: later
    /// than its data time, or any time while that is unknown or absent.
    fn newer_than_fleet(&self, at: Option<DateTime<Local>>) -> bool {
        let Some(at) = at else {
            return false;
        };
        match self.github.freshness().data_time {
            DataTime::At(fleet) => at > fleet,
            DataTime::Unknown | DataTime::NoData => true,
        }
    }

    /// Whether `repo`'s row shows the second, dimmer mark (FZ-83): the held
    /// runs, newer than the fleet poll, have one in progress on a commit
    /// other than the default branch's held tip. Never while the tip is not
    /// known, since then no run can be told apart from the default branch's.
    pub fn ci_side_run(&self, repo: &RepoStatus) -> bool {
        let Some(state) = crate::ci::target(repo).and_then(|(key, ..)| self.ci.states.get(&key))
        else {
            return false;
        };
        let Some(tip) = &state.tip else {
            return false;
        };
        let runs_at = match state.freshness.data_time {
            DataTime::At(t) => Some(t),
            DataTime::Unknown | DataTime::NoData => None,
        };
        self.newer_than_fleet(runs_at)
            && state
                .runs
                .iter()
                .any(|r| r.is_live() && r.head_sha.as_deref() != Some(tip.sha.as_str()))
    }

    /// The selected repo's `ci` list as the view draws it (FZ-83): the held
    /// runs and the tip's other contexts (`CiTip::context_runs`), newest
    /// first by the time each row's age counts from, a run's creation or a
    /// context's `at`; a context with no time leads. One cursor stop each.
    pub fn ci_entries(&self) -> Vec<CiEntry<'_>> {
        let Some(state) = self.selected_ci().and_then(|(_, state)| state) else {
            return Vec::new();
        };
        let mut contexts = state
            .tip
            .as_ref()
            .map(CiTip::context_runs)
            .unwrap_or_default();
        contexts.sort_by(|a, b| match (a.at, b.at) {
            (Some(a), Some(b)) => b.cmp(&a),
            (a, b) => a.is_some().cmp(&b.is_some()),
        });
        let mut contexts = contexts.into_iter().peekable();
        let mut entries = Vec::new();
        for run in &state.runs {
            while let Some(context) = contexts.next_if(|c| c.at.is_none_or(|at| at >= run.created))
            {
                entries.push(CiEntry::Context(context));
            }
            entries.push(CiEntry::Run(run));
        }
        entries.extend(contexts.map(CiEntry::Context));
        entries
    }

    /// The entry under the `ci` view's cursor, clamped onto the last as the
    /// view draws it.
    fn highlighted_ci_entry(&self) -> Option<CiEntry<'_>> {
        let mut entries = self.ci_entries();
        let last = entries.len().checked_sub(1)?;
        let at = self.detail_selections[HistoryView::Ci.index()].min(last);
        Some(entries.swap_remove(at))
    }

    /// The other context under the `ci` view's cursor, if it is on one.
    pub fn highlighted_ci_context(&self) -> Option<crate::ci::ContextRun> {
        match self.highlighted_ci_entry()? {
            CiEntry::Context(context) => Some(context),
            CiEntry::Run(_) => None,
        }
    }

    /// The run under the `ci` view's cursor, clamped onto the last row as
    /// the view draws it; `None` while the selected repo holds no runs or
    /// the cursor is on another context's row.
    pub fn highlighted_ci_run(&self) -> Option<&crate::ci::WorkflowRun> {
        match self.highlighted_ci_entry()? {
            CiEntry::Run(run) => Some(run),
            CiEntry::Context(_) => None,
        }
    }

    /// Whether the CI data layer may poll: pane 2 is on screen showing the
    /// `ci` view. The one gate the poll trigger, the loading spinner and
    /// the stuck-poll expiry read. Never true for `info`, whose `ci` section
    /// draws only what is already held.
    pub fn ci_wanted(&self) -> bool {
        self.view == View::Detail && self.history_view == HistoryView::Ci
    }

    /// Whether the `ci` view draws the loading spinner, which repaints on
    /// every tick: the selected repo has a GitHub identity and no answer
    /// has landed for it yet.
    pub fn ci_view_loading(&self) -> bool {
        if !self.ci_wanted() || self.dirty_mode {
            return false;
        }
        match self.selected_ci() {
            Some((_, None)) => true,
            Some((_, Some(state))) => state.freshness.last_attempt.is_none(),
            None => false,
        }
    }

    /// Whether the `ci` view shows a run still moving, whose duration
    /// ticks and whose lamp pulses: about one repaint a second. A pending
    /// context's row (FZ-83) counts too.
    pub fn ci_view_live(&self) -> bool {
        self.ci_wanted()
            && self.ci_entries().iter().any(|entry| match entry {
                CiEntry::Run(run) => run.is_live(),
                CiEntry::Context(context) => context.state == CiStatus::Pending,
            })
    }

    /// Give up on CI polls claimed longer than `limit` before `instant` that
    /// never answered, as `GithubSync::expire_stuck` does for the
    /// fleet poll: the lock is released and the attempt recorded as a
    /// timed-out failure on that repo, which climbs its ladder. An answer
    /// that still arrives is applied like any.
    pub fn expire_stuck_ci_polls(
        &mut self,
        limit: std::time::Duration,
        instant: std::time::Instant,
        now: DateTime<Local>,
    ) {
        for state in self.ci.states.values_mut() {
            let stuck = state
                .started
                .is_some_and(|t| instant.saturating_duration_since(t) > limit);
            if !state.inflight || !stuck {
                continue;
            }
            state.inflight = false;
            state.started = None;
            state.freshness.gave_up(limit, now);
            if let Outcome::Failed(failure) = state.freshness.outcome.clone() {
                state
                    .backoff
                    .failed(&failure, now, &self.ci.budget, self.github.jitter_mut());
            }
            self.needs_redraw = true;
        }
    }

    /// Take the repo's in-flight lock and an attempt id, and build the poll.
    pub fn claim_ci_poll(
        &mut self,
        key: RemoteKey,
        owner: String,
        name: String,
    ) -> crate::ci::CiRequest {
        let state = self.ci.states.entry(key.clone()).or_default();
        state.inflight = true;
        state.started = Some(std::time::Instant::now());
        let attempt = state.freshness.begin_attempt();
        let expanded = state.expanded.map(|run_id| {
            let held = state.jobs.get(&run_id);
            crate::ci::ExpandedRequest {
                run_id,
                jobs_etag: held.and_then(|j| j.etag.clone()),
                held_jobs: held.map(|j| j.jobs.clone()).unwrap_or_default(),
                // A whole log the inspector holds or is reading brings its
                // tail with it, so the poll does not download it again.
                logs_held: state
                    .logs
                    .keys()
                    .chain(state.full_logs.iter().filter_map(|(id, held)| {
                        matches!(
                            held,
                            crate::ci::Detail::Loading | crate::ci::Detail::Ready(_)
                        )
                        .then_some(id)
                    }))
                    .copied()
                    .collect(),
            }
        });
        crate::ci::CiRequest {
            key,
            owner,
            name,
            attempt,
            runs_etag: state.runs_etag.clone(),
            expanded,
            tip: Some(crate::ci::TipRequest {
                sha: state.tip.as_ref().map(|t| t.sha.clone()),
                status_etag: state.tip.as_ref().and_then(|t| t.status_etag.clone()),
                checks_etag: state.tip.as_ref().and_then(|t| t.checks_etag.clone()),
                has_statuses: state.tip.as_ref().is_some_and(|t| !t.statuses.is_empty()),
                history_etag: state.tip.as_ref().and_then(|t| t.history_etag.clone()),
            }),
        }
    }

    /// Land a CI poll's answer (`AppEvent::Ci`) at `now`, and ask for a
    /// redraw. Only CI state changes (`ci.states`, `ci.budget`): never a row, the
    /// fleet poll's freshness, budget or backoff, or the cache.
    ///
    /// The fleet poll's rules, per repo: the newest attempt releases the lock;
    /// an answer older than one applied is discarded but may still lower the
    /// budget; a failure keeps the data and advances the repo's own retry
    /// clock; a 304 is a success that keeps the data. A list that answered
    /// (fresh or 304) advances the data time even when a later `/jobs` or log
    /// call failed the attempt: the runs on screen are that current, and the
    /// failure still marks the view and climbs the ladder.
    pub fn apply_ci(&mut self, answer: crate::ci::CiAnswer, now: DateTime<Local>) {
        self.needs_redraw = true;
        let Some(state) = self.ci.states.get_mut(&answer.key) else {
            return;
        };
        if state.freshness.is_latest(answer.attempt) {
            state.inflight = false;
            state.started = None;
        }
        let applied = state.freshness.accept_answer(answer.attempt);
        if let Some(reading) = &answer.budget
            && self
                .ci
                .budget
                .observe(reading, applied, now, self.github.jitter_mut())
                == Observed::Switched
        {
            for state in self.ci.states.values_mut() {
                state.backoff.account_switched();
            }
        }
        if !applied {
            return;
        }
        let Some(state) = self.ci.states.get_mut(&answer.key) else {
            return;
        };
        match answer.failure.clone() {
            Some(failure) => {
                state.freshness.failed(failure.clone(), now);
                if answer.runs.is_some() {
                    state.freshness.data_time = DataTime::At(now);
                }
                state
                    .backoff
                    .failed(&failure, now, &self.ci.budget, self.github.jitter_mut());
            }
            None => {
                state.freshness.succeeded(now);
                state.backoff.succeeded();
            }
        }
        let key = answer.key.clone();
        state.land(answer, now);
        // A shorter list for the repo on screen pulls the `ci` cursor onto
        // its last run rather than leaving it past the end.
        if self
            .selected_ci()
            .is_some_and(|(selected, _)| selected == key)
        {
            let rows = self.ci_entries().len();
            let slot = &mut self.detail_selections[HistoryView::Ci.index()];
            *slot = (*slot).min(rows.saturating_sub(1));
            self.sync_ci_inspector(std::time::Instant::now());
        }
    }

    /// Land an inspector fetch's answer (`AppEvent::CiDetail`) at `now`, and
    /// ask for a redraw. Its budget reading is taken like a poll's.
    pub fn apply_ci_detail(&mut self, answer: crate::ci::DetailAnswer, now: DateTime<Local>) {
        self.needs_redraw = true;
        if let Some(reading) = &answer.budget
            && self
                .ci
                .budget
                .observe(reading, true, now, self.github.jitter_mut())
                == Observed::Switched
        {
            for state in self.ci.states.values_mut() {
                state.backoff.account_switched();
            }
        }
        if let Some(state) = self.ci.states.get_mut(&answer.key) {
            state.land_detail(answer.got, now);
        }
    }

    /// The run the `ci` view's cursor is on for the selected repo, which the
    /// inspector details: read from the view's own selection slot, so it
    /// holds while another view is on screen.
    pub fn ci_highlighted_run(&self) -> Option<&crate::ci::WorkflowRun> {
        self.highlighted_ci_run()
    }

    /// The highlighted run's jobs, once read.
    pub fn ci_highlighted_jobs(&self) -> Option<&[crate::ci::Job]> {
        let run = self.ci_highlighted_run()?;
        let state = self.selected_ci()?.1?;
        state.jobs.get(&run.id).map(|j| &j.jobs[..])
    }

    /// Whether the scrolls and the mark belong to the highlighted run. Until
    /// `sync_ci_inspector` catches up with a change, they read as reset.
    fn ci_run_is_inspected(&self) -> bool {
        let run = self.ci_highlighted_run().map(|r| r.id);
        run.is_some() && run == self.ci.inspected.map(|(id, _)| id)
    }

    /// The index of the job the `jobs` body marks: where `j`/`k` left it,
    /// else the first failed job, else the first.
    pub fn ci_marked_job_index(&self) -> Option<usize> {
        let jobs = self.ci_highlighted_jobs()?;
        let last = jobs.len().checked_sub(1)?;
        let moved = self.ci.job_mark.filter(|_| self.ci_run_is_inspected());
        Some(match moved {
            Some(i) => i.min(last),
            None => jobs
                .iter()
                .position(|j| j.conclusion.as_deref() == Some("failure"))
                .unwrap_or(0),
        })
    }

    /// The marked job, whose log the `log` body shows.
    pub fn ci_marked_job(&self) -> Option<&crate::ci::Job> {
        self.ci_highlighted_jobs()?.get(self.ci_marked_job_index()?)
    }

    /// The shown body's scroll offset: zero for a run just highlighted. The
    /// `jobs` body's is where it last followed its cursor to, until a scroll
    /// moves it on.
    pub fn ci_body_scroll(&self) -> u16 {
        if !self.ci_run_is_inspected() {
            return 0;
        }
        match (self.ci.body, self.drawn.ci_job_follow.get()) {
            (CiBody::Jobs, JobFollow::At(followed)) => followed,
            _ => self.ci.body_scrolls[self.ci.body.index()],
        }
    }

    /// Catch up with the highlighted run at `instant`: a new run resets every
    /// body's scroll and the job mark, brings the marked job into view and
    /// restarts the dwell. While the `ci` view shows, the selected repo's
    /// poll reads the highlighted run's jobs when the inspector is open, and
    /// no run's when it is folded.
    pub fn sync_ci_inspector(&mut self, instant: std::time::Instant) {
        let run = self.ci_highlighted_run().map(|r| r.id);
        if run != self.ci.inspected.map(|(id, _)| id) {
            self.ci.inspected = run.map(|id| (id, instant));
            self.ci.body_scrolls = [0; 3];
            self.ci.job_mark = None;
            self.drawn.ci_job_follow.set(JobFollow::Due);
        }
        if !self.ci_wanted() {
            return;
        }
        let expanded = run.filter(|_| !self.inspector_folded);
        if let Some((key, _)) = self.selected_ci()
            && let Some(state) = self.ci.states.get_mut(&key)
        {
            state.expanded = expanded;
        }
    }

    /// `l` on the highlighted run in the `ci` view (FZ-30e, amended
    /// 2026-10-22): the inspector, opened if folded and focused as the
    /// pane's number key does, on the run's `log` body, which shows the
    /// marked job (the failed one, until `j`/`k` move it). Stays in fleetz
    /// rather than suspending into `gh run view --log`.
    pub fn inspect_ci_log(&mut self) {
        self.inspector_folded = false;
        self.focus_panel(FocusedPanel::Inspector);
        self.sync_ci_inspector(std::time::Instant::now());
        self.ci.body = CiBody::Log;
        self.needs_redraw = true;
    }

    /// Whether the inspector shows a run body: pane 2 shows `ci` and the
    /// inspector is open.
    pub fn inspects_ci_run(&self) -> bool {
        self.ci_wanted() && !self.inspector_folded
    }

    /// `[` or `]`: the next inspector body while the inspector has focus and
    /// holds a run, else pane 2's next view as always. A streak of them
    /// begun on the focused inspector in another view cycles on through
    /// `ci` (`brackets_cycling_views`); any other key ends it.
    pub fn press_bracket(&mut self, back: bool) {
        let on_inspector = self.focused_panel == FocusedPanel::Inspector;
        let holds_run = self.inspects_ci_run() && self.ci_highlighted_run().is_some();
        if !on_inspector || !holds_run || self.brackets_cycling_views {
            self.cycle_history_view(back);
            self.brackets_cycling_views = on_inspector;
            return;
        }
        self.sync_ci_inspector(std::time::Instant::now());
        let n = CiBody::ALL.len();
        let i = self.ci.body.index();
        let next = if back { (i + n - 1) % n } else { (i + 1) % n };
        self.ci.body = CiBody::ALL[next];
        self.needs_redraw = true;
    }

    /// `j`/`k` on the focused inspector: in the `jobs` body they move the
    /// job mark, and the body follows it; anywhere else they scroll.
    pub fn inspector_nav(&mut self, delta: i32) {
        if !self.inspects_ci_run() || self.ci.body != CiBody::Jobs {
            self.scroll_inspector(delta);
            return;
        }
        self.sync_ci_inspector(std::time::Instant::now());
        let Some(current) = self.ci_marked_job_index() else {
            return;
        };
        let last = self.ci_highlighted_jobs().map_or(0, |j| j.len() - 1);
        let next = (current as i64 + i64::from(delta)).clamp(0, last as i64) as usize;
        self.ci.job_mark = Some(next);
        // The follow starts from where the body stands, not where it was
        // last scrolled by hand.
        self.ci.body_scrolls[CiBody::Jobs.index()] = self.ci_body_scroll();
        self.drawn.ci_job_follow.set(JobFollow::Due);
        self.needs_redraw = true;
    }

    /// The inspector fetches the shown body needs for the highlighted run,
    /// each claimed (marked loading) so it starts once: the run's jobs when
    /// no poll has read them, the marked job's whole log once the job has
    /// finished, the workflow file at the run's commit (and the jobs, for
    /// the failing step's highlight, when the run failed). Nothing outside
    /// the `ci` view, under `-d`, with the inspector folded, before the run
    /// has been highlighted for `ci::DWELL`, or while the `core` budget's
    /// pace is paused (at or below the reserve, or spent).
    pub fn claim_ci_details(
        &mut self,
        now: DateTime<Local>,
        instant: std::time::Instant,
    ) -> Vec<crate::ci::DetailRequest> {
        use crate::ci::{Detail, DetailRequest, DetailWant};
        if !self.inspects_ci_run() || self.dirty_mode {
            return Vec::new();
        }
        self.sync_ci_inspector(instant);
        let Some((_, since)) = self.ci.inspected else {
            return Vec::new();
        };
        if instant.saturating_duration_since(since) < crate::ci::DWELL {
            return Vec::new();
        }
        // The poll's own pace: at or below the reserve, or with too little
        // left for one call (spent), paused until the resume.
        if let Pace::Paused { until } = self.ci.budget.pace(crate::ci::CALL_COST, now)
            && now < until
        {
            return Vec::new();
        }
        let Some((key, owner, name)) = self.selected_repo().and_then(crate::ci::target) else {
            return Vec::new();
        };
        let Some(run) = self.ci_highlighted_run().cloned() else {
            return Vec::new();
        };
        let clone = self
            .selected_repo()
            .and_then(|r| r.path.as_deref())
            .map(|p| std::path::PathBuf::from(shellexpand::tilde(p).to_string()));
        let marked = self.ci_marked_job().cloned();
        let Some(state) = self.ci.states.get_mut(&key) else {
            return Vec::new();
        };
        let mut wants = Vec::new();
        let failed = crate::ui::ci::run_status(&run) == crate::model::CiStatus::Failed;
        let needs_jobs = match self.ci.body {
            CiBody::Jobs | CiBody::Log => true,
            CiBody::Script => failed,
        };
        if needs_jobs
            && !state.jobs.contains_key(&run.id)
            && !state.gone.contains(&run.id)
            && Detail::wants_fetch(state.jobs_fetch.get(&run.id), now)
        {
            state.jobs_fetch.insert(run.id, Detail::Loading);
            wants.push(DetailWant::Jobs { run_id: run.id });
        }
        // The jobs body needs the marked job's log only for a failed job's
        // tail; the log body needs any finished job's. A poll in flight may
        // bring a failed job's log with its jobs, so a claim waits for it
        // rather than download the same log twice.
        let wants_log = |job: &crate::ci::Job| match self.ci.body {
            CiBody::Jobs => {
                job.conclusion.as_deref() == Some("failure")
                    && crate::ci::failing_step(job).is_some()
                    && !state.logs.contains_key(&job.id)
            }
            CiBody::Log => true,
            CiBody::Script => false,
        };
        match self.ci.body {
            CiBody::Jobs | CiBody::Log => {
                if let Some(job) = marked
                    && wants_log(&job)
                    && job.status == "completed"
                    && !state.inflight
                    && Detail::wants_fetch(state.full_logs.get(&job.id), now)
                {
                    let step = crate::ci::failing_step(&job)
                        .filter(|_| !state.logs.contains_key(&job.id))
                        .cloned();
                    state.full_logs.insert(job.id, Detail::Loading);
                    wants.push(DetailWant::Log {
                        job_id: job.id,
                        step,
                    });
                }
            }
            CiBody::Script => {
                if let (Some(sha), Some(path)) = (run.head_sha.clone(), run.path.clone()) {
                    let held = (sha.clone(), path.clone());
                    if Detail::wants_fetch(state.scripts.get(&held), now) {
                        state.scripts.insert(held, Detail::Loading);
                        wants.push(DetailWant::Script { sha, path, clone });
                    }
                }
            }
        }
        wants
            .into_iter()
            .map(|want| DetailRequest {
                key: key.clone(),
                owner: owner.clone(),
                name: name.clone(),
                want,
            })
            .collect()
    }
}
