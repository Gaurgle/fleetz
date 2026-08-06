use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Padding, Paragraph};

use crate::app::{App, View};
use crate::model::Summary;
use crate::ui::colors;

/// Render the footer: summary stats on the left, keybind hints on the right.
///
/// Never lets the two sides collide: hints drop their descriptions first
/// (keys only) when tight, and the summary drops entirely if even that
/// doesn't fit, rather than risk a negative spacer width.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    // An in-flight window apply takes over the whole footer with a gauge,
    // so the user sees real-time progress instead of a frozen list.
    if let Some(progress) = app.apply_progress {
        let gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(colors::SURFACE))
                    // Inset to match the summary it replaces, so the footer
                    // does not jump two columns wider mid-apply.
                    .padding(Padding::horizontal(crate::ui::PANE_CONTENT_INSET)),
            )
            .gauge_style(Style::default().fg(colors::GREEN))
            .ratio(progress.ratio())
            .label(progress.label());
        frame.render_widget(gauge, area);
        return;
    }

    let summary = app.summary();
    // The fit and spacer math below is about the *padded* width, not the
    // footer's full rect: the block insets the content on both sides so the
    // summary lines up with the pane content above it rather than starting two
    // columns further left, hard against the terminal edge.
    let width = area.width.saturating_sub(2 * crate::ui::PANE_CONTENT_INSET) as usize;

    // A transient status message (fetch/switch/refresh result) takes over the
    // left side while present, so single-shot actions actually report back --
    // otherwise the summary just sits there and the action looks like a no-op.
    let left = match &app.status_message {
        Some(msg) => {
            let color = if msg.is_error {
                colors::RED
            } else {
                colors::GREEN
            };
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
            .border_style(Style::default().fg(colors::SURFACE))
            .padding(Padding::horizontal(crate::ui::PANE_CONTENT_INSET)),
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
    if spans
        .last()
        .map(|s| s.content.contains('·'))
        .unwrap_or(false)
    {
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
    hints.push(Span::styled("[r]", key_style));
    if with_desc {
        hints.push(Span::styled(" refresh", desc_style));
    }
    hints.push(gap.clone());
    hints.push(Span::styled("[R]", key_style));
    if with_desc {
        hints.push(Span::styled(" reload", desc_style));
    }
    hints.push(gap.clone());
    hints.push(Span::styled("[f]", key_style));
    if with_desc {
        hints.push(Span::styled(" fetch", desc_style));
    }
    hints.push(gap.clone());
    hints.push(Span::styled("[w]", key_style));
    if with_desc {
        hints.push(Span::styled(" watch", desc_style));
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
    // Help sits second-to-last rather than first: it is the least urgent hint
    // in normal use, but it is also the one that must never be dropped, since
    // it is how every other binding is discovered. The keys-only fallback
    // keeps `[?]` visible even when the descriptions go.
    hints.push(Span::styled("[?]", key_style));
    if with_desc {
        hints.push(Span::styled(" help", desc_style));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn help_is_hinted_in_the_footer_with_the_other_global_actions() {
        // It used to live in the header while every other action key was down
        // here, which is the inconsistency this guards.
        assert!(text(&keybind_hints(View::Detail, true)).contains("[?] help"));
    }

    #[test]
    fn help_survives_the_keys_only_fallback() {
        // Help is how every other binding is discovered, so it is the one hint
        // that must stay visible when the descriptions are dropped for width.
        let compact = text(&keybind_hints(View::Detail, false));
        assert!(compact.contains("[?]"), "got {compact:?}");
        assert!(!compact.contains("help"), "descriptions should be gone");
    }

    #[test]
    fn the_view_hint_names_the_view_it_switches_to() {
        assert!(text(&keybind_hints(View::Detail, true)).contains("[v] compact"));
        assert!(text(&keybind_hints(View::Compact, true)).contains("[v] detail"));
    }

    #[test]
    fn quit_stays_last() {
        // Muscle memory: the rightmost hint should not move as hints are added.
        assert!(text(&keybind_hints(View::Detail, true)).ends_with(" quit"));
        assert!(text(&keybind_hints(View::Detail, false)).ends_with("[q]"));
    }
}
