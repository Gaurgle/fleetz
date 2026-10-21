//! The CI pane (pane 4, FZ-30c): the selected repo's recent workflow runs,
//! one row each, in a band docked across the full width above the
//! contribution strip. See `docs/superpowers/specs/2026-10-21-ci-pane-design.md`,
//! sections A, D and E. The data is `ci.rs`'s; jobs, steps and the log tail
//! are FZ-30d.

use chrono::{DateTime, Local, Utc};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};

use crate::app::{App, CiState, DataTime, FocusedPanel, Outcome};
use crate::ci::WorkflowRun;
use crate::model::CiStatus;
use crate::ui::{CI_NUM, FOLDED_HEIGHT, badges, colors, contributions};

/// Rows the open band takes: its frame and eight lines of runs.
pub const CI_HEIGHT: u16 = 10;

/// Run rows the open band shows at once.
pub const RUN_ROWS: usize = (CI_HEIGHT - 2) as usize;

/// The pane's label, after its `[4]` tag.
const LABEL: &str = "ci";

/// Column caps, so one long workflow or branch name cannot take the title's
/// room. Longer names truncate with `…`.
const WORKFLOW_MAX: usize = 16;
const BRANCH_MAX: usize = 24;
const EVENT_MAX: usize = 17;

/// Blank columns between two run columns.
const COLUMN_GAP: &str = "  ";

/// Columns the selection marker holds open on every row (`▸ ` or blank).
const MARKER_W: usize = 2;

/// Dock both bands off the bottom of `main`: the strip at the very bottom,
/// the CI band above it, the panes above both. Returns `(body, ci, strip)`.
///
/// The single source of truth for that geometry: `ui::panel_rects` reports it
/// and `ui::render` draws from `panel_rects`, so a click lands where the band
/// is drawn. The strip is carved by `split_main_area` and the band from what
/// is left with the same carve (`contributions::dock_bottom`).
///
/// When the height runs out, in this order: an open band drops to its folded
/// row; then the strip gives way, as it always has; the band's folded row
/// stays as long as the panes above keep `MAIN_MIN_HEIGHT`. The band never
/// disappears while the strip stays.
pub fn split_docks(
    main: Rect,
    ci_folded: bool,
    strip_folded: bool,
) -> (Rect, Option<Rect>, Option<Rect>) {
    let (rest, strip) = contributions::split_main_area(main, strip_folded);
    if strip.is_some() {
        let heights = if ci_folded {
            &[FOLDED_HEIGHT][..]
        } else {
            &[CI_HEIGHT, FOLDED_HEIGHT][..]
        };
        for &height in heights {
            if let (body, Some(ci)) = contributions::dock_bottom(rest, height) {
                return (body, Some(ci), strip);
            }
        }
    }
    // No room for the strip beside the band: the strip gives way, and the
    // band keeps its folded row.
    let (body, ci) = contributions::dock_bottom(main, FOLDED_HEIGHT);
    (body, ci, None)
}

/// Whether a band `split_docks` laid out is drawn folded: its title row
/// alone, whether `4` folded it or a short terminal did.
pub fn is_folded(band: Rect) -> bool {
    band.height <= FOLDED_HEIGHT
}

/// The run row under (`col`, `row`) in the open band `area`, an index into
/// the selected repo's runs, or `None` off the rows. Reads the same scroll
/// `render` draws with.
pub fn run_at(area: Rect, app: &App, col: u16, row: u16) -> Option<usize> {
    if is_folded(area) {
        return None;
    }
    let inner = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    if !inner.contains(ratatui::layout::Position { x: col, y: row }) {
        return None;
    }
    let (_, state) = app.selected_ci()?;
    let state = state?;
    let index = first_row(state) + (row - inner.y) as usize;
    (index < state.runs.len()).then_some(index)
}

/// The first run drawn: the stored scroll, clamped so the last run sits on
/// the bottom row once the list is shorter than it was.
fn first_row(state: &CiState) -> usize {
    state.scroll.min(state.runs.len().saturating_sub(RUN_ROWS))
}

