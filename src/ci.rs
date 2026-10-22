//! The `ci` view's data (FZ-30b): the selected repo's recent workflow runs,
//! the highlighted run's jobs and steps, and a failed job's log tail, read
//! from the REST Actions API; and the inspector's own fetches for that run, a
//! job's whole log and the workflow file (FZ-30f). See
//! `docs/superpowers/specs/2026-10-21-ci-pane-design.md`, sections C and D
//! and "Amendment 2026-10-21".
//!
//! REST, not GraphQL: no GraphQL connection lists a repo's runs across
//! branches by recency. The calls bill the `core` resource, which the fleet
//! poll's GraphQL budget never sees, so they are paced on a budget of their
//! own (`App::ci_budget`). Every URL remembers its `ETag` and sends
//! `If-None-Match`: a `304 Not Modified` costs nothing (measured on #67 and
//! again on #90) and confirms the data in hand.
//!
//! Memory only. Nothing here touches the disk cache: a new file format would
//! be a contract change, and the data is seconds old by design.

use chrono::{DateTime, Local, Utc};
use serde::Deserialize;

use crate::app::{Pace, RemoteKey};
use crate::github::{BudgetReading, Failure, FailureKind};

/// Where the REST API lives.
pub const API_ROOT: &str = "https://api.github.com";
/// Runs per list call. `per_page=20` measured 234 KB, since each run embeds
/// two repository objects; 10 is half that and fills the view's first rows.
pub const RUNS_PER_PAGE: u32 = 10;
/// How long the cursor must rest on a repo before its first poll, so a `j`
/// run through the list sends nothing.
pub const DWELL: std::time::Duration = std::time::Duration::from_millis(300);
/// The interval while any shown run is in progress or queued.
pub const LIVE_SECS: i64 = 10;
/// The interval once every shown run has finished.
pub const SETTLED_SECS: i64 = 60;
/// Log lines kept from the failing step.
pub const LOG_TAIL_LINES: usize = 12;
/// Lines of a whole job log held for the `log` body, the last ones kept, so
/// one runaway log cannot hold the session's memory (FZ-30f).
pub const LOG_LINE_CAP: usize = 5000;
/// The first line of a log cut to `LOG_LINE_CAP`.
pub const LOG_TRUNCATED: &str = "log truncated to the last 5000 lines; l opens the whole log in gh";
/// What one REST call costs on `core`. A 304 costs nothing, but the pace is
/// judged before the answer is known.
pub const CALL_COST: u32 = 1;
/// How long the `gh auth token` lookup may take. The requests themselves are
/// bounded by the shared client's deadlines, so with this every poll ends.
const TOKEN_BOUND: std::time::Duration = std::time::Duration::from_secs(10);

/// One workflow run, as `GET /repos/{owner}/{repo}/actions/runs` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkflowRun {
    pub id: u64,
    /// A re-run keeps the id and raises this.
    #[serde(rename = "run_attempt", default = "first_attempt")]
    pub attempt: u32,
    /// The workflow's name. Nullable in GitHub's schema.
    #[serde(rename = "name", default)]
    pub workflow: Option<String>,
    #[serde(rename = "display_title")]
    pub title: String,
    /// The branch, or the tag for a tag push. Null for some events.
    #[serde(rename = "head_branch", default)]
    pub branch: Option<String>,
    pub event: String,
    /// `queued`, `in_progress`, `completed`, and the rarer `requested`,
    /// `waiting` and `pending`. Nullable in GitHub's schema.
    #[serde(default)]
    pub status: Option<String>,
    /// Set once the run completed: `success`, `failure`, `cancelled`, ...
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(rename = "created_at")]
    pub created: DateTime<Utc>,
    #[serde(rename = "run_started_at", default)]
    pub started: Option<DateTime<Utc>>,
    #[serde(rename = "updated_at")]
    pub updated: DateTime<Utc>,
    #[serde(rename = "html_url")]
    pub url: String,
    /// The commit the run ran on, at which the `script` body reads the
    /// workflow file (FZ-30f).
    #[serde(default)]
    pub head_sha: Option<String>,
    /// The workflow file, `.github/workflows/ci.yml`.
    #[serde(default)]
    pub path: Option<String>,
}

fn first_attempt() -> u32 {
    1
}

impl WorkflowRun {
    /// Whether the run is still moving, which keeps the poll at `LIVE_SECS`.
    pub fn is_live(&self) -> bool {
        matches!(self.status.as_deref(), Some("in_progress" | "queued"))
    }
}

/// One job of a run, as `GET /repos/{owner}/{repo}/actions/runs/{id}/jobs`
/// lists it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Job {
    pub id: u64,
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(rename = "started_at", default)]
    pub started: Option<DateTime<Utc>>,
    #[serde(rename = "completed_at", default)]
    pub completed: Option<DateTime<Utc>>,
    /// Absent or empty while the job is queued.
    #[serde(default)]
    pub steps: Vec<Step>,
}

/// One step of a job.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Step {
    pub number: u32,
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub conclusion: Option<String>,
    /// Null until the step starts.
    #[serde(rename = "started_at", default)]
    pub started: Option<DateTime<Utc>>,
    #[serde(rename = "completed_at", default)]
    pub completed: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct RunsPage {
    workflow_runs: Vec<WorkflowRun>,
}

#[derive(Deserialize)]
struct JobsPage {
    jobs: Vec<Job>,
}

/// The runs a list body holds, newest first as GitHub sends them.
pub fn parse_runs(body: &str) -> Result<Vec<WorkflowRun>, Failure> {
    serde_json::from_str::<RunsPage>(body)
        .map(|page| page.workflow_runs)
        .map_err(|e| malformed("workflow runs", &e))
}

/// The jobs a `/jobs` body holds.
pub fn parse_jobs(body: &str) -> Result<Vec<Job>, Failure> {
    serde_json::from_str::<JobsPage>(body)
        .map(|page| page.jobs)
        .map_err(|e| malformed("jobs", &e))
}

fn malformed(what: &str, error: &serde_json::Error) -> Failure {
    Failure::new(
        FailureKind::Malformed,
        format!("failed to parse the {what}: {error}"),
    )
}

/// The step that failed the job: the first one concluded `failure`.
pub fn failing_step(job: &Job) -> Option<&Step> {
    job.steps
        .iter()
        .find(|step| step.conclusion.as_deref() == Some("failure"))
}

/// The last `LOG_TAIL_LINES` lines `step` wrote into the job's `log`, with the
/// runner's timestamps and the terminal colour codes stripped.
///
/// The cut is the step's `started..completed` window. Step times are whole
/// seconds while log lines carry fractions, so the window runs to the end of
/// the completing second, and two edges are sharpened from the log itself:
/// the step's own `##[group]` header, when the window holds one, starts it
/// (a step started in the second its predecessor ended), and the runner's
/// last `##[error]` line ends it (the steps skipped after a failure, and the
/// job's cleanup, all start in the second it failed). Measured on run
/// 37958308363: without that end, the tail was sixteen lines of `Post job
/// cleanup` and none of clippy's error.
pub fn log_tail(log: &str, step: &Step) -> Vec<String> {
    let (Some(start), Some(end)) = (step.started, step.completed) else {
        return Vec::new();
    };
    let end = end + chrono::Duration::seconds(1);
    let mut stamp: Option<DateTime<Utc>> = None;
    let mut window: Vec<String> = Vec::new();
    for raw in log.trim_start_matches('\u{feff}').lines() {
        let (at, text) = split_stamp(raw.trim_end_matches('\r'));
        // A line without a stamp continues the one before it.
        if at.is_some() {
            stamp = at;
        }
        if stamp.is_some_and(|t| t >= start && t < end) {
            window.push(strip_ansi(text));
        }
    }
    let header = format!("##[group]{}", step.name);
    if let Some(first) = window.iter().position(|line| *line == header) {
        window.drain(..first);
    }
    if let Some(last) = window
        .iter()
        .rposition(|line| line.starts_with("##[error]"))
    {
        window.truncate(last + 1);
    }
    let skip = window.len().saturating_sub(LOG_TAIL_LINES);
    window.split_off(skip)
}

/// A job's whole log as the inspector's `log` body shows it: one entry per
/// line, the runner's timestamps and the terminal colour codes stripped.
/// Past `LOG_LINE_CAP` lines only the last ones are kept, the end being
/// where a failure is, under a first line that says so (`LOG_TRUNCATED`).
pub fn log_lines(log: &str) -> Vec<String> {
    let all: Vec<&str> = log.trim_start_matches('\u{feff}').lines().collect();
    let skip = all.len().saturating_sub(LOG_LINE_CAP);
    let mut lines = Vec::with_capacity(all.len() - skip + 1);
    if skip > 0 {
        lines.push(LOG_TRUNCATED.to_string());
    }
    lines.extend(
        all[skip..]
            .iter()
            .map(|raw| strip_ansi(split_stamp(raw.trim_end_matches('\r')).1)),
    );
    lines
}

