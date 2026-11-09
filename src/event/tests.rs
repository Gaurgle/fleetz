use super::*;
use crate::app::FocusedPanel;
use crate::tests::{app_under_loading_field, chord, press, shows_loading_banner, yank_on};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent};

/// An idle loop paints as soon as it has something to paint. This is the
/// ordinary case and must not be delayed by the coalescing at all.
#[test]
fn an_empty_queue_paints_immediately() {
    assert!(should_paint(true, 0, false));
}

/// An animation frame is a deadline, not a data update: it is never held,
/// however much background work is queued behind it. Holding them is what
/// made the intro stutter under a release build's faster repo probes.
#[test]
fn an_animation_frame_is_never_held() {
    assert!(should_paint(false, 0, true));
    assert!(should_paint(false, MAX_COALESCED_FRAMES - 1, true));
    assert!(should_paint(true, 0, true));
}

/// The bug: a trackpad flick delivers wheel events faster than a repaint
/// takes, so painting one frame per event let the queue run away and the
/// scroll froze, then played out the backlog after the gesture had ended.
#[test]
fn a_queued_burst_holds_the_paint() {
    assert!(!should_paint(false, 0, false));
    assert!(!should_paint(false, 1, false));
    assert!(!should_paint(false, MAX_COALESCED_FRAMES - 1, false));
}

/// The hold is bounded, so a source that never stops arriving still gets
/// drawn rather than freezing the pane for as long as it lasts.
#[test]
fn a_queue_that_never_drains_still_paints_eventually() {
    assert!(should_paint(false, MAX_COALESCED_FRAMES, false));
    assert!(should_paint(false, MAX_COALESCED_FRAMES + 1, false));
}

/// `intro = false` puts no loading field up, and neither does a launch
/// with no load running. `intro = true` with a load running does.
#[test]
fn intro_off_shows_no_loading_field() {
    assert!(!shows_loading_field(false, true));
    assert!(!shows_loading_field(false, false));
    assert!(!shows_loading_field(true, false));
    assert!(shows_loading_field(true, true));
}

/// The loading field is up until the load lands, and the very next frame
/// is the finished screen exactly: nothing is drawn in between, so there is
/// no wave frame and no noise left over. The removed intro wave painted
/// 1.5 s of frames here.
#[test]
fn the_frame_after_the_load_lands_is_the_finished_screen() {
    use ratatui::backend::TestBackend;
    let mut app = app_under_loading_field();
    let running = std::sync::atomic::AtomicBool::new(true);
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|frame| draw_screen(frame, &app)).unwrap();
    assert!(
        shows_loading_banner(term.backend().buffer()),
        "the loading field was not painted while the load ran"
    );

    // What the `Loaded` arm does: the load is applied, then the field goes.
    app.loading = false;
    dismiss_loading_field(&mut app, &running);
    assert!(app.loading_field.is_none(), "the field is still up");
    assert!(
        !running.load(std::sync::atomic::Ordering::Relaxed),
        "the field's frame timer is still running"
    );
    term.draw(|frame| draw_screen(frame, &app)).unwrap();

    let mut finished = Terminal::new(TestBackend::new(120, 40)).unwrap();
    finished.draw(|frame| ui::render(frame, &app)).unwrap();
    assert_eq!(
        term.backend().buffer(),
        finished.backend().buffer(),
        "the frame after the load is not the finished screen"
    );
}

/// The bug (#64): a `LoadingFrame` queued behind `Loaded` arrives after the
/// load's paint was held for it, and overwrote `needs_redraw` with false.
/// The held paint was lost and the loading field stayed on screen until
/// something else asked for a redraw.
#[test]
fn a_late_loading_frame_keeps_the_held_paint_of_the_finished_screen() {
    let mut app = app_under_loading_field();
    let running = std::sync::atomic::AtomicBool::new(true);

    // What the `Loaded` arm does: the load is applied, the field goes, and
    // the finished screen is asked for.
    app.loading = false;
    dismiss_loading_field(&mut app, &running);
    app.needs_redraw = true;
    // The late frame is already queued and nothing animates, so the loop
    // holds that paint for it and leaves the flag set.
    assert!(!should_paint(false, 0, app.loading_field.is_some()));

    on_loading_frame(&mut app);
    assert!(
        app.needs_redraw,
        "the late frame cancelled the held paint of the finished screen"
    );
    // The queue is drained now, so the finished screen paints at once.
    assert!(should_paint(true, 1, app.loading_field.is_some()));
}

/// While the field is up, each of its frames asks for a repaint.
#[test]
fn a_loading_frame_while_the_field_is_up_asks_for_a_redraw() {
    let mut app = app_under_loading_field();
    app.needs_redraw = false;
    on_loading_frame(&mut app);
    assert!(app.needs_redraw);
}