/// Draw the band: folded, its title row; open, a frame titled `[4] ci` and
/// the freshness mark, one row per run, and the status line in the bottom
/// border.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let now = Local::now();
    let focused = app.focused_panel == FocusedPanel::Ci;
    let state = app.selected_ci().and_then(|(_, state)| state);
    let mark = freshness_mark(state, now);
    if is_folded(area) {
        crate::ui::render_folded(frame, area, CI_NUM, LABEL, focused);
        if let Some(mark) = mark {
            let title = crate::ui::panel_title(CI_NUM, LABEL, focused).width() as u16;
            let x = area.x + 1 + title;
            let rest = Rect::new(x, area.y, area.width.saturating_sub(1 + title), 1);
            frame.render_widget(
                Line::from(vec![mark, Span::raw(" ")]),
                rest.intersection(area),
            );
        }
        return;
    }
    let border_color = if focused {
        colors::LAVENDER
    } else {
        colors::SURFACE
    };
    let mut title = crate::ui::panel_title(CI_NUM, LABEL, focused);
    if let Some(mark) = mark {
        title.spans.push(mark);
        title.spans.push(Span::raw(" "));
    }
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::horizontal(1))
        .title_top(title);
    if let Some(status) = status_line(app, now) {
        block = block.title_bottom(Line::from(status));
    }
    let inner = block.inner(area);
    let lines = match state {
        Some(state) => {
            let dim_pending = app.watch_mode && !app.blink_on;
            let first = first_row(state);
            let shown = &state.runs[first..state.runs.len().min(first + RUN_ROWS)];
            let widths = Widths::of(&state.runs, now);
            shown
                .iter()
                .enumerate()
                .map(|(i, run)| {
                    let selected = first + i == state.cursor;
                    run_line(run, &widths, inner.width as usize, now, dim_pending)
                        .marked(selected, focused)
                })
                .collect()
        }
        None => Vec::new(),
    };
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// The title's freshness mark (spec section D): `as of 12:02` once an
/// attempt succeeded, dated by the data on screen; `⚠ 12:05` while the last
/// one failed, dated by that failed attempt (when fleetz last tried), as the
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

