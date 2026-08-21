use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, List, ListItem, ListState, Padding, Paragraph, Wrap,
};

use crate::app::{App, FocusedPanel, HistoryView};
use crate::model::{CiStatus, Commit, RepoStatus};
use crate::ui::badges;
use crate::ui::colors;
use crate::ui::format_age;

/// Smallest useful inspector (incl. border): header line, blank, a couple of
/// message lines, and the stat. Below this it is dropped rather than shown as
/// a sliver.
const INSPECTOR_MIN_HEIGHT: u16 = 8;

/// Ceiling on the inspector's share of the right column. A commit's details
/// are bounded in a way the detail pane's history list is not, so past this
/// the extra rows are better spent on the pane above.
const INSPECTOR_MAX_HEIGHT: u16 = 24;

/// Rows the detail pane keeps for itself before the inspector gets any.
const DETAIL_MIN_HEIGHT: u16 = 8;

/// Split the right column into the detail pane and, when there is room for a
/// useful one, the inspector docked under it.
///
/// Single source of truth for that geometry: `render` draws from it and
/// `ui::panel_rects` hit-tests mouse clicks against it. They used to carry
/// separate copies of the same arithmetic, kept in sync by hand, where any
/// drift would land clicks on the wrong pane.
///
/// `pct` is the inspector's requested share of the column, which the resize
/// keys move. It is a request, not a promise: the min/max clamp and the detail
/// pane's floor both outrank it, so a short terminal still yields a readable
/// split (or drops the inspector entirely) whatever the user asked for.
pub fn split_right_column(area: Rect, pct: u16) -> (Rect, Option<Rect>) {
    if area.height < DETAIL_MIN_HEIGHT + INSPECTOR_MIN_HEIGHT {
        return (area, None);
    }
    let height = (area.height * pct / 100)
        .clamp(INSPECTOR_MIN_HEIGHT, INSPECTOR_MAX_HEIGHT)
        .min(area.height - DETAIL_MIN_HEIGHT);
    let v = Layout::vertical([Constraint::Min(1), Constraint::Length(height)]).split(area);
    (v[0], Some(v[1]))
}

/// Split the main area into the repo list and the right column. Shared by
/// `render` and `ui::panel_rects` for the same reason as the vertical split.
pub fn split_main(area: Rect, repo_list_pct: u16) -> (Rect, Rect) {
    let h = Layout::horizontal([
        Constraint::Percentage(repo_list_pct),
        Constraint::Percentage(100 - repo_list_pct),
    ])
    .spacing(1)
    .split(area);
    (h[0], h[1])
}

/// PR numbers shown inline on the header's remote row before the rest are
/// summarised with a `+`. Two keeps the row short enough that CI still fits
/// beside it on a normal pane.
const PR_NUMBERS_SHOWN: usize = 2;

/// Most changed files listed in the inspector before the rest are collapsed
/// into a "… and N more" line -- keeps a huge commit from turning the pane
/// into an endless scroll.
const INSPECTOR_FILE_CAP: usize = 30;

/// Render the detail view: repo list (left 35%) + a right column (65%) that
/// stacks the detail pane over the inspector pane.
/// Draw the three Detail panes into rectangles `ui::panel_rects` already
/// computed.
///
/// It takes the rects rather than an area to split, because the splitting is
/// no longer purely detail's business: the contribution strip is carved out
/// of the right column between the horizontal and vertical splits, so the
/// order lives one level up where both views can be seen at once.
pub fn render(
    frame: &mut Frame,
    list_area: Rect,
    detail_area: Rect,
    inspector_area: Option<Rect>,
    app: &App,
) {
    render_repo_list(frame, list_area, app);
    render_detail_pane(frame, detail_area, app);
    // The inspector is docked under the detail pane only when there's room --
    // on a short terminal the detail pane keeps the whole column.
    if let Some(inspector_area) = inspector_area {
        render_inspector(frame, inspector_area, app);
    }
}

