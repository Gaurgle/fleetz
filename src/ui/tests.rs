use super::author_code;
use super::*;

fn rects() -> PanelRects {
    // 100x40 main area, repo list 35 wide, inspector 16 tall across the
    // full width at the bottom; the list and the detail pane share the
    // 24 rows above it.
    let main = Rect::new(0, 1, 100, 40);
    PanelRects {
        main,
        // No strip in this fixture, so the splits get the whole area.
        body: main,
        repo_list: Rect::new(0, 1, 35, 24),
        detail: Some(Rect::new(36, 1, 64, 24)),
        inspector: Some(Rect::new(0, 25, 100, 16)),
        inspector_folded: false,
        reader: None,
        contributions: None,
    }
}

fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> chrono::DateTime<chrono::Local> {
    use chrono::TimeZone;
    chrono::Local
        .with_ymd_and_hms(y, m, d, h, min, 0)
        .single()
        .expect("unambiguous local time")
}

/// The rendered status line, border and all.
fn header_row(header: &str, width: u16) -> String {
    header_row_with(header, width, |_| {})
}

/// The same, with a hook to set app state the header reads.
fn header_row_with(header: &str, width: u16, setup: impl FnOnce(&mut crate::app::App)) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = crate::app::App::new(Vec::new(), header.to_string(), width, 40);
    setup(&mut app);
    let app = app;
    let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
    terminal
        .draw(|f| render_status_line(f, f.area(), &app))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..width)
        .map(|x| buffer[(x, 0)].symbol())
        .collect::<String>()
}

/// The foreground colour of the `watch` indicator's glyph at a given blink
/// phase.
fn watch_glyph_color(blink_on: bool) -> ratatui::style::Color {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let width = 80u16;
    let mut app = crate::app::App::new(Vec::new(), "t".to_string(), width, 40);
    app.watch_mode = true;
    app.blink_on = blink_on;
    let app = app;
    let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
    terminal
        .draw(|f| render_status_line(f, f.area(), &app))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let x = (0..width)
        .find(|x| buffer[(*x, 0)].symbol() == WATCH_ON_GLYPH)
        .expect("watch indicator not drawn");
    buffer[(x, 0)]
        .style()
        .fg
        .expect("watch glyph has no colour")
}

/// The foreground colour of the `watch` label's first letter, at a given
/// blink phase.
fn watch_label_color(blink_on: bool) -> ratatui::style::Color {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let width = 80u16;
    let mut app = crate::app::App::new(Vec::new(), "t".to_string(), width, 40);
    app.watch_mode = true;
    app.blink_on = blink_on;
    let app = app;
    let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
    terminal
        .draw(|f| render_status_line(f, f.area(), &app))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let lamp = (0..width)
        .find(|x| buffer[(*x, 0)].symbol() == WATCH_ON_GLYPH)
        .expect("watch indicator not drawn");
    // The first cell of "watch", one space past the lamp.
    buffer[(lamp + 2, 0)]
        .style()
        .fg
        .expect("watch label has no colour")
}

/// The house rule bans em and en dashes, and the header drew an em dash
/// between the version and the window label.
#[test]
fn header_draws_no_em_or_en_dash() {
    let row = header_row("last 7 days", 120);
    assert!(row.contains("last 7 days"), "the window label is drawn");
    assert!(
        !row.contains(['\u{2013}', '\u{2014}']),
        "header has a dash character: {row:?}"
    );
}

/// Only the lamp moves. Same rule as `spinner_group`: a word that pulses
/// along with the glyph beside it is motion where the eye is trying to
/// read, and the light is what the pulse is meant to draw attention to.
#[test]
fn only_the_watch_lamp_pulses_not_its_label() {
    assert_ne!(
        watch_glyph_color(true),
        watch_glyph_color(false),
        "the lamp is not pulsing"
    );
    assert_eq!(
        watch_label_color(true),
        watch_label_color(false),
        "the label pulsed along with the lamp"
    );
    assert_eq!(watch_label_color(true), colors::WATCH_ON);
}

/// The pulse is two neighbouring reds. It used to be one colour and two
/// disk shapes; the shape change was the harder of the two to catch at the
/// edge of vision, which is the only place this indicator is read.
#[test]
fn the_watch_indicator_pulses_between_two_reds() {
    assert_eq!(watch_glyph_color(true), colors::WATCH_ON);
    assert_eq!(watch_glyph_color(false), colors::WATCH_OFF);
    assert_ne!(
        colors::WATCH_ON,
        colors::WATCH_OFF,
        "a pulse needs two phases"
    );
}

/// Both phases have to still read as red, or the indicator changes hue
/// rather than pulsing.
#[test]
fn both_watch_phases_are_red_dominant() {
    for (phase, color) in [("on", colors::WATCH_ON), ("off", colors::WATCH_OFF)] {
        let ratatui::style::Color::Rgb(r, g, b) = color else {
            panic!("{phase} phase is not an RGB colour");
        };
        assert!(r > g && r > b, "{phase} phase is not red-dominant");
    }
}

/// Colour only: the header's fit loop lays out spans by width, so a phase
/// that drew a different glyph could shift what sits beside it.
#[test]
fn both_watch_phases_draw_the_same_text() {
    let lit = header_row_with("t", 80, |a| {
        a.watch_mode = true;
        a.blink_on = true;
    });
    let dim = header_row_with("t", 80, |a| {
        a.watch_mode = true;
        a.blink_on = false;
    });
    assert_eq!(lit, dim, "the phases differ in colour, not in text");
    assert!(
        lit.contains(&format!("{WATCH_ON_GLYPH} watch")),
        "got {lit:?}"
    );
}

/// Off is shown, not omitted: the struck-through eye, never the open one,
/// and in a colour that does not pulse or read as the live red.
#[test]
fn watch_off_draws_a_crossed_out_eye() {
    let width = 80u16;
    let row = header_row_with("t", width, |a| a.watch_mode = false);
    assert!(
        row.contains(&format!("{WATCH_OFF_GLYPH} watch")),
        "got {row:?}"
    );
    assert!(
        !row.contains(WATCH_ON_GLYPH),
        "the open eye drawn while off"
    );

    for blink_on in [true, false] {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut app = crate::app::App::new(Vec::new(), "t".to_string(), width, 40);
        app.blink_on = blink_on;
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal
            .draw(|f| render_status_line(f, f.area(), &app))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let x = (0..width)
            .find(|x| buffer[(*x, 0)].symbol() == WATCH_OFF_GLYPH)
            .expect("watch-off indicator not drawn");
        assert_eq!(buffer[(x, 0)].style().fg, Some(colors::OVERLAY));
    }
}

#[test]
fn the_window_key_sits_on_the_window_it_edits() {
    // The only key hint in the header, and it earns its place by being
    // attached to the state it changes.
    let row = header_row("since 5 days", 100);
    assert!(row.contains("since 5 days [t]"), "got {row:?}");
}

#[test]
fn global_action_keys_are_not_in_the_header() {
    // They live in the footer with the rest. `[v]` used to appear in both,
    // which is the duplication this guards against coming back.
    let row = header_row("since 5 days", 100);
    assert!(
        !row.contains("[v]"),
        "view hint belongs to the footer: {row:?}"
    );
    assert!(
        !row.contains("[?]"),
        "help hint belongs to the footer: {row:?}"
    );
}

#[test]
fn the_window_key_goes_with_the_label_it_annotates() {
    // A bare `[t]` next to nothing explains nothing, so it is dropped
    // whenever the header text it points at is absent.
    let row = header_row("", 100);
    assert!(!row.contains("[t]"), "got {row:?}");
}

/// A manual `r` says which half it is on: the local re-read is quick, the
/// GitHub poll after it takes seconds, and one word for both made the
/// fast part look slow.
#[test]
fn a_manual_refresh_names_the_half_it_is_on() {
    let local = header_row_with("h", 120, |app| {
        app.sweep_started = Some(std::time::Instant::now());
        app.sweep_manual = true;
        app.refresh_awaits_poll = true;
    });
    assert!(local.contains("checking local changes"), "{local}");
    assert!(!local.contains("checking remote"), "{local}");

    let remote = header_row_with("h", 120, |app| app.refresh_awaits_poll = true);
    assert!(remote.contains("checking remote"), "{remote}");
    assert!(!remote.contains("checking local"), "{remote}");

    let fetching = header_row_with("h", 120, |app| {
        app.refresh_fetches.insert("~/repos/a".to_string());
    });
    assert!(fetching.contains("checking remote"), "{fetching}");
}

#[test]
fn a_sweep_today_shows_only_the_clock_time() {
    let ts = at(2026, 8, 6, 11, 42);
    let now = at(2026, 8, 6, 14, 3);
    assert_eq!(swept_label(ts, now), "11:42");
}

#[test]
fn a_sweep_before_today_shows_its_date() {
    // The tmux-across-midnight case: a bare "23:11" reads as today and
    // would silently mean yesterday.
    let ts = at(2026, 8, 5, 23, 11);
    let now = at(2026, 8, 6, 9, 0);
    assert_eq!(swept_label(ts, now), "Aug 5 23:11");
}

