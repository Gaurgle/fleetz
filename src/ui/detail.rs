use chrono::{DateTime, Utc};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::model::{CiStatus, RepoStatus};
use crate::ui::colors;

/// Render the detail view: repo list (left 35%) + detail pane (right 65%).
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::horizontal([
        Constraint::Percentage(35),
        Constraint::Percentage(65),
    ])
    .spacing(1)
    .split(area);

    render_repo_list(frame, chunks[0], app);
    render_detail_pane(frame, chunks[1], app);
}

fn render_repo_list(frame: &mut Frame, area: Rect, app: &App) {
    let mut items: Vec<ListItem> = Vec::new();
    // Available width inside the block (minus border)
    let col_width = area.width.saturating_sub(2) as usize;

    let primary = app.primary_repos();
    let elsewhere = app.elsewhere_repos();
    let local_only = app.local_only_repos();
    let selected_idx = app.selected_repo_index().unwrap_or(usize::MAX);

    let total = primary.len() + elsewhere.len() + local_only.len();

    // Primary repos
    for (idx, repo) in &primary {
        items.push(repo_list_item(repo, *idx == selected_idx, col_width));
    }

    // "also active" section
    if !elsewhere.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("── ", Style::default().fg(colors::SURFACE)),
            Span::styled("also active", Style::default().fg(colors::MAUVE)),
            Span::styled(" ──", Style::default().fg(colors::SURFACE)),
        ])));
        for (idx, repo) in &elsewhere {
            items.push(repo_list_item(repo, *idx == selected_idx, col_width));
        }
    }

    // "local changes" section
    if !local_only.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("── ", Style::default().fg(colors::SURFACE)),
            Span::styled("local changes", Style::default().fg(colors::MAUVE)),
            Span::styled(" ──", Style::default().fg(colors::SURFACE)),
        ])));
        for (idx, repo) in &local_only {
            items.push(repo_list_item(repo, *idx == selected_idx, col_width));
        }
    }

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
                format!(" {total} "),
                Style::default().fg(colors::OVERLAY),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Filter strip + divider, then the repo list below.
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

    let list = List::new(items);
    frame.render_widget(list, rows[2]);
}

