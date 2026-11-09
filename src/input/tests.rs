use super::*;
use crate::poll::claim_github_poll;
use crate::tests::{
    RestoreIcons, answer_at, app_for_icons, apply_open_settings, at, budget_app, chord, ci_app,
    ci_key, ci_landed, drawn_cursor_row, drawn_icon_marks, drawn_icons_row, err_with, fetch_failed,
    his_notes, land_at, land_primary_limit, missing_paths_app, notes_tree_app, ok_with, on_screen,
    open_settings_on_the_icons_row, poll_says_passed, press, reading, status_text, timed_out_at,
    yank_app, yank_frame, yank_on, yank_repo, yank_status, yank_terminal,
};
use crate::{github, model, notes};
use clap::Parser;

/// FZ-34 (#83): `f` is manual, so it fetches a row held after a timeout.
#[tokio::test]
async fn f_fetches_a_row_held_after_a_timeout() {
    let mut app = missing_paths_app(1, "fkey");
    let held = timed_out_at(&mut app, 0);
    assert!(app.fetch_held(&held, chrono::Local::now()));
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    fetch_key(&mut app, &tx, None);
    assert!(
        app.fetches_in_flight.contains_key(&held),
        "f skipped the held row"
    );
}

/// FZ-34 (#83): a manual `r` or `R` lifts every timeout hold.
#[test]
fn r_and_shift_r_lift_the_timeout_holds() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    let budget = reading("A", 2, at(17, 23, 6), at(17, 0, 0));
    land_at(&mut app, ok_with(Some(budget)), at(17, 0, 0));
    let timed_out = fetch_failed(git::FetchFailureKind::TimedOut);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let args = Args::parse_from(["fleetz"]);

    app.fetch_backoff.record("~/a", &timed_out, at(17, 9, 0));
    assert!(app.fetch_held("~/a", at(17, 10, 0)));
    // Under a known limit `r` sends nothing, so no runtime is needed.
    refresh_key(&mut app, &tx, &args, &config, at(17, 10, 0));
    assert!(!app.fetch_held("~/a", at(17, 10, 0)), "r kept the hold");

    app.fetch_backoff.record("~/a", &timed_out, at(17, 9, 30));
    assert!(app.fetch_held("~/a", at(17, 10, 0)));
    // A load already running: `R` starts nothing new.
    app.loading = true;
    reload_key(
        &mut app,
        &tx,
        &args,
        &config,
        Path::new("/nonexistent"),
        at(17, 10, 0),
    );
    assert!(!app.fetch_held("~/a", at(17, 10, 0)), "R kept the hold");
}

/// `4` is the strip's key (back from `5`, FZ-30d). From another pane it
/// focuses a folded strip and unfolds it; on the focused strip it folds
/// it and keeps focus. Fails if the handler binds the strip to any other
/// key.
#[tokio::test]
async fn the_4_key_reaches_and_folds_the_strip() {
    let args = Args::parse_from(["fleetz"]);
    let config = config::Config::default();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let backend = CrosstermBackend::new(io::stdout());
    let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 120, 44));
    let mut terminal =
        Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap();
    let mut app = App::new(Vec::new(), String::new(), 120, 44);
    app.view = app::View::Detail;
    app.contributions = app::ContributionsMode::Github;
    app.focused_panel = FocusedPanel::Detail;
    app.contributions_folded = true;

    let repo_dir = Path::new("/nonexistent");
    handle_normal_input(
        &mut app,
        &mut terminal,
        press('4'),
        &tx,
        &args,
        &config,
        repo_dir,
    )
    .await
    .unwrap();
    assert_eq!(
        app.focused_panel,
        FocusedPanel::Contributions,
        "not focused"
    );
    assert!(!app.contributions_folded, "a folded strip stayed folded");

    handle_normal_input(
        &mut app,
        &mut terminal,
        press('4'),
        &tx,
        &args,
        &config,
        repo_dir,
    )
    .await
    .unwrap();
    assert!(app.contributions_folded, "the focused strip did not fold");
    assert_eq!(
        app.focused_panel,
        FocusedPanel::Contributions,
        "folding moved focus"
    );
}

