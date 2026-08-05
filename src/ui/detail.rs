use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, List, ListItem, ListState, Padding, Paragraph, Wrap,
};

use crate::app::{App, FocusedPanel};
use crate::model::{CiStatus, Commit, RepoStatus};
use crate::ui::badges;
use crate::ui::colors;
use crate::ui::format_age;

/// Height (incl. border) of the inspector pane docked under the detail pane.
/// Enough for the header line, a blank, a few message lines, and the stat;
/// only shown when the right column is tall enough to spare it.
pub const INSPECTOR_HEIGHT: u16 = 10;

/// Most changed files listed in the inspector before the rest are collapsed
/// into a "… and N more" line -- keeps a huge commit from turning the pane
/// into an endless scroll.
const INSPECTOR_FILE_CAP: usize = 30;

/// Render the detail view: repo list (left 35%) + a right column (65%) that
/// stacks the detail pane over the inspector pane.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)])
        .spacing(1)
        .split(area);

    render_repo_list(frame, chunks[0], app);

    // Dock the inspector under the detail pane, but only when there's room --
    // on a short terminal the detail pane keeps the whole column.
    if chunks[1].height > INSPECTOR_HEIGHT + 6 {
        let right = Layout::vertical([Constraint::Min(1), Constraint::Length(INSPECTOR_HEIGHT)])
            .split(chunks[1]);
        render_detail_pane(frame, right[0], app);
        render_inspector(frame, right[1], app);
    } else {
        render_detail_pane(frame, chunks[1], app);
    }
}

fn render_repo_list(frame: &mut Frame, area: Rect, app: &App) {
    let mut items: Vec<ListItem> = Vec::new();
    // Width available for a row inside the block: the pane minus its border (2)
    // and horizontal padding (2, from `Padding::horizontal(1)` on the block).
    let col_width = area.width.saturating_sub(4) as usize;

    let primary = app.primary_repos();
    let elsewhere = app.elsewhere_repos();
    let local_only = app.local_only_repos();
    let selected_idx = app.selected_repo_index().unwrap_or(usize::MAX);

    let total = primary.len() + elsewhere.len() + local_only.len();

    // Tracks where in the flattened `items` (repos interleaved with section
    // header rows) the selected repo lands, so ListState can be told to keep
    // that exact row in view -- rather than the repo's position among just
    // the repos, which would drift once header rows are counted in.
    let mut selected_display_idx = None;

    // "github activity" section -- only labeled when another section follows,
    // matching how a lone unlabeled list reads fine but three stacked groups
    // need every one of them named to tell them apart.
    if !primary.is_empty() && (!elsewhere.is_empty() || !local_only.is_empty()) {
        items.push(section_header_item("github activity"));
    }
    for (idx, repo) in &primary {
        if *idx == selected_idx {
            selected_display_idx = Some(items.len());
        }
        items.push(repo_list_item(
            repo,
            *idx == selected_idx,
            col_width,
            app.watch_mode && !app.blink_on,
        ));
    }

    // "also active" section, set off by the same blank row as the section
    // below it so the three groups are spaced identically.
    if !elsewhere.is_empty() {
        if !items.is_empty() {
            items.push(ListItem::new(Line::from("")));
        }
        items.push(section_header_item("also active"));
        for (idx, repo) in &elsewhere {
            if *idx == selected_idx {
                selected_display_idx = Some(items.len());
            }
            items.push(repo_list_item(
                repo,
                *idx == selected_idx,
                col_width,
                app.watch_mode && !app.blink_on,
            ));
        }
    }

    // "local changes" section, set off by a blank row from the github-activity
    // / also-active repos above it so it reads as its own block rather than
    // butting up against the last entry above (only when something precedes it).
    if !local_only.is_empty() {
        if !items.is_empty() {
            items.push(ListItem::new(Line::from("")));
        }
        items.push(section_header_item("local changes"));
        for (idx, repo) in &local_only {
            if *idx == selected_idx {
                selected_display_idx = Some(items.len());
            }
            items.push(repo_list_item(
                repo,
                *idx == selected_idx,
                col_width,
                app.watch_mode && !app.blink_on,
            ));
        }
    }

    let focused = app.focused_panel == FocusedPanel::RepoList;
    let border_color = if focused {
        colors::LAVENDER
    } else {
        colors::SURFACE
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::horizontal(1))
        .title_top(crate::ui::panel_title(1, "repos", focused))
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

    // A fresh ListState each frame is fine -- ratatui's List recomputes the
    // scroll offset from `.select()` on every render, so the selected row is
    // always brought into view without needing to persist offset ourselves.
    let list = List::new(items);
    let mut state = ListState::default();
    state.select(selected_display_idx);
    frame.render_stateful_widget(list, rows[2], &mut state);
}

