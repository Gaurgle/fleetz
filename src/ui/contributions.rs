//! The contribution strip: a year of activity docked under the main panes.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, ContributionsMode, FocusedPanel};
use crate::contributions::{Calendar, WEEKS};
use crate::ui::colors;

/// Rows the strip occupies: title, month labels, seven day rows, legend.
/// Fixed, because the content height is fixed: a resize handle here could
/// only ever add empty space.
pub const CONTRIB_HEIGHT: u16 = 10;

/// Rows the panes above must keep for the strip to be worth docking. Below
/// this the strip is dropped: a dashboard squeezed into slivers is worse
/// than no heatmap.
const MAIN_MIN_HEIGHT: u16 = 12;

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

/// Columns reserved for the Mon/Wed/Fri gutter.
const GUTTER: u16 = 4;

/// First of the seven day rows, counted from the strip's top edge. Row 0 is
/// the title and row 1 the month labels.
const FIRST_DAY_ROW: u16 = 2;

/// The cell glyph. Not a Nerd Font codepoint: this is the one part of the UI
/// that reads correctly without one.
const CELL: &str = "\u{25a0}";

/// How much room each week column gets, chosen from the strip's own width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Cell plus a gap, the closest to GitHub's separated squares.
    Wide,
    /// One cell per week: a solid ribbon, but the whole year fits.
    Dense,
    /// Dense, and only the newest N weeks fit at all.
    Trimmed(usize),
}

impl Tier {
    pub fn cell_width(self) -> u16 {
        match self {
            Self::Wide => 2,
            Self::Dense | Self::Trimmed(_) => 1,
        }
    }

    pub fn weeks_shown(self) -> usize {
        match self {
            Self::Wide | Self::Dense => WEEKS,
            Self::Trimmed(n) => n,
        }
    }
}

