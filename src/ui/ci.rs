//! The `ci` view of pane 2 (FZ-30d): the selected repo's recent workflow
//! runs, one row each, under a first line that says how current they are or
//! why there are none; and the small `ci` section of the `info` view, drawn
//! from what is already in hand. See
//! `docs/superpowers/specs/2026-10-21-ci-pane-design.md`, sections D and E
//! and "Amendment 2026-10-21". The data is `ci.rs`'s; the inspector's run
//! bodies are FZ-30f.

use chrono::{DateTime, Local, Utc};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::{App, CiState, DataTime, Outcome};
use crate::ci::WorkflowRun;
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
    if run.status.as_deref() != Some("completed") {
        return CiStatus::Pending;
    }
    match run.conclusion.as_deref() {
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
    let secs = (end - start).num_seconds().max(0);
    match secs {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{FocusedPanel, HistoryView, View};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    const RUNS: &str = include_str!("../ci/fixtures/runs.json");

    fn runs() -> Vec<WorkflowRun> {
        crate::ci::parse_runs(RUNS).unwrap()
    }

    fn local(text: &str) -> DateTime<Local> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Local)
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
        let at = |t: &str| local(t).with_timezone(&Utc);
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
        let at = |t: &str| local(t).with_timezone(&Utc);
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
    /// commits`, and drawing it polls nothing.
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
        assert!(rows[ci + 1].contains("default branch passed"));
        assert!(rows[ci + 2].contains(RUNS_HINT));
        assert!(app.ci.is_empty(), "nothing claimed");
    }
}