/// Toggling the row and applying it changes the list row's kind glyph
/// and the header's watch mark on the next frame, and toggling again
/// brings the Nerd set back, with no restart. Fails if applying the
/// overlay leaves the active set alone.
#[test]
fn toggling_the_icons_row_redraws_the_rows_and_the_header() {
    let _restore = RestoreIcons::capture();
    ui::badges::set_active_icons(app::Icons::Nerd);
    let mut app = app_for_icons();
    let nerd = drawn_icon_marks(&app);
    assert_eq!(
        nerd.0,
        ui::badges::kind_glyph(model::RepoKind::Public).unwrap(),
        "the control did not draw the Nerd set"
    );

    open_settings_on_the_icons_row(&mut app);
    handle_settings_input(&mut app, press('l'));
    app.needs_redraw = false;
    apply_open_settings(&mut app);
    assert!(app.needs_redraw, "applying did not ask for a frame");
    assert_eq!(ui::badges::active_icons(), app::Icons::Plain);
    assert_eq!(
        drawn_icon_marks(&app),
        ("+".to_string(), "o".to_string()),
        "the next frame still draws the old set"
    );

    handle_settings_input(&mut app, press('l'));
    apply_open_settings(&mut app);
    assert_eq!(ui::badges::active_icons(), app::Icons::Nerd);
    assert_eq!(drawn_icon_marks(&app), nerd);
}

/// Stepping the icons row repaints the next frame at once, pane 1 and
/// the row both, with no Enter, no save and no config read in between,
/// and again the other way. Fails if the row only changes the overlay's
/// own copy until it is saved.
#[test]
fn stepping_the_icons_row_repaints_the_next_frame_without_a_save() {
    let _restore = RestoreIcons::capture();
    ui::badges::set_active_icons(app::Icons::Nerd);
    let mut app = app_for_icons();
    let nerd = drawn_icon_marks(&app);

    open_settings_on_the_icons_row(&mut app);
    assert!(drawn_icons_row(&app).contains("nerd"));
    handle_settings_input(&mut app, press('l'));
    assert_eq!(
        drawn_icon_marks(&app),
        ("+".to_string(), "o".to_string()),
        "pane 1 still draws the Nerd set after the step"
    );
    assert!(drawn_icons_row(&app).contains("plain"));

    handle_settings_input(&mut app, press('h'));
    assert_eq!(
        drawn_icon_marks(&app),
        nerd,
        "the step back did not repaint"
    );
    assert!(drawn_icons_row(&app).contains("nerd"));
}

/// Esc after toggling the row discards it, as it discards every row:
/// the row previews at once and Esc restores the set in force.
#[test]
fn cancelling_the_overlay_leaves_the_icon_set_as_it_was() {
    let _restore = RestoreIcons::capture();
    for before in [app::Icons::Nerd, app::Icons::Plain] {
        ui::badges::set_active_icons(before);
        let mut app = app_for_icons();
        let drawn = drawn_icon_marks(&app);

        open_settings_on_the_icons_row(&mut app);
        handle_settings_input(&mut app, press('l'));
        handle_settings_input(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(matches!(app.overlay, Overlay::None), "Esc did not close");
        assert_eq!(ui::badges::active_icons(), before);
        assert_eq!(drawn_icon_marks(&app), drawn);
    }
}

/// The overlay opens on the set the app is drawing with, not on a
/// default, so saving another row writes back the set in force. Fails
/// if a session switched to `plain` would save `nerd` the next time any
/// setting is saved.
#[test]
fn the_overlay_opens_on_the_icon_set_in_force() {
    let _restore = RestoreIcons::capture();
    let app = app_for_icons();
    for in_force in [app::Icons::Plain, app::Icons::Nerd] {
        ui::badges::set_active_icons(in_force);
        let so = settings_overlay(&app, &config::Config::default());
        assert_eq!(so.icons, in_force);
        let written = so
            .config_updates()
            .into_iter()
            .find(|(key, _)| *key == "icons")
            .map(|(_, value)| value);
        let expected = match in_force {
            app::Icons::Nerd => "nerd",
            app::Icons::Plain => "plain",
        };
        assert_eq!(written, Some(config::SettingValue::Str(expected)));
    }
}

#[test]
fn s15_r_under_a_rate_limit_sweeps_and_sends_nothing() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    land_primary_limit(&mut app);
    let before = app.github_freshness.clone();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let args = Args::parse_from(["fleetz"]);
    // Outside a runtime: a spawned request would panic here.
    refresh_key(&mut app, &tx, &args, &config, at(17, 10, 0));
    assert!(!app.github_poll_inflight, "a request was sent");
    assert!(status_text(&app).contains("17:23"), "{}", status_text(&app));
    assert_eq!(app.github_freshness, before);
}

