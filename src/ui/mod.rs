pub mod branches;
pub mod colors;
pub mod compact;
pub mod detail;
pub mod footer;
pub mod help;
pub mod search;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{App, Overlay, View};

/// Top-level render: header + main view + optional search + footer + optional overlay.
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Determine layout chunks
    let search_height = if app.search_active { 1 } else { 0 };
    let chunks = Layout::vertical([
        Constraint::Length(2),                     // header
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

    let mut spans = vec![
        Span::styled(
            " epoz",
            Style::default()
                .fg(colors::LAVENDER)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" — {}", app.header),
            Style::default().fg(colors::OVERLAY),
        ),
    ];

    if let Some(group) = app.active_group_label() {
        spans.push(Span::styled(
            format!("  [{group}]"),
            Style::default().fg(colors::MAUVE),
        ));
    }

    if let Some(ref ts) = app.last_refreshed {
        spans.push(Span::styled(
            format!("  (refreshed {})", ts.format("%H:%M")),
            Style::default().fg(colors::SURFACE),
        ));
    }

    spans.extend([
        Span::raw("  "),
        Span::styled(view_hint.to_string(), Style::default().fg(colors::SURFACE)),
        Span::raw("  "),
        Span::styled("[?] help".to_string(), Style::default().fg(colors::SURFACE)),
    ]);

    let line = Line::from(spans);

    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}