/// A log line's leading `2026-10-09T16:20:37.5048777Z ` and the rest.
fn split_stamp(line: &str) -> (Option<DateTime<Utc>>, &str) {
    let (head, rest) = line.split_once(' ').unwrap_or((line, ""));
    match DateTime::parse_from_rfc3339(head) {
        Ok(at) => (Some(at.with_timezone(&Utc)), rest),
        Err(_) => (None, line),
    }
}

/// `text` without its CSI escape sequences (`ESC [ ... final`), which a
/// terminal pane would otherwise interpret.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            // Parameter and intermediate bytes, then one final byte.
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }
    out
}

/// What a conditional call returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched<T> {
    /// A 200: new data, and the `ETag` to send next time.
    Fresh { data: T, etag: Option<String> },
    /// A 304: what is held is current. Free, and it changes nothing.
    NotModified,
}

/// What the expanded run's `/jobs` returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobsAnswer {
    Fetched(Fetched<Vec<Job>>),
    /// A 404: the run was deleted. The inspector says `run gone` until the next
    /// list poll drops it.
    Gone,
}

/// A failed job's log tail, fetched once per job id and kept for the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogTail {
    Lines(Vec<String>),
    /// GitHub answered for good that there is no log (expired, removed); the
    /// message says how. Not asked again.
    Unavailable(String),
}

/// The expanded run a poll also reads, with what is held for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandedRequest {
    pub run_id: u64,
    pub jobs_etag: Option<String>,
    /// The jobs held, so a 304 still names the failed job whose log is owed.
    pub held_jobs: Vec<Job>,
    /// Job ids whose log tail is already held.
    pub logs_held: Vec<u64>,
}

/// One CI poll to run, built by `App::claim_ci_poll`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiRequest {
    pub key: RemoteKey,
    pub owner: String,
    pub name: String,
    /// The id from the repo's `GithubFreshness::begin_attempt`.
    pub attempt: u64,
    pub runs_etag: Option<String>,
    pub expanded: Option<ExpandedRequest>,
}

/// One CI poll's answer, carried by `AppEvent::Ci`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiAnswer {
    pub key: RemoteKey,
    pub attempt: u64,
    /// `None` when the poll failed before the list answered.
    pub runs: Option<Fetched<Vec<WorkflowRun>>>,
    pub jobs: Option<(u64, JobsAnswer)>,
    pub log: Option<(u64, LogTail)>,
    /// The same log whole (`log_lines`), for the inspector's `log` body, so
    /// a job's log is downloaded once for both (FZ-30f).
    pub full_log: Option<(u64, Vec<String>)>,
    /// What failed the attempt, if anything did. Whatever answered before it
    /// is still applied.
    pub failure: Option<Failure>,
    /// The newest `core` reading the answers' headers carried.
    pub budget: Option<BudgetReading>,
}

/// The repo a row's CI lives in: its remote key, and the owner and name for
/// the URL. `None` for a row with no GitHub identity, or one known only by
/// name (a legacy row with no owner), which has no URL.
pub fn target(repo: &crate::model::RepoStatus) -> Option<(RemoteKey, String, String)> {
    let key = crate::app::carrier_key(repo)?;
    let owner = key.0.clone()?;
    let name = key.1.clone();
    Some((key, owner, name))
}

/// The interval the shown runs ask for: `LIVE_SECS` while any is in
/// progress or queued, `SETTLED_SECS` once all have finished (or there are
/// none).
pub fn base_interval_secs(runs: &[WorkflowRun]) -> i64 {
    if runs.iter().any(WorkflowRun::is_live) {
        LIVE_SECS
    } else {
        SETTLED_SECS
    }
}

/// `base_interval_secs`, stretched when the `core` budget spreads polls
/// further apart (the `Pace` ladder of `docs/api-budget-design.md`).
pub fn interval_secs(runs: &[WorkflowRun], pace: Pace) -> i64 {
    let base = base_interval_secs(runs);
    match pace {
        Pace::Every(secs) => base.max(secs),
        Pace::Unknown | Pace::Paused { .. } => base,
    }
}

/// Everything the poll decision reads, so it is a pure function.
#[derive(Debug, Clone)]
pub struct Cadence<'a> {
    /// Pane 2 is not showing `ci` (`App::ci_wanted`): nothing polls.
    pub folded: bool,
    /// The selected row has a GitHub identity (`target`).
    pub has_identity: bool,
    /// How long the cursor has rested on this repo.
    pub settled_for: std::time::Duration,
    /// A poll for this repo is out.
    pub inflight: bool,
    /// The runs held for this repo, empty when none are.
    pub runs: &'a [WorkflowRun],
    /// This repo's last attempt; `None` when it was never polled.
    pub last_attempt: Option<DateTime<Local>>,
    /// The `core` budget's pace.
    pub pace: Pace,
    /// The CI backoff's not-before (`GithubBackoff::not_before`).
    pub not_before: Option<DateTime<Local>>,
    pub now: DateTime<Local>,
}

/// Whether the selected repo's CI poll should start now: never while folded,
/// without an identity, or with one already out; only once the cursor has
/// rested `DWELL`; then when the repo was never polled or its interval has
/// passed, unless the budget pauses polling or the backoff holds it.
pub fn poll_due(c: &Cadence) -> bool {
    if c.folded || !c.has_identity || c.inflight || c.settled_for < DWELL {
        return false;
    }
    if let Pace::Paused { until } = c.pace
        && c.now < until
    {
        return false;
    }
    if c.not_before.is_some_and(|t| c.now < t) {
        return false;
    }
    let every = interval_secs(c.runs, c.pace);
    c.last_attempt
        .is_none_or(|t| (c.now - t).num_seconds() >= every)
}

/// The `gh` token, looked up once per session and kept in memory only, so a
/// poll every 10 s does not spawn `gh auth token` every 10 s. Forgotten when
/// an answer says the token is no good, so the next poll asks `gh` again.
pub struct TokenCache(std::sync::Mutex<Option<String>>);

impl TokenCache {
    pub const fn new() -> Self {
        Self(std::sync::Mutex::new(None))
    }

    /// The held token, or a fresh one from `source`, kept when it answers.
    /// The lock is never held across the lookup.
    pub async fn token<S, F>(&self, source: S) -> Result<String, String>
    where
        S: FnOnce() -> F,
        F: std::future::Future<Output = Result<String, String>>,
    {
        if let Some(token) = self.held() {
            return Ok(token);
        }
        let token = source().await?;
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(token.clone());
        Ok(token)
    }