/// A frame that arrives after the field is gone, with nothing pending,
/// does not invent a repaint.
#[test]
fn a_late_loading_frame_with_no_pending_paint_asks_for_none() {
    let mut app = app_under_loading_field();
    let running = std::sync::atomic::AtomicBool::new(true);
    dismiss_loading_field(&mut app, &running);
    app.needs_redraw = false;
    on_loading_frame(&mut app);
    assert!(!app.needs_redraw);
}

/// A resize while the field is up paints the field at the new size, banner
/// re-centred and no cell left over from the old layout.
#[test]
fn a_resize_while_loading_paints_the_field_at_the_new_size() {
    use ratatui::backend::TestBackend;
    let app = app_under_loading_field();
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|frame| draw_screen(frame, &app)).unwrap();

    term.backend_mut().resize(80, 24);
    term.draw(|frame| draw_screen(frame, &app)).unwrap();
    let buf = term.backend().buffer();
    assert_eq!(buf.area, ratatui::layout::Rect::new(0, 0, 80, 24));
    assert!(
        shows_loading_banner(buf),
        "the banner is gone after a resize"
    );

    let mut fresh = ratatui::buffer::Buffer::empty(buf.area);
    app.loading_field
        .as_ref()
        .expect("the field")
        .paint(&mut fresh);
    let differing_symbols = (0..24u16)
        .flat_map(|y| (0..80u16).map(move |x| (x, y)))
        .filter(|(x, y)| {
            // The noise churns with the clock, so compare where the banner
            // sits, which is still and derived from the area alone.
            buf[(*x, *y)].symbol().starts_with('█') != fresh[(*x, *y)].symbol().starts_with('█')
        })
        .count();
    assert_eq!(
        differing_symbols, 0,
        "the banner is not where a fresh field at the new size puts it"
    );
}

#[test]
fn a_key_press_a_click_or_a_scroll_ends_the_flash_and_motion_does_not() {
    use crossterm::event::{KeyEventKind, KeyEventState};
    let mouse = |kind| {
        Event::Mouse(MouseEvent {
            kind,
            column: 3,
            row: 3,
            modifiers: KeyModifiers::NONE,
        })
    };
    let release = Event::Key(KeyEvent {
        code: KeyCode::Char('j'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Release,
        state: KeyEventState::NONE,
    });
    let keeps = [
        mouse(MouseEventKind::Moved),
        Event::Resize(80, 24),
        Event::FocusLost,
        Event::FocusGained,
        release,
    ];
    let ends = [
        Event::Key(press('j')),
        mouse(MouseEventKind::Down(MouseButton::Left)),
        mouse(MouseEventKind::ScrollDown),
        Event::Paste("x".to_string()),
    ];
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    for ev in &keeps {
        chord(&mut app, "yy");
        end_yank_flash_on(&mut app, ev);
        assert!(app.yank_flash.is_active(), "{ev:?} ended the flash");
    }
    for ev in &ends {
        chord(&mut app, "yy");
        end_yank_flash_on(&mut app, ev);
        assert!(!app.yank_flash.is_active(), "{ev:?} left the flash");
    }
}

/// With a flash live and nothing queued, the loop wakes for a frame on
/// its own; with none it waits on the channel alone, as before FZ-81.
#[tokio::test]
async fn the_loop_wakes_for_the_flash_frame_and_only_while_it_lasts() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    let bound = std::time::Duration::from_millis(80);
    let idle = tokio::time::timeout(bound, next_event(&mut rx, &app.yank_flash)).await;
    assert!(idle.is_err(), "woke with no flash and nothing queued");
    chord(&mut app, "yy");
    let frame = tokio::time::timeout(bound, next_event(&mut rx, &app.yank_flash)).await;
    assert!(matches!(frame, Ok(Some(AppEvent::YankFrame))), "no frame");
    tx.send(AppEvent::Tick).unwrap();
    let queued = next_event(&mut rx, &app.yank_flash).await;
    assert!(
        matches!(queued, Some(AppEvent::Tick)),
        "a queued event waits"
    );
}

/// A live flash is an animation: its frames are painted through a burst
/// of queued background events, not held behind it.
#[test]
fn a_live_flash_is_never_held_behind_queued_events() {
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    assert!(!is_animating(&app));
    chord(&mut app, "yy");
    assert!(is_animating(&app), "the flash is not an animation");
    assert!(should_paint(false, 0, is_animating(&app)));
    app.yank_flash.clear();
    assert!(!is_animating(&app));
}
