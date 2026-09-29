//! The contribution strip: a year of activity docked under the main panes.
//!
//! A day is a Nerd Font square icon in one character cell, with a blank column
//! between weeks. The icon is the point: it is drawn by the font to sit on the
//! text line, centred where a capital letter is centred, so it lines up with
//! the `Mon`/`Wed`/`Fri` label beside it. Nothing in the block-element range
//! can do that. A half block (`\u{2584}` or `\u{2580}`) is a true square, but
//! it fills the bottom or top half of the cell and so sits a quarter row off
//! the label's centre either way; this strip was drawn with the lower half
//! for a long time and the labels never looked attached to their rows. A full
//! block aligns but is a rectangle twice as tall as it is wide, with no
//! padding between days. The icon has the square's shape and the text's
//! baseline, and its unpainted surround is the padding, in both directions.
//!
//! Three sizes ship (`CellSize`), picked by the `contributions_cell_size`
//! config key; they are three different icons, measured in the font rather
//! than named by feel. All are one column wide in the `unicode-width` sense
//! ratatui lays out by, which the grid's exact column arithmetic (`slot_at`
//! reverses it to route clicks) depends on.
//!
//! The grid draws at a fixed size and scrolls, rather than shrinking to fit.
//! The newest weeks are pinned to the right edge and older ones hide off to
//! the left, which is the direction time already runs in.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};

use crate::app::{App, ContributionsMode, FocusedPanel};
use crate::contributions::{Calendar, WEEKS};
use crate::ui::colors;

/// Points at the column holding the marked day, since a single small square
/// is easy to lose in a year of them.
const CARET: &str = "▾";

/// A day is one column wide, with one blank column between weeks.
const CELL_COLS: u16 = 1;
const GAP_COLS: u16 = 1;
/// Columns one week occupies, cell plus its trailing gap. The gap is always
/// emitted, including after the last column, so the grid width is uniform.
const WEEK_COLS: u16 = CELL_COLS + GAP_COLS;

/// One row per weekday. The icon does not fill its cell, so the row needs
/// no padding row of its own.
const DAY_ROWS: u16 = 7;

/// Columns reserved for the weekday gutter.
const GUTTER: u16 = 4;

/// Columns the frame eats on each side: the border plus the block's own
/// one-column padding.
const FRAME_COLS: u16 = 2;

/// First grid row, counted from the strip's outer top edge: row 0 is the
/// border, row 1 the month labels, which the caret shares.
const FIRST_DAY_ROW: u16 = 2;

/// Rows the strip occupies: border, the month/caret row, the seven day rows,
/// border. The count, the source, and the legend all ride in the borders
/// rather than costing rows of their own.
///
/// The caret used to own a row, which was blank whenever no day was marked --
/// so the common case paid a full row of white space between the labels and
/// the grid, and it read as a gap rather than as breathing room. Sharing
/// costs a month label when the two collide, which is cheap: the title spells
/// the marked date out in full anyway.
pub const CONTRIB_HEIGHT: u16 = 1 + 1 + DAY_ROWS + 1;

/// Rows the panes above must keep for the strip to be worth docking. Below
/// this the strip is dropped: a dashboard squeezed into slivers is worse
/// than no heatmap.
const MAIN_MIN_HEIGHT: u16 = 10;