/// A section divider row inside the repo list, matching repoz's
/// `section_div` bash helper: "── label ──" in the dim/mauve pairing.
fn section_header_item(label: &str) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::styled("── ", Style::default().fg(colors::SURFACE)),
        Span::styled(label.to_string(), Style::default().fg(colors::MAUVE)),
        Span::styled(" ──", Style::default().fg(colors::SURFACE)),
    ]))
}

fn repo_list_item(
    repo: &RepoStatus,
    is_selected: bool,
    col_width: usize,
    dim_pending: bool,
) -> ListItem<'static> {
    let marker = if is_selected { "▸ " } else { "  " };
    let name_style = if is_selected {
        Style::default()
            .fg(colors::SAPPHIRE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors::SAPPHIRE)
    };

    // Badges are built tight and right-aligned flush to the inner edge, with
    // a dot-fill running from the name out to them (repoz style -- see
    // ui::badges::badge_spans). The marker is a fixed display width, so the
    // badges land at the same right edge whether or not the row is selected;
    // only differing badge sets between rows shift a status token, exactly
    // as in repoz. Truncate the name rather than let it overflow when narrow.
    let (badge_spans, badge_w) = badges::badge_spans(repo, dim_pending);
    let right_len = badge_w + 1; // +1 for space after dots
    let reserved = badges::MARKER_W + badges::GLYPH_W + 1 + right_len;
    let name_budget = col_width.saturating_sub(reserved).max(3);
    let name = crate::ui::truncate(&repo.name, name_budget);

    let left_len = badges::MARKER_W + badges::GLYPH_W + name.chars().count() + 1; // +1 for space before dots
    let dot_count = col_width.saturating_sub(left_len + right_len).max(1);
    let dots = "·".repeat(dot_count);

    let mut spans = vec![
        Span::raw(marker.to_string()),
        badges::glyph_span(repo),
        Span::styled(name, name_style),
        Span::styled(format!(" {dots} "), Style::default().fg(colors::SURFACE)),
    ];
    spans.extend(badge_spans);

    ListItem::new(Line::from(spans))
}

/// A commit's age, rendered fresh on every draw. Empty when git handed back
/// a timestamp we couldn't parse, so the separator collapses rather than the
/// row showing a bogus age.
pub fn commit_age(commit: &Commit) -> String {
    commit.date.map(format_age).unwrap_or_default()
}

/// Minimum blank columns between the path and the right-aligned owner group.
/// Below this the two read as one run-on string, so the group is dropped.
const MIN_IDENTITY_GAP: usize = 2;

/// Blank columns to insert so the identity group lands flush right, or None
/// when the pane is too narrow to hold it with a readable gap (in which case
/// the caller drops the group entirely).
fn identity_gap(width: usize, left_w: usize, identity_w: usize) -> Option<usize> {
    width
        .checked_sub(left_w + identity_w)
        .filter(|gap| *gap >= MIN_IDENTITY_GAP)
}

/// Total display width of a span run.
fn span_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| s.content.chars().count()).sum()
}

