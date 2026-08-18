//! The contribution strip: a year of activity docked under the main panes.
//!
//! The grid draws at a fixed square size and scrolls, rather than shrinking
//! to fit. A year at this size is wider than any terminal, so the newest
//! weeks are pinned to the right edge and older ones hide off to the left,
//! which is the direction time already runs in.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};

use crate::app::{App, ContributionsMode, FocusedPanel};
use crate::contributions::{Calendar, WEEKS};
use crate::ui::colors;

/// A day cell, two columns wide. A terminal cell is about twice as tall as
/// it is wide, so two of them side by side read as a square; one reads as a
/// thin dot.
const CELL: &str = "\u{2588}\u{2588}";
const CELL_COLS: u16 = 2;

/// Blank columns between two day cells, and blank rows between two day rows.
///
/// One row of vertical gap is not optional: the selection frame is drawn in
/// the rows above and below the marked cell, so removing it would leave
/// nowhere to put the frame. The horizontal gap is one column, which reads
/// tighter than the row gap but keeps the grid compact enough to be worth
/// looking at.
const GAP_COLS: u16 = 1;
const GAP_ROWS: u16 = 1;

/// Columns one week column occupies, cell plus its trailing gap. The gap is
/// always emitted, including after the last column, so the selection frame
/// has somewhere to draw its right edge.
const WEEK_COLS: u16 = CELL_COLS + GAP_COLS;
/// Rows one weekday row occupies, cell plus gap.
const DAY_ROWS: u16 = 1 + GAP_ROWS;

/// Columns reserved for the Mon/Wed/Fri gutter.
const GUTTER: u16 = 4;

/// Columns the frame eats on each side: the border plus the block's own
/// one-column padding.
const FRAME_COLS: u16 = 2;

/// First of the day rows, counted from the strip's outer top edge: row 0 is
/// the border, row 1 the month labels, row 2 the frame row above the first
/// day. The count and the source ride in the border title, and the legend in
/// the bottom border, as the other panes put their headers.
const FIRST_DAY_ROW: u16 = 3;

/// Grid rows: a frame row, then each weekday followed by another frame row.
const GRID_ROWS: u16 = 7 * DAY_ROWS + 1;

/// Rows the strip occupies: two border rows, the month labels, and the grid.
/// The legend rides in the bottom border rather than costing a row of its own.
pub const CONTRIB_HEIGHT: u16 = 2 + 1 + GRID_ROWS;

/// Rows the panes above must keep for the strip to be worth docking. Below
/// this the strip is dropped: a dashboard squeezed into slivers is worse
/// than no heatmap.
const MAIN_MIN_HEIGHT: u16 = 10;

/// Carve the strip off the bottom of the main area.
///
/// Single source of truth for that geometry, in the same sense as
/// `detail::split_main` and `detail::split_right_column`: `ui::render` draws
/// from it and `ui::panel_rects` hit-tests against it. It lives here rather
/// than in `detail.rs` because the strip exists in Compact view too.
pub fn split_main_area(main: Rect, open: bool) -> (Rect, Option<Rect>) {
    if !open || main.height < MAIN_MIN_HEIGHT + CONTRIB_HEIGHT + 1 {
        return (main, None);
    }
    let v = Layout::vertical([Constraint::Min(1), Constraint::Length(CONTRIB_HEIGHT)])
        .spacing(1)
        .split(main);
    (v[0], Some(v[1]))
}

/// How many week columns fit inside a strip of this outer width.
///
/// Both the renderer and the click hit-test go through here, so they cannot
/// disagree about which day a cell is: the same failure `panel_rects` exists
/// to prevent, one level down.
pub fn visible_weeks(area: Rect) -> usize {
    let grid = area
        .width
        .saturating_sub(FRAME_COLS * 2)
        .saturating_sub(GUTTER);
    (grid / WEEK_COLS).min(WEEKS as u16) as usize
}

