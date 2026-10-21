//! The CI pane's data (FZ-30b): the selected repo's recent workflow runs, an
//! expanded run's jobs and steps, and a failed job's log tail, read from the
//! REST Actions API. See `docs/superpowers/specs/2026-10-21-ci-pane-design.md`,
//! sections C and D.
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
/// two repository objects; 10 is half that and fills the pane's eight lines.
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
    /// A 404: the run was deleted. The pane says `run gone` until the next
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
    /// The pane is folded: nothing polls.
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

/// Run one CI poll against GitHub with the `gh` token.
pub async fn poll(request: CiRequest) -> CiAnswer {
    poll_with(
        request,
        crate::github::get_gh_token,
        crate::github::client(),
        API_ROOT,
    )
    .await
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
        failure: None,
        budget: None,
    };
    let token = match tokio::time::timeout(TOKEN_BOUND, token_source()).await {
        Ok(Ok(token)) => token,
        Ok(Err(message)) => {
            answer.failure = Some(Failure::new(FailureKind::Auth, message));
            return answer;
        }
        Err(_) => {
            answer.failure = Some(Failure::new(
                FailureKind::Auth,
                format!(
                    "`gh auth token` did not answer within {} s",
                    TOKEN_BOUND.as_secs()
                ),
            ));
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
