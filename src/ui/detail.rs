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

    // The compact graph list: git's topology in a column, then hash, author,
    // decorations and subject. `info` shows the newest few as a glance;
    // `commits` shows the lot.
    // `info` shows the newest few, `commits` the lot, through one renderer so
    // the two cannot drift into drawing the same rows differently.
    let mut selected_row: Option<usize> = None;
    if matches!(view, HistoryView::Info | HistoryView::Commits) && !repo.graph.is_empty() {
        let limit = (view == HistoryView::Info).then_some(RECENT_COMMIT_COUNT);
        lines.push(Line::from(section_label(match view {
            HistoryView::Info => "recent commits",
            _ => "commits",
        })));
        lines.extend(graph_list_lines(
            repo,
            width,
            app.detail_selected(),
            limit,
            &mut selected_row,
        ));
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
            // The block the selection names starts here, so scrolling to it
            // puts the whole commit on screen rather than its last line.
            if !selected_hash.is_empty()
                && raw
                    .split_whitespace()
                    .skip_while(|w| *w != "commit")
                    .nth(1)
                    .is_some_and(|h| {
                        h.trim_end_matches(|c: char| !c.is_alphanumeric()) == selected_hash
                    })
            {
                selected_row = Some(lines.len());
            }
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

            if ri == app.detail_selected() {
                selected_row = Some(lines.len());
            }
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
    let mut scroll = app.detail_scroll().min(max_scroll);

    // Follow the cursor. This is the only place that knows which row the
    // selection landed on, since the row depends on how many sections were
    // drawn above it.
    //
    // Without it the cursor walked rows that were off screen and the pane sat
    // still, so `j` looked like it did nothing at all -- survivable at six
    // commits, and the whole view at two hundred.
    if app.follow_selection.get()
        && let Some(row) = selected_row
    {
        let row = row as u16;
        // The expanded view anchors the selected commit to the top of the
        // pane rather than merely bringing it on screen: its blocks run to
        // twenty lines, and a header scrolled to the last visible row leaves
        // the body it heads entirely below the fold.
        let anchor_top = view == HistoryView::Graph || row < scroll;
        scroll = if anchor_top {
            row
        } else if row >= scroll + area.height {
            row + 1 - area.height
        } else {
            scroll
        };
        scroll = scroll.min(max_scroll);
        app.set_detail_scroll(scroll);
        app.follow_selection.set(false);
    }

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

/// Commits shown in the `info` view: a glance, not a history. `commits`
/// carries the complete list.
const RECENT_COMMIT_COUNT: usize = 6;

/// The compact graph list, shared by `info` and `commits` so the two cannot
/// drift into rendering the same rows differently.
///
/// `limit` caps the rows drawn, which is also the number of commits: the graph
/// is computed from parent hashes, so a fan-out happens on the merge commit's
/// own row rather than on a connector row of its own.
fn graph_list_lines(
    repo: &RepoStatus,
    width: usize,
    selected: usize,
    limit: Option<usize>,
    selected_row: &mut Option<usize>,
) -> Vec<Line<'static>> {
    // Lanes are two columns each. Padded to the widest row rather than left
    // ragged the way lazygit leaves it: a dashboard is read by scanning down a
    // column, and a subject that starts two columns further right on the rows
    // that happen to sit under a merge defeats that.
    // Lanes are two columns each, padded to a common width so the subject
    // column lines up down the pane rather than running ragged the way lazygit
    // leaves it: a dashboard is read by scanning a column, and a subject that
    // starts further right on whichever rows happen to sit under a merge
    // defeats that.
    //
    // Capped, though, and the measurement is what forced the cap. A long
    // PR-merge history puts many lanes in flight for a handful of rows -- pinz
    // reaches twenty over two hundred commits -- and padding the other hundred
    // and ninety to that spends a third of the pane on whitespace. Past the cap
    // a row keeps its whole graph and gives up subject width instead, which is
    // the honest trade: that row really does have twenty branches in it.
    let widest = 2 * repo.graph.iter().map(|g| g.lanes.len()).max().unwrap_or(0);
    let lane_w = match widest {
        0 => 0,
        // Rounded down to a whole cell, and never narrower than one.
        w => w.min(((width / 4) & !1).max(2)),
    };
    let author_w = crate::ui::AUTHOR_W;
    let hash_w = repo
        .graph
        .iter()
        .map(|g| g.hash.chars().count())
        .max()
        .unwrap_or(7);
    let mut lines = Vec::new();

    // One row per commit, so the row index and the commit index are the same
    // number. They were not while the graph came from `git log --graph`: a
    // fan-out or a collapse got a row of its own carrying no commit, and the
    // two had to be counted separately.
    for (row, g) in repo.graph.iter().enumerate() {
        if limit.is_some_and(|n| row >= n) {
            break;
        }
        let is_sel = row == selected;
        if is_sel {
            *selected_row = Some(row);
        }

        // lazygit's column order: who and what first, then the topology, then
        // the subject. The graph is context for the row, not its identity, so
        // leading with it pushed the two columns you actually read rightwards
        // by however wide the branching happened to be.
        //
        // A row deeper than the cap sets its own width, so the budget below
        // measures what the row will really draw rather than the cap, which
        // would overrun the pane.
        let row_lane_w = lane_w.max(2 * g.lanes.len());
        let lane_pad = " ".repeat(row_lane_w - 2 * g.lanes.len());

        let author = crate::ui::author_code(&g.author);
        let author_pad = " ".repeat(author_w.saturating_sub(author.chars().count()));
        let hash_pad = " ".repeat(hash_w.saturating_sub(g.hash.chars().count()));
        // Decorations are dropped whole on a narrow pane rather than
        // truncated, and the subject keeps a floor: a row that spends its
        // width naming three remotes and then shows nine characters of what
        // was done has the priority backwards. A branch name is recoverable
        // from the branches view; the subject is why you are reading the row.
        let fixed = MARKER_W + hash_w + 1 + author_w + 1 + row_lane_w;
        let refs_budget = width.saturating_sub(fixed + MIN_MESSAGE_W);
        let refs_len = match g.refs.is_empty() {
            true => 0,
            false => 1 + g.refs_width(),
        };
        let show_refs = refs_len > 0 && refs_len <= refs_budget;
        let refs_len = if show_refs { refs_len } else { 0 };
        // The churn, looked up by hash: the graph rows and the commit list are
        // separate probes, so a row deeper than the commit window simply has
        // none rather than showing a wrong one.
        let stat = repo
            .commits
            .iter()
            .find(|c| c.hash == g.hash)
            .map(commit_stat_text)
            .unwrap_or_default();
        let stat_len = stat.chars().count();
        let right = if stat_len > 0 { 1 + stat_len } else { 0 };
        let used = fixed + refs_len;
        // No floor here. `show_refs` above already guarantees the subject
        // `MIN_MESSAGE_W` columns by dropping the decorations that would have
        // taken them, so a floor on top of that only fires when the row is out
        // of width for reasons no floor can fix -- a graph twenty lanes deep --
        // and forcing one there pushed the row past the pane edge and wrapped
        // it, shifting every row below.
        let msg_budget = width.saturating_sub(used + 2 + right);
        let message = crate::ui::truncate(&g.message, msg_budget);
        let msg_len = message.chars().count();
        // Two spaces are spent inside the row -- before the message and before
        // the dots -- and `right` already carries its own leading one.
        // Not `.max(1)`: a row whose refs and subject already fill the width
        // has no room for a fill, and forcing one is what pushed the tail
        // onto a line of its own.
        let dot_count = width.saturating_sub(used + msg_len + 2 + right);

        // A pointer as well as the wash: the band says which row, the caret
        // survives a terminal that renders backgrounds poorly.
        let mut row = vec![Span::styled(
            if is_sel { "▸ " } else { "  " },
            Style::default().fg(colors::SAPPHIRE),
        )];
        row.push(Span::styled(g.hash.clone(), hash_style(is_sel)));
        row.push(Span::raw(hash_pad));
        row.push(Span::raw(" "));
        row.push(Span::styled(
            author,
            Style::default().fg(colors::author_color(&g.author)),
        ));
        row.push(Span::raw(author_pad));
        row.push(Span::raw(" "));
        row.extend(lane_spans(&g.lanes));
        row.push(Span::raw(lane_pad));
        if show_refs {
            row.extend(ref_spans(&g.refs));
        }
        row.push(Span::raw(" "));
        row.push(Span::styled(message, Style::default().fg(colors::TEXT)));
        if dot_count > 0 {
            row.push(Span::raw(" "));
            row.push(Span::styled(
                "·".repeat(dot_count),
                Style::default().fg(colors::SURFACE),
            ));
        }
        if stat_len > 0 {
            row.push(Span::raw(" "));
            row.extend(stat_spans(&stat));
        }
        if is_sel {
            row = wash(row);
        }
        lines.push(Line::from(row));
    }
    lines
}