#[test]
fn s16a_r_under_a_rate_limit_with_nothing_in_memory_only_names_the_resume() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    land_primary_limit(&mut app);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let args = Args::parse_from(["fleetz"]);
    // Outside a runtime: a spawned load would panic here.
    reload_key(
        &mut app,
        &tx,
        &args,
        &config,
        Path::new("/nonexistent"),
        at(17, 10, 0),
    );
    assert_eq!(status_text(&app), "GitHub rate limited, resumes at 17:23");
    assert!(!app.loading);
    assert!(!app.github_poll_inflight);
}

#[test]
fn s16b_r_under_a_rate_limit_with_a_poll_in_flight_waits_for_it() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    app.dataset = poll_says_passed().ok().unwrap().repos;
    land_primary_limit(&mut app);
    let in_flight = claim_github_poll(&mut app).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let args = Args::parse_from(["fleetz"]);
    reload_key(
        &mut app,
        &tx,
        &args,
        &config,
        Path::new("/nonexistent"),
        at(17, 10, 0),
    );
    assert_eq!(status_text(&app), REFRESH_RUNNING);
    assert!(!app.loading, "a rebuild started");

    let budget = reading("A", 0, at(17, 23, 6), at(17, 10, 5));
    answer_at(
        &mut app,
        in_flight,
        err_with(
            github::Failure::rate_limit("API rate limit exceeded"),
            Some(budget),
        ),
        at(17, 10, 5),
    );
    assert_eq!(
        reload_action(&app, &config, at(17, 10, 6)),
        ReloadAction::FromMemory
    );
}

#[test]
fn s18_r_while_a_load_runs_is_ignored() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    app.loading = true;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let args = Args::parse_from(["fleetz"]);
    // Outside a runtime: a second load would panic here.
    reload_key(
        &mut app,
        &tx,
        &args,
        &config,
        Path::new("/nonexistent"),
        at(17, 0, 0),
    );
    assert_eq!(status_text(&app), REFRESH_RUNNING);
    assert!(!app.github_poll_inflight);
}

/// The key handler hands keys to the chord: `y` on the compact grid
/// copies nothing and arms it, and the `j` after it ends it and still
/// moves the cursor. Fails if the handler stops calling the chord.
#[tokio::test]
async fn the_key_handler_feeds_the_yank_chord() {
    let args = Args::parse_from(["fleetz"]);
    let config = config::Config::default();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut terminal = yank_terminal();
    let mut app = App::new(vec![yank_repo(0), yank_repo(0)], String::new(), 120, 40);
    app.view = app::View::Compact;
    app.focused_panel = FocusedPanel::RepoList;
    let repo_dir = Path::new("/nonexistent");
    handle_normal_input(
        &mut app,
        &mut terminal,
        press('y'),
        &tx,
        &args,
        &config,
        repo_dir,
    )
    .await
    .unwrap();
    assert!(app.pending_yank, "`y` did not reach the chord");
    assert_eq!(yank_status(&app), None, "`y` alone copied something");
    handle_normal_input(
        &mut app,
        &mut terminal,
        press('j'),
        &tx,
        &args,
        &config,
        repo_dir,
    )
    .await
    .unwrap();
    assert!(!app.pending_yank, "`j` left the chord armed");
    assert_eq!(app.selected, 1, "`j` after `y` did not move the cursor");
}

/// A mouse press ends the chord: `y`, a click, then `p` copies nothing.
#[test]
fn a_click_ends_the_yank_chord() {
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    let terminal = yank_terminal();
    assert!(chord(&mut app, "y").is_empty());
    assert!(app.pending_yank);
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse_input(&mut app, &terminal, click);
    assert!(!app.pending_yank, "the click left the chord armed");
    assert!(
        chord(&mut app, "p").is_empty(),
        "`p` after the click copied"
    );
}