#[test]
fn the_label_is_stable_as_time_passes() {
    // The whole point of dropping the relative age: the same sweep renders
    // identically an hour later, so nothing has to repaint to stay true.
    let ts = at(2026, 8, 6, 11, 42);
    assert_eq!(
        swept_label(ts, at(2026, 8, 6, 11, 43)),
        swept_label(ts, at(2026, 8, 6, 18, 30))
    );
}

#[test]
fn overdue_needs_watch_mode_and_two_intervals() {
    let ts = at(2026, 8, 6, 11, 0);
    let interval = 60;

    // Not watching: no promised cadence, so never late.
    assert!(!sweep_overdue(false, interval, ts, at(2026, 8, 6, 12, 0)));
    // Watching, but inside the grace window.
    assert!(!sweep_overdue(true, interval, ts, at(2026, 8, 6, 11, 1)));
    // Watching and past two intervals.
    assert!(sweep_overdue(true, interval, ts, at(2026, 8, 6, 11, 5)));
}

#[test]
fn overdue_scales_with_the_configured_interval() {
    // A five-minute interval must not be judged against a one-minute one.
    let ts = at(2026, 8, 6, 11, 0);
    let now = at(2026, 8, 6, 11, 8);
    assert!(sweep_overdue(true, 60, ts, now), "60s interval is late");
    assert!(!sweep_overdue(true, 600, ts, now), "600s interval is not");
}

#[test]
fn spinner_hole_travels_clockwise() {
    // Every frame is a full cell minus exactly one dot; the dot's bit
    // position is what the eye tracks. Braille bits: 1,2,3 are the left
    // column top-to-bottom, 4,5,6 the right, 7 and 8 the bottom row.
    // Clockwise from the top-left means right column down, bottom, then
    // left column up.
    let missing: Vec<u32> = SPINNER
        .iter()
        .map(|f| {
            let cell = f.chars().next().unwrap() as u32 - 0x2800;
            (0xFF ^ cell).trailing_zeros() + 1 // 1-indexed dot number
        })
        .collect();
    assert_eq!(missing, vec![4, 5, 6, 8, 7, 3, 2, 1]);
}

#[test]
fn every_spinner_frame_is_one_cell_wide() {
    // A wider frame would jitter the header on every tick.
    for f in SPINNER {
        assert_eq!(f.chars().count(), 1, "{f:?}");
    }
}

#[test]
fn only_the_spinner_glyph_fades_not_its_label() {
    // The glyph drifts through the palette; the word beside it must hold
    // one steady color so it stays legible at every point in the cycle.
    let label_color = |frame: usize| spinner_group(frame, "loading")[1].style.fg;
    let glyph_color = |frame: usize| spinner_group(frame, "loading")[0].style.fg;

    // Two frames a full leg apart, so the glyph is definitely a different
    // hue by the second one.
    assert_ne!(glyph_color(0), glyph_color(SPINNER_HUE_TICKS));
    assert_eq!(label_color(0), label_color(SPINNER_HUE_TICKS));
    assert_eq!(label_color(0), Some(colors::PEACH));
}

#[test]
fn spinner_group_keeps_the_glyph_and_label_separate() {
    let spans = spinner_group(0, "sweeping");
    assert_eq!(
        spans.len(),
        2,
        "one span each, so they can be colored apart"
    );
    assert!(spans[0].content.contains(SPINNER[0]));
    assert!(spans[1].content.contains("sweeping"));
    // Together they must still read as one phrase with its old spacing.
    let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(joined, format!(" {} sweeping ", SPINNER[0]));
}

#[test]
fn spinner_color_walks_the_palette_and_wraps() {
    // Lands exactly on each hue at the start of its leg...
    for (i, hue) in SPINNER_HUES.iter().enumerate() {
        assert_eq!(spinner_color(i * SPINNER_HUE_TICKS), *hue, "leg {i}");
    }
    // ...and returns to the first after a full loop, so there's no seam.
    assert_eq!(
        spinner_color(SPINNER_HUES.len() * SPINNER_HUE_TICKS),
        SPINNER_HUES[0]
    );
}

#[test]
fn spinner_color_is_between_its_two_endpoints_mid_leg() {
    // Midway from LAVENDER to SAPPHIRE: every channel must sit between
    // the two, which is what makes it read as a fade rather than a jump.
    let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) = (SPINNER_HUES[0], SPINNER_HUES[1])
    else {
        panic!("palette must be RGB to interpolate");
    };
    let Color::Rgb(r, g, b) = spinner_color(SPINNER_HUE_TICKS / 2) else {
        panic!("blend of two RGB colors must be RGB");
    };
    assert!(r >= ar.min(br) && r <= ar.max(br), "red {r}");
    assert!(g >= ag.min(bg) && g <= ag.max(bg), "green {g}");
    assert!(b >= ab.min(bb) && b <= ab.max(bb), "blue {b}");
}

#[test]
fn an_empty_query_renders_nothing_before_the_caret() {
    // The reported gap: an empty query used to emit a lone space, so `/`
    // sat one cell away from its own block cursor.
    assert!(query_spans("").is_empty());
    // And a typed query must not trail a space into the caret either.
    let text: String = query_spans("fle")
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(text, "fle");
}

#[test]
fn query_spans_keep_interior_spacing() {
    let text: String = query_spans("a #dirty")
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>();
    assert_eq!(text, "a #dirty");
}

#[test]
fn vertical_divider_is_grabbable_with_slop() {
    let r = rects();
    // The gap itself is one cell; a single-cell target is unhittable, so
    // one column either side counts too.
    assert_eq!(r.divider_at(35, 10), Some(Divider::Vertical));
    assert_eq!(r.divider_at(34, 10), Some(Divider::Vertical));
    assert_eq!(r.divider_at(36, 10), Some(Divider::Vertical));
    assert_eq!(r.divider_at(20, 10), None, "well inside the repo list");
}

#[test]
fn horizontal_divider_is_grabbable_across_the_full_width() {
    let r = rects();
    assert_eq!(r.divider_at(60, 25), Some(Divider::Horizontal));
    // The inspector spans under the repo list too, so its top edge is
    // grabbable there as well.
    assert_eq!(r.divider_at(10, 25), Some(Divider::Horizontal));
    assert_eq!(r.divider_at(10, 12), None, "well inside the repo list");
}

#[test]
fn a_click_outside_the_main_area_grabs_nothing() {
    let r = rects();
    assert_eq!(r.divider_at(35, 0), None, "header row");
    assert_eq!(r.divider_at(35, 99), None, "below the panels");
}

#[test]
fn drag_position_converts_back_to_a_percentage() {
    let r = rects();
    assert_eq!(r.repo_list_pct_at(50), 50, "midpoint of a 100-wide area");
    assert_eq!(r.repo_list_pct_at(25), 25);
    assert_eq!(r.repo_list_pct_at(0), 0, "clamping is the caller's job");

    // The inspector is docked at the bottom of the main area, so its size
    // is measured from there: dragging its top edge upward makes it bigger.
    assert_eq!(r.inspector_pct_at(21), 50, "half the 40-row main area");
    assert_eq!(r.inspector_pct_at(31), 25);
}

#[test]
fn a_zero_sized_area_does_not_divide_by_zero() {
    let empty = PanelRects {
        main: Rect::new(0, 0, 0, 0),
        body: Rect::new(0, 0, 0, 0),
        repo_list: Rect::new(0, 0, 0, 0),
        detail: None,
        inspector: None,
        inspector_folded: false,
        reader: None,
        contributions: None,
    };
    assert_eq!(empty.repo_list_pct_at(10), 0);
    assert_eq!(empty.inspector_pct_at(10), 0);
}

#[test]
fn one_word_uses_first_three_letters_original_case() {
    assert_eq!(author_code("Gaurgle"), "Gau");
    assert_eq!(author_code("torvalds"), "tor");
}

#[test]
fn two_words_use_uppercase_initials() {
    assert_eq!(author_code("Andreas Roos"), "AR");
}

#[test]
fn hyphenated_counts_as_two_words() {
    assert_eq!(author_code("Maria-Hazell"), "MH");
}

#[test]
fn three_or_more_words_use_first_two_initials() {
    assert_eq!(author_code("Lars Nils Andreas"), "LN");
    assert_eq!(author_code("Jean Luc Picard"), "JL");
}

#[test]
fn short_single_word_does_not_panic() {
    assert_eq!(author_code("x"), "x");
    assert_eq!(author_code("Bo"), "Bo");
}

#[test]
fn empty_name_falls_back() {
    assert_eq!(author_code(""), "?");
    assert_eq!(author_code("   "), "?");
}

fn layout_app() -> crate::app::App {
    crate::app::App::new(Vec::new(), "test".to_string(), 120, 44)
}

