//! The `ci` view of pane 2 (FZ-30d): the selected repo's recent workflow
//! runs, one row each, under a first line that says how current they are or
//! why there are none; and the small `ci` section of the `info` view, drawn
//! from what is already in hand. See
//! `docs/superpowers/specs/2026-10-21-ci-pane-design.md`, sections D and E
//! and "Amendment 2026-10-21"; and the inspector's three bodies for the
//! highlighted run, `jobs`, `log` and `script` (FZ-30f, point 5). The data
//! is `ci.rs`'s.

use chrono::{DateTime, Local, Utc};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::{App, CiBody, CiState, DataTime, Outcome};
use crate::ci::{Detail, Job, LogTail, Step, WorkflowRun};
use crate::model::{CiStatus, RepoStatus};
use crate::ui::{badges, colors};

/// Run rows the `info` section shows at most: the latest run of each of
/// the three most recently run branches.
pub const INFO_RUNS: usize = 3;

/// The `info` section's pointer to the full list, while no runs are held.
pub const RUNS_HINT: &str = "] for runs";

/// Column caps, so one long workflow or branch name cannot take the title's
/// room. Longer names truncate with `…`.
const WORKFLOW_MAX: usize = 16;
const BRANCH_MAX: usize = 24;
const EVENT_MAX: usize = 17;

/// Blank columns between two run columns.
const COLUMN_GAP: &str = "  ";

/// Columns the selection marker holds open on every row (`▸ ` or blank).
const MARKER_W: usize = 2;

/// The `ci` view's first line: the freshness mark and the status line folded
/// into one (`as of 12:02`, `⚠ 12:05  offline`, `loading`, `no GitHub
/// remote`, `no workflow runs`, `not signed in`, `GitHub off under -d`).
pub fn first_line(app: &App, now: DateTime<Local>) -> Line<'static> {
    let state = app.selected_ci().and_then(|(_, state)| state);
    let mut spans = Vec::new();
    if let Some(mark) = freshness_mark(state, now) {
        spans.push(mark);
    }
    if let Some(status) = status_line(app, now) {
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
        }
        spans.extend(trimmed(status));
    }
    Line::from(spans)
}

/// `spans` without the blank the status line pads itself with for a border.
fn trimmed(mut spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    if let Some(first) = spans.first_mut() {
        first.content = first.content.trim_start().to_string().into();
    }
    if let Some(last) = spans.last_mut() {
        last.content = last.content.trim_end().to_string().into();
    }
    spans
}

/// The `ci` view's run rows, one line each and `width` wide, the run at
/// `selected` carrying the cursor (washed while pane 2 has focus). Empty
/// while the selected repo holds no runs; the first line says why.
pub fn run_rows(
    app: &App,
    width: usize,
    now: DateTime<Local>,
    selected: usize,
    focused: bool,
) -> Vec<Line<'static>> {
    let Some(state) = app.selected_ci().and_then(|(_, state)| state) else {
        return Vec::new();
    };
    let dim_pending = app.watch_mode && !app.blink_on;
    let widths = Widths::of(&state.runs, now);
    state
        .runs
        .iter()
        .enumerate()
        .map(|(i, run)| {
            run_line(run, &widths, width, now, dim_pending).marked(i == selected, focused)
        })
        .collect()
}

/// The `info` view's `ci` section under its label, from what is in hand and
/// never polled for: the default branch's check state as the fleet poll
/// left it (the pane 1 lamp's fact), then the latest run of each of the
/// three most recently run branches when runs are held from a visit to
/// `ci`, else a dim `] for runs` hint. `None` for a row with no GitHub
/// identity, which has no section.
pub fn info_rows(
    app: &App,
    repo: &RepoStatus,
    width: usize,
    now: DateTime<Local>,
) -> Option<Vec<Line<'static>>> {
    let (key, ..) = crate::ci::target(repo)?;
    let mut lines = vec![default_branch_line(app, repo)];
    let runs = app.ci.get(&key).map_or(&[][..], |s| &s.runs[..]);
    let latest = latest_per_branch(runs);
    if latest.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("  {RUNS_HINT}"),
            Style::default().fg(colors::OVERLAY),
        )));
        return Some(lines);
    }
    let widths = Widths::of(&latest, now);
    lines.extend(
        latest
            .iter()
            .map(|run| run_line(run, &widths, width, now, false).marked(false, false)),
    );
    Some(lines)
}

/// The default branch's line: the lamp and word the repo list and the
/// header draw for `ci_status`, dimmed as they are when the last poll did
/// not return the repo, or `no checks`. Held still, as the section's run
/// rows are: the list lamp and the header's already pulse for this fact,
/// and a third in step reads as a third thing happening.
fn default_branch_line(app: &App, repo: &RepoStatus) -> Line<'static> {
    let (glyph, color, label) = badges::ci_display(&repo.ci_status, false);
    if label.is_empty() {
        return Line::from(Span::styled(
            "  no checks",
            Style::default().fg(colors::OVERLAY),
        ));
    }
    let color = if app.is_not_in_last_poll(repo) {
        badges::EARLIER_REFRESH
    } else {
        color
    };
    Line::from(vec![
        Span::styled(format!("  {glyph} "), Style::default().fg(color)),
        Span::styled("default branch ", Style::default().fg(colors::TEXT)),
        Span::styled(label, Style::default().fg(color)),
    ])
}

/// The newest run of each branch, for the first `INFO_RUNS` branches in the
/// list's order (newest first). A run with no branch is a branch of its own.
fn latest_per_branch(runs: &[WorkflowRun]) -> Vec<WorkflowRun> {
    let mut seen: Vec<Option<&str>> = Vec::new();
    let mut latest = Vec::new();
    for run in runs {
        let branch = run.branch.as_deref();
        if seen.contains(&branch) {
            continue;
        }
        seen.push(branch);
        latest.push(run.clone());
        if latest.len() == INFO_RUNS {
            break;
        }
    }
    latest
}

/// The freshness mark (spec section D): `as of 12:02` once an attempt
/// succeeded, dated by the data on screen; `⚠ 12:05` while the last one
/// failed, dated by that failed attempt (when fleetz last tried), as the
/// header's `at 12:05` is. Nothing before the first answer.
pub fn freshness_mark(state: Option<&CiState>, now: DateTime<Local>) -> Option<Span<'static>> {
    let freshness = &state?.freshness;
    let data_time = match freshness.data_time {
        DataTime::At(t) => Some(t),
        DataTime::NoData | DataTime::Unknown => None,
    };
    match &freshness.outcome {
        Outcome::NotAttempted => None,
        Outcome::Success => data_time.map(|t| {
            Span::styled(
                format!("as of {}", crate::ui::swept_label(t, now)),
                Style::default().fg(colors::OVERLAY),
            )
        }),
        Outcome::Failed(_) => {
            let text = match freshness.last_attempt {
                Some(t) => format!("⚠ {}", crate::ui::swept_label(t, now)),
                None => "⚠".to_string(),
            };
            Some(Span::styled(text, Style::default().fg(colors::PEACH)))
        }
    }
}

/// The status half of the first line: why there are no runs to show, or
/// how the last attempt failed. `None` when the runs speak for themselves.
/// Padded with a blank each side; `first_line` trims it.
///
/// - `loading`, on the spinner, before the first answer for this repo;
/// - `no GitHub remote` on a row with no GitHub identity, and
///   `GitHub off under -d`, where nothing polls;
/// - a failure in the header's words (`offline`, `not signed in`, ...), and
///   `rate limited until 12:40` while a limit holds;
/// - `no workflow runs` once GitHub answered with none.
pub fn status_line(app: &App, now: DateTime<Local>) -> Option<Vec<Span<'static>>> {
    let say = |text: String, color| {
        Some(vec![Span::styled(
            format!(" {text} "),
            Style::default().fg(color),
        )])
    };
    if app.dirty_mode {
        return say("GitHub off under -d".to_string(), colors::OVERLAY);
    }
    let Some((key, state)) = app.selected_ci() else {
        return say("no GitHub remote".to_string(), colors::OVERLAY);
    };
    let state = match state {
        Some(state) if state.freshness.last_attempt.is_some() => state,
        _ => return Some(crate::ui::spinner_group(app.spinner_frame, "loading")),
    };
    match &state.freshness.outcome {
        Outcome::Failed(failure) => {
            let label = crate::ui::github_failure_label(failure);
            let text = match app.ci_limit_until(&key, now) {
                Some(until) if failure.rate_limited => {
                    format!("{label} until {}", crate::ui::swept_label(until, now))
                }
                _ => label.to_string(),
            };
            say(text, colors::PEACH)
        }
        _ if state.runs.is_empty() => say("no workflow runs".to_string(), colors::OVERLAY),
        _ => None,
    }
}

