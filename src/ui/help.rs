use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::colors;

/// Render the help overlay centered on screen.
pub fn render(frame: &mut Frame, area: Rect) {
    let popup_area = centered_rect(60, 70, area);

    // Clear the area behind the popup
    frame.render_widget(Clear, popup_area);

    let keybinds = [
        ("j/k ↑/↓", "Move selection"),
        ("v", "Toggle detail ↔ compact view"),
        ("enter", "Open repo in lazygit"),
        ("e", "Open repo in $EDITOR"),
        ("o", "Open shell in repo dir"),
        ("O", "Open PR in browser"),
        ("p", "Pull selected repo (ff-only)"),
        ("P", "Pull all clean repos (ff-only)"),
        ("b", "Branch cleanup"),
        ("g", "Cycle group filter"),
        ("G", "Show all (clear group)"),
        ("r", "Refresh data"),
        ("/", "Search/filter repos"),
        ("Esc", "Close search / help"),
        ("?", "Toggle this help"),
        ("q", "Quit"),
    ];

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    for (key, desc) in keybinds {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {key:>10}"),
                Style::default()
                    .fg(colors::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("   {desc}"), Style::default().fg(colors::OVERLAY)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Press ? or Esc to close",
        Style::default().fg(colors::SURFACE),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::LAVENDER))
        .title(Span::styled(
            " Keybindings ",
            Style::default()
                .fg(colors::LAVENDER)
                .add_modifier(Modifier::BOLD),
        ));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, popup_area);
}

/// Return a centered Rect of `percent_x` x `percent_y` within `area`.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Percentage(percent_y)])
        .flex(Flex::Center)
        .split(area);
    Layout::horizontal([Constraint::Percentage(percent_x)])
        .flex(Flex::Center)
        .split(vertical[0])[0]
}