/// FZ-88: the inspector launches at its default share of the right column, and `=`
/// (`reset_pane_sizes`) returns to it after a resize. Measured through
/// `panel_rects`, the geometry clicks are tested against.
#[test]
fn the_inspector_starts_at_its_default_and_equals_returns_to_it() {
    let area = Rect::new(0, 0, 200, 60);
    let mut app = layout_app();
    app.view = crate::app::View::Detail;
    app.contributions_folded = true;

    let drawn_pct = |app: &crate::app::App| {
        let r = panel_rects(area, app);
        let inspector = r.inspector.expect("inspector laid out");
        let detail = r.detail.expect("detail laid out");
        inspector.height * 100 / (inspector.height + detail.height)
    };

    let launch = drawn_pct(&app);
    assert_eq!(app.inspector_pct, crate::app::DEFAULT_INSPECTOR_PCT);
    let want = crate::app::DEFAULT_INSPECTOR_PCT;
    assert!(
        (want - 2..=want).contains(&launch),
        "launch share was {launch}%"
    );

    app.resize_inspector(1);
    app.resize_inspector(1);
    assert!(drawn_pct(&app) > launch, "the resize did not grow it");

    app.reset_pane_sizes();
    assert_eq!(drawn_pct(&app), launch, "= returns to the default share");
}

/// `panel_rects` and `render` must agree about the strip, or clicks land
/// on a pane that is not where the eye sees it.
#[test]
fn panel_rects_reports_the_strip_as_it_is_drawn() {
    let area = Rect::new(0, 0, 200, 44);
    let mut app = layout_app();
    app.view = crate::app::View::Detail;

    app.contributions_folded = true;
    let folded = panel_rects(area, &app);
    assert_eq!(
        folded.contributions.map(|r| r.height),
        Some(FOLDED_HEIGHT),
        "a folded strip is still laid out"
    );

    app.contributions_folded = false;
    let open = panel_rects(area, &app);
    let strip = open.contributions.expect("strip present");
    assert_eq!(strip.height, contributions::CONTRIB_HEIGHT);
    assert_eq!(
        open.at(strip.x + 2, strip.y + 2),
        Some(FocusedPanel::Contributions),
        "a click in the strip lands on the strip"
    );
    // The strip comes off the bottom of everything, under the inspector,
    // so the panes above all give up rows for it.
    assert!(open.repo_list.height < folded.repo_list.height);
    assert!(open.detail.unwrap().height < folded.detail.unwrap().height);
}

/// The strip spans the full width at the very bottom whatever the
/// terminal size: the inspector sits directly above it and the list and
/// detail pane above that.
#[test]
fn the_strip_spans_the_bottom_under_the_inspector() {
    let mut app = layout_app();
    app.view = crate::app::View::Detail;
    app.contributions = crate::app::ContributionsMode::Github;
    for width in [120u16, 200] {
        let rects = panel_rects(Rect::new(0, 0, width, 44), &app);
        let strip = rects.contributions.expect("strip present");
        let inspector = rects.inspector.expect("inspector present");
        assert_eq!(strip.x, rects.main.x, "strip starts at the left edge");
        assert_eq!(strip.width, rects.main.width, "strip spans the width");
        assert_eq!(
            strip.y + strip.height,
            rects.main.y + rects.main.height,
            "strip is the bottom of the main area"
        );
        // `split_main_area` keeps one blank row between the strip and
        // whatever is above it.
        assert_eq!(inspector.y + inspector.height + 1, strip.y);
        assert_eq!(inspector.width, rects.main.width);
    }
}

/// Compact has no detail or inspector, but it does have the strip.
#[test]
fn the_strip_is_laid_out_in_compact_view_too() {
    let area = Rect::new(0, 0, 120, 44);
    let mut app = layout_app();
    app.view = crate::app::View::Compact;
    app.contributions = crate::app::ContributionsMode::Local;
    let rects = panel_rects(area, &app);
    assert!(rects.contributions.is_some());
    assert!(rects.detail.is_none());
    assert!(rects.repo_list.height < area.height - contributions::CONTRIB_HEIGHT);
}

/// A terminal too short for both loses the strip, not the dashboard.
#[test]
fn a_short_terminal_drops_the_strip_rather_than_the_panes() {
    let mut app = layout_app();
    app.contributions = crate::app::ContributionsMode::Github;
    let rects = panel_rects(Rect::new(0, 0, 120, 18), &app);
    assert!(rects.contributions.is_none());
    assert!(rects.repo_list.height > 0, "the panes survive");
}

/// The inspector is docked under both panes, not under the detail pane
/// alone: the repo list is rarely tall and scrolls when it must, while a
/// commit's files and message both want width. The list and the detail
/// pane share the row above it.
#[test]
fn the_inspector_spans_the_full_width_under_both_panes() {
    let mut app = layout_app();
    app.view = crate::app::View::Detail;
    let rects = panel_rects(Rect::new(0, 0, 200, 44), &app);
    let inspector = rects.inspector.expect("inspector present");
    let detail = rects.detail.expect("detail present");
    assert_eq!(inspector.x, rects.main.x, "starts at the left edge");
    assert_eq!(inspector.width, rects.main.width, "spans the width");
    assert_eq!(
        rects.repo_list.height, detail.height,
        "the list and the detail pane share the row above"
    );
    assert_eq!(
        rects.repo_list.y + rects.repo_list.height,
        inspector.y,
        "the list stops where the inspector starts"
    );
    assert_eq!(
        rects.at(rects.repo_list.x + 2, inspector.y + 2),
        Some(FocusedPanel::Inspector),
        "a click under the list lands on the inspector"
    );
}

/// A GitHub poll that failed has to leave a mark that a keypress cannot
/// clear. It used to set only a status message, so a rate limit -- an
/// hour long -- was invisible within one keystroke, and the frozen CI
/// lamps and contribution strip looked like current data.
#[test]
fn a_failing_github_poll_is_marked_in_the_header() {
    let row = header_row_with("", 80, fail_github);
    assert!(row.contains("⚠ github"), "no failure marker in: {row:?}");
}

/// A poll that came back an error, as the event loop records one.
fn fail_github(app: &mut crate::app::App) {
    app.github_freshness.failed(
        crate::github::Failure::new(crate::github::FailureKind::Http, "HTTP 502"),
        chrono::Local::now(),
    );
}

fn failure(kind: crate::github::FailureKind, message: &str) -> crate::github::Failure {
    crate::github::Failure::new(kind, message)
}

fn failed_fetch(
    kind: crate::git::FetchFailureKind,
    last_success: Option<chrono::DateTime<chrono::Local>>,
) -> crate::model::FetchFacts {
    let mut facts = crate::model::FetchFacts {
        last_success,
        ..Default::default()
    };
    let failure = crate::git::FetchFailure { kind, detail: None };
    facts.record(
        &crate::git::FetchOutcome::Failed(failure),
        at(2026, 10, 1, 14, 30),
    );
    facts
}

/// Rule 5 in the detail pane: the failure, why, and how old the remote
/// refs behind the counts are, written the way the header writes it.
#[test]
fn the_fetch_failure_line_names_the_kind_and_the_last_success() {
    let now = at(2026, 10, 1, 14, 30);
    let facts = failed_fetch(
        crate::git::FetchFailureKind::NoRemote,
        Some(at(2026, 10, 1, 14, 2)),
    );
    assert_eq!(
        fetch_failure_line(&facts, now).as_deref(),
        Some("⚠ fetch failed: remote not found, as of 14:02")
    );
}

/// Rule 3: unknown is said, never drawn as a time.
#[test]
fn an_unknown_last_success_is_said_to_be_unknown() {
    let now = at(2026, 10, 1, 14, 30);
    let facts = failed_fetch(crate::git::FetchFailureKind::Auth, None);
    assert_eq!(
        fetch_failure_line(&facts, now).as_deref(),
        Some("⚠ fetch failed: authentication failed, age unknown")
    );
}

/// FZ-72 (#128): an ssh refusal names what to fix; an https refusal
/// keeps its own phrase.
#[test]
fn an_ssh_refusal_names_the_key_and_an_auth_refusal_the_credential() {
    let now = at(2026, 10, 1, 14, 30);
    let ssh = failed_fetch(crate::git::FetchFailureKind::Ssh, None);
    assert_eq!(
        fetch_failure_line(&ssh, now).as_deref(),
        Some("⚠ fetch failed: ssh key refused (run ssh-add), age unknown")
    );
    let auth = failed_fetch(crate::git::FetchFailureKind::Auth, None);
    assert_eq!(
        fetch_failure_line(&auth, now).as_deref(),
        Some("⚠ fetch failed: authentication failed, age unknown")
    );
    assert_ne!(
        fetch_failure_short(crate::git::FetchFailureKind::Ssh),
        fetch_failure_short(crate::git::FetchFailureKind::Auth)
    );
}

/// Nothing extra while fetches work, or before any has run.
#[test]
fn a_fetch_that_did_not_fail_draws_nothing() {
    let now = at(2026, 10, 1, 14, 30);
    let mut ok = crate::model::FetchFacts::default();
    assert_eq!(fetch_failure_line(&ok, now), None);
    assert_eq!(fetch_marker(&ok), None);
    ok.record(&crate::git::FetchOutcome::Succeeded, now);
    assert_eq!(fetch_failure_line(&ok, now), None);
    assert_eq!(fetch_marker(&ok), None);
}

