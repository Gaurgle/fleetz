//! Grab handles on the two draggable pane boundaries.
//!
//! Nothing on screen used to say a divider could be dragged: you had to know,
//! or read the help overlay. A short mark centred on each boundary says it the
//! way a GUI splitter does.
//!
//! The marks are drawn from the same `PanelRects` that `ui::render` painted
//! from and `PanelRects::divider_at` hit-tests against, so a grip cannot sit
//! on a column that is not actually grabbable. They are a hint only: the whole
//! divider line keeps its `GRAB_SLOP` target, since narrowing the target to
//! the mark would undo the point of drawing one.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Color;

use crate::app::App;
use crate::ui::{Divider, PanelRects, colors};

/// Cells a grip spans along its divider, per orientation.
///
/// Not one constant, because a terminal cell is about twice as tall as it is
/// wide: three of them stacked draw a mark twice as long as three of them in a
/// row, and the two handles read as different controls. Six columns against
/// three rows brings them to roughly the same length on screen.
///
/// Three rows is the floor for the vertical one -- fewer reads as a speck --
/// and six columns still leaves the horizontal one clear of the pane title it
/// shares a row with.
const GRIP_ROWS: u16 = 3;
const GRIP_COLS: u16 = 6;

/// The marks the grips are drawn with: braille, for the texture of a real grip
/// pad, but one glyph per orientation.
///
/// A braille cell is two dots wide by three tall, so it does not survive a
/// ninety-degree turn. `⠿` fills the cell, which stacks into a bar two dots
/// thick; laid in a row the same glyph draws a bar *three* dots thick, and the
/// two handles stop looking like the same control. `⠶` lights only the middle
/// two dot-rows, so the horizontal bar comes out two dots thick as well, and
/// centred in the cell rather than riding high in it.
const GRIP_VERTICAL: &str = "⠿";
const GRIP_HORIZONTAL: &str = "⠶";

/// Draw a grip on every boundary that can currently be dragged.
///
/// Called after the panes so the horizontal grip lands on top of the
/// inspector's own top border rather than under it.
pub fn render(frame: &mut Frame, rects: &PanelRects, app: &App) {
    // Compact has no vertical divider and no inspector, so it has nothing to
    // mark. `detail` being None says the same for a Detail frame too narrow to
    // split.
    let Some(_) = rects.detail else {
        return;
    };
    // Vertical: the gap column `split_main` leaves between the repo list and
    // the right column, which is exactly the column `divider_at` tests.
    // Centred on the repo list rather than on the whole main area, because the
    // gap only runs beside the list.
    let list = rects.repo_list;
    if list.height >= GRIP_ROWS {
        let x = list.x + list.width;
        let top = list.y + (list.height - GRIP_ROWS) / 2;
        let fg = grip_color(app, Divider::Vertical);
        for y in top..top + GRIP_ROWS {
            paint(frame, x, y, GRIP_VERTICAL, fg);
        }
    }

    // Horizontal: the inspector's own top border, the row `divider_at` treats
    // as the handle. Absent when the terminal is too short for an inspector,
    // in which case there is no horizontal divider to advertise.
    if let Some(inspector) = rects.inspector
        && let Some((left, right)) = inspector_grip_span(inspector)
    {
        let y = inspector.y;
        let fg = grip_color(app, Divider::Horizontal);
        for x in left..right {
            paint(frame, x, y, GRIP_HORIZONTAL, fg);
        }
    }
}

/// The columns the inspector's grip covers on its top border, `left..right`,
/// or `None` when the pane is too narrow to carry one. The inspector's title
/// line reads it to keep its tabs clear of the mark (FZ-64).
pub fn inspector_grip_span(inspector: Rect) -> Option<(u16, u16)> {
    if inspector.width < GRIP_COLS {
        return None;
    }
    let left = inspector.x + (inspector.width - GRIP_COLS) / 2;
    Some((left, left + GRIP_COLS))
}

/// A grip is furniture at rest and lights up while its own divider is being
/// dragged, so the mark that invited the drag is also the one confirming it.
fn grip_color(app: &App, divider: Divider) -> Color {
    match app.dragging == Some(divider) {
        true => colors::LAVENDER,
        false => colors::SURFACE,
    }
}