/// The lamp state of a run: running or queued is pending, a completed run
/// passed or failed by its conclusion, and anything else (cancelled,
/// skipped, neutral) has no verdict to show.
pub fn run_status(run: &WorkflowRun) -> CiStatus {
    verdict(run.status.as_deref(), run.conclusion.as_deref())
}

/// `run_status` for anything with a status and a conclusion: a run, a job.
fn verdict(status: Option<&str>, conclusion: Option<&str>) -> CiStatus {
    if status != Some("completed") {
        return CiStatus::Pending;
    }
    match conclusion {
        Some("success") => CiStatus::Passed,
        Some("failure" | "timed_out" | "startup_failure" | "action_required") => CiStatus::Failed,
        _ => CiStatus::Unknown,
    }
}

/// How long the run took, or has taken so far while it is still moving:
/// `8s`, `1m 12s`, `2m 03s`, `1h 04m`. From its start (its creation when
/// GitHub sent none) to its last update, or to `now` while it runs.
pub fn run_duration(run: &WorkflowRun, now: DateTime<Utc>) -> String {
    let start = run.started.unwrap_or(run.created);
    let end = if run.is_live() { now } else { run.updated };
    duration_text((end - start).num_seconds())
}

/// `secs` in the run rows' words: `8s`, `1m 12s`, `1h 04m`.
fn duration_text(secs: i64) -> String {
    match secs.max(0) {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {:02}s", s / 60, s % 60),
        s => format!("{}h {:02}m", s / 3600, (s % 3600) / 60),
    }
}

/// How long ago the run was created, always with `ago`: `3m ago`, `1d ago`.
fn run_age(run: &WorkflowRun, now: DateTime<Utc>) -> String {
    let diff = now - run.created;
    let days = diff.num_days();
    match days {
        0 if diff.num_hours() == 0 => format!("{}m ago", diff.num_minutes().max(1)),
        0 => format!("{}h ago", diff.num_hours()),
        1..30 => format!("{days}d ago"),
        _ => format!("{}mo ago", days / 30),
    }
}

/// The branch column: the branch, the tag for a tag push, or a dash for an
/// event that names neither.
fn branch_text(run: &WorkflowRun) -> &str {
    run.branch.as_deref().unwrap_or("-")
}

fn workflow_text(run: &WorkflowRun) -> &str {
    run.workflow.as_deref().unwrap_or("-")
}

/// Column widths, sized as the repo list sizes its columns: to the widest
/// entry among all the runs held (not only the ones scrolled into view, so
/// the columns hold still while the view scrolls), capped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Widths {
    workflow: usize,
    branch: usize,
    event: usize,
    duration: usize,
    age: usize,
}

impl Widths {
    pub fn of(runs: &[WorkflowRun], now: DateTime<Local>) -> Self {
        let now = now.with_timezone(&Utc);
        let widest = |f: &dyn Fn(&WorkflowRun) -> usize, cap: usize| {
            runs.iter().map(f).max().unwrap_or(0).min(cap)
        };
        Self {
            workflow: widest(&|r| workflow_text(r).chars().count(), WORKFLOW_MAX),
            branch: widest(&|r| branch_text(r).chars().count(), BRANCH_MAX),
            event: widest(&|r| r.event.chars().count(), EVENT_MAX),
            duration: widest(&|r| run_duration(r, now).chars().count(), usize::MAX),
            age: widest(&|r| run_age(r, now).chars().count(), usize::MAX),
        }
    }

    /// Every column but the title's, with the marker, the lamp and the gaps.
    fn fixed(&self) -> usize {
        MARKER_W
            + 2
            + self.workflow
            + self.branch
            + self.event
            + self.duration
            + self.age
            + COLUMN_GAP.len() * 5
    }
}

/// One run row before the cursor is applied.
pub struct RunLine(Vec<Span<'static>>, usize);

impl RunLine {
    /// The row with its cursor marker, washed in the selection colour across
    /// the full width while the pane holds focus.
    fn marked(self, selected: bool, focused: bool) -> Line<'static> {
        let RunLine(spans, width) = self;
        let marker = if selected { "▸ " } else { "  " };
        let mut out = vec![Span::styled(marker, Style::default().fg(colors::LAVENDER))];
        out.extend(spans);
        if selected && focused {
            let used: usize = out.iter().map(|s| s.width()).sum();
            out.push(Span::raw(" ".repeat(width.saturating_sub(used))));
            out = out
                .into_iter()
                .map(|s| {
                    let style = s.style.bg(colors::SELECTED_BG);
                    Span::styled(s.content, style)
                })
                .collect();
        }
        Line::from(out)
    }
}

/// The cells of one run row (spec section E): lamp, workflow, branch, event,
/// title, duration, age. The title takes whatever `width` leaves.
pub fn run_line(
    run: &WorkflowRun,
    widths: &Widths,
    width: usize,
    now: DateTime<Local>,
    dim_pending: bool,
) -> RunLine {
    let utc_now = now.with_timezone(&Utc);
    let status = run_status(run);
    let (glyph, color, _) = badges::ci_display(&status, dim_pending);
    // `ci_display` draws no lamp for a repo without CI; a run always has
    // one, so a run with no verdict gets the same ring in that grey.
    let glyph = if glyph.is_empty() { "◉" } else { glyph };
    let pad = |text: &str, w: usize| format!("{:<w$}", crate::ui::truncate(text, w));
    let title_w = width.saturating_sub(widths.fixed());
    let dim = Style::default().fg(colors::OVERLAY);
    let spans = vec![
        Span::styled(format!("{glyph} "), Style::default().fg(color)),
        Span::styled(
            pad(workflow_text(run), widths.workflow),
            Style::default()
                .fg(colors::TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(COLUMN_GAP),
        Span::styled(
            pad(branch_text(run), widths.branch),
            Style::default().fg(colors::SAPPHIRE),
        ),
        Span::raw(COLUMN_GAP),
        Span::styled(pad(&run.event, widths.event), dim),
        Span::raw(COLUMN_GAP),
        Span::styled(pad(&run.title, title_w), Style::default().fg(colors::TEXT)),
        Span::raw(COLUMN_GAP),
        Span::styled(
            format!("{:>w$}", run_duration(run, utc_now), w = widths.duration),
            dim,
        ),
        Span::raw(COLUMN_GAP),
        Span::styled(
            format!("{:>w$}", run_age(run, utc_now), w = widths.age),
            dim,
        ),
    ];
    RunLine(spans, width)
}

// --- The inspector's run bodies (FZ-30f) ---

/// The inspector's lines for the highlighted run in the shown body
/// (`App::ci_body`), unwrapped: the pane wraps and scrolls them. A held
/// whole log is not among them; `shown_log` hands it over apart.
pub fn inspector_body(app: &App, now: DateTime<Local>) -> Vec<Line<'static>> {
    let Some(run) = app.ci_highlighted_run() else {
        return vec![dim_line("no run selected")];
    };
    let mut lines = run_masthead(run);
    lines.extend(match app.ci_body {
        CiBody::Jobs => jobs_body(app, run, now),
        CiBody::Log => log_body(app, run),
        CiBody::Script => script_body(app, run),
    });
    lines
}

fn dim_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(colors::OVERLAY),
    ))
}

/// The run's lamp and title, its workflow, branch and event beneath, and a
/// blank: the commit masthead's shape.
fn run_masthead(run: &WorkflowRun) -> Vec<Line<'static>> {
    let (glyph, color, _) = badges::ci_display(&run_status(run), false);
    let glyph = if glyph.is_empty() { "◉" } else { glyph };
    vec![
        Line::from(vec![
            Span::styled(format!("{glyph} "), Style::default().fg(color)),
            Span::styled(
                run.title.clone(),
                Style::default()
                    .fg(colors::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        dim_line(&format!(
            "{} · {} · {}",
            workflow_text(run),
            branch_text(run),
            run.event
        )),
        Line::from(""),
    ]
}

/// The held state of the inspector's own `/jobs` fetch in words, for a run
/// whose jobs are not in hand.
fn jobs_missing(state: Option<&CiState>, run: &WorkflowRun) -> Line<'static> {
    let Some(state) = state else {
        return dim_line("loading jobs");
    };
    if state.gone.contains(&run.id) {
        return dim_line("run gone");
    }
    match state.jobs_fetch.get(&run.id) {
        Some(Detail::Failed { message, .. }) => warn_line(message),
        Some(Detail::Unavailable(message)) => dim_line(message),
        _ => dim_line("loading jobs"),
    }
}

fn warn_line(message: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("⚠ {message}"),
        Style::default().fg(colors::PEACH),
    ))
}