/// Width of the selection marker column, held open on every row so the rows
/// do not shift sideways as the cursor moves.
const MARKER_W: usize = 2;

/// Columns the subject keeps whatever the decorations want.
///
/// The one lever that yields to it is the ref list, which is dropped whole
/// rather than truncated: a branch name is recoverable from the branches view,
/// while the subject is why you are reading the row. It is not a floor on the
/// rendered message -- see the budget in `graph_list_lines`.
const MIN_MESSAGE_W: usize = 24;

/// Paint a whole row as selected, keeping each span's own foreground.
///
/// The band has to cover the row rather than the text, so the caller pads to
/// the pane width before this: a wash that stops where the message stops
/// reads as a highlight on the words, not a cursor on the line.
fn wash(spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    spans
        .into_iter()
        .map(|s| {
            let style = s.style.bg(colors::SELECTED_BG);
            Span::styled(s.content, style)
        })
        .collect()
}

/// A row's graph column: two spans per cell, since a cell's connector often
/// belongs to a different branch than its node and so carries its own colour.
fn lane_spans(cells: &[crate::graph::Cell]) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(cells.len() * 2);
    for cell in cells {
        spans.push(Span::styled(
            cell.glyph.to_string(),
            Style::default().fg(colors::graph_lane_color(cell.style as usize)),
        ));
        spans.push(Span::styled(
            cell.connector.to_string(),
            Style::default().fg(colors::graph_lane_color(cell.connector_style as usize)),
        ));
    }
    spans
}