/// The owner group shown flush right on the detail pane's status row: the
/// kind glyph plus the owner login. Empty when no owner is known (no GitHub
/// match and no parseable remote), which also suppresses the name row below
/// it, since `name` alone would just repeat the panel title.
///
/// The glyph and the owner share `badges::kind_color`, so org/private/public
/// reads the same here as in the repo list.
fn owner_spans(repo: &RepoStatus) -> (Vec<Span<'static>>, usize) {
    let Some(owner) = repo.owner_display() else {
        return (Vec::new(), 0);
    };

    let color = badges::kind_color(repo.repo_kind);
    let mut spans = Vec::new();

    if let Some(glyph) = badges::kind_glyph(repo.repo_kind) {
        spans.push(Span::styled(
            format!("{glyph} "),
            Style::default().fg(color),
        ));
    }
    spans.push(Span::styled(
        owner,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ));

    let width = span_width(&spans);
    (spans, width)
}

/// The repo name, rendered flush right on the row under the owner. Paired
/// with `owner_spans`: empty whenever that one is, so the block is all or
/// nothing.
fn name_spans(repo: &RepoStatus) -> (Vec<Span<'static>>, usize) {
    if repo.owner_display().is_none() {
        return (Vec::new(), 0);
    }
    let spans = vec![Span::styled(
        repo.name.clone(),
        Style::default()
            .fg(colors::SAPPHIRE)
            .add_modifier(Modifier::BOLD),
    )];
    let width = span_width(&spans);
    (spans, width)
}

