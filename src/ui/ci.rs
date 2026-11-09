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

/// Column caps, so one long workflow, branch or trigger cannot take the
/// title's room. Longer names truncate with `…`.
const WORKFLOW_MAX: usize = 40;
const BRANCH_MAX: usize = 30;
const EVENT_MAX: usize = 18;

/// The narrowest title worth drawing. Below it the trigger goes first, to
/// give the title room, and then the title itself.
const TITLE_MIN: usize = 20;

/// How far the workflow name truncates before the branch gives way; past
/// both, the two shrink to nothing and duration and age stay.
const WORKFLOW_MIN: usize = 8;
const BRANCH_MIN: usize = 6;

/// The headline's words, one per column after the lamp's: workflow,
/// branch, trigger, title, duration, age.
const HEADINGS: [&str; 6] = ["workflow", "branch", "trigger", "title", "duration", "age"];

/// The `source` column's word (FZ-83), between `workflow` and `branch`
/// while the list holds a row from outside GitHub Actions.
const SOURCE_HEADING: &str = "source";

/// The headline's words in `cells`' order.
fn headings() -> [&'static str; 7] {
    let [workflow, branch, event, title, duration, age] = HEADINGS;
    [
        workflow,
        SOURCE_HEADING,
        branch,
        event,
        title,
        duration,
        age,
    ]
}

/// The `source` column's cap (`cloudflare`, `buildkite`).
const SOURCE_MAX: usize = 12;

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
    let rows = list_rows(app, now);
    let dim_pending = app.watch_mode && !app.blink_on;
    let widths = Widths::of_rows(&rows).headed();
    rows.iter()
        .enumerate()
        .map(|(i, row)| {
            let line = row_line(row, &widths, width, dim_pending).marked(i == selected, focused);
            // Below the row's floor (lamp, duration and age) the row is cut,
            // so the wrapping body never breaks it onto a second row.
            Line::from(crate::ui::detail::clip_spans(line.spans, width))
        })
        .collect()
}

/// The `ci` list's rows as text (`App::ci_entries`): the runs, and the
/// tip's other contexts among them (FZ-83), their branch the default
/// branch's name when known.
fn list_rows(app: &App, now: DateTime<Local>) -> Vec<RowText> {
    let now = now.with_timezone(&Utc);
    let branch = app
        .selected_repo()
        .and_then(|repo| app.ci_shown(repo).branch);
    app.ci_entries()
        .iter()
        .map(|entry| match entry {
            crate::app::CiEntry::Run(run) => RowText::of_run(run, now),
            crate::app::CiEntry::Context(context) => {
                RowText::of_context(context, branch.as_deref(), now)
            }
        })
        .collect()
}

/// The colour of a row's `source` (FZ-83): the `ci` tab's maroon for
/// GitHub Actions, a palette colour per known source, dim for the rest.
pub fn source_color(source: &str) -> ratatui::style::Color {
    match source {
        "actions" => colors::MAROON,
        "vercel" => colors::LAVENDER,
        "netlify" => colors::TEAL,
        "cloudflare" => colors::PR_CLR,
        "circleci" => colors::YELLOW,
        "buildkite" => colors::MAUVE,
        "codecov" => colors::HASH_CLR,
        _ => colors::OVERLAY,
    }
}

/// The headline over the run rows (FZ-40): each shown column's word, dim,
/// in the columns `run_rows` draws at the same `width`. `None` while no
/// runs are listed. Not a run, so not a cursor stop.
pub fn headline(app: &App, width: usize, now: DateTime<Local>) -> Option<Line<'static>> {
    let rows = list_rows(app, now);
    if rows.is_empty() {
        return None;
    }
    let columns = Widths::of_rows(&rows).headed().fit(width);
    let mut text = " ".repeat(MARKER_W + 2);
    text.extend(
        cells(&columns, headings())
            .into_iter()
            .map(|(_, cell)| cell),
    );
    let words = Span::styled(text, Style::default().fg(colors::OVERLAY));
    Some(Line::from(crate::ui::detail::clip_spans(
        vec![words],
        width,
    )))
}

/// The `info` view's `ci` section under its label, from what is in hand and
/// never polled for: the default branch's check state (`App::ci_shown`,
/// the pane 1 lamp's fact), then the latest run of each of the
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
/// header draw (`App::ci_shown`), dimmed as they are when the last poll did
/// not return the repo, or `no checks`. Held still, as the section's run
/// rows are: the list lamp and the header's already pulse for this fact,
/// and a third in step reads as a third thing happening.
fn default_branch_line(app: &App, repo: &RepoStatus) -> Line<'static> {
    let (glyph, color, label) = badges::ci_display(&app.ci_shown(repo).status, false);
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
    age_text(run.created, now)
}

