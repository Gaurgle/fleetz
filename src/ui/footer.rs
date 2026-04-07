use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, View};
use crate::model::Summary;
use crate::ui::colors;

/// Render the footer: summary stats on the left, keybind hints on the right.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let summary = app.summary();

    // Build summary spans
    let mut spans = summary_spans(&summary, app.show_prs);

    // Add keybind hints on the right
    let hints = keybind_hints(app.view);
    let spacer_width = area.width as usize - line_width(&spans) - line_width(&hints);
    let spacer = if spacer_width > 2 {
        " ".repeat(spacer_width)
    } else {
        "  ".to_string()
    };
    spans.push(Span::raw(spacer));
    spans.extend(hints);

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(colors::SURFACE)),
    );
    frame.render_widget(paragraph, area);
}

fn summary_spans(summary: &Summary, show_prs: bool) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let dot = || Span::styled(" · ", Style::default().fg(colors::SURFACE));

    if summary.behind_count > 0 {
        spans.push(Span::styled(
            format!("{}↓", summary.behind_count),
            Style::default().fg(colors::PEACH),
        ));
        spans.push(dot());
    }
    if summary.ahead_count > 0 {
        spans.push(Span::styled(
            format!("{}↑", summary.ahead_count),
            Style::default().fg(colors::PEACH),
        ));
        spans.push(dot());
    }
    if summary.dirty_count > 0 {
        spans.push(Span::styled(
            format!("{}!", summary.dirty_count),
            Style::default().fg(colors::YELLOW),
        ));
        spans.push(dot());
    }
    if summary.synced_count > 0 {
        spans.push(Span::styled(
            format!("{} synced", summary.synced_count),
            Style::default().fg(colors::GREEN),
        ));
        spans.push(dot());
    }
    if summary.not_local_count > 0 {
        spans.push(Span::styled(
            format!("{} not local", summary.not_local_count),
            Style::default().fg(colors::OVERLAY),
        ));
        spans.push(dot());
    }
    if show_prs && summary.pr_count > 0 {
        spans.push(Span::styled(
            format!("{} PRs", summary.pr_count),
            Style::default().fg(colors::MAUVE),
        ));
        spans.push(dot());
    }

    // Remove trailing dot
    if spans.last().map(|s| s.content.contains('·')).unwrap_or(false) {
        spans.pop();
    }

    spans
}

fn keybind_hints(view: View) -> Vec<Span<'static>> {
    let key_style = Style::default()
        .fg(colors::TEXT)
        .add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(colors::OVERLAY);
    let gap = Span::raw("  ");

    let view_hint = match view {
        View::Detail => "compact",
        View::Compact => "detail",
    };

    vec![
        Span::styled("[enter]", key_style),
        Span::styled(" lazygit", desc_style),
        gap.clone(),
        Span::styled("[p]", key_style),
        Span::styled(" pull", desc_style),
        gap.clone(),
        Span::styled("[v]", key_style),
        Span::styled(format!(" {view_hint}"), desc_style),
        gap.clone(),
        Span::styled("[q]", key_style),
        Span::styled(" quit", desc_style),
    ]
}

fn line_width(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.len()).sum()
}