/// The leftmost week to draw: whatever the app asked for, clamped so the
/// grid can never scroll past its own ends. An out-of-range request pins the
/// newest week to the right edge, which is where the grid opens.
fn first_week(app: &App, visible: usize) -> usize {
    let max = WEEKS.saturating_sub(visible);
    app.contrib_scroll.get().min(max)
}

/// Group a count with commas, the way the GitHub header does.
pub fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The strip's title. With a day marked it reads that day out; otherwise it
/// reports the year's total and which source filled it, because the two
/// sources legitimately disagree.
///
/// Shortens to fit `width` rather than letting the renderer clip it. The
/// source label is the last thing dropped: a strip that does not say where
/// its numbers came from is the one failure this line exists to prevent.
pub fn title(calendar: &Calendar, cursor: Option<usize>, width: u16) -> String {
    let width = width as usize;
    let source = calendar.source().label();
    // "1 contributions" is the kind of thing that makes a dashboard look
    // unfinished, and a one-commit day is common enough to hit often.
    let noun = |n: u32| {
        if n == 1 {
            "contribution"
        } else {
            "contributions"
        }
    };

    if let Some(slot) = cursor
        && let Some((date, count)) = calendar.day_at(slot / 7, slot % 7)
    {
        let when = date.format("%a %-d %b");
        let full = if count == 0 {
            format!("no contributions on {when}")
        } else {
            format!("{} {} on {when}", thousands(count), noun(count))
        };
        if full.chars().count() <= width {
            return full;
        }
        return format!("{} on {when}", thousands(count));
    }

    let total = thousands(calendar.total());
    for candidate in [
        format!(
            "{total} {} in the last year ({source})",
            noun(calendar.total())
        ),
        format!("{total} in the last year ({source})"),
        format!("{total} ({source})"),
    ] {
        if candidate.chars().count() <= width {
            return candidate;
        }
    }
    total
}

/// The grid slot under a screen cell, or None for the gutter, the gaps, the
/// label rows, and anything outside the drawn window.
pub fn slot_at(area: Rect, scroll: usize, col: u16, row: u16) -> Option<usize> {
    let visible = visible_weeks(area);
    let down = row.checked_sub(area.y + FIRST_DAY_ROW)?;
    // Odd rows are the blank gap between two day rows.
    if !down.is_multiple_of(DAY_ROWS) {
        return None;
    }
    let weekday = (down / DAY_ROWS) as usize;
    if weekday >= 7 {
        return None;
    }
    let across = col.checked_sub(area.x + FRAME_COLS + GUTTER)?;
    // The trailing column of each step is the gap, not the cell.
    if across % WEEK_COLS >= CELL_COLS {
        return None;
    }
    let column = (across / WEEK_COLS) as usize;
    if column >= visible {
        return None;
    }
    let week = scroll.min(WEEKS.saturating_sub(visible)) + column;
    Some(week * 7 + weekday)
}

/// Draw the strip: a bordered pane whose title carries the count and the
/// source, over the month labels, the seven day rows, and the legend.
///
/// Framed like the other three panes rather than drawn bare, because it is
/// focusable the same way they are: without a border there is nothing on
/// screen to say `4` reached it or that it currently holds the keys.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let today = chrono::Local::now().date_naive();
    let focused = app.focused_panel == FocusedPanel::Contributions;
    let border_color = if focused {
        colors::LAVENDER
    } else {
        colors::SURFACE
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::horizontal(1));

    let Some(calendar) = calendar_for(app, today) else {
        // No poll has landed yet. The frame still draws, so the pane does not
        // blink into existence when the first fetch completes.
        frame.render_widget(
            block.title_top(crate::ui::panel_title(4, "contributions", focused)),
            area,
        );
        return;
    };

    // Report the measured window back, so the scroll keys and the cursor
    // have something real to clamp against. This is the render pass telling
    // the input layer how wide the view turned out, not state it owns.
    let visible = visible_weeks(area);
    app.contrib_visible_weeks.set(visible.max(1));
    let first = first_week(app, visible);
    app.contrib_scroll.set(first);

    let cursor = app.contrib_cursor.map(|c| c.min(calendar.today_slot()));
    let inner_width = area.width.saturating_sub(FRAME_COLS * 2);

    let mut lines = Vec::with_capacity(CONTRIB_HEIGHT as usize);
    lines.push(month_line(&calendar, first, visible));
    lines.extend(grid_rows(&calendar, first, visible, cursor));

    let heading = title(&calendar, cursor, inner_width);
    frame.render_widget(
        Paragraph::new(lines).block(
            block
                .title_top(crate::ui::panel_title(4, "contributions", focused))
                .title_top(
                    Line::from(Span::styled(
                        format!(" {heading} "),
                        Style::default().fg(if focused {
                            colors::TEXT
                        } else {
                            colors::OVERLAY
                        }),
                    ))
                    .right_aligned(),
                )
                // The legend rides in the bottom border rather than costing a
                // row of its own, which is what the other panes do with their
                // secondary information.
                .title_bottom(legend_line(calendar.source()).right_aligned()),
        ),
        area,
    );
}

