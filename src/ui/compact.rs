use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};

use crate::app::{App, FocusedPanel};
use crate::model::RepoStatus;
use crate::ui::badges;
use crate::ui::colors;

/// Render the compact view: a dense grid of all repos. The column count
/// follows the width and the widest name and badges (`column_count`), up to
/// `MAX_COLUMNS`.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let visible = app.visible_repos();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(colors::SURFACE))
        // Same horizontal padding as the detail view's repo list, so content
        // starts at the same column in both views and the footer below lines
        // up with either one.
        .padding(Padding::horizontal(1))
        // The contribution strip can take focus in compact too, so the title
        // follows it rather than always reading as active.
        .title_top(crate::ui::panel_title(
            1,
            "repos",
            app.focused_panel == FocusedPanel::RepoList,
        ))
        .title_top(
            Line::from(Span::styled(
                format!(" {} ", visible.len()),
                Style::default().fg(colors::OVERLAY),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Filter strip + divider, then content below.
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(inner);
    frame.render_widget(Paragraph::new(crate::ui::filter_strip_line(app)), rows[0]);
    frame.render_widget(
        Paragraph::new(crate::ui::divider_line(rows[1].width as usize)),
        rows[1],
    );
    let content = rows[2];

    if visible.is_empty() {
        let empty =
            Paragraph::new("No repos to display").style(Style::default().fg(colors::OVERLAY));
        frame.render_widget(empty, content);
        return;
    }

    let columns = column_count(&visible, content.width as usize);
    let col_width = cell_width(columns, content.width as usize);

    let selected = app.selected_repo_index().unwrap_or(usize::MAX);
    let mut lines: Vec<Line> = Vec::new();

    for row in visible.chunks(columns) {
        let mut row_spans: Vec<Span<'static>> = Vec::new();
        for (i, entry) in row.iter().enumerate() {
            if i > 0 {
                let prev_len: usize = row_spans.iter().map(|s| s.content.chars().count()).sum();
                let pad = col_width.saturating_sub(prev_len) + GUTTER;
                row_spans.push(Span::raw(" ".repeat(pad)));
            }
            row_spans.extend(compact_repo_spans_marked(
                entry.1,
                entry.0 == selected,
                col_width,
                app.watch_mode && !app.blink_on,
                app.is_not_in_last_poll(entry.1),
            ));
        }
        lines.push(Line::from(row_spans));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, content);
}

/// Blank columns between two grid cells.
const GUTTER: usize = 2;

/// Dots kept between the longest name and the badges, so the fill still reads
/// as a fill rather than as two tokens jammed together.
const MIN_DOTS: usize = 3;

/// Name columns a cell plans for. Longer names truncate, which is cheaper than
/// letting one `marshall-code-test` set the width of every column in the grid
/// -- the same tax the history pane's graph column had to be capped against.
const NAME_TARGET: usize = 20;

/// More columns than this and the eye stops reading a grid and starts reading
/// noise, however much room the terminal has.
const MAX_COLUMNS: usize = 6;

/// How many grid columns `width` can carry, given what the visible repos
/// actually need.
///
/// Was a constant 2, at any width from 48 columns to 300. That is the whole
/// bug: a wider terminal bought longer leader dots instead of more repos on
/// screen, in the one view whose entire job is showing the fleet at once.
fn column_count(visible: &[(usize, &RepoStatus)], width: usize) -> usize {
    if visible.is_empty() || width == 0 {
        return 1;
    }
    let widest_name = visible
        .iter()
        .map(|(_, r)| r.name.chars().count())
        .max()
        .unwrap_or(0)
        .min(NAME_TARGET);
    let widest_badges = visible
        .iter()
        .map(|(_, r)| badges::badge_spans(r, false).1)
        .max()
        .unwrap_or(0);
    // Mirrors what `compact_repo_spans` lays out: marker, glyph, name, the
    // space-dots-space fill, then the badges.
    let cell = badges::MARKER_W + badges::GLYPH_W + widest_name + 1 + MIN_DOTS + 1 + widest_badges;
    ((width + GUTTER) / (cell + GUTTER)).clamp(1, MAX_COLUMNS)
}

/// The width one cell gets, with the gutters taken out first so the row lands
/// inside `width` rather than one column past it for every column added.
fn cell_width(columns: usize, width: usize) -> usize {
    width.saturating_sub(GUTTER * (columns - 1)) / columns
}

#[cfg(test)]
fn compact_repo_spans(
    repo: &RepoStatus,
    is_selected: bool,
    max_width: usize,
    dim_pending: bool,
) -> Vec<Span<'static>> {
    compact_repo_spans_marked(repo, is_selected, max_width, dim_pending, false)
}

/// One grid cell. `not_in_last_poll` draws the CI lamp and PR count as from
/// an earlier refresh, as the detail view's list does
/// (`badges::badge_spans_marked`), so the mark looks the same in every view.
fn compact_repo_spans_marked(
    repo: &RepoStatus,
    is_selected: bool,
    max_width: usize,
    dim_pending: bool,
    not_in_last_poll: bool,
) -> Vec<Span<'static>> {
    let marker = if is_selected { "▸ " } else { "  " };
    let name_style = if is_selected {
        Style::default()
            .fg(colors::SAPPHIRE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors::SAPPHIRE)
    };

    // Tight, right-aligned badges flush to each grid cell's right edge, dots
    // filling from the name to them (see ui::badges::badge_spans). Reserve
    // room for marker, glyph, at least one dot, and the badges; whatever
    // remains is the name's budget -- truncate rather than overflow into the
    // next column.
    let (badge_spans, badge_w) = badges::badge_spans_marked(repo, dim_pending, not_in_last_poll);
    let right_len = badge_w + 1; // +1 for space after dots
    let reserved = badges::MARKER_W + badges::GLYPH_W + 1 + right_len;
    let name_budget = max_width.saturating_sub(reserved).max(3);
    let name = crate::ui::truncate(&repo.name, name_budget);

    let left_len = badges::MARKER_W + badges::GLYPH_W + name.chars().count() + 1;
    let dot_count = max_width.saturating_sub(left_len + right_len).max(1);
    let dots = "·".repeat(dot_count);

    let mut spans = vec![
        Span::raw(marker.to_string()),
        badges::glyph_span(repo),
        Span::styled(name, name_style),
        Span::styled(format!(" {dots} "), Style::default().fg(colors::SURFACE)),
    ];
    spans.extend(badge_spans);
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CiStatus, RepoKind, RepoLocation};

    fn repo(name: &str) -> RepoStatus {
        RepoStatus {
            daily: None,
            daily_key: None,
            last_local: None,
            name: name.to_string(),
            path: Some("~/repos/x".to_string()),
            location: RepoLocation::Primary,
            branches: vec![],
            commits: vec![],
            behind: 3,
            ahead: 2,
            dirty: 5,
            pr_count: 1,
            prs: vec![],
            ci_status: CiStatus::Unknown,
            ci_workflow: None,
            ci_duration: None,
            groups: vec![],
            stash_count: 1,
            stash_entries: vec![],
            worktrees: vec![],
            reflog: vec![],
            graph: vec![],
            graph_expanded: Vec::new(),
            repo_kind: RepoKind::Public,
            owner: Some("Gaurgle".to_string()),
            remote_url: None,
            remote_only_branches: vec![],
            remote_branches: vec![],
            work_key: None,
            fetch: crate::model::FetchFacts::default(),
            authors: Default::default(),
        }
    }

    fn fleet(n: usize) -> Vec<RepoStatus> {
        (0..n).map(|i| repo(&format!("repo-{i:02}"))).collect()
    }

    /// The bug this view had: the column count was the constant 2 at every
    /// width from 48 columns to 300, so a wider terminal bought longer leader
    /// dots rather than more repos on screen -- in the one view whose whole
    /// job is showing the fleet at once.
    #[test]
    fn a_wider_pane_shows_more_columns() {
        let repos = fleet(30);
        let visible: Vec<(usize, &RepoStatus)> = repos.iter().enumerate().collect();

        let narrow = column_count(&visible, 58);
        let normal = column_count(&visible, 98);
        let wide = column_count(&visible, 196);
        assert!(
            narrow < normal && normal < wide,
            "width buys nothing: 58 -> {narrow}, 98 -> {normal}, 196 -> {wide}"
        );
        assert!(wide >= 4, "a 196-column pane fits four cells, got {wide}");
    }

    #[test]
    fn the_column_count_stays_within_its_bounds() {
        let repos = fleet(4);
        let visible: Vec<(usize, &RepoStatus)> = repos.iter().enumerate().collect();
        assert_eq!(column_count(&visible, 1), 1, "never zero columns");
        assert_eq!(
            column_count(&visible, 10_000),
            MAX_COLUMNS,
            "a grid stops being readable long before it stops fitting"
        );
        assert_eq!(column_count(&[], 200), 1, "an empty fleet still lays out");
    }

    /// Names widen the cell, but only up to `NAME_TARGET`. Past that a single
    /// outlier would be setting the width of every column in the grid, which
    /// is the same tax the history pane's graph column had to be capped
    /// against; it truncates instead.
    #[test]
    fn a_name_past_the_target_stops_widening_the_grid() {
        let at_target = {
            let mut f = fleet(8);
            f.push(repo(&"n".repeat(NAME_TARGET)));
            f
        };
        let far_past = {
            let mut f = fleet(8);
            f.push(repo(&"n".repeat(NAME_TARGET * 4)));
            f
        };

        let a: Vec<(usize, &RepoStatus)> = at_target.iter().enumerate().collect();
        let b: Vec<(usize, &RepoStatus)> = far_past.iter().enumerate().collect();
        assert_eq!(
            column_count(&a, 196),
            column_count(&b, 196),
            "a name four times the target changed the layout"
        );

        // And it does widen up to the target, which is the other half of the
        // rule: a fleet of short names should pack more columns than one of
        // target-length ones.
        let short_repos = fleet(8);
        let short: Vec<(usize, &RepoStatus)> = short_repos.iter().enumerate().collect();
        assert!(
            column_count(&short, 196) > column_count(&a, 196),
            "short names should pack tighter than target-length ones"
        );
    }

    /// The old cell arithmetic was `width / columns - 1`, which happens to
    /// land exactly on `width` at two columns and one column past it for every
    /// column after that. Adding columns would have overrun the pane.
    #[test]
    fn a_full_row_of_cells_fits_inside_the_pane() {
        for width in [40usize, 58, 98, 140, 196, 240] {
            let columns =
                column_count(&fleet(30).iter().enumerate().collect::<Vec<_>>()[..], width);
            let cell = cell_width(columns, width);
            let row = columns * cell + GUTTER * (columns - 1);
            assert!(
                row <= width,
                "{columns} cells of {cell} plus gutters is {row}, past a {width}-column pane"
            );
        }
    }

    /// Every cell is exactly the width the grid allotted it, which is what
    /// lets the row arithmetic above hold and the columns line up.
    #[test]
    fn a_cell_fills_its_column_exactly() {
        let r = repo("fleetz");
        for width in [30usize, 44, 60] {
            let spans = compact_repo_spans(&r, false, width, false);
            let drawn: usize = spans.iter().map(|s| s.content.chars().count()).sum();
            assert_eq!(drawn, width, "a {width}-column cell drew {drawn}");
        }
    }

    /// The color the `repos` label of the list title is drawn in.
    fn list_title_color(focused_panel: crate::app::FocusedPanel) -> Option<ratatui::style::Color> {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let width = 60;
        let mut app = App::new(fleet(3), "t".to_string(), width, 20);
        app.view = crate::app::View::Compact;
        app.focused_panel = focused_panel;
        let mut term = Terminal::new(TestBackend::new(width, 20)).unwrap();
        term.draw(|f| render(f, f.area(), &app)).unwrap();
        let buf = term.backend().buffer().clone();
        let row: String = (0..width).map(|x| buf[(x, 0)].symbol()).collect();
        let x = row.find("repos").expect("list title not drawn") as u16;
        buf[(x, 0)].style().fg
    }

    /// The compact list title was hardcoded focused, so with the strip
    /// focused both panes claimed the keys.
    #[test]
    fn the_list_title_reads_focused_only_while_the_list_has_focus() {
        use crate::app::FocusedPanel;
        assert_eq!(
            list_title_color(FocusedPanel::RepoList),
            Some(colors::LAVENDER)
        );
        assert_eq!(
            list_title_color(FocusedPanel::Contributions),
            Some(colors::OVERLAY),
            "the list title drawn focused while the strip has focus"
        );
    }

    /// FZ-09d: a repo the last poll did not return draws its lamp in the
    /// grey of an earlier refresh, steady through the blink, as the detail
    /// view's list does; a repo the poll returned draws as before.
    #[test]
    fn a_compact_cell_not_in_the_last_poll_is_dimmed_and_its_lamp_holds_still() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut repos = vec![repo("stale"), repo("fresh")];
        for r in &mut repos {
            r.ci_status = CiStatus::Pending;
        }
        let width = 60u16;
        let mut app = App::new(repos, "t".to_string(), width, 20);
        app.view = crate::app::View::Compact;
        app.watch_mode = true;
        app.not_in_last_poll
            .insert((Some("gaurgle".to_string()), "stale".to_string()));

        let lamp = |app: &App, name: &str| {
            let mut term = Terminal::new(TestBackend::new(width, 20)).unwrap();
            term.draw(|f| render(f, f.area(), app)).unwrap();
            let buf = term.backend().buffer().clone();
            (0..20u16)
                .find_map(|y| {
                    let row: String = (0..width).map(|x| buf[(x, y)].symbol()).collect();
                    let start = row.find(&format!(" {name} "))?;
                    let start = row[..start].chars().count() as u16;
                    (start..width)
                        .find(|&x| buf[(x, y)].symbol() == "◉")
                        .map(|x| buf[(x, y)].fg)
                })
                .expect("lamp drawn")
        };
        for blink_on in [true, false] {
            app.blink_on = blink_on;
            assert_eq!(
                lamp(&app, "stale"),
                badges::EARLIER_REFRESH,
                "blink_on {blink_on}"
            );
        }
        app.blink_on = true;
        assert_eq!(lamp(&app, "fresh"), colors::PEACH);
        app.blink_on = false;
        assert_eq!(lamp(&app, "fresh"), colors::SURFACE);
    }
}