/// The pane's status line, in its bottom border: why there are no runs to
/// show, or how the last attempt failed. `None` when the runs speak for
/// themselves.
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
/// the columns hold still while the band scrolls), capped.
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
    use crate::app::View;
    use crate::model::RepoStatus;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

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

    /// An app on one GitHub repo, the CI pane open, nothing polled.
    fn app() -> App {
        let mut app = App::new(Vec::new(), String::new(), 120, 44);
        app.repos = vec![repo()];
        app.view = View::Detail;
        app.ci_folded = false;
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

    fn status_text(app: &App) -> Option<String> {
        status_line(app, Local::now())
            .map(|spans| spans.iter().map(|s| s.content.as_ref()).collect())
    }

    // --- Geometry: the one split, reported where it is drawn ---

    #[test]
    fn the_band_docks_above_the_strip_with_one_gap_row_each() {
        let main = Rect::new(0, 1, 120, 41);
        let (body, ci, strip) = split_docks(main, false, false);
        let (ci, strip) = (ci.expect("band laid out"), strip.expect("strip laid out"));
        assert_eq!(ci.height, CI_HEIGHT);
        assert_eq!(strip.height, contributions::CONTRIB_HEIGHT);
        assert_eq!(
            strip.y + strip.height,
            main.y + main.height,
            "strip at the bottom"
        );
        assert_eq!(
            ci.y + ci.height + 1,
            strip.y,
            "band directly above the strip"
        );
        assert_eq!(
            body.y + body.height + 1,
            ci.y,
            "panes directly above the band"
        );
        assert_eq!((ci.x, ci.width), (main.x, main.width), "full width");

        let (_, folded, _) = split_docks(main, true, false);
        assert_eq!(folded.expect("folded band laid out").height, FOLDED_HEIGHT);
    }

    /// On a short terminal the band folds to its row before anything else
    /// gives way, and the panes above keep their floor.
    #[test]
    fn a_short_area_folds_the_band_before_the_strip_goes() {
        let open_both = 10 + CI_HEIGHT + 1 + contributions::CONTRIB_HEIGHT + 1;
        let at = |h| split_docks(Rect::new(0, 0, 120, h), false, false);
        let (_, ci, strip) = at(open_both);
        assert_eq!(ci.unwrap().height, CI_HEIGHT);
        assert!(strip.is_some());
        let (body, ci, strip) = at(open_both - 1);
        assert_eq!(ci.unwrap().height, FOLDED_HEIGHT, "folded first");
        assert_eq!(
            strip.unwrap().height,
            contributions::CONTRIB_HEIGHT,
            "strip kept"
        );
        assert!(body.height >= 10);
    }

    /// Spec A's order when the height runs out, at every height from 15 to
    /// 30 and every fold state: the open band folds first, the strip gives
    /// way next, and the band's folded row stays while the panes keep
    /// `MAIN_MIN_HEIGHT`. The band never disappears while the strip stays.
    #[test]
    fn the_band_keeps_its_row_longer_than_the_strip_at_every_short_height() {
        const MAIN_MIN: u16 = 10;
        // The blank row above each docked band.
        const GAP: u16 = 1;
        let row = FOLDED_HEIGHT + GAP;
        for ci_folded in [false, true] {
            for strip_folded in [false, true] {
                let strip_h = if strip_folded {
                    FOLDED_HEIGHT
                } else {
                    contributions::CONTRIB_HEIGHT
                };
                for h in 15u16..=30 {
                    let case = format!("h {h}, ci folded {ci_folded}, strip folded {strip_folded}");
                    let main = Rect::new(0, 0, 120, h);
                    let (body, ci, strip) = split_docks(main, ci_folded, strip_folded);
                    let ci = ci.unwrap_or_else(|| panic!("{case}: the band's row is gone"));
                    assert!(body.height >= MAIN_MIN, "{case}: body {}", body.height);
                    let strip_fits = h >= MAIN_MIN + row + strip_h + GAP;
                    assert_eq!(strip.is_some(), strip_fits, "{case}: strip");
                    let open_fits = !ci_folded && h >= MAIN_MIN + CI_HEIGHT + GAP + strip_h + GAP;
                    let expected = if open_fits { CI_HEIGHT } else { FOLDED_HEIGHT };
                    assert_eq!(ci.height, expected, "{case}: band height");
                    assert_eq!(body.y + body.height + 1, ci.y, "{case}: gap above the band");
                    match strip {
                        Some(strip) => {
                            assert_eq!(ci.y + ci.height + 1, strip.y, "{case}: band above strip");
                            assert_eq!(strip.y + strip.height, h, "{case}: strip at the bottom");
                        }
                        None => assert_eq!(ci.y + ci.height, h, "{case}: band at the bottom"),
                    }
                }
            }
        }
    }

    /// `panel_rects` reports the band exactly where `render` draws it, open,
    /// folded and in Compact, with the strip below it.
    #[test]
    fn panel_rects_reports_the_band_where_render_draws_it() {
        for view in [View::Detail, View::Compact] {
            for folded in [false, true] {
                let mut app = app();
                app.view = view;
                app.ci_folded = folded;
                let (rects, buffer) = draw(&app, 120, 44);
                let band = rects
                    .ci
                    .unwrap_or_else(|| panic!("{view:?} lays out the band"));
                let strip = rects.contributions.expect("strip laid out");
                assert_eq!(band.y + band.height + 1, strip.y, "{view:?}: strip below");
                assert_eq!(rects.ci_folded, folded);
                let x = (band.x..band.x + band.width - 2)
                    .find(|&x| {
                        (0..3).all(|d| {
                            buffer[(x + d, band.y)].symbol() == &"[4]"[d as usize..=d as usize]
                        })
                    })
                    .unwrap_or_else(|| panic!("{view:?}: [4] drawn on the band's top row"));
                assert_eq!(rects.fold_tag_at(x + 1, band.y), Some(FocusedPanel::Ci));
                assert_eq!(rects.at(band.x + 3, band.y), Some(FocusedPanel::Ci));
                if !folded {
                    let corner = buffer[(band.x, band.y + band.height - 1)].symbol();
                    assert_eq!(
                        corner, "╰",
                        "{view:?}: the frame ends on the band's last row"
                    );
                    assert_eq!(rects.at(band.x + 3, band.y + 4), Some(FocusedPanel::Ci));
                }
                let above = band.y - 1;
                assert_ne!(rects.at(band.x + 3, above), Some(FocusedPanel::Ci));
            }
        }
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
    fn the_band_draws_ci_with_its_tag_and_mark() {
        let mut app = app();
        landed(&mut app, runs(), Local::now());
        let (rects, buffer) = draw(&app, 120, 44);
        let top = row_text(&buffer, rects.ci.unwrap().y);
        assert!(top.contains("[4] ci as of"), "{top}");
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

    // --- Cursor, scroll and mouse ---

    fn many_runs(n: usize) -> Vec<WorkflowRun> {
        let base = runs()[1].clone();
        (0..n)
            .map(|i| WorkflowRun {
                id: i as u64 + 1,
                title: format!("run {i}"),
                ..base.clone()
            })
            .collect()
    }

    #[test]
    fn j_and_k_move_the_cursor_and_the_band_scrolls_to_keep_it() {
        let mut app = app();
        landed(&mut app, many_runs(12), Local::now());
        app.move_ci_cursor(-1);
        assert_eq!(app.ci[&key()].cursor, 0, "clamped at the top");
        for _ in 0..RUN_ROWS {
            app.move_ci_cursor(1);
        }
        let state = &app.ci[&key()];
        assert_eq!((state.cursor, state.scroll), (RUN_ROWS, 1), "scrolled one");
        for _ in 0..20 {
            app.move_ci_cursor(1);
        }
        let state = &app.ci[&key()];
        assert_eq!(
            (state.cursor, state.scroll),
            (11, 12 - RUN_ROWS),
            "clamped at the end"
        );
        for _ in 0..11 {
            app.move_ci_cursor(-1);
        }
        let state = &app.ci[&key()];
        assert_eq!((state.cursor, state.scroll), (0, 0));
    }

    #[test]
    fn the_cursor_row_is_the_one_drawn_marked() {
        let mut app = app();
        landed(&mut app, many_runs(12), Local::now());
        app.focused_panel = FocusedPanel::Ci;
        for _ in 0..9 {
            app.move_ci_cursor(1);
        }
        let (rects, buffer) = draw(&app, 120, 44);
        let band = rects.ci.unwrap();
        let marked: Vec<u16> = (band.y + 1..band.y + band.height - 1)
            .filter(|&y| row_text(&buffer, y).contains('▸'))
            .collect();
        assert_eq!(marked.len(), 1);
        assert!(row_text(&buffer, marked[0]).contains("run 9"));
        assert_eq!(run_at(band, &app, band.x + 4, marked[0]), Some(9));
    }

    #[test]
    fn a_click_selects_the_row_and_the_wheel_scrolls_the_view() {
        let mut app = app();
        landed(&mut app, many_runs(12), Local::now());
        let (rects, _) = draw(&app, 120, 44);
        let band = rects.ci.unwrap();
        let third = band.y + 3;
        let index = run_at(band, &app, band.x + 4, third).unwrap();
        assert_eq!(index, 2);
        app.select_ci_run(index);
        assert_eq!(app.ci[&key()].cursor, 2);
        assert_eq!(
            run_at(band, &app, band.x + 4, band.y),
            None,
            "the title row"
        );

        app.scroll_ci(3);
        assert_eq!(app.ci[&key()].scroll, 3);
        assert_eq!(app.ci[&key()].cursor, 2, "the wheel leaves the cursor");
        app.scroll_ci(50);
        assert_eq!(
            app.ci[&key()].scroll,
            12 - RUN_ROWS,
            "the last run on the bottom row"
        );
        assert_eq!(
            run_at(band, &app, band.x + 4, band.y + 1),
            Some(12 - RUN_ROWS)
        );
    }

    #[test]
    fn unfolding_puts_the_cursor_on_the_first_run() {
        let mut app = app();
        landed(&mut app, many_runs(12), Local::now());
        for _ in 0..10 {
            app.move_ci_cursor(1);
        }
        app.focused_panel = FocusedPanel::Ci;
        app.press_panel_key(FocusedPanel::Ci);
        assert!(app.ci_folded, "the focused pane's number folds it");
        app.press_panel_key(FocusedPanel::Ci);
        assert!(!app.ci_folded);
        let state = &app.ci[&key()];
        assert_eq!((state.cursor, state.scroll), (0, 0));

        for _ in 0..10 {
            app.move_ci_cursor(1);
        }
        app.toggle_fold(FocusedPanel::Ci);
        app.toggle_fold(FocusedPanel::Ci);
        assert_eq!(app.ci[&key()].cursor, 0, "the tag click too");
    }
}
