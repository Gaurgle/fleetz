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

use crate::github::{BudgetReading, Failure, FailureKind};
use crate::model::CiStatus;
use crate::model::RemoteKey;
use crate::time::Pace;

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
pub const LOG_TRUNCATED: &str =
    "log truncated to the last 5000 lines; the whole log is on the run's page (O)";
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

/// Check runs per `/check-runs` call: GitHub's cap, so one call lists every
/// check of a commit and the verdict knows it saw them all.
pub const CHECKS_PER_PAGE: u32 = 100;

/// One context of the default branch's tip (FZ-83): a commit status (a
/// deploy service such as Vercel) or a check run. What the fleet poll's
/// `statusCheckRollup` is made of, read here because the rollup carries no
/// names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    pub name: String,
    pub state: CiStatus,
    /// A status's description, a check run's output title.
    pub description: Option<String>,
    /// Where `Enter` and `O` go: a status's target, a check run's details.
    pub url: Option<String>,
    /// A GitHub Actions check run, which the run list already shows.
    pub actions: bool,
    /// Where it ran, the `ci` view's `source` column (`source_of`).
    pub source: String,
    /// A check run's start and finish; `None` for a commit status, whose
    /// times come from its history (`status_run`).
    pub started: Option<DateTime<Utc>>,
    pub completed: Option<DateTime<Utc>>,
}

/// The sources the `ci` view names and colours (FZ-83), matched in a
/// status context's name or a check run's app slug.
pub const KNOWN_SOURCES: [&str; 6] = [
    "vercel",
    "netlify",
    "cloudflare",
    "circleci",
    "buildkite",
    "codecov",
];

/// The `source` a context ran on: `actions` for GitHub Actions; a known
/// source named in `name` (`Vercel`, `ci/circleci: build`); else `name`
/// lowercased up to its first `/` or `:`.
pub fn source_of(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower == "github-actions" {
        return "actions".to_string();
    }
    if let Some(known) = KNOWN_SOURCES.iter().find(|k| lower.contains(*k)) {
        return known.to_string();
    }
    lower
        .split(['/', ':'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// One state change of a commit status, as
/// `GET /repos/{o}/{r}/commits/{sha}/statuses` lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEvent {
    pub context: String,
    pub state: CiStatus,
    pub description: Option<String>,
    pub at: DateTime<Utc>,
}

/// A context drawn as a row of the `ci` list among the runs (FZ-83): a
/// commit status, or a check run of an app other than GitHub Actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextRun {
    pub name: String,
    pub source: String,
    /// `status` for a commit status, `check` for a check run.
    pub trigger: &'static str,
    pub state: CiStatus,
    pub title: String,
    pub url: Option<String>,
    /// When the attempt the row shows began, if known.
    pub started: Option<DateTime<Utc>>,
    /// When it reached its final state; `None` while pending.
    pub finished: Option<DateTime<Utc>>,
    /// The time the age column counts from, and the list sorts by.
    pub at: Option<DateTime<Utc>>,
}

/// The row of the commit status `latest` from its `history` (any order,
/// other contexts mixed in). Its duration is the latest job's own span: from
/// the latest `pending` to the first state after it that is not `pending`,
/// and to now while none has come; with no `pending` at all, no start is
/// known. Its age is the final state's time, which may come later than
/// that span (a Vercel alias failing half an hour after its deploy
/// finished). Without a history the row has no times.
pub fn status_run(latest: &Context, history: &[StatusEvent]) -> ContextRun {
    let mut own: Vec<&StatusEvent> = history
        .iter()
        .filter(|e| e.context == latest.name)
        .collect();
    own.sort_by_key(|e| e.at);
    let pending = own.iter().rposition(|e| e.state == CiStatus::Pending);
    let finished = pending.and_then(|at| {
        own[at + 1..]
            .iter()
            .find(|e| e.state != CiStatus::Pending)
            .map(|e| e.at)
    });
    ContextRun {
        name: latest.name.clone(),
        source: latest.source.clone(),
        trigger: "status",
        state: latest.state.clone(),
        title: latest.description.clone().unwrap_or_default(),
        url: latest.url.clone(),
        started: pending.map(|at| own[at].at),
        finished,
        at: own.last().map(|e| e.at),
    }
}

/// The row of a check run of another app, from its own times.
pub fn check_run(check: &Context) -> ContextRun {
    ContextRun {
        name: check.name.clone(),
        source: check.source.clone(),
        trigger: "check",
        state: check.state.clone(),
        title: check.description.clone().unwrap_or_default(),
        url: check.url.clone(),
        started: check.started,
        finished: check.completed.filter(|_| check.state != CiStatus::Pending),
        at: check.started.or(check.completed),
    }
}

/// `GET /repos/{o}/{r}/commits/HEAD/status`: the default branch's tip and
/// its commit statuses. `HEAD` resolves to the default branch, so neither
/// its name nor its sha has to be known first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TipStatus {
    pub sha: String,
    pub statuses: Vec<Context>,
    /// Every status is listed (none past the page).
    pub complete: bool,
}