fn render_repo_list(frame: &mut Frame, area: Rect, app: &App) {
    let mut items: Vec<ListItem> = Vec::new();
    // Width available for a row inside the block: the pane minus its border (2)
    // and horizontal padding (2, from `Padding::horizontal(1)` on the block).
    let col_width = area.width.saturating_sub(4) as usize;

    let selected_idx = app.selected_repo_index().unwrap_or(usize::MAX);
    let dim_pending = app.watch_mode && !app.blink_on;

    // Tracks where in the flattened `items` (repos interleaved with section
    // header rows) the selected repo lands, so ListState can be told to keep
    // that exact row in view -- rather than the repo's position among just
    // the repos, which would drift once header rows are counted in.
    let mut selected_display_idx = None;
    let mut push_repos = |items: &mut Vec<ListItem<'static>>, repos: &[(usize, &RepoStatus)]| {
        for (idx, repo) in repos {
            if *idx == selected_idx {
                selected_display_idx = Some(items.len());
            }
            items.push(repo_list_item(
                repo,
                *idx == selected_idx,
                col_width,
                dim_pending,
            ));
        }
    };

    let total;
    if app.has_text_query() {
        // While searching, the list is one set ranked best-match-first (see
        // `App::visible_repos`). Sections would fight that ordering, since a
        // repo's section says nothing about how well it matched.
        let results = app.visible_repos();
        total = results.len();
        items.push(section_header_item("results"));
        push_repos(&mut items, &results);
    } else {
        let primary = app.primary_repos();
        let elsewhere = app.elsewhere_repos();
        let local_only = app.local_only_repos();
        total = primary.len() + elsewhere.len() + local_only.len();

        // "github activity" section -- only labeled when another section
        // follows, matching how a lone unlabeled list reads fine but three
        // stacked groups need every one of them named to tell them apart.
        if !primary.is_empty() && (!elsewhere.is_empty() || !local_only.is_empty()) {
            items.push(section_header_item("github activity"));
        }
        push_repos(&mut items, &primary);

        // "also active" section, set off by the same blank row as the section
        // below it so the three groups are spaced identically.
        if !elsewhere.is_empty() {
            if !items.is_empty() {
                items.push(ListItem::new(Line::from("")));
            }
            items.push(section_header_item("also active"));
            push_repos(&mut items, &elsewhere);
        }

        // "local changes" section, set off by a blank row from the
        // github-activity / also-active repos above it so it reads as its own
        // block rather than butting up against the last entry above (only when
        // something precedes it).
        if !local_only.is_empty() {
            if !items.is_empty() {
                items.push(ListItem::new(Line::from("")));
            }
            items.push(section_header_item("local changes"));
            push_repos(&mut items, &local_only);
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

/// Right-aligned readout of which commit the pane's cursor is on, pinned under
/// the panel title: `[3/10 · a1b2c3d]` in grey.
///
/// Answers "which commit am I reading?" once the commit list itself has been
/// scrolled off the top, which is the whole reason it exists. The ordinal is
/// there for the same reason as the hash: with the list out of view, the hash
/// alone says *which* commit but not *where* in the list it sits.
///
/// Renders blank (rather than collapsing) when the repo has no commits, so the
/// body below never shifts up and down by a row as the selection changes.
fn render_selected_commit_readout(frame: &mut Frame, area: Rect, app: &App) {
    // Counted against the *active* view's list, and naming the hash that view
    // selected. It used to index `repo.commits` whatever was on screen, which
    // meant the commits, reflog and graph views all reported a commit from a
    // different list -- the same index-versus-hash trap the inspector has.
    let total = app.selected_commit_count();
    if total == 0 {
        return;
    }
    let index = app.detail_selected().min(total - 1);
    let Some(hash) = app.inspected_hash() else {
        return;
    };

    // No trailing space: the owner and repo-name rows below are padded flush
    // to the pane's inner edge, so a space here would park this one cell short
    // of the column they all share.
    let text = format!("[{}/{} · {}]", index + 1, total, hash);
    // Bright while the commit cursor is what the keys are actually driving:
    // this is a readout you go looking for, so it has to win against the
    // grey furniture around it. Steps down to OVERLAY (not all the way to
    // SURFACE) when focus is elsewhere, so it stays readable rather than
    // vanishing.
    let color = if matches!(
        app.focused_panel,
        FocusedPanel::Detail | FocusedPanel::Inspector
    ) {
        colors::TEXT
    } else {
        colors::OVERLAY
    };

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, Style::default().fg(color))).right_aligned()),
        area,
    );
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

    // CI used to be named in this frame as well. It sits on the header's
    // remote row now, beside the PRs, which is where the other things GitHub
    // knows live -- and the frame was repeating those exact words a row
    // above them. The right title is left free rather than refilled.
    let right_title: Vec<Span> = Vec::new();

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

    // One pinned row directly under the title, then the scrolling body. The
    // readout has to sit outside the paragraph: the whole body scrolls as a
    // unit, so a line inside it would scroll away exactly when it is needed,
    // which is when the commit list has been scrolled past.
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(inner);
    render_selected_commit_readout(frame, rows[0], app);
    let area = rows[1];
    let width = area.width as usize;

    let mut lines: Vec<Line> = Vec::new();

    // The pane's header block reads left-to-right as local-to-remote: what is
    // on this disk sits on the left of each row, what is on GitHub sits flush
    // right. The identity pair already worked that way; the rest of the block
    // now says so out loud rather than mixing "how it is doing" with "where
    // it lives" in one run of tokens.
    //
    //   ○ 3 behind, 1 ahead                               ● Gaurgle
    //   ~/repos/fleetz              git@github.com:Gaurgle/fleetz.git
    //   * main · 1 stash               2 open PRs #13 · CI ◉ passed
    //
    // Each row right-aligns its own remote half independently and drops it
    // when it will not fit. Owner and name used to be placed together or not
    // at all, because they were one fact stacked over two rows; row two now
    // carries the remote URL instead, which spells the name out anyway.
    let status_color = badges::status_color(repo);
    // `✓` when synced, matching the list badge, and never `◉`: the ringed disk
    // belongs to CI alone now.
    let status_icon = if repo.is_synced() { "✓" } else { "○" };
    let path_str = repo.path.as_deref().unwrap_or("not cloned");

    // Row 1 left: how it is doing relative to the remote.
    let row1_left = vec![Span::styled(
        format!("{status_icon} {}", repo.long_status()),
        Style::default().fg(status_color),
    )];

    // Row 2 left: where it lives on this machine.
    let row2_left = vec![Span::styled(
        format!("  {path_str}"),
        Style::default().fg(colors::OVERLAY),
    )];

    // Row 3 left: facts that exist only on this disk.
    let mut row3_left: Vec<Span> = Vec::new();
    if let Some(head) = repo.branches.iter().find(|b| b.is_head) {
        row3_left.push(Span::styled(
            "  * ",
            Style::default().fg(colors::ref_color(crate::model::RefKind::Head)),
        ));
        row3_left.push(Span::styled(
            head.name.clone(),
            Style::default().fg(colors::ref_color(crate::model::RefKind::Head)),
        ));
    }
    // Stashes carry the same glyph the repo list uses, so the eye can find
    // them here the way it learned to find them there. Both these and the
    // worktree count are drawn in SUBTEXT rather than OVERLAY: OVERLAY is for
    // paths and furniture, and at that brightness real news reads as
    // decoration you have already dismissed.
    // Each carries its own colour rather than a shared grey. A stash is real
    // news and gets the colour its badge uses; a worktree count is closer to
    // furniture, so it keeps the quieter one.
    let mut local_extra: Vec<(String, ratatui::style::Color)> = Vec::new();
    if repo.stash_count > 0 {
        let noun = if repo.stash_count == 1 {
            "stash"
        } else {
            "stashes"
        };
        local_extra.push((
            format!("{} {} {noun}", badges::STASH_GLYPH, repo.stash_count),
            colors::STASH_CLR,
        ));
    }
    if !repo.worktrees.is_empty() {
        let noun = if repo.worktrees.len() == 1 {
            "worktree"
        } else {
            "worktrees"
        };
        local_extra.push((format!("{} {noun}", repo.worktrees.len()), colors::SUBTEXT));
    }
    for (extra, color) in local_extra {
        if row3_left.is_empty() {
            row3_left.push(Span::raw("  "));
        } else {
            row3_left.push(Span::styled(" · ", Style::default().fg(colors::SURFACE)));
        }
        row3_left.push(Span::styled(extra, Style::default().fg(color)));
    }

    // Row 3 right: facts that live on GitHub.
    let mut row3_right: Vec<Span> = Vec::new();
    if repo.pr_count > 0 {
        let label = if repo.pr_count == 1 { "PR" } else { "PRs" };
        let mut text = format!("{} open {label}", repo.pr_count);
        // The numbers themselves, so the pane names the PR rather than only
        // counting it. Capped so a busy repo cannot push CI off the row.
        if !repo.prs.is_empty() {
            let shown: Vec<String> = repo
                .prs
                .iter()
                .take(PR_NUMBERS_SHOWN)
                .map(|pr| format!("#{}", pr.number))
                .collect();
            text.push(' ');
            text.push_str(&shown.join(" "));
            if repo.prs.len() > PR_NUMBERS_SHOWN {
                text.push('+');
            }
        }
        row3_right.push(Span::styled(text, Style::default().fg(colors::PR_CLR)));
    }
    if repo.ci_status != CiStatus::Unknown {
        let (ci_sym, ci_color, ci_label) = badges::ci_display(&repo.ci_status);
        if !row3_right.is_empty() {
            row3_right.push(Span::styled(" · ", Style::default().fg(colors::SURFACE)));
        }
        row3_right.push(Span::styled(
            format!("CI {ci_sym} {ci_label}"),
            Style::default().fg(ci_color),
        ));
    }

    // Right halves. Owner on row 1, the remote locator on row 2, GitHub's own
    // state on row 3.
    let (owner_group, owner_w) = owner_spans(repo);
    let (remote_loc, remote_loc_w) = match repo.remote_url.as_deref() {
        Some(url) => (
            vec![Span::styled(
                url.to_string(),
                Style::default().fg(colors::OVERLAY),
            )],
            url.chars().count(),
        ),
        None => name_spans(repo),
    };
    let row3_right_w = span_width(&row3_right);

    let mut row1 = row1_left;
    let mut row2 = row2_left;
    let mut row3 = row3_left;
    for (line, right, right_w) in [
        (&mut row1, owner_group, owner_w),
        (&mut row2, remote_loc, remote_loc_w),
        (&mut row3, row3_right, row3_right_w),
    ] {
        if right.is_empty() {
            continue;
        }
        if let Some(gap) = identity_gap(width, span_width(line), right_w) {
            line.push(Span::raw(" ".repeat(gap)));
            line.extend(right);
        }
    }

    lines.push(Line::from(row1));
    lines.push(Line::from(row2));
    if !row3.is_empty() {
        lines.push(Line::from(row3));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(view_tabs(app.history_view)));
    lines.push(Line::from(""));

    // Branches (worktrees shown inline under the branch they're checked out on)
    let view = app.history_view;
    let mut shown_worktrees = std::collections::HashSet::new();
    if view == HistoryView::Info
        && (!repo.branches.is_empty() || !repo.remote_only_branches.is_empty())
    {
        lines.push(Line::from(section_label("branches")));
        for b in &repo.branches {
            // The checked-out branch gets git's own `*` marker, which keeps
            // the name column aligned, painted in the colour the history
            // pane already uses for HEAD so the two agree about what "where
            // you are" looks like. Nothing marked it before, so a repo with
            // two level branches drew them identically and the pane could
            // not answer the first thing anyone asks it.
            let head_color = colors::ref_color(crate::model::RefKind::Head);
            let (marker, name_color) = if b.is_head {
                ("* ", head_color)
            } else {
                ("  ", colors::TEXT)
            };
            let mut branch_spans = vec![
                Span::styled(marker, Style::default().fg(head_color)),
                Span::styled(b.name.clone(), Style::default().fg(name_color)),
            ];
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
            // An untracked branch used to render bare, which reads as "fine"
            // when it actually means "not on any remote". Same trap as a
            // not-cloned repo whose counters are all zero: say the state in
            // words rather than letting silence stand in for health. That
            // also makes the ✓ mean one thing again -- level with an upstream
            // that exists -- instead of covering two cases by omission.
            if b.upstream.is_none() {
                branch_spans.push(Span::styled(
                    " no upstream",
                    Style::default().fg(colors::OVERLAY),
                ));
            } else if b.behind == 0 && b.ahead == 0 {
                branch_spans.push(Span::styled(" ✓", Style::default().fg(colors::GREEN)));
            }
            lines.push(Line::from(branch_spans));

            for wt in repo.worktrees.iter().filter(|wt| wt.branch == b.name) {
                shown_worktrees.insert(&wt.path);
                lines.push(Line::from(Span::styled(
                    // The word, not a glyph: `⎇` is the Alt key symbol that
                    // shell prompts use for *branch*, so it read as "branch"
                    // in a list of branches. Categorical things get words.
                    format!("    worktree {}", wt.path),
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
                    Span::styled(" not fetched", Style::default().fg(colors::SUBTEXT)),
                ]));
            }
        }
        lines.push(Line::from(""));
    }

    // Recent commits, repoz print_commit_line style: hash, fixed-width author
    // code, message, then a dot-fill running out to a diffstat right-aligned
    // at the pane edge. The dots always fill to the edge -- even when a commit
    // has no diffstat -- so every row reads to the same right margin.
    // The newest few with their diffstats: a glance, and the one place the
    // per-commit churn is shown. `commits` carries the complete list in the
    // graph's own column layout, which has no room for a diffstat.
    if view == HistoryView::Info && !repo.commits.is_empty() {
        lines.push(Line::from(section_label("recent commits")));
        // Authors show as a fixed-width short code (see ui::author_code), so
        // the message column after it never drifts.
        let author_w = crate::ui::AUTHOR_W;

        // The commit the inspector is showing gets a ▸ marker so it's clear
        // which row the pane below reflects; the marker brightens while the
        // detail/inspector panels are the ones being driven.
        let sel = app.detail_selected();
        let commits_active = matches!(
            app.focused_panel,
            FocusedPanel::Detail | FocusedPanel::Inspector
        );

        for (ci, c) in repo.commits.iter().take(RECENT_COMMIT_COUNT).enumerate() {
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

            // The selected row's hash goes bold: the ▸ marker sits in a
            // fixed-width column of its own, so on a long list the eye has to
            // travel to find it. Bolding the hash puts the emphasis on the
            // thing the inspector below is actually showing.
            let hash_style = if is_sel {
                Style::default()
                    .fg(colors::HASH_CLR)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors::HASH_CLR)
            };
            let mut commit_spans = vec![
                Span::styled(marker, marker_style),
                Span::styled(c.hash.clone(), hash_style),
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
    // The compact graph list: git's topology in a column, then hash, author,
    // decorations and subject. `info` shows the newest few as a glance;
    // `commits` shows the lot.
    if view == HistoryView::Commits && !repo.graph.is_empty() {
        lines.push(Line::from(section_label("commits")));
        lines.extend(graph_list_lines(repo, width, app.detail_selected(), None));
        lines.push(Line::from(""));
    }

    // The expanded graph: git's own `log --graph` layout, one block per
    // commit with Merge/Author/Date and the full message. Reproduced rather
    // than recomposed -- the value is that it looks like what you get in a
    // terminal, so the renderer colours the lines and changes nothing else.
    if view == HistoryView::Graph && !repo.graph_expanded.is_empty() {
        lines.push(Line::from(section_label("history")));
        let selected_hash = app.inspected_hash().unwrap_or_default().to_string();
        for raw in &repo.graph_expanded {
            lines.push(expanded_graph_line(raw, &selected_hash, width));
        }
        lines.push(Line::from(""));
    }

    // Reflog -- recent HEAD movements, for a "what did I just do" glance.
    if view == HistoryView::Reflog && !repo.reflog.is_empty() {
        lines.push(Line::from(section_label("reflog")));
        for (ri, entry) in repo.reflog.iter().enumerate() {
            let prefix_len = 2 + entry.hash.chars().count();
            let action_len = if entry.action.is_empty() {
                0
            } else {
                1 + entry.action.chars().count()
            };
            let msg_budget = width.saturating_sub(prefix_len + action_len + 1).max(10);
            let description = crate::ui::truncate(&entry.description, msg_budget);

            let mut reflog_spans = vec![
                Span::raw("  "),
                Span::styled(entry.hash.clone(), hash_style(ri == app.detail_selected())),
            ];
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

    // CI's headline moved up to the header's remote row, where it belongs
    // with the other things GitHub knows. The workflow name and duration stay
    // here: they are detail rather than state, and the header row has to
    // leave room for the PR numbers beside them.
    if view == HistoryView::Info
        && repo.ci_status != CiStatus::Unknown
        && (repo.ci_workflow.is_some() || repo.ci_duration.is_some())
    {
        let (_, ci_color, _) = badges::ci_display(&repo.ci_status);
        let mut ci_spans = vec![Span::styled("CI ", Style::default().fg(ci_color))];
        let detail: Vec<String> = [repo.ci_workflow.clone(), repo.ci_duration.clone()]
            .into_iter()
            .flatten()
            .collect();
        ci_spans.push(Span::styled(
            detail.join(" · "),
            Style::default().fg(colors::OVERLAY),
        ));
        lines.push(Line::from(ci_spans));
    }

    // PRs
    // Gated on `-p`: the header row already names the open PRs by number, so
    // this section is the opt-in detail rather than the news itself.
    if view == HistoryView::Info && app.show_pr_detail && !repo.prs.is_empty() {
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
                    Style::default().fg(colors::PR_CLR),
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
    }
    // No count-only fallback here any more: without `-p` there is no list to
    // head, and the status line at the top of the pane already carries the
    // number. A bare heading repeating it three sections down was the same
    // fact twice.

    // Worktrees not matched to a local branch above (e.g. detached HEAD) --
    // the common case is already shown inline under its branch.
    let orphan_worktrees: Vec<_> = repo
        .worktrees
        .iter()
        .filter(|wt| !shown_worktrees.contains(&wt.path))
        .collect();
    if view == HistoryView::Info && !orphan_worktrees.is_empty() {
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
    if view == HistoryView::Info && repo.stash_count > 0 {
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

    // Measured from *wrapped* rows, not `lines.len()`: this paragraph wraps,
    // so a long commit subject occupies several rows and counting source lines
    // under-reports the height, leaving the last rows unreachable.
    //
    // Recorded back into the app so the scroll handlers clamp against the real
    // content instead of a fixed cap. Clamping here alone only fixed the
    // display: the offset itself kept climbing, so scrolling back stalled for
    // as many keypresses as it had overshot.
    let max_scroll = wrapped_row_count(&lines, area.width).saturating_sub(area.height);
    app.detail_max_scroll.set(max_scroll);
    let scroll = app.detail_scroll().min(max_scroll);

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
        // Distinguish "nothing selected" from "selected something this pane
        // cannot show". The reflog reaches back further than the commit list
        // does, so picking an old entry names a real commit that simply was
        // not loaded -- saying "no commit selected" there would be a lie.
        let text = match app.inspected_hash() {
            Some(hash) => format!("{hash} is outside the loaded commit range"),
            None => "no commit selected".to_string(),
        };
        let empty = Paragraph::new(text)
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
    // Recorded back so `App::scroll_inspector` clamps against it, rather than
    // letting the offset run past the end and stall the way back.
    let max_scroll = wrapped_row_count(&lines, inner.width).saturating_sub(inner.height);
    app.inspector_max_scroll.set(max_scroll);
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
/// A commit's decorations, coloured per ref kind: `(HEAD -> main, origin/main)`
/// with HEAD, the local branch and the remote branch each in their own colour
/// rather than the whole blob in one.
///
/// Returns the leading space too, so a commit with no decorations contributes
/// nothing at all rather than a stray column. Must agree with
/// `GraphLine::refs_width`, which budgets the message column against it.
fn ref_spans(refs: &[crate::model::GitRef]) -> Vec<Span<'static>> {
    if refs.is_empty() {
        return Vec::new();
    }
    let punct = || Style::default().fg(colors::OVERLAY);
    let mut spans = vec![Span::styled(" (", punct())];
    for (i, r) in refs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(", ", punct()));
        }
        if r.is_head {
            spans.push(Span::styled(
                crate::model::HEAD_ARROW,
                Style::default().fg(colors::ref_color(crate::model::RefKind::Head)),
            ));
        }
        spans.push(Span::styled(
            r.name.clone(),
            Style::default().fg(colors::ref_color(r.kind)),
        ));
    }
    spans.push(Span::styled(")", punct()));
    spans
}

/// An ordinary commit's node in the graph column.
///
/// Git draws every commit as `*`, which is a heavy glyph repeated on every
/// row and makes a long history read as noise. A small circle carries the
/// same meaning at a fraction of the weight, and leaves `*` free to mean the
/// one thing worth picking out: a merge.
///
/// U+26AC is East Asian Narrow. That is not incidental -- the graph column is
/// tiled arithmetic padded to the widest prefix, so an Ambiguous-width glyph
/// would be one column in some terminals and two in others, and every row
/// after it would misalign. The obvious candidates (`●` `○` `•` `◆` `⊙`) are
/// all Ambiguous; this one and `◦` `∘` `◊` are not.
const COMMIT_NODE: char = '⚬';

/// Commits shown in the `info` view: a glance, not a history. `commits`
/// carries the complete list.
const RECENT_COMMIT_COUNT: usize = 6;

/// The compact graph list, shared by `info` and `commits` so the two cannot
/// drift into rendering the same rows differently.
///
/// `limit` counts *commits*, not lines: connector rows carry no commit and
/// must ride along with the ones they connect, or the topology they draw
/// stops meaning anything.
fn graph_list_lines(
    repo: &RepoStatus,
    width: usize,
    selected: usize,
    limit: Option<usize>,
) -> Vec<Line<'static>> {
    let prefix_w = repo
        .graph
        .iter()
        .map(|g| g.prefix.chars().count())
        .max()
        .unwrap_or(0);
    let author_w = crate::ui::AUTHOR_W;
    let mut lines = Vec::new();
    let mut commit_index = 0usize;

    for g in &repo.graph {
        let prefix_pad = " ".repeat(prefix_w.saturating_sub(g.prefix.chars().count()));
        if g.hash.is_empty() {
            let mut spans = vec![Span::raw("  ")];
            spans.extend(graph_prefix_spans(&g.prefix, g.is_merge));
            spans.push(Span::raw(prefix_pad));
            lines.push(Line::from(spans));
            continue;
        }
        if limit.is_some_and(|n| commit_index >= n) {
            break;
        }
        let is_sel = commit_index == selected;
        commit_index += 1;

        let author = crate::ui::author_code(&g.author);
        let author_pad = " ".repeat(author_w.saturating_sub(author.chars().count()));
        // Leading space plus the parenthesised list; measured rather than
        // rendered-then-measured, so the message budget is known before any
        // span exists.
        let refs_len = if g.refs.is_empty() {
            0
        } else {
            1 + g.refs_width()
        };
        let msg_budget = width
            .saturating_sub(2 + prefix_w + g.hash.chars().count() + 1 + author_w + 1 + refs_len)
            .max(10);
        let message = crate::ui::truncate(&g.message, msg_budget);

        let mut graph_spans = vec![Span::raw("  ")];
        graph_spans.extend(graph_prefix_spans(&g.prefix, g.is_merge));
        graph_spans.push(Span::raw(prefix_pad));
        graph_spans.push(Span::styled(g.hash.clone(), hash_style(is_sel)));
        graph_spans.push(Span::raw(" "));
        graph_spans.push(Span::styled(
            author,
            Style::default().fg(colors::author_color(&g.author)),
        ));
        graph_spans.push(Span::raw(author_pad));
        graph_spans.extend(ref_spans(&g.refs));
        graph_spans.push(Span::raw(" "));
        graph_spans.push(Span::styled(message, Style::default().fg(colors::TEXT)));
        lines.push(Line::from(graph_spans));
    }
    lines
}

/// The selected row's hash goes bold in every list, so the pane always says
/// which commit the inspector below is showing.
fn hash_style(selected: bool) -> Style {
    let base = Style::default().fg(colors::HASH_CLR);
    if selected {
        base.add_modifier(Modifier::BOLD)
    } else {
        base
    }
}

/// One line of `git log --graph`'s expanded output, coloured.
///
/// Split at the first character that is not part of the graph drawing, so the
/// topology keeps its per-lane colours and the text after it is styled by what
/// git put there. The commit whose hash matches `selected` goes bold, which is
/// the only way this view can say what the inspector is showing.
fn expanded_graph_line(raw: &str, selected_hash: &str, width: usize) -> Line<'static> {
    const GRAPH_CHARS: [char; 6] = ['*', '|', '\\', '/', '_', ' '];
    let split = raw
        .char_indices()
        .find(|(_, c)| !GRAPH_CHARS.contains(c))
        .map_or(raw.len(), |(i, _)| i);
    let (prefix, rest) = raw.split_at(split);

    let mut spans = vec![Span::raw("  ")];
    spans.extend(graph_prefix_spans_with(prefix, None));

    let rest = crate::ui::truncate(rest, width.saturating_sub(2 + prefix.chars().count()));
    let style = if let Some(hash) = rest.strip_prefix("commit ") {
        // "commit 1c51c6e (tag: v0.3.0)" -- the hash is the first word.
        let bold = hash
            .split_whitespace()
            .next()
            .is_some_and(|h| !selected_hash.is_empty() && h == selected_hash);
        hash_style(bold)
    } else if ["Author:", "Date:", "Merge:"]
        .iter()
        .any(|k| rest.starts_with(k))
    {
        // Metadata rows: present, but not what you are reading the block for.
        Style::default().fg(colors::OVERLAY)
    } else {
        Style::default().fg(colors::TEXT)
    };
    spans.push(Span::styled(rest, style));
    Line::from(spans)
}