#[test]
fn a_click_outside_any_row_does_nothing() {
    let mut app = yank_app(app::HistoryView::Commits, 0);
    app.set_detail_selected(1);
    let buf = yank_frame(&app);
    let pane = ui::panel_rects(buf.area, &app).detail.unwrap();
    let (_, last) = on_screen(&buf, &app, "ccc3333");
    let (x, header) = on_screen(&buf, &app, "/tmp/yank");
    let mut copied = Vec::new();
    // Below the last row, in the pinned header, and on the border.
    for (cx, cy) in [(x, last + 2), (x, header), (pane.left(), last)] {
        click_detail(&mut app, cx, cy, |h| {
            copied.push(h.to_string());
            Ok(())
        });
    }
    assert!(copied.is_empty(), "copied {copied:?}");
    assert_eq!(app.inspected_hash(), Some("bbb2222"), "the cursor moved");
    assert!(app.status_message.is_none());
}

/// FZ-37 review: a click copies the hash that was on screen, even when
/// the list changed between the paint and the click (a `RepoUpdated`
/// landing in between).
#[test]
fn a_click_copies_the_painted_hash_after_the_list_changes() {
    let mut app = yank_app(app::HistoryView::Commits, 0);
    let buf = yank_frame(&app);
    let (x, y) = on_screen(&buf, &app, "bbb2222");
    // A new commit lands on top: index 1 now names `aaa1111`.
    let mut newer = app.repos[0].graph[0].clone();
    newer.hash = "fff0000".to_string();
    app.repos[0].graph.insert(0, newer);
    let mut copied = Vec::new();
    click_detail(&mut app, x, y, |h| {
        copied.push(h.to_string());
        Ok(())
    });
    assert_eq!(copied, ["bbb2222"]);
    assert_eq!(
        yank_status(&app),
        Some(("copied bbb2222".to_string(), false))
    );
}

/// FZ-59 owner ruling: `p` flips the `notes` reader between rendered
/// and source from the tree and from the reader, and `r` refreshes there
/// as everywhere. Fails while `r` is the toggle or `p` is unbound.
#[tokio::test]
async fn p_flips_the_notes_reader_and_r_still_refreshes() {
    let args = Args::parse_from(["fleetz"]);
    let config = config::Config::default();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let backend = CrosstermBackend::new(io::stdout());
    let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 120, 44));
    let mut terminal =
        Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap();
    let mut app = App::new(Vec::new(), String::new(), 120, 44);
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Notes;
    // No poll leaves the process: `r` below only sets its status.
    app.dirty_mode = true;
    let repo_dir = Path::new("/nonexistent");

    for (panel, raw) in [(FocusedPanel::Detail, true), (FocusedPanel::Reader, false)] {
        app.focused_panel = panel;
        handle_normal_input(
            &mut app,
            &mut terminal,
            press('p'),
            &tx,
            &args,
            &config,
            repo_dir,
        )
        .await
        .unwrap();
        assert_eq!(app.reader_raw, raw, "p from {panel:?}");

        app.status_message = None;
        handle_normal_input(
            &mut app,
            &mut terminal,
            press('r'),
            &tx,
            &args,
            &config,
            repo_dir,
        )
        .await
        .unwrap();
        assert_eq!(app.reader_raw, raw, "r flipped the reader from {panel:?}");
        let status = app.status_message.as_ref().map(|m| m.text.as_str());
        assert_eq!(
            status,
            Some("Refreshing…"),
            "r did not refresh from {panel:?}"
        );
    }
}

/// FZ-65: the wheel over the `notes` tree keeps the highlighted file
/// inside the rows the tree draws, at a narrow, short pane and with his
/// terminal's two events per notch. Fails while the wheel only scrolls
/// the tree's view and leaves the highlight behind, off the drawn rows
/// within one notch.
#[test]
fn the_notes_tree_wheel_keeps_the_highlight_on_the_drawn_rows() {
    const W: u16 = 64;
    const H: u16 = 26;
    let area = ratatui::layout::Rect::new(0, 0, W, H);
    let mut app = notes_tree_app(W, H, 40);
    let tree = ui::panel_rects(area, &app).detail.expect("the tree");
    // Inside the border: the rows a file can be drawn on.
    let body = ratatui::layout::Rect::new(tree.x + 1, tree.y + 1, tree.width - 2, tree.height - 2);
    // The first frame measures the pane the wheel steps against.
    drawn_cursor_row(&app, W, H, body);
    let wheel = |kind| MouseEvent {
        kind,
        column: tree.x + tree.width / 2,
        row: tree.y + tree.height / 2,
        modifiers: KeyModifiers::NONE,
    };

    for (kind, notches) in [
        (MouseEventKind::ScrollDown, 20),
        (MouseEventKind::ScrollUp, 20),
    ] {
        for notch in 0..notches {
            let before = app.detail_selected();
            // Two events a notch, as his terminal sends them, and one
            // frame after both.
            handle_mouse_in(&mut app, area, wheel(kind));
            handle_mouse_in(&mut app, area, wheel(kind));
            let row = drawn_cursor_row(&app, W, H, body);
            assert!(
                row.is_some(),
                "{kind:?} notch {notch}: the highlight (file {}) left the drawn rows",
                app.detail_selected()
            );
            assert_ne!(
                app.detail_selected(),
                before,
                "{kind:?} notch {notch} moved nothing"
            );
        }
    }
    assert_eq!(app.detail_selected(), 0, "the wheel walks back to the top");
}