    fn held(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Drop the held token when `answer` failed on it: `gh` had none
    /// (`Auth`), or GitHub refused it (`Refused`, a 401 or a 403 that is not
    /// a limit). A token `gh` refreshed or re-issued is then picked up.
    pub fn forget_after(&self, answer: &CiAnswer) {
        self.forget_on(answer.failure.as_ref());
    }

    /// `forget_after` for any call's failure, the inspector's fetches too.
    pub fn forget_on(&self, failure: Option<&Failure>) {
        let refused =
            failure.is_some_and(|f| matches!(f.kind, FailureKind::Auth | FailureKind::Refused));
        if refused {
            *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }
}

impl Default for TokenCache {
    fn default() -> Self {
        Self::new()
    }
}

/// The session's `gh` token for the CI calls.
static SESSION_TOKEN: TokenCache = TokenCache::new();

/// Run one CI poll against GitHub with the session's `gh` token.
pub async fn poll(request: CiRequest) -> CiAnswer {
    let answer = poll_with(
        request,
        || SESSION_TOKEN.token(crate::github::get_gh_token),
        crate::github::client(),
        API_ROOT,
    )
    .await;
    SESSION_TOKEN.forget_after(&answer);
    answer
}

/// `poll` with the token source, the client and the API root passed in, so a
/// test drives every path on the loopback.
///
/// The list first; then the expanded run's jobs; then, once per job id, the
/// log of its first failed job. A failure stops the poll where it happened
/// and is the attempt's outcome; what answered before it is kept.
async fn poll_with<S, F>(
    request: CiRequest,
    token_source: S,
    client: &reqwest::Client,
    root: &str,
) -> CiAnswer
where
    S: FnOnce() -> F,
    F: std::future::Future<Output = Result<String, String>>,
{
    let mut answer = CiAnswer {
        key: request.key.clone(),
        attempt: request.attempt,
        runs: None,
        jobs: None,
        log: None,
        full_log: None,
        failure: None,
        budget: None,
    };
    let token = match token_within(token_source).await {
        Ok(token) => token,
        Err(failure) => {
            answer.failure = Some(failure);
            return answer;
        }
    };
    let base = format!("{root}/repos/{}/{}/actions", request.owner, request.name);

    let url = format!("{base}/runs?per_page={RUNS_PER_PAGE}");
    let (result, reading) = get(client, &url, &token, request.runs_etag.as_deref()).await;
    answer.budget = reading.or(answer.budget);
    answer.runs = match result {
        Ok(Answer::Fresh { body, etag }) => match parse_runs(&body) {
            Ok(data) => Some(Fetched::Fresh { data, etag }),
            Err(failure) => {
                answer.failure = Some(failure);
                return answer;
            }
        },
        Ok(Answer::NotModified) => Some(Fetched::NotModified),
        Ok(Answer::NotFound) => {
            answer.failure = Some(Failure::new(
                FailureKind::Http,
                "GitHub API returned 404 Not Found: no workflow runs readable here",
            ));
            return answer;
        }
        Err(failure) => {
            answer.failure = Some(failure);
            return answer;
        }
    };

    let Some(expanded) = request.expanded else {
        return answer;
    };
    let url = format!("{base}/runs/{}/jobs", expanded.run_id);
    let (result, reading) = get(client, &url, &token, expanded.jobs_etag.as_deref()).await;
    answer.budget = reading.or(answer.budget);
    let jobs = match result {
        Ok(Answer::Fresh { body, etag }) => match parse_jobs(&body) {
            Ok(data) => {
                answer.jobs = Some((
                    expanded.run_id,
                    JobsAnswer::Fetched(Fetched::Fresh {
                        data: data.clone(),
                        etag,
                    }),
                ));
                data
            }
            Err(failure) => {
                answer.failure = Some(failure);
                return answer;
            }
        },
        Ok(Answer::NotModified) => {
            answer.jobs = Some((expanded.run_id, JobsAnswer::Fetched(Fetched::NotModified)));
            expanded.held_jobs
        }
        Ok(Answer::NotFound) => {
            answer.jobs = Some((expanded.run_id, JobsAnswer::Gone));
            return answer;
        }
        Err(failure) => {
            answer.failure = Some(failure);
            return answer;
        }
    };

    let owed = jobs.iter().find_map(|job| {
        let failed = job.conclusion.as_deref() == Some("failure");
        let step = failing_step(job)?;
        (failed && !expanded.logs_held.contains(&job.id)).then_some((job.id, step))
    });
    let Some((job_id, step)) = owed else {
        return answer;
    };
    // A redirect to a download, which the client follows; it drops the
    // `Authorization` header on the way to the other host.
    let url = format!("{base}/jobs/{job_id}/logs");
    let (result, reading) = get(client, &url, &token, None).await;
    answer.budget = reading.or(answer.budget);
    match result {
        Ok(Answer::Fresh { body, .. }) => {
            answer.log = Some((job_id, LogTail::Lines(log_tail(&body, step))));
            answer.full_log = Some((job_id, log_lines(&body)));
        }
        Ok(Answer::NotModified) => {}
        Ok(Answer::NotFound) => {
            let gone = LogTail::Unavailable("log not found".to_string());
            answer.log = Some((job_id, gone));
        }
        // A client error that is not a limit (a 410 for an expired log) is
        // GitHub's final word; everything else may clear and is asked again.
        Err(failure) if failure.kind == FailureKind::Http && !failure.rate_limited => {
            answer.log = Some((job_id, LogTail::Unavailable(failure.message)));
        }
        Err(failure) => answer.failure = Some(failure),
    }
    answer
}

/// The token from `source`, bounded by `TOKEN_BOUND`; an `Auth` failure when
/// `gh` has none or does not answer in time.
async fn token_within<S, F>(source: S) -> Result<String, Failure>
where
    S: FnOnce() -> F,
    F: std::future::Future<Output = Result<String, String>>,
{
    match tokio::time::timeout(TOKEN_BOUND, source()).await {
        Ok(Ok(token)) => Ok(token),
        Ok(Err(message)) => Err(Failure::new(FailureKind::Auth, message)),
        Err(_) => Err(Failure::new(
            FailureKind::Auth,
            format!(
                "`gh auth token` did not answer within {} s",
                TOKEN_BOUND.as_secs()
            ),
        )),
    }
}

// --- The inspector's fetches (FZ-30f) ---

/// Something the inspector shows for the highlighted run, fetched once and
/// held for the session: a run's jobs, a job's whole log, a workflow file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detail<T> {
    /// A fetch is out.
    Loading,
    Ready(T),
    /// GitHub said for good there is none (a 404, an expired log, a file
    /// too large for the contents API). Not asked again.
    Unavailable(String),
    /// The fetch failed in a way that may clear (offline, a limit, a 5xx).
    /// Asked again `SETTLED_SECS` after `at`, never sooner.
    Failed {
        message: String,
        at: DateTime<Local>,
    },
}

impl<T> Detail<T> {
    /// Whether a fetch for `held` should start at `now`: when nothing was
    /// asked yet, or a failed one has waited out `SETTLED_SECS`. Never while
    /// one is out, once it is held, or once it is unavailable.
    pub fn wants_fetch(held: Option<&Self>, now: DateTime<Local>) -> bool {
        match held {
            None => true,
            Some(Self::Failed { at, .. }) => (now - *at).num_seconds() >= SETTLED_SECS,
            Some(Self::Loading | Self::Ready(_) | Self::Unavailable(_)) => false,
        }
    }

    /// A finished fetch's result as held: a client error that is not a limit
    /// is GitHub's final word, everything else may clear.
    pub fn from_result(result: Result<T, Failure>, now: DateTime<Local>) -> Self {
        match result {
            Ok(data) => Self::Ready(data),
            Err(f) if f.kind == FailureKind::Http && !f.rate_limited => {
                Self::Unavailable(f.message)
            }
            Err(f) => Self::Failed {
                message: f.message,
                at: now,
            },
        }
    }
}

/// What one inspector fetch asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailWant {
    /// The highlighted run's jobs, when no poll has read them yet.
    Jobs { run_id: u64 },
    /// A job's whole log. `step` is its failing step while its tail is not
    /// held yet, so the one download fills both.
    Log { job_id: u64, step: Option<Step> },
    /// The workflow file at `path` in commit `sha`: from `clone` when it has
    /// the commit, else from the contents API.
    Script {
        sha: String,
        path: String,
        clone: Option<std::path::PathBuf>,
    },
}

/// One inspector fetch to run, built by `App::claim_ci_details`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailRequest {
    pub key: RemoteKey,
    pub owner: String,
    pub name: String,
    pub want: DetailWant,
}

/// What an inspector fetch brought back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailGot {
    Jobs {
        run_id: u64,
        result: Result<JobsAnswer, Failure>,
    },
    Log {
        job_id: u64,
        result: Result<Vec<String>, Failure>,
        tail: Option<LogTail>,
    },
    Script {
        sha: String,
        path: String,
        result: Result<String, Failure>,
    },
}

/// One inspector fetch's answer, carried by `AppEvent::CiDetail`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailAnswer {
    pub key: RemoteKey,
    pub got: DetailGot,
    /// The `core` reading the answer's headers carried, if a request went.
    pub budget: Option<BudgetReading>,
}

/// Run one inspector fetch with the session's `gh` token.
pub async fn fetch_detail(request: DetailRequest) -> DetailAnswer {
    let answer = fetch_detail_with(
        request,
        || SESSION_TOKEN.token(crate::github::get_gh_token),
        crate::github::client(),
        API_ROOT,
    )
    .await;
    let failure = match &answer.got {
        DetailGot::Jobs { result, .. } => result.as_ref().err(),
        DetailGot::Log { result, .. } => result.as_ref().err(),
        DetailGot::Script { result, .. } => result.as_ref().err(),
    };
    SESSION_TOKEN.forget_on(failure);
    answer
}

