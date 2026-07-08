use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::BranchOverlay;
use crate::ui::colors;

/// Render the branch cleanup overlay.
pub fn render(frame: &mut Frame, area: Rect, overlay: &BranchOverlay) {
    let popup_area = centered_rect(70, 80, area);
    frame.render_widget(Clear, popup_area);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    if overlay.confirm_delete {
        let count = overlay.selected_count();
        let names = overlay.selected_branch_names();
        lines.push(Line::from(Span::styled(
            format!("  Delete {} branch{}?", count, if count == 1 { "" } else { "es" }),
            Style::default().fg(colors::YELLOW),
        )));
        lines.push(Line::from(""));
        for name in &names {
            lines.push(Line::from(Span::styled(
                format!("    {name}"),
                Style::default().fg(colors::RED),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  y", Style::default().fg(colors::TEXT).add_modifier(Modifier::BOLD)),
            Span::styled(" confirm  ", Style::default().fg(colors::OVERLAY)),
            Span::styled("n/Esc", Style::default().fg(colors::TEXT).add_modifier(Modifier::BOLD)),
            Span::styled(" cancel", Style::default().fg(colors::OVERLAY)),
        ]));
    } else {
        for (i, branch) in overlay.branches.iter().enumerate() {
            let is_selected = i == overlay.selected;
            let marker = if branch.selected_for_delete {
                "  [x] "
            } else if is_selected {
                "  [ ] "
            } else {
                "      "
            };

            let name_style = if branch.is_current {
                Style::default().fg(colors::GREEN).add_modifier(Modifier::BOLD)
            } else if branch.is_protected {
                Style::default().fg(colors::OVERLAY)
            } else if is_selected {
                Style::default().fg(colors::SAPPHIRE).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors::TEXT)
            };

            let mut spans = vec![
                Span::raw(marker.to_string()),
                Span::styled(branch.name.clone(), name_style),
            ];

            if branch.is_current {
                spans.push(Span::styled(" (current)", Style::default().fg(colors::GREEN)));
            }
            if branch.is_protected {
                spans.push(Span::styled(" (protected)", Style::default().fg(colors::OVERLAY)));
            }
            if branch.is_merged && !branch.is_current && !branch.is_protected {
                spans.push(Span::styled(" merged", Style::default().fg(colors::PEACH)));
            }

            lines.push(Line::from(spans));
        }

        lines.push(Line::from(""));
        let sel_count = overlay.selected_count();
        let mut hint_spans = vec![
            Span::styled("  enter", Style::default().fg(colors::TEXT).add_modifier(Modifier::BOLD)),
            Span::styled(" switch  ", Style::default().fg(colors::OVERLAY)),
            Span::styled("space", Style::default().fg(colors::TEXT).add_modifier(Modifier::BOLD)),
            Span::styled(" mark  ", Style::default().fg(colors::OVERLAY)),
        ];
        if sel_count > 0 {
            hint_spans.extend([
                Span::styled("d", Style::default().fg(colors::TEXT).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" delete ({sel_count})  "), Style::default().fg(colors::OVERLAY)),
            ]);
        }
        hint_spans.extend([
            Span::styled("Esc", Style::default().fg(colors::TEXT).add_modifier(Modifier::BOLD)),
            Span::styled(" close", Style::default().fg(colors::OVERLAY)),
        ]);
        lines.push(Line::from(hint_spans));

        // Read-only preview of the highlighted branch's recent commits, so
        // you can see what happened there before switching. Filled lazily as
        // branches are highlighted (see main::ensure_preview).
        if let Some(branch) = overlay.selected_branch() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("── recent on {branch} ──"),
                Style::default().fg(colors::MAUVE),
            )));
            match overlay.previews.get(branch) {
                Some(commits) if !commits.is_empty() => {
                    for c in commits {
                        let author = crate::ui::author_code(&c.author);
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("  {}", c.hash),
                                Style::default().fg(colors::HASH_CLR),
                            ),
                            Span::styled(
                                format!(" {author} "),
                                Style::default().fg(colors::author_color(&c.author)),
                            ),
                            Span::styled(
                                crate::ui::truncate(&c.message, 52),
                                Style::default().fg(colors::TEXT),
                            ),
                            Span::styled(
                                format!("  {}", c.date),
                                Style::default().fg(colors::OVERLAY),
                            ),
                        ]));
                    }
                }
                Some(_) => lines.push(Line::from(Span::styled(
                    "  (no commits)",
                    Style::default().fg(colors::OVERLAY),
                ))),
                None => lines.push(Line::from(Span::styled(
                    "  loading…",
                    Style::default().fg(colors::OVERLAY),
                ))),
            }
        }
    }

    let title = format!(" Branches — {} ", overlay.repo_name);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::LAVENDER))
        .title(Span::styled(
            title,
            Style::default().fg(colors::LAVENDER).add_modifier(Modifier::BOLD),
        ));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, popup_area);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Percentage(percent_y)])
        .flex(Flex::Center)
        .split(area);
    Layout::horizontal([Constraint::Percentage(percent_x)])
        .flex(Flex::Center)
        .split(vertical[0])[0]
}
