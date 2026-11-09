//! The contribution strip: three years of activity docked under the main panes.
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
    dock_bottom(main, height)
}

/// Carve a band `height` rows tall off the bottom of `main`, with one blank
/// row above it, or nothing when that would leave the panes above fewer than
/// `MAIN_MIN_HEIGHT` rows.
pub(crate) fn dock_bottom(main: Rect, height: u16) -> (Rect, Option<Rect>) {
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

/// "1 contributions" is the kind of thing that makes a dashboard look
/// unfinished, and a one-commit day is common enough to hit often.
fn noun(n: u32) -> &'static str {
    if n == 1 {
        "contribution"
    } else {
        "contributions"
    }
}

/// The strip's title. With a day marked it reads that day out; otherwise it
/// falls back to `period_total`.
///
/// Shortens to fit `width` rather than letting the renderer clip it.
pub fn title(calendar: &Calendar, cursor: Option<usize>, width: u16) -> String {
    if let Some(slot) = cursor
        && let Some((date, count)) = calendar.day_at(slot / 7, slot % 7)
    {
        let when = date.format("%a %-d %b");
        let full = if count == 0 {
            format!("no contributions on {when}")
        } else {
            format!("{} {} on {when}", thousands(count), noun(count))
        };
        if full.chars().count() <= width as usize {
            return full;
        }
        return format!("{} on {when}", thousands(count));
    }
    period_total(calendar, width)
}

/// The total for the whole span and which source filled it, because the two
/// sources legitimately disagree. Drawn on the bottom border beside the
/// legend, and in the title when no day is marked.
///
/// Shortens to fit `width`. The source label is the last thing dropped: a
/// strip that does not say where its numbers came from is the one failure
/// this line exists to prevent. The bare total is returned even when it does
/// not fit; callers that must not overflow check the length.
pub fn period_total(calendar: &Calendar, width: u16) -> String {
    let width = width as usize;
    let source = calendar.source().label();
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
    let legend = legend_line(calendar.source(), glyph);
    // The total takes the left of the bottom border. Its budget leaves its own
    // two padding spaces and two border columns of clearance before the
    // legend, so it sheds words rather than running into it, and is dropped
    // outright when even the bare number would.
    let total_room = inner_width.saturating_sub(legend.width() as u16 + 2);
    let total = period_total(&calendar, total_room);
    let total = (total.chars().count() <= total_room as usize).then(|| {
        Line::from(Span::styled(
            format!(" {total} "),
            Style::default().fg(colors::OVERLAY),
        ))
    });
    let mut block = block
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
        );
    if let Some(total) = total {
        block = block.title_bottom(total);
    }
    frame.render_widget(
        Paragraph::new(lines).block(
            // The legend rides in the bottom border rather than costing a row
            // of its own, which is what the other panes do with their
            // secondary information.
            block.title_bottom(legend.right_aligned()),
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
/// They share it because a row of its own would hold the caret's single
/// column and nothing else, and a row that is blank but for one character
/// reads as a gap in the strip rather than as breathing room under the labels.
/// A label that would collide with the caret is dropped rather than drawn
/// through: the caret says which column you are on, which is the more urgent
/// of the two, and the title spells the date out in full regardless. A label
/// colliding with the label before it is dropped for the reason it always was.
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
    // One scale for both sources, by count: GitHub's `contributionLevel`
    // is not drawn (FZ-91).
    let scale = calendar.shade_scale();
    let mut spans = vec![
        Span::raw(" ".repeat(pad as usize)),
        Span::styled(label.to_string(), Style::default().fg(colors::OVERLAY)),
    ];

    for column in 0..visible {
        let week = first + column;
        match calendar.day_at(week, weekday) {
            Some((_, count)) => {
                // The marked day is painted in plain text white: it has to
                // beat both ladders, and every status colour in the palette
                // already means something else.
                let color = if marked == Some((column, weekday)) {
                    colors::TEXT
                } else {
                    ladder[scale.step(count) as usize]
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
mod tests;
