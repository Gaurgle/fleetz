use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::model::RepoStatus;
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

    let primary = app.primary_repos();
    let elsewhere = app.elsewhere_repos();
    let local_only = app.local_only_repos();

    // Primary repos
    for (idx, repo) in &primary {
        items.push(repo_list_item(repo, *idx == app.selected_repo_index().unwrap_or(usize::MAX)));
    }

    // "also active" section
    if !elsewhere.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("── ", Style::default().fg(colors::SURFACE)),
            Span::styled("also active", Style::default().fg(colors::MAUVE)),
            Span::styled(" ──", Style::default().fg(colors::SURFACE)),
        ])));
        for (idx, repo) in &elsewhere {
            items.push(repo_list_item(repo, *idx == app.selected_repo_index().unwrap_or(usize::MAX)));
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
            items.push(repo_list_item(repo, *idx == app.selected_repo_index().unwrap_or(usize::MAX)));
        }
    }

    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(colors::SURFACE))
        .title(Span::styled("REPOS", Style::default().fg(colors::OVERLAY)));

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn repo_list_item(repo: &RepoStatus, is_selected: bool) -> ListItem<'static> {
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

    let mut spans = vec![
        Span::raw(marker.to_string()),
        Span::styled(repo.name.clone(), name_style),
        Span::styled(" ", Style::default()),
        Span::styled(status, Style::default().fg(status_color)),
    ];

    // PR badge
    if repo.pr_count > 0 {
        spans.push(Span::styled(
            format!(" {}PR", repo.pr_count),
            Style::default().fg(colors::MAUVE),
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

    // PR count
    if repo.pr_count > 0 {
        let pr_label = if repo.pr_count == 1 { "PR" } else { "PRs" };
        lines.push(Line::from(Span::styled(
            format!("{} open {pr_label}", repo.pr_count),
            Style::default().fg(colors::MAUVE),
        )));
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}