/// `GET /repos/{o}/{r}/commits/{sha}/check-runs`: the tip's check runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TipChecks {
    pub runs: Vec<Context>,
    /// Every check run is listed (none past the page).
    pub complete: bool,
}

/// What is held for the tip, so both reads go out conditionally.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TipRequest {
    pub sha: Option<String>,
    pub status_etag: Option<String>,
    /// Sent only while the tip is still `sha`: another commit is another URL.
    pub checks_etag: Option<String>,
    /// The held tip has commit statuses, so a 304 on the status still reads
    /// their history.
    pub has_statuses: bool,
    /// Sent only while the tip is still `sha`, as `checks_etag`.
    pub history_etag: Option<String>,
}

/// The tip reads' answer: the status, then the status history and the check
/// runs of the sha it named, when those calls answered too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TipAnswer {
    pub status: Fetched<TipStatus>,
    pub history: Option<(String, Fetched<Vec<StatusEvent>>)>,
    pub checks: Option<(String, Fetched<TipChecks>)>,
}

/// A context's verdict among all of `contexts`: failed when any failed,
/// pending when any is pending, passed when any passed, else `Unknown`; and
/// the culprit while it is not passed, the first context that is not
/// (failures first), `+n` for the rest (`Vercel +2`).
pub fn tip_verdict<'a>(contexts: impl Iterator<Item = &'a Context>) -> (CiStatus, Option<String>) {
    let all: Vec<&Context> = contexts.collect();
    let with = |state: CiStatus| all.iter().filter(move |c| c.state == state);
    let status = if with(CiStatus::Failed).next().is_some() {
        CiStatus::Failed
    } else if with(CiStatus::Pending).next().is_some() {
        CiStatus::Pending
    } else if with(CiStatus::Passed).next().is_some() {
        CiStatus::Passed
    } else {
        CiStatus::Unknown
    };
    let culprits: Vec<&&Context> = with(CiStatus::Failed)
        .chain(with(CiStatus::Pending))
        .collect();
    let culprit = match culprits.split_first() {
        Some((first, rest)) if status != CiStatus::Passed => Some(match rest.len() {
            0 => first.name.clone(),
            n => format!("{} +{n}", first.name),
        }),
        _ => None,
    };
    (status, culprit)
}

#[derive(Deserialize)]
struct CombinedStatus {
    sha: String,
    #[serde(default)]
    total_count: usize,
    #[serde(default)]
    statuses: Vec<RawStatus>,
}

#[derive(Deserialize)]
struct RawStatus {
    context: String,
    state: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    target_url: Option<String>,
    #[serde(default)]
    created_at: Option<DateTime<Utc>>,
}

fn status_state(state: &str) -> CiStatus {
    match state {
        "success" => CiStatus::Passed,
        "pending" => CiStatus::Pending,
        "failure" | "error" => CiStatus::Failed,
        _ => CiStatus::Unknown,
    }
}

/// The state changes a `/statuses` body lists, newest first as GitHub
/// sends them; an entry without a time is dropped.
pub fn parse_status_history(body: &str) -> Result<Vec<StatusEvent>, Failure> {
    let page: Vec<RawStatus> =
        serde_json::from_str(body).map_err(|e| malformed("status history", &e))?;
    Ok(page
        .into_iter()
        .filter_map(|s| {
            Some(StatusEvent {
                at: s.created_at?,
                state: status_state(&s.state),
                description: s.description.filter(|d| !d.is_empty()),
                context: s.context,
            })
        })
        .collect())
}