fn render_detail_pane(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.focused_panel == FocusedPanel::Detail;
    let border_color = if focused {
        colors::LAVENDER
    } else {
        colors::SURFACE
    };
    let num_color = if focused {
        colors::LAVENDER
    } else {
        colors::SURFACE
    };
    let panel = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::new(1, 1, 1, 0));

    let repo = match app.selected_repo() {
        Some(r) => r,
        None => {
            let empty = Paragraph::new("No repos to display")
                .style(Style::default().fg(colors::OVERLAY))
                .block(panel.title_top(crate::ui::panel_title(2, "detail", focused)));
            frame.render_widget(empty, area);
            return;
        }
    };

    let mut right_title: Vec<Span> = Vec::new();
    if repo.ci_status != CiStatus::Unknown {
        let (ci_sym, ci_color, ci_label) = match repo.ci_status {
            CiStatus::Passed => ("✓", colors::GREEN, "passed"),
            CiStatus::Failed => ("✗", colors::RED, "failed"),
            CiStatus::Pending => ("○", colors::PEACH, "pending"),
            CiStatus::Unknown => ("", colors::OVERLAY, ""),
        };
        // Named, not just a glyph: "✓ passed" in a pane frame reads as
        // ambiguous praise for whatever the pane holds, whereas "CI passed"
        // says what actually passed.
        right_title.push(Span::styled(
            format!(" {ci_sym} CI {ci_label} "),
            Style::default().fg(ci_color),
        ));
    }

    let panel = panel
        .title_top(
            Line::from(vec![
                Span::styled(" [2] ", Style::default().fg(num_color)),
                Span::styled(
                    repo.name.clone(),
                    Style::default()
                        .fg(colors::SAPPHIRE)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" {} ", repo.short_status()),
                    Style::default().fg(colors::OVERLAY),
                ),
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

    let mut status_line = vec![
        Span::styled(
            format!("{status_icon} {}", repo.long_status()),
            Style::default().fg(status_color),
        ),
        Span::raw("   "),
        Span::styled(path_str.to_string(), Style::default().fg(colors::OVERLAY)),
    ];
    let mut url_line: Vec<Span> = Vec::new();
    if let Some(ref url) = repo.remote_url {
        url_line.push(Span::styled(
            format!("  {url}"),
            Style::default().fg(colors::OVERLAY),
        ));
    }

    // Identity, stacked flush to the pane's right edge: owner on the status
    // row, repo name directly beneath it on the remote-URL row. Both rows are
    // placed together or not at all, so the pair never renders half-aligned.
    let (owner_group, owner_w) = owner_spans(repo);
    let (name_group, name_w) = name_spans(repo);
    if !owner_group.is_empty() {
        let status_w = span_width(&status_line);
        let url_w = span_width(&url_line);
        if let (Some(owner_gap), Some(name_gap)) = (
            identity_gap(width, status_w, owner_w),
            identity_gap(width, url_w, name_w),
        ) {
            status_line.push(Span::raw(" ".repeat(owner_gap)));
            status_line.extend(owner_group);
            url_line.push(Span::raw(" ".repeat(name_gap)));
            url_line.extend(name_group);
        }
    }

    lines.push(Line::from(status_line));
    if !url_line.is_empty() {
        lines.push(Line::from(url_line));
    }
    lines.push(Line::from(""));

    // Branches (worktrees shown inline under the branch they're checked out on)
    let mut shown_worktrees = std::collections::HashSet::new();
    if !repo.branches.is_empty() || !repo.remote_only_branches.is_empty() {
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
                branch_spans.push(Span::styled(" ✓", Style::default().fg(colors::GREEN)));
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

        // Remote branches with no local counterpart -- never fetched/checked
        // out, or checked out once and deleted locally after merging. Shown
        // so "what am I missing locally" is visible at a glance.
        if !repo.remote_only_branches.is_empty() {
            for name in &repo.remote_only_branches {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {name}"), Style::default().fg(colors::OVERLAY)),
                    Span::styled(" not fetched", Style::default().fg(colors::SURFACE)),
                ]));
            }
        }
        lines.push(Line::from(""));
    }

    // Recent commits, repoz print_commit_line style: hash, fixed-width author
    // code, message, then a dot-fill running out to a diffstat right-aligned
    // at the pane edge. The dots always fill to the edge -- even when a commit
    // has no diffstat -- so every row reads to the same right margin.
    if !repo.commits.is_empty() {
        lines.push(Line::from(section_label("recent commits")));
        // Authors show as a fixed-width short code (see ui::author_code), so
        // the message column after it never drifts.
        let author_w = crate::ui::AUTHOR_W;

        // The commit the inspector is showing gets a ▸ marker so it's clear
        // which row the pane below reflects; the marker brightens while the
        // detail/inspector panels are the ones being driven.
        let sel = app.detail_selected;
        let commits_active = matches!(
            app.focused_panel,
            FocusedPanel::Detail | FocusedPanel::Inspector
        );

        for (ci, c) in repo.commits.iter().enumerate() {
            let author = crate::ui::author_code(&c.author);
            let author_pad = " ".repeat(author_w.saturating_sub(author.chars().count()));
            let is_sel = ci == sel;
            let marker = if is_sel { "▸ " } else { "  " };
            let marker_style = if is_sel && commits_active {
                Style::default()
                    .fg(colors::SAPPHIRE)
                    .add_modifier(Modifier::BOLD)
            } else if is_sel {
                Style::default().fg(colors::SAPPHIRE)
            } else {
                Style::default().fg(colors::SURFACE)
            };

            // marker(2) + hash + " " + author-column + " " up to the message start.
            let prefix_len = 2 + c.hash.chars().count() + 1 + author_w + 1;
            let stat_text = commit_stat_text(c);
            let stat_len = stat_text.chars().count();
            let right = if stat_len > 0 { 1 + stat_len } else { 0 }; // space + stat

            let avail = width.saturating_sub(prefix_len);
            // Leave room for a leading space, at least one dot, and the stat.
            let msg_budget = avail.saturating_sub(2 + right).max(6);
            let message = crate::ui::truncate(&c.message, msg_budget);
            let msg_len = message.chars().count();
            // message + " " + dots + right == avail
            let dot_count = avail.saturating_sub(msg_len + 1 + right).max(1);

            let mut commit_spans = vec![
                Span::styled(marker, marker_style),
                Span::styled(c.hash.clone(), Style::default().fg(colors::HASH_CLR)),
                Span::raw(" "),
                Span::styled(author, Style::default().fg(colors::author_color(&c.author))),
                Span::raw(author_pad),
                Span::raw(" "),
                Span::styled(message, Style::default().fg(colors::TEXT)),
                Span::raw(" "),
                Span::styled("·".repeat(dot_count), Style::default().fg(colors::SURFACE)),
            ];

            if stat_len > 0 {
                commit_spans.push(Span::raw(" "));
                if c.insertions > 0 {
                    commit_spans.push(Span::styled(
                        format!("+{}", c.insertions),
                        Style::default().fg(colors::DIFF_ADD),
                    ));
                    if c.deletions > 0 {
                        commit_spans.push(Span::raw(" "));
                    }
                }
                if c.deletions > 0 {
                    commit_spans.push(Span::styled(
                        format!("-{}", c.deletions),
                        Style::default().fg(colors::DIFF_DEL),
                    ));
                }
            }

            lines.push(Line::from(commit_spans));
        }
        lines.push(Line::from(""));
    }

    // History graph -- git's own --graph topology, just colorized. The ASCII
    // prefix's width varies line to line with the branch topology (a merge
    // pulls it wider than a straight line), so it's padded to the widest
    // prefix in this repo's graph -- otherwise hash/author/message would
    // drift left and right per row. Author gets its own fixed-width, per-
    // author-colored column (hashed to a dull palette -- see
    // colors::author_color) so commits read as "who did this" at a glance,
    // the same way the repo list's badge columns stay put regardless of
    // what's on a given row.
    if !repo.graph.is_empty() {
        lines.push(Line::from(section_label("history")));
        let prefix_w = repo
            .graph
            .iter()
            .map(|g| g.prefix.chars().count())
            .max()
            .unwrap_or(0);
        // Fixed-width author code, same as the recent-commits column.
        let author_w = crate::ui::AUTHOR_W;
        for g in &repo.graph {
            let prefix_pad = " ".repeat(prefix_w.saturating_sub(g.prefix.chars().count()));
            if g.hash.is_empty() {
                let mut spans = vec![Span::raw("  ")];
                spans.extend(graph_prefix_spans(&g.prefix));
                spans.push(Span::raw(prefix_pad));
                lines.push(Line::from(spans));
                continue;
            }
            let author = crate::ui::author_code(&g.author);
            let author_pad = " ".repeat(author_w.saturating_sub(author.chars().count()));
            let refs_len = if g.refs.is_empty() {
                0
            } else {
                1 + g.refs.chars().count()
            };
            let msg_budget = width
                .saturating_sub(2 + prefix_w + g.hash.chars().count() + 1 + author_w + 1 + refs_len)
                .max(10);
            let message = crate::ui::truncate(&g.message, msg_budget);

            let mut graph_spans = vec![Span::raw("  ")];
            graph_spans.extend(graph_prefix_spans(&g.prefix));
            graph_spans.push(Span::raw(prefix_pad));
            graph_spans.push(Span::styled(
                g.hash.clone(),
                Style::default().fg(colors::HASH_CLR),
            ));
            graph_spans.push(Span::raw(" "));
            graph_spans.push(Span::styled(
                author,
                Style::default().fg(colors::author_color(&g.author)),
            ));
            graph_spans.push(Span::raw(author_pad));
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
            CiStatus::Pending => ("○", colors::PEACH, "pending"),
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
                Span::styled(
                    format!("  {}", wt.branch),
                    Style::default().fg(colors::TEXT),
                ),
                Span::styled(
                    format!("  {}", wt.path),
                    Style::default().fg(colors::OVERLAY),
                ),
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

    // Clamp against actual content length here (rather than trusting
    // app.detail_scroll's own generous cap) so scrolling past the end of a
    // short repo's content doesn't leave a blank gap.
    let max_scroll = (lines.len() as u16).saturating_sub(area.height);
    let scroll = app.detail_scroll.min(max_scroll);

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Render the `[3] inspector` pane: the full details of the commit the detail
/// pane's cursor is on -- a two-line masthead (hash, then author · date), the
/// full message (subject bold, body untruncated), and the per-file diffstat --
/// so you can read a commit in full without leaving the dashboard. The block's
/// padding insets every line uniformly, so wrapped prose keeps its left edge
/// instead of falling back to column zero.
fn render_inspector(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.focused_panel == FocusedPanel::Inspector;
    let border_color = if focused {
        colors::LAVENDER
    } else {
        colors::SURFACE
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::new(2, 2, 1, 0))
        .title_top(crate::ui::panel_title(3, "inspector", focused));
    let inner = block.inner(area);

    let Some(c) = app.inspected_commit() else {
        let empty = Paragraph::new("no commit selected")
            .style(Style::default().fg(colors::OVERLAY))
            .block(block);
        frame.render_widget(empty, area);
        return;
    };

    // Masthead: hash on its own line (bold, the pane's anchor), then author and
    // relative date beneath -- two calm lines rather than one crowded row --
    // a blank, then the subject (bold, the heading). Body and files follow.
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            c.hash.clone(),
            Style::default()
                .fg(colors::HASH_CLR)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                c.author.clone(),
                Style::default().fg(colors::author_color(&c.author)),
            ),
            Span::styled(" · ", Style::default().fg(colors::SURFACE)),
            Span::styled(commit_age(c), Style::default().fg(colors::OVERLAY)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            c.message.clone(),
            Style::default()
                .fg(colors::TEXT)
                .add_modifier(Modifier::BOLD),
        )),
    ];

    // Body below the subject, untruncated; wraps within the block padding.
    if !c.body.is_empty() {
        lines.push(Line::from(""));
        for body_line in c.body.lines() {
            lines.push(Line::from(Span::styled(
                body_line.to_string(),
                Style::default().fg(colors::TEXT),
            )));
        }
    }

    // Changed files, with the commit's totals on the section header. Capped so
    // a sprawling commit can't wall off the pane -- the rest collapse into a
    // trailing "… and N more".
    if !c.files.is_empty() {
        lines.push(Line::from(""));
        let mut header = vec![section_label(&format!("changed files ({})", c.files.len()))];
        if c.insertions > 0 {
            header.push(Span::styled(
                format!("   +{}", c.insertions),
                Style::default().fg(colors::DIFF_ADD),
            ));
        }
        if c.deletions > 0 {
            header.push(Span::styled(
                format!(" -{}", c.deletions),
                Style::default().fg(colors::DIFF_DEL),
            ));
        }
        lines.push(Line::from(header));

        for f in c.files.iter().take(INSPECTOR_FILE_CAP) {
            lines.push(file_stat_line(f));
        }
        if c.files.len() > INSPECTOR_FILE_CAP {
            lines.push(Line::from(Span::styled(
                format!("… and {} more", c.files.len() - INSPECTOR_FILE_CAP),
                Style::default().fg(colors::OVERLAY),
            )));
        }
    }

    // Clamp against the *wrapped* row count, not the raw line count -- long
    // prose wraps to several rows, so a raw count would stop scrolling short of
    // the real end.
    let max_scroll = wrapped_row_count(&lines, inner.width).saturating_sub(inner.height);
    let scroll = app.inspector_scroll.min(max_scroll);
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0))
        .block(block);
    frame.render_widget(paragraph, area);
}