/// `fetch_detail` with the token source, client and API root passed in.
/// A script asks the clone first and touches neither the token nor the
/// network when the clone has the commit.
async fn fetch_detail_with<S, F>(
    request: DetailRequest,
    token_source: S,
    client: &reqwest::Client,
    root: &str,
) -> DetailAnswer
where
    S: FnOnce() -> F,
    F: std::future::Future<Output = Result<String, String>>,
{
    let repo = format!("{root}/repos/{}/{}", request.owner, request.name);
    let mut budget = None;
    let got = match request.want {
        DetailWant::Jobs { run_id } => {
            let result = match token_within(token_source).await {
                Ok(token) => {
                    let url = format!("{repo}/actions/runs/{run_id}/jobs");
                    let (result, reading) = get(client, &url, &token, None).await;
                    budget = reading;
                    match result {
                        Ok(Answer::Fresh { body, etag }) => parse_jobs(&body)
                            .map(|data| JobsAnswer::Fetched(Fetched::Fresh { data, etag })),
                        Ok(Answer::NotModified) => Ok(JobsAnswer::Fetched(Fetched::NotModified)),
                        Ok(Answer::NotFound) => Ok(JobsAnswer::Gone),
                        Err(failure) => Err(failure),
                    }
                }
                Err(failure) => Err(failure),
            };
            DetailGot::Jobs { run_id, result }
        }
        DetailWant::Log { job_id, step } => {
            let (result, tail) = match token_within(token_source).await {
                Ok(token) => {
                    let url = format!("{repo}/actions/jobs/{job_id}/logs");
                    let (result, reading) = get(client, &url, &token, None).await;
                    budget = reading;
                    match result {
                        Ok(Answer::Fresh { body, .. }) => {
                            let tail = step.map(|s| LogTail::Lines(log_tail(&body, &s)));
                            (Ok(log_lines(&body)), tail)
                        }
                        Ok(Answer::NotModified) | Ok(Answer::NotFound) => {
                            (Err(not_found("log not found")), None)
                        }
                        Err(failure) => (Err(failure), None),
                    }
                }
                Err(failure) => (Err(failure), None),
            };
            DetailGot::Log {
                job_id,
                result,
                tail,
            }
        }
        DetailWant::Script { sha, path, clone } => {
            let local = async {
                match &clone {
                    Some(dir) => crate::git::show_file(dir, &sha, &path).await,
                    None => None,
                }
            };
            let remote = || async {
                let token = token_within(token_source).await?;
                let url = contents_url(&repo, &path, &sha);
                let (result, reading) = get(client, &url, &token, None).await;
                budget = reading;
                match result? {
                    Answer::Fresh { body, .. } => parse_contents(&body),
                    Answer::NotModified | Answer::NotFound => {
                        Err(not_found("workflow file not found at this commit"))
                    }
                }
            };
            let result = local_first(local, remote).await;
            DetailGot::Script { sha, path, result }
        }
    };
    DetailAnswer {
        key: request.key,
        got,
        budget,
    }
}

/// A 404 in the words the inspector shows: `Http` and not a limit, so it is
/// held as unavailable rather than asked again.
fn not_found(message: &str) -> Failure {
    Failure::new(FailureKind::Http, message)
}

/// The script decision: the clone's copy when `local` finds one, else
/// whatever `remote` fetches. `remote` is not started when the clone answers.
pub async fn local_first<L, R, RF>(local: L, remote: R) -> Result<String, Failure>
where
    L: std::future::Future<Output = Option<String>>,
    R: FnOnce() -> RF,
    RF: std::future::Future<Output = Result<String, Failure>>,
{
    match local.await {
        Some(text) => Ok(text),
        None => remote().await,
    }
}

/// `GET /repos/{o}/{r}/contents/{path}?ref={sha}`, the path percent-encoded
/// segment by segment.
fn contents_url(repo: &str, path: &str, sha: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    format!("{repo}/contents/{encoded}?ref={sha}")
}

#[derive(Deserialize)]
struct Contents {
    #[serde(default)]
    content: String,
    #[serde(default)]
    encoding: String,
}

