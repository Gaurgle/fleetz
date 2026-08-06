use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::{TimeWindowOverlay, WindowField};
use crate::time::WindowUnit;
use crate::ui::colors;

/// Render the time-window spinner overlay centered on screen.
pub fn render(frame: &mut Frame, area: Rect, overlay: &TimeWindowOverlay) {
    let popup_area = centered_fixed(48, 7, area);
    frame.render_widget(Clear, popup_area);

    let slot = overlay.unit == WindowUnit::Slot;

    let focused = Style::default()
        .fg(colors::LAVENDER)
        .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    let idle = Style::default().fg(colors::TEXT);
    // The count is meaningless for Slot, so it renders dimmed and
    // unfocusable rather than disappearing (layout stays stable).
    let number_style = if slot {
        Style::default().fg(colors::SURFACE)
    } else if overlay.field == WindowField::Number {
        focused
    } else {
        idle
    };
    let unit_style = if overlay.field == WindowField::Unit {
        focused
    } else {
        idle
    };

    let unit_label = match overlay.unit {
        WindowUnit::Slot => "last slot",
        WindowUnit::Hours => "hours",
        WindowUnit::Days => "days",
        WindowUnit::Weeks => "weeks",
        WindowUnit::Months => "months",
        WindowUnit::Years => "years",
    };
    let number_label = if slot {
        " -".to_string()
    } else {
        format!("{:2}", overlay.n)
    };

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("   since:  ", Style::default().fg(colors::OVERLAY)),
            Span::styled(format!("[ {number_label} ]"), number_style),
            Span::raw("  "),
            Span::styled(format!("[ {unit_label} ]"), unit_style),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "   h/l switch field · j/k adjust · 0-9 type",
            Style::default().fg(colors::SUBTEXT),
        )),
        Line::from(Span::styled(
            "   Enter apply · Esc cancel",
            Style::default().fg(colors::SUBTEXT),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::LAVENDER))
        .title(Span::styled(
            " Time window ",
            Style::default()
                .fg(colors::LAVENDER)
                .add_modifier(Modifier::BOLD),
        ));

    frame.render_widget(Paragraph::new(lines).block(block), popup_area);
}

/// Return a centered Rect of fixed `width` x `height` within `area`.
fn centered_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .split(area);
    Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .split(vertical[0])[0]
}