/// One "changed file" row for the inspector: a fixed-width +A/-B stat column
/// (green adds, red deletes) then the path, so paths line up in a column no
/// matter how big each file's diff is. Binary files (0/0) show only the path.
fn file_stat_line(f: &crate::model::FileStat) -> Line<'static> {
    const STAT_W: usize = 9;
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut stat_len = 0;
    if f.added > 0 {
        let s = format!("+{}", f.added);
        stat_len += s.chars().count();
        spans.push(Span::styled(s, Style::default().fg(colors::DIFF_ADD)));
    }
    if f.removed > 0 {
        if stat_len > 0 {
            spans.push(Span::raw(" "));
            stat_len += 1;
        }
        let s = format!("-{}", f.removed);
        stat_len += s.chars().count();
        spans.push(Span::styled(s, Style::default().fg(colors::DIFF_DEL)));
    }
    // Pad the stat column so every path starts at the same column (+1 so even
    // an over-long stat keeps a gap before the path).
    spans.push(Span::raw(" ".repeat(STAT_W.saturating_sub(stat_len) + 1)));
    spans.push(Span::styled(
        f.path.clone(),
        Style::default().fg(colors::TEXT),
    ));
    Line::from(spans)
}

/// Estimate how many terminal rows `lines` occupy once wrapped to `width`, so
/// the inspector's scroll offset can be clamped to real content height. A
/// `Paragraph` wraps long lines, so a raw line count under-counts. Guards a
/// zero width so a degenerate pane can't divide by zero.
fn wrapped_row_count(lines: &[Line], width: u16) -> u16 {
    let w = width.max(1) as usize;
    let mut rows = 0usize;
    for line in lines {
        let len: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        rows += if len == 0 { 1 } else { len.div_ceil(w) };
    }
    rows.min(u16::MAX as usize) as u16
}