/// FZ-35a (#84): the received size in the unit a person would say it,
/// and nothing before the first byte.
#[test]
fn a_running_fetch_is_labelled_with_its_received_size() {
    let label = |bytes: Option<u64>| {
        let facts = crate::model::FetchFacts {
            progress: bytes.map(|bytes| crate::git::FetchProgress {
                phase: crate::git::FetchPhase::Receiving,
                objects_done: 1,
                objects_total: 2,
                bytes,
            }),
            ..Default::default()
        };
        fetch_progress_label(&facts)
    };
    assert_eq!(label(None), None);
    assert_eq!(label(Some(0)), None);
    assert_eq!(label(Some(1)).as_deref(), Some("fetching 1 KB"));
    assert_eq!(label(Some(700 * 1024)).as_deref(), Some("fetching 700 KB"));
    assert_eq!(
        label(Some(312 * 1024 * 1024)).as_deref(),
        Some("fetching 312 MB")
    );
    assert_eq!(
        label(Some(3 * 1024 * 1024 * 1024 / 2)).as_deref(),
        Some("fetching 1.5 GB")
    );
}

/// Every kind has its own short word and phrase.
#[test]
fn every_fetch_failure_kind_has_its_own_words() {
    use crate::git::FetchFailureKind::*;
    let kinds = [TimedOut, Auth, Unreachable, NoRemote, Other];
    let shorts: std::collections::HashSet<_> =
        kinds.iter().map(|k| fetch_failure_short(*k)).collect();
    let phrases: std::collections::HashSet<_> =
        kinds.iter().map(|k| fetch_failure_phrase(*k)).collect();
    assert_eq!(shorts.len(), kinds.len());
    assert_eq!(phrases.len(), kinds.len());
}

/// FZ-35b: `TooBig` has words of its own too.
#[test]
fn a_fetch_cut_at_the_cap_has_its_own_words() {
    use crate::git::FetchFailureKind::*;
    let others = [TimedOut, Auth, Unreachable, NoRemote, Other];
    assert!(
        others
            .iter()
            .all(|k| fetch_failure_short(*k) != fetch_failure_short(TooBig)
                && fetch_failure_phrase(*k) != fetch_failure_phrase(TooBig))
    );
}

/// FZ-35b (#85): the two hold labels, sized as the progress label is,
/// and nothing for a repo nothing holds.
#[test]
fn a_held_repo_is_labelled_with_the_size_and_the_key() {
    const MIB: u64 = 1024 * 1024;
    let mut facts = crate::model::FetchFacts {
        disk_baseline: Some(10),
        ..Default::default()
    };
    assert_eq!(fetch_hold_label(&facts), None);
    facts.assess_growth(Some(10 + 3 * 1024 * 1024 / 2), Some(200 * MIB));
    assert_eq!(
        fetch_hold_label(&facts).as_deref(),
        Some("fetch ~1.5 GB, press f")
    );
    facts.record(
        &crate::git::FetchOutcome::Failed(crate::git::FetchFailure::too_big(201 * MIB)),
        at(2026, 10, 20, 14, 0),
    );
    assert_eq!(
        fetch_hold_label(&facts).as_deref(),
        Some("fetch over 201 MB, press f")
    );
}

/// Rule 9's status line, for each shape of result.
#[test]
fn the_fetch_report_says_how_many_failed() {
    use crate::git::FetchFailureKind::*;
    let report = |total, failed: &[(&str, crate::git::FetchFailureKind)]| crate::app::FetchReport {
        total,
        failed: failed.iter().map(|(n, k)| (n.to_string(), *k)).collect(),
    };
    assert_eq!(
        fetch_report_label(&report(1, &[("fleetz", TimedOut)])),
        "fetch failed: fleetz, timed out"
    );
    assert_eq!(
        fetch_report_label(&report(5, &[("fleetz", Auth)])),
        "1 of 5 fetches failed: fleetz"
    );
    assert_eq!(
        fetch_report_label(&report(5, &[("a", Auth), ("b", Unreachable)])),
        "2 of 5 fetches failed"
    );
}

/// The case that was invisible: a poll that failed offline with good data
/// on screen names the failure and how old the data is.
#[test]
fn a_stale_header_names_the_failure_and_the_data_age() {
    let now = at(2026, 10, 1, 14, 30);
    let mut freshness = crate::app::GithubFreshness::default();
    freshness.succeeded(at(2026, 10, 1, 14, 2));
    freshness.failed(failure(crate::github::FailureKind::Offline, "dns"), now);
    let (long, short, color) = github_freshness_segment(&freshness, now).unwrap();
    assert_eq!(long, " ⚠ github offline at 14:30, as of 14:02 ");
    assert_eq!(short, " ⚠ github ");
    assert_eq!(color, colors::PEACH);
}

/// A rate limit is an API failure, never "offline", and with a cache of
/// unreadable age the age is said to be unknown rather than made up.
#[test]
fn a_rate_limit_on_data_of_unknown_age_says_both() {
    let now = at(2026, 10, 1, 14, 30);
    let mut freshness = crate::app::GithubFreshness::default();
    freshness.loaded(&crate::app::DatasetSource::Cache { written: None }, now);
    freshness.failed(
        crate::github::Failure::rate_limit("GitHub API errors: API rate limit exceeded"),
        now,
    );
    let (long, _, _) = github_freshness_segment(&freshness, now).unwrap();
    assert_eq!(long, " ⚠ github rate limited at 14:30, age unknown ");
}

/// #79. The failure is dated by the attempt that failed, not only by the
/// data it left on screen: an hour-old offline verdict and one from this
/// minute used to read the same.
#[test]
fn a_failed_header_says_when_the_attempt_failed() {
    let now = at(2026, 10, 1, 12, 30);
    let mut freshness = crate::app::GithubFreshness::default();
    freshness.succeeded(at(2026, 10, 1, 11, 2));
    freshness.failed(
        failure(crate::github::FailureKind::Offline, "dns"),
        at(2026, 10, 1, 12, 2),
    );
    let (long, short, color) = github_freshness_segment(&freshness, now).unwrap();
    assert_eq!(long, " ⚠ github offline at 12:02, as of 11:02 ");
    assert_eq!(short, " ⚠ github ");
    assert_eq!(color, colors::PEACH);
}

/// #79. No threshold: an attempt seconds old is stamped too.
#[test]
fn a_failed_attempt_seconds_old_is_stamped_too() {
    let now = at(2026, 10, 1, 12, 2) + chrono::Duration::seconds(5);
    let mut freshness = crate::app::GithubFreshness::default();
    freshness.succeeded(at(2026, 10, 1, 11, 2));
    freshness.failed(
        failure(crate::github::FailureKind::Offline, "dns"),
        at(2026, 10, 1, 12, 2),
    );
    let (long, _, _) = github_freshness_segment(&freshness, now).unwrap();
    assert_eq!(long, " ⚠ github offline at 12:02, as of 11:02 ");
}

/// #79. Data of unknown age keeps saying so after the stamp.
#[test]
fn a_failed_attempt_on_data_of_unknown_age_is_stamped() {
    let now = at(2026, 10, 1, 12, 30);
    let mut freshness = crate::app::GithubFreshness::default();
    freshness.loaded(&crate::app::DatasetSource::Cache { written: None }, now);
    freshness.failed(
        failure(crate::github::FailureKind::Offline, "dns"),
        at(2026, 10, 1, 12, 2),
    );
    let (long, _, _) = github_freshness_segment(&freshness, now).unwrap();
    assert_eq!(long, " ⚠ github offline at 12:02, age unknown ");
}

/// #79. With no data at all there is no data part, only the stamp.
#[test]
fn a_failed_attempt_with_no_data_is_stamped() {
    let now = at(2026, 10, 1, 12, 30);
    let mut freshness = crate::app::GithubFreshness::default();
    freshness.failed(
        failure(crate::github::FailureKind::Offline, "dns"),
        at(2026, 10, 1, 12, 2),
    );
    let (long, _, _) = github_freshness_segment(&freshness, now).unwrap();
    assert_eq!(long, " ⚠ github offline at 12:02 ");
}

/// #79. A failure with no attempt recorded (built by hand: no real path
/// sets one without the other) renders as before, with no stamp.
#[test]
fn a_failure_with_no_attempt_recorded_has_no_stamp() {
    let now = at(2026, 10, 1, 12, 30);
    let mut freshness = crate::app::GithubFreshness::default();
    freshness.data_time = crate::app::DataTime::At(at(2026, 10, 1, 11, 2));
    freshness.outcome =
        crate::app::Outcome::Failed(failure(crate::github::FailureKind::Offline, "dns"));
    assert_eq!(freshness.last_attempt, None);
    let (long, _, _) = github_freshness_segment(&freshness, now).unwrap();
    assert_eq!(long, " ⚠ github offline, as of 11:02 ");
}

/// #79. An attempt on another day carries its date, as `swept` does.
#[test]
fn a_failed_attempt_from_yesterday_carries_its_date() {
    let now = at(2026, 10, 2, 9, 0);
    let mut freshness = crate::app::GithubFreshness::default();
    freshness.succeeded(at(2026, 10, 1, 11, 2));
    freshness.failed(
        failure(crate::github::FailureKind::Offline, "dns"),
        at(2026, 10, 1, 12, 2),
    );
    let (long, _, _) = github_freshness_segment(&freshness, now).unwrap();
    assert_eq!(long, " ⚠ github offline at Oct 1 12:02, as of Oct 1 11:02 ");
}