/// FZ-65 keeps the rule the wheel had before it: on the commit list it
/// scrolls the view and leaves the highlighted commit where it is.
#[test]
fn the_wheel_on_the_commit_list_still_leaves_its_highlight() {
    const W: u16 = 64;
    const H: u16 = 26;
    let area = ratatui::layout::Rect::new(0, 0, W, H);
    let mut app = notes_tree_app(W, H, 40);
    app.history_view = app::HistoryView::Commits;
    app.detail_max_scroll.set(50);
    let pane = ui::panel_rects(area, &app).detail.expect("pane 2");
    let wheel = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: pane.x + pane.width / 2,
        row: pane.y + pane.height / 2,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse_in(&mut app, area, wheel);
    handle_mouse_in(&mut app, area, wheel);
    assert_eq!(app.detail_selected(), 0, "the highlight stayed");
    assert_eq!(
        app.detail_scroll(),
        2 * DETAIL_WHEEL_ROWS as u16,
        "the view scrolled"
    );
}

/// FZ-65 rework 1, his look on the branch build: at his window the
/// highlight ran six wheel notches past the tree's last drawn row before
/// the tree moved. Long names wrap in the tree's column, and the follow
/// counted lines where the tree draws wrapped rows. Walks the tree down
/// and back up a notch at a time at two real window sizes and asserts
/// the highlight is drawn inside the tree's rows on every notch, and
/// inside the `SCROLLOFF` margin while the tree has rows past it.
#[test]
fn the_notes_tree_wheel_follows_wrapped_rows_at_his_window() {
    for (w, h) in [(200u16, 50u16), (120, 40), (64, 26), (80, 24), (100, 30)] {
        let area = ratatui::layout::Rect::new(0, 0, w, h);
        let mut app = notes_tree_app(w, h, 0);
        let walk = app::NotesWalk {
            key: "/repo".to_string(),
            root: "/repo".into(),
            generation: 1,
        };
        app.apply_notes_walk(walk, Ok(notes::Notes { files: his_notes() }));
        let pane2 = ui::panel_rects(area, &app).detail.expect("pane 2");
        let body = ui::detail::notes_pane_body(pane2, &app)
            .expect("a tree")
            .body;
        drawn_cursor_row(&app, w, h, body);
        let files = app.selected_commit_count();
        assert_eq!(files, 36, "{w}x{h}: one stop per file");
        // The rows the tree scrolls in: under the mode line where it is
        // pinned, the whole body where it is not.
        let rows = app.detail_view_height.get();
        let (top, bottom) = (body.bottom() - rows, body.bottom() - 1);
        let pad = app::scrolloff_for(rows);
        let wheel = |kind| MouseEvent {
            kind,
            column: body.x + body.width / 2,
            row: body.y + body.height / 2,
            modifiers: KeyModifiers::NONE,
        };
        for kind in [MouseEventKind::ScrollDown, MouseEventKind::ScrollUp] {
            let mut past = Vec::new();
            let mut misplaced = Vec::new();
            let mut in_margin = Vec::new();
            for notch in 0..files {
                let before = app.detail_selected();
                handle_mouse_in(&mut app, area, wheel(kind));
                handle_mouse_in(&mut app, area, wheel(kind));
                let drawn = drawn_cursor_row(&app, w, h, body);
                // Past either end the wheel scrolls the header or the
                // tail in instead (round 1), which on a pane too short
                // to pin the header takes the first file off the foot.
                if app.detail_selected() == before {
                    continue;
                }
                let Some(row) = drawn else {
                    past.push(notch);
                    continue;
                };
                // Where the follow counted it: its line less the offset.
                let line = app.detail_row_tops.borrow()[app.detail_selected()];
                if row != top + line - app.detail_scroll() {
                    misplaced.push(notch);
                }
                let more_below = app.detail_scroll() < app.detail_max_scroll.get();
                let more_above = app.detail_scroll() > 0;
                let margin = match kind {
                    MouseEventKind::ScrollDown => more_below && row > bottom - pad,
                    _ => more_above && row < top + pad,
                };
                if margin {
                    in_margin.push(notch);
                }
            }
            assert!(
                past.is_empty(),
                "{w}x{h} {kind:?}: the highlight ran {} notches past the drawn rows \
                 (notches {past:?})",
                past.len()
            );
            assert!(
                misplaced.is_empty(),
                "{w}x{h} {kind:?}: the highlight was drawn off the row the follow \
                 counted on notches {misplaced:?}"
            );
            assert!(
                in_margin.is_empty(),
                "{w}x{h} {kind:?}: the view did not follow at the margin on notches \
                 {in_margin:?}"
            );
        }
        assert_eq!(app.detail_selected(), 0, "{w}x{h}: back at the top");
    }
}

