use crate::app::tests::{make_app, today};
use crate::app::*;

#[test]
fn c_switches_the_source_while_the_strip_is_up() {
    let mut app = make_app(1);
    assert_eq!(
        app.contrib.mode,
        ContributionsMode::Github,
        "defaults to github"
    );
    app.cycle_contributions_source();
    assert_eq!(app.contrib.mode, ContributionsMode::Local);
    app.cycle_contributions_source();
    assert_eq!(app.contrib.mode, ContributionsMode::Github);
}

/// The grid reads as columns of weeks and rows of weekdays, so the keys
/// must move that way: left/right change the week, up/down change the
/// weekday. They were wired the other way round at first, which moved the
/// cursor at right angles to what the eye expected.
#[test]
fn left_right_walks_weeks_and_up_down_walks_weekdays() {
    let mut app = make_app(1);
    let last = app.contrib_last_slot(today());
    app.contrib.cursor = Some(last);

    app.move_contrib_cursor(-1, 0, today());
    assert_eq!(
        app.contrib.cursor,
        Some(last - 7),
        "left is one column back, same weekday"
    );

    app.move_contrib_cursor(0, -1, today());
    assert_eq!(
        app.contrib.cursor,
        Some(last - 8),
        "up is one row, same column"
    );

    app.move_contrib_cursor(1, 0, today());
    assert_eq!(app.contrib.cursor, Some(last - 1), "right returns");
}

/// A source change reopens the strip, so it lands on today as well, even
/// after the user travelled away.
#[test]
fn a_source_change_leaves_the_cursor_on_today() {
    let mut app = make_app(1);
    app.contrib.cursor = Some(0);
    app.cycle_contributions_source();
    assert_eq!(app.contrib.mode, ContributionsMode::Local);
    let today = chrono::Local::now().date_naive();
    assert_eq!(app.contrib.cursor, Some(app.contrib_last_slot(today)));

    app.contrib.cursor = Some(0);
    app.cycle_contributions_source();
    assert_eq!(app.contrib.mode, ContributionsMode::Github);
    assert_eq!(app.contrib.cursor, Some(app.contrib_last_slot(today)));
}

/// Walking up a column must not spill into the neighbouring week, and
/// walking right must not walk into the future.
#[test]
fn each_axis_clamps_without_spilling_into_the_other() {
    let mut app = make_app(1);
    let last = app.contrib_last_slot(today());
    let (last_week, last_weekday) = (last / 7, last % 7);

    // Top of a column: further up is a no-op, not a jump to last week.
    app.contrib.cursor = Some(last_week * 7);
    app.move_contrib_cursor(0, -1, today());
    assert_eq!(app.contrib.cursor, Some(last_week * 7));

    // 2026-08-18 is a Tuesday, so the newest column ends there.
    app.contrib.cursor = Some(last);
    app.move_contrib_cursor(0, 1, today());
    assert_eq!(
        app.contrib.cursor,
        Some(last),
        "cannot walk into the future"
    );

    app.move_contrib_cursor(1, 0, today());
    assert_eq!(app.contrib.cursor, Some(last), "no column after this one");

    // Entering the short final column from a later weekday slides up to
    // today rather than landing on a cell that does not exist.
    app.contrib.cursor = Some((last_week - 1) * 7 + 6);
    app.move_contrib_cursor(1, 0, today());
    assert_eq!(app.contrib.cursor, Some(last_week * 7 + last_weekday));

    // The oldest column is a hard left edge.
    app.contrib.cursor = Some(3);
    app.move_contrib_cursor(-1, 0, today());
    assert_eq!(app.contrib.cursor, Some(3));
}

#[test]
fn home_and_end_jump_to_the_ends_keeping_the_weekday() {
    let mut app = make_app(1);
    let last = app.contrib_last_slot(today());
    app.contrib.cursor = Some(20 * 7 + 4); // a Thursday mid-year

    app.jump_contrib_cursor(false, today());
    assert_eq!(app.contrib.cursor, Some(4), "oldest column, same weekday");

    app.jump_contrib_cursor(true, today());
    assert_eq!(
        app.contrib.cursor,
        Some(last),
        "newest column clamps to today"
    );
}

#[test]
fn a_repo_with_no_histogram_contributes_nothing() {
    let app = make_app(1);
    assert_eq!(app.local_calendar(today()).total(), 0);
    assert_eq!(
        app.local_calendar(today()).source(),
        crate::contributions::Source::Local
    );
}