/// How long ago `at` was, in the age column's words.
fn age_text(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let diff = now - at;
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

/// One row of the `ci` list as its cells' text (FZ-83): a workflow run, or
/// another context of the tip drawn the same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowText {
    status: CiStatus,
    workflow: String,
    source: String,
    branch: String,
    event: String,
    title: String,
    duration: String,
    age: String,
}

impl RowText {
    fn of_run(run: &WorkflowRun, now: DateTime<Utc>) -> Self {
        Self {
            status: run_status(run),
            workflow: workflow_text(run).to_string(),
            source: "actions".to_string(),
            branch: branch_text(run).to_string(),
            event: run.event.clone(),
            title: run.title.clone(),
            duration: run_duration(run, now),
            age: run_age(run, now),
        }
    }

    /// A context's row: its name, its source, the default branch's name
    /// (blank while unknown), its trigger, its title, how long its latest
    /// attempt took (to `now` while pending; `-` with no start known) and
    /// its age (`-` with no time known).
    fn of_context(
        context: &crate::ci::ContextRun,
        branch: Option<&str>,
        now: DateTime<Utc>,
    ) -> Self {
        let end = context.finished.unwrap_or(now);
        Self {
            status: context.state.clone(),
            workflow: context.name.clone(),
            source: context.source.clone(),
            branch: branch.unwrap_or_default().to_string(),
            event: context.trigger.to_string(),
            title: context.title.clone(),
            duration: context.started.map_or_else(
                || "-".to_string(),
                |start| duration_text((end - start).num_seconds()),
            ),
            age: context
                .at
                .map_or_else(|| "-".to_string(), |at| age_text(at, now)),
        }
    }
}

/// Column widths, sized as the repo list sizes its columns: to the widest
/// entry among all the runs held (not only the ones scrolled into view, so
/// the columns hold still while the view scrolls), capped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Widths {
    workflow: usize,
    source: usize,
    branch: usize,
    event: usize,
    duration: usize,
    age: usize,
}

impl Widths {
    pub fn of(runs: &[WorkflowRun], now: DateTime<Local>) -> Self {
        let now = now.with_timezone(&Utc);
        let rows: Vec<RowText> = runs.iter().map(|r| RowText::of_run(r, now)).collect();
        Self::of_rows(&rows)
    }

    fn of_rows(rows: &[RowText]) -> Self {
        let widest = |f: &dyn Fn(&RowText) -> &str, cap: usize| {
            rows.iter()
                .map(|r| f(r).chars().count())
                .max()
                .unwrap_or(0)
                .min(cap)
        };
        // A list of Actions runs alone says `actions` on every row: no column.
        let sources = rows.iter().any(|r| r.source != "actions");
        Self {
            workflow: widest(&|r| &r.workflow, WORKFLOW_MAX),
            source: if sources {
                widest(&|r| &r.source, SOURCE_MAX)
            } else {
                0
            },
            branch: widest(&|r| &r.branch, BRANCH_MAX),
            event: widest(&|r| &r.event, EVENT_MAX),
            duration: widest(&|r| &r.duration, usize::MAX),
            age: widest(&|r| &r.age, usize::MAX),
        }
    }

    /// Wide enough for the headline's words too, so a column of `ci`
    /// names still reads `workflow` over it.
    fn headed(self) -> Self {
        let [workflow, source, branch, event, _, duration, age] = headings().map(str::len);
        Self {
            workflow: self.workflow.max(workflow),
            source: if self.source > 0 {
                self.source.max(source)
            } else {
                0
            },
            branch: self.branch.max(branch),
            event: self.event.max(event),
            duration: self.duration.max(duration),
            age: self.age.max(age),
        }
    }