// FZ-30d: the `ci` view in pane 2 is the only thing that polls.

/// `]` from `branches` reaches `ci`, and there `j`/`k` walk its runs in
/// the view's own selection slot, never the repo list. Fails while `ci`
/// is not a pane 2 view.
#[tokio::test]
async fn j_and_k_walk_the_runs_in_the_ci_view() {
    let args = Args::parse_from(["fleetz"]);
    let config = config::Config::default();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let backend = CrosstermBackend::new(io::stdout());
    let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 120, 44));
    let mut terminal =
        Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap();
    let mut app = ci_app();
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Branches;
    ci_landed(&mut app, at(12, 0, 0));
    app.focused_panel = FocusedPanel::Detail;
    let repo_dir = Path::new("/nonexistent");
    handle_normal_input(
        &mut app,
        &mut terminal,
        press(']'),
        &tx,
        &args,
        &config,
        repo_dir,
    )
    .await
    .unwrap();
    assert_eq!(app.history_view, app::HistoryView::Ci);
    assert!(app.ci_wanted());
    let runs = app.ci[&ci_key()].runs.len();
    assert!(runs >= 3, "fixture has {runs} runs");
    for (key, cursor) in [('j', 1), ('j', 2), ('k', 1)] {
        handle_normal_input(
            &mut app,
            &mut terminal,
            press(key),
            &tx,
            &args,
            &config,
            repo_dir,
        )
        .await
        .unwrap();
        assert_eq!(app.detail_selected(), cursor, "after {key}");
    }
    assert_eq!(app.selected, 0, "the repo list did not move");
}

/// FZ-30f fix cycle 3: `]` from the focused inspector in `branches`
/// enters `ci` with its runs held and the next `[` goes back to `branches`;
/// back in `ci`, any other key ends the streak and `]` steps the bodies.
/// Fails while the second `[` sticks on the bodies.
#[tokio::test]
async fn a_bracket_streak_cycles_views_through_ci_until_another_key() {
    let args = Args::parse_from(["fleetz"]);
    let config = config::Config::default();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let backend = CrosstermBackend::new(io::stdout());
    let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 120, 44));
    let mut terminal =
        Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap();
    let mut app = ci_app();
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Ci;
    ci_landed(&mut app, at(12, 0, 0));
    app.history_view = app::HistoryView::Branches;
    app.focused_panel = FocusedPanel::Inspector;
    let repo_dir = Path::new("/nonexistent");
    let mut views = Vec::new();
    for key in [']', '[', ']', 'j', ']'] {
        handle_normal_input(
            &mut app,
            &mut terminal,
            press(key),
            &tx,
            &args,
            &config,
            repo_dir,
        )
        .await
        .unwrap();
        views.push((app.history_view, app.ci_body));
    }
    use app::{CiBody, HistoryView};
    assert_eq!(
        views,
        vec![
            (HistoryView::Ci, CiBody::Jobs),
            (HistoryView::Branches, CiBody::Jobs),
            (HistoryView::Ci, CiBody::Jobs),
            (HistoryView::Ci, CiBody::Jobs),
            (HistoryView::Ci, CiBody::Log),
        ]
    );
}
