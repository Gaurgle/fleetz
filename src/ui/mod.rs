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
/// alone shows what's applied. The label only appears once a filter is
/// active, since an idle dot+number is a keybind reminder (see help), not
/// something that needs spelling out -- keeps the strip compact by default.
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
        let text = if active {
            format!("{dot}{} {label}", i + 1)
        } else {
            format!("{dot}{}", i + 1)
        };
        spans.push(Span::styled(text, style));
        spans.push(Span::raw("  "));
    }
    Line::from(spans)
}

/// Truncate `s` to at most `budget` chars, replacing the tail with `…` when
/// it doesn't fit, rather than letting it silently overflow into whatever
/// renders next to it.
pub fn truncate(s: &str, budget: usize) -> String {
    if s.chars().count() <= budget {
        return s.to_string();
    }
    if budget == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(budget.saturating_sub(1)).collect();
    out.push('…');
    out
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

/// Render the header. The left (name/header/group) and right (refresh time/
/// view hint/help) titles are positioned independently by ratatui with no
/// mutual awareness, so on a narrow pane they can literally overlap
/// character-for-character. To avoid that, progressively drop the least
/// important piece from each side -- refresh timestamp first, then the view
/// hint, then the group tag, then the header description -- until both sides
/// fit side by side.
fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let width = area.width as usize;

    let name = Span::styled(
        " fleetz",
        Style::default()
            .fg(colors::LAVENDER)
            .add_modifier(Modifier::BOLD),
    );
    let header_span = (!app.header.is_empty()).then(|| {
        Span::styled(
            format!(" — {}", app.header),
            Style::default().fg(colors::OVERLAY),
        )
    });
    let group_span = app.active_group_label().map(|group| {
        Span::styled(format!("  [{group}]"), Style::default().fg(colors::MAUVE))
    });

    let view_hint = match app.view {
        View::Detail => "[v] compact",
        View::Compact => "[v] detail",
    };
    let ts_span = app.last_refreshed.map(|ts| {
        Span::styled(
            format!(" refreshed {}  ", ts.format("%H:%M")),
            Style::default().fg(colors::SURFACE),
        )
    });
    let view_span = Span::styled(view_hint, Style::default().fg(colors::SURFACE));
    let help_span = Span::styled("[?] help ", Style::default().fg(colors::SURFACE));

    let mut include_ts = true;
    let mut include_view = true;
    let mut include_group = true;
    let mut include_header = true;

    loop {
        let mut left: Vec<Span> = vec![name.clone()];
        if include_header {
            if let Some(ref h) = header_span {
                left.push(h.clone());
            }
        }
        if include_group {
            if let Some(ref g) = group_span {
                left.push(g.clone());
            }
        }

        let mut right: Vec<Span> = Vec::new();
        if include_ts {
            if let Some(ref t) = ts_span {
                right.push(t.clone());
            } else {
                right.push(Span::raw(" "));
            }
        }
        if include_view {
            right.push(view_span.clone());
            right.push(Span::raw("  "));
        }
        right.push(help_span.clone());

        let left_w: usize = left.iter().map(|s| s.content.len()).sum();
        let right_w: usize = right.iter().map(|s| s.content.len()).sum();
        let exhausted = !include_ts && !include_view && !include_group && !include_header;

        if left_w + right_w < width || exhausted {
            let block = Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(colors::SURFACE))
                .title_bottom(Line::from(left).left_aligned())
                .title_bottom(Line::from(right).right_aligned());
            frame.render_widget(block, area);
            return;
        }

        if include_ts {
            include_ts = false;
        } else if include_view {
            include_view = false;
        } else if include_group {
            include_group = false;
        } else {
            include_header = false;
        }
    }
}
