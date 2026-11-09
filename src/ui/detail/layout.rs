use ratatui::layout::{Constraint, Layout, Rect};

use super::notes::notes_mode_line_for;

/// Smallest useful inspector (incl. border): header line, blank, a couple of
/// message lines, and the stat. Below this it is dropped rather than shown as
/// a sliver.
const INSPECTOR_MIN_HEIGHT: u16 = 8;

/// Ceiling on the inspector's share of the right column. A commit's details
/// are bounded in a way the detail pane's history list is not, so past this
/// the extra rows are better spent on the pane above.
const INSPECTOR_MAX_HEIGHT: u16 = 24;

/// Rows the panes above keep for themselves before the inspector gets any.
const DETAIL_MIN_HEIGHT: u16 = 8;

/// Split the main area into the row that holds the repo list and detail pane
/// and, when there is room for a useful one, the inspector docked under both.
///
/// Single source of truth for that geometry: `render` draws from it and
/// `ui::panel_rects` hit-tests mouse clicks against it. They used to carry
/// separate copies of the same arithmetic, kept in sync by hand, where any
/// drift would land clicks on the wrong pane.
///
/// The inspector spans the full width rather than the right column alone: a
/// commit's message and its file list both want columns, while the repo list
/// is rarely tall and scrolls when it has to be.
///
/// `pct` is the inspector's requested share of the area, which the resize
/// keys move. It is a request, not a promise: the min/max clamp and the row
/// above's floor both outrank it, so a short terminal still yields a readable
/// split (or drops the inspector entirely) whatever the user asked for.
///
/// `folded` overrides `pct` with the single title row a folded pane keeps.
pub fn split_inspector(
    area: Rect,
    pct: u16,
    show_inspector: bool,
    folded: bool,
) -> (Rect, Option<Rect>) {
    let min_height = if folded {
        crate::ui::FOLDED_HEIGHT
    } else {
        INSPECTOR_MIN_HEIGHT
    };
    // The graph view gives the whole column to the history: its blocks
    // already say what an inspector would.
    if !show_inspector || area.height < DETAIL_MIN_HEIGHT + min_height {
        return (area, None);
    }
    let height = if folded {
        crate::ui::FOLDED_HEIGHT
    } else {
        (area.height * pct / 100)
            .clamp(INSPECTOR_MIN_HEIGHT, INSPECTOR_MAX_HEIGHT)
            .min(area.height - DETAIL_MIN_HEIGHT)
    };
    let v = Layout::vertical([Constraint::Min(1), Constraint::Length(height)]).split(area);
    (v[0], Some(v[1]))
}

/// Split the main area into the repo list and the right column. Shared by
/// `render` and `ui::panel_rects` for the same reason as the vertical split.
pub fn split_main(area: Rect, repo_list_pct: u16) -> (Rect, Rect) {
    // A percentage of a very wide terminal is more room than a repo row can
    // use: the row is a name and a handful of badges, and everything past
    // about fifty columns is leader dots. The right column has the opposite
    // appetite -- commit subjects grow into whatever they are given -- so the
    // surplus goes there.
    //
    // Only the default yields. Once the divider has been dragged the number
    // is the user's answer to this question and is honoured exactly, however
    // wide the terminal.
    let width = repo_list_width(area.width, repo_list_pct);
    let h = Layout::horizontal([Constraint::Length(width), Constraint::Min(0)])
        .spacing(1)
        .split(area);
    (h[0], h[1])
}

/// The repo list width `split_main` draws in a main area `area_width`
/// columns wide, cap included. The resize keys step from it (FZ-69).
pub fn repo_list_width(area_width: u16, repo_list_pct: u16) -> u16 {
    if repo_list_pct == crate::app::DEFAULT_REPO_LIST_PCT {
        (area_width * repo_list_pct / 100).min(REPO_LIST_MAX_COLS)
    } else {
        area_width * repo_list_pct / 100
    }
}

/// Split pane 2's inner area of the `notes` layout, its full height, into
/// the tree's column and the column right of it (FZ-62, FZ-68), whose
/// first column is the divider (`notes_pane_body`). Shared by `render` and
/// `ui::panel_rects`, as the other splits are, so a click lands in the pane
/// drawn under it.
///
/// The tree takes the repo list's rule within pane 2 (35%, capped at
/// `REPO_LIST_MAX_COLS`): it is a list of names, and the reader is what
/// wants the width. Floored at `notes_tree_floor`, the width that holds the
/// mode line whole, or half the pane when that is less, so a small terminal
/// keeps a reader to read in.
pub fn split_notes_pane(pane: Rect) -> (Rect, Rect) {
    let rule = (pane.width * crate::app::DEFAULT_REPO_LIST_PCT / 100).min(REPO_LIST_MAX_COLS);
    let width = rule.max(notes_tree_floor().min(pane.width / 2));
    let h = Layout::horizontal([Constraint::Length(width), Constraint::Min(0)])
        .spacing(1)
        .split(pane);
    (h[0], h[1])
}

