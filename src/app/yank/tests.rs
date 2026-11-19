use super::*;

// FZ-81: the copy flash, as pure functions of an injected instant.

fn flash_at(from_right: bool) -> (YankFlash, std::time::Instant) {
    let start = std::time::Instant::now();
    let source = FlashSource {
        pane: FocusedPanel::RepoList,
        view: HistoryView::Info,
        row: 0,
    };
    let mut flash = YankFlash::default();
    flash.show(source, from_right, start);
    (flash, start)
}

fn ms(n: u64) -> std::time::Duration {
    std::time::Duration::from_millis(n)
}

/// The backgrounds of a 10-cell row at `start + at`.
fn row_at(flash: &YankFlash, start: std::time::Instant, at: u64) -> Vec<ratatui::style::Color> {
    (0..10)
        .map(|c| flash.cell_bg(start + ms(at), c, 10))
        .collect()
}

#[test]
fn yy_sweeps_the_row_from_the_left_then_fades_to_the_band() {
    use crate::ui::colors::{SELECTED_BG, YANKED_BG, mix};
    let (flash, start) = flash_at(false);
    let (y, b) = (YANKED_BG, SELECTED_BG);
    assert_eq!(row_at(&flash, start, 0), vec![b; 10], "nothing swept yet");
    assert_eq!(
        row_at(&flash, start, 50),
        [vec![y; 5], vec![b; 5]].concat(),
        "the left half at 50 ms"
    );
    assert_eq!(row_at(&flash, start, 100), vec![y; 10], "fully yanked");
    let half = mix(YANKED_BG, SELECTED_BG, 0.5);
    assert_ne!(half, y);
    assert_ne!(half, b);
    assert_eq!(row_at(&flash, start, 200), vec![half; 10], "halfway faded");
    assert_eq!(row_at(&flash, start, 300), vec![b; 10], "the plain band");
    assert_eq!(row_at(&flash, start, 900), vec![b; 10]);
}

#[test]
fn yp_sweeps_the_row_from_the_right() {
    use crate::ui::colors::{SELECTED_BG, YANKED_BG};
    let (flash, start) = flash_at(true);
    assert_eq!(
        row_at(&flash, start, 50),
        [vec![SELECTED_BG; 5], vec![YANKED_BG; 5]].concat()
    );
    assert_eq!(row_at(&flash, start, 100), vec![YANKED_BG; 10]);
}

#[test]
fn no_flash_paints_the_plain_band() {
    let flash = YankFlash::default();
    let now = std::time::Instant::now();
    assert_eq!(flash.cell_bg(now, 0, 10), crate::ui::colors::SELECTED_BG);
    assert_eq!(flash.source(), None);
}

/// The loop waits at most `YANK_FRAME` while the flash lasts, not at
/// all once it is due to end, and the normal tick after `tick` ends it.
#[test]
fn the_loop_waits_a_frame_while_the_flash_is_live() {
    let tick = ms(100);
    let (mut flash, start) = flash_at(false);
    for at in [0, 50, 150, 280] {
        assert!(flash.wait(tick, start + ms(at)) <= YANK_FRAME, "at {at} ms");
    }
    assert_eq!(flash.wait(tick, start + ms(295)), ms(5), "the last frame");
    assert_eq!(flash.wait(tick, start + ms(300)), ms(0), "due to end");
    assert!(flash.tick(start + ms(150)), "live: redraw");
    assert!(flash.is_active());
    assert!(flash.tick(start + ms(300)), "ended: one last redraw");
    assert!(!flash.is_active());
    assert_eq!(flash.wait(tick, start + ms(310)), tick, "the normal tick");
    assert!(!flash.tick(start + ms(320)), "nothing left to redraw");
}