#[derive(Deserialize)]
struct CheckRunsPage {
    #[serde(default)]
    total_count: usize,
    check_runs: Vec<RawCheckRun>,
}

#[derive(Deserialize)]
struct RawCheckRun {
    name: String,
    status: String,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    details_url: Option<String>,
    #[serde(default)]
    app: Option<RawApp>,
    #[serde(default)]
    output: Option<RawOutput>,
    #[serde(default)]
    started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    completed_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct RawApp {
    #[serde(default)]
    slug: Option<String>,
}

#[derive(Deserialize)]
struct RawOutput {
    #[serde(default)]
    title: Option<String>,
}

/// The tip and its statuses a combined-status body holds.
pub fn parse_tip_status(body: &str) -> Result<TipStatus, Failure> {
    let page: CombinedStatus =
        serde_json::from_str(body).map_err(|e| malformed("commit status", &e))?;
    let statuses: Vec<Context> = page
        .statuses
        .into_iter()
        .map(|s| Context {
            source: source_of(&s.context),
            name: s.context,
            state: status_state(&s.state),
            description: s.description.filter(|d| !d.is_empty()),
            url: s.target_url.filter(|u| !u.is_empty()),
            actions: false,
            started: None,
            completed: None,
        })
        .collect();
    Ok(TipStatus {
        sha: page.sha,
        complete: statuses.len() >= page.total_count,
        statuses,
    })
}

/// The check runs a `/check-runs` body holds. A run not yet completed is
/// pending; `success`, `neutral` and `skipped` pass; any other conclusion
/// fails, as the rollup counts it.
pub fn parse_tip_checks(body: &str) -> Result<TipChecks, Failure> {
    let page: CheckRunsPage =
        serde_json::from_str(body).map_err(|e| malformed("check runs", &e))?;
    let runs: Vec<Context> = page
        .check_runs
        .into_iter()
        .map(|r| {
            let slug = r.app.and_then(|a| a.slug).unwrap_or_default();
            Context {
                state: match (r.status.as_str(), r.conclusion.as_deref()) {
                    (status, _) if status != "completed" => CiStatus::Pending,
                    (_, Some("success" | "neutral" | "skipped")) => CiStatus::Passed,
                    (_, Some(_)) => CiStatus::Failed,
                    (_, None) => CiStatus::Unknown,
                },
                source: source_of(if slug.is_empty() { &r.name } else { &slug }),
                name: r.name,
                description: r.output.and_then(|o| o.title).filter(|t| !t.is_empty()),
                url: r.details_url.or(r.html_url).filter(|u| !u.is_empty()),
                actions: slug == "github-actions",
                started: r.started_at,
                completed: r.completed_at,
            }
        })
        .collect();
    Ok(TipChecks {
        complete: runs.len() >= page.total_count,
        runs,
    })
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
    /// Read the default branch's tip too (FZ-83), with what is held for it.
    pub tip: Option<TipRequest>,
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
    /// The default branch tip's reads (FZ-83); `None` when not asked, when
    /// the repo has no commits, or when the status call failed.
    pub tip: Option<TipAnswer>,
}

/// The repo a row's CI lives in: its remote key, and the owner and name for
/// the URL. `None` for a row with no GitHub identity, or one known only by
/// name (a legacy row with no owner), which has no URL.
pub fn target(repo: &crate::model::RepoStatus) -> Option<(RemoteKey, String, String)> {
    let key = crate::model::carrier_key(repo)?;
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
/// log of its first failed job; then the default branch tip's reads. A
/// failure stops the poll where it happened and is the attempt's outcome;
/// what answered before it is kept.
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
        tip: None,
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

    if let Some(expanded) = request.expanded {
        read_expanded(&mut answer, expanded, client, &token, &base).await;
    }

    // The tip last, so a token that cannot read statuses or checks still
    // loads the expanded run's jobs and log (FZ-83 review).
    if let (None, Some(held)) = (&answer.failure, &request.tip) {
        let repo = format!("{root}/repos/{}/{}", request.owner, request.name);
        if let Err(failure) = read_tip(&mut answer, held, client, &token, &repo).await {
            answer.failure = Some(failure);
        }
    }
    answer
}

/// The expanded run's jobs, then the log of its first failed job not held,
/// into `answer`. A failure stops here and is the attempt's outcome.
async fn read_expanded(
    answer: &mut CiAnswer,
    expanded: ExpandedRequest,
    client: &reqwest::Client,
    token: &str,
    base: &str,
) {
    let url = format!("{base}/runs/{}/jobs", expanded.run_id);
    let (result, reading) = get(client, &url, token, expanded.jobs_etag.as_deref()).await;
    answer.budget = reading.or(answer.budget.take());
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
                return;
            }
        },
        Ok(Answer::NotModified) => {
            answer.jobs = Some((expanded.run_id, JobsAnswer::Fetched(Fetched::NotModified)));
            expanded.held_jobs
        }
        Ok(Answer::NotFound) => {
            answer.jobs = Some((expanded.run_id, JobsAnswer::Gone));
            return;
        }
        Err(failure) => {
            answer.failure = Some(failure);
            return;
        }
    };

    let owed = jobs.iter().find_map(|job| {
        let failed = job.conclusion.as_deref() == Some("failure");
        let step = failing_step(job)?;
        (failed && !expanded.logs_held.contains(&job.id)).then_some((job.id, step))
    });
    let Some((job_id, step)) = owed else {
        return;
    };
    // A redirect to a download, which the client follows; it drops the
    // `Authorization` header on the way to the other host.
    let url = format!("{base}/jobs/{job_id}/logs");
    let (result, reading) = get(client, &url, token, None).await;
    answer.budget = reading.or(answer.budget.take());
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
}