fn repo_list_item(repo: &RepoStatus, is_selected: bool, col_width: usize) -> ListItem<'static> {
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

    // Build right-side badge string to calculate dot fill
    let ci_sym = repo.ci_status.symbol();
    let mut badge_len = status.len();
    if !ci_sym.is_empty() {
        badge_len += 2; // " ✓"
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

    // Dot-fill between name and right-aligned badges. Truncate the name with
    // an ellipsis rather than let it (and the badges after it) overflow the
    // column when the pane is narrow.
    let right_len = badge_len + 1; // +1 for space after dots
    let reserved = marker.len() + 1 + right_len;
    let name_budget = col_width.saturating_sub(reserved).max(3);
    let name = crate::ui::truncate(&repo.name, name_budget);

    let left_len = marker.len() + name.chars().count() + 1; // +1 for space before dots
    let dot_count = col_width.saturating_sub(left_len + right_len).max(1);
    let dots = "·".repeat(dot_count);

    let mut spans = vec![
        Span::raw(marker.to_string()),
        Span::styled(name, name_style),
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

    // PR badge
    if repo.pr_count > 0 {
        spans.push(Span::styled(
            format!(" {}PR", repo.pr_count),
            Style::default().fg(colors::MAUVE),
        ));
    }

    // Stash badge
    if repo.stash_count > 0 {
        spans.push(Span::styled(
            format!(" {}s", repo.stash_count),
            Style::default().fg(colors::OVERLAY),
        ));
    }

    // Worktree badge
    if !repo.worktrees.is_empty() {
        spans.push(Span::styled(
            format!(" {}w", repo.worktrees.len()),
            Style::default().fg(colors::OVERLAY),
        ));
    }

    ListItem::new(Line::from(spans))
}

fn render_detail_pane(frame: &mut Frame, area: Rect, app: &App) {
    let panel = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(colors::SURFACE));

    let repo = match app.selected_repo() {
        Some(r) => r,
        None => {
            let empty = Paragraph::new("No repos to display")
                .style(Style::default().fg(colors::OVERLAY))
                .block(panel.title_top(Line::from(" detail ").left_aligned()));
            frame.render_widget(empty, area);
            return;
        }
    };

    let mut right_title: Vec<Span> = Vec::new();
    if repo.ci_status != CiStatus::Unknown {
        let (ci_sym, ci_color, ci_label) = match repo.ci_status {
            CiStatus::Passed => ("✓", colors::GREEN, "passed"),
            CiStatus::Failed => ("✗", colors::RED, "failed"),
            CiStatus::Pending => ("○", colors::YELLOW, "pending"),
            CiStatus::Unknown => ("", colors::OVERLAY, ""),
        };
        right_title.push(Span::styled(
            format!(" {ci_sym} {ci_label} "),
            Style::default().fg(ci_color),
        ));
    }

    let panel = panel
        .title_top(
            Line::from(vec![
                Span::styled(
                    format!(" {} ", repo.name),
                    Style::default()
                        .fg(colors::SAPPHIRE)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(repo.short_status(), Style::default().fg(colors::OVERLAY)),
            ])
            .left_aligned(),
        )
        .title_top(Line::from(right_title).right_aligned());
    let inner = panel.inner(area);
    frame.render_widget(panel, area);
    let area = inner;
    let width = area.width as usize;

    let mut lines: Vec<Line> = Vec::new();

    // Status + path
    let status_color = if repo.is_synced() {
        colors::GREEN
    } else if repo.dirty > 0 {
        colors::YELLOW
    } else {
        colors::PEACH
    };
    let status_icon = if repo.is_synced() { "●" } else { "○" };
    let path_str = repo.path.as_deref().unwrap_or("not cloned");

    lines.push(Line::from(vec![
        Span::styled(
            format!("{status_icon} {}", repo.long_status()),
            Style::default().fg(status_color),
        ),
        Span::raw("   "),
        Span::styled(path_str.to_string(), Style::default().fg(colors::OVERLAY)),
    ]));
    lines.push(Line::from(""));

    // Branches (worktrees shown inline under the branch they're checked out on)
    let mut shown_worktrees = std::collections::HashSet::new();
    if !repo.branches.is_empty() {
        lines.push(Line::from(section_label("branches")));
        for b in &repo.branches {
            let mut branch_spans = vec![Span::styled(
                format!("  {}", b.name),
                Style::default().fg(colors::TEXT),
            )];
            if b.behind > 0 {
                branch_spans.push(Span::styled(
                    format!(" {} behind", b.behind),
                    Style::default().fg(colors::PEACH),
                ));
            }
            if b.ahead > 0 {
                branch_spans.push(Span::styled(
                    format!(" {} ahead", b.ahead),
                    Style::default().fg(colors::PEACH),
                ));
            }
            if b.behind == 0 && b.ahead == 0 && b.upstream.is_some() {
                branch_spans
                    .push(Span::styled(" ✓", Style::default().fg(colors::GREEN)));
            }
            lines.push(Line::from(branch_spans));

            for wt in repo.worktrees.iter().filter(|wt| wt.branch == b.name) {
                shown_worktrees.insert(&wt.path);
                lines.push(Line::from(Span::styled(
                    format!("    ⎇ {}", wt.path),
                    Style::default().fg(colors::OVERLAY),
                )));
            }
        }
        lines.push(Line::from(""));
    }

    // Recent commits. The message is truncated to what's left after the
    // hash/diffstat/author -- letting it wrap instead orphans the author
    // onto its own line, which reads as broken rather than as a long commit.
    if !repo.commits.is_empty() {
        lines.push(Line::from(section_label("recent commits")));
        for c in &repo.commits {
            let prefix_len = 2 + c.hash.chars().count() + 1;
            let mut stat_len = 0;
            if c.insertions > 0 {
                stat_len += format!("  +{}", c.insertions).chars().count();
            }
            if c.deletions > 0 {
                stat_len += format!(" -{}", c.deletions).chars().count();
            }
            let author_len = 2 + c.author.chars().count();
            let msg_budget = width
                .saturating_sub(prefix_len + stat_len + author_len)
                .max(10);
            let message = crate::ui::truncate(&c.message, msg_budget);

            let mut commit_spans = vec![
                Span::styled(
                    format!("  {}", c.hash),
                    Style::default().fg(colors::HASH_CLR),
                ),
                Span::raw(" "),
                Span::styled(message, Style::default().fg(colors::TEXT)),
            ];

            if c.insertions > 0 || c.deletions > 0 {
                commit_spans.push(Span::raw("  "));
                if c.insertions > 0 {
                    commit_spans.push(Span::styled(
                        format!("+{}", c.insertions),
                        Style::default().fg(colors::DIFF_ADD),
                    ));
                    commit_spans.push(Span::raw(" "));
                }
                if c.deletions > 0 {
                    commit_spans.push(Span::styled(
                        format!("-{}", c.deletions),
                        Style::default().fg(colors::DIFF_DEL),
                    ));
                }
            }

            commit_spans.push(Span::raw("  "));
            commit_spans.push(Span::styled(
                c.author.clone(),
                Style::default().fg(colors::OVERLAY),
            ));

            lines.push(Line::from(commit_spans));
        }
        lines.push(Line::from(""));
    }

    // History graph -- git's own --graph topology, just colorized: the
    // ASCII structure dimmed, hash/refs/message carrying the color.
    if !repo.graph.is_empty() {
        lines.push(Line::from(section_label("history")));
        for g in &repo.graph {
            if g.hash.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("  {}", g.prefix),
                    Style::default().fg(colors::SURFACE),
                )));
                continue;
            }
            let prefix_len = 2 + g.prefix.chars().count();
            let refs_len = if g.refs.is_empty() {
                0
            } else {
                1 + g.refs.chars().count()
            };
            let msg_budget = width
                .saturating_sub(prefix_len + g.hash.chars().count() + refs_len + 1)
                .max(10);
            let message = crate::ui::truncate(&g.message, msg_budget);

            let mut graph_spans = vec![
                Span::styled(format!("  {}", g.prefix), Style::default().fg(colors::SURFACE)),
                Span::styled(g.hash.clone(), Style::default().fg(colors::HASH_CLR)),
            ];
            if !g.refs.is_empty() {
                graph_spans.push(Span::styled(
                    format!(" {}", g.refs),
                    Style::default().fg(colors::MAUVE),
                ));
            }
            graph_spans.push(Span::raw(" "));
            graph_spans.push(Span::styled(message, Style::default().fg(colors::TEXT)));
            lines.push(Line::from(graph_spans));
        }
        lines.push(Line::from(""));
    }

    // Reflog -- recent HEAD movements, for a "what did I just do" glance.
    if !repo.reflog.is_empty() {
        lines.push(Line::from(section_label("reflog")));
        for entry in &repo.reflog {
            let prefix_len = 2 + entry.hash.chars().count();
            let action_len = if entry.action.is_empty() {
                0
            } else {
                1 + entry.action.chars().count()
            };
            let msg_budget = width.saturating_sub(prefix_len + action_len + 1).max(10);
            let description = crate::ui::truncate(&entry.description, msg_budget);

            let mut reflog_spans = vec![Span::styled(
                format!("  {}", entry.hash),
                Style::default().fg(colors::HASH_CLR),
            )];
            if !entry.action.is_empty() {
                reflog_spans.push(Span::styled(
                    format!(" {}", entry.action),
                    Style::default().fg(colors::MAUVE),
                ));
            }
            reflog_spans.push(Span::raw(" "));
            reflog_spans.push(Span::styled(description, Style::default().fg(colors::TEXT)));
            lines.push(Line::from(reflog_spans));
        }
        lines.push(Line::from(""));
    }

    // CI status
    if repo.ci_status != CiStatus::Unknown {
        let (ci_sym, ci_color, ci_label) = match repo.ci_status {
            CiStatus::Passed => ("✓", colors::GREEN, "passed"),
            CiStatus::Failed => ("✗", colors::RED, "failed"),
            CiStatus::Pending => ("○", colors::YELLOW, "pending"),
            CiStatus::Unknown => ("", colors::OVERLAY, ""),
        };
        let mut ci_spans = vec![
            Span::styled(format!("CI {ci_sym} "), Style::default().fg(ci_color)),
            Span::styled(ci_label, Style::default().fg(ci_color)),
        ];
        if let Some(ref wf) = repo.ci_workflow {
            ci_spans.push(Span::styled(
                format!(" · {wf}"),
                Style::default().fg(colors::OVERLAY),
            ));
        }
        if let Some(ref dur) = repo.ci_duration {
            ci_spans.push(Span::styled(
                format!(" · {dur}"),
                Style::default().fg(colors::OVERLAY),
            ));
        }
        lines.push(Line::from(ci_spans));
    }

    // PRs
    if !repo.prs.is_empty() {
        let pr_label = if repo.prs.len() == 1 { "PR" } else { "PRs" };
        lines.push(Line::from(section_label(&format!(
            "{} open {pr_label}",
            repo.prs.len()
        ))));
        for pr in &repo.prs {
            let age = format_age(pr.created_at);
            let review = pr.review_status.label();

            let number_len = 3 + pr.number.to_string().chars().count();
            let trailer = format!("  {} · {age}", pr.author);
            let review_len = if review.is_empty() {
                0
            } else {
                3 + review.chars().count()
            };
            let title_budget = width
                .saturating_sub(number_len + trailer.chars().count() + review_len)
                .max(10);
            let title = crate::ui::truncate(&pr.title, title_budget);

            let mut pr_spans = vec![
                Span::styled(
                    format!("  #{}", pr.number),
                    Style::default().fg(colors::MAUVE),
                ),
                Span::raw(" "),
                Span::styled(title, Style::default().fg(colors::TEXT)),
            ];
            pr_spans.push(Span::styled(trailer, Style::default().fg(colors::OVERLAY)));
            if !review.is_empty() {
                pr_spans.push(Span::styled(
                    format!(" · {review}"),
                    Style::default().fg(colors::OVERLAY),
                ));
            }
            lines.push(Line::from(pr_spans));
        }
    } else if repo.pr_count > 0 {
        let pr_label = if repo.pr_count == 1 { "PR" } else { "PRs" };
        lines.push(Line::from(section_label(&format!(
            "{} open {pr_label}",
            repo.pr_count
        ))));
    }

    // Worktrees not matched to a local branch above (e.g. detached HEAD) --
    // the common case is already shown inline under its branch.
    let orphan_worktrees: Vec<_> = repo
        .worktrees
        .iter()
        .filter(|wt| !shown_worktrees.contains(&wt.path))
        .collect();
    if !orphan_worktrees.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(section_label("other worktrees")));
        for wt in orphan_worktrees {
            lines.push(Line::from(vec![
                Span::styled(format!("  {}", wt.branch), Style::default().fg(colors::TEXT)),
                Span::styled(format!("  {}", wt.path), Style::default().fg(colors::OVERLAY)),
            ]));
        }
    }

    // Stashes
    if repo.stash_count > 0 {
        lines.push(Line::from(""));
        lines.push(Line::from(section_label(&format!(
            "{} stash{}",
            repo.stash_count,
            if repo.stash_count == 1 { "" } else { "es" }
        ))));
        for entry in &repo.stash_entries {
            lines.push(Line::from(Span::styled(
                format!("  {entry}"),
                Style::default().fg(colors::OVERLAY),
            )));
        }
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// A bold section header inside the detail pane (e.g. "branches", "history").
fn section_label(text: &str) -> Span<'static> {
    Span::styled(
        text.to_string(),
        Style::default()
            .fg(colors::MAUVE)
            .add_modifier(Modifier::BOLD),
    )
}

fn format_age(created: DateTime<Utc>) -> String {
    let diff = Utc::now() - created;
    let days = diff.num_days();
    if days == 0 {
        let hours = diff.num_hours();
        if hours == 0 {
            format!("{}m ago", diff.num_minutes().max(1))
        } else {
            format!("{hours}h ago")
        }
    } else if days == 1 {
        "1d ago".to_string()
    } else if days < 30 {
        format!("{days}d ago")
    } else {
        format!("{}mo ago", days / 30)
    }
}