/// A jitter that draws nothing, so a resume is the reset to the second.
struct NoJitter;

impl crate::app::Jitter for NoJitter {
    fn backoff_factor(&mut self) -> f64 {
        1.0
    }
    fn resume_secs(&mut self) -> i64 {
        0
    }
}

/// An app watching GitHub whose last good data is from 14:02, with the
/// default interval (30 s) and cost (3 points).
fn polled_app() -> crate::app::App {
    let mut app = crate::app::App::new(Vec::new(), String::new(), 120, 40);
    app.jitter = Box::new(NoJitter);
    app.watch_mode = true;
    app.github_freshness.succeeded(at(2026, 10, 1, 14, 2));
    app
}

/// The poll at 14:10 answered with `remaining` of 5000 points left, the
/// window resetting at 15:00.
fn polled_with(app: &mut crate::app::App, remaining: u32) {
    let arrival = at(2026, 10, 1, 14, 10);
    let reading = crate::github::BudgetReading {
        limit: 5000,
        remaining,
        used: 5000 - remaining,
        reset: at(2026, 10, 1, 15, 0).timestamp(),
        date: Some(arrival.with_timezone(&chrono::Utc)),
        token_id: "t".to_string(),
    };
    app.observe_budget(Some(&reading), true, arrival);
}

/// FZ-10c, 4.6. Rate limited: today's segment, with when polling resumes.
#[test]
fn a_rate_limited_header_says_when_polling_resumes() {
    let mut app = polled_app();
    polled_with(&mut app, 0);
    let refused = crate::github::Failure {
        rate_limited: true,
        limit: Some(crate::github::RateLimit::Primary),
        ..crate::github::Failure::new(crate::github::FailureKind::Http, "HTTP 403")
    };
    app.github_attempt_failed(&refused, at(2026, 10, 1, 14, 10));
    app.github_freshness
        .failed(refused, at(2026, 10, 1, 14, 10));
    let (long, short, color) = github_segment(&app, at(2026, 10, 1, 14, 20)).unwrap();
    assert_eq!(long, " ⚠ github rate limited until 15:00, as of 14:02 ");
    assert_eq!(short, " ⚠ github ");
    assert_eq!(color, colors::PEACH);
    // Once the limit has passed the time goes; the failure stays until a
    // poll succeeds.
    let (long, _, _) = github_segment(&app, at(2026, 10, 1, 15, 1)).unwrap();
    assert_eq!(long, " ⚠ github rate limited at 14:10, as of 14:02 ");
}

/// FZ-10c, 4.6. Paused at the reserve: polls succeed, but none goes again
/// before the reset. Watch off has no polling to pause, and says nothing.
#[test]
fn a_paused_header_says_until_when_and_why() {
    let mut app = polled_app();
    polled_with(&mut app, 400);
    app.github_freshness.succeeded(at(2026, 10, 1, 14, 10));
    let (long, short, color) = github_segment(&app, at(2026, 10, 1, 14, 20)).unwrap();
    assert_eq!(long, " github paused until 15:00, allowance nearly used ");
    assert_eq!(short, " github paused ");
    assert_eq!(color, colors::PEACH);
    assert_eq!(github_segment(&app, at(2026, 10, 1, 15, 0)), None);
    app.watch_mode = false;
    assert_eq!(github_segment(&app, at(2026, 10, 1, 14, 20)), None);
}

/// FZ-10c, 4.6. Slowed: 100 points above the reserve over 50 minutes
/// spaces 3-point polls 90 s apart, past the 30 s interval. Plenty left
/// says nothing.
#[test]
fn a_slowed_header_says_the_allowance_is_low() {
    let mut app = polled_app();
    polled_with(&mut app, 600);
    app.github_freshness.succeeded(at(2026, 10, 1, 14, 10));
    let (long, short, color) = github_segment(&app, at(2026, 10, 1, 14, 20)).unwrap();
    assert_eq!(long, " github slowed, allowance low ");
    assert_eq!(short, " github slowed ");
    assert_eq!(color, colors::OVERLAY);

    let mut healthy = polled_app();
    polled_with(&mut healthy, 4000);
    healthy.github_freshness.succeeded(at(2026, 10, 1, 14, 10));
    assert_eq!(github_segment(&healthy, at(2026, 10, 1, 14, 20)), None);
}

/// FZ-10c, 4.6. Backing off after failures in a row adds nothing: the
/// segment already says it failed and how old the data is.
#[test]
fn a_backing_off_header_keeps_the_failure_and_its_age() {
    let mut app = polled_app();
    polled_with(&mut app, 4000);
    for minute in [10, 11, 13] {
        let now = at(2026, 10, 1, 14, minute);
        let failed = failure(crate::github::FailureKind::Server, "HTTP 502");
        app.github_attempt_failed(&failed, now);
        app.github_freshness.failed(failed, now);
    }
    let (long, _, color) = github_segment(&app, at(2026, 10, 1, 14, 14)).unwrap();
    assert_eq!(long, " ⚠ github server error at 14:13, as of 14:02 ");
    assert_eq!(color, colors::PEACH);
}

/// A cache-served launch, before its attempt lands: quiet, and dated the
/// way the `swept` label is, so yesterday's cache carries its date.
#[test]
fn a_cache_served_launch_is_marked_cached_with_its_age() {
    let now = at(2026, 10, 1, 9, 0);
    let mut freshness = crate::app::GithubFreshness::default();
    freshness.loaded(
        &crate::app::DatasetSource::Cache {
            written: Some(at(2026, 9, 30, 18, 5)),
        },
        now,
    );
    let (long, short, color) = github_freshness_segment(&freshness, now).unwrap();
    assert_eq!(long, " github cached Sep 30 18:05 ");
    assert_eq!(short, " github cached ");
    assert_eq!(color, colors::OVERLAY);

    let mut unknown = crate::app::GithubFreshness::default();
    unknown.loaded(&crate::app::DatasetSource::Cache { written: None }, now);
    let (long, _, _) = github_freshness_segment(&unknown, now).unwrap();
    assert_eq!(long, " github cached, age unknown ");
}

/// Fresh says nothing, and neither does a dashboard with no GitHub data
/// at all (`-d`, or before the first load lands).
#[test]
fn fresh_or_no_github_data_draws_no_segment() {
    let now = chrono::Local::now();
    let mut freshness = crate::app::GithubFreshness::default();
    assert!(github_freshness_segment(&freshness, now).is_none());
    freshness.succeeded(now);
    assert!(github_freshness_segment(&freshness, now).is_none());
}

/// The long form when there is room for it, the short form when there is
/// not, and never neither.
#[test]
fn the_cached_segment_shortens_rather_than_disappears() {
    let cached = |app: &mut crate::app::App| {
        app.github_freshness.loaded(
            &crate::app::DatasetSource::Cache { written: None },
            chrono::Local::now(),
        )
    };
    let wide = header_row_with("", 100, cached);
    assert!(wide.contains("github cached, age unknown"), "{wide:?}");
    let narrow = header_row_with("last 7 days", 30, cached);
    assert!(narrow.contains("github cached"), "dropped: {narrow:?}");
}

/// The other half: a healthy poll must not leave a warning sitting there.
#[test]
fn a_healthy_github_poll_leaves_the_header_clean() {
    let row = header_row_with("", 80, |app| {
        app.github_freshness.succeeded(chrono::Local::now())
    });
    assert!(!row.contains("⚠"), "spurious failure marker in: {row:?}");
}

/// The fit loop drops the version, then the timestamp, then the group,
/// then the header text. This marker is not on that list: a narrow
/// terminal is not a reason to hide that the data stopped updating.
#[test]
fn the_failure_marker_survives_a_narrow_header() {
    let row = header_row_with("last 7 days", 30, fail_github);
    assert!(
        row.contains("⚠ github"),
        "marker dropped by the fit loop: {row:?}"
    );
}

