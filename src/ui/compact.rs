use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::model::{CiStatus, RepoStatus};
use crate::ui::colors;

/// Render the compact view: two-column dense grid of all repos.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let visible = app.visible_repos();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(colors::SURFACE))
        .title_top(
            Line::from(Span::styled(
                " repos ",
                Style::default()
                    .fg(colors::LAVENDER)
                    .add_modifier(Modifier::BOLD),
            ))
            .left_aligned(),
        )
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
        let empty = Paragraph::new("No repos to display")
            .style(Style::default().fg(colors::OVERLAY));
        frame.render_widget(empty, content);
        return;
    }

    let col_width = (content.width as usize / 2).saturating_sub(1);
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
    frame.render_widget(paragraph, content);
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

    // Build dots between name and right-aligned badges
    let ci_sym = repo.ci_status.symbol();
    let mut badge_len = status.len();
    if !ci_sym.is_empty() {
        badge_len += 2;
    }
    if repo.pr_count > 0 {
        badge_len += format!(" {}PR", repo.pr_count).len();
    }
    if repo.stash_count > 0 {
        badge_len += format!(" {}s", repo.stash_count).len();
    }
    if !repo.worktrees.is_empty() {
        badge_len += format!(" {}w", repo.worktrees.len()).len();
    }

    let left_len = marker.len() + repo.name.len() + 1;
    let right_len = badge_len + 1;
    let dot_count = max_width.saturating_sub(left_len + right_len).max(1);
    let dots = "·".repeat(dot_count);

    let mut spans = vec![
        Span::raw(marker.to_string()),
        Span::styled(repo.name.clone(), name_style),
        Span::styled(format!(" {dots} "), Style::default().fg(colors::SURFACE)),
        Span::styled(status, Style::default().fg(status_color)),
    ];

    // CI badge
    if !ci_sym.is_empty() {
        let ci_color = match repo.ci_status {
            CiStatus::Passed => colors::GREEN,
            CiStatus::Failed => colors::RED,
            CiStatus::Pending => colors::YELLOW,
            CiStatus::Unknown => colors::OVERLAY,
        };
        spans.push(Span::styled(
            format!(" {ci_sym}"),
            Style::default().fg(ci_color),
        ));
    }

    if repo.pr_count > 0 {
        spans.push(Span::styled(
            format!(" {}PR", repo.pr_count),
            Style::default().fg(colors::MAUVE),
        ));
    }

    if repo.stash_count > 0 {
        spans.push(Span::styled(
            format!(" {}s", repo.stash_count),
            Style::default().fg(colors::OVERLAY),
        ));
    }

    if !repo.worktrees.is_empty() {
        spans.push(Span::styled(
            format!(" {}w", repo.worktrees.len()),
            Style::default().fg(colors::OVERLAY),
        ));
    }

    spans
}
