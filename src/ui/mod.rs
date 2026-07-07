pub mod branches;
pub mod colors;
pub mod compact;
pub mod detail;
pub mod footer;
pub mod help;
pub mod search;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::app::{App, FilterKind, Overlay, View};

/// Quick filters in strip order, with the digit key each is bound to (index + 1)
/// and the color it lights up in -- matching the color that filter's data
/// already uses elsewhere (e.g. dirty badges are YELLOW, so is this dot).
const FILTER_DEFS: [(FilterKind, &str, Color); 6] = [
    (FilterKind::Dirty, "dirty", colors::YELLOW),
    (FilterKind::Behind, "behind", colors::PEACH),
    (FilterKind::Ahead, "ahead", colors::SAPPHIRE),
    (FilterKind::CiFailed, "CI\u{2717}", colors::RED),
    (FilterKind::HasPrs, "PRs", colors::MAUVE),
    (FilterKind::HasStashes, "stash", colors::HASH_CLR),
];

/// The always-visible quick-filter strip: one dot per filter, lit in its own
/// color when active and dimmed (same hue) otherwise -- so the row of dots
/// alone shows what's applied, no separate legend needed.
pub fn filter_strip_line(app: &App) -> Line<'static> {
    let mut spans = vec![Span::raw("  ")];
    for (i, (kind, label, color)) in FILTER_DEFS.iter().enumerate() {
        let active = app.filters.is_active(*kind);
        let style = if active {
            Style::default().fg(*color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(colors::dim(*color))
        };
        let dot = if active { "●" } else { "·" };
        spans.push(Span::styled(format!("{dot}{} {label}", i + 1), style));
        spans.push(Span::raw("  "));
    }
    Line::from(spans)
}

/// A thin divider rule, the width of `width`, separating the filter strip
/// from the panel content below it.
pub fn divider_line(width: usize) -> Line<'static> {
    Line::from(Span::styled(
        "─".repeat(width),
        Style::default().fg(colors::SURFACE),
    ))
}

/// Top-level render: header + main view + optional search + footer + optional overlay.
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Determine layout chunks
    let search_height = if app.search_active { 1 } else { 0 };
    let chunks = Layout::vertical([
        Constraint::Length(1),                     // header (a single divider line, tags embedded)
        Constraint::Min(1),                        // main content
        Constraint::Length(search_height),          // search bar (if active)
        Constraint::Length(2),                      // footer
    ])
    .split(area);

    // Header
    render_header(frame, chunks[0], app);

    // Main content area
    match app.view {
        View::Detail => detail::render(frame, chunks[1], app),
        View::Compact => compact::render(frame, chunks[1], app),
    }

    // Search bar
    if app.search_active {
        search::render(frame, chunks[2], app);
    }

    // Footer
    footer::render(frame, chunks[3], app);

    // Overlay (rendered last, on top)
    match &app.overlay {
        Overlay::Help => help::render(frame, area),
        Overlay::Branches(bo) => branches::render(frame, area, bo),
        Overlay::None => {}
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let view_hint = match app.view {
        View::Detail => "[v] compact",
        View::Compact => "[v] detail",
    };

    let mut left = vec![Span::styled(
        " fleetz",
        Style::default()
            .fg(colors::LAVENDER)
            .add_modifier(Modifier::BOLD),
    )];
    if !app.header.is_empty() {
        left.push(Span::styled(
            format!(" — {}", app.header),
            Style::default().fg(colors::OVERLAY),
        ));
    }
    if let Some(group) = app.active_group_label() {
        left.push(Span::styled(
            format!("  [{group}]"),
            Style::default().fg(colors::MAUVE),
        ));
    }
    let mut right: Vec<Span> = Vec::new();
    if let Some(ref ts) = app.last_refreshed {
        right.push(Span::styled(
            format!(" refreshed {}  ", ts.format("%H:%M")),
            Style::default().fg(colors::SURFACE),
        ));
    } else {
        right.push(Span::raw(" "));
    }
    right.push(Span::styled(view_hint, Style::default().fg(colors::SURFACE)));
    right.push(Span::raw("  "));
    right.push(Span::styled("[?] help ", Style::default().fg(colors::SURFACE)));

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(colors::SURFACE))
        .title_bottom(Line::from(left).left_aligned())
        .title_bottom(Line::from(right).right_aligned());

    frame.render_widget(block, area);
}