/// The narrowest the `notes` tree is drawn where pane 2 has the room: its
/// mode line in either mode, plus the pane's border and padding on each
/// side. Measured from the line itself, so new words move the floor with
/// them rather than cutting the `(m)` off.
fn notes_tree_floor() -> u16 {
    let line = [false, true]
        .map(|all| notes_mode_line_for(all).width())
        .into_iter()
        .max()
        .unwrap_or(0);
    u16::try_from(line).unwrap_or(u16::MAX) + 2 * crate::ui::PANE_CONTENT_INSET
}

/// Rows pane 2's body keeps under a pinned header, or the header is not
/// pinned. On a short pane the history is what you came for, and a header
/// taking half the height to repeat what the frame's own title already says
/// is the worse trade.
pub(super) const MIN_BODY_ROWS: u16 = 6;

/// Columns past which the repo list is only drawing longer dots: two borders,
/// two columns of padding, the marker and kind glyph, a generous name, the
/// dot fill, and the widest badge run a row can carry (`1PR 1 3↓ 2↑ 5!`).
pub(super) const REPO_LIST_MAX_COLS: u16 = 52;

/// Interior width from which the inspector shows the changed files in a lane
/// beside the message rather than under it. Half of this leaves each lane
/// about fifty columns, enough for a subject line or a stat plus a path.
pub const INSPECTOR_TWO_LANE_WIDTH: u16 = 100;

/// The rows the repo list keeps above pane 1's summary box before the box
/// gives way: a list that cannot show a handful of repos is worse than no
/// box, and the cursor's scroll margin needs the rows.
const MIN_LIST_ROWS_BESIDE_SUMMARY: u16 = 8;

/// Pane 1's inner area split into the filter strip and list above, and the
/// summary box at the foot: a divider row and `lines` rows. `None` for the
/// box where the list would be left too short; the footer then carries the
/// fleet's state. The list's clicks resolve against where it is drawn
/// (`ListHits`), so this split needs no copy in `panel_rects`.
pub(super) fn split_summary_box(inner: Rect, lines: usize) -> (Rect, Option<Rect>) {
    let box_h = u16::try_from(lines).unwrap_or(u16::MAX).saturating_add(1);
    // The filter strip and its divider sit above the list.
    if inner.height < 2 + MIN_LIST_ROWS_BESIDE_SUMMARY + box_h {
        return (inner, None);
    }
    let v = Layout::vertical([Constraint::Min(1), Constraint::Length(box_h)]).split(inner);
    (v[0], Some(v[1]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::detail::tests::list_repo;

    /// A percentage of a very wide terminal is more room than a repo row can
    /// use: past about fifty columns the list is only drawing longer dots.
    /// The right column has the opposite appetite, so the surplus goes there.
    #[test]
    fn the_repo_list_stops_growing_once_the_rows_have_what_they_need() {
        use ratatui::layout::Rect;
        let at = |w: u16| {
            let (list, right) =
                split_main(Rect::new(0, 0, w, 40), crate::app::DEFAULT_REPO_LIST_PCT);
            (list.width, right.width)
        };

        // Below the cap nothing changes: the percentage is still the answer.
        let (list, _) = at(120);
        assert_eq!(list, 120 * crate::app::DEFAULT_REPO_LIST_PCT / 100);

        let (narrow_list, narrow_right) = at(150);
        let (wide_list, wide_right) = at(300);
        assert_eq!(wide_list, REPO_LIST_MAX_COLS, "the list kept growing");
        assert!(
            wide_right - narrow_right >= 150,
            "the surplus did not reach the right column: {narrow_right} -> {wide_right}"
        );
        assert!(narrow_list <= REPO_LIST_MAX_COLS);
    }

    /// Only the default yields to the cap. Once the divider has been dragged
    /// the number is the user's answer to this question, and a cap that
    /// overrode it would make the drag look broken.
    #[test]
    fn a_dragged_divider_is_honoured_past_the_cap() {
        use ratatui::layout::Rect;
        let dragged = crate::app::DEFAULT_REPO_LIST_PCT + 5;
        let (list, _) = split_main(Rect::new(0, 0, 300, 40), dragged);
        assert_eq!(list.width, 300 * dragged / 100);
        assert!(list.width > REPO_LIST_MAX_COLS);
    }

    #[test]
    fn fresh_app_inspector_opens_at_its_default_with_a_row_floor() {
        use ratatui::layout::Rect;
        let rects = |h: u16| {
            let mut app = crate::app::App::new(vec![list_repo(3)], "h".to_string(), 120, h);
            app.view = crate::app::View::Detail;
            app.contributions_folded = true;
            assert_eq!(app.inspector_pct, 25);
            crate::ui::panel_rects(Rect::new(0, 0, 120, h), &app)
        };
        // Tall terminal: 25 percent of the body, above the floor.
        let r = rects(80);
        let tall = r
            .inspector
            .expect("tall terminal docks an inspector")
            .height;
        assert!(tall > INSPECTOR_MIN_HEIGHT, "not above the floor: {tall}");
        assert_eq!(tall, r.body.height * 25 / 100, "not 25 percent: {tall}");
        // Short terminal: 25 percent is under the floor, so the floor holds.
        let r = rects(22);
        assert!(r.body.height * 25 / 100 < INSPECTOR_MIN_HEIGHT);
        let short = r
            .inspector
            .expect("short terminal keeps an inspector")
            .height;
        assert_eq!(short, INSPECTOR_MIN_HEIGHT, "floor lost: {short}");
    }
}
