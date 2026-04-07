use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::App;
use crate::model::RepoStatus;
use crate::ui::colors;

/// Render the compact view: two-column dense grid of all repos.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let visible = app.visible_repos();
    if visible.is_empty() {
        let empty = Paragraph::new("No repos to display")
            .style(Style::default().fg(colors::OVERLAY));
        frame.render_widget(empty, area);
        return;
    }

    let col_width = (area.width as usize / 2).saturating_sub(1);
    let mut lines: Vec<Line> = Vec::new();

    // Pair repos into rows of two
    let pairs: Vec<_> = visible.chunks(2).collect();
    for pair in pairs {
        let left = &pair[0];
        let right = pair.get(1);

        let is_left_selected = left.0 == app.selected_repo_index().unwrap_or(usize::MAX);
        let left_spans = compact_repo_spans(left.1, is_left_selected, col_width);

        let mut row_spans = left_spans;

        if let Some(right_entry) = right {
            let is_right_selected =
                right_entry.0 == app.selected_repo_index().unwrap_or(usize::MAX);
            // Pad left column to col_width
            let left_text_len: usize = row_spans.iter().map(|s| s.content.len()).sum();
            let pad = col_width.saturating_sub(left_text_len) + 2;
            row_spans.push(Span::raw(" ".repeat(pad)));
            row_spans.extend(compact_repo_spans(right_entry.1, is_right_selected, col_width));
        }

        lines.push(Line::from(row_spans));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

fn compact_repo_spans(repo: &RepoStatus, is_selected: bool, max_width: usize) -> Vec<Span<'static>> {
    let marker = if is_selected { "▸ " } else { "  " };
    let name_style = if is_selected {
        Style::default()
            .fg(colors::SAPPHIRE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors::SAPPHIRE)
    };

    let status = repo.short_status();
    let status_color = if repo.behind > 0 {
        colors::PEACH
    } else if repo.dirty > 0 {
        colors::YELLOW
    } else if repo.ahead > 0 {
        colors::PEACH
    } else {
        colors::GREEN
    };

    // Build dots between name and status
    let content_len = marker.len() + repo.name.len() + 1 + status.len();
    let dot_count = max_width
        .saturating_sub(content_len)
        .saturating_sub(repo.pr_count.to_string().len() + if repo.pr_count > 0 { 3 } else { 0 })
        .max(1);
    let dots = "·".repeat(dot_count);

    let mut spans = vec![
        Span::raw(marker.to_string()),
        Span::styled(repo.name.clone(), name_style),
        Span::styled(format!(" {dots} "), Style::default().fg(colors::SURFACE)),
        Span::styled(status, Style::default().fg(status_color)),
    ];

    if repo.pr_count > 0 {
        spans.push(Span::styled(
            format!(" {}PR", repo.pr_count),
            Style::default().fg(colors::MAUVE),
        ));
    }

    spans
}