/// A step's glyph and colour: `✓` passed, `✗` failed, `●` running, `·`
/// queued, skipped or cancelled. The colours are the lamp's.
fn step_glyph(step: &Step) -> (&'static str, ratatui::style::Color) {
    let status = verdict(Some(step.status.as_str()), step.conclusion.as_deref());
    let glyph = match (&status, step.status.as_str()) {
        (CiStatus::Passed, _) => "✓",
        (CiStatus::Failed, _) => "✗",
        (CiStatus::Pending, "in_progress") => "●",
        _ => "·",
    };
    let color = match glyph {
        "·" => colors::OVERLAY,
        _ => badges::ci_display(&status, false).1,
    };
    (glyph, color)
}

/// How long a job took, or has taken so far: from its start to its end, or
/// to `now` while it runs. Empty while it is queued.
fn job_duration(job: &Job, now: DateTime<Local>) -> String {
    let Some(start) = job.started else {
        return String::new();
    };
    let end = job.completed.unwrap_or_else(|| now.with_timezone(&Utc));
    duration_text((end - start).num_seconds())
}

/// The `jobs` body: each job with its lamp, name and duration, the marked
/// one carrying `▸`, its steps on one line beneath, and under a failed job
/// its failing step's name and the dimmed tail of its log.
fn jobs_body(app: &App, run: &WorkflowRun, now: DateTime<Local>) -> Vec<Line<'static>> {
    let state = app.selected_ci().and_then(|(_, state)| state);
    let Some(jobs) = app.ci_highlighted_jobs() else {
        return vec![jobs_missing(state, run)];
    };
    if jobs.is_empty() {
        return vec![dim_line("no jobs")];
    }
    let marked = app.ci_marked_job_index();
    let mut lines = Vec::new();
    for (i, job) in jobs.iter().enumerate() {
        if i > 0 {
            lines.push(Line::from(""));
        }
        let status = verdict(Some(job.status.as_str()), job.conclusion.as_deref());
        let (glyph, color, _) = badges::ci_display(&status, false);
        let glyph = if glyph.is_empty() { "◉" } else { glyph };
        let marker = if marked == Some(i) { "▸ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(colors::LAVENDER)),
            Span::styled(format!("{glyph} "), Style::default().fg(color)),
            Span::styled(
                job.name.clone(),
                Style::default()
                    .fg(colors::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", job_duration(job, now)),
                Style::default().fg(colors::OVERLAY),
            ),
        ]));
        if !job.steps.is_empty() {
            let mut spans = vec![Span::raw("    ")];
            for (n, step) in job.steps.iter().enumerate() {
                let (glyph, color) = step_glyph(step);
                if n > 0 {
                    spans.push(Span::raw("  "));
                }
                spans.push(Span::styled(
                    format!("{glyph} "),
                    Style::default().fg(color),
                ));
                spans.push(Span::styled(
                    step.name.clone(),
                    Style::default().fg(colors::SUBTEXT),
                ));
            }
            lines.push(Line::from(spans));
        }
        if status != CiStatus::Failed {
            continue;
        }
        if let Some(step) = crate::ci::failing_step(job) {
            lines.push(Line::from(Span::styled(
                format!("    ✗ {}", step.name),
                Style::default().fg(colors::RED),
            )));
        }
        match state.and_then(|s| s.logs.get(&job.id)) {
            Some(LogTail::Lines(tail)) => {
                lines.extend(tail.iter().map(|l| dim_line(&format!("    {l}"))));
            }
            Some(LogTail::Unavailable(message)) => lines.push(dim_line(&format!("    {message}"))),
            None => lines.push(dim_line("    loading log")),
        }
    }
    lines
}