/// The default branch tip's conditional reads (FZ-83) into `answer`: its
/// combined status, then the check runs of the sha that named, then, when
/// the tip has commit statuses, their history (`/statuses`), last so the
/// lamp's facts never wait on it. A 404 (a repo with no commits) reads
/// nothing more and fails nothing; any other refusal is the poll's failure,
/// with what answered before it kept.
async fn read_tip(
    answer: &mut CiAnswer,
    held: &TipRequest,
    client: &reqwest::Client,
    token: &str,
    repo: &str,
) -> Result<(), Failure> {
    let url = format!("{repo}/commits/HEAD/status");
    let (result, reading) = get(client, &url, token, held.status_etag.as_deref()).await;
    answer.budget = reading.or(answer.budget.take());
    let (sha, status, has_statuses) = match result? {
        Answer::Fresh { body, etag } => {
            let data = parse_tip_status(&body)?;
            let has = !data.statuses.is_empty();
            (data.sha.clone(), Fetched::Fresh { data, etag }, has)
        }
        Answer::NotModified => match &held.sha {
            Some(sha) => (sha.clone(), Fetched::NotModified, held.has_statuses),
            None => return Ok(()),
        },
        Answer::NotFound => return Ok(()),
    };
    answer.tip = Some(TipAnswer {
        status,
        history: None,
        checks: None,
    });
    let same_tip = held.sha.as_deref() == Some(sha.as_str());
    let etag = held.checks_etag.as_deref().filter(|_| same_tip);
    let url = format!("{repo}/commits/{sha}/check-runs?per_page={CHECKS_PER_PAGE}");
    let (result, reading) = get(client, &url, token, etag).await;
    answer.budget = reading.or(answer.budget.take());
    let checks = match result? {
        Answer::Fresh { body, etag } => Fetched::Fresh {
            data: parse_tip_checks(&body)?,
            etag,
        },
        Answer::NotModified => Fetched::NotModified,
        Answer::NotFound => return Ok(()),
    };
    if let Some(tip) = answer.tip.as_mut() {
        tip.checks = Some((sha.clone(), checks));
    }
    if !has_statuses {
        return Ok(());
    }
    let etag = held.history_etag.as_deref().filter(|_| same_tip);
    let url = format!("{repo}/commits/{sha}/statuses?per_page={CHECKS_PER_PAGE}");
    let (result, reading) = get(client, &url, token, etag).await;
    answer.budget = reading.or(answer.budget.take());
    let history = match result? {
        Answer::Fresh { body, etag } => Fetched::Fresh {
            data: parse_status_history(&body)?,
            etag,
        },
        Answer::NotModified => Fetched::NotModified,
        Answer::NotFound => return Ok(()),
    };
    if let Some(tip) = answer.tip.as_mut() {
        tip.history = Some((sha, history));
    }
    Ok(())
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
mod tests;
