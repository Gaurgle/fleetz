use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, View};
use crate::model::Summary;
use crate::ui::colors;

/// Render the footer: summary stats on the left, keybind hints on the right.
///
/// Never lets the two sides collide: hints drop their descriptions first
/// (keys only) when tight, and the summary drops entirely if even that
/// doesn't fit, rather than risk a negative spacer width.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let summary = app.summary();
    let width = area.width as usize;

    // A transient status message (fetch/switch/refresh result) takes over the
    // left side while present, so single-shot actions actually report back --
    // otherwise the summary just sits there and the action looks like a no-op.
    let left = match &app.status_message {
        Some(msg) => {
            let color = if msg.is_error { colors::RED } else { colors::GREEN };
            vec![Span::styled(msg.text.clone(), Style::default().fg(color))]
        }
        None => summary_spans(&summary, app.show_prs),
    };

    let full_hints = keybind_hints(app.view, true);
    let compact_hints = keybind_hints(app.view, false);

    let (mut spans, hints) = if fits(width, &left, &full_hints) {
        (left, full_hints)
    } else if fits(width, &left, &compact_hints) {
        (left, compact_hints)
    } else {
        (Vec::new(), compact_hints)
    };

    let spacer_width = width.saturating_sub(line_width(&spans) + line_width(&hints));
    spans.push(Span::raw(" ".repeat(spacer_width.max(1))));
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

/// Keybind hints. `with_desc` shows the trailing word ("lazygit", "pull", ...);
/// set false to fall back to keys only when space is tight.
fn keybind_hints(view: View, with_desc: bool) -> Vec<Span<'static>> {
    let key_style = Style::default()
        .fg(colors::TEXT)
        .add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(colors::OVERLAY);
    let gap = Span::raw(if with_desc { "  " } else { " " });

    let view_hint = match view {
        View::Detail => "compact",
        View::Compact => "detail",
    };

    let mut hints = vec![Span::styled("[enter]", key_style)];
    if with_desc {
        hints.push(Span::styled(" lazygit", desc_style));
    }
    hints.push(gap.clone());
    hints.push(Span::styled("[f]", key_style));
    if with_desc {
        hints.push(Span::styled(" fetch", desc_style));
    }
    hints.push(gap.clone());
    hints.push(Span::styled("[s]", key_style));
    if with_desc {
        hints.push(Span::styled(" switch", desc_style));
    }
    hints.push(gap.clone());
    hints.push(Span::styled("[v]", key_style));
    if with_desc {
        hints.push(Span::styled(format!(" {view_hint}"), desc_style));
    }
    hints.push(gap.clone());
    hints.push(Span::styled("[q]", key_style));
    if with_desc {
        hints.push(Span::styled(" quit", desc_style));
    }
    hints
}

fn line_width(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.len()).sum()
}

/// Whether summary + hints fit side by side in `width` with at least one
/// column of breathing room between them.
fn fits(width: usize, spans: &[Span], hints: &[Span]) -> bool {
    line_width(spans) + line_width(hints) < width
}
