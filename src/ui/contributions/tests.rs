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
        span.contains(&crate::contributions::YEARS.to_string()) || crate::contributions::YEARS == 1,
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

/// The strip opens with today marked (#66), and travelling moves the
/// mark. A box cannot be drawn around a one-cell icon, so the marked day
/// is painted in plain white with a caret over its column.
#[test]
fn the_marked_day_is_flagged_by_a_caret_from_the_moment_the_strip_opens() {
    let mut app = app_with_calendar(1);
    app.focused_panel = FocusedPanel::Contributions;
    app.apply_contributions_setting(ContributionsMode::Github);

    let opened = drawn(120, &app);
    assert!(
        opened[HEADER_ROW].contains(CARET),
        "caret drawn on open: {}",
        opened[HEADER_ROW]
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

/// Opening the strip marks today at every width: the caret sits over
/// today's column, which is inside the drawn window, and the title reads
/// today's count out (#66).
#[test]
fn an_opened_strip_marks_today_in_view_at_every_width() {
    let today = chrono::Local::now().date_naive();
    for width in [40u16, 80, 120, 260] {
        let mut app = app_with_calendar(1);
        app.contrib_scroll.set(0);
        app.apply_contributions_setting(ContributionsMode::Github);
        let last = app.contrib_last_slot(today);
        assert_eq!(app.contrib_cursor, Some(last), "width {width}");

        let lines = drawn(width, &app);
        let area = Rect::new(0, 0, width, CONTRIB_HEIGHT);
        let first = app.contrib_scroll.get();
        let visible = visible_weeks(area);
        let week = last / 7;
        assert!(
            week >= first && week < first + visible,
            "width {width}: today's week {week} outside [{first}, {})",
            first + visible
        );
        let expected =
            (FRAME_COLS + grid_pad(area) + GUTTER) as usize + (week - first) * WEEK_COLS as usize;
        let caret = lines[HEADER_ROW]
            .chars()
            .position(|c| c.to_string() == CARET);
        assert_eq!(
            caret,
            Some(expected),
            "width {width}: caret over today's column: {}",
            lines[HEADER_ROW]
        );
    }

    for width in [120u16, 260] {
        let mut app = app_with_calendar(1);
        app.apply_contributions_setting(ContributionsMode::Github);
        let top = &drawn(width, &app)[0];
        let when = today.format("%a %-d %b").to_string();
        assert!(
            top.contains(&format!("1 contribution on {when}")),
            "width {width}: top title reads today out: {top}"
        );
    }
}

/// Launch with the setting off: the strip starts folded and never opened,
/// so it has no cursor. Unfolding it must stand on today with the caret
/// over today's column, whichever way it is unfolded (#66).
fn assert_unfolds_onto_today(unfold: impl Fn(&mut App), path: &str) {
    let mut app = app_with_calendar(1);
    app.apply_contributions_setting(ContributionsMode::Off);
    assert!(
        app.contributions_folded,
        "{path}: premise, folded at launch"
    );
    assert_eq!(app.contrib_cursor, None, "{path}: premise, never opened");
    app.contrib_scroll.set(0);

    unfold(&mut app);
    assert!(!app.contributions_folded, "{path}: unfolded");
    let last = app.contrib_last_slot(chrono::Local::now().date_naive());
    assert_eq!(app.contrib_cursor, Some(last), "{path}: on today");

    let width = 120;
    let lines = drawn(width, &app);
    let area = Rect::new(0, 0, width, CONTRIB_HEIGHT);
    let first = app.contrib_scroll.get();
    let week = last / 7;
    assert!(
        week >= first && week < first + visible_weeks(area),
        "{path}: today's week in view"
    );
    let expected =
        (FRAME_COLS + grid_pad(area) + GUTTER) as usize + (week - first) * WEEK_COLS as usize;
    let caret = lines[HEADER_ROW]
        .chars()
        .position(|c| c.to_string() == CARET);
    assert_eq!(
        caret,
        Some(expected),
        "{path}: caret over today: {}",
        lines[HEADER_ROW]
    );
}

#[test]
fn the_4_key_unfolds_a_never_opened_strip_onto_today() {
    assert_unfolds_onto_today(
        |app| app.press_panel_key(FocusedPanel::Contributions),
        "4 key",
    );
}

#[test]
fn a_tag_click_unfolds_a_never_opened_strip_onto_today() {
    assert_unfolds_onto_today(
        |app| app.toggle_fold(FocusedPanel::Contributions),
        "tag click",
    );
}

/// The unfold scrolls to the newest weeks itself. With the window already
/// measured at this width, the first draw is not a resize, so nothing in
/// the renderer would pull a cursor back into view: today's column is on
/// screen only because unfolding asked for it.
#[test]
fn unfolding_onto_today_scrolls_today_into_view_without_a_resize() {
    let width = 120;
    let area = Rect::new(0, 0, width, CONTRIB_HEIGHT);
    let visible = visible_weeks(area);
    let mut app = app_with_calendar(1);
    app.apply_contributions_setting(ContributionsMode::Off);
    app.contrib_visible_weeks.set(visible);
    app.contrib_scroll.set(0);

    app.toggle_fold(FocusedPanel::Contributions);
    let lines = drawn(width, &app);
    let last = app.contrib_last_slot(chrono::Local::now().date_naive());
    let (first, week) = (app.contrib_scroll.get(), last / 7);
    assert!(
        week >= first && week < first + visible,
        "today's week {week} outside [{first}, {})",
        first + visible
    );
    assert!(
        lines[HEADER_ROW].contains(CARET),
        "caret drawn: {}",
        lines[HEADER_ROW]
    );
}

/// Folding gets the strip out of the way; it does not reset it. A
/// travelled day survives a fold and unfold by either path.
#[test]
fn a_travelled_strip_keeps_its_day_through_a_fold() {
    let mut app = app_with_calendar(1);
    app.apply_contributions_setting(ContributionsMode::Github);
    let last = app.contrib_last_slot(chrono::Local::now().date_naive());
    let travelled = last - 7 * 3 - 2;
    app.contrib_cursor = Some(travelled);

    app.toggle_fold(FocusedPanel::Contributions);
    assert!(app.contributions_folded);
    app.toggle_fold(FocusedPanel::Contributions);
    assert!(!app.contributions_folded);
    assert_eq!(
        app.contrib_cursor,
        Some(travelled),
        "tag click kept the day"
    );

    app.focused_panel = FocusedPanel::Contributions;
    app.press_panel_key(FocusedPanel::Contributions);
    assert!(app.contributions_folded);
    app.press_panel_key(FocusedPanel::Contributions);
    assert!(!app.contributions_folded);
    assert_eq!(app.contrib_cursor, Some(travelled), "4 key kept the day");
}

/// While travelling, the top title reads out the day under the cursor,
/// not the period total.
#[test]
fn the_top_title_follows_the_travelled_day() {
    let mut app = app_with_calendar(1);
    app.focused_panel = FocusedPanel::Contributions;
    app.apply_contributions_setting(ContributionsMode::Github);
    let today = chrono::Local::now().date_naive();
    app.move_contrib_cursor(-1, 0, today);
    let when = (today - chrono::Duration::days(7))
        .format("%a %-d %b")
        .to_string();
    let top = &drawn(120, &app)[0];
    assert!(
        top.contains(&format!("1 contribution on {when}")),
        "top title reads the travelled day: {top}"
    );
}

/// The text left of the legend on the bottom border, trimmed of border
/// and padding, and whether at least one border column still separates
/// it from the legend. A row with no legend is read whole, so whatever
/// overwrote the legend shows up as the label.
fn bottom_total(bottom: &str) -> (String, bool) {
    let after_corner: String = bottom.chars().skip(1).collect();
    let before_legend = after_corner
        .find(" Less")
        .map_or(after_corner.as_str(), |at| &after_corner[..at]);
    let label = before_legend
        .trim_matches(|c| c == '─' || c == ' ')
        .to_string();
    let clear = label.is_empty() || before_legend.trim_end_matches(' ').ends_with('─');
    (label, clear)
}

/// The period total moved from the top title to the bottom border, left
/// of the Less/More legend (#66, option b). It sheds words on the title's
/// ladder before it can touch the legend, and the legend always survives.
#[test]
fn the_period_total_rides_the_bottom_border_beside_the_legend() {
    let app = app_with_calendar(1);
    let today = chrono::Local::now().date_naive();
    let calendar = app.github_calendar.as_deref().unwrap();
    let total = thousands(calendar.total());
    let span = crate::contributions::span_label();
    let full = format!("{total} contributions in {span} (github)");
    let ladder = [
        full.clone(),
        format!("{total} in {span} (github)"),
        format!("{total} (github)"),
        total.clone(),
    ];
    assert_eq!(calendar.today_slot(), app.contrib_last_slot(today));

    for width in [120u16, 200, 260] {
        let lines = drawn(width, &app);
        assert_eq!(
            bottom_total(&lines[LEGEND_ROW]),
            (full.clone(), true),
            "width {width}: {}",
            lines[LEGEND_ROW]
        );
    }

    let legend_cols = legend_line(Source::Github, CELL).width() as u16;
    let mut shortened = false;
    // Narrower than the legend plus its corners, the legend itself is
    // clipped, as it was before the total moved here.
    for width in legend_cols + 2..=260 {
        let lines = drawn(width, &app);
        let bottom = &lines[LEGEND_ROW];
        let (label, clear) = bottom_total(bottom);
        assert!(
            clear,
            "width {width}: the total ran into the legend: {bottom}"
        );
        if !label.is_empty() {
            assert!(
                ladder.contains(&label),
                "width {width}: not a ladder step: {label:?}"
            );
            shortened |= label != full;
        }
        assert!(
            bottom.contains("Less") && bottom.contains("More"),
            "width {width}: legend lost: {bottom}"
        );
    }
    assert!(shortened, "no width exercised the shortening ladder");
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

/// FZ-91: both sources shade a day by `shade_scale` of its count, and a
/// GitHub day carrying a `contributionLevel` that disagrees is still
/// drawn by count.
#[test]
fn both_sources_shade_by_count_and_a_github_level_is_not_drawn() {
    let today = day("2026-08-18");
    let start = crate::contributions::grid_start(today);
    // Week 0, Sunday to Saturday: 1, 2, ..., 6 commits and an empty day.
    let counts = [1u32, 2, 3, 4, 5, 6, 0];
    for source in [Source::Github, Source::Local] {
        let mut cal = Calendar::new(today, source);
        for (weekday, n) in counts.iter().enumerate() {
            let date = start + chrono::Duration::days(weekday as i64);
            cal.set(date, *n);
            // GitHub's verdict, inverted against the counts.
            cal.set_level(date, 4 - (weekday as u8).min(4));
        }
        let scale = cal.shade_scale();
        let ladder = colors::contrib_ladder(source);
        for (weekday, n) in counts.iter().enumerate() {
            let line = day_line(&cal, 0, 1, weekday, None, 0, CELL);
            let cell = line.spans.iter().find(|s| s.content == CELL).unwrap();
            assert_eq!(
                cell.style.fg,
                Some(ladder[scale.step(*n) as usize]),
                "{source:?} weekday {weekday} ({n} commits) not drawn by count"
            );
        }
        let busiest = day_line(&cal, 0, 1, 5, None, 0, CELL);
        let cell = busiest.spans.iter().find(|s| s.content == CELL).unwrap();
        assert_eq!(cell.style.fg, Some(ladder[4]), "{source:?}: 6 is the top");
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