/// `git log --graph`'s own ASCII prefix, coloured per column.
///
/// Only the expanded view still needs this: it reproduces git's block layout
/// verbatim, so it gets git's drawing too. The compact list computes its own
/// (`lane_spans`).
fn graph_prefix_spans_raw(prefix: &str) -> Vec<Span<'static>> {
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

/// `+12 -3` split so additions and deletions keep their own colours.
fn stat_spans(stat: &str) -> Vec<Span<'static>> {
    // No trailing space on the last part: the caller measured `stat` and
    // budgeted the row against exactly that width, so an extra column here
    // pushes the diffstat onto a line of its own.
    let parts: Vec<&str> = stat.split_whitespace().collect();
    let last = parts.len().saturating_sub(1);
    parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            let color = if part.starts_with('+') {
                colors::DIFF_ADD
            } else {
                colors::DIFF_DEL
            };
            let text = if i == last {
                (*part).to_string()
            } else {
                format!("{part} ")
            };
            Span::styled(text, Style::default().fg(color))
        })
        .collect()
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
    spans.extend(graph_prefix_spans_raw(prefix));

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
    use crate::graph::{COMMIT_NODE, MERGE_NODE};
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
    /// git draws every commit as `*` and its lanes as punctuation. Both are
    /// translated: the lanes to box drawing, so the column reads as a drawn
    /// line, and the node to a circle -- with merges keeping the asterisk,
    /// circled, so they stay pickable out of the column.
    #[test]
    fn the_graph_is_drawn_with_box_characters_not_ascii() {
        // A merge and a collapse, so every shape the column can draw appears.
        let spec: [(&str, &[&str]); 4] = [
            ("aaaaaaa", &["bbbbbbb", "ccccccc"]),
            ("ccccccc", &["ddddddd"]),
            ("bbbbbbb", &["ddddddd"]),
            ("ddddddd", &[]),
        ];
        let parents: Vec<Vec<String>> = spec
            .iter()
            .map(|(_, p)| p.iter().map(|s| (*s).to_string()).collect())
            .collect();
        let commits: Vec<crate::graph::Commit<'_>> = spec
            .iter()
            .zip(&parents)
            .map(|((hash, _), parents)| crate::graph::Commit { hash, parents })
            .collect();

        let mut r = busy_repo();
        r.graph = crate::graph::layout(&commits)
            .into_iter()
            .zip(spec)
            .map(|(lanes, (hash, parents))| crate::model::GraphLine {
                lanes,
                hash: hash.to_string(),
                is_merge: parents.len() > 1,
                refs: vec![],
                author: "Gaurgle".to_string(),
                message: "subject".to_string(),
            })
            .collect();
        let mut selected_row = None;
        let text: String = graph_list_lines(&r, 96, 0, None, &mut selected_row)
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        for drawn in [COMMIT_NODE, MERGE_NODE, '│', '─', '╮', '╯'] {
            assert!(text.contains(drawn), "{drawn} missing from:\n{text}");
        }
        for ascii in ['*', '|', '\\', '_'] {
            assert!(!text.contains(ascii), "git's own {ascii} leaked through");
        }
    }

    /// The node is the one glyph here we are free to pick, so it stays Narrow
    /// even though the lanes cannot be. The lanes are safe for a different
    /// reason: every pane border in the app is drawn with the same box
    /// characters already.
    #[test]
    fn both_nodes_occupy_one_column() {
        for node in [COMMIT_NODE, MERGE_NODE] {
            assert_eq!(Span::from(node.to_string()).width(), 1, "{node}");
        }
        for ambiguous in ['●', '○', '•', '◆', '⊙'] {
            assert_ne!(COMMIT_NODE, ambiguous);
            assert_ne!(MERGE_NODE, ambiguous);
        }
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
            lanes: vec![crate::graph::commit_cell()],
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
                lanes: vec![crate::graph::commit_cell()],
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

        // Both draw the same list through one renderer; only the length
        // differs, which is the point of sharing it.
        let info = rendered(HistoryView::Info);
        assert!(info.contains("g000005"), "the 6th is shown");
        assert!(
            !info.contains("g000006"),
            "the 7th is not: {RECENT_COMMIT_COUNT} max"
        );

        let commits = rendered(HistoryView::Commits);
        assert!(commits.contains("g000000"), "commits starts at the newest");
        assert!(commits.contains("g000020"), "and runs past info's cap");
    }

    /// Regression: the cursor walked every row before the pane scrolled at
    /// all. Survivable at six commits and the whole view at two hundred --
    /// `j` moved a cursor that was already off screen, so the pane looked
    /// frozen. It follows the selection now.
    #[test]
    fn the_pane_scrolls_to_keep_the_selection_visible() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut r = busy_repo();
        r.graph = (0..200)
            .map(|i| crate::model::GraphLine {
                lanes: vec![crate::graph::commit_cell()],
                hash: format!("h{i:06}"),
                is_merge: false,
                refs: vec![],
                author: "Gaurgle".to_string(),
                message: format!("subject {i}"),
            })
            .collect();

        let mut app = crate::app::App::new(vec![r], "t".to_string(), 96, 30);
        app.history_view = HistoryView::Commits;

        let draw = |app: &crate::app::App| {
            let mut term = Terminal::new(TestBackend::new(96, 20)).unwrap();
            term.draw(|f| render_detail_pane(f, f.area(), app)).unwrap();
            let buf = term.backend().buffer().clone();
            (0..20u16)
                .map(|y| (0..96u16).map(|x| buf[(x, y)].symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        };

        assert!(draw(&app).contains("h000000"), "opens at the newest");
        assert_eq!(app.detail_scroll(), 0);

        // A selection far below the fold must drag the pane with it.
        app.set_detail_selected(120);
        let shown = draw(&app);
        assert!(shown.contains("h000120"), "selection off screen: {shown:?}");
        assert!(app.detail_scroll() > 0, "the pane did not follow");

        // And back up again.
        app.set_detail_selected(2);
        let shown = draw(&app);
        assert!(shown.contains("h000002"), "did not come back: {shown:?}");
    }

    /// A row must not spill past the pane. The refs and the subject can each
    /// eat the width on their own, so the dot fill has to be allowed to
    /// vanish rather than being forced to one -- that single dot was enough
    /// to push the diffstat onto a line of its own.
    #[test]
    fn no_commit_row_overflows_the_pane() {
        let mut r = busy_repo();
        r.commits = vec![crate::model::Commit {
            hash: "a1b2c3d".to_string(),
            author: "Gaurgle".to_string(),
            date: Some(chrono::Utc::now()),
            message: "x".repeat(200),
            body: String::new(),
            insertions: 4321,
            deletions: 8765,
            files: vec![],
        }];
        r.graph = vec![crate::model::GraphLine {
            lanes: vec![crate::graph::commit_cell()],
            hash: "a1b2c3d".to_string(),
            is_merge: false,
            refs: vec![
                crate::model::GitRef {
                    kind: crate::model::RefKind::Local,
                    name: "a-rather-long-branch-name".to_string(),
                    is_head: true,
                },
                crate::model::GitRef {
                    kind: crate::model::RefKind::Remote,
                    name: "origin/a-rather-long-branch-name".to_string(),
                    is_head: false,
                },
            ],
            author: "Gaurgle".to_string(),
            message: "x".repeat(200),
        }];

        for width in [60usize, 80, 100, 140] {
            let mut row_marker = None;
            let lines = graph_list_lines(&r, width, 0, None, &mut row_marker);
            for line in &lines {
                let drawn: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
                assert!(
                    drawn <= width,
                    "row is {drawn} wide in a {width} pane: {:?}",
                    line.spans
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect::<String>()
                );
            }
        }
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

        // The compact list is the opposite: it computes its own topology and
        // draws it in box characters. See
        // `the_graph_is_drawn_with_box_characters_not_ascii`.
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
        r.graph = (0..3)
            .map(|i| crate::model::GraphLine {
                lanes: vec![crate::graph::commit_cell()],
                hash: format!("abc123{i}"),
                is_merge: false,
                refs: vec![],
                author: "Gaurgle".to_string(),
                message: format!("commit {i}"),
            })
            .collect();
        let mut app = crate::app::App::new(vec![r], "t".to_string(), 100, 44);
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
                lanes: vec![crate::graph::commit_cell()],
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

    /// Regression class this pane has been bitten by once already: a row that
    /// budgets its width against something other than what it draws wraps, and
    /// a wrapped row silently shifts every row beneath it.
    ///
    /// The graph column is where that can happen now, because it is padded to a
    /// cap rather than to the widest row. A history deep enough to blow past
    /// the cap has to give up subject width, not spill.
    #[test]
    fn a_history_deeper_than_the_cap_still_fits_the_pane() {
        // Every commit a merge whose second parent is never listed, so lanes
        // pile up and none is ever reclaimed.
        let hashes: Vec<String> = (0..24).map(|i| format!("h{i:06}")).collect();
        let parents: Vec<Vec<String>> = (0..24)
            .map(|i| {
                let mut p: Vec<String> = hashes.get(i + 1).cloned().into_iter().collect();
                p.push(format!("never{i:03}"));
                p
            })
            .collect();
        let commits: Vec<crate::graph::Commit<'_>> = hashes
            .iter()
            .zip(&parents)
            .map(|(hash, parents)| crate::graph::Commit { hash, parents })
            .collect();

        let mut r = busy_repo();
        r.graph = crate::graph::layout(&commits)
            .into_iter()
            .zip(&hashes)
            .map(|(lanes, hash)| crate::model::GraphLine {
                lanes,
                hash: hash.clone(),
                is_merge: true,
                refs: vec![],
                author: "Gaurgle".to_string(),
                message: "a subject long enough to want every column it can get".to_string(),
            })
            .collect();

        let deepest = r.graph.last().map(|g| g.lanes.len()).unwrap_or(0);
        assert!(deepest > 12, "fixture is not deep enough: {deepest} lanes");

        for width in [72, 100, 140] {
            let mut selected_row = None;
            for line in graph_list_lines(&r, width, 0, None, &mut selected_row) {
                assert!(
                    line.width() <= width,
                    "a {}-lane row ran {} past a {width}-column pane: {:?}",
                    deepest,
                    line.width() - width,
                    line.spans
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect::<String>()
                );
            }
        }
    }
}