/// Pick a tier for a strip of this width. The wide tier drops the gap after
/// the final column, so it needs 109 rather than 110.
pub fn tier(width: u16) -> Tier {
    let year = WEEKS as u16;
    if width >= GUTTER + year * 2 - 1 {
        Tier::Wide
    } else if width >= GUTTER + year {
        Tier::Dense
    } else {
        Tier::Trimmed(width.saturating_sub(GUTTER) as usize)
    }
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

/// The strip's first line. Focused, it reads out the selected day; otherwise
/// it reports the span actually drawn and which source filled it, because the
/// two sources legitimately disagree.
///
/// Shortens to fit `width` rather than letting the renderer clip it. The
/// source label is the last thing dropped, and only once the line has nothing
/// else left to give: a strip that does not say where its numbers came from
/// is the one failure mode this whole line exists to prevent.
pub fn title(calendar: &Calendar, tier: Tier, cursor: Option<usize>, width: u16) -> String {
    let width = width as usize;
    let source = calendar.source().label();

    if let Some(slot) = cursor
        && let Some((date, count)) = calendar.day_at(slot / 7, slot % 7)
    {
        let when = date.format("%a %-d %b");
        let full = if count == 0 {
            format!("no contributions on {when}")
        } else {
            format!("{} contributions on {when}", thousands(count))
        };
        if full.chars().count() <= width {
            return full;
        }
        return format!("{} on {when}", thousands(count));
    }

    let total = thousands(calendar.total());
    let span = match tier {
        Tier::Wide | Tier::Dense => "the last year".to_string(),
        Tier::Trimmed(n) => format!("the last {n} weeks"),
    };
    for candidate in [
        format!("{total} contributions in {span} ({source})"),
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
/// label rows, and anything past the last drawn week.
pub fn slot_at(area: Rect, tier: Tier, col: u16, row: u16) -> Option<usize> {
    let weekday = row.checked_sub(area.y + FIRST_DAY_ROW)?;
    if weekday >= 7 {
        return None;
    }
    let offset = col.checked_sub(area.x + GUTTER)?;
    let cell = tier.cell_width();
    if cell == 2 && !offset.is_multiple_of(2) {
        return None; // the gap between two columns is not a day
    }
    let drawn = (offset / cell) as usize;
    let shown = tier.weeks_shown();
    if drawn >= shown {
        return None;
    }
    // A trimmed grid shows the NEWEST weeks, so column 0 is not week 0.
    Some((WEEKS - shown + drawn) * 7 + weekday as usize)
}

/// The first drawn week, given that a trimmed grid shows the newest ones.
fn first_week(tier: Tier) -> usize {
    WEEKS - tier.weeks_shown()
}

/// Draw the strip: title, month labels, seven day rows, legend.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let today = chrono::Local::now().date_naive();
    let Some(calendar) = calendar_for(app, today) else {
        return;
    };
    let tier = tier(area.width);
    let focused = app.focused_panel == FocusedPanel::Contributions;
    let cursor = focused.then(|| app.contrib_slot.min(calendar.today_slot()));

    let mut lines = Vec::with_capacity(CONTRIB_HEIGHT as usize);
    lines.push(Line::from(Span::styled(
        title(&calendar, tier, cursor, area.width),
        Style::default()
            .fg(if focused {
                colors::TEXT
            } else {
                colors::SUBTEXT
            })
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(month_line(&calendar, tier));
    for weekday in 0..7 {
        lines.push(day_line(&calendar, tier, weekday, cursor));
    }
    lines.push(legend_line());

    frame.render_widget(Paragraph::new(lines), area);
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

fn month_line(calendar: &Calendar, tier: Tier) -> Line<'static> {
    let cell = tier.cell_width() as usize;
    let start = first_week(tier);
    let width = GUTTER as usize + tier.weeks_shown() * cell;
    let mut row = " ".repeat(GUTTER as usize);
    for (week, name) in calendar.month_labels() {
        if week < start {
            continue;
        }
        let at = GUTTER as usize + (week - start) * cell;
        // Skip a label that would overwrite the previous one or run off the
        // end: a half-drawn month name is noise, not information.
        if at < row.chars().count() || at + name.len() > width {
            continue;
        }
        row.push_str(&" ".repeat(at - row.chars().count()));
        row.push_str(name);
    }
    Line::from(Span::styled(row, Style::default().fg(colors::OVERLAY)))
}

fn day_line(
    calendar: &Calendar,
    tier: Tier,
    weekday: usize,
    cursor: Option<usize>,
) -> Line<'static> {
    // Only three of the seven rows are labelled, as GitHub does: seven labels
    // is a wall of text beside a picture.
    let label = match weekday {
        1 => "Mon ",
        3 => "Wed ",
        5 => "Fri ",
        _ => "    ",
    };
    let mut spans = vec![Span::styled(
        label.to_string(),
        Style::default().fg(colors::OVERLAY),
    )];
    let start = first_week(tier);
    for week in start..start + tier.weeks_shown() {
        let slot = week * 7 + weekday;
        let Some((_, count)) = calendar.day_at(week, weekday) else {
            // Past today: hold the column open so the grid keeps its shape.
            spans.push(Span::raw(" ".repeat(tier.cell_width() as usize)));
            continue;
        };
        let mut style =
            Style::default().fg(colors::CONTRIB_LADDER[calendar.intensity(count) as usize]);
        if cursor == Some(slot) {
            style = style.add_modifier(Modifier::REVERSED);
        }
        spans.push(Span::styled(CELL, style));
        if tier.cell_width() == 2 {
            spans.push(Span::raw(" "));
        }
    }
    Line::from(spans)
}

fn legend_line() -> Line<'static> {
    let mut spans = vec![Span::styled(
        "    Less ".to_string(),
        Style::default().fg(colors::OVERLAY),
    )];
    for shade in colors::CONTRIB_LADDER {
        spans.push(Span::styled(CELL, Style::default().fg(shade)));
    }
    spans.push(Span::styled(
        " More".to_string(),
        Style::default().fg(colors::OVERLAY),
    ));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(height: u16) -> Rect {
        Rect::new(0, 1, 120, height)
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
        assert!(main.height < 40, "the strip costs the panes rows");
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
        assert!(split_main_area(area(22), true).1.is_none());
        assert!(split_main_area(area(23), true).1.is_some());
    }

    use crate::contributions::{Calendar, Source, WEEKS};

    fn day(s: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// A year is 53 columns. Wide spends two cells on each and drops the gap
    /// after the last one; dense spends one. Both sit behind a 4-column day
    /// gutter. Getting these off by one silently crops the oldest week.
    #[test]
    fn tiers_switch_at_the_widths_they_actually_need() {
        assert!(matches!(tier(109), Tier::Wide));
        assert!(matches!(tier(108), Tier::Dense), "one column short of wide");
        assert!(matches!(tier(57), Tier::Dense));
        assert!(matches!(tier(56), Tier::Trimmed(52)), "one short of dense");
        assert!(matches!(tier(30), Tier::Trimmed(26)));
        assert!(matches!(tier(4), Tier::Trimmed(0)));
        assert!(matches!(tier(0), Tier::Trimmed(0)), "never underflows");
    }

    #[test]
    fn tier_cell_widths_and_week_counts_agree() {
        assert_eq!(tier(120).cell_width(), 2);
        assert_eq!(tier(120).weeks_shown(), WEEKS);
        assert_eq!(tier(60).cell_width(), 1);
        assert_eq!(tier(60).weeks_shown(), WEEKS);
        assert_eq!(tier(30).weeks_shown(), 26);
    }

    #[test]
    fn thousands_groups_the_total_the_way_github_does() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(7), "7");
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
            title(&cal, tier(120), None, 120),
            "3 contributions in the last year (github)"
        );
    }

    /// A trimmed grid must not be presented as a full year: the title says
    /// how much is actually on screen.
    #[test]
    fn a_trimmed_title_names_the_weeks_it_can_draw() {
        let cal = Calendar::new(day("2026-08-18"), Source::Local);
        assert_eq!(
            title(&cal, tier(30), None, 120),
            "0 contributions in the last 26 weeks (local)"
        );
    }

    #[test]
    fn a_focused_title_reads_out_the_selected_day() {
        let mut cal = Calendar::new(day("2026-08-18"), Source::Local);
        cal.add(day("2025-08-19"), 7);
        assert_eq!(
            title(&cal, tier(120), Some(2), 120),
            "7 contributions on Tue 19 Aug"
        );
    }

    #[test]
    fn a_focused_title_says_none_rather_than_zero_contributions() {
        let cal = Calendar::new(day("2026-08-18"), Source::Local);
        assert_eq!(
            title(&cal, tier(120), Some(2), 120),
            "no contributions on Tue 19 Aug"
        );
    }

    #[test]
    fn a_click_maps_back_to_the_slot_under_it() {
        let area = Rect::new(0, 5, 120, CONTRIB_HEIGHT);
        let t = tier(area.width);
        // Row 0 is the title, row 1 the month labels, rows 2..9 the days.
        // Column 4 is the first week, and each week is two cells wide.
        assert_eq!(slot_at(area, t, 4, 7), Some(0), "first Sunday");
        assert_eq!(slot_at(area, t, 4, 8), Some(1), "the Monday under it");
        assert_eq!(slot_at(area, t, 6, 7), Some(7), "next week's Sunday");
        assert_eq!(slot_at(area, t, 5, 7), None, "the gap between columns");
        assert_eq!(slot_at(area, t, 2, 7), None, "the day gutter");
        assert_eq!(slot_at(area, t, 4, 5), None, "the title row");
        assert_eq!(slot_at(area, t, 119, 7), None, "past the last week");
    }

    /// A trimmed grid draws the NEWEST weeks, so its leftmost column is not
    /// week 0. Clicking it must not report a day from last August.
    #[test]
    fn a_click_on_a_trimmed_grid_accounts_for_the_dropped_weeks() {
        let area = Rect::new(0, 5, 30, CONTRIB_HEIGHT);
        let t = tier(area.width);
        assert_eq!(t.weeks_shown(), 26);
        assert_eq!(slot_at(area, t, 4, 7), Some((WEEKS - 26) * 7));
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

    fn app_with_local_history() -> App {
        let mut app = App::new(Vec::new(), "test".to_string(), false, 120, 44);
        app.contributions = ContributionsMode::Local;
        app
    }

    #[test]
    fn the_strip_draws_a_gutter_seven_day_rows_and_a_legend() {
        let lines = drawn(120, &app_with_local_history());
        assert_eq!(lines.len(), CONTRIB_HEIGHT as usize);
        assert!(lines[0].starts_with("0 contributions in the last year (local)"));
        assert!(lines[1].contains("Aug"), "month labels: {}", lines[1]);
        assert!(lines[2].starts_with("    "), "Sunday row is unlabelled");
        assert!(lines[3].starts_with("Mon "));
        assert!(lines[5].starts_with("Wed "));
        assert!(lines[7].starts_with("Fri "));
        assert!(lines[9].contains("Less") && lines[9].contains("More"));
    }

    /// The grid must never spill past the strip's right edge, at any width.
    #[test]
    fn no_row_overflows_the_strip_at_any_tier() {
        for width in [40u16, 57, 80, 108, 109, 120, 200] {
            for line in drawn(width, &app_with_local_history()) {
                assert!(
                    line.chars().count() <= width as usize,
                    "width {width} produced a {}-char line: {line}",
                    line.chars().count()
                );
            }
        }
    }

    /// The GitHub source with no poll yet has nothing to draw. It must leave
    /// the strip blank rather than render a misleading empty year.
    #[test]
    fn the_github_source_draws_nothing_before_the_first_poll() {
        let mut app = App::new(Vec::new(), "test".to_string(), false, 120, 44);
        app.contributions = ContributionsMode::Github;
        assert!(app.github_calendar.is_none());
        assert!(drawn(120, &app).iter().all(|l| l.is_empty()));
    }

    /// A narrow strip clipped the source label off the end, which is the one
    /// word that says whether you are looking at GitHub's count or your own.
    /// The title now sheds words instead, keeping the source to the last.
    #[test]
    fn a_narrow_title_sheds_words_before_it_sheds_the_source() {
        let cal = Calendar::new(day("2026-08-18"), Source::Github);
        let narrow = title(&cal, tier(40), None, 40);
        assert!(
            narrow.chars().count() <= 40,
            "still {} chars: {narrow}",
            narrow.chars().count()
        );
        assert!(narrow.contains("(github)"), "source survived: {narrow}");

        // Absurdly narrow: the count alone is all that fits.
        assert_eq!(title(&cal, tier(12), None, 8), "0");
    }
}
