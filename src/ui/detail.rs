use chrono::{DateTime, Utc};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
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
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(colors::SURFACE))
        .title(Span::styled("REPOS", Style::default().fg(colors::OVERLAY)));

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
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

    // Dot-fill between name and right-aligned badges
    let left_len = marker.len() + repo.name.len() + 1; // +1 for space before dots
    let right_len = badge_len + 1; // +1 for space after dots
    let dot_count = col_width.saturating_sub(left_len + right_len).max(1);
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
    let repo = match app.selected_repo() {
        Some(r) => r,
        None => {
            let empty = Paragraph::new("No repos to display")
                .style(Style::default().fg(colors::OVERLAY));
            frame.render_widget(empty, area);
            return;
        }
    };

    let mut lines: Vec<Line> = Vec::new();

    // Header: repo name + status + path
    lines.push(Line::from(vec![
        Span::styled(
            format!("DETAIL — {}", repo.name),
            Style::default().fg(colors::OVERLAY),
        ),
    ]));
    lines.push(Line::from(""));

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

    // Branches
    if !repo.branches.is_empty() {
        lines.push(Line::from(Span::styled(
            "branches",
            Style::default().fg(colors::OVERLAY),
        )));
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
        }
        lines.push(Line::from(""));
    }

    // Recent commits
    if !repo.commits.is_empty() {
        lines.push(Line::from(Span::styled(
            "recent commits",
            Style::default().fg(colors::OVERLAY),
        )));
        for c in &repo.commits {
            let mut commit_spans = vec![
                Span::styled(
                    format!("  {}", c.hash),
                    Style::default().fg(colors::HASH_CLR),
                ),
                Span::raw(" "),
                Span::styled(c.message.clone(), Style::default().fg(colors::TEXT)),
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
        lines.push(Line::from(Span::styled(
            format!("{} open {pr_label}", repo.prs.len()),
            Style::default().fg(colors::MAUVE),
        )));
        for pr in &repo.prs {
            let age = format_age(pr.created_at);
            let review = pr.review_status.label();
            let mut pr_spans = vec![
                Span::styled(
                    format!("  #{}", pr.number),
                    Style::default().fg(colors::MAUVE),
                ),
                Span::raw(" "),
                Span::styled(pr.title.clone(), Style::default().fg(colors::TEXT)),
            ];
            pr_spans.push(Span::styled(
                format!("  {} · {age}", pr.author),
                Style::default().fg(colors::OVERLAY),
            ));
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
        lines.push(Line::from(Span::styled(
            format!("{} open {pr_label}", repo.pr_count),
            Style::default().fg(colors::MAUVE),
        )));
    }

    // Worktrees
    if !repo.worktrees.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("{} worktree{}", repo.worktrees.len(), if repo.worktrees.len() == 1 { "" } else { "s" }),
            Style::default().fg(colors::OVERLAY),
        )));
        for wt in &repo.worktrees {
            lines.push(Line::from(vec![
                Span::styled(format!("  {}", wt.branch), Style::default().fg(colors::TEXT)),
                Span::styled(format!("  {}", wt.path), Style::default().fg(colors::OVERLAY)),
            ]));
        }
    }

    // Stashes
    if repo.stash_count > 0 {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("{} stash{}", repo.stash_count, if repo.stash_count == 1 { "" } else { "es" }),
            Style::default().fg(colors::OVERLAY),
        )));
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