/// Colorize a `git log --graph` ASCII prefix by character column ("lane"),
/// so parallel branch lines read as distinct dull hues instead of one flat,
/// nearly-invisible color -- see colors::graph_lane_color.
fn graph_prefix_spans(prefix: &str) -> Vec<Span<'static>> {
    prefix
        .chars()
        .enumerate()
        .map(|(i, ch)| {
            let color = if ch == ' ' {
                colors::SURFACE
            } else {
                colors::graph_lane_color(i)
            };
            Span::styled(ch.to_string(), Style::default().fg(color))
        })
        .collect()
}

/// Plain-text diffstat like "+12 -3", for width calc as well as display.
fn commit_stat_text(c: &crate::model::Commit) -> String {
    if c.insertions == 0 && c.deletions == 0 {
        return String::new();
    }
    let mut s = String::new();
    if c.insertions > 0 {
        s.push_str(&format!("+{}", c.insertions));
    }
    if c.deletions > 0 {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&format!("-{}", c.deletions));
    }
    s
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RepoKind, RepoLocation};

    fn repo(name: &str, kind: RepoKind, owner: Option<&str>) -> RepoStatus {
        RepoStatus {
            name: name.to_string(),
            path: Some("~/repos/x".to_string()),
            location: RepoLocation::Primary,
            branches: vec![],
            commits: vec![],
            behind: 0,
            ahead: 0,
            dirty: 0,
            pr_count: 0,
            prs: vec![],
            ci_status: CiStatus::Unknown,
            ci_workflow: None,
            ci_duration: None,
            groups: vec![],
            stash_count: 0,
            stash_entries: vec![],
            worktrees: vec![],
            reflog: vec![],
            graph: vec![],
            repo_kind: kind,
            owner: owner.map(|s| s.to_string()),
            remote_url: None,
            remote_only_branches: vec![],
            work_key: None,
        }
    }

    /// The reported width must match what actually gets drawn, or the group
    /// stops being flush right.
    fn drawn_width(spans: &[Span<'_>]) -> usize {
        spans.iter().map(|s| s.content.chars().count()).sum()
    }

    #[test]
    fn identity_splits_owner_above_name() {
        let r = repo("fleetz", RepoKind::Public, Some("Gaurgle"));
        let (owner, owner_w) = owner_spans(&r);
        let (name, name_w) = name_spans(&r);

        let owner_text: String = owner.iter().map(|s| s.content.as_ref()).collect();
        let name_text: String = name.iter().map(|s| s.content.as_ref()).collect();
        assert!(owner_text.ends_with("Gaurgle"), "got {owner_text:?}");
        assert!(
            !owner_text.contains("fleetz"),
            "name belongs on its own row: {owner_text:?}"
        );
        assert_eq!(name_text, "fleetz");

        assert_eq!(owner_w, drawn_width(&owner));
        assert_eq!(name_w, drawn_width(&name));
    }

    #[test]
    fn identity_is_empty_without_an_owner() {
        // Name alone would just repeat the panel title, so neither row
        // renders: the block is all or nothing.
        let r = repo("fleetz", RepoKind::Unknown, None);
        assert!(owner_spans(&r).0.is_empty());
        assert_eq!(owner_spans(&r).1, 0);
        assert!(name_spans(&r).0.is_empty());
        assert_eq!(name_spans(&r).1, 0);
    }

    #[test]
    fn identity_falls_back_to_the_remote_url() {
        let mut r = repo("fleetz", RepoKind::Unknown, None);
        r.remote_url = Some("git@github.com:Gaurgle/fleetz.git".to_string());
        let owner_text: String = owner_spans(&r)
            .0
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(owner_text, "Gaurgle");
    }

    #[test]
    fn identity_kind_glyph_is_only_drawn_when_known() {
        let (org, org_w) = owner_spans(&repo("svc", RepoKind::Org, Some("acme")));
        let (unknown, unknown_w) = owner_spans(&repo("svc", RepoKind::Unknown, Some("acme")));
        // Same text, but the org row carries the leading building glyph.
        assert_eq!(org_w - unknown_w, badges::GLYPH_W);
        assert_eq!(org_w, drawn_width(&org));
        assert_eq!(unknown_w, drawn_width(&unknown));
    }

    #[test]
    fn identity_gap_pushes_the_group_flush_right() {
        // 40 wide, 10 used on the left, 14 for the group: 16 blank between.
        assert_eq!(identity_gap(40, 10, 14), Some(16));
    }

    #[test]
    fn identity_gap_drops_the_group_when_it_would_not_fit() {
        // Exactly filling the row leaves no separation, so it is dropped
        // rather than run on against the path.
        assert_eq!(identity_gap(24, 10, 14), None);
        assert_eq!(identity_gap(25, 10, 14), None, "one column is not a gap");
        assert_eq!(identity_gap(26, 10, 14), Some(2), "two columns is enough");
        // Overflowing must not underflow the subtraction.
        assert_eq!(identity_gap(20, 10, 14), None);
    }

    /// Renders the real pane and reads rows back out, so placement is
    /// verified against actual output rather than the arithmetic behind it.
    /// Row 2 is the status line, row 3 the remote-URL line (rows 0 and 1 are
    /// the border and the block's top padding). The border and padding are
    /// stripped so assertions are about content, not the frame around it.
    fn pane_rows(repo: RepoStatus, width: u16) -> (String, String) {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let app = crate::app::App::new(vec![repo], "test".to_string(), false, width, 40);
        let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                render_detail_pane(f, area, &app);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        // Leading whitespace is kept deliberately: the URL row is indented,
        // so trimming the front would make two flush-right rows measure
        // different lengths and hide a real misalignment.
        let row = |y: u16| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_matches('│')
                .trim_end()
                .to_string()
        };
        (row(2), row(3))
    }

    fn cloned_repo() -> RepoStatus {
        let mut r = repo("fleetz", RepoKind::Public, Some("Gaurgle"));
        r.path = Some("~/repos/fleetz".to_string());
        r.remote_url = Some("git@github.com:Gaurgle/fleetz.git".to_string());
        r
    }

    #[test]
    fn owner_and_name_stack_flush_to_the_right_edge() {
        let (status, url) = pane_rows(cloned_repo(), 60);

        assert!(
            status.contains("~/repos/fleetz"),
            "path missing: {status:?}"
        );
        assert!(url.contains("git@github.com"), "url missing: {url:?}");
        assert!(
            status.ends_with("Gaurgle"),
            "owner not flush right: {status:?}"
        );
        assert!(url.ends_with("fleetz"), "name not flush right: {url:?}");
        // The name sits directly under the owner, not merged onto one row.
        assert!(
            !status.contains("Gaurgle/fleetz"),
            "rows should not be joined: {status:?}"
        );
    }

    #[test]
    fn stacked_identity_is_right_aligned_on_both_rows() {
        // Both rows must end at the same column, which is the whole point of
        // stacking them; the left content differs in length, so this only
        // holds if each row is padded independently.
        let (status, url) = pane_rows(cloned_repo(), 60);
        let owner_end = status.chars().count();
        let name_end = url.chars().count();
        assert_eq!(
            owner_end, name_end,
            "rows end at different columns: {status:?} vs {url:?}"
        );
    }

    #[test]
    fn identity_is_dropped_whole_on_a_narrow_pane() {
        let (status, url) = pane_rows(cloned_repo(), 34);

        assert!(
            status.contains("~/repos/fleetz"),
            "path must survive: {status:?}"
        );
        assert!(
            !status.contains("Gaurgle"),
            "owner should be dropped, not truncated: {status:?}"
        );
        // Dropping is all-or-nothing: no orphan name row without its owner.
        assert!(
            !url.ends_with("fleetz") || url.contains("git@github.com:Gaurgle/fleetz.git"),
            "name row orphaned: {url:?}"
        );
    }
}