/// Which calendar the strip is showing. Borrowed for the GitHub source so a
/// 1.5KB clone does not happen every frame; owned for the local one, which is
/// summed from the visible repos on demand.
fn calendar_for(app: &App, today: chrono::NaiveDate) -> Option<std::borrow::Cow<'_, Calendar>> {
    use std::borrow::Cow;
    match app.contributions {
        ContributionsMode::Off => None,
        ContributionsMode::Github => app.github_calendar.as_deref().map(Cow::Borrowed),
        ContributionsMode::Local => Some(Cow::Owned(app.local_calendar(today))),
    }
}

/// Month labels, one per month that opens inside the visible window, each
/// sitting over the column its month starts in. A label that would collide
/// with the one before it is dropped rather than drawn on top of it.
fn month_line(calendar: &Calendar, first: usize, visible: usize) -> Line<'static> {
    let mut row = " ".repeat(GUTTER as usize);
    for (week, name) in calendar.month_labels() {
        if week < first || week >= first + visible {
            continue;
        }
        let at = GUTTER as usize + (week - first) * WEEK_COLS as usize;
        let used = row.chars().count();
        // One blank column of clearance, so two labels never touch.
        if at <= used || at + name.len() > GUTTER as usize + visible * WEEK_COLS as usize {
            continue;
        }
        row.push_str(&" ".repeat(at - used));
        row.push_str(name);
    }
    Line::from(Span::styled(row, Style::default().fg(colors::OVERLAY)))
}

/// The whole grid: a frame row, then each weekday row followed by another
/// frame row. Built together rather than per-line because the selection frame
/// spans three rows, and the row above and below a marked cell have to know
/// where it is.
fn grid_rows(
    calendar: &Calendar,
    first: usize,
    visible: usize,
    cursor: Option<usize>,
) -> Vec<Line<'static>> {
    // Where the cursor sits in the drawn window, if it is in it at all.
    let marked = cursor.and_then(|slot| {
        let (week, weekday) = (slot / 7, slot % 7);
        (week >= first && week < first + visible).then_some((week - first, weekday))
    });

    (0..GRID_ROWS)
        .map(|row| {
            if row.is_multiple_of(DAY_ROWS) {
                // A gap row: blank, unless it is the top or bottom edge of the
                // frame around the marked cell.
                let boundary = (row / DAY_ROWS) as usize;
                match marked {
                    Some((column, weekday)) if weekday == boundary => {
                        frame_row(column, visible, FrameEdge::Top)
                    }
                    Some((column, weekday)) if weekday + 1 == boundary => {
                        frame_row(column, visible, FrameEdge::Bottom)
                    }
                    _ => Line::from(""),
                }
            } else {
                let weekday = (row / DAY_ROWS) as usize;
                day_line(calendar, first, visible, weekday, marked)
            }
        })
        .collect()
}

enum FrameEdge {
    Top,
    Bottom,
}