/// Write one cell, ignoring positions off the buffer. The rects come from the
/// same area the frame was sized to, so this only bites on a terminal too
/// small to have drawn the pane at all.
fn paint(frame: &mut Frame, x: u16, y: u16, symbol: &str, fg: Color) {
    if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
        cell.set_symbol(symbol);
        cell.set_fg(fg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::View;
    use crate::ui::panel_rects;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn draw(width: u16, height: u16, setup: impl FnOnce(&mut App)) -> (Buffer, PanelRects) {
        let mut app = App::new(Vec::new(), String::new(), width, height);
        setup(&mut app);
        let app = app;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| crate::ui::render(f, &app)).unwrap();
        let rects = panel_rects(Rect::new(0, 0, width, height), &app);
        (terminal.backend().buffer().clone(), rects)
    }

    /// Every cell in the buffer carrying either grip mark.
    fn grip_cells(buffer: &Buffer) -> Vec<(u16, u16)> {
        let area = buffer.area;
        (0..area.height)
            .flat_map(|y| (0..area.width).map(move |x| (x, y)))
            .filter(|&(x, y)| matches!(buffer[(x, y)].symbol(), GRIP_VERTICAL | GRIP_HORIZONTAL))
            .collect()
    }

    /// The dot-grid a braille glyph lights, as (rows, columns) counts. The
    /// cell is two dots wide by four tall, numbered down the left column
    /// (1, 2, 3, 7) then down the right (4, 5, 6, 8).
    fn braille_extent(glyph: &str) -> (usize, usize) {
        let bits = glyph.chars().next().expect("a glyph") as u32 - 0x2800;
        let cells = [
            (0x01, 0, 0),
            (0x02, 1, 0),
            (0x04, 2, 0),
            (0x40, 3, 0),
            (0x08, 0, 1),
            (0x10, 1, 1),
            (0x20, 2, 1),
            (0x80, 3, 1),
        ];
        let lit: Vec<_> = cells
            .iter()
            .filter(|(mask, _, _)| bits & mask != 0)
            .collect();
        let rows: std::collections::BTreeSet<_> = lit.iter().map(|(_, r, _)| r).collect();
        let cols: std::collections::BTreeSet<_> = lit.iter().map(|(_, _, c)| c).collect();
        (rows.len(), cols.len())
    }

    /// The failure this closes: both bars used `⠿`, and a braille cell is two
    /// dots wide by three tall. Stacked it draws a bar two dots thick; laid in
    /// a row the same glyph draws one *three* dots thick, so the handles did
    /// not survive the ninety-degree turn between them. Each bar's thickness
    /// is its glyph's extent across the divider it runs along.
    #[test]
    fn both_grips_draw_a_bar_two_dots_thick() {
        let (_, vertical_thickness) = braille_extent(GRIP_VERTICAL);
        let (horizontal_thickness, _) = braille_extent(GRIP_HORIZONTAL);
        assert_eq!(vertical_thickness, 2, "vertical bar is not 2 dots wide");
        assert_eq!(horizontal_thickness, 2, "horizontal bar is not 2 dots tall");
    }

    /// The failure this closes: a grip drawn a column off the real divider is
    /// worse than no grip, because it invites a drag at a cell that does not
    /// start one. Both marks are asserted to sit where `divider_at` answers.
    #[test]
    fn grips_land_only_on_cells_that_actually_grab_a_divider() {
        let (buffer, rects) = draw(120, 44, |app| app.view = View::Detail);
        let cells = grip_cells(&buffer);
        assert!(!cells.is_empty(), "no grip drawn");
        for (x, y) in cells {
            assert!(
                rects.divider_at(x, y).is_some(),
                "grip at ({x}, {y}) grabs nothing"
            );
        }
    }

    /// Both dividers get a mark, not just the one that happens to be first.
    #[test]
    fn both_dividers_are_marked() {
        let (buffer, rects) = draw(120, 44, |app| app.view = View::Detail);
        let inspector = rects.inspector.expect("inspector at this size");
        let cells = grip_cells(&buffer);
        assert!(
            cells.iter().any(|&(_, y)| y != inspector.y),
            "no vertical grip: {cells:?}"
        );
        assert!(
            cells.iter().any(|&(_, y)| y == inspector.y),
            "no horizontal grip: {cells:?}"
        );
    }

    /// The failure this closes: both grips used one length in cells, so the
    /// horizontal mark drew less than half the length of the vertical one on
    /// screen and the two did not read as the same control. A terminal cell is
    /// about twice as tall as it is wide, so the counts have to differ for the
    /// marks not to.
    #[test]
    fn the_two_grips_are_the_same_length_on_screen() {
        let (buffer, rects) = draw(120, 44, |app| app.view = View::Detail);
        let inspector = rects.inspector.expect("inspector at this size");
        let cells = grip_cells(&buffer);
        let rows = cells.iter().filter(|&&(_, y)| y != inspector.y).count();
        let cols = cells.iter().filter(|&&(_, y)| y == inspector.y).count();
        assert_eq!(
            cols,
            rows * 2,
            "horizontal grip spans {cols} columns against the vertical's {rows} rows"
        );
    }

    /// Compact has no vertical divider and no inspector. Marking a boundary
    /// there would promise a drag that does nothing.
    #[test]
    fn compact_view_draws_no_grips() {
        let (buffer, _) = draw(120, 44, |app| app.view = View::Compact);
        assert!(grip_cells(&buffer).is_empty());
    }

    /// The contribution strip's gap row looks identical to the vertical
    /// divider's gap column but is not draggable, so it must stay unmarked.
    #[test]
    fn the_contribution_strip_boundary_is_not_marked() {
        let (buffer, rects) = draw(120, 44, |app| {
            app.view = View::Detail;
            app.contributions = crate::app::ContributionsMode::Github;
        });
        let strip = rects.contributions.expect("strip open");
        for (_, y) in grip_cells(&buffer) {
            assert!(y < strip.y.saturating_sub(1), "grip on the strip boundary");
        }
    }

    /// A short terminal drops the inspector entirely; with no horizontal
    /// divider left there is nothing to advertise on that row.
    #[test]
    fn no_horizontal_grip_without_an_inspector() {
        let (buffer, rects) = draw(120, 18, |app| app.view = View::Detail);
        assert!(
            rects.inspector.is_none(),
            "expected no inspector at 18 rows"
        );
        let detail = rects.detail.expect("detail pane");
        for (x, _) in grip_cells(&buffer) {
            assert!(x < detail.x, "grip inside the detail pane");
        }
    }
}