/// The header row is the name and the version, with GitHub, watch and
/// the sweep time flush right outside the compact view, whose status
/// line carries those instead. The window and the summary are in the
/// panes (FZ-95) or on that line, never here.
#[test]
fn the_header_draws_the_name_and_version_with_github_watch_and_sweep() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    for (view, expected) in [
        (
            View::Detail,
            vec![
                "fleetz",
                VERSION,
                "○",
                "github",
                watch_icon(false),
                "watch",
                "swept",
                "Jan",
                "2",
                "03:04",
            ],
        ),
        (View::Compact, vec!["fleetz", VERSION]),
    ] {
        let mut app = crate::app::App::new(Vec::new(), "since 5 days".to_string(), 120, 40);
        full_header_app(&mut app);
        app.view = view;
        let mut terminal = Terminal::new(TestBackend::new(120, 1)).unwrap();
        terminal.draw(|f| render_header(f, f.area(), &app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let row: String = (0..120).map(|x| buffer[(x, 0)].symbol()).collect();
        let words = row.replace('─', " ");
        assert_eq!(
            words.split_whitespace().collect::<Vec<_>>(),
            expected,
            "{view:?}: {row:?}"
        );
    }
}

/// While the fleet loads the header row says so, left of GitHub: pane 2
/// has no header to carry it until the first repo lands, and launch
/// used to be a blank screen of pane outlines (his look, 2026-11-05).
#[test]
fn the_header_says_loading_before_any_repo_lands() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let mut app = crate::app::App::new(Vec::new(), String::new(), 120, 40);
    app.view = View::Detail;
    app.loading = true;
    let mut terminal = Terminal::new(TestBackend::new(120, 1)).unwrap();
    terminal.draw(|f| render_header(f, f.area(), &app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let row: String = (0..120).map(|x| buffer[(x, 0)].symbol()).collect();
    let loading = row.find("loading").expect("no loading on the header");
    let github = row.find("github").expect("no github on the header");
    assert!(loading < github, "{row:?}");
}

/// The header's two sides never overprint: as the terminal narrows,
/// GitHub shortens, then the sweep time and then the version go, and
/// the name and the watch eye stay whole (FZ-93 review).
#[test]
fn a_narrow_header_sheds_pieces_rather_than_overprinting() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    for width in 30u16..=90 {
        let mut app = crate::app::App::new(Vec::new(), String::new(), width, 40);
        full_header_app(&mut app);
        app.view = View::Detail;
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal.draw(|f| render_header(f, f.area(), &app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let row: String = (0..width).map(|x| buffer[(x, 0)].symbol()).collect();
        assert!(row.starts_with(" fleetz"), "w{width}: {row:?}");
        assert!(row.contains(" watch"), "w{width}: {row:?}");
        assert!(row.contains("github"), "w{width}: {row:?}");
    }
}

/// Where pane 1 draws no summary box (here a pane 1 too short for it),
/// the whole frame still reports an action: the footer carries the
/// message (FZ-93 review, B1; FZ-95). Fails while the per-frame flag is left set from
/// an earlier frame or the footer stops reading it.
#[test]
fn with_no_fleet_column_the_frame_shows_the_message_in_the_footer() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let (w, h) = (160u16, 30u16);
    let mut app = crate::app::App::new(Vec::new(), String::new(), w, h);
    app.view = View::Detail;
    app.fleet_drawn.set(true);
    app.set_status("Already refreshing".to_string(), true);
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let last: String = (0..w).map(|x| buffer[(x, h - 1)].symbol()).collect();
    assert!(last.contains("Already refreshing"), "{last:?}");
}

/// The window is drawn in its own colour, not one more grey phrase, and
/// on no ground of its own: a tinted chip read as a selection.
#[test]
fn the_window_is_drawn_in_teal_on_no_ground() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let app = crate::app::App::new(Vec::new(), "since 5 days".to_string(), 100, 40);
    let mut terminal = Terminal::new(TestBackend::new(100, 1)).unwrap();
    terminal
        .draw(|f| render_status_line(f, f.area(), &app))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let row: String = (0..100).map(|x| buffer[(x, 0)].symbol()).collect();
    let x = row.find("since").expect("window drawn") as u16;
    let style = buffer[(x, 0)].style();
    assert_eq!(style.fg, Some(colors::TEAL));
    assert_ne!(style.bg, Some(colors::SELECTED_BG));
}

/// The GitHub lamp's colour for each state: green connected, peach for
/// an issue that clears on its own, red for an error, grey before any
/// poll answered.
#[test]
fn the_github_lamp_says_connected_issue_or_error() {
    let now = chrono::Local::now();
    let fresh = || crate::app::App::new(Vec::new(), String::new(), 100, 40);

    let app = fresh();
    assert_eq!(github_status(&app, now).0, GithubHealth::Unknown);

    let mut app = fresh();
    app.dirty_mode = true;
    let (health, long, _) = github_status(&app, now);
    assert_eq!(health, GithubHealth::Unknown);
    assert!(long.contains("github off"), "{long:?}");

    let mut app = fresh();
    app.github_freshness.succeeded(now);
    let (health, long, short) = github_status(&app, now);
    assert_eq!(health, GithubHealth::Connected);
    assert_eq!(health.color(), colors::GREEN);
    assert!(long.contains("github connected"), "{long:?}");
    assert!(short.contains("github"), "{short:?}");

    let mut app = fresh();
    fail_github(&mut app);
    let (health, long, _) = github_status(&app, now);
    assert_eq!(health, GithubHealth::Error);
    assert_eq!(health.color(), colors::RED);
    assert!(long.contains("⚠ github"), "{long:?}");

    let mut app = fresh();
    app.github_freshness.failed(
        crate::github::Failure::rate_limit("API rate limit exceeded"),
        now,
    );
    let (health, _, _) = github_status(&app, now);
    assert_eq!(health, GithubHealth::Issue);
    assert_eq!(health.color(), colors::PEACH);
}

/// A healthy poll is drawn too, in green: silence used to stand for
/// connected, which a narrow terminal could not tell from not drawn.
#[test]
fn a_healthy_github_poll_says_connected_in_green() {
    let row = header_row_with("", 100, |app| {
        app.github_freshness.succeeded(chrono::Local::now())
    });
    assert!(row.contains("● github connected"), "{row:?}");
}

/// The chrome is three rows in both views, as it was before the status
/// box: a row less drops the inspector at its minimum height. Only the
/// compact view, with no pane 1 to dock the box under, has a status line.
#[test]
fn both_views_keep_the_main_area_height() {
    let area = Rect::new(0, 0, 120, 30);
    let mut app = crate::app::App::new(Vec::new(), String::new(), 120, 30);
    app.view = View::Detail;
    let detail = frame_chunks(area, &app);
    app.view = View::Compact;
    let compact = frame_chunks(area, &app);
    assert_eq!(detail.main.height, area.height - 3);
    assert_eq!(compact.main.height, area.height - 3);
    assert_eq!(detail.status, None);
    assert_eq!(compact.status.map(|r| r.y), Some(1));
}

/// A status line carrying every piece the fit loop can drop: a window
/// label with an arrow, a group, a fixed sweep time, a status message in
/// the summary's slot, and the Nerd Font watch eye. Several of these are
/// multi-byte but one column.
fn full_header_app(app: &mut crate::app::App) {
    app.group_names = vec!["work".to_string()];
    app.active_group = Some(0);
    app.last_refreshed = Some(at(2026, 1, 2, 3, 4));
    app.set_status("1↓ · 2↑ copied".to_string(), false);
}

const ARROW_HEADER: &str = "since 2 weeks → before 2026-09-01";

/// The columns a status line draws: everything but the border filler.
fn drawn_columns(row: &str) -> usize {
    row.chars().filter(|c| *c != '─').count()
}

/// Regression: the fit was measured in bytes, so the arrow, the
/// separator and the watch eye cost two to four columns each and a piece
/// went while there was room for it. At the width the pieces draw, plus
/// the one column the loop keeps between them, everything is kept; one
/// column less, the sweep time goes first.
#[test]
fn the_status_line_keeps_every_piece_that_fits_as_drawn() {
    let wide = header_row_with(ARROW_HEADER, 200, full_header_app);
    let drawn = drawn_columns(&wide);
    let pieces = wide.replace('─', "");
    assert!(pieces.len() > drawn + 1, "no multi-byte glyph: {pieces:?}");
    let width = (drawn + 1) as u16;

    let row = header_row_with(ARROW_HEADER, width, full_header_app);
    assert!(row.contains(&format!("{ARROW_HEADER} [t]")), "{row:?}");
    assert!(row.contains("[work]"), "{row:?}");
    assert!(row.contains("1↓ · 2↑ copied"), "{row:?}");
    assert!(row.contains("swept Jan 2 03:04"), "{row:?}");
    assert!(
        row.contains(&format!("{} watch", watch_icon(false))),
        "{row:?}"
    );

    let tighter = header_row_with(ARROW_HEADER, width - 1, full_header_app);
    assert!(!tighter.contains("swept"), "{tighter:?}");
    assert!(tighter.contains("[work]"), "{tighter:?}");
}

/// The drop order, read from the drawn status line at every width from
/// wide to narrow: the sweep time, then the group, then the GitHub
/// segment's long form, then the summary, then the window. A
/// reordering, or a step that is skipped, fails.
#[test]
fn the_status_line_drops_its_pieces_in_order() {
    let setup = |app: &mut crate::app::App| {
        full_header_app(app);
        fail_github(app);
    };
    let state = |row: &str| {
        (
            row.contains("swept"),
            row.contains("[work]"),
            row.contains("http error"),
            row.contains("copied"),
            row.contains("since 2 weeks"),
        )
    };
    let mut states = Vec::new();
    for width in (10u16..=200).rev() {
        let s = state(&header_row_with(ARROW_HEADER, width, setup));
        if states.last() != Some(&s) {
            states.push(s);
        }
        if s == (false, false, false, false, false) {
            break;
        }
    }
    assert_eq!(
        states,
        vec![
            (true, true, true, true, true),
            (false, true, true, true, true),
            (false, false, true, true, true),
            (false, false, false, true, true),
            (false, false, false, false, true),
            (false, false, false, false, false),
        ]
    );
}

/// Sizes worth checking a divider round-trip at: the strip open and shut,
/// since the two give the inspector split different areas to work in.
fn drag_cases() -> Vec<(u16, u16, crate::app::ContributionsMode)> {
    let mut cases = Vec::new();
    for mode in [
        crate::app::ContributionsMode::Off,
        crate::app::ContributionsMode::Github,
    ] {
        for (w, h) in [(120u16, 30u16), (120, 40), (120, 50), (133, 44), (157, 60)] {
            cases.push((w, h, mode));
        }
    }
    cases
}

/// The failure this closes: pressing on the horizontal divider and holding
/// still moved it. `inspector_pct_at` measured the row against `main`,
/// while `split_inspector` applies the percentage to `body` -- main minus
/// the contribution strip -- so with the strip open (the default) a grab
/// sent the divider four to five rows away from the cursor before the
/// first drag event arrived.
#[test]
fn grabbing_the_horizontal_divider_leaves_it_where_it_was() {
    for (w, h, mode) in drag_cases() {
        let mut app = crate::app::App::new(Vec::new(), String::new(), w, h);
        app.view = View::Detail;
        app.contributions = mode;
        let area = Rect::new(0, 0, w, h);
        let rects = panel_rects(area, &app);
        let Some(before) = rects.inspector else {
            continue;
        };
        app.set_inspector_pct(rects.inspector_pct_at(before.y));
        let after = panel_rects(area, &app).inspector.expect("inspector kept");
        assert_eq!(
            after.y, before.y,
            "divider moved on a grab at {w}x{h}, strip {mode:?}"
        );
    }
}

/// The same round-trip for the vertical divider, over every position a
/// drag can actually put it in. Both directions used to floor, so a grab
/// could quantise the column away even though the mouse had not moved.
///
/// Walked over the percentages rather than the columns because only a
/// percentage-produced column is representable: see
/// `grabbing_the_capped_default_divider_moves_it_at_most_one_column`.
#[test]
fn grabbing_the_vertical_divider_leaves_it_where_it_was() {
    for (w, h, mode) in drag_cases() {
        for pct in crate::app::REPO_LIST_PCT_MIN..=crate::app::REPO_LIST_PCT_MAX {
            if pct == crate::app::DEFAULT_REPO_LIST_PCT {
                continue; // capped, and covered by its own test below
            }
            let mut app = crate::app::App::new(Vec::new(), String::new(), w, h);
            app.view = View::Detail;
            app.contributions = mode;
            app.set_repo_list_pct(pct);
            let area = Rect::new(0, 0, w, h);
            let rects = panel_rects(area, &app);
            let before = rects.repo_list.x + rects.repo_list.width;
            app.set_repo_list_pct(rects.repo_list_pct_at(before));
            let list = panel_rects(area, &app).repo_list;
            assert_eq!(
                list.x + list.width,
                before,
                "divider moved on a grab at {w}x{h} pct {pct}, strip {mode:?}"
            );
        }
    }
}

/// The one position a grab cannot reproduce exactly.
///
/// `split_main` caps the repo list at `REPO_LIST_MAX_COLS` *only* at the
/// default percentage, so on a wide terminal the drawn width is one no
/// percentage of that terminal produces: at 157 columns the list is capped
/// to 52, and 52 sits between the widths 33% and 34% draw. Grabbing it
/// therefore releases the cap and the divider shifts by a column, once.
/// Bounded here so the shift can never grow into the multi-row jump the
/// horizontal divider had.
#[test]
fn grabbing_the_capped_default_divider_moves_it_at_most_one_column() {
    for (w, h, mode) in drag_cases() {
        let mut app = crate::app::App::new(Vec::new(), String::new(), w, h);
        app.view = View::Detail;
        app.contributions = mode;
        let area = Rect::new(0, 0, w, h);
        let rects = panel_rects(area, &app);
        let before = rects.repo_list.x + rects.repo_list.width;
        app.set_repo_list_pct(rects.repo_list_pct_at(before));
        let list = panel_rects(area, &app).repo_list;
        let moved = (list.x + list.width) as i32 - before as i32;
        assert!(
            moved.abs() <= 1,
            "divider jumped {moved} columns on a grab at {w}x{h}, strip {mode:?}"
        );
    }
}

/// A drag tracks the pointer: the divider lands on the row grabbed, not
/// merely stays put. Outside the range the clamps in `split_inspector`
/// allow it parks at the limit, which is deliberate -- so the limits are
/// measured first by dragging past them, and exact tracking is asserted
/// between.
#[test]
fn dragging_the_horizontal_divider_lands_on_the_grabbed_row() {
    let (w, h) = (120u16, 50u16);
    let mut app = crate::app::App::new(Vec::new(), String::new(), w, h);
    app.view = View::Detail;
    app.contributions = crate::app::ContributionsMode::Github;
    let area = Rect::new(0, 0, w, h);
    let body = panel_rects(area, &app).body;

    let drag_to = |app: &mut crate::app::App, row: u16| {
        app.set_inspector_pct(panel_rects(area, app).inspector_pct_at(row));
        panel_rects(area, app).inspector.expect("inspector kept").y
    };
    // Drag hard against each stop to find where the clamps park it.
    let tallest = drag_to(&mut app, body.y);
    let shortest = drag_to(&mut app, body.y + body.height);

    for row in body.y..body.y + body.height {
        let landed = drag_to(&mut app, row);
        assert_eq!(
            landed,
            row.clamp(tallest, shortest),
            "drag to row {row} landed at {landed} (stops {tallest}..={shortest})"
        );
    }
}

/// The whole screen rendered, so the test sees what the user sees.
fn screen(width: u16, height: u16, setup: impl FnOnce(&mut crate::app::App)) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = crate::app::App::new(Vec::new(), String::new(), width, height);
    setup(&mut app);
    let app = app;
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The failure this closes. A failed initial load left `repos` empty and
/// said why only in a status message, which `clear_status` wipes on the
/// next keypress -- so a rate-limited launch was a blank dashboard with no
/// stated cause, identical to a fleet with nothing in the window.
#[test]
fn a_failed_load_says_why_instead_of_drawing_nothing() {
    let out = screen(80, 24, |app| {
        app.load_error = Some("GitHub API errors: API rate limit exceeded".to_string());
    });
    assert!(
        out.contains("Could not load the fleet"),
        "no cause given:\n{out}"
    );
    assert!(
        out.contains("rate limit exceeded"),
        "the reason itself is missing:\n{out}"
    );
}

/// A rate limit clears itself, and saying so is the difference between
/// waiting and going bug-hunting.
#[test]
fn a_rate_limited_load_says_it_clears_on_its_own() {
    let out = screen(80, 24, |app| {
        app.load_error = Some("GitHub API errors: API rate limit exceeded".to_string());
    });
    assert!(out.contains("clears on its own"), "no reassurance:\n{out}");
    // A different failure must not claim to be self-healing.
    let other = screen(80, 24, |app| {
        app.load_error = Some("GitHub API request failed: dns error".to_string());
    });
    assert!(
        !other.contains("clears on its own"),
        "wrong advice:\n{other}"
    );
}

/// FZ-10c. A 403 or 429 limit is `Http`, and its message is a status line
/// and a code with no "rate limit" in it, so the word match alone missed
/// it. The typed flag the fetch layer set decides.
#[test]
fn an_http_rate_limit_load_says_it_clears_on_its_own() {
    use crate::github::{Failure, FailureKind, RateLimit};
    let message = "GitHub answered HTTP 403 Forbidden";
    let out = screen(80, 24, |app| {
        let limited = Failure {
            rate_limited: true,
            limit: Some(RateLimit::Primary),
            ..Failure::new(FailureKind::Http, message)
        };
        app.github_freshness.failed(limited, at(2026, 10, 9, 14, 2));
        app.load_error = Some(message.to_string());
    });
    assert!(out.contains("clears on its own"), "no reassurance:\n{out}");
    // The same message without the flag is an ordinary refusal.
    let other = screen(80, 24, |app| {
        app.github_freshness.failed(
            Failure::new(FailureKind::Refused, message),
            at(2026, 10, 9, 14, 2),
        );
        app.load_error = Some(message.to_string());
    });
    assert!(
        !other.contains("clears on its own"),
        "wrong advice:\n{other}"
    );
}

/// FZ-10c. The panel said `r` rebuilds from cache; `r` sweeps the local
/// repos and polls GitHub, and `R` is the full reload.
#[test]
fn the_load_error_panel_names_what_each_key_does() {
    let out = screen(100, 24, |app| {
        app.load_error = Some("GitHub API request failed: dns error".to_string());
    });
    assert!(!out.contains("rebuilds from cache"), "stale hint:\n{out}");
    assert!(out.contains("R reloads"), "no R hint:\n{out}");
    assert!(out.contains("r re-checks"), "no r hint:\n{out}");
}

/// The clean case: nothing failed, so nothing is claimed.
#[test]
fn a_healthy_launch_shows_no_error_panel() {
    let out = screen(80, 24, |app| app.load_error = None);
    assert!(!out.contains("Could not load the fleet"));
}

/// `panel_rects` must report exactly what `render` draws, or clicks land
/// on the wrong pane silently. Both call `split_inspector`, so
/// suppressing the inspector in one suppresses it in the other.
#[test]
fn panel_rects_reports_no_inspector_in_the_graph_view() {
    let mut app = crate::app::App::new(Vec::new(), String::new(), 120, 44);
    app.view = View::Detail;
    app.history_view = crate::app::HistoryView::Graph;
    let rects = panel_rects(Rect::new(0, 1, 120, 40), &app);
    assert!(
        rects.inspector.is_none(),
        "the graph view still docks an inspector"
    );
    let tall = rects.detail.expect("detail is always drawn in Detail view");

    app.history_view = crate::app::HistoryView::Commits;
    let with = panel_rects(Rect::new(0, 1, 120, 40), &app);
    assert!(
        tall.height > with.detail.unwrap().height,
        "suppressing the inspector did not give its rows to the graph"
    );
}

/// The whole screen rendered, with the rects `panel_rects` reports for it.
fn rendered(setup: impl FnOnce(&mut crate::app::App)) -> (PanelRects, ratatui::buffer::Buffer) {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (width, height) = (120u16, 44u16);
    let mut app = crate::app::App::new(Vec::new(), String::new(), width, height);
    app.view = View::Detail;
    app.contributions = crate::app::ContributionsMode::Github;
    setup(&mut app);
    let app = app;
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| render(f, &app)).unwrap();
    let rects = panel_rects(Rect::new(0, 0, width, height), &app);
    (rects, terminal.backend().buffer().clone())
}

