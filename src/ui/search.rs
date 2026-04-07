use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::App;
use crate::ui::colors;

/// Render the search input bar (shown when search is active).
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let spans = vec![
        Span::styled(
            " / ",
            Style::default()
                .fg(colors::LAVENDER)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            app.search_query.clone(),
            Style::default().fg(colors::TEXT),
        ),
        Span::styled("█", Style::default().fg(colors::LAVENDER)),
    ];

    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}