/// The file text a contents body carries, base64-decoded. GitHub sends no
/// content (`encoding: none`) for a file over 1 MB, which is unavailable
/// rather than empty.
fn parse_contents(body: &str) -> Result<String, Failure> {
    let contents: Contents = serde_json::from_str(body).map_err(|e| malformed("contents", &e))?;
    if contents.encoding != "base64" {
        return Err(not_found("workflow file too large for the contents API"));
    }
    let bytes = decode_base64(&contents.content).ok_or_else(|| {
        Failure::new(
            FailureKind::Malformed,
            "the contents API sent a body that is not base64",
        )
    })?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Standard base64 (RFC 4648, `+` and `/`) to bytes, ignoring the line
/// breaks GitHub wraps it in. `None` on any other character. By hand, since
/// a dependency for twenty lines is not worth its weight.
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for c in text.bytes() {
        if c.is_ascii_whitespace() {
            continue;
        }
        if c == b'=' {
            break;
        }
        acc = (acc << 6) | value(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The lines (0-based, end exclusive) of the step `step` names in workflow
/// file `yaml`, for the `script` body's highlight. YAML read as text, no
/// parser: a step is a list item, from its `- ` line to the next line at
/// its indent or less.
///
/// Matched on the step's `name:` first, then, for a step without one, on
/// the `run:` or `uses:` text GitHub built the name from (`Run cargo test`
/// for `- run: cargo test`, the first line for a `run: |` block). `None`
/// when nothing matches, or the match is not inside a list item (the
/// workflow's or a job's own `name:`).
pub fn step_block(yaml: &str, step: &str) -> Option<std::ops::Range<usize>> {
    let lines: Vec<&str> = yaml.lines().collect();
    let command = step.strip_prefix("Run ").unwrap_or(step);
    let found = (0..lines.len())
        .filter_map(|i| item_start(&lines, i).map(|start| (i, start)))
        .find(|&(i, _)| key_value(lines[i], "name").is_some_and(|v| v == step))
        .or_else(|| {
            (0..lines.len())
                .filter_map(|i| item_start(&lines, i).map(|start| (i, start)))
                .find(|&(i, _)| {
                    if key_value(lines[i], "uses").is_some_and(|v| strip_comment(&v) == command) {
                        return true;
                    }
                    match key_value(lines[i], "run") {
                        Some(v) if is_block_scalar(&v) => lines[i + 1..]
                            .iter()
                            .find(|l| !l.trim().is_empty())
                            .is_some_and(|l| l.trim() == command),
                        Some(v) => v == command,
                        None => false,
                    }
                })
        });
    let (_, start) = found?;
    let item_indent = indent(lines[start]);
    let mut end = start + 1;
    while end < lines.len() {
        let line = lines[end];
        if !line.trim().is_empty() && indent(line) <= item_indent {
            break;
        }
        end += 1;
    }
    while end > start + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    Some(start..end)
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The `- ` line of the list item line `i` belongs to as one of its keys:
/// `i` itself, or the nearest item line above at a smaller indent, with
/// nothing between them that ends the item.
fn item_start(lines: &[&str], i: usize) -> Option<usize> {
    if lines[i].trim_start().starts_with("- ") {
        return Some(i);
    }
    let key_indent = indent(lines[i]);
    for j in (0..i).rev() {
        let line = lines[j];
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || indent(line) >= key_indent {
            continue;
        }
        return trimmed.starts_with("- ").then_some(j);
    }
    None
}

/// The value of `key:` on `line`, which may open a list item (`- key:`),
/// with surrounding quotes removed.
fn key_value(line: &str, key: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let trimmed = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim_start();
    let value = trimmed.strip_prefix(key)?.strip_prefix(':')?.trim();
    let unquoted = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value);
    Some(unquoted.to_string())
}

/// `value` without a trailing ` # comment`.
fn strip_comment(value: &str) -> &str {
    value.split_once(" #").map_or(value, |(v, _)| v).trim_end()
}

/// `|`, `>` and their chomping forms: the text starts on the next line.
fn is_block_scalar(value: &str) -> bool {
    matches!(value, "|" | ">" | "|-" | ">-" | "|+" | ">+")
}

/// One REST call's answer before parsing.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Fresh { body: String, etag: Option<String> },
    NotModified,
    NotFound,
}

/// One conditional GET. Refusals are classified as the fleet poll's are
/// (`github::classify_refusal`), and the budget is read from every answer
/// that carries the headers, a 304 and a refusal among them. An error body
/// is never kept as data.
async fn get(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    etag: Option<&str>,
) -> (Result<Answer, Failure>, Option<BudgetReading>) {
    let mut request = client
        .get(url)
        .header("Authorization", format!("bearer {token}"))
        .header("User-Agent", "fleetz")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(etag) = etag {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    let resp = match request.send().await {
        Ok(resp) => resp,
        Err(e) => {
            let failure = crate::github::transport_failure(&e, "GitHub API request failed");
            return (Err(failure), None);
        }
    };
    let status = resp.status();
    let headers = resp.headers().clone();
    let budget = crate::github::read_budget(&headers, &crate::github::token_fingerprint(token));
    if status == reqwest::StatusCode::NOT_MODIFIED {
        return (Ok(Answer::NotModified), budget);
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return (Ok(Answer::NotFound), budget);
    }
    let body = match resp.text().await {
        Ok(body) => body,
        Err(e) if status.is_success() => {
            let failure = crate::github::transport_failure(&e, "failed to read response");
            return (Err(failure), budget);
        }
        Err(_) => String::new(),
    };
    if !status.is_success() {
        let failure = crate::github::classify_refusal(status, &headers, &body);
        return (Err(failure), budget);
    }
    let etag = headers
        .get(reqwest::header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    (Ok(Answer::Fresh { body, etag }), budget)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNS: &str = include_str!("ci/fixtures/runs.json");
    const JOBS_FAILED: &str = include_str!("ci/fixtures/jobs-failed.json");
    const LOG_FAILED: &str = include_str!("ci/fixtures/job-log-failed.txt");

    fn utc(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn local(text: &str) -> DateTime<Local> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Local)
    }

    fn runs() -> Vec<WorkflowRun> {
        parse_runs(RUNS).unwrap()
    }

    fn finished_runs() -> Vec<WorkflowRun> {
        runs().into_iter().filter(|r| !r.is_live()).collect()
    }

    // --- Parsing, on payloads recorded against Gaurgle/fleetz ---

    #[test]
    fn the_runs_list_parses_into_the_spec_fields() {
        let runs = runs();
        assert_eq!(runs.len(), 3);
        let live = &runs[0];
        assert_eq!(live.id, 39590684061);
        assert_eq!(live.attempt, 1);
        assert_eq!(live.workflow.as_deref(), Some("ci"));
        assert_eq!(
            live.title,
            "feat(ui): date the failed GitHub attempt in the header"
        );
        assert_eq!(
            live.branch.as_deref(),
            Some("feat/79-date-the-failed-attempt")
        );
        assert_eq!(live.event, "push");
        assert_eq!(live.status.as_deref(), Some("in_progress"));
        assert_eq!(live.conclusion, None);
        assert_eq!(live.created, utc("2026-10-21T11:16:07Z"));
        assert_eq!(live.started, Some(utc("2026-10-21T11:16:07Z")));
        assert_eq!(live.updated, utc("2026-10-21T11:16:10Z"));
        assert_eq!(
            live.url,
            "https://github.com/Gaurgle/fleetz/actions/runs/39590684061"
        );
        assert!(live.is_live());
        assert_eq!(runs[1].conclusion.as_deref(), Some("success"));
        assert_eq!(runs[2].conclusion.as_deref(), Some("failure"));
        assert!(!runs[2].is_live());
    }

    #[test]
    fn nullable_run_fields_parse_as_none() {
        let body = r#"{"workflow_runs":[{"id":1,"name":null,"display_title":"t",
            "head_branch":null,"event":"schedule","status":null,"conclusion":null,
            "created_at":"2026-10-21T11:16:07Z","updated_at":"2026-10-21T11:16:07Z",
            "html_url":"u"}]}"#;
        let run = &parse_runs(body).unwrap()[0];
        assert_eq!(run.workflow, None);
        assert_eq!(run.branch, None);
        assert_eq!(run.status, None);
        assert_eq!(run.started, None);
        assert_eq!(run.attempt, 1, "no run_attempt reads as the first");
        assert!(!run.is_live());
    }

    #[test]
    fn the_jobs_list_parses_jobs_and_steps() {
        let jobs = parse_jobs(JOBS_FAILED).unwrap();
        assert_eq!(jobs.len(), 1);
        let job = &jobs[0];
        assert_eq!(job.id, 113914377501);
        assert_eq!(job.name, "check");
        assert_eq!(job.status, "completed");
        assert_eq!(job.conclusion.as_deref(), Some("failure"));
        assert_eq!(job.started, Some(utc("2026-10-09T16:19:46Z")));
        assert_eq!(job.completed, Some(utc("2026-10-09T16:20:40Z")));
        assert_eq!(job.steps.len(), 8);
        let step = failing_step(job).expect("one step failed");
        assert_eq!(step.number, 5);
        assert_eq!(step.name, "Run cargo clippy --all-targets -- -D warnings");
        assert_eq!(step.status, "completed");
        assert_eq!(step.started, Some(utc("2026-10-09T16:19:52Z")));
        assert_eq!(step.completed, Some(utc("2026-10-09T16:20:37Z")));
        assert_eq!(job.steps[5].conclusion.as_deref(), Some("skipped"));
    }

    #[test]
    fn a_queued_job_without_steps_or_times_parses() {
        let body = r#"{"jobs":[{"id":7,"name":"check","status":"queued",
            "conclusion":null,"started_at":null,"completed_at":null}]}"#;
        let job = &parse_jobs(body).unwrap()[0];
        assert!(job.steps.is_empty());
        assert_eq!(job.started, None);
        assert_eq!(failing_step(job), None);
    }

    #[test]
    fn a_body_that_does_not_parse_is_malformed() {
        let failure = parse_runs(r#"{"message":"Bad credentials"}"#).unwrap_err();
        assert_eq!(failure.kind, FailureKind::Malformed);
        assert_eq!(
            parse_jobs("<html>").unwrap_err().kind,
            FailureKind::Malformed
        );
    }

    // --- The log tail ---

    #[test]
    fn the_log_tail_is_the_failing_steps_last_twelve_lines() {
        let jobs = parse_jobs(JOBS_FAILED).unwrap();
        let tail = log_tail(LOG_FAILED, failing_step(&jobs[0]).unwrap());
        assert_eq!(
            tail,
            vec![
                "2697 | | > {",
                "     | |_^ the `Err`-variant is at least 128 bytes",
                "     |",
                "     = help: try reducing the size of `LoadError`, for example by boxing large elements or replacing it with `Box<LoadError>`",
                "     = help: for further information visit https://rust-lang.github.io/rust-clippy/rust-1.99.0/index.html#result_large_err",
                "     = note: `-D clippy::result-large-err` implied by `-D warnings`",
                "     = help: to override `-D warnings` add `#[allow(clippy::result_large_err)]`",
                "",
                "error: could not compile `fleetz` (bin \"fleetz\") due to 1 previous error",
                "warning: build failed, waiting for other jobs to finish...",
                "error: could not compile `fleetz` (bin \"fleetz\" test) due to 1 previous error",
                "##[error]Process completed with exit code 101.",
            ]
        );
        assert!(
            tail.iter().all(|l| !l.contains('\u{1b}')),
            "colours stripped"
        );
    }

    #[test]
    fn the_log_tail_keeps_what_the_step_wrote_and_nothing_after_its_error() {
        // The cleanup steps start in the second the failing step ended; the
        // error line closes the step.
        let jobs = parse_jobs(JOBS_FAILED).unwrap();
        let tail = log_tail(LOG_FAILED, failing_step(&jobs[0]).unwrap());
        assert!(!tail.iter().any(|l| l.contains("Post job cleanup")));
        assert!(
            !tail.iter().any(|l| l.starts_with("2026-")),
            "stamps stripped"
        );
    }

    #[test]
    fn the_log_tail_starts_at_the_steps_own_group_header() {
        let log = "\u{feff}2026-10-09T16:19:52.1Z tail of the step before\n\
                   2026-10-09T16:19:52.2Z ##[group]Run make\n\
                   2026-10-09T16:19:52.3Z make: *** [all] Error 1\n";
        let step = Step {
            number: 2,
            name: "Run make".to_string(),
            status: "completed".to_string(),
            conclusion: Some("failure".to_string()),
            started: Some(utc("2026-10-09T16:19:52Z")),
            completed: Some(utc("2026-10-09T16:19:52Z")),
        };
        assert_eq!(
            log_tail(log, &step),
            vec!["##[group]Run make", "make: *** [all] Error 1"]
        );
    }

    #[test]
    fn a_step_that_never_ran_has_no_tail() {
        let step = Step {
            number: 1,
            name: "Run make".to_string(),
            status: "queued".to_string(),
            conclusion: None,
            started: None,
            completed: None,
        };
        assert!(log_tail(LOG_FAILED, &step).is_empty());
    }

    // --- Cadence ---

    fn cadence(runs: &[WorkflowRun]) -> Cadence<'_> {
        Cadence {
            folded: false,
            has_identity: true,
            settled_for: DWELL,
            inflight: false,
            runs,
            last_attempt: None,
            pace: Pace::Unknown,
            not_before: None,
            now: local("2026-10-21T12:00:00+02:00"),
        }
    }

    #[test]
    fn a_folded_pane_never_polls() {
        let c = Cadence {
            folded: true,
            ..cadence(&[])
        };
        assert!(!poll_due(&c));
    }

    #[test]
    fn a_row_without_a_github_identity_never_polls() {
        let c = Cadence {
            has_identity: false,
            ..cadence(&[])
        };
        assert!(!poll_due(&c));
    }

    #[test]
    fn the_first_poll_waits_for_the_cursor_to_dwell() {
        let moving = Cadence {
            settled_for: DWELL - std::time::Duration::from_millis(1),
            ..cadence(&[])
        };
        assert!(!poll_due(&moving), "a j run through the list sends nothing");
        assert!(poll_due(&cadence(&[])), "settled on a repo never polled");
        assert_eq!(DWELL, std::time::Duration::from_millis(300));
    }

    #[test]
    fn a_poll_in_flight_is_not_stacked() {
        let c = Cadence {
            inflight: true,
            ..cadence(&[])
        };
        assert!(!poll_due(&c));
    }

    #[test]
    fn a_live_run_polls_every_ten_seconds() {
        let runs = runs();
        let now = local("2026-10-21T12:00:00+02:00");
        let at = |secs| Cadence {
            last_attempt: Some(now - chrono::Duration::seconds(secs)),
            ..cadence(&runs)
        };
        assert_eq!(base_interval_secs(&runs), 10);
        assert!(!poll_due(&at(9)));
        assert!(poll_due(&at(10)));
    }

    #[test]
    fn finished_runs_poll_every_minute() {
        let runs = finished_runs();
        let now = local("2026-10-21T12:00:00+02:00");
        let at = |secs| Cadence {
            last_attempt: Some(now - chrono::Duration::seconds(secs)),
            ..cadence(&runs)
        };
        assert_eq!(base_interval_secs(&runs), 60);
        assert_eq!(base_interval_secs(&[]), 60, "no runs at all");
        assert!(!poll_due(&at(59)));
        assert!(poll_due(&at(60)));
    }

    #[test]
    fn a_queued_run_counts_as_live() {
        let mut runs = finished_runs();
        runs[0].status = Some("queued".to_string());
        assert_eq!(base_interval_secs(&runs), LIVE_SECS);
    }

    #[test]
    fn a_low_budget_stretches_the_interval_and_a_pause_holds_it() {
        let runs = runs();
        let now = local("2026-10-21T12:00:00+02:00");
        assert_eq!(interval_secs(&runs, Pace::Every(25)), 25);
        assert_eq!(interval_secs(&runs, Pace::Every(3)), 10, "never faster");
        let stretched = Cadence {
            last_attempt: Some(now - chrono::Duration::seconds(20)),
            pace: Pace::Every(25),
            ..cadence(&runs)
        };
        assert!(!poll_due(&stretched));
        let paused = Cadence {
            pace: Pace::Paused {
                until: now + chrono::Duration::seconds(1),
            },
            ..cadence(&runs)
        };
        assert!(!poll_due(&paused), "even a repo never polled waits");
        let resumed = Cadence {
            pace: Pace::Paused {
                until: now - chrono::Duration::seconds(1),
            },
            ..cadence(&runs)
        };
        assert!(poll_due(&resumed));
    }

    #[test]
    fn the_backoff_holds_the_poll() {
        let now = local("2026-10-21T12:00:00+02:00");
        let held = Cadence {
            not_before: Some(now + chrono::Duration::seconds(30)),
            ..cadence(&[])
        };
        assert!(!poll_due(&held));
        let over = Cadence {
            not_before: Some(now),
            ..cadence(&[])
        };
        assert!(poll_due(&over));
    }

    // --- The REST exchange, on the loopback ---

    /// A loopback listener answering one connection per canned response, in
    /// order, and handing back each request's head as it read it.
    async fn serve(
        responses: Vec<String>,
    ) -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (sent, received) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            for response in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut seen = Vec::new();
                let mut chunk = [0u8; 16 * 1024];
                while !String::from_utf8_lossy(&seen).contains("\r\n\r\n") {
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => seen.extend_from_slice(&chunk[..n]),
                    }
                }
                let _ = sent.send(String::from_utf8_lossy(&seen).to_string());
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        (format!("http://{addr}"), received)
    }

    fn response(status: &str, headers: &[&str], body: &str) -> String {
        let mut text = format!("HTTP/1.1 {status}\r\n");
        for header in headers {
            text.push_str(header);
            text.push_str("\r\n");
        }
        text.push_str(&format!(
            "content-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        ));
        text
    }

    const BUDGET: [&str; 5] = [
        "x-ratelimit-limit: 5000",
        "x-ratelimit-remaining: 4952",
        "x-ratelimit-used: 48",
        "x-ratelimit-reset: 1792583935",
        "x-ratelimit-resource: core",
    ];

    fn test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(2))
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap()
    }

    fn request(etag: Option<&str>, expanded: Option<ExpandedRequest>) -> CiRequest {
        CiRequest {
            key: (Some("gaurgle".to_string()), "fleetz".to_string()),
            owner: "gaurgle".to_string(),
            name: "fleetz".to_string(),
            attempt: 1,
            runs_etag: etag.map(str::to_string),
            expanded,
        }
    }

    async fn run(request: CiRequest, root: &str) -> CiAnswer {
        poll_with(
            request,
            || async { Ok("tok".to_string()) },
            &test_client(),
            root,
        )
        .await
    }

    #[tokio::test]
    async fn the_list_call_names_the_endpoint_and_returns_the_etag() {
        let mut headers = BUDGET.to_vec();
        headers.push("etag: W/\"6b13\"");
        let (root, mut seen) = serve(vec![response("200 OK", &headers, RUNS)]).await;
        let answer = run(request(None, None), &root).await;
        let head = seen.recv().await.unwrap();
        assert!(
            head.starts_with("GET /repos/gaurgle/fleetz/actions/runs?per_page=10 HTTP/1.1"),
            "{head}"
        );
        assert!(head.to_lowercase().contains("authorization: bearer tok"));
        assert!(!head.to_lowercase().contains("if-none-match"));
        assert_eq!(answer.failure, None);
        assert_eq!(
            answer.runs,
            Some(Fetched::Fresh {
                data: runs(),
                etag: Some("W/\"6b13\"".to_string())
            })
        );
        let budget = answer.budget.expect("the headers carried a reading");
        assert_eq!((budget.limit, budget.remaining), (5000, 4952));
        assert_eq!(answer.jobs, None, "no run expanded, no /jobs call");
    }

    #[tokio::test]
    async fn the_etag_is_sent_back_and_a_304_is_not_modified() {
        let (root, mut seen) = serve(vec![response("304 Not Modified", &BUDGET, "")]).await;
        let answer = run(request(Some("W/\"6b13\""), None), &root).await;
        let head = seen.recv().await.unwrap().to_lowercase();
        assert!(head.contains("if-none-match: w/\"6b13\""), "{head}");
        assert_eq!(answer.failure, None);
        assert_eq!(answer.runs, Some(Fetched::NotModified));
        assert!(answer.budget.is_some(), "a 304 states the budget too");
    }

    fn expanded(run_id: u64) -> ExpandedRequest {
        ExpandedRequest {
            run_id,
            jobs_etag: None,
            held_jobs: Vec::new(),
            logs_held: Vec::new(),
        }
    }

    #[tokio::test]
    async fn an_expanded_failed_run_reads_its_jobs_and_its_log_once() {
        let (root, mut seen) = serve(vec![
            response("304 Not Modified", &BUDGET, ""),
            response("200 OK", &["etag: W/\"jobs\""], JOBS_FAILED),
            response("200 OK", &[], LOG_FAILED),
        ])
        .await;
        let answer = run(
            request(Some("W/\"6b13\""), Some(expanded(37958308363))),
            &root,
        )
        .await;
        assert_eq!(answer.failure, None);
        let _list = seen.recv().await.unwrap();
        let jobs = seen.recv().await.unwrap();
        assert!(
            jobs.starts_with("GET /repos/gaurgle/fleetz/actions/runs/37958308363/jobs "),
            "{jobs}"
        );
        let log = seen.recv().await.unwrap();
        assert!(
            log.starts_with("GET /repos/gaurgle/fleetz/actions/jobs/113914377501/logs "),
            "{log}"
        );
        let (job_id, tail) = answer.log.expect("the failed job's log");
        assert_eq!(job_id, 113914377501);
        let LogTail::Lines(lines) = tail else {
            panic!("a log was read")
        };
        assert_eq!(lines.len(), LOG_TAIL_LINES);

        // Held, it is not asked for again.
        let (root, _seen) = serve(vec![
            response("304 Not Modified", &BUDGET, ""),
            response("304 Not Modified", &BUDGET, ""),
        ])
        .await;
        let again = ExpandedRequest {
            jobs_etag: Some("W/\"jobs\"".to_string()),
            held_jobs: parse_jobs(JOBS_FAILED).unwrap(),
            logs_held: vec![113914377501],
            ..expanded(37958308363)
        };
        let answer = run(request(Some("W/\"6b13\""), Some(again)), &root).await;
        assert_eq!(answer.failure, None);
        assert_eq!(answer.log, None);
        assert_eq!(
            answer.jobs,
            Some((37958308363, JobsAnswer::Fetched(Fetched::NotModified)))
        );
    }

    #[tokio::test]
    async fn a_404_on_the_expanded_runs_jobs_says_the_run_is_gone() {
        let (root, _seen) = serve(vec![
            response("304 Not Modified", &BUDGET, ""),
            response("404 Not Found", &BUDGET, r#"{"message":"Not Found"}"#),
        ])
        .await;
        let answer = run(request(Some("W/\"6b13\""), Some(expanded(1))), &root).await;
        assert_eq!(answer.failure, None, "a gone run is not a failed poll");
        assert_eq!(answer.jobs, Some((1, JobsAnswer::Gone)));
    }

    #[tokio::test]
    async fn a_primary_rate_limit_is_a_limited_failure_with_its_reading() {
        let headers = [
            "x-ratelimit-limit: 5000",
            "x-ratelimit-remaining: 0",
            "x-ratelimit-used: 5000",
            "x-ratelimit-reset: 1792583935",
        ];
        let body = r#"{"message":"API rate limit exceeded"}"#;
        let (root, _seen) = serve(vec![response("403 Forbidden", &headers, body)]).await;
        let answer = run(request(None, None), &root).await;
        let failure = answer.failure.expect("refused");
        assert!(failure.rate_limited);
        assert_eq!(failure.limit, Some(crate::github::RateLimit::Primary));
        assert_eq!(answer.runs, None, "an error body is never data");
        assert_eq!(answer.budget.unwrap().remaining, 0);
    }

    #[tokio::test]
    async fn nothing_listening_is_offline() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let answer = run(request(None, None), &format!("http://{addr}")).await;
        assert_eq!(answer.failure.unwrap().kind, FailureKind::Offline);
        assert_eq!(answer.runs, None);
        assert_eq!(answer.budget, None);
    }

    // --- The session token (FZ-30c follow-up of #90) ---

    #[tokio::test]
    async fn the_token_is_looked_up_once_and_again_after_an_auth_failure() {
        let cache = TokenCache::new();
        let lookups = std::sync::atomic::AtomicUsize::new(0);
        let source = || async {
            let n = lookups.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok::<_, String>(format!("tok{n}"))
        };
        assert_eq!(cache.token(source).await.unwrap(), "tok0");
        assert_eq!(cache.token(source).await.unwrap(), "tok0", "held");
        assert_eq!(lookups.load(std::sync::atomic::Ordering::SeqCst), 1);

        let mut answer = poll_answer();
        answer.failure = Some(Failure::new(FailureKind::Offline, "no route"));
        cache.forget_after(&answer);
        assert_eq!(
            cache.token(source).await.unwrap(),
            "tok0",
            "offline keeps it"
        );

        answer.failure = Some(Failure::new(FailureKind::Refused, "Bad credentials"));
        cache.forget_after(&answer);
        assert_eq!(cache.token(source).await.unwrap(), "tok1", "asked again");
        answer.failure = Some(Failure::new(FailureKind::Auth, "not logged in"));
        cache.forget_after(&answer);
        assert_eq!(cache.token(source).await.unwrap(), "tok2");
    }

    #[tokio::test]
    async fn a_failed_lookup_is_not_held() {
        let cache = TokenCache::new();
        let failed = cache
            .token(|| async { Err::<String, _>("not signed in".to_string()) })
            .await;
        assert!(failed.is_err());
        let token = cache.token(|| async { Ok::<_, String>("tok".to_string()) });
        assert_eq!(token.await.unwrap(), "tok");
    }

    fn poll_answer() -> CiAnswer {
        CiAnswer {
            key: (Some("gaurgle".to_string()), "fleetz".to_string()),
            attempt: 1,
            runs: None,
            jobs: None,
            log: None,
            full_log: None,
            failure: None,
            budget: None,
        }
    }

    // --- The inspector's fetches (FZ-30f) ---

    const CI_YML: &str = include_str!("ci/fixtures/ci.yml");

    #[test]
    fn the_whole_log_is_every_line_without_stamps_or_colours() {
        let lines = log_lines(LOG_FAILED);
        assert_eq!(lines.len(), LOG_FAILED.lines().count());
        assert!(lines.iter().all(|l| !l.starts_with("2026-")), "stamps");
        assert!(lines.iter().all(|l| !l.contains('\u{1b}')), "colours");
        assert!(lines.iter().all(|l| !l.starts_with('\u{feff}')), "BOM");
        assert!(lines.contains(&"##[error]Process completed with exit code 101.".to_string()));
        assert!(
            lines.iter().any(|l| l.contains("Post job cleanup")),
            "the whole log, past the failing step"
        );
    }

    /// A runaway log is held as its last `LOG_LINE_CAP` lines under a line
    /// that says so; one at the cap is held whole.
    #[test]
    fn a_whole_log_past_the_cap_keeps_its_last_lines() {
        let log = |n: usize| {
            (1..=n)
                .map(|i| format!("2026-10-09T16:20:37.5Z line {i}\n"))
                .collect::<String>()
        };
        let held = log_lines(&log(LOG_LINE_CAP + 3));
        assert_eq!(held.len(), LOG_LINE_CAP + 1);
        assert_eq!(held[0], LOG_TRUNCATED);
        assert_eq!(held[1], "line 4", "the first lines go");
        assert_eq!(held.last().unwrap(), &format!("line {}", LOG_LINE_CAP + 3));
        assert_eq!(
            LOG_TRUNCATED,
            format!("log truncated to the last {LOG_LINE_CAP} lines; l opens the whole log in gh")
        );
        let whole = log_lines(&log(LOG_LINE_CAP));
        assert_eq!(whole.len(), LOG_LINE_CAP);
        assert_eq!(whole[0], "line 1", "no note at the cap");
    }

    #[tokio::test]
    async fn the_poll_that_reads_a_tail_keeps_the_whole_log_too() {
        let (root, _seen) = serve(vec![
            response("304 Not Modified", &BUDGET, ""),
            response("200 OK", &[], JOBS_FAILED),
            response("200 OK", &[], LOG_FAILED),
        ])
        .await;
        let answer = run(
            request(Some("W/\"6b13\""), Some(expanded(37958308363))),
            &root,
        )
        .await;
        assert_eq!(
            answer.full_log,
            Some((113914377501, log_lines(LOG_FAILED))),
            "one download serves the tail and the log body"
        );
    }

    /// The failing step of the recorded failed run, matched in the
    /// workflow file recorded at its commit (`93f4c8e`).
    #[test]
    fn the_failing_steps_block_is_found_by_its_run_text() {
        let lines: Vec<&str> = CI_YML.lines().collect();
        let jobs = parse_jobs(JOBS_FAILED).unwrap();
        let step = failing_step(&jobs[0]).unwrap();
        let block = step_block(CI_YML, &step.name).expect("matched");
        assert_eq!(block.len(), 1);
        assert_eq!(
            lines[block.start].trim(),
            "- run: cargo clippy --all-targets -- -D warnings"
        );
    }

    #[test]
    fn a_uses_step_matches_past_its_comment_and_takes_its_with_block() {
        let lines: Vec<&str> = CI_YML.lines().collect();
        let name = "Run dtolnay/rust-toolchain@6bed0761d98439e5a578e2877258200ad565ba87";
        let block = step_block(CI_YML, name).expect("matched");
        assert!(lines[block.start].contains("- uses: dtolnay/rust-toolchain"));
        assert_eq!(block.len(), 4, "uses, with and its two keys");
        let checkout = step_block(CI_YML, "Run actions/checkout@v4").expect("matched");
        assert_eq!(
            checkout.len(),
            1,
            "the comment after it opens the next step"
        );
    }

    #[test]
    fn a_named_step_matches_by_name_first_and_a_block_run_by_its_first_line() {
        let yaml = "name: Build\n\
                    jobs:\n  \
                      build:\n    \
                        name: Build\n    \
                        steps:\n      \
                          - uses: actions/upload-artifact@v4\n        \
                            with:\n          \
                              name: Build\n      \
                          - name: \"Build\"\n        \
                            run: make\n\n      \
                          - run: |\n          \
                              cargo test\n          \
                              cargo doc\n";
        let lines: Vec<&str> = yaml.lines().collect();
        let named = step_block(yaml, "Build").expect("the step named Build");
        assert_eq!(lines[named.start].trim(), "- name: \"Build\"");
        assert_eq!(named.len(), 2, "trailing blank lines are not the block's");
        let block = step_block(yaml, "Run cargo test").expect("the block run");
        assert_eq!(lines[block.start].trim(), "- run: |");
        assert_eq!(block.end, lines.len());
        assert_eq!(
            step_block(yaml, "Run cargo bench"),
            None,
            "no match, no highlight"
        );
        assert_eq!(
            step_block("name: ci\n", "ci"),
            None,
            "the workflow's own name"
        );
    }

    #[test]
    fn base64_is_decoded_by_hand_across_line_breaks() {
        assert_eq!(decode_base64("Zm9vYmFy").unwrap(), b"foobar");
        assert_eq!(decode_base64("Zm9v\nYg==\n").unwrap(), b"foob");
        assert_eq!(decode_base64("Zm8=").unwrap(), b"fo");
        assert_eq!(decode_base64("Zm9v!"), None);
        let body = r#"{"content":"bmFtZTog\nY2kK\n","encoding":"base64"}"#;
        assert_eq!(parse_contents(body).unwrap(), "name: ci\n");
        let large = parse_contents(r#"{"content":"","encoding":"none"}"#).unwrap_err();
        assert_eq!(
            Detail::<String>::from_result(Err(large), local("2026-10-21T12:00:00+02:00")),
            Detail::Unavailable("workflow file too large for the contents API".to_string())
        );
    }

    #[test]
    fn a_detail_is_fetched_once_and_a_failed_one_again_after_a_minute() {
        let now = local("2026-10-21T12:00:00+02:00");
        let failed = Detail::<()>::Failed {
            message: "offline".to_string(),
            at: now,
        };
        assert!(Detail::<()>::wants_fetch(None, now));
        assert!(!Detail::wants_fetch(Some(&Detail::<()>::Loading), now));
        assert!(!Detail::wants_fetch(Some(&Detail::Ready(())), now));
        let gone = Detail::<()>::Unavailable("log not found".to_string());
        assert!(!Detail::wants_fetch(
            Some(&gone),
            now + chrono::Duration::days(1)
        ));
        assert!(!Detail::wants_fetch(
            Some(&failed),
            now + chrono::Duration::seconds(59)
        ));
        assert!(Detail::wants_fetch(
            Some(&failed),
            now + chrono::Duration::seconds(60)
        ));
        let offline = Failure::new(FailureKind::Offline, "no route");
        assert!(matches!(
            Detail::<()>::from_result(Err(offline), now),
            Detail::Failed { .. }
        ));
        let limited = Failure::rate_limit("API rate limit exceeded");
        assert!(matches!(
            Detail::<()>::from_result(Err(limited), now),
            Detail::Failed { .. }
        ));
    }

    #[tokio::test]
    async fn the_script_comes_from_the_clone_and_github_only_without_it() {
        let asked = std::sync::atomic::AtomicBool::new(false);
        let remote = || async {
            asked.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok::<_, Failure>("from GitHub".to_string())
        };
        let local = async { Some("from the clone".to_string()) };
        assert_eq!(local_first(local, remote).await.unwrap(), "from the clone");
        assert!(
            !asked.load(std::sync::atomic::Ordering::SeqCst),
            "not asked"
        );
        let lacking = async { None };
        assert_eq!(local_first(lacking, remote).await.unwrap(), "from GitHub");
        assert!(asked.load(std::sync::atomic::Ordering::SeqCst));
    }

    fn script_request(sha: &str, clone: Option<std::path::PathBuf>) -> DetailRequest {
        DetailRequest {
            key: (Some("gaurgle".to_string()), "fleetz".to_string()),
            owner: "gaurgle".to_string(),
            name: "fleetz".to_string(),
            want: DetailWant::Script {
                sha: sha.to_string(),
                path: ".github/workflows/ci.yml".to_string(),
                clone,
            },
        }
    }

    /// A clone with the commit answers from git and asks nothing of
    /// GitHub, the token included; a sha the clone lacks goes to the
    /// contents API at that ref.
    #[tokio::test]
    async fn a_script_falls_back_to_the_contents_api_on_a_sha_the_clone_lacks() {
        use std::process::Command as StdCommand;
        let dir = std::env::temp_dir().join(format!("fleetz_ci_script_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".github/workflows")).unwrap();
        let git = |args: &[&str]| {
            let out = StdCommand::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "t"]);
        std::fs::write(dir.join(".github/workflows/ci.yml"), CI_YML).unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "ci"]);
        let sha = git(&["rev-parse", "HEAD"]);

        let answer = fetch_detail_with(
            script_request(&sha, Some(dir.clone())),
            || async { panic!("the clone had it: no token lookup") },
            &test_client(),
            "http://127.0.0.1:9",
        )
        .await;
        let DetailGot::Script { result, .. } = answer.got else {
            panic!("a script answer")
        };
        assert_eq!(result.unwrap(), CI_YML);
        assert_eq!(answer.budget, None, "no request went");

        let lacking = "93f4c8edaf3438b5413579aaa1b65f5c6c228b87";
        let body = r#"{"content":"bmFtZTog\nY2kK\n","encoding":"base64"}"#;
        let (root, mut seen) = serve(vec![response("200 OK", &BUDGET, body)]).await;
        let answer = fetch_detail_with(
            script_request(lacking, Some(dir.clone())),
            || async { Ok("tok".to_string()) },
            &test_client(),
            &root,
        )
        .await;
        let head = seen.recv().await.unwrap();
        assert!(
            head.starts_with(&format!(
                "GET /repos/gaurgle/fleetz/contents/.github/workflows/ci.yml?ref={lacking} "
            )),
            "{head}"
        );
        let DetailGot::Script { result, .. } = answer.got else {
            panic!("a script answer")
        };
        assert_eq!(result.unwrap(), "name: ci\n");
        assert!(answer.budget.is_some(), "the contents call bills core");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_log_body_reads_a_jobs_whole_log_and_its_tail_in_one_call() {
        let (root, mut seen) = serve(vec![response("200 OK", &BUDGET, LOG_FAILED)]).await;
        let jobs = parse_jobs(JOBS_FAILED).unwrap();
        let request = DetailRequest {
            want: DetailWant::Log {
                job_id: jobs[0].id,
                step: failing_step(&jobs[0]).cloned(),
            },
            ..script_request("", None)
        };
        let answer = fetch_detail_with(
            request,
            || async { Ok("tok".to_string()) },
            &test_client(),
            &root,
        )
        .await;
        let head = seen.recv().await.unwrap();
        assert!(
            head.starts_with("GET /repos/gaurgle/fleetz/actions/jobs/113914377501/logs "),
            "{head}"
        );
        let DetailGot::Log { result, tail, .. } = answer.got else {
            panic!("a log answer")
        };
        assert_eq!(result.unwrap(), log_lines(LOG_FAILED));
        let Some(LogTail::Lines(tail)) = tail else {
            panic!("the tail rides along")
        };
        assert_eq!(tail.len(), LOG_TAIL_LINES);
    }

    #[tokio::test]
    async fn an_expired_log_is_unavailable_for_good() {
        let body = r#"{"message":"Gone"}"#;
        let (root, _seen) = serve(vec![response("410 Gone", &BUDGET, body)]).await;
        let request = DetailRequest {
            want: DetailWant::Log {
                job_id: 1,
                step: None,
            },
            ..script_request("", None)
        };
        let answer = fetch_detail_with(
            request,
            || async { Ok("tok".to_string()) },
            &test_client(),
            &root,
        )
        .await;
        let DetailGot::Log { result, .. } = answer.got else {
            panic!("a log answer")
        };
        let held = Detail::from_result(result, local("2026-10-21T12:00:00+02:00"));
        assert!(matches!(held, Detail::Unavailable(_)), "{held:?}");
    }

    #[tokio::test]
    async fn no_token_is_an_auth_failure_and_sends_nothing() {
        let answer = poll_with(
            request(None, None),
            || async { Err("not authenticated with GitHub".to_string()) },
            &test_client(),
            "http://127.0.0.1:9",
        )
        .await;
        assert_eq!(answer.failure.unwrap().kind, FailureKind::Auth);
    }
}