/// The top or bottom edge of the selection frame, drawn in the gap row above
/// or below the marked cell. Rounded corners, matching the pane borders.
fn frame_row(column: usize, visible: usize, edge: FrameEdge) -> Line<'static> {
    let (left, right) = match edge {
        FrameEdge::Top => ('\u{256d}', '\u{256e}'),
        FrameEdge::Bottom => ('\u{2570}', '\u{256f}'),
    };
    // The frame sits one column outside the cell on each side. For the very
    // first column that is the gutter's last space, which is always blank.
    let cell_start = GUTTER as usize + column * WEEK_COLS as usize;
    let mut row = String::with_capacity(GUTTER as usize + visible * WEEK_COLS as usize);
    row.push_str(&" ".repeat(cell_start - 1));
    row.push(left);
    for _ in 0..CELL_COLS {
        row.push('\u{2500}');
    }
    row.push(right);
    Line::from(Span::styled(row, Style::default().fg(colors::LAVENDER)))
}

fn day_line(
    calendar: &Calendar,
    first: usize,
    visible: usize,
    weekday: usize,
    marked: Option<(usize, usize)>,
) -> Line<'static> {
    // Only three of the seven rows are labelled, as GitHub does: seven labels
    // is a wall of text beside a picture.
    let label = match weekday {
        1 => "Mon ",
        3 => "Wed ",
        5 => "Fri ",
        _ => "    ",
    };
    let ladder = colors::contrib_ladder(calendar.source());
    let mut gutter = label.to_string();
    // The marked cell's left frame edge lands in the gutter when it is the
    // first drawn column.
    if marked == Some((0, weekday)) {
        gutter.pop();
        gutter.push('\u{2502}');
    }
    let mut spans = vec![Span::styled(gutter, Style::default().fg(colors::OVERLAY))];

    let edge = |on: bool| {
        if on {
            Span::styled(
                '\u{2502}'.to_string(),
                Style::default().fg(colors::LAVENDER),
            )
        } else {
            Span::raw(" ".repeat(GAP_COLS as usize))
        }
    };

    for column in 0..visible {
        let week = first + column;
        match calendar.day_at(week, weekday) {
            Some((_, count)) => spans.push(Span::styled(
                CELL,
                Style::default().fg(ladder[calendar.intensity(count) as usize]),
            )),
            // Past today: hold the column open so the grid keeps its shape.
            None => spans.push(Span::raw(" ".repeat(CELL_COLS as usize))),
        }
        // The gap after this cell doubles as the right edge of a frame around
        // it, or the left edge of one around the next column.
        let right_of_marked = marked == Some((column, weekday));
        let left_of_marked = marked == Some((column + 1, weekday));
        spans.push(edge(right_of_marked || left_of_marked));
    }
    Line::from(spans)
}