/// The column of `needle` on `row`, as drawn.
fn column_of(buffer: &ratatui::buffer::Buffer, row: u16, needle: &str) -> Option<u16> {
    let line: Vec<&str> = (0..buffer.area.width)
        .map(|x| buffer[(x, row)].symbol())
        .collect();
    let chars: Vec<&str> = needle.split("").filter(|c| !c.is_empty()).collect();
    (0..line.len().saturating_sub(chars.len()))
        .find(|&x| line[x..x + chars.len()] == chars[..])
        .map(|x| x as u16)
}

/// The click target has to be where the tag is drawn, folded or not, or
/// a click on `[3]` folds nothing and a click beside it folds the pane.
#[test]
fn the_fold_tag_is_where_the_title_draws_it() {
    for folded in [false, true] {
        let (rects, buffer) = rendered(|a| {
            a.inspector_folded = folded;
            a.contributions_folded = folded;
        });
        for (rect, panel, tag, label) in [
            (rects.inspector, FocusedPanel::Inspector, "[3]", "inspector"),
            (
                rects.contributions,
                FocusedPanel::Contributions,
                "[4]",
                "contributions",
            ),
        ] {
            let r = rect.unwrap_or_else(|| panic!("{label} not laid out"));
            let x = column_of(&buffer, r.y, tag)
                .unwrap_or_else(|| panic!("{tag} not drawn (folded: {folded})"));
            for dx in 0..3 {
                assert_eq!(rects.fold_tag_at(x + dx, r.y), Some(panel), "{tag}");
            }
            let lx = column_of(&buffer, r.y, label).expect("label drawn");
            assert_eq!(rects.fold_tag_at(lx, r.y), None, "{label} is not the tag");
            assert_eq!(rects.fold_tag_at(x, r.y + 1), None, "below the title row");
        }
    }
}