    /// The columns a row `width` wide shows (spec "Amendment 2026-10-22"):
    /// every column while the title keeps `TITLE_MIN`, the title taking the
    /// slack; then without the trigger; then without the source (FZ-83);
    /// then without the title; then the workflow name truncates to
    /// `WORKFLOW_MIN`, then the branch to `BRANCH_MIN`, then both to
    /// nothing. Duration and age always stay.
    fn fit(&self, width: usize) -> Columns {
        let gap = COLUMN_GAP.len();
        let core = |workflow: usize, branch: usize| {
            MARKER_W
                + 2
                + workflow
                + if branch > 0 { gap + branch } else { 0 }
                + gap
                + self.duration
                + gap
                + self.age
        };
        let mut columns = Columns {
            workflow: self.workflow,
            source: None,
            branch: self.branch,
            event: None,
            title: None,
            slack: 0,
            duration: self.duration,
            age: self.age,
        };
        let base = core(self.workflow, self.branch);
        let source = (self.source > 0).then_some(self.source);
        let with_source = base + source.map_or(0, |w| gap + w);
        let with_event = with_source + gap + self.event;
        if width >= with_event + gap + TITLE_MIN {
            columns.source = source;
            columns.event = Some(self.event);
            columns.title = Some(width - with_event - gap);
        } else if source.is_some() && width >= with_source + gap + TITLE_MIN {
            columns.source = source;
            columns.title = Some(width - with_source - gap);
        } else if width >= base + gap + TITLE_MIN {
            columns.title = Some(width - base - gap);
        } else {
            let mut over = base.saturating_sub(width);
            let mut give = |column: &mut usize, floor: usize| {
                let given = column.saturating_sub(floor).min(over);
                *column -= given;
                over -= given;
            };
            give(&mut columns.workflow, WORKFLOW_MIN);
            give(&mut columns.branch, BRANCH_MIN);
            give(&mut columns.workflow, 0);
            give(&mut columns.branch, 0);
            columns.slack = width.saturating_sub(core(columns.workflow, columns.branch));
        }
        columns
    }
}

/// What one row `width` wide shows, from `Widths::fit`: the run rows and the
/// headline both lay out through it, so the words sit over their columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Columns {
    workflow: usize,
    source: Option<usize>,
    /// Zero once the branch has shrunk away, its gap with it.
    branch: usize,
    event: Option<usize>,
    title: Option<usize>,
    /// Blank columns before the duration while there is no title to fill.
    slack: usize,
    duration: usize,
    age: usize,
}

/// Which column a cell is, for its style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    Workflow,
    Source,
    Branch,
    Event,
    Title,
    Duration,
    Age,
    Blank,
}

/// The cells of a row after the lamp, gaps included: `text` is the workflow,
/// source, branch, trigger, title, duration and age, each padded or
/// truncated with `…` to its column, the duration and the age right-aligned.
fn cells(columns: &Columns, text: [&str; 7]) -> Vec<(Cell, String)> {
    let [workflow, source, branch, event, title, duration, age] = text;
    let pad = |text: &str, w: usize| format!("{:<w$}", crate::ui::truncate(text, w));
    let gap = || (Cell::Blank, COLUMN_GAP.to_string());
    let mut out = vec![(Cell::Workflow, pad(workflow, columns.workflow))];
    if let Some(w) = columns.source {
        out.extend([gap(), (Cell::Source, pad(source, w))]);
    }
    if columns.branch > 0 {
        out.extend([gap(), (Cell::Branch, pad(branch, columns.branch))]);
    }
    if let Some(w) = columns.event {
        out.extend([gap(), (Cell::Event, pad(event, w))]);
    }
    match columns.title {
        Some(w) => out.extend([gap(), (Cell::Title, pad(title, w))]),
        None => out.push((Cell::Blank, " ".repeat(columns.slack))),
    }
    out.extend([
        gap(),
        (
            Cell::Duration,
            format!("{duration:>w$}", w = columns.duration),
        ),
        gap(),
        (Cell::Age, format!("{age:>w$}", w = columns.age)),
    ]);
    out
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

/// The cells of one run row (spec section E): lamp, workflow, source,
/// branch, event, title, duration, age, as many as `width` holds
/// (`Widths::fit`). The title takes whatever `width` leaves.
pub fn run_line(
    run: &WorkflowRun,
    widths: &Widths,
    width: usize,
    now: DateTime<Local>,
    dim_pending: bool,
) -> RunLine {
    let row = RowText::of_run(run, now.with_timezone(&Utc));
    row_line(&row, widths, width, dim_pending)
}

/// `run_line` for any row of the list, a context's too (FZ-83).
fn row_line(row: &RowText, widths: &Widths, width: usize, dim_pending: bool) -> RunLine {
    let (glyph, color, _) = badges::ci_display(&row.status, dim_pending);
    // `ci_display` draws no lamp for a repo without CI; a run always has
    // one, so a run with no verdict gets the same ring in that grey.
    let glyph = if glyph.is_empty() { "◉" } else { glyph };
    let dim = Style::default().fg(colors::OVERLAY);
    let text = [
        row.workflow.as_str(),
        row.source.as_str(),
        row.branch.as_str(),
        row.event.as_str(),
        row.title.as_str(),
        row.duration.as_str(),
        row.age.as_str(),
    ];
    let mut spans = vec![Span::styled(
        format!("{glyph} "),
        Style::default().fg(color),
    )];
    spans.extend(
        cells(&widths.fit(width), text)
            .into_iter()
            .map(|(cell, text)| match cell {
                Cell::Workflow => Span::styled(
                    text,
                    Style::default()
                        .fg(colors::TEXT)
                        .add_modifier(Modifier::BOLD),
                ),
                Cell::Source => Span::styled(text, Style::default().fg(source_color(&row.source))),
                Cell::Branch => Span::styled(text, Style::default().fg(colors::SAPPHIRE)),
                Cell::Title => Span::styled(text, Style::default().fg(colors::TEXT)),
                Cell::Event | Cell::Duration | Cell::Age => Span::styled(text, dim),
                Cell::Blank => Span::raw(text),
            }),
    );
    RunLine(spans, width)
}

// --- The inspector's run bodies (FZ-30f) ---

/// `inspector_lines` without the marked job's lines, as the tests read it.
#[cfg(test)]
pub fn inspector_body(app: &App, now: DateTime<Local>) -> Vec<Line<'static>> {
    inspector_lines(app, now).0
}

