//! Scroll arithmetic shared by the lists the cursor walks.
//!
//! The margin itself is `app::SCROLLOFF`, shrunk in a short pane by
//! `app::scrolloff_for`: pane 1 hands it to ratatui's `List::scroll_padding`,
//! which shrinks it the same way, and pane 2's views follow their cursor
//! through it. This module holds what happens when a row expands in place.

use crate::app::scrolloff_for;

/// The scroll offset that reveals a row just expanded in place, in a pane
/// `height` rows tall scrolled to `offset`. The expanded row starts on body
/// row `folder` and now spans `rows` rows, its own first.
///
/// Scrolls down far enough that the last of those rows sits on the bottom
/// row, but never so far that the expanded row leaves the top margin
/// (`scrolloff_for(height)`, the margin the cursor keeps). Rows that already
/// fit keep `offset`, and a reveal never scrolls up. Ported from notez
/// (`d32a412`).
pub fn reveal_offset(rows: u16, folder: u16, offset: u16, height: u16) -> u16 {
    let last = folder + rows.saturating_sub(1);
    let margin = scrolloff_for(height);
    let bottom = (last + 1).saturating_sub(height);
    offset.max(bottom.min(folder.saturating_sub(margin)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The expanded row on body row 14 of a 20-row pane; its eight rows end
    /// on row 21, below the pane, so the view scrolls two rows and no
    /// further.
    #[test]
    fn reveal_puts_the_last_expanded_row_on_the_bottom_row() {
        let offset = reveal_offset(8, 14, 0, 20);
        assert_eq!(offset, 2);
        assert_eq!(offset + 20 - 1, 14 + 8 - 1, "last row on the bottom row");
    }

    /// An expansion taller than the pane cannot show whole, so the expanded
    /// row stops at the top margin: what it opened is read from its head.
    #[test]
    fn reveal_of_rows_taller_than_the_pane_stops_at_the_top_margin() {
        let offset = reveal_offset(30, 14, 0, 20);
        assert_eq!(14 - offset, scrolloff_for(20));
        assert_eq!(14 - offset, crate::app::SCROLLOFF);
    }

    /// Eight rows: the margin shrinks to (8 - 1) / 2 = 3, so the expanded
    /// row climbs to row 3 of the pane, not to the full margin's row.
    #[test]
    fn reveal_in_a_short_pane_uses_the_shrunken_margin() {
        let offset = reveal_offset(11, 14, 10, 8);
        assert_eq!(offset, 11);
        assert_eq!(14 - offset, 3);
    }

    #[test]
    fn reveal_keeps_the_offset_when_the_rows_already_fit() {
        assert_eq!(reveal_offset(6, 14, 0, 20), 0);
        assert_eq!(reveal_offset(1, 14, 0, 20), 0);
        // Exactly to the bottom row is still a fit.
        assert_eq!(reveal_offset(6, 14, 0, 20), 0);
        assert_eq!(reveal_offset(7, 14, 0, 20), 1);
    }

    /// The expanded row already sits on row 2, inside the top margin: the
    /// reveal leaves it there rather than pulling the view back up.
    #[test]
    fn reveal_never_scrolls_up() {
        assert_eq!(reveal_offset(30, 14, 12, 20), 12);
        assert_eq!(reveal_offset(1, 14, 12, 20), 12);
    }
}