fn graph_prefix_spans(prefix: &str, is_merge: bool) -> Vec<Span<'static>> {
    graph_prefix_spans_with(prefix, (!is_merge).then_some(COMMIT_NODE))
}

/// The prefix with `*` optionally swapped for a lighter node.
///
/// `None` leaves it alone, which is what the expanded view wants: that format
/// spells merges out on their own `Merge:` line, so `*` is not carrying the
/// distinction there and swapping it would only make the output stop looking
/// like the `git log --graph` it is reproducing.
fn graph_prefix_spans_with(prefix: &str, node: Option<char>) -> Vec<Span<'static>> {
    prefix
        .chars()
        .enumerate()
        .map(|(i, ch)| {
            let color = if ch == ' ' {
                colors::SURFACE
            } else {
                colors::graph_lane_color(i)
            };
            // Exactly one `*` per commit row, and none on a connector row, so
            // this cannot touch the lane characters around it.
            let ch = match node {
                Some(n) if ch == '*' => n,
                _ => ch,
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
/// The view switcher, one row under the header.
///
/// Drawn above the scrolling body so it does not scroll away: it is how you
/// know what you are looking at, which is exactly the thing that must not
/// disappear when you page down. Active view in the focus colour used
/// elsewhere, inactive in SUBTEXT rather than OVERLAY -- these are choices
/// you can make, not furniture.
fn view_tabs(active: HistoryView) -> Vec<Span<'static>> {
    let mut spans = vec![Span::raw("  ")];
    for (i, view) in HistoryView::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(colors::SURFACE)));
        }
        let style = if *view == active {
            Style::default()
                .fg(colors::LAVENDER)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(colors::SUBTEXT)
        };
        spans.push(Span::styled(view.label(), style));
    }
    spans
}

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
    use crate::model::{GitRef, RefKind, RepoKind, RepoLocation};

    fn git_ref(kind: RefKind, name: &str, is_head: bool) -> GitRef {
        GitRef {
            kind,
            name: name.to_string(),
            is_head,
        }
    }

    fn span_text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// `*` on every row is a heavy glyph that turns a long history into
    /// noise. Ordinary commits get a light node; `*` is kept for merges,
    /// which is the one row worth picking out of a column.
    #[test]
    fn only_merges_keep_the_asterisk() {
        let ordinary: String = graph_prefix_spans("* ", false)
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(ordinary, "⚬ ", "got {ordinary:?}");

        let merge: String = graph_prefix_spans("* ", true)
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(merge, "* ", "merges keep the asterisk: {merge:?}");

        // Lane characters are untouched either way: the substitution must not
        // reach past the single node on the row.
        let branched: String = graph_prefix_spans("| * ", false)
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(branched, "| ⚬ ", "got {branched:?}");
        let connector: String = graph_prefix_spans("|/  ", false)
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(connector, "|/  ");
    }

    /// The graph column is padded to the widest prefix and drawn per cell, so
    /// an Ambiguous-width node would be one column in some terminals and two
    /// in others, and every row after it would misalign. Narrow is a
    /// requirement here, not a preference.
    #[test]
    fn the_commit_node_occupies_one_column() {
        // Ratatui's own measurement, which is the one the layout uses, so a
        // mismatch here is a mismatch on screen. U+26AC is East Asian Narrow;
        // the obvious alternatives (● ○ • ◆ ⊙) are all Ambiguous, meaning
        // their width is a terminal setting rather than a property.
        assert_eq!(Span::from(COMMIT_NODE.to_string()).width(), 1);
        for ambiguous in ['●', '○', '•', '◆', '⊙'] {
            assert_ne!(
                COMMIT_NODE, ambiguous,
                "{ambiguous} is Ambiguous-width and would misalign the column"
            );
        }
    }

    /// A repo carrying something for every view, so a view showing the wrong
    /// section is visible rather than merely absent.
    fn busy_repo() -> RepoStatus {
        let mut r = cloned_repo();
        r.stash_count = 2;
        r.branches = vec![crate::model::BranchStatus {
            name: "main".to_string(),
            upstream: Some("origin/main".to_string()),
            behind: 0,
            ahead: 0,
            is_head: true,
        }];
        r.commits = vec![crate::model::Commit {
            hash: "a1b2c3d".to_string(),
            author: "Gaurgle".to_string(),
            date: Some(chrono::Utc::now()),
            message: "feat: a thing".to_string(),
            body: String::new(),
            insertions: 3,
            deletions: 1,
            files: vec![],
        }];
        r.reflog = vec![crate::model::ReflogEntry {
            hash: "a1b2c3d".to_string(),
            action: "commit".to_string(),
            description: "feat: a thing".to_string(),
        }];
        r.graph = vec![crate::model::GraphLine {
            prefix: "* ".to_string(),
            hash: "a1b2c3d".to_string(),
            is_merge: false,
            refs: vec![],
            author: "Gaurgle".to_string(),
            message: "feat: a thing".to_string(),
        }];
        // Real `git log --graph` shape, so the expanded view has its own
        // content rather than borrowing the compact list's.
        r.graph_expanded = vec![
            "* commit a1b2c3d (HEAD -> main)".to_string(),
            "| Author: Gaurgle <g@example.com>".to_string(),
            "| Date:   2 hours ago".to_string(),
            "|".to_string(),
            "|     feat: a thing".to_string(),
        ];
        r
    }

    fn pane_text(view: HistoryView, width: u16) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), width, 44);
        app.history_view = view;
        let mut term = Terminal::new(TestBackend::new(width, 30)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        (0..30u16)
            .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Each view draws its own section and none of the others'. The whole
    /// point is that they take turns rather than queueing for one scroll
    /// offset, so a section leaking across views defeats it.
    #[test]
    fn each_view_shows_only_its_own_section() {
        let sections = [
            (HistoryView::Info, "recent commits"),
            (HistoryView::Commits, "commits"),
            (HistoryView::Reflog, "reflog"),
            (HistoryView::Graph, "history"),
        ];
        for (view, own) in sections {
            let text = pane_text(view, 92);
            assert!(text.contains(own), "{} missing {own:?}", view.label());
            for (other_view, other) in sections {
                if other_view == view || other == own {
                    continue;
                }
                // The tab strip names every view, so match the section label
                // rather than the word alone.
                assert!(
                    !text.contains(&format!(" {other}\n")) || other == own,
                    "{} leaked the {other:?} section",
                    view.label()
                );
            }
        }
    }

    /// `info` is a glance: the newest few with their diffstats, capped even
    /// when the loaded list runs to hundreds. `commits` is the whole thing.
    #[test]
    fn info_shows_a_glance_and_commits_shows_everything() {
        let mut r = busy_repo();
        r.commits = (0..30)
            .map(|i| crate::model::Commit {
                hash: format!("c{i:06}"),
                author: "Gaurgle".to_string(),
                date: Some(chrono::Utc::now()),
                message: format!("subject {i}"),
                body: String::new(),
                insertions: 1,
                deletions: 0,
                files: vec![],
            })
            .collect();
        r.graph = (0..30)
            .map(|i| crate::model::GraphLine {
                prefix: "* ".to_string(),
                hash: format!("g{i:06}"),
                is_merge: false,
                refs: vec![],
                author: "Gaurgle".to_string(),
                message: format!("graph {i}"),
            })
            .collect();

        let rendered = |view: HistoryView| {
            use ratatui::Terminal;
            use ratatui::backend::TestBackend;
            let mut app = crate::app::App::new(vec![r.clone()], "t".to_string(), 96, 44);
            app.history_view = view;
            let mut term = Terminal::new(TestBackend::new(96, 60)).unwrap();
            term.draw(|f| render_detail_pane(f, f.area(), &app))
                .unwrap();
            let buf = term.backend().buffer().clone();
            (0..60u16)
                .map(|y| (0..96u16).map(|x| buf[(x, y)].symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        };

        let info = rendered(HistoryView::Info);
        assert!(info.contains("c000005"), "the 6th is shown");
        assert!(
            !info.contains("c000006"),
            "the 7th is not: {RECENT_COMMIT_COUNT} max"
        );

        let commits = rendered(HistoryView::Commits);
        assert!(commits.contains("g000000") && commits.contains("g000020"));
        assert!(!commits.contains("c000000"), "commits draws the graph list");
    }

    /// Every list bolds its selection, so the pane always says which commit
    /// the inspector is showing. Only the commit list did before.
    #[test]
    fn every_list_view_bolds_its_selection() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        for (view, hash) in [
            (HistoryView::Info, "a1b2c3d"),
            (HistoryView::Commits, "a1b2c3d"),
            (HistoryView::Reflog, "a1b2c3d"),
            (HistoryView::Graph, "a1b2c3d"),
        ] {
            let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 96, 44);
            app.history_view = view;
            app.set_detail_selected(0);
            let mut term = Terminal::new(TestBackend::new(96, 40)).unwrap();
            term.draw(|f| render_detail_pane(f, f.area(), &app))
                .unwrap();
            let buf = term.backend().buffer().clone();

            let mut found_bold = false;
            for y in 0..40u16 {
                let row: String = (0..96u16).map(|x| buf[(x, y)].symbol()).collect();
                if let Some(col) = row.find(hash)
                    && buf[(col as u16, y)].modifier.contains(Modifier::BOLD)
                {
                    found_bold = true;
                }
            }
            assert!(found_bold, "{} does not bold its selection", view.label());
        }
    }

    /// The expanded view reproduces `git log --graph`, so its node stays `*`.
    /// That format spells merges out on their own `Merge:` line, and swapping
    /// the glyph would only stop it looking like the thing it is copying.
    #[test]
    fn the_expanded_graph_keeps_gits_own_node() {
        let text: String = expanded_graph_line("* commit a1b2c3d", "", 80)
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(text.contains("* commit"), "got {text:?}");
        assert!(!text.contains(COMMIT_NODE), "no substitution here");

        // The compact list is the opposite: that is where the swap belongs.
        let compact: String = graph_prefix_spans("* ", false)
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(compact, "⚬ ");
    }

    /// The header and the switcher are what tell you where you are, so they
    /// must survive every view and every scroll.
    #[test]
    fn the_header_and_tab_strip_are_in_every_view() {
        for view in HistoryView::ALL {
            let text = pane_text(view, 92);
            assert!(text.contains("~/repos/fleetz"), "{}", view.label());
            assert!(text.contains("synced"), "{}", view.label());
            for tab in HistoryView::ALL {
                assert!(
                    text.contains(tab.label()),
                    "{} is missing the {} tab",
                    view.label(),
                    tab.label()
                );
            }
        }
    }

    /// The inspector follows whichever list is active, resolving the hash
    /// rather than an index -- the reflog and the commit list are different
    /// lists of different lengths, so an index into one means nothing in the
    /// other.
    #[test]
    fn the_inspector_follows_the_active_view() {
        for view in [
            HistoryView::Commits,
            HistoryView::Reflog,
            HistoryView::Graph,
        ] {
            let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 92, 44);
            app.history_view = view;
            assert_eq!(
                app.inspected_hash(),
                Some("a1b2c3d"),
                "{} did not reach the commit",
                view.label()
            );
            assert_eq!(
                app.inspected_commit().map(|c| c.message.as_str()),
                Some("feat: a thing"),
                "{}",
                view.label()
            );
        }
    }

    /// Every view has a selection of its own now, `info` included -- it lists
    /// the newest commits rather than nothing.
    #[test]
    fn every_view_including_info_resolves_its_own_selection() {
        for view in HistoryView::ALL {
            let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 92, 44);
            app.history_view = view;
            assert_eq!(
                app.inspected_hash(),
                Some("a1b2c3d"),
                "{} resolved nothing",
                view.label()
            );
        }
    }

    /// A reflog entry can name a commit older than the loaded list. Saying
    /// "no commit selected" there would be a lie: something is selected, it
    /// just is not in the window.
    #[test]
    fn a_selection_outside_the_loaded_range_is_not_reported_as_none() {
        let mut r = busy_repo();
        r.reflog[0].hash = "deadbee".to_string();
        let mut app = crate::app::App::new(vec![r], "t".to_string(), 92, 44);
        app.history_view = HistoryView::Reflog;
        assert_eq!(app.inspected_hash(), Some("deadbee"), "still a selection");
        assert!(app.inspected_commit().is_none(), "but not a loaded commit");
    }

    /// The inspector below shows one commit; the list has to say which.    /// The inspector below shows one commit; the list has to say which. The
    /// ▸ marker lives in its own fixed-width column, so on a long list the
    /// eye has to travel to it -- bold puts the weight on the hash itself.
    #[test]
    fn the_selected_commits_hash_is_bold() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut r = cloned_repo();
        r.commits = (0..3)
            .map(|i| crate::model::Commit {
                hash: format!("abc123{i}"),
                author: "Gaurgle".to_string(),
                date: Some(chrono::Utc::now()),
                message: format!("commit {i}"),
                body: String::new(),
                insertions: 0,
                deletions: 0,
                files: vec![],
            })
            .collect();
        let mut app = crate::app::App::new(vec![r], "t".to_string(), 100, 44);
        // `info` is where the diffstat list lives; `commits` is the graph.
        app.history_view = HistoryView::Info;
        app.set_detail_selected(1);

        let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();

        // Only the list rows, which are the ones carrying the dot fill. The
        // pinned readout above the body repeats the selected hash, and
        // counting that as a list row makes the test pass for the wrong
        // reason -- the readout has always been styled separately.
        let mut weights = Vec::new();
        for y in 0..30u16 {
            let row: String = (0..100u16).map(|x| buf[(x, y)].symbol()).collect();
            for i in 0..3 {
                // A list row carries both the hash and its message. The
                // pinned readout above the body repeats the selected hash on
                // its own (`[2/3 · abc1231]`), and counting that as a list
                // row makes this pass for the wrong reason: the readout has
                // always been styled separately from the list.
                if !row.contains(&format!("commit {i}")) {
                    continue;
                }
                if let Some(col) = row.find(&format!("abc123{i}")) {
                    weights.push((i, buf[(col as u16, y)].modifier.contains(Modifier::BOLD)));
                }
            }
        }
        assert_eq!(weights.len(), 3, "one list row per commit: {weights:?}");
        for (i, bold) in weights {
            assert_eq!(bold, i == 1, "hash {i} boldness wrong");
        }
    }

    /// Stashes were drawn in OVERLAY, the colour used for paths and other
    /// furniture, which made a real fact read as something already dismissed.
    /// They also carry the repo list's glyph now, so the same symbol means
    /// the same thing in both panes.
    #[test]
    fn the_stash_count_is_findable_in_the_repo_pane() {
        let mut r = cloned_repo();
        r.stash_count = 3;
        let (_, _, facts) = header_rows(r, 100);
        assert!(facts.contains("3 stashes"), "got {facts:?}");
        assert!(
            facts.contains(badges::STASH_GLYPH),
            "the repo list's glyph is missing: {facts:?}"
        );
    }

    /// Grey said "shown because we must, not because you want it", which is
    /// right for a path and wrong for a stash. The badge and the pane have to
    /// pick the same colour, or the two disagree about what a stash looks
    /// like the moment either is touched.
    #[test]
    fn stashes_are_not_drawn_as_furniture() {
        for grey in [colors::OVERLAY, colors::SUBTEXT, colors::SURFACE] {
            assert_ne!(colors::STASH_CLR, grey, "stash is news, not furniture");
        }
        assert_ne!(colors::STASH_CLR, colors::RED, "nor a failing build");
        assert_ne!(colors::STASH_CLR, colors::YELLOW, "nor uncommitted work");
    }

    /// CI was named in the pane frame and again on the header's remote row,
    /// in the same words one line apart. The frame gave way: the row groups
    /// it with the PRs, which is the other half of what GitHub knows.
    #[test]
    fn ci_is_named_once_not_in_the_frame_as_well() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut r = cloned_repo();
        r.ci_status = CiStatus::Passed;
        let app = crate::app::App::new(vec![r], "t".to_string(), 92, 44);
        let mut term = Terminal::new(TestBackend::new(92, 20)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        let rows: Vec<String> = (0..20u16)
            .map(|y| (0..92u16).map(|x| buf[(x, y)].symbol()).collect())
            .collect();

        assert!(
            !rows[0].contains("CI"),
            "frame should not name CI: {:?}",
            rows[0]
        );
        assert!(
            rows.iter().any(|r| r.contains("CI ◉ passed")),
            "but the header row still should"
        );
    }

    /// This pane is the repo's main window, so an open PR belongs on its top
    /// line beside the sync state, not only in a section three groups down
    /// that a default run does not even render.
    #[test]
    fn the_status_line_says_when_a_repo_has_open_prs() {
        let facts = |n: u32, numbers: Vec<u32>| {
            let mut r = cloned_repo();
            r.pr_count = n;
            r.prs = numbers.into_iter().map(pr_numbered).collect();
            header_rows(r, 120).2
        };

        // Flush right, on the remote half of the fact row.
        let two = facts(2, vec![13, 9]);
        assert!(two.contains("2 open PRs"), "got {two:?}");
        assert!(
            two.contains("#13") && two.contains("#9"),
            "numbers: {two:?}"
        );

        let one = facts(1, vec![13]);
        assert!(one.contains("1 open PR #13"), "singular: {one:?}");
        assert!(!one.contains("1 open PRs"), "got {one:?}");

        // More than fit are summarised rather than pushing CI off the row.
        let many = facts(5, vec![13, 9, 7, 4, 2]);
        assert!(many.contains("5 open PRs #13 #9+"), "got {many:?}");

        let none = facts(0, vec![]);
        assert!(!none.contains("open PR"), "silent when there are none");
    }

    /// One colour per concept: the badge, the footer count, the PR number in
    /// the list, and the `#prs` filter tag must not drift apart. They used to
    /// share MAUVE with section labels, which made a PR count read as
    /// furniture instead of as news.
    #[test]
    fn every_pr_surface_uses_the_same_colour() {
        assert_eq!(
            crate::ui::filter_color(crate::app::FilterKind::HasPrs),
            colors::PR_CLR,
            "the filter tag must match the badge"
        );
        assert_ne!(colors::PR_CLR, colors::MAUVE, "PRs are not a section label");
        assert_ne!(colors::PR_CLR, colors::PEACH, "nor a sync counter");
    }

    /// `refs_width` budgets the message column and `ref_spans` draws the
    /// refs. Two pieces of arithmetic over the same thing: when they drift,
    /// the message is truncated to the wrong width and nothing fails.
    #[test]
    fn the_measured_ref_width_matches_what_gets_drawn() {
        let cases: Vec<Vec<GitRef>> = vec![
            vec![],
            vec![git_ref(RefKind::Local, "main", true)],
            vec![git_ref(RefKind::Head, "HEAD", false)],
            vec![
                git_ref(RefKind::Local, "main", true),
                git_ref(RefKind::Remote, "origin/main", false),
                git_ref(RefKind::Tag, "v1.0", false),
            ],
        ];
        for refs in cases {
            let line = crate::model::GraphLine {
                prefix: "* ".to_string(),
                hash: "a1b2c3d".to_string(),
                is_merge: false,
                refs: refs.clone(),
                author: "Alice".to_string(),
                message: "feat: thing".to_string(),
            };
            let drawn = span_text(&ref_spans(&refs));
            // refs_width excludes the leading space; ref_spans emits it.
            let measured = if refs.is_empty() {
                0
            } else {
                line.refs_width() + 1
            };
            assert_eq!(drawn.chars().count(), measured, "width drift on {drawn:?}");
        }
    }

    #[test]
    fn an_attached_head_draws_the_arrow_and_a_detached_one_does_not() {
        let attached = span_text(&ref_spans(&[
            git_ref(RefKind::Local, "main", true),
            git_ref(RefKind::Remote, "origin/main", false),
        ]));
        assert_eq!(attached, " (HEAD -> main, origin/main)");

        // Detached HEAD sitting on a tag: git separates these with a comma,
        // not an arrow. HEAD does not point at the tag.
        let detached = span_text(&ref_spans(&[
            git_ref(RefKind::Head, "HEAD", false),
            git_ref(RefKind::Tag, "v1.0", false),
        ]));
        assert_eq!(detached, " (HEAD, v1.0)");
    }

    /// Regression: the whole `(...)` blob was drawn in one flat mauve, so a
    /// local branch and its remote counterpart were indistinguishable.
    #[test]
    fn each_ref_kind_gets_its_own_colour() {
        let kinds = [RefKind::Head, RefKind::Local, RefKind::Remote, RefKind::Tag];
        let seen: std::collections::HashSet<_> =
            kinds.iter().map(|k| colors::ref_color(*k)).collect();
        assert_eq!(seen.len(), kinds.len(), "two kinds share a colour");
    }

    /// A hash is a hash in every pane, but `YELLOW` already means
    /// "uncommitted" in the badges and the footer. The gold has to be its own
    /// colour, not a second meaning stacked on that one.
    #[test]
    fn the_hash_gold_is_not_the_uncommitted_yellow() {
        assert_ne!(colors::HASH_CLR, colors::YELLOW);
    }

    fn repo(name: &str, kind: RepoKind, owner: Option<&str>) -> RepoStatus {
        RepoStatus {
            daily: None,
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
            graph_expanded: Vec::new(),
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
    /// Row 3 is the status line, row 4 the remote-URL line. Rows 0 and 1 are
    /// the border and the block's top padding, and row 2 is the pinned
    /// selected-commit readout, which sits outside the scrolling body. The
    /// border and padding are stripped so assertions are about content, not
    /// the frame around it.
    fn pane_rows(repo: RepoStatus, width: u16) -> (String, String) {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let app = crate::app::App::new(vec![repo], "test".to_string(), width, 40);
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
        (row(3), row(4))
    }

    /// The header's three rows: state, location, and the local/remote fact
    /// row beneath them.
    fn header_rows(repo: RepoStatus, width: u16) -> (String, String, String) {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let app = crate::app::App::new(vec![repo], "test".to_string(), width, 40);
        let mut terminal = Terminal::new(TestBackend::new(width, 14)).unwrap();
        terminal
            .draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let row = |y: u16| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_matches('│')
                .trim_end()
                .to_string()
        };
        (row(3), row(4), row(5))
    }

    fn pr_numbered(number: u32) -> crate::model::PullRequest {
        crate::model::PullRequest {
            number,
            title: format!("pr {number}"),
            author: "Gaurgle".to_string(),
            created_at: chrono::Utc::now(),
            url: format!("https://github.com/Gaurgle/fleetz/pull/{number}"),
            review_status: crate::model::ReviewStatus::Unknown,
        }
    }

    fn cloned_repo() -> RepoStatus {
        let mut r = repo("fleetz", RepoKind::Public, Some("Gaurgle"));
        r.path = Some("~/repos/fleetz".to_string());
        r.remote_url = Some("git@github.com:Gaurgle/fleetz.git".to_string());
        r
    }

    /// The pinned readout row (row 2), border stripped.
    fn readout_row(repo: RepoStatus, selected: usize, width: u16) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::app::App::new(vec![repo], "test".to_string(), width, 40);
        app.set_detail_selected(selected);
        let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
        terminal
            .draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..width)
            .map(|x| buffer[(x, 2)].symbol())
            .collect::<String>()
            .trim_matches('│')
            .trim_end()
            .to_string()
    }

    fn repo_with_commits(n: usize) -> RepoStatus {
        let mut r = cloned_repo();
        r.commits = (0..n)
            .map(|i| crate::model::Commit {
                hash: format!("{i}abc123"),
                message: format!("commit {i}"),
                author: "Gaurgle".to_string(),
                date: None,
                body: String::new(),
                insertions: 0,
                deletions: 0,
                files: vec![],
            })
            .collect();
        r
    }

    #[test]
    fn readout_names_the_selected_commit_and_its_position() {
        // The point of the row: with the commit list scrolled off the top,
        // the hash alone says which commit but not where in the list it sits.
        // Counted against what the view actually lists: `info` shows the
        // newest few, so ten loaded commits still navigate as six.
        let row = readout_row(repo_with_commits(10), 3, 60);
        assert!(
            row.contains(&format!("4/{RECENT_COMMIT_COUNT}")),
            "ordinal missing: {row:?}"
        );
        assert!(row.contains("3abc123"), "hash missing: {row:?}");
    }

    #[test]
    fn readout_ends_in_the_same_column_as_the_owner_and_name_rows() {
        // The alignment that matters: the readout, the owner, and the repo
        // name all sit flush against the pane's inner right edge, so they read
        // as one right-hand column. A trailing space in any of them parks that
        // row one cell short, which is exactly the bug this guards.
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let width = 60;
        let mut app =
            crate::app::App::new(vec![repo_with_commits(4)], "test".to_string(), width, 40);
        app.set_detail_selected(1);
        let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
        terminal
            .draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        // Trim only the border, never trailing spaces: the whole point is
        // where each row's last non-blank cell lands.
        let end_col = |y: u16| {
            (0..width)
                .rfind(|x| buffer[(*x, y)].symbol().trim() != "" && buffer[(*x, y)].symbol() != "│")
        };

        let readout = end_col(2).expect("readout row is drawn");
        let owner = end_col(3).expect("owner row is drawn");
        let name = end_col(4).expect("name row is drawn");
        assert_eq!(readout, owner, "readout does not align with the owner");
        assert_eq!(owner, name, "owner does not align with the repo name");
    }

    #[test]
    fn readout_is_blank_for_a_repo_with_no_commits() {
        // Blank, not collapsed: the body below must not shift up a row.
        let row = readout_row(cloned_repo(), 0, 60);
        assert_eq!(row, "");
    }

    #[test]
    fn readout_survives_a_selection_past_the_end() {
        // detail_selected is reset when the repo changes, but a stale index
        // must clamp rather than panic on the unwrap path.
        let row = readout_row(repo_with_commits(2), 99, 60);
        assert!(row.contains("2/2"), "should clamp to the last: {row:?}");
    }

    #[test]
    /// Local on the left of every header row, remote flush right. The path
    /// used to share row one with the sync status, which read as "how it is
    /// doing" and "where it lives" run together.
    fn owner_and_name_stack_flush_to_the_right_edge() {
        let (state, location, _) = header_rows(cloned_repo(), 80);

        // Row one is state only, with the owner flush right.
        assert!(!state.contains("~/repos"), "path left row one: {state:?}");
        assert!(
            state.ends_with("Gaurgle"),
            "owner not flush right: {state:?}"
        );

        // Row two is location: local path left, remote URL right.
        assert!(
            location.trim_start().starts_with("~/repos/fleetz"),
            "local path not on the left: {location:?}"
        );
        assert!(
            location.ends_with("git@github.com:Gaurgle/fleetz.git"),
            "remote URL not flush right: {location:?}"
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
    /// On a narrow pane each row sheds its own remote half rather than
    /// truncating it. The local half always survives: it is the half you
    /// cannot look up anywhere else.
    fn identity_is_dropped_whole_on_a_narrow_pane() {
        for width in [24u16, 30, 34, 48, 80] {
            let (state, location, _) = header_rows(cloned_repo(), width);
            // The local half always survives: it is the half you cannot look
            // up anywhere else.
            assert!(
                location.contains("~/repos/fleetz"),
                "w{width}: local path lost: {location:?}"
            );
            // A remote half is either complete and flush right, or absent.
            // Never a truncated fragment.
            assert!(
                !state.contains("Gaurgle") || state.ends_with("Gaurgle"),
                "w{width}: owner truncated: {state:?}"
            );
            assert!(
                !location.contains("github.com")
                    || location.ends_with("git@github.com:Gaurgle/fleetz.git"),
                "w{width}: URL truncated: {location:?}"
            );
        }
        // And at a width that cannot hold it, it really is gone.
        let (_, location, _) = header_rows(cloned_repo(), 34);
        assert!(
            !location.contains("github.com"),
            "URL should have been dropped: {location:?}"
        );
    }
}