fn legend_line(source: crate::contributions::Source) -> Line<'static> {
    let mut spans = vec![Span::styled(
        " Less ".to_string(),
        Style::default().fg(colors::OVERLAY),
    )];
    for (i, shade) in colors::contrib_ladder(source).iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(CELL, Style::default().fg(*shade)));
    }
    spans.push(Span::styled(
        " More ".to_string(),
        Style::default().fg(colors::OVERLAY),
    ));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contributions::{Calendar, Source, WEEKS};

    fn area(height: u16) -> Rect {
        Rect::new(0, 1, 120, height)
    }

    fn day(s: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn a_closed_pane_leaves_the_main_area_alone() {
        let (main, strip) = split_main_area(area(40), false);
        assert_eq!(main, area(40));
        assert!(strip.is_none());
    }

    #[test]
    fn an_open_pane_takes_a_fixed_strip_off_the_bottom() {
        let (main, strip) = split_main_area(area(40), true);
        let strip = strip.expect("room for the strip");
        assert_eq!(strip.height, CONTRIB_HEIGHT);
        assert_eq!(
            strip.y + strip.height,
            area(40).y + area(40).height,
            "docked to the bottom edge"
        );
        assert_eq!(main.y, area(40).y, "the panes above keep the top edge");
        assert_eq!(
            main.y + main.height + 1,
            strip.y,
            "one blank row between them, like the other splits"
        );
    }

    /// The dashboard outranks the heatmap: on a short terminal the strip is
    /// dropped rather than squeezing the panes into slivers.
    #[test]
    fn a_short_area_suppresses_the_strip_entirely() {
        let floor = MAIN_MIN_HEIGHT + CONTRIB_HEIGHT + 1;
        assert!(split_main_area(area(floor - 1), true).1.is_none());
        assert!(split_main_area(area(floor), true).1.is_some());
    }

    /// Every square is padded on all four sides, one column each way and one
    /// row each way. The row gap is load-bearing beyond looks: the selection
    /// frame is drawn in the rows above and below the marked cell, so a grid
    /// with no row gap would have nowhere to put it.
    #[test]
    fn every_square_is_padded_on_all_four_sides() {
        assert_eq!(CELL.chars().count(), CELL_COLS as usize);
        assert_eq!(GAP_COLS, 1);
        assert_eq!(GAP_ROWS, 1);
        assert_eq!(WEEK_COLS, 3);
        assert_eq!(DAY_ROWS, 2);
        // A frame row, then each of seven days followed by another frame row.
        assert_eq!(GRID_ROWS, 15);
        // border + months + grid + border; the legend rides in the border.
        assert_eq!(CONTRIB_HEIGHT, 2 + 1 + 15);
    }

    /// The grid never shrinks to fit. A narrower pane shows fewer weeks at
    /// the same square size, which is what makes scrolling the answer.
    #[test]
    fn a_narrower_strip_shows_fewer_weeks_at_the_same_size() {
        // 120 wide: 120 - 4 frame - 4 gutter = 112 grid columns, at 3 each.
        assert_eq!(visible_weeks(Rect::new(0, 0, 120, CONTRIB_HEIGHT)), 37);
        assert_eq!(visible_weeks(Rect::new(0, 0, 60, CONTRIB_HEIGHT)), 17);
        assert_eq!(visible_weeks(Rect::new(0, 0, 12, CONTRIB_HEIGHT)), 1);
        assert_eq!(visible_weeks(Rect::new(0, 0, 8, CONTRIB_HEIGHT)), 0);
        // A year never needs more than 53 columns even on a huge terminal.
        assert_eq!(visible_weeks(Rect::new(0, 0, 500, CONTRIB_HEIGHT)), WEEKS);
    }

    #[test]
    fn thousands_groups_the_total_the_way_github_does() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(2_516), "2,516");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn the_title_names_the_source_and_the_span() {
        let mut cal = Calendar::new(day("2026-08-18"), Source::Github);
        cal.add(day("2026-08-18"), 3);
        assert_eq!(
            title(&cal, None, 120),
            "3 contributions in the last year (github)"
        );
    }

    /// "1 contributions" reads as a bug. A one-commit day is common.
    #[test]
    fn a_count_of_one_is_singular() {
        let mut cal = Calendar::new(day("2026-08-18"), Source::Local);
        cal.add(day("2025-08-19"), 1);
        assert_eq!(title(&cal, Some(2), 120), "1 contribution on Tue 19 Aug");
        assert_eq!(
            title(&cal, None, 120),
            "1 contribution in the last year (local)"
        );
    }

    #[test]
    fn a_marked_day_reads_itself_out() {
        let mut cal = Calendar::new(day("2026-08-18"), Source::Local);
        cal.add(day("2025-08-19"), 7);
        assert_eq!(title(&cal, Some(2), 120), "7 contributions on Tue 19 Aug");
    }

    #[test]
    fn a_marked_empty_day_says_none_rather_than_zero() {
        let cal = Calendar::new(day("2026-08-18"), Source::Local);
        assert_eq!(title(&cal, Some(2), 120), "no contributions on Tue 19 Aug");
    }

    /// A narrow strip clipped the source label off the end, which is the one
    /// word that says whether you are looking at GitHub's count or your own.
    #[test]
    fn a_narrow_title_sheds_words_before_it_sheds_the_source() {
        let cal = Calendar::new(day("2026-08-18"), Source::Github);
        let narrow = title(&cal, None, 34);
        assert!(narrow.chars().count() <= 34, "still long: {narrow}");
        assert!(narrow.contains("(github)"), "source survived: {narrow}");
        assert_eq!(title(&cal, None, 8), "0");
    }

    #[test]
    fn a_click_maps_back_to_the_slot_under_it() {
        // Outer rect: row 0 is the border, row 1 the month labels, then day
        // rows at 2, 4, 6 ... with a blank between each. Columns 0..1 are the
        // border and padding, 2..5 the gutter, so week 0 starts at column 6
        // and each week step is 4 columns.
        let area = Rect::new(0, 5, 120, CONTRIB_HEIGHT);
        assert_eq!(slot_at(area, 0, 6, 8), Some(0), "first Sunday");
        assert_eq!(slot_at(area, 0, 7, 8), Some(0), "cell is two columns wide");
        assert_eq!(slot_at(area, 0, 8, 8), None, "the gap after it");
        assert_eq!(slot_at(area, 0, 10, 8), Some(7), "next week's Sunday");
        assert_eq!(slot_at(area, 0, 6, 10), Some(1), "the Monday row");
        assert_eq!(slot_at(area, 0, 6, 9), None, "the blank row between");
        assert_eq!(slot_at(area, 0, 3, 8), None, "the day gutter");
        assert_eq!(slot_at(area, 0, 6, 5), None, "the top border");
        assert_eq!(slot_at(area, 0, 6, 6), None, "the month label row");
        assert_eq!(slot_at(area, 0, 6, 7), None, "the blank padding the top");
        assert_eq!(slot_at(area, 0, 119, 8), None, "past the last drawn week");
    }

    /// A scrolled grid's leftmost column is not week 0. Clicking it must not
    /// report a day from last August.
    #[test]
    fn a_click_on_a_scrolled_grid_accounts_for_the_offset() {
        let area = Rect::new(0, 5, 120, CONTRIB_HEIGHT);
        assert_eq!(slot_at(area, 10, 6, 8), Some(10 * 7));
        // Scroll past the end: clamped, never a slot outside the year.
        let visible = visible_weeks(area);
        assert_eq!(slot_at(area, 999, 6, 8), Some((WEEKS - visible) * 7));
    }

    /// Render the strip into a buffer and read it back as text lines.
    fn drawn(width: u16, app: &App) -> Vec<String> {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut terminal = Terminal::new(TestBackend::new(width, CONTRIB_HEIGHT)).unwrap();
        terminal.draw(|f| render(f, f.area(), app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..CONTRIB_HEIGHT)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    /// The strip's interior, with the pane's own border stripped off. The
    /// block uses rounded corners too, so a search for the selection frame
    /// has to exclude them or it matches the pane itself.
    fn interior(lines: &[String]) -> Vec<String> {
        lines[1..lines.len() - 1]
            .iter()
            .map(|l| {
                let chars: Vec<char> = l.chars().collect();
                chars.iter().skip(1).collect::<String>()
            })
            .collect()
    }

    fn app_with_calendar(total_per_day: u32) -> App {
        let mut app = App::new(Vec::new(), "test".to_string(), false, 120, 44);
        app.contributions = ContributionsMode::Github;
        let today = chrono::Local::now().date_naive();
        let mut cal = Calendar::new(today, Source::Github);
        let start = crate::contributions::grid_start(today);
        for slot in 0..crate::contributions::SLOTS {
            cal.add(start + chrono::Duration::days(slot as i64), total_per_day);
        }
        app.github_calendar = Some(Box::new(cal));
        app
    }

    #[test]
    fn the_strip_draws_a_framed_pane_with_spaced_rows_and_a_legend() {
        let lines = drawn(120, &app_with_calendar(1));
        assert_eq!(lines.len(), CONTRIB_HEIGHT as usize);
        assert!(
            lines[0].contains("[4] contributions"),
            "title: {}",
            lines[0]
        );
        assert!(lines[1].contains("Aug") || lines[1].contains("Jul"));
        assert!(
            !lines[2].contains(CELL),
            "blank padding above the first row"
        );
        assert!(lines[3].contains(CELL), "Sunday row");
        assert!(!lines[4].contains(CELL), "blank row between day rows");
        assert!(lines[5].contains("Mon "), "Monday row: {}", lines[5]);
        assert!(lines[9].contains("Wed "));
        assert!(lines[13].contains("Fri "));
        assert!(
            !lines[16].contains("Less"),
            "blank padding below the last row"
        );
        assert!(lines[17].contains("Less") && lines[17].contains("More"));
    }

    /// The grid opens with the newest weeks against the right edge: today is
    /// what you came to look at, and time runs left to right.
    #[test]
    fn the_grid_opens_anchored_on_the_newest_weeks() {
        // `contrib_scroll` is a Cell, so the app need not be mut here.
        let app = app_with_calendar(1);
        // Whatever the app asked for, the first frame pins today to the right.
        app.contrib_scroll.set(crate::contributions::WEEKS);
        let strip = Rect::new(0, 0, 120, CONTRIB_HEIGHT);
        let _ = drawn(120, &app);
        let visible = visible_weeks(strip);
        assert_eq!(
            app.contrib_scroll.get(),
            WEEKS - visible,
            "oldest weeks hide off to the left"
        );
        assert_eq!(app.contrib_visible_weeks.get(), visible);
    }

    /// No row may spill past the strip's right edge, at any width.
    #[test]
    fn no_row_overflows_the_strip_at_any_width() {
        for width in [20u16, 40, 57, 80, 120, 200, 260] {
            for line in drawn(width, &app_with_calendar(2)) {
                assert!(
                    line.chars().count() <= width as usize,
                    "width {width} produced a {}-char line: {line}",
                    line.chars().count()
                );
            }
        }
    }

    /// Month labels sat two characters apart when the grid was dense. With
    /// four columns per week they are ~17 apart, and any that would still
    /// collide is dropped rather than drawn over its neighbour.
    #[test]
    fn month_labels_never_collide() {
        for width in [40u16, 80, 120, 200, 260] {
            let app = app_with_calendar(1);
            let lines = drawn(width, &app);
            let months = &lines[1];
            let mut last_end = 0usize;
            for (i, ch) in months.char_indices() {
                if ch.is_ascii_alphabetic()
                    && (i == 0 || !months.as_bytes()[i - 1].is_ascii_alphabetic())
                {
                    assert!(i > last_end, "labels touch at width {width}: {months}");
                    last_end = i + 3;
                }
            }
        }
    }

    /// Before the first poll there is no calendar. The frame still draws, so
    /// the pane does not blink into existence when the fetch lands, but no
    /// grid is painted: an empty year would read as "you did nothing".
    #[test]
    fn the_github_source_draws_a_frame_but_no_grid_before_the_first_poll() {
        let mut app = App::new(Vec::new(), "test".to_string(), false, 120, 44);
        app.contributions = ContributionsMode::Github;
        assert!(app.github_calendar.is_none());
        let lines = drawn(120, &app);
        assert!(lines[0].contains("[4] contributions"), "frame drawn");
        assert!(!lines[5].contains(CELL), "no cells painted: {}", lines[5]);
    }

    /// Nothing is highlighted until the user travels the grid, and when a day
    /// is marked it gets a rounded frame around it. It used to be drawn with
    /// REVERSED, which on a solid block just swaps foreground and background
    /// and turns the marked cell into an invisible hole.
    #[test]
    fn the_marked_day_gets_a_rounded_frame_and_nothing_is_marked_before_that() {
        let mut app = app_with_calendar(1);
        app.focused_panel = FocusedPanel::Contributions;
        app.contrib_scroll.set(WEEKS);
        assert_eq!(app.contrib_cursor, None);

        let bare = interior(&drawn(120, &app)).join("\n");
        for corner in ['\u{256d}', '\u{256e}', '\u{2570}', '\u{256f}'] {
            assert!(
                !bare.contains(corner),
                "nothing framed before travelling: {corner}"
            );
        }

        // Mark a day well inside the window so the whole frame is drawn.
        let last = app.contrib_last_slot(chrono::Local::now().date_naive());
        app.contrib_cursor = Some(last - 7 * 3 - 4);
        let lines = interior(&drawn(120, &app));
        let marked = lines.join("\n");
        for corner in ['\u{256d}', '\u{256e}', '\u{2570}', '\u{256f}'] {
            assert!(marked.contains(corner), "frame corner missing: {corner}");
        }
        assert!(marked.contains('\u{2502}'), "frame sides missing");

        // The frame brackets the cell: a top edge row, then the day row, then
        // a bottom edge row, and the cell itself is still painted.
        let top = lines.iter().position(|l| l.contains('\u{256d}')).unwrap();
        let bottom = lines.iter().position(|l| l.contains('\u{2570}')).unwrap();
        assert_eq!(bottom, top + 2, "one day row between the two edges");
        assert!(
            lines[top + 1].contains(CELL),
            "the marked cell is still lit"
        );
    }

    /// The frame must not be clipped when the marked day is the first drawn
    /// column: its left edge lands in the day gutter, which is blank there.
    #[test]
    fn a_frame_on_the_first_column_draws_inside_the_gutter() {
        let mut app = app_with_calendar(1);
        app.focused_panel = FocusedPanel::Contributions;
        app.contrib_scroll.set(0);
        app.contrib_cursor = Some(3); // week 0, a Wednesday
        let lines = interior(&drawn(120, &app));
        let top = lines
            .iter()
            .position(|l| l.contains('\u{256d}'))
            .expect("top edge drawn");
        assert!(
            lines[top].contains("\u{256d}\u{2500}\u{2500}\u{256e}"),
            "whole top edge present: {}",
            lines[top]
        );
        assert!(
            lines[top + 1].contains('\u{2502}'),
            "side edges present: {}",
            lines[top + 1]
        );
    }

    /// The two sources count different things and can never agree, so they
    /// must not look identical: the colour says which one you are reading
    /// before the title does.
    #[test]
    fn each_source_gets_its_own_intensity_ladder() {
        let github = colors::contrib_ladder(Source::Github);
        let local = colors::contrib_ladder(Source::Local);
        assert_eq!(github[0], local[0], "an empty day is the same either way");
        assert_ne!(github[4], local[4], "a busy day is not");
        for step in 1..5 {
            assert_ne!(github[step], local[step], "step {step} must differ");
        }
    }

    /// Cells and the legend read the same ladder, so a source switch can
    /// never leave half the strip on the other palette.
    #[test]
    fn the_legend_follows_the_source_the_cells_use() {
        let mut app = app_with_calendar(4);
        let github_cells = colors::contrib_ladder(Source::Github);

        let styled = |app: &App| {
            use ratatui::Terminal;
            use ratatui::backend::TestBackend;
            let mut t = Terminal::new(TestBackend::new(120, CONTRIB_HEIGHT)).unwrap();
            t.draw(|f| render(f, f.area(), app)).unwrap();
            let buf = t.backend().buffer().clone();
            let mut seen = std::collections::HashSet::new();
            for x in 0..120 {
                for y in 0..CONTRIB_HEIGHT {
                    if buf[(x, y)].symbol() == "\u{2588}\u{2588}"
                        || buf[(x, y)].symbol() == "\u{2588}"
                    {
                        seen.insert(format!("{:?}", buf[(x, y)].style().fg));
                    }
                }
            }
            seen
        };

        let on_github = styled(&app);
        assert!(on_github.contains(&format!("{:?}", Some(github_cells[4]))));

        app.contributions = ContributionsMode::Local;
        let on_local = styled(&app);
        assert!(
            !on_local.contains(&format!("{:?}", Some(github_cells[4]))),
            "no github green survives a switch to local"
        );
    }
}