/// Only the auxiliary panes fold: the list and detail tags do nothing.
#[test]
fn the_repo_list_and_detail_tags_do_not_fold() {
    let (rects, buffer) = rendered(|_| {});
    let row = rects.repo_list.y;
    for tag in ["[1]", "[2]"] {
        let x = column_of(&buffer, row, tag).expect("tag drawn");
        assert_eq!(rects.fold_tag_at(x + 1, row), None, "{tag}");
    }
}

/// Folded is the frame alone, and the rows it gave up go to the panes
/// above.
#[test]
fn a_folded_pane_keeps_only_its_frame() {
    let (open, _) = rendered(|_| {});
    let (folded, buffer) = rendered(|a| {
        a.inspector_folded = true;
        a.contributions_folded = true;
    });
    let inspector = folded.inspector.expect("folded inspector still laid out");
    let strip = folded.contributions.expect("folded strip still laid out");
    assert_eq!(inspector.height, FOLDED_HEIGHT);
    assert_eq!(strip.height, FOLDED_HEIGHT);
    assert!(folded.detail.unwrap().height > open.detail.unwrap().height);
    assert!(column_of(&buffer, inspector.y, "inspector").is_some());
    assert!(column_of(&buffer, strip.y, "contributions").is_some());
    for r in [inspector, strip] {
        let bottom = r.y + r.height - 1;
        assert_eq!(buffer[(r.x, r.y)].symbol(), "─", "top frame");
        assert_eq!(bottom, r.y, "the title row is the whole pane");
        assert!(!r.contains(Position::new(r.x + 2, r.y + 1)), "no content");
    }
}

/// A folded pane is its title row alone: each fold hands the panes above
/// the two rows a three-row frame spent on a blank row and a bottom
/// border.
#[test]
fn a_folded_pane_is_one_row_and_the_detail_pane_gets_the_rest() {
    let area = Rect::new(0, 0, 120, 44);
    let mut app = layout_app();
    app.view = View::Detail;
    app.contributions = crate::app::ContributionsMode::Github;
    let open = panel_rects(area, &app);
    app.inspector_folded = true;
    app.contributions_folded = true;
    let folded = panel_rects(area, &app);

    let inspector = folded.inspector.expect("folded inspector laid out");
    let strip = folded.contributions.expect("folded strip laid out");
    assert_eq!(inspector.height, 1, "the inspector folds to its title row");
    assert_eq!(strip.height, 1, "the strip folds to its title row");

    let open_inspector = open.inspector.expect("open inspector").height;
    let open_strip = open.contributions.expect("open strip").height;
    assert_eq!(
        folded.detail.unwrap().height,
        open.detail.unwrap().height + (open_inspector - 1) + (open_strip - 1),
        "every row a fold gives up goes to the detail pane"
    );
}

/// The one row is a rule with the pane's tag on it, no corners, in the
/// pane's focus styling, and the tag is still where a click folds it.
#[test]
fn a_folded_row_is_a_rule_carrying_its_tag_in_focus_colors() {
    let (rects, buffer) = rendered(|a| {
        a.inspector_folded = true;
        a.contributions_folded = true;
        a.focused_panel = FocusedPanel::Inspector;
    });
    for (rect, panel, tag, tag_color, rule_color) in [
        (
            rects.inspector,
            FocusedPanel::Inspector,
            "[3]",
            colors::LAVENDER,
            colors::LAVENDER,
        ),
        (
            rects.contributions,
            FocusedPanel::Contributions,
            "[4]",
            colors::OVERLAY,
            colors::SURFACE,
        ),
    ] {
        let r = rect.unwrap_or_else(|| panic!("{tag} pane not laid out"));
        let right = r.x + r.width - 1;
        for x in [r.x, right] {
            assert_eq!(buffer[(x, r.y)].symbol(), "─", "{tag}: no corner");
            assert_eq!(buffer[(x, r.y)].fg, rule_color, "{tag}: rule color");
        }
        let x = column_of(&buffer, r.y, tag).unwrap_or_else(|| panic!("{tag} not drawn"));
        assert_eq!(buffer[(x, r.y)].fg, tag_color, "{tag}: tag color");
        assert_eq!(
            rects.fold_tag_at(x, r.y),
            Some(panel),
            "{tag}: click target"
        );
    }
}

/// Below a folded inspector's one row there is no frame left: the next
/// row is the gap above the strip, and the strip's title follows it.
#[test]
fn the_row_below_a_folded_inspector_belongs_to_the_next_pane() {
    let (rects, buffer) = rendered(|a| {
        a.inspector_folded = true;
        a.contributions_folded = true;
    });
    let inspector = rects.inspector.expect("folded inspector laid out");
    let strip = rects.contributions.expect("folded strip laid out");
    let below = inspector.y + inspector.height;
    assert_eq!(below, inspector.y + 1, "the fold is the title row alone");
    assert_ne!(
        rects.at(inspector.x + 2, below),
        Some(FocusedPanel::Inspector)
    );
    for x in [
        inspector.x,
        inspector.x + 2,
        inspector.x + inspector.width - 1,
    ] {
        assert_eq!(buffer[(x, below)].symbol(), " ", "no frame below at {x}");
    }
    assert_eq!(strip.y, below + 1, "the strip follows the one-row gap");
    assert!(column_of(&buffer, strip.y, "contributions").is_some());
}

/// A folded inspector has nothing to resize, and its top row is its tag.
#[test]
fn a_folded_inspector_is_not_a_divider() {
    let (open, _) = rendered(|_| {});
    let r = open.inspector.unwrap();
    assert_eq!(open.divider_at(r.x + 20, r.y), Some(Divider::Horizontal));

    let (folded, _) = rendered(|a| a.inspector_folded = true);
    let r = folded.inspector.unwrap();
    assert_eq!(folded.divider_at(r.x + 20, r.y), None);
}