/// Carve the strip off the bottom of the main area.
///
/// Single source of truth for that geometry, in the same sense as
/// `detail::split_main` and `detail::split_inspector`: `ui::render` draws
/// from it and `ui::panel_rects` hit-tests against it. It lives here rather
/// than in `detail.rs` because the strip exists in Compact view too.
pub fn split_main_area(main: Rect, folded: bool) -> (Rect, Option<Rect>) {
    let height = if folded {
        crate::ui::FOLDED_HEIGHT
    } else {
        CONTRIB_HEIGHT
    };
    if main.height < MAIN_MIN_HEIGHT + height + 1 {
        return (main, None);
    }
    let v = Layout::vertical([Constraint::Min(1), Constraint::Length(height)])
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

/// Blank columns left of the gutter, so the newest week sits flush against
/// the right edge rather than the oldest sitting against the left.
///
/// Time already runs left to right, so the end worth looking at is today's.
/// `visible_weeks` caps at `WEEKS`, so on a pane wider than a year needs the
/// surplus used to pool on the right and push today away from the edge the
/// eye goes to first. The pad sits *before* the gutter so `Mon`/`Wed`/`Fri`
/// travel with the grid; after it they would strand themselves at the far
/// left with the surplus between them and the squares they label.
///
/// Both the renderer and `slot_at` go through here for the same reason they
/// both go through `visible_weeks`: an offset applied on one side only
/// misroutes every click by however wide the pad is, and nothing fails.
pub fn grid_pad(area: Rect) -> u16 {
    let inner = area.width.saturating_sub(FRAME_COLS * 2);
    // Every week emits a trailing gap, the last one included, but nothing is
    // painted in it: the grid's visible right edge is one column short of the
    // space it occupies. Reclaim that column or the newest week stops a step
    // shy of the border.
    let painted = (visible_weeks(area) as u16 * WEEK_COLS).saturating_sub(GAP_COLS);
    inner.saturating_sub(GUTTER + painted)
}

/// Whole weeks a horizontal drag of `cols` screen columns amounts to.
///
/// Rounds toward zero, so a drag shorter than one week's worth of columns
/// moves nothing. The caller keeps the remainder by advancing its anchor only
/// by what was consumed -- see `App::pan_contributions`.
pub fn weeks_dragged(cols: i32) -> i32 {
    cols / WEEK_COLS as i32
}

/// Screen columns `weeks` weeks occupy, for advancing a drag anchor by
/// exactly the distance that was turned into scrolling.
pub fn cols_for_weeks(weeks: i32) -> i32 {
    weeks * WEEK_COLS as i32
}

/// The leftmost week to draw: whatever the app asked for, clamped so the
/// grid can never scroll past its own ends. An out-of-range request pins the
/// newest week to the right edge, which is where the grid opens.
///
/// **A resize holds the right edge, not the left.** The window is stored as a
/// left edge, so a narrower pane used to shorten it from the right: shrinking
/// the terminal took the newest weeks off the screen -- including the day you
/// were standing on -- and kept the old ones nobody was looking at. Time runs
/// left to right here and the end worth looking at is today's, the same reason
/// `grid_pad` pools its surplus on the left.
///
/// Growing already anchored right, by accident of the clamp above. This makes
/// the two directions agree on purpose rather than by luck.
///
/// The cursor outranks even that. Holding the right edge covers the common
/// case, but it would push a cursor parked in the oldest weeks off the left
/// instead, which is the same bug seen in a mirror. Applied only on a resize:
/// on an ordinary frame a pan must be free to travel away from the cursor,
/// and yanking the view back to it would make panning impossible.
fn first_week(app: &App, visible: usize) -> usize {
    let max = WEEKS.saturating_sub(visible);
    let scroll = app.contrib_scroll.get();
    let previous = app.contrib_visible_weeks.get();
    if previous == visible {
        return scroll.min(max);
    }

    let shift = previous as isize - visible as isize;
    let mut first = (scroll as isize + shift).clamp(0, max as isize) as usize;
    if let Some(slot) = app.contrib_cursor {
        let week = slot / 7;
        if week < first {
            first = week;
        } else if week >= first + visible {
            first = week + 1 - visible;
        }
        first = first.min(max);
    }
    first
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
    let span = crate::contributions::span_label();
    for candidate in [
        format!("{total} {} in {span} ({source})", noun(calendar.total())),
        format!("{total} in {span} ({source})"),
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
///
/// A click anywhere in a day's character cell selects that day, including
/// the unpainted surround of the icon: a click that lands a pixel off still
/// means the day you were pointing at.
pub fn slot_at(area: Rect, scroll: usize, col: u16, row: u16) -> Option<usize> {
    let visible = visible_weeks(area);
    let weekday = row.checked_sub(area.y + FIRST_DAY_ROW)?;
    if weekday >= DAY_ROWS {
        return None;
    }
    let across = col.checked_sub(area.x + FRAME_COLS + grid_pad(area) + GUTTER)?;
    // The trailing column of each step is the gap, not the cell.
    if across % WEEK_COLS >= CELL_COLS {
        return None;
    }
    let column = (across / WEEK_COLS) as usize;
    if column >= visible {
        return None;
    }
    let week = scroll.min(WEEKS.saturating_sub(visible)) + column;
    Some(week * 7 + weekday as usize)
}

/// Draw the strip: a bordered pane whose title carries the count and the
/// source, over the month labels, the caret row, and the packed day rows.
///
/// Framed like the other three panes rather than drawn bare, because it is
/// focusable the same way they are: without a border there is nothing on
/// screen to say `4` reached it or that it currently holds the keys.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    if app.contributions_folded {
        crate::ui::render_folded(
            frame,
            area,
            crate::ui::CONTRIBUTIONS_NUM,
            "contributions",
            app.focused_panel == FocusedPanel::Contributions,
        );
        return;
    }
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
            block.title_top(crate::ui::panel_title(
                crate::ui::CONTRIBUTIONS_NUM,
                "contributions",
                focused,
            )),
            area,
        );
        return;
    };

    // Report the measured window back, so the scroll keys and the cursor
    // have something real to clamp against. This is the render pass telling
    // the input layer how wide the view turned out, not state it owns.
    let visible = visible_weeks(area);
    // Read before the write: `first_week` compares this frame's width against
    // the last one to tell a resize from a pan, so reporting the new width
    // first would hide every resize from it.
    let first = first_week(app, visible);
    app.contrib_visible_weeks.set(visible.max(1));
    app.contrib_scroll.set(first);

    let cursor = app.contrib_cursor.map(|c| c.min(calendar.today_slot()));
    // Where the cursor sits in the drawn window, if it is in it at all.
    let marked = cursor.and_then(|slot| {
        let (week, weekday) = (slot / 7, slot % 7);
        (week >= first && week < first + visible).then_some((week - first, weekday))
    });
    let inner_width = area.width.saturating_sub(FRAME_COLS * 2);

    let pad = grid_pad(area);
    let glyph = app.contrib_cell.glyph();
    let mut lines = Vec::with_capacity(CONTRIB_HEIGHT as usize);
    lines.push(month_and_caret_line(&calendar, first, visible, pad, marked));
    for weekday in 0..DAY_ROWS as usize {
        lines.push(day_line(
            &calendar, first, visible, weekday, marked, pad, glyph,
        ));
    }

    let heading = title(&calendar, cursor, inner_width);
    frame.render_widget(
        Paragraph::new(lines).block(
            block
                .title_top(crate::ui::panel_title(
                    crate::ui::CONTRIBUTIONS_NUM,
                    "contributions",
                    focused,
                ))
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
                .title_bottom(legend_line(calendar.source(), glyph).right_aligned()),
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
/// sitting over the column its month starts in, with the day cursor's caret
/// sharing the row.
///
/// They share it because the caret is absent most of the time, and a row that
/// is blank most of the time reads as a gap in the strip rather than as
/// breathing room under the labels. A label that would collide with the caret
/// is dropped rather than drawn through: the caret says which column you are
/// on, which is the more urgent of the two, and the title spells the date out
/// in full regardless. A label colliding with the label before it is dropped
/// for the reason it always was.
fn month_and_caret_line(
    calendar: &Calendar,
    first: usize,
    visible: usize,
    pad: u16,
    marked: Option<(usize, usize)>,
) -> Line<'static> {
    let base = (pad + GUTTER) as usize;
    let caret_at = marked.map(|(column, _)| base + column * WEEK_COLS as usize);
    let label_style = Style::default().fg(colors::OVERLAY);

    let mut row = " ".repeat(base);
    for (week, name) in calendar.month_labels() {
        if week < first || week >= first + visible {
            continue;
        }
        let at = base + (week - first) * WEEK_COLS as usize;
        let used = row.chars().count();
        // One blank column of clearance, so two labels never touch.
        if at <= used || at + name.len() > base + visible * WEEK_COLS as usize {
            continue;
        }
        // Never draw a label through the caret: "M▾r" is worse than no label.
        if caret_at.is_some_and(|c| c >= at && c < at + name.len()) {
            continue;
        }
        row.push_str(&" ".repeat(at - used));
        row.push_str(name);
    }

    let Some(caret_at) = caret_at else {
        return Line::from(Span::styled(row, label_style));
    };
    let caret = Span::styled(CARET, Style::default().fg(colors::LAVENDER));
    // The caret can sit past the last label drawn, in which case the row only
    // needs padding out to it.
    if row.chars().count() <= caret_at {
        row.push_str(&" ".repeat(caret_at - row.chars().count()));
        return Line::from(vec![Span::styled(row, label_style), caret]);
    }
    // Otherwise it falls inside the drawn range. The label that would have
    // covered it was skipped above, so that column is blank and the caret
    // takes it rather than overwriting a letter.
    let before: String = row.chars().take(caret_at).collect();
    let after: String = row.chars().skip(caret_at + 1).collect();
    Line::from(vec![
        Span::styled(before, label_style),
        caret,
        Span::styled(after, label_style),
    ])
}

/// One row of the grid: a single weekday.
#[allow(clippy::too_many_arguments)]
fn day_line(
    calendar: &Calendar,
    first: usize,
    visible: usize,
    weekday: usize,
    marked: Option<(usize, usize)>,
    pad: u16,
    glyph: &'static str,
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
    let mut spans = vec![
        Span::raw(" ".repeat(pad as usize)),
        Span::styled(label.to_string(), Style::default().fg(colors::OVERLAY)),
    ];

    for column in 0..visible {
        let week = first + column;
        match calendar.day_at(week, weekday) {
            Some(_) => {
                // The marked day is painted in plain text white: it has to
                // beat both ladders, and every status colour in the palette
                // already means something else.
                let color = if marked == Some((column, weekday)) {
                    colors::TEXT
                } else {
                    ladder[calendar.level_at(week, weekday) as usize]
                };
                spans.push(Span::styled(glyph, Style::default().fg(color)));
            }
            // Past today: hold the column open so the grid keeps its shape.
            None => spans.push(Span::raw(" ".repeat(CELL_COLS as usize))),
        }
        spans.push(Span::raw(" ".repeat(GAP_COLS as usize)));
    }
    Line::from(spans)
}

fn legend_line(source: crate::contributions::Source, glyph: &'static str) -> Line<'static> {
    let mut spans = vec![Span::styled(
        " Less ".to_string(),
        Style::default().fg(colors::OVERLAY),
    )];
    // Drawn with the same icon and the same one-column gap as the grid, so
    // the legend is a sample of the thing it explains rather than a separate
    // rendering of it.
    for (i, shade) in colors::contrib_ladder(source).iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(glyph, Style::default().fg(*shade)));
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
    use crate::app::CellSize;
    use crate::contributions::{Calendar, Source, WEEKS};

    /// What a fresh `App` draws a day as.
    const CELL: &str = CellSize::Medium.glyph();

    fn area(height: u16) -> Rect {
        Rect::new(0, 1, 120, height)
    }

    fn day(s: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn a_folded_pane_keeps_only_its_frame_at_the_bottom() {
        let (main, strip) = split_main_area(area(40), true);
        let strip = strip.expect("a folded strip is still drawn");
        assert_eq!(strip.height, crate::ui::FOLDED_HEIGHT);
        assert_eq!(strip.y + strip.height, area(40).y + area(40).height);
        assert_eq!(main.y + main.height + 1, strip.y);
    }

    #[test]
    fn an_open_pane_takes_a_fixed_strip_off_the_bottom() {
        let (main, strip) = split_main_area(area(40), false);
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
        assert!(split_main_area(area(floor - 1), false).1.is_none());
        assert!(split_main_area(area(floor), false).1.is_some());
    }

    /// Every size is one Nerd Font icon in one column. The grid's column
    /// arithmetic, and `slot_at` reversing it to route clicks, both assume a
    /// day is exactly `CELL_COLS` wide as ratatui measures it; a glyph that
    /// laid out wider would overflow every row and misroute every click.
    #[test]
    fn every_cell_size_is_one_icon_one_column_wide() {
        for size in [CellSize::Small, CellSize::Medium, CellSize::Large] {
            let glyph = size.glyph();
            assert_eq!(glyph.chars().count(), 1, "{size:?} is one glyph");
            assert_eq!(
                Span::raw(glyph).width(),
                CELL_COLS as usize,
                "{size:?} lays out one column wide"
            );
            let cp = glyph.chars().next().unwrap() as u32;
            assert!(
                (0xE000..=0xF8FF).contains(&cp) || (0xF0000..=0xFFFFD).contains(&cp),
                "{size:?} is a private-use icon, not a block element"
            );
        }
        assert_eq!(CellSize::default(), CellSize::Medium);
        assert_eq!(GAP_COLS, 1);
        assert_eq!(WEEK_COLS, 2);
    }

    /// The configured size reaches the grid and the legend alike, or the
    /// legend explains a strip it does not match.
    #[test]
    fn the_configured_size_is_drawn_in_the_grid_and_the_legend() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = app_with_calendar(1);
        app.contrib_cell = CellSize::Large;
        let mut t = Terminal::new(TestBackend::new(120, CONTRIB_HEIGHT)).unwrap();
        t.draw(|f| render(f, f.area(), &app)).unwrap();
        let buf = t.backend().buffer();
        let large = CellSize::Large.glyph();
        let rows: Vec<String> = (0..CONTRIB_HEIGHT)
            .map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect())
            .collect();
        let grid = &rows[FIRST_DAY_ROW as usize + 1];
        assert!(
            grid.contains(large),
            "Monday row uses the large icon: {grid}"
        );
        assert!(!grid.contains(CELL), "and none of the default: {grid}");
        let bottom = &rows[CONTRIB_HEIGHT as usize - 1];
        assert!(
            bottom.contains(large),
            "legend samples the same icon: {bottom}"
        );
    }

    /// The grid never shrinks to fit. A narrower pane shows fewer weeks at
    /// the same square size, which is what makes scrolling the answer.
    #[test]
    fn a_narrower_strip_shows_fewer_weeks_at_the_same_size() {
        // 120 wide: 120 - 4 frame - 4 gutter = 112 grid columns, at 2 each.
        assert_eq!(visible_weeks(Rect::new(0, 0, 120, CONTRIB_HEIGHT)), 56);
        assert_eq!(visible_weeks(Rect::new(0, 0, 60, CONTRIB_HEIGHT)), 26);
        assert_eq!(visible_weeks(Rect::new(0, 0, 10, CONTRIB_HEIGHT)), 1);
        assert_eq!(visible_weeks(Rect::new(0, 0, 8, CONTRIB_HEIGHT)), 0);
        // The grid never needs more than its own width, however big the
        // terminal: past that the surplus becomes right-align pad.
        let wide = (WEEKS as u16 * WEEK_COLS) + GUTTER + FRAME_COLS * 2 + 40;
        assert_eq!(visible_weeks(Rect::new(0, 0, wide, CONTRIB_HEIGHT)), WEEKS);
    }

    #[test]
    fn thousands_groups_the_total_the_way_github_does() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(2_516), "2,516");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    /// Slot 2 is the Tuesday of the first column, whichever date the grid
    /// opens on. Spelled out so the span can change without these reading as
    /// a regression.
    fn slot_two(today: chrono::NaiveDate) -> String {
        let date = crate::contributions::grid_start(today) + chrono::Duration::days(2);
        date.format("%a %-d %b").to_string()
    }

    /// The title must name the span it is actually drawing. It said "the last
    /// year" while showing three, which is the kind of thing that makes a
    /// number look wrong when it is right.
    #[test]
    fn the_title_names_the_source_and_the_span() {
        let mut cal = Calendar::new(day("2026-08-18"), Source::Github);
        cal.add(day("2026-08-18"), 3);
        let span = crate::contributions::span_label();
        assert_eq!(
            title(&cal, None, 120),
            format!("3 contributions in {span} (github)")
        );
        assert!(
            span.contains(&crate::contributions::YEARS.to_string())
                || crate::contributions::YEARS == 1,
            "span label must name the year count: {span}"
        );
    }

    /// "1 contributions" reads as a bug. A one-commit day is common.
    #[test]
    fn a_count_of_one_is_singular() {
        let today = day("2026-08-18");
        let mut cal = Calendar::new(today, Source::Local);
        cal.add(
            crate::contributions::grid_start(today) + chrono::Duration::days(2),
            1,
        );
        assert_eq!(
            title(&cal, Some(2), 120),
            format!("1 contribution on {}", slot_two(today))
        );
        assert_eq!(
            title(&cal, None, 120),
            format!(
                "1 contribution in {} (local)",
                crate::contributions::span_label()
            )
        );
    }

    #[test]
    fn a_marked_day_reads_itself_out() {
        let today = day("2026-08-18");
        let mut cal = Calendar::new(today, Source::Local);
        cal.add(
            crate::contributions::grid_start(today) + chrono::Duration::days(2),
            7,
        );
        assert_eq!(
            title(&cal, Some(2), 120),
            format!("7 contributions on {}", slot_two(today))
        );
    }

    #[test]
    fn a_marked_empty_day_says_none_rather_than_zero() {
        let today = day("2026-08-18");
        let cal = Calendar::new(today, Source::Local);
        assert_eq!(
            title(&cal, Some(2), 120),
            format!("no contributions on {}", slot_two(today))
        );
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

    /// Screen column of the grid's leftmost cell. Derived rather than
    /// hardcoded: the grid is pushed right by `grid_pad`, and a test that
    /// baked in the old flush-left origin would have to be rewritten every
    /// time the alignment moves instead of catching that it moved wrongly.
    fn first_col(area: Rect) -> u16 {
        area.x + FRAME_COLS + grid_pad(area) + GUTTER
    }

    /// Screen row of the grid's first weekday, derived for the same reason.
    fn first_row(area: Rect) -> u16 {
        area.y + FIRST_DAY_ROW
    }

    /// Row indices into `drawn`, derived from the layout constants so that
    /// moving a row shows up as a real failure rather than as every render
    /// test needing a hand edit.
    const HEADER_ROW: usize = FIRST_DAY_ROW as usize - 1;
    const FIRST_GRID_ROW: usize = FIRST_DAY_ROW as usize;
    const LEGEND_ROW: usize = CONTRIB_HEIGHT as usize - 1;

    #[test]
    fn a_click_maps_back_to_the_slot_under_it() {
        // Outer rect: row 0 is the border, row 1 the month labels (which the
        // caret shares), then the seven day rows. Columns 0..1 are the border
        // and padding, then the right-align pad, then 4 columns of gutter,
        // and each week step is 2 columns.
        let area = Rect::new(0, 5, 120, CONTRIB_HEIGHT);
        let (c0, r0) = (first_col(area), first_row(area));
        assert_eq!(slot_at(area, 0, c0, r0), Some(0), "the Sunday row");
        assert_eq!(slot_at(area, 0, c0 + 1, r0), None, "the gap after the cell");
        assert_eq!(slot_at(area, 0, c0 + 2, r0), Some(7), "next week's Sunday");
        assert_eq!(slot_at(area, 0, c0, r0 + 1), Some(1), "the Monday row");
        assert_eq!(slot_at(area, 0, c0, r0 + 6), Some(6), "the Saturday row");
        assert_eq!(slot_at(area, 0, c0 - 1, r0), None, "the day gutter");
        assert_eq!(slot_at(area, 0, c0, area.y), None, "the top border");
        assert_eq!(slot_at(area, 0, c0, r0 - 1), None, "the month/caret row");
        assert_eq!(
            slot_at(area, 0, c0, r0 + DAY_ROWS),
            None,
            "past the last day row"
        );
    }

    /// A scrolled grid's leftmost column is not week 0. Clicking it must not
    /// report a day from last August.
    #[test]
    fn a_click_on_a_scrolled_grid_accounts_for_the_offset() {
        let area = Rect::new(0, 5, 40, CONTRIB_HEIGHT);
        let (c0, r0) = (first_col(area), first_row(area));
        assert_eq!(slot_at(area, 10, c0, r0), Some(10 * 7));
        // Scroll past the end: clamped, never a slot outside the grid.
        let visible = visible_weeks(area);
        assert_eq!(slot_at(area, 999, c0, r0), Some((WEEKS - visible) * 7));
    }

    /// The grid is anchored to the right edge, so the newest week is the one
    /// against the border. It used to be flush left, which parked a year of
    /// history under the eye and pushed today into the middle of the pane.
    #[test]
    fn the_newest_week_sits_against_the_right_edge() {
        // Wide enough that the whole grid fits with columns to spare: that
        // surplus is exactly what used to pool on the right. Derived from
        // WEEKS so widening the span does not silently gut the test by
        // making its premise unreachable.
        let width = WEEKS as u16 * WEEK_COLS + GUTTER + FRAME_COLS * 2 + 20;
        let area = Rect::new(0, 5, width, CONTRIB_HEIGHT);
        assert_eq!(visible_weeks(area), WEEKS, "test premise: the year fits");
        assert!(grid_pad(area) > 0, "test premise: there is surplus width");

        // The newest week's cell must land on the last column inside the
        // frame. One short of it is the bug this test exists for: the
        // trailing gap every week emits is unpainted, so a pad that does not
        // reclaim it leaves a visible notch at the right edge.
        let last = first_col(area) + (WEEKS as u16 - 1) * WEEK_COLS;
        assert_eq!(
            last,
            area.x + area.width - FRAME_COLS - 1,
            "newest week is not flush right"
        );
    }

    /// The pad must reach the hit-test as well as the renderer. Applied to
    /// one side only, every click reports a day `pad / 2` weeks too early and
    /// nothing anywhere fails.
    #[test]
    fn the_right_align_pad_moves_the_renderer_and_the_hit_test_together() {
        let width = WEEKS as u16 * WEEK_COLS + GUTTER + FRAME_COLS * 2 + 20;
        let area = Rect::new(0, 5, width, CONTRIB_HEIGHT);
        let pad = grid_pad(area);
        assert!(pad > 1, "test premise: a pad wide enough to notice");
        // A click at the old flush-left origin now lands in the pad, which is
        // not the grid at all.
        assert_eq!(
            slot_at(area, 0, area.x + FRAME_COLS + GUTTER, first_row(area)),
            None,
            "the old origin is dead space now"
        );
        assert_eq!(slot_at(area, 0, first_col(area), first_row(area)), Some(0));
    }

    /// A pane too narrow for the year opens on the newest weeks, not the
    /// oldest: `contrib_scroll` starts past the end so `first_week` clamps it
    /// to the right edge. Nothing guarded this before.
    #[test]
    fn a_squeezed_pane_opens_on_the_most_recent_weeks() {
        let area = Rect::new(0, 5, 40, CONTRIB_HEIGHT);
        let visible = visible_weeks(area);
        assert!(visible < WEEKS, "test premise: the year does not fit");

        let app = App::new(vec![], "test".to_string(), 40, 40);
        assert_eq!(
            first_week(&app, visible),
            WEEKS - visible,
            "a fresh strip should open on today, not last August"
        );
    }

    /// The reported bug: standing on a recent day and shrinking the terminal
    /// took the newest weeks off the right, including the marked day, and left
    /// the old weeks nobody was looking at. The window is stored as a left
    /// edge, so a narrower pane shortened it from the right.
    #[test]
    fn shrinking_the_pane_drops_the_oldest_weeks_not_the_newest() {
        let wide = Rect::new(0, 5, 120, CONTRIB_HEIGHT);
        let narrow = Rect::new(0, 5, 70, CONTRIB_HEIGHT);
        let (was, now) = (visible_weeks(wide), visible_weeks(narrow));
        assert!(now < was, "test premise: {now} should be under {was}");

        let app = App::new(vec![], "test".to_string(), 120, 40);
        // Settle at the wide size, as a real first frame would.
        let first = first_week(&app, was);
        app.contrib_visible_weeks.set(was);
        app.contrib_scroll.set(first);
        let right_edge = first + was;

        let after = first_week(&app, now);
        assert_eq!(
            after + now,
            right_edge,
            "the right edge moved: window was [{first}, {right_edge}), now [{after}, {})",
            after + now
        );
    }

    /// Growing the pane is the same rule seen from the other side: the newest
    /// week stays put against the right edge and the view reaches further
    /// back, rather than the grid sliding forward into weeks that do not exist
    /// yet.
    #[test]
    fn growing_the_pane_reaches_further_back_not_forward() {
        let narrow = Rect::new(0, 5, 70, CONTRIB_HEIGHT);
        let wide = Rect::new(0, 5, 120, CONTRIB_HEIGHT);
        let (was, now) = (visible_weeks(narrow), visible_weeks(wide));

        let app = App::new(vec![], "test".to_string(), 70, 40);
        let first = first_week(&app, was);
        app.contrib_visible_weeks.set(was);
        app.contrib_scroll.set(first);
        let right_edge = first + was;

        let after = first_week(&app, now);
        assert_eq!(
            after + now,
            right_edge,
            "the right edge moved while growing"
        );
    }

    /// A resize must not obscure the day you are standing on. Holding the
    /// right edge covers the common case, but a cursor parked in the oldest
    /// weeks would fall off the left instead -- the same bug mirrored.
    #[test]
    fn a_resize_keeps_the_marked_day_in_view() {
        let wide = Rect::new(0, 5, 120, CONTRIB_HEIGHT);
        let narrow = Rect::new(0, 5, 60, CONTRIB_HEIGHT);
        let (was, now) = (visible_weeks(wide), visible_weeks(narrow));

        let mut app = App::new(vec![], "test".to_string(), 120, 40);
        let first = first_week(&app, was);
        app.contrib_visible_weeks.set(was);
        app.contrib_scroll.set(first);
        // Park the cursor on the oldest week currently drawn.
        app.contrib_cursor = Some(first * 7);

        let after = first_week(&app, now);
        assert!(
            first >= after && first < after + now,
            "the marked week {first} fell outside [{after}, {})",
            after + now
        );
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

    fn app_with_calendar(total_per_day: u32) -> App {
        let mut app = App::new(Vec::new(), "test".to_string(), 120, 44);
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
    fn the_strip_draws_a_framed_pane_with_one_row_per_weekday() {
        let lines = drawn(120, &app_with_calendar(1));
        assert_eq!(lines.len(), CONTRIB_HEIGHT as usize);
        assert!(
            lines[0].contains("[4] contributions"),
            "title: {}",
            lines[0]
        );
        let header = &lines[HEADER_ROW];
        assert!(header.contains("Aug") || header.contains("Jul"), "{header}");
        assert!(!header.contains(CELL), "no grid cells in the label row");
        assert!(!header.contains(CARET), "no caret before travelling");
        // Weekday rows, counted off the first: Mon is 1, Wed 3, Fri 5.
        assert!(lines[FIRST_GRID_ROW + 1].contains("Mon "));
        assert!(lines[FIRST_GRID_ROW + 3].contains("Wed "));
        assert!(lines[FIRST_GRID_ROW + 5].contains("Fri "));
        for (weekday, line) in lines
            .iter()
            .enumerate()
            .skip(FIRST_GRID_ROW)
            .take(DAY_ROWS as usize)
        {
            assert!(line.contains(CELL), "row {weekday} has cells");
        }
        assert!(lines[LEGEND_ROW].contains("Less") && lines[LEGEND_ROW].contains("More"));
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
        let mut app = App::new(Vec::new(), "test".to_string(), 120, 44);
        app.contributions = ContributionsMode::Github;
        assert!(app.github_calendar.is_none());
        let lines = drawn(120, &app);
        assert!(lines[0].contains("[4] contributions"), "frame drawn");
        assert!(!lines[3].contains(CELL), "no cells painted: {}", lines[3]);
    }

    /// Nothing is highlighted until the user travels the grid. A box cannot
    /// be drawn around a one-cell icon, so the marked day is painted in plain
    /// white with a caret over its column.
    #[test]
    fn the_marked_day_is_flagged_by_a_caret_and_only_after_travelling() {
        let mut app = app_with_calendar(1);
        app.focused_panel = FocusedPanel::Contributions;
        app.contrib_scroll.set(WEEKS);
        assert_eq!(app.contrib_cursor, None);

        let bare = drawn(120, &app);
        assert!(
            !bare[HEADER_ROW].contains(CARET),
            "no caret before travelling: {}",
            bare[HEADER_ROW]
        );

        let last = app.contrib_last_slot(chrono::Local::now().date_naive());
        app.contrib_cursor = Some(last - 7 * 3 - 4);
        let lines = drawn(120, &app);
        let header = &lines[HEADER_ROW];
        assert!(header.contains(CARET), "caret drawn: {header}");

        // The caret sits directly over the marked day's column, in the same
        // row as the month labels: it shares that row rather than owning one.
        let caret_col = header.find(CARET).unwrap();
        let marked_row = FIRST_GRID_ROW + (last - 7 * 3 - 4) % 7;
        assert!(
            lines[marked_row].chars().count() > caret_col,
            "the marked row reaches the caret column"
        );
    }

    /// The marked day is painted in plain text white, which has to beat both
    /// intensity ladders: every other bright colour in the palette already
    /// means something (behind, failed, ahead).
    #[test]
    fn the_marked_day_is_painted_in_a_colour_no_ladder_uses() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = app_with_calendar(4);
        app.focused_panel = FocusedPanel::Contributions;
        app.contrib_scroll.set(0);
        app.contrib_cursor = Some(3 * 7); // week 3, a Sunday

        let mut t = Terminal::new(TestBackend::new(120, CONTRIB_HEIGHT)).unwrap();
        t.draw(|f| render(f, f.area(), &app)).unwrap();
        let buf = t.backend().buffer().clone();

        // Only grid cells count: the pane title is drawn in TEXT too when
        // the strip has focus.
        let marked = (0..120)
            .flat_map(|x| (0..CONTRIB_HEIGHT).map(move |y| (x, y)))
            .filter(|(x, y)| {
                buf[(*x, *y)].symbol() == CELL && buf[(*x, *y)].style().fg == Some(colors::TEXT)
            })
            .count();
        assert_eq!(marked, 1, "exactly one cell is painted as marked");

        for shade in colors::contrib_ladder(Source::Github) {
            assert_ne!(colors::TEXT, shade, "the mark cannot collide with a shade");
        }
        for shade in colors::contrib_ladder(Source::Local) {
            assert_ne!(colors::TEXT, shade);
        }
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
                    if buf[(x, y)].symbol() == CELL {
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

    /// Relative luminance, the standard weighting. Used to check that the
    /// ladder actually spans a range rather than five shades of the same.
    fn luminance(c: ratatui::style::Color) -> f64 {
        let ratatui::style::Color::Rgb(r, g, b) = c else {
            panic!("ladder colours are literal RGB")
        };
        0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b)
    }

    /// A busy day must be obviously different from a quiet one, not a
    /// slightly brighter version of it. The ladders that shipped before this
    /// interpolated between two palette colours and climbed by as little as
    /// 1.19x per step, which read as one flat tone.
    ///
    /// The thresholds are calibrated against github.com's own dark-dimmed
    /// ladder, which is what `CONTRIB_LADDER_GITHUB` is sampled from: it
    /// climbs at least 1.26x per step and spans 2.86x overall. An earlier
    /// version of this test demanded 3.0x, a number invented before that
    /// reference existed, and the real reference failed it.
    #[test]
    fn the_intensity_ladders_climb_at_a_visible_rate() {
        for source in [Source::Github, Source::Local] {
            let ladder = colors::contrib_ladder(source);
            let steps: Vec<f64> = ladder.iter().map(|c| luminance(*c)).collect();

            for step in 1..4 {
                let ratio = steps[step + 1] / steps[step];
                assert!(
                    ratio >= 1.25,
                    "{source:?} step {step} to {} climbs only {ratio:.2}x",
                    step + 1
                );
            }
            assert!(
                steps[4] / steps[1] >= 2.5,
                "{source:?} spans only {:.2}x from quietest to busiest",
                steps[4] / steps[1]
            );
            // The empty step is structure, not data: it must not read as a
            // quiet day, so the first real step clears it.
            assert!(
                steps[1] / steps[0] >= 1.2,
                "{source:?} step 1 is only {:.2}x the empty slate, so a whole \
                 intensity level disappears into it",
                steps[1] / steps[0]
            );
        }
    }

    /// The two ladders must climb at the same *rate*, or the same amount of
    /// work looks busier on one source than the other.
    ///
    /// This used to demand equal luminance step for step, which is stricter
    /// than the goal and turned out to be unsatisfiable in blue. Blue carries
    /// 0.0722 of luminance to green's 0.7152, so matching the green top's
    /// luminance of 202 forces a colour sRGB cannot make blue: the most
    /// saturated thing at that luminance is (166, 208, 255), chroma 89, which
    /// reads as paper. The blue therefore tops out lower and climbs 1.393x to
    /// the green's 1.490x.
    ///
    /// What is still pinned: the shared empty slate, an identical step 1 so a
    /// quiet day matches on both sources, and climb rates within 10% of each
    /// other. The ladders are never on screen together, so the reading that
    /// matters is each against itself.
    #[test]
    fn both_ladders_climb_at_the_same_rate() {
        let github: Vec<f64> = colors::contrib_ladder(Source::Github)
            .iter()
            .map(|c| luminance(*c))
            .collect();
        let local: Vec<f64> = colors::contrib_ladder(Source::Local)
            .iter()
            .map(|c| luminance(*c))
            .collect();
        for step in 0..2 {
            assert!(
                (github[step] - local[step]).abs() <= 2.0,
                "step {step} must match on both sources: github {:.0} vs local {:.0}",
                github[step],
                local[step]
            );
        }
        let rate = |l: &[f64]| (l[4] / l[1]).powf(1.0 / 3.0);
        let (g, b) = (rate(&github), rate(&local));
        assert!(
            (g - b).abs() / g <= 0.10,
            "climb rates diverge: github {g:.3}x vs local {b:.3}x per step"
        );
    }

    /// The ladder is tuned, not sampled, so pinning it is what stops a later
    /// tidy-up from drifting.
    ///
    /// It used to be sampled straight off github.com's dark-dimmed graph and
    /// this test guarded that correspondence. The sampled ramp decelerated
    /// (1.51x, 1.42x, 1.34x), so the three busiest levels were the closest
    /// together and the days worth spotting were the hardest to tell apart.
    /// Steps 0 and 1 are still GitHub's; steps 2 to 4 are solved for an even
    /// 1.49x climb. Changing them is a deliberate design decision, which is
    /// why it has to break a test to happen.
    #[test]
    fn the_github_ladder_is_pinned_to_its_tuned_values() {
        use ratatui::style::Color;
        assert_eq!(
            colors::contrib_ladder(Source::Github),
            [
                Color::Rgb(43, 49, 59),
                Color::Rgb(39, 70, 37),
                Color::Rgb(59, 120, 52),
                Color::Rgb(75, 176, 60),
                Color::Rgb(149, 224, 136),
            ]
        );
        // GitHub's own quiet day, kept: the bottom of the ramp is where the
        // sampled palette was already right.
        assert_eq!(
            colors::contrib_ladder(Source::Github)[1],
            Color::Rgb(39, 70, 37)
        );
    }

    /// CIELAB lightness: how light a colour *looks*, as opposed to how much
    /// light it emits. Luminance is linear in energy and the eye is not, which
    /// is the whole reason this function exists alongside `luminance`.
    fn perceptual_lightness(c: ratatui::style::Color) -> f64 {
        let ratatui::style::Color::Rgb(r, g, b) = c else {
            panic!("ladder colours are literal RGB")
        };
        let lin = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        let y = 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
        if y > 0.008856 {
            116.0 * y.cbrt() - 16.0
        } else {
            903.3 * y
        }
    }

    /// The four intensities must look evenly spaced, which means even in L*,
    /// not even in luminance.
    ///
    /// Both ladders this replaced got that wrong, in opposite directions. The
    /// original decelerated in luminance, so the busiest levels were the
    /// closest together. The fix for that made the luminance *ratio* even,
    /// which still left L* gaps of 13.7, 19.3, 23.0: the middles read as too
    /// dark and the jump to the top step was nearly twice the jump off the
    /// bottom. Luminance is linear in energy, the eye is not, and only the
    /// second of those is what a heatmap gets read by.
    #[test]
    fn the_ladders_are_evenly_spaced_to_the_eye() {
        for source in [Source::Github, Source::Local] {
            let ladder = colors::contrib_ladder(source);
            let steps: Vec<f64> = ladder.iter().map(|c| perceptual_lightness(*c)).collect();
            let gaps: Vec<f64> = (1..4).map(|i| steps[i + 1] - steps[i]).collect();
            let (lo, hi) = gaps
                .iter()
                .fold((f64::MAX, 0.0f64), |(l, h), g| (l.min(*g), h.max(*g)));
            assert!(
                hi / lo <= 1.10,
                "{source:?} is unevenly spaced to the eye: L* gaps {gaps:?}, \
                 widest/narrowest {:.2}",
                hi / lo
            );
        }
    }

    /// Distance from the grey axis: how much colour a step actually carries.
    /// A near-white and a near-grey both collapse to nearly zero here, which
    /// is what "washed out" means numerically.
    fn chroma(c: ratatui::style::Color) -> f64 {
        let ratatui::style::Color::Rgb(r, g, b) = c else {
            panic!("ladder colours are literal RGB")
        };
        f64::from(r.max(g).max(b)) - f64::from(r.min(g).min(b))
    }

    /// Matching the greens' luminance is only half the job: blue carries 0.072
    /// of luminance against green's 0.715, so a blue at a green's luminance
    /// sits far lighter, and at the green's *saturation* the busiest step came
    /// out (179, 205, 235), a near-white. Equal luminance and equal climb both
    /// still passed, because neither notices a colour turning into paper.
    ///
    /// So the blue must stay about as colourful as the green it mirrors. The
    /// washed-out step scored 0.64 of its green's chroma; every step now
    /// scores at least 1.0.
    #[test]
    fn the_local_ladder_is_as_colourful_as_the_green_it_mirrors() {
        let github = colors::contrib_ladder(Source::Github);
        let local = colors::contrib_ladder(Source::Local);
        for step in 1..5 {
            let ratio = chroma(local[step]) / chroma(github[step]);
            assert!(
                ratio >= 0.8,
                "local step {step} carries only {ratio:.2} of the green's colour, \
                 so it reads as washed out where the green does not"
            );
        }
    }

    /// An empty day looks the same whichever source is showing: "nothing
    /// happened" is not a property of where you counted it.
    #[test]
    fn an_empty_day_is_source_independent() {
        assert_eq!(
            colors::contrib_ladder(Source::Github)[0],
            colors::contrib_ladder(Source::Local)[0]
        );
    }
}
