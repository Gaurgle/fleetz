use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::{Applies, SETTING_FIELDS, SettingsOverlay};
use crate::ui::colors;

/// Column the value box starts at, so every row's value lines up regardless of
/// label length.
const VALUE_COL: usize = 22;

/// Render the settings overlay centered on screen.
pub fn render(frame: &mut Frame, area: Rect, overlay: &SettingsOverlay) {
    // Two frame rows, one row per field, a blank, two hint rows, and a row
    // reserved for the save result so the popup never changes height.
    let height = SETTING_FIELDS.len() as u16 + 7;
    let popup_area = centered_fixed(58, height, area);
    frame.render_widget(Clear, popup_area);

    let focused = Style::default()
        .fg(colors::LAVENDER)
        .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    let idle = Style::default().fg(colors::TEXT);

    let mut lines = vec![Line::from("")];

    for (i, field) in SETTING_FIELDS.iter().enumerate() {
        let is_focused = i == overlay.cursor;
        let label = field.label();
        let pad = VALUE_COL.saturating_sub(label.chars().count() + 3);

        let mut spans = vec![
            Span::styled(
                format!("   {label}"),
                Style::default().fg(if is_focused {
                    colors::LAVENDER
                } else {
                    colors::OVERLAY
                }),
            ),
            Span::raw(" ".repeat(pad + 1)),
            Span::styled(
                format!("[ {:^9} ]", overlay.value_label(*field)),
                if is_focused { focused } else { idle },
            ),
        ];
        // Restart-only fields say so inline: a field that looks inert after
        // Enter is otherwise indistinguishable from a broken one.
        if field.applies() == Applies::Restart {
            spans.push(Span::styled(
                "  restart",
                Style::default().fg(colors::SUBTEXT),
            ));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "   j/k move · h/l adjust · 0-9 type",
        Style::default().fg(colors::SUBTEXT),
    )));
    lines.push(Line::from(Span::styled(
        "   Enter save to config.toml · Esc cancel",
        Style::default().fg(colors::SUBTEXT),
    )));
    lines.push(Line::from(match &overlay.status {
        Some(message) => Span::styled(
            format!("   {message}"),
            Style::default().fg(if message.starts_with("saved") {
                colors::GREEN
            } else {
                colors::RED
            }),
        ),
        None => Span::raw(""),
    }));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::LAVENDER))
        .title(Span::styled(
            " Settings ",
            Style::default()
                .fg(colors::LAVENDER)
                .add_modifier(Modifier::BOLD),
        ))
        // Flush right in the frame: the settings menu is where you look when
        // you are already asking "what is this configured as", so it is the
        // natural second home for "and which build is it".
        .title_top(
            Line::from(Span::styled(
                format!(" fleetz {} ", crate::ui::VERSION),
                Style::default().fg(colors::SUBTEXT),
            ))
            .right_aligned(),
        );

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
