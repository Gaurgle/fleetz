use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::model::RepoStatus;
use crate::ui::badges;
use crate::ui::colors;

/// Render the compact view: two-column dense grid of all repos.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let visible = app.visible_repos();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(colors::SURFACE))
        // Compact view is a single pane -- always the focused one -- so its
        // numbered title reads as active.
        .title_top(crate::ui::panel_title(1, "repos", true))
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

    // Below this width a second column would squeeze each repo's name/status
    // below a usable minimum, so fall back to one dense column instead.
    const MIN_TWO_COL_WIDTH: usize = 48;
    let columns = if content.width as usize >= MIN_TWO_COL_WIDTH { 2 } else { 1 };
    let col_width = (content.width as usize / columns).saturating_sub(1);

    let selected = app.selected_repo_index().unwrap_or(usize::MAX);
    let mut lines: Vec<Line> = Vec::new();

    for row in visible.chunks(columns) {
        let mut row_spans: Vec<Span<'static>> = Vec::new();
        for (i, entry) in row.iter().enumerate() {
            if i > 0 {
                let prev_len: usize = row_spans.iter().map(|s| s.content.chars().count()).sum();
                let pad = col_width.saturating_sub(prev_len) + 2;
                row_spans.push(Span::raw(" ".repeat(pad)));
            }
            row_spans.extend(compact_repo_spans(entry.1, entry.0 == selected, col_width));
        }
        lines.push(Line::from(row_spans));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, content);
}

fn compact_repo_spans(
    repo: &RepoStatus,
    is_selected: bool,
    max_width: usize,
) -> Vec<Span<'static>> {
    let marker = if is_selected { "▸ " } else { "  " };
    let name_style = if is_selected {
        Style::default()
            .fg(colors::SAPPHIRE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors::SAPPHIRE)
    };

    // Tight, right-aligned badges flush to each grid cell's right edge, dots
    // filling from the name to them (see ui::badges::badge_spans). Reserve
    // room for marker, glyph, at least one dot, and the badges; whatever
    // remains is the name's budget -- truncate rather than overflow into the
    // next column.
    let (badge_spans, badge_w) = badges::badge_spans(repo);
    let right_len = badge_w + 1; // +1 for space after dots
    let reserved = badges::MARKER_W + badges::GLYPH_W + 1 + right_len;
    let name_budget = max_width.saturating_sub(reserved).max(3);
    let name = crate::ui::truncate(&repo.name, name_budget);

    let left_len = badges::MARKER_W + badges::GLYPH_W + name.chars().count() + 1;
    let dot_count = max_width.saturating_sub(left_len + right_len).max(1);
    let dots = "·".repeat(dot_count);

    let mut spans = vec![
        Span::raw(marker.to_string()),
        badges::glyph_span(repo),
        Span::styled(name, name_style),
        Span::styled(format!(" {dots} "), Style::default().fg(colors::SURFACE)),
    ];
    spans.extend(badge_spans);
    spans
}