/// The inspector's lines for the highlighted run in the shown body
/// (`App::ci_body`), unwrapped: the pane wraps and scrolls them. A held
/// whole log is not among them; `shown_log` hands it over apart. In the
/// `jobs` body, also the lines the marked job takes (its row, and its steps
/// and tail beneath), which the pane keeps in view as the job cursor moves.
pub fn inspector_lines(
    app: &App,
    now: DateTime<Local>,
) -> (Vec<Line<'static>>, Option<std::ops::Range<usize>>) {
    if let Some(context) = app.highlighted_ci_context() {
        let text = format!("no jobs or log for {}", context.source);
        return (vec![dim_line(&text)], None);
    }
    let Some(run) = app.ci_highlighted_run() else {
        return (vec![dim_line("no run selected")], None);
    };
    let mut lines = run_masthead(run);
    let head = lines.len();
    let marked = match app.ci_body {
        CiBody::Jobs => {
            let (body, marked) = jobs_body(app, run, now);
            lines.extend(body);
            marked.map(|m| m.start + head..m.end + head)
        }
        CiBody::Log => {
            lines.extend(log_body(app, run));
            None
        }
        CiBody::Script => {
            lines.extend(script_body(app, run));
            None
        }
    };
    (lines, marked)
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

/// The `jobs` body: a list, one row per job with its lamp, name and
/// duration, the marked one carrying `▸`. Only the marked job shows its
/// steps beneath, one per line, and when it failed the dimmed tail of its
/// log under the failing step, once the log is in hand: the list never
/// waits for it.
/// Also returns the lines the marked job takes.
fn jobs_body(
    app: &App,
    run: &WorkflowRun,
    now: DateTime<Local>,
) -> (Vec<Line<'static>>, Option<std::ops::Range<usize>>) {
    let state = app.selected_ci().and_then(|(_, state)| state);
    let Some(jobs) = app.ci_highlighted_jobs() else {
        return (vec![jobs_missing(state, run)], None);
    };
    if jobs.is_empty() {
        return (vec![dim_line("no jobs")], None);
    }
    let marked = app.ci_marked_job_index();
    let mut lines = Vec::new();
    let mut marked_lines = None;
    for (i, job) in jobs.iter().enumerate() {
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
        if marked != Some(i) {
            continue;
        }
        let start = lines.len() - 1;
        let failing = (status == CiStatus::Failed)
            .then(|| crate::ci::failing_step(job))
            .flatten();
        for step in &job.steps {
            let (glyph, color) = step_glyph(step);
            lines.push(Line::from(vec![
                Span::styled(format!("    {glyph} "), Style::default().fg(color)),
                Span::styled(step.name.clone(), Style::default().fg(colors::SUBTEXT)),
            ]));
            if failing == Some(step) {
                lines.extend(failed_tail(state, job));
            }
        }
        marked_lines = Some(start..lines.len());
    }
    (lines, marked_lines)
}

/// A failed job's tail as the jobs body shows it: the dimmed lines once
/// read, why there are none, or nothing while the log is still to come.
fn failed_tail(state: Option<&CiState>, job: &Job) -> Vec<Line<'static>> {
    let Some(state) = state else {
        return Vec::new();
    };
    match (state.logs.get(&job.id), state.full_logs.get(&job.id)) {
        (Some(LogTail::Lines(tail)), _) => tail
            .iter()
            .map(|l| dim_line(&format!("      {l}")))
            .collect(),
        (Some(LogTail::Unavailable(message)), _) | (None, Some(Detail::Unavailable(message))) => {
            vec![dim_line(&format!("      {message}"))]
        }
        (None, Some(Detail::Failed { message, .. })) => vec![warn_line(message)],
        (None, _) => Vec::new(),
    }
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
mod tests;