/// The `log` body: the marked job's whole log, `##[error]` lines in the
/// failing red and `##[group]` headers lifted.
fn log_body(app: &App, run: &WorkflowRun) -> Vec<Line<'static>> {
    let state = app.selected_ci().and_then(|(_, state)| state);
    let Some(job) = app.ci_marked_job() else {
        return match app.ci_highlighted_jobs() {
            Some(_) => vec![dim_line("no jobs")],
            None => vec![jobs_missing(state, run)],
        };
    };
    let mut lines = vec![
        Line::from(Span::styled(
            format!("{} log", job.name),
            Style::default()
                .fg(colors::TEXT)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    if job.status != "completed" {
        lines.push(dim_line("the log arrives once the job finishes"));
        return lines;
    }
    match state.and_then(|s| s.full_logs.get(&job.id)) {
        // The log itself follows these lines: `shown_log`, wrapped once.
        Some(Detail::Ready(_)) => {}
        Some(Detail::Unavailable(message)) => lines.push(dim_line(message)),
        Some(Detail::Failed { message, .. }) => lines.push(warn_line(message)),
        Some(Detail::Loading) | None => lines.push(dim_line("loading log")),
    }
    lines
}

/// The whole log the `log` body shows under `inspector_body`'s lines, and
/// its job id: the marked, finished job's held log. Kept apart so the pane
/// wraps it once per job and width (`App::ci_log_wrap`) rather than on
/// every frame.
pub fn shown_log(app: &App) -> Option<(u64, &[String])> {
    if app.ci_body != CiBody::Log {
        return None;
    }
    let job = app.ci_marked_job()?;
    if job.status != "completed" {
        return None;
    }
    let state = app.selected_ci()?.1?;
    match state.full_logs.get(&job.id)? {
        Detail::Ready(log) => Some((job.id, &log[..])),
        _ => None,
    }
}

/// One whole-log line, `##[error]` in the failing red and `##[group]`
/// headers lifted.
pub fn log_line(text: &str) -> Line<'static> {
    let style = if text.starts_with("##[error]") {
        Style::default().fg(colors::RED)
    } else if text.starts_with("##[group]") {
        Style::default()
            .fg(colors::SUBTEXT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors::TEXT)
    };
    Line::from(Span::styled(text.to_string(), style))
}

/// The step whose block the `script` body highlights: when the run failed,
/// the marked job's failing step, else the first failed job's.
fn highlighted_step(app: &App, run: &WorkflowRun) -> Option<Step> {
    if run_status(run) != CiStatus::Failed {
        return None;
    }
    let marked = app.ci_marked_job().and_then(crate::ci::failing_step);
    marked
        .or_else(|| {
            app.ci_highlighted_jobs()?
                .iter()
                .find_map(crate::ci::failing_step)
        })
        .cloned()
}

/// The `script` body: the workflow file the run ran, at its commit, with
/// line numbers, and the failing step's block marked in red when the run
/// failed and the step can be found.
fn script_body(app: &App, run: &WorkflowRun) -> Vec<Line<'static>> {
    let (Some(sha), Some(path)) = (run.head_sha.as_deref(), run.path.as_deref()) else {
        return vec![dim_line("no workflow file named for this run")];
    };
    let short: String = sha.chars().take(7).collect();
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                path.to_string(),
                Style::default()
                    .fg(colors::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" @ ", Style::default().fg(colors::SURFACE)),
            Span::styled(short, Style::default().fg(colors::HASH_CLR)),
        ]),
        Line::from(""),
    ];
    let state = app.selected_ci().and_then(|(_, state)| state);
    let held = state.and_then(|s| s.scripts.get(&(sha.to_string(), path.to_string())));
    let text = match held {
        Some(Detail::Ready(text)) => text,
        Some(Detail::Unavailable(message)) => {
            lines.push(dim_line(message));
            return lines;
        }
        Some(Detail::Failed { message, .. }) => {
            lines.push(warn_line(message));
            return lines;
        }
        Some(Detail::Loading) | None => {
            lines.push(dim_line("loading script"));
            return lines;
        }
    };
    let step = highlighted_step(app, run);
    let block = step
        .as_ref()
        .and_then(|s| crate::ci::step_block(text, &s.name));
    if let (Some(step), Some(block)) = (&step, &block) {
        lines.push(Line::from(Span::styled(
            format!("✗ {}  (line {})", step.name, block.start + 1),
            Style::default().fg(colors::RED),
        )));
        lines.push(Line::from(""));
    }
    let count = text.lines().count();
    let gutter = count.to_string().len();
    for (i, line) in text.lines().enumerate() {
        let marked = block.as_ref().is_some_and(|b| b.contains(&i));
        let (mark, text_style) = if marked {
            (
                Span::styled("▌", Style::default().fg(colors::RED)),
                Style::default()
                    .fg(colors::TEXT)
                    .add_modifier(Modifier::BOLD),
            )
        } else if line.trim_start().starts_with('#') {
            (Span::raw(" "), Style::default().fg(colors::OVERLAY))
        } else {
            (Span::raw(" "), Style::default().fg(colors::TEXT))
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:>gutter$} ", i + 1),
                Style::default().fg(colors::SURFACE),
            ),
            mark,
            Span::styled(format!(" {line}"), text_style),
        ]));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{FocusedPanel, HistoryView, View};
    use chrono::{Datelike, TimeZone, Timelike};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    const RUNS: &str = include_str!("../ci/fixtures/runs.json");

    fn runs() -> Vec<WorkflowRun> {
        crate::ci::parse_runs(RUNS).unwrap()
    }

    // Wall-clock components, not the offset: CI runs in UTC, and the tests assert local-time strings.
    fn local(text: &str) -> DateTime<Local> {
        let wall = DateTime::parse_from_rfc3339(text).unwrap().naive_local();
        Local
            .with_ymd_and_hms(
                wall.year(),
                wall.month(),
                wall.day(),
                wall.hour(),
                wall.minute(),
                wall.second(),
            )
            .single()
            .expect("unambiguous local time")
    }

    fn key() -> crate::app::RemoteKey {
        (Some("gaurgle".to_string()), "fleetz".to_string())
    }

    fn repo() -> RepoStatus {
        use crate::model;
        RepoStatus {
            daily: None,
            daily_key: None,
            last_local: None,
            name: "fleetz".to_string(),
            path: None,
            location: model::RepoLocation::Primary,
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
            repo_kind: model::RepoKind::Unknown,
            owner: Some("Gaurgle".to_string()),
            remote_url: None,
            remote_only_branches: vec![],
            remote_branches: vec![],
            work_key: None,
            fetch: model::FetchFacts::default(),
        }
    }

    /// No jitter: the ladder and the resume land on whole numbers.
    struct NoJitter;

    impl crate::app::Jitter for NoJitter {
        fn backoff_factor(&mut self) -> f64 {
            1.0
        }
        fn resume_secs(&mut self) -> i64 {
            0
        }
    }

    /// An app on one GitHub repo, pane 2 showing `ci`, nothing polled.
    fn app() -> App {
        let mut app = App::new(Vec::new(), String::new(), 120, 44);
        app.repos = vec![repo()];
        app.view = View::Detail;
        app.history_view = HistoryView::Ci;
        app.jitter = Box::new(NoJitter);
        app
    }

    /// Land a successful list of `runs` at `now`.
    fn landed(app: &mut App, runs: Vec<WorkflowRun>, now: DateTime<Local>) {
        let request = app.claim_ci_poll(key(), "gaurgle".into(), "fleetz".into());
        app.apply_ci(
            crate::ci::CiAnswer {
                key: key(),
                attempt: request.attempt,
                runs: Some(crate::ci::Fetched::Fresh {
                    data: runs,
                    etag: None,
                }),
                jobs: None,
                log: None,
                full_log: None,
                failure: None,
                budget: None,
            },
            now,
        );
    }

    fn failed(app: &mut App, failure: crate::github::Failure, now: DateTime<Local>) {
        let request = app.claim_ci_poll(key(), "gaurgle".into(), "fleetz".into());
        app.apply_ci(
            crate::ci::CiAnswer {
                key: key(),
                attempt: request.attempt,
                runs: None,
                jobs: None,
                log: None,
                full_log: None,
                failure: Some(failure),
                budget: None,
            },
            now,
        );
    }

    fn draw(app: &App, width: u16, height: u16) -> (crate::ui::PanelRects, Buffer) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| crate::ui::render(f, app)).unwrap();
        let rects = crate::ui::panel_rects(Rect::new(0, 0, width, height), app);
        (rects, terminal.backend().buffer().clone())
    }

    fn row_text(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    /// The detail pane's rows as drawn, one string each.
    fn detail_rows(app: &App) -> Vec<String> {
        let (rects, buffer) = draw(app, 160, 50);
        let detail = rects.detail.expect("detail pane laid out");
        (detail.y..detail.y + detail.height)
            .map(|y| {
                (detail.x..detail.x + detail.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }

    /// The start of a run's title, which a narrow title column truncates.
    fn head(run: &WorkflowRun) -> String {
        run.title.chars().take(20).collect()
    }

    fn line_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn status_text(app: &App) -> Option<String> {
        status_line(app, Local::now())
            .map(|spans| spans.iter().map(|s| s.content.as_ref()).collect())
    }

    // --- Title and status line ---

    #[test]
    fn the_title_carries_the_freshness_mark() {
        let now = local("2026-10-21T12:05:00+02:00");
        let mut app = app();
        assert_eq!(freshness_mark(None, now), None, "nothing before an answer");
        landed(&mut app, runs(), local("2026-10-21T12:02:00+02:00"));
        let mark = freshness_mark(app.ci.get(&key()), now).unwrap();
        assert_eq!(mark.content, "as of 12:02");
        let offline = crate::github::Failure::new(crate::github::FailureKind::Offline, "x");
        failed(&mut app, offline, now);
        let mark = freshness_mark(app.ci.get(&key()), now).unwrap();
        assert_eq!(mark.content, "⚠ 12:05", "dated by the failed attempt");
        assert_eq!(mark.style.fg, Some(colors::PEACH));
    }

    #[test]
    fn the_status_line_names_each_empty_state() {
        let mut app = app();
        let loading = status_text(&app).unwrap();
        assert!(loading.contains("loading"), "{loading}");

        landed(&mut app, Vec::new(), Local::now());
        assert_eq!(status_text(&app).as_deref(), Some(" no workflow runs "));

        let mut signed_out = self::app();
        let auth = crate::github::Failure::new(crate::github::FailureKind::Auth, "no token");
        failed(&mut signed_out, auth, Local::now());
        assert_eq!(status_text(&signed_out).as_deref(), Some(" not signed in "));

        let mut offline = self::app();
        let failure = crate::github::Failure::new(crate::github::FailureKind::Offline, "x");
        failed(&mut offline, failure, Local::now());
        assert_eq!(status_text(&offline).as_deref(), Some(" offline "));

        let mut local_only = self::app();
        local_only.repos[0].owner = None;
        assert_eq!(
            status_text(&local_only).as_deref(),
            Some(" no GitHub remote ")
        );

        landed(&mut app, runs(), Local::now());
        assert_eq!(status_text(&app), None, "the runs speak for themselves");
    }

    #[test]
    fn a_rate_limit_says_until_when() {
        let now = local("2026-10-21T12:00:10+02:00");
        let mut app = app();
        let request = app.claim_ci_poll(key(), "gaurgle".into(), "fleetz".into());
        let reset = local("2026-10-21T12:40:00+02:00");
        let reading = crate::github::BudgetReading {
            limit: 5000,
            remaining: 0,
            used: 5000,
            reset: reset.timestamp(),
            date: Some(now.with_timezone(&Utc)),
            token_id: "tok".to_string(),
        };
        app.apply_ci(
            crate::ci::CiAnswer {
                key: key(),
                attempt: request.attempt,
                runs: None,
                jobs: None,
                log: None,
                full_log: None,
                failure: Some(crate::github::Failure::rate_limit(
                    "API rate limit exceeded",
                )),
                budget: Some(reading),
            },
            now,
        );
        let text: String = status_line(&app, now)
            .unwrap()
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(text, " rate limited until 12:40 ");
    }

    /// FZ-30d, ruling 5: the view's first line is the freshness mark and
    /// the status folded into one, unpadded. Fails while the two live in a
    /// band's title and bottom border.
    #[test]
    fn the_first_line_folds_the_mark_and_the_status_together() {
        let now = local("2026-10-21T12:05:00+02:00");
        let first = |app: &App| line_text(&first_line(app, now));

        let mut app = app();
        assert!(first(&app).ends_with("loading"), "{}", first(&app));

        landed(&mut app, runs(), local("2026-10-21T12:02:00+02:00"));
        assert_eq!(first(&app), "as of 12:02");

        let offline = crate::github::Failure::new(crate::github::FailureKind::Offline, "x");
        failed(&mut app, offline, now);
        assert_eq!(first(&app), "⚠ 12:05  offline");

        let mut empty = self::app();
        landed(&mut empty, Vec::new(), local("2026-10-21T12:02:00+02:00"));
        assert_eq!(first(&empty), "as of 12:02  no workflow runs");

        let mut local_only = self::app();
        local_only.repos[0].owner = None;
        assert_eq!(first(&local_only), "no GitHub remote");

        let mut dirty = self::app();
        dirty.dirty_mode = true;
        assert_eq!(first(&dirty), "GitHub off under -d");

        let mut signed_out = self::app();
        let auth = crate::github::Failure::new(crate::github::FailureKind::Auth, "no token");
        failed(&mut signed_out, auth, now);
        assert_eq!(first(&signed_out), "⚠ 12:05  not signed in");
    }

    // --- Run rows ---

    #[test]
    fn a_run_row_has_every_column_and_the_title_takes_the_slack() {
        let runs = runs();
        let now = local("2026-10-21T13:16:30+02:00");
        let widths = Widths::of(&runs, now);
        let text = |width| -> String {
            let line = run_line(&runs[1], &widths, width, now, false).marked(false, false);
            line.spans.iter().map(|s| s.content.as_ref()).collect()
        };
        let wide = text(160);
        let run = &runs[1];
        let branch = crate::ui::truncate(run.branch.as_deref().unwrap(), BRANCH_MAX);
        assert!(
            branch.ends_with('…'),
            "the fixture's branch is past the cap"
        );
        for cell in [
            run.workflow.as_deref().unwrap(),
            branch.as_str(),
            run.event.as_str(),
            run.title.as_str(),
            "ago",
        ] {
            assert!(wide.contains(cell), "{cell} missing from {wide}");
        }
        assert!(wide.contains(&run_duration(run, now.with_timezone(&Utc))));
        assert_eq!(wide.chars().count(), 160, "the title fills the row");
        let narrow = text(90);
        assert_eq!(narrow.chars().count(), 90);
        assert!(narrow.contains('…'), "the title gives way: {narrow}");
        assert!(narrow.ends_with("ago"), "duration and age kept: {narrow}");
    }

    #[test]
    fn the_lamp_takes_the_ci_colours_and_pulses_while_running() {
        let runs = runs();
        assert_eq!(run_status(&runs[0]), CiStatus::Pending, "in progress");
        assert_eq!(run_status(&runs[1]), CiStatus::Passed);
        assert_eq!(run_status(&runs[2]), CiStatus::Failed);
        let now = local("2026-10-21T13:16:30+02:00");
        let widths = Widths::of(&runs, now);
        let lamp = |run, dim| run_line(run, &widths, 120, now, dim).0[0].style.fg;
        assert_eq!(lamp(&runs[1], false), Some(colors::GREEN));
        assert_eq!(lamp(&runs[2], false), Some(colors::RED));
        assert_eq!(lamp(&runs[0], false), Some(colors::PEACH));
        assert_eq!(lamp(&runs[0], true), Some(colors::SURFACE), "the off phase");
        assert_eq!(
            lamp(&runs[1], true),
            Some(colors::GREEN),
            "only pending pulses"
        );
        let mut cancelled = runs[1].clone();
        cancelled.conclusion = Some("cancelled".to_string());
        assert_eq!(run_status(&cancelled), CiStatus::Unknown);
    }

    #[test]
    fn the_duration_ticks_while_the_run_is_live_and_holds_once_done() {
        let runs = runs();
        let live = &runs[0];
        let at = |t: &str| DateTime::parse_from_rfc3339(t).unwrap().with_timezone(&Utc);
        // Started 11:16:07Z.
        assert_eq!(run_duration(live, at("2026-10-21T11:16:15Z")), "8s");
        assert_eq!(run_duration(live, at("2026-10-21T11:18:10Z")), "2m 03s");
        let done = &runs[1];
        let held = run_duration(done, at("2026-10-21T11:30:00Z"));
        assert_eq!(held, run_duration(done, at("2026-10-22T11:30:00Z")));
        let mut long = done.clone();
        long.started = Some(long.updated - chrono::Duration::seconds(3840));
        assert_eq!(run_duration(&long, at("2026-10-22T11:30:00Z")), "1h 04m");
    }

    #[test]
    fn the_age_always_says_ago() {
        let run = &runs()[0];
        let at = |t: &str| DateTime::parse_from_rfc3339(t).unwrap().with_timezone(&Utc);
        assert_eq!(run_age(run, at("2026-10-21T11:16:30Z")), "1m ago");
        assert_eq!(run_age(run, at("2026-10-21T11:19:07Z")), "3m ago");
        assert_eq!(run_age(run, at("2026-10-21T14:16:07Z")), "3h ago");
        assert_eq!(run_age(run, at("2026-10-22T12:00:00Z")), "1d ago");
    }

    // --- The `ci` view in pane 2 ---

    /// `ci` is pane 2's sixth view: its tab names it, its first line says
    /// how current the runs are, and the recorded runs follow one row each,
    /// the cursor on the first. Fails while the runs live in a band.
    #[test]
    fn the_ci_view_draws_the_held_runs_under_its_first_line() {
        let mut app = app();
        app.focused_panel = FocusedPanel::Detail;
        landed(&mut app, runs(), Local::now());
        let rows = detail_rows(&app);
        assert!(rows[0].contains("CI"), "the tab: {}", rows[0]);
        let first = rows
            .iter()
            .position(|r| r.contains("as of "))
            .expect("the first line");
        let held = runs();
        for (i, run) in held.iter().enumerate() {
            let row = &rows[first + 1 + i];
            assert!(row.contains(&head(run)), "row {i}: {row}");
            assert_eq!(row.contains('▸'), i == 0, "cursor on the first: {row}");
        }
        assert!(
            rows.iter().all(|r| !r.contains("[5]")),
            "no band, no fifth pane"
        );
    }

    /// With nothing held the view is its first line alone: the empty state
    /// and no rows.
    #[test]
    fn the_ci_view_shows_its_empty_states_on_the_first_line() {
        let mut app = app();
        landed(&mut app, Vec::new(), Local::now());
        let rows = detail_rows(&app);
        assert!(
            rows.iter().any(|r| r.contains("no workflow runs")),
            "{rows:#?}"
        );
        assert!(rows.iter().all(|r| !r.contains('▸')), "no cursor");

        let mut local_only = self::app();
        local_only.repos[0].owner = None;
        let rows = detail_rows(&local_only);
        assert!(rows.iter().any(|r| r.contains("no GitHub remote")));
    }

    /// `j`/`k` walk the runs through the view's own selection slot, and the
    /// marked row follows. Fails while the cursor lives on `CiState`.
    #[test]
    fn the_cursor_is_the_views_selection_and_the_marked_row_follows() {
        let mut app = app();
        app.focused_panel = FocusedPanel::Detail;
        landed(&mut app, runs(), Local::now());
        assert_eq!(app.selected_commit_count(), 3);
        app.detail_nav_down();
        app.detail_nav_down();
        app.detail_nav_down();
        assert_eq!(app.detail_selected(), 2, "clamped to the last run");
        let rows = detail_rows(&app);
        let marked: Vec<&String> = rows.iter().filter(|r| r.contains('▸')).collect();
        assert_eq!(marked.len(), 1);
        assert!(marked[0].contains(&head(&runs()[2])), "{}", marked[0]);
        app.detail_nav_up();
        assert_eq!(app.detail_selected(), 1);
        assert_eq!(
            app.detail_selections[HistoryView::Commits.index()],
            0,
            "another view's slot untouched"
        );
    }

    /// The #91 review's follow-up: a shorter list landing for the repo on
    /// screen pulls the cursor onto its last run. Fails while the cursor can
    /// sit past the end of the list.
    #[test]
    fn a_shorter_list_landing_clamps_the_cursor() {
        let mut app = app();
        landed(&mut app, runs(), Local::now());
        app.set_detail_selected(2);
        landed(&mut app, runs()[..1].to_vec(), Local::now());
        assert_eq!(app.detail_selected(), 0);
        landed(&mut app, Vec::new(), Local::now());
        assert_eq!(app.detail_selected(), 0, "an empty list parks it at 0");
    }

    /// A click on a run row moves the view's cursor there, through the
    /// pane's own hit map.
    #[test]
    fn a_click_on_a_run_row_selects_it() {
        let mut app = app();
        landed(&mut app, runs(), Local::now());
        let (_, buffer) = draw(&app, 160, 50);
        let title = head(&runs()[2]);
        let y = (0..50)
            .find(|&y| row_text(&buffer, y).contains(title.as_str()))
            .expect("third run drawn");
        let hits = app.detail_hits.borrow().clone();
        let click = hits.at(hits.body.x + 4, y).expect("a run row");
        app.click_detail(&click);
        assert_eq!(app.detail_selected(), 2);
    }

    // --- The inspector's run bodies (FZ-30f) ---

    const JOBS_FAILED: &str = include_str!("../ci/fixtures/jobs-failed.json");
    const LOG_FAILED: &str = include_str!("../ci/fixtures/job-log-failed.txt");
    const CI_YML: &str = include_str!("../ci/fixtures/ci.yml");
    /// The recorded failed run is the fixture's third.
    const FAILED_RUN: usize = 2;
    const FAILED_JOB: u64 = 113914377501;
    const WORKFLOW: &str = ".github/workflows/ci.yml";

    /// The runs fixture with the failed run naming its workflow file, which
    /// the trimmed recording left out.
    fn runs_with_path() -> Vec<WorkflowRun> {
        let mut runs = runs();
        runs[FAILED_RUN].path = Some(WORKFLOW.to_string());
        runs
    }

    fn jobs() -> Vec<Job> {
        crate::ci::parse_jobs(JOBS_FAILED).unwrap()
    }

    /// An app in `ci` on the recorded failed run, its jobs, tail and whole
    /// log landed by one poll, as the poll for a highlighted failed run
    /// lands them.
    fn on_failed_run() -> App {
        let mut app = app();
        landed(&mut app, runs_with_path(), Local::now());
        app.set_detail_selected(FAILED_RUN);
        let run_id = runs()[FAILED_RUN].id;
        let request = app.claim_ci_poll(key(), "gaurgle".into(), "fleetz".into());
        let step = crate::ci::failing_step(&jobs()[0]).cloned().unwrap();
        app.apply_ci(
            crate::ci::CiAnswer {
                key: key(),
                attempt: request.attempt,
                runs: Some(crate::ci::Fetched::NotModified),
                jobs: Some((
                    run_id,
                    crate::ci::JobsAnswer::Fetched(crate::ci::Fetched::Fresh {
                        data: jobs(),
                        etag: None,
                    }),
                )),
                log: Some((
                    FAILED_JOB,
                    LogTail::Lines(crate::ci::log_tail(LOG_FAILED, &step)),
                )),
                full_log: Some((FAILED_JOB, crate::ci::log_lines(LOG_FAILED))),
                failure: None,
                budget: None,
            },
            Local::now(),
        );
        app
    }

    fn body_text(app: &App) -> Vec<String> {
        inspector_body(app, Local::now())
            .iter()
            .map(line_text)
            .collect()
    }

    fn state(app: &App) -> &CiState {
        &app.ci[&key()]
    }

    /// FZ-30f: the `jobs` body from recorded payloads: the run, each job
    /// with its lamp and duration, its steps on one line, and under the
    /// failed job its failing step and the dimmed log tail. Fails while the
    /// inspector says `no commit selected` on a run.
    #[test]
    fn the_jobs_body_draws_the_jobs_steps_and_the_failing_tail() {
        let app = on_failed_run();
        assert_eq!(app.ci_body, CiBody::Jobs, "jobs is the first body");
        let text = body_text(&app);
        assert!(text[0].contains("feat: pace GitHub polling"), "{text:#?}");
        let job = text.iter().position(|l| l.contains("check")).unwrap();
        assert!(text[job].starts_with("▸ ◉ check"), "marked: {}", text[job]);
        assert!(
            text[job].ends_with("54s"),
            "16:19:46 to 16:20:40: {}",
            text[job]
        );
        let steps = &text[job + 1];
        assert!(steps.contains("✓ Set up job"), "{steps}");
        assert!(steps.contains("✗ Run cargo clippy"), "{steps}");
        assert!(steps.contains("· Run cargo test"), "skipped: {steps}");
        assert_eq!(
            text[job + 2],
            "    ✗ Run cargo clippy --all-targets -- -D warnings"
        );
        let tail = &text[job + 3..];
        assert_eq!(tail.len(), crate::ci::LOG_TAIL_LINES);
        assert_eq!(
            tail.last().unwrap(),
            "    ##[error]Process completed with exit code 101."
        );
        let lines = inspector_body(&app, Local::now());
        assert_eq!(
            lines[job + 3].spans[0].style.fg,
            Some(colors::OVERLAY),
            "the tail is dimmed"
        );
    }

    /// The `log` body: the marked job's whole log, stamps stripped.
    #[test]
    fn the_log_body_draws_the_marked_jobs_whole_log() {
        let mut app = on_failed_run();
        app.ci_body = CiBody::Log;
        let text = body_text(&app);
        assert!(text.contains(&"check log".to_string()), "{text:#?}");
        let (job_id, log) = shown_log(&app).expect("the held log");
        assert_eq!(job_id, FAILED_JOB);
        assert_eq!(log, &crate::ci::log_lines(LOG_FAILED)[..]);
        assert!(log.iter().all(|l| !l.starts_with("2026-")));
        let (rects, buffer) = draw(&app, 160, 50);
        let inspector = rects.inspector.expect("inspector laid out");
        let drawn: Vec<String> = (inspector.y..inspector.y + inspector.height)
            .map(|y| row_text(&buffer, y))
            .collect();
        assert!(
            drawn
                .iter()
                .any(|r| r.contains("Current runner version: '2.337.0'")),
            "the log is drawn under its head: {drawn:#?}"
        );
    }

    /// The whole log is wrapped once per job and width and the copy kept:
    /// a second frame draws the kept rows, a new width wraps again. Fails
    /// while every frame re-wraps the log.
    #[test]
    fn the_whole_log_is_wrapped_once_per_job_and_width() {
        let mut app = on_failed_run();
        app.ci_body = CiBody::Log;
        draw(&app, 160, 50);
        let width = {
            let mut kept = app.ci_log_wrap.borrow_mut();
            let wrap = kept.as_mut().expect("wrapped on the first frame");
            assert_eq!(wrap.job_id, FAILED_JOB);
            wrap.lines[0] = Line::from("KEPT ROW");
            wrap.width
        };
        let (_, buffer) = draw(&app, 160, 50);
        let rows: Vec<String> = (0..50).map(|y| row_text(&buffer, y)).collect();
        assert!(
            rows.iter().any(|r| r.contains("KEPT ROW")),
            "not re-wrapped"
        );
        let (_, buffer) = draw(&app, 140, 50);
        let rows: Vec<String> = (0..50).map(|y| row_text(&buffer, y)).collect();
        assert!(
            rows.iter().all(|r| !r.contains("KEPT ROW")),
            "a new width wraps again"
        );
        assert_ne!(app.ci_log_wrap.borrow().as_ref().unwrap().width, width);
    }

    /// Another job's log at the same width is wrapped afresh, never drawn
    /// from the first job's kept rows. Fails while the cache is keyed by
    /// width alone.
    #[test]
    fn another_jobs_log_at_the_same_width_is_wrapped_afresh() {
        let mut app = on_failed_run();
        let mut second = jobs()[0].clone();
        second.id = 7;
        second.name = "docs".to_string();
        let run_id = runs()[FAILED_RUN].id;
        let held = app.ci.get_mut(&key()).unwrap();
        held.jobs.get_mut(&run_id).unwrap().jobs.push(second);
        held.full_logs.insert(
            7,
            Detail::Ready(vec!["the second job's own log".to_string()]),
        );
        app.ci_body = CiBody::Log;
        draw(&app, 160, 50);
        {
            let mut kept = app.ci_log_wrap.borrow_mut();
            let wrap = kept.as_mut().expect("wrapped on the first frame");
            assert_eq!(wrap.job_id, FAILED_JOB);
            wrap.lines[0] = Line::from("KEPT ROW");
        }
        app.ci_job_mark = Some(1);
        assert_eq!(app.ci_marked_job().map(|j| j.id), Some(7));
        let (_, buffer) = draw(&app, 160, 50);
        let rows: Vec<String> = (0..50).map(|y| row_text(&buffer, y)).collect();
        assert!(
            rows.iter().all(|r| !r.contains("KEPT ROW")),
            "the first job's rows are gone: {rows:#?}"
        );
        assert!(rows.iter().any(|r| r.contains("the second job's own log")));
        assert_eq!(app.ci_log_wrap.borrow().as_ref().unwrap().job_id, 7);
    }

    /// An app whose next `claim_ci_details` claims the second run's jobs
    /// at `at`, so each gate below can be shown to hold that claim alone.
    fn claimable() -> (App, std::time::Instant) {
        let mut app = on_failed_run();
        app.set_detail_selected(1);
        (app, std::time::Instant::now() + crate::ci::DWELL * 2)
    }

    #[test]
    fn under_d_the_inspector_fetches_nothing() {
        let (mut app, at) = claimable();
        app.dirty_mode = true;
        assert!(app.claim_ci_details(Local::now(), at).is_empty());
        app.dirty_mode = false;
        assert_eq!(app.claim_ci_details(Local::now(), at).len(), 1);
    }

    #[test]
    fn outside_the_ci_view_the_inspector_fetches_nothing() {
        let (mut app, at) = claimable();
        app.history_view = HistoryView::Branches;
        assert!(app.claim_ci_details(Local::now(), at).is_empty());
        app.history_view = HistoryView::Ci;
        assert_eq!(app.claim_ci_details(Local::now(), at).len(), 1);
    }

    #[test]
    fn a_folded_inspector_fetches_nothing() {
        let (mut app, at) = claimable();
        app.inspector_folded = true;
        assert!(app.claim_ci_details(Local::now(), at).is_empty());
        app.inspector_folded = false;
        assert_eq!(app.claim_ci_details(Local::now(), at).len(), 1);
    }

    /// Take a `core` reading of `remaining` of 5000, resetting at 12:40.
    fn read_budget(app: &mut App, remaining: u32, now: DateTime<Local>) {
        let reading = crate::github::BudgetReading {
            limit: 5000,
            remaining,
            used: 5000 - remaining,
            reset: local("2026-10-21T12:40:00+02:00").timestamp(),
            date: Some(now.with_timezone(&Utc)),
            token_id: "tok".to_string(),
        };
        app.ci_budget.observe(&reading, true, now, &mut NoJitter);
    }

    #[test]
    fn a_spent_core_budget_holds_the_inspector_fetches_until_the_reset() {
        let (mut app, at) = claimable();
        let now = local("2026-10-21T12:00:10+02:00");
        read_budget(&mut app, 0, now);
        assert!(app.claim_ci_details(now, at).is_empty());
        let reset = local("2026-10-21T12:40:01+02:00");
        assert_eq!(app.claim_ci_details(reset, at).len(), 1);
    }

    /// F1 of the review: the inspector keeps the poll's reserve, so it
    /// cannot spend the last tenth of the budget the poll paces around.
    #[test]
    fn the_inspector_fetches_honour_the_polls_reserve() {
        let (mut app, at) = claimable();
        let now = local("2026-10-21T12:00:10+02:00");
        read_budget(&mut app, 500, now);
        assert!(app.claim_ci_details(now, at).is_empty(), "at the reserve");
        let reset = local("2026-10-21T12:40:01+02:00");
        assert_eq!(app.claim_ci_details(reset, at).len(), 1);
        let (mut above, at) = claimable();
        read_budget(&mut above, 501, now);
        assert_eq!(above.claim_ci_details(now, at).len(), 1, "above it");
    }

    /// The `script` body: the workflow file at the run's commit, the failing
    /// step's line marked when the run failed. Fails while the inspector
    /// has no script.
    #[test]
    fn the_script_body_highlights_the_failing_steps_block() {
        let mut app = on_failed_run();
        app.ci_body = CiBody::Script;
        let sha = runs()[FAILED_RUN].head_sha.clone().unwrap();
        assert!(body_text(&app).contains(&"loading script".to_string()));
        app.ci.get_mut(&key()).unwrap().scripts.insert(
            (sha, WORKFLOW.to_string()),
            Detail::Ready(CI_YML.to_string()),
        );
        let text = body_text(&app);
        assert!(
            text.iter()
                .any(|l| l.contains(".github/workflows/ci.yml @ 93f4c8e"))
        );
        assert!(
            text.contains(
                &"✗ Run cargo clippy --all-targets -- -D warnings  (line 27)".to_string()
            ),
            "{text:#?}"
        );
        let marked: Vec<&String> = text.iter().filter(|l| l.contains('▌')).collect();
        assert_eq!(marked.len(), 1, "{marked:#?}");
        assert!(marked[0].contains("- run: cargo clippy --all-targets -- -D warnings"));
        assert!(marked[0].starts_with("27 ▌"), "{}", marked[0]);

        // A passing run highlights nothing.
        app.ci.get_mut(&key()).unwrap().runs[FAILED_RUN].conclusion = Some("success".to_string());
        assert!(body_text(&app).iter().all(|l| !l.contains('▌')));
    }

    /// `[`/`]` on the focused inspector cycle its bodies, both ways, and the
    /// title names the body; anywhere else they still cycle pane 2's views.
    #[test]
    fn brackets_cycle_the_bodies_on_the_focused_inspector_and_the_title_names_it() {
        let mut app = on_failed_run();
        app.focused_panel = FocusedPanel::Inspector;
        app.press_bracket(false);
        assert_eq!(app.ci_body, CiBody::Log);
        app.press_bracket(false);
        assert_eq!(app.ci_body, CiBody::Script);
        app.press_bracket(false);
        assert_eq!(app.ci_body, CiBody::Jobs, "wraps forward");
        app.press_bracket(true);
        assert_eq!(app.ci_body, CiBody::Script, "wraps backward");
        assert_eq!(app.history_view, HistoryView::Ci, "the view held");

        app.ci_body = CiBody::Log;
        let (rects, buffer) = draw(&app, 160, 50);
        let inspector = rects.inspector.expect("inspector laid out");
        let title = row_text(&buffer, inspector.y);
        assert!(title.contains("[3] inspector  log"), "{title}");

        app.focused_panel = FocusedPanel::Detail;
        app.press_bracket(false);
        assert_eq!(
            app.history_view,
            HistoryView::Info,
            "pane 2 cycles its views"
        );
        assert_eq!(app.ci_body, CiBody::Log);
    }

    /// Each body keeps its own scroll; a different highlighted run starts
    /// every body at the top and the job mark on its default.
    #[test]
    fn a_run_change_resets_every_bodys_scroll_and_the_mark() {
        let mut app = on_failed_run();
        app.focused_panel = FocusedPanel::Inspector;
        app.inspector_max_scroll.set(100);
        app.ci_body = CiBody::Log;
        app.scroll_inspector(5);
        app.ci_body = CiBody::Script;
        app.scroll_inspector(2);
        assert_eq!(app.ci_body_scroll(), 2);
        app.ci_body = CiBody::Log;
        assert_eq!(app.ci_body_scroll(), 5, "kept per body");
        assert_eq!(app.inspector_scroll, 0, "the commit inspector's untouched");

        app.focused_panel = FocusedPanel::Detail;
        app.detail_nav_up();
        assert_eq!(app.ci_body_scroll(), 0, "reads as reset at once");
        app.detail_nav_down();
        assert_eq!(app.ci_body_scroll(), 0, "and stays reset");
        app.sync_ci_inspector(std::time::Instant::now());
        assert_eq!(app.ci_body_scrolls, [0, 0, 0]);
    }

    /// `j`/`k` in the focused `jobs` body move the mark the `log` body
    /// follows; in the other bodies they scroll.
    #[test]
    fn j_and_k_move_the_job_mark_in_the_jobs_body() {
        let mut app = on_failed_run();
        let mut second = jobs()[0].clone();
        second.id = 7;
        second.name = "docs".to_string();
        second.conclusion = Some("success".to_string());
        let run_id = runs()[FAILED_RUN].id;
        let held = app.ci.get_mut(&key()).unwrap();
        held.jobs.get_mut(&run_id).unwrap().jobs.insert(0, second);
        assert_eq!(app.ci_marked_job_index(), Some(1), "the first failed job");
        app.focused_panel = FocusedPanel::Inspector;
        app.inspector_nav(-1);
        assert_eq!(app.ci_marked_job().unwrap().name, "docs");
        app.inspector_nav(-1);
        assert_eq!(app.ci_marked_job_index(), Some(0), "clamped");
        app.inspector_nav(1);
        app.inspector_nav(1);
        assert_eq!(app.ci_marked_job_index(), Some(1), "clamped at the last");
        app.ci_body = CiBody::Log;
        app.inspector_max_scroll.set(100);
        app.inspector_nav(1);
        assert_eq!(app.ci_marked_job_index(), Some(1), "log scrolls instead");
        assert_eq!(app.ci_body_scroll(), 1);
    }

    /// The inspector's poll reads the highlighted run's jobs while it is
    /// open, and no run's once it is folded.
    #[test]
    fn the_highlighted_run_is_the_one_the_poll_expands() {
        let mut app = app();
        landed(&mut app, runs(), Local::now());
        app.set_detail_selected(1);
        app.sync_ci_inspector(std::time::Instant::now());
        assert_eq!(state(&app).expanded, Some(runs()[1].id));
        app.inspector_folded = true;
        app.sync_ci_inspector(std::time::Instant::now());
        assert_eq!(state(&app).expanded, None);
    }

    /// The fetch-once rules: nothing before the run has been highlighted
    /// for the dwell; then the shown body's fetch once; held or loading, not
    /// again; a failure again after a minute; a log the poll brought, never.
    #[test]
    fn each_body_fetches_once_per_job_or_commit_and_path() {
        use crate::ci::DetailWant;
        let mut app = on_failed_run();
        let now = Local::now();
        let t0 = std::time::Instant::now();
        let settled = t0 + crate::ci::DWELL;
        assert!(
            app.claim_ci_details(now, t0).is_empty(),
            "jobs and tail held"
        );

        // The poll brought the failed job's whole log: the body never asks.
        app.ci_body = CiBody::Log;
        assert!(app.claim_ci_details(now, settled).is_empty());

        // A run whose jobs no poll read: the jobs body asks once.
        app.ci_body = CiBody::Jobs;
        app.set_detail_selected(1);
        assert!(
            app.claim_ci_details(now, settled).is_empty(),
            "dwell restarts"
        );
        let later = settled + crate::ci::DWELL;
        let wants: Vec<DetailWant> = app
            .claim_ci_details(now, later)
            .into_iter()
            .map(|r| r.want)
            .collect();
        assert_eq!(
            wants,
            vec![DetailWant::Jobs {
                run_id: runs()[1].id
            }]
        );
        assert!(app.claim_ci_details(now, later).is_empty(), "loading");

        // The failed run's script: once per commit and path, the clone
        // named; a failure is asked again a minute on.
        app.set_detail_selected(FAILED_RUN);
        app.repos[0].path = Some("/tmp/fleetz".to_string());
        app.ci_body = CiBody::Script;
        let at = later + crate::ci::DWELL * 2;
        let requests = app.claim_ci_details(now, at);
        assert_eq!(requests.len(), 1, "{requests:#?}");
        let sha = runs()[FAILED_RUN].head_sha.clone().unwrap();
        assert_eq!(
            requests[0].want,
            DetailWant::Script {
                sha: sha.clone(),
                path: WORKFLOW.to_string(),
                clone: Some(std::path::PathBuf::from("/tmp/fleetz")),
            }
        );
        assert!(app.claim_ci_details(now, at).is_empty());
        app.apply_ci_detail(
            crate::ci::DetailAnswer {
                key: key(),
                got: crate::ci::DetailGot::Script {
                    sha: sha.clone(),
                    path: WORKFLOW.to_string(),
                    result: Err(crate::github::Failure::new(
                        crate::github::FailureKind::Offline,
                        "no route",
                    )),
                },
                budget: None,
            },
            now,
        );
        assert!(
            app.claim_ci_details(now + chrono::Duration::seconds(59), at)
                .is_empty()
        );
        assert_eq!(
            app.claim_ci_details(now + chrono::Duration::seconds(60), at)
                .len(),
            1,
            "again after a minute"
        );
    }

    /// A job's whole log the body is reading carries its tail, so the poll
    /// does not download the same log for the tail.
    #[test]
    fn a_log_the_body_reads_is_not_read_again_by_the_poll() {
        let mut app = app();
        landed(&mut app, runs(), Local::now());
        app.set_detail_selected(FAILED_RUN);
        let run_id = runs()[FAILED_RUN].id;
        app.ci.get_mut(&key()).unwrap().jobs.insert(
            run_id,
            crate::app::CiJobs {
                etag: None,
                jobs: jobs(),
            },
        );
        app.ci_body = CiBody::Log;
        let t0 = std::time::Instant::now();
        app.claim_ci_details(Local::now(), t0);
        let requests = app.claim_ci_details(Local::now(), t0 + crate::ci::DWELL);
        let want = &requests[0].want;
        let crate::ci::DetailWant::Log { job_id, step } = want else {
            panic!("a log request: {want:?}")
        };
        assert_eq!(*job_id, FAILED_JOB);
        assert!(step.is_some(), "the tail is owed, so the step rides along");
        app.sync_ci_inspector(t0);
        let poll = app.claim_ci_poll(key(), "gaurgle".into(), "fleetz".into());
        assert_eq!(poll.expanded.unwrap().logs_held, vec![FAILED_JOB]);
    }

    // --- The `info` section ---

    /// Four branches: `latest_per_branch` picks one run for each of the
    /// three newest.
    fn four_branch_runs() -> Vec<WorkflowRun> {
        let base = runs()[1].clone();
        ["main", "main", "feat/a", "feat/b", "feat/c"]
            .iter()
            .enumerate()
            .map(|(i, branch)| WorkflowRun {
                id: i as u64 + 1,
                title: format!("run {i} on {branch}"),
                branch: Some(branch.to_string()),
                ..base.clone()
            })
            .collect()
    }

    fn info_text(app: &App) -> Option<Vec<String>> {
        info_rows(app, &app.repos[0], 120, Local::now())
            .map(|lines| lines.iter().map(line_text).collect())
    }

    /// Held runs on four branches: the default branch's line, then the
    /// latest run of each of the three newest branches. Fails while `info`
    /// has no `ci` section.
    #[test]
    fn the_info_section_shows_the_default_branch_and_three_branches() {
        let mut app = app();
        app.repos[0].ci_status = CiStatus::Passed;
        landed(&mut app, four_branch_runs(), Local::now());
        let lines = info_text(&app).expect("a section");
        assert_eq!(lines.len(), 1 + INFO_RUNS, "{lines:#?}");
        assert_eq!(lines[0], "  ◉ default branch passed");
        for (line, title) in
            lines[1..]
                .iter()
                .zip(["run 0 on main", "run 2 on feat/a", "run 3 on feat/b"])
        {
            assert!(line.contains(title), "{title} missing: {line}");
        }
        assert!(lines.iter().all(|l| !l.contains('▸')), "nothing selectable");
    }

    /// No runs held (the repo was never visited in `ci`): the line and the
    /// dim hint. No GitHub identity: no section at all.
    #[test]
    fn the_info_section_hints_at_runs_and_needs_a_github_identity() {
        let mut app = app();
        app.history_view = HistoryView::Info;
        app.repos[0].ci_status = CiStatus::Failed;
        let lines = info_text(&app).expect("a section");
        assert_eq!(lines, vec!["  ◉ default branch failed", "  ] for runs"]);
        app.repos[0].ci_status = CiStatus::Unknown;
        assert_eq!(info_text(&app).unwrap()[0], "  no checks");

        app.repos[0].owner = None;
        assert_eq!(info_text(&app), None);
    }

    /// The section is drawn in `info` between `branches` and `recent
    /// commits`, ahead of `worktrees`, and drawing it polls nothing.
    #[test]
    fn info_draws_the_ci_section_and_polls_nothing() {
        let mut app = app();
        app.history_view = HistoryView::Info;
        app.repos[0].branches = vec![crate::model::BranchStatus {
            name: "main".to_string(),
            upstream: None,
            behind: 0,
            ahead: 0,
            is_head: true,
            tip: "abc1234".to_string(),
        }];
        app.repos[0].worktrees = vec![crate::model::Worktree {
            path: "~/repos/fleetz/.claude/worktrees/feat".to_string(),
            branch: "feature".to_string(),
            detached: false,
            head: "abc1234".to_string(),
            dirty: Some(0),
            head_at: None,
        }];
        app.repos[0].ci_status = CiStatus::Passed;
        assert!(!app.ci_wanted());
        let rows = detail_rows(&app);
        let at = |label: &str| {
            rows.iter()
                .position(|r| r.trim_matches(|c| c == '│' || c == ' ') == label)
        };
        let branches = at("branches").expect("branches label");
        let ci = at("ci").expect("ci label");
        assert!(branches < ci, "{rows:#?}");
        let worktrees = at("worktrees").expect("worktrees label");
        assert!(ci < worktrees, "ci precedes worktrees: {rows:#?}");
        assert!(rows[ci + 1].contains("default branch passed"));
        assert!(rows[ci + 2].contains(RUNS_HINT));
        assert!(app.ci.is_empty(), "nothing claimed");
    }
}
