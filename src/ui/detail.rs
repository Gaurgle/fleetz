use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, List, ListItem, ListState, Padding, Paragraph, Wrap,
};

use crate::app::{App, FocusedPanel, HistoryView, ListRow};
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

/// Rows the panes above keep for themselves before the inspector gets any.
const DETAIL_MIN_HEIGHT: u16 = 8;

/// Split the main area into the row that holds the repo list and detail pane
/// and, when there is room for a useful one, the inspector docked under both.
///
/// Single source of truth for that geometry: `render` draws from it and
/// `ui::panel_rects` hit-tests mouse clicks against it. They used to carry
/// separate copies of the same arithmetic, kept in sync by hand, where any
/// drift would land clicks on the wrong pane.
///
/// The inspector spans the full width rather than the right column alone: a
/// commit's message and its file list both want columns, while the repo list
/// is rarely tall and scrolls when it has to be.
///
/// `pct` is the inspector's requested share of the area, which the resize
/// keys move. It is a request, not a promise: the min/max clamp and the row
/// above's floor both outrank it, so a short terminal still yields a readable
/// split (or drops the inspector entirely) whatever the user asked for.
///
/// `folded` overrides `pct` with the single title row a folded pane keeps.
pub fn split_inspector(
    area: Rect,
    pct: u16,
    show_inspector: bool,
    folded: bool,
) -> (Rect, Option<Rect>) {
    let min_height = if folded {
        crate::ui::FOLDED_HEIGHT
    } else {
        INSPECTOR_MIN_HEIGHT
    };
    // The graph view gives the whole column to the history: its blocks
    // already say what an inspector would.
    if !show_inspector || area.height < DETAIL_MIN_HEIGHT + min_height {
        return (area, None);
    }
    let height = if folded {
        crate::ui::FOLDED_HEIGHT
    } else {
        (area.height * pct / 100)
            .clamp(INSPECTOR_MIN_HEIGHT, INSPECTOR_MAX_HEIGHT)
            .min(area.height - DETAIL_MIN_HEIGHT)
    };
    let v = Layout::vertical([Constraint::Min(1), Constraint::Length(height)]).split(area);
    (v[0], Some(v[1]))
}

/// Split the main area into the repo list and the right column. Shared by
/// `render` and `ui::panel_rects` for the same reason as the vertical split.
pub fn split_main(area: Rect, repo_list_pct: u16) -> (Rect, Rect) {
    // A percentage of a very wide terminal is more room than a repo row can
    // use: the row is a name and a handful of badges, and everything past
    // about fifty columns is leader dots. The right column has the opposite
    // appetite -- commit subjects grow into whatever they are given -- so the
    // surplus goes there.
    //
    // Only the default yields. Once the divider has been dragged the number
    // is the user's answer to this question and is honoured exactly, however
    // wide the terminal.
    let width = repo_list_width(area.width, repo_list_pct);
    let h = Layout::horizontal([Constraint::Length(width), Constraint::Min(0)])
        .spacing(1)
        .split(area);
    (h[0], h[1])
}

/// The repo list width `split_main` draws in a main area `area_width`
/// columns wide, cap included. The resize keys step from it (FZ-69).
pub fn repo_list_width(area_width: u16, repo_list_pct: u16) -> u16 {
    if repo_list_pct == crate::app::DEFAULT_REPO_LIST_PCT {
        (area_width * repo_list_pct / 100).min(REPO_LIST_MAX_COLS)
    } else {
        area_width * repo_list_pct / 100
    }
}

/// Split pane 2's body of the `notes` layout, under its pinned header
/// (`notes_pane_body`), into the file tree and the preview beside it (FZ-62,
/// FZ-67). Shared by `render` and `ui::panel_rects`, as the other splits
/// are, so a click lands in the pane drawn under it.
///
/// The tree takes the repo list's rule within pane 2 (35%, capped at
/// `REPO_LIST_MAX_COLS`): it is a list of names, and the reader is what
/// wants the width. Floored at `notes_tree_floor`, the width that holds the
/// mode line whole, or half the pane when that is less, so a small terminal
/// keeps a reader to read in.
pub fn split_notes_pane(pane: Rect) -> (Rect, Rect) {
    let rule = (pane.width * crate::app::DEFAULT_REPO_LIST_PCT / 100).min(REPO_LIST_MAX_COLS);
    let width = rule.max(notes_tree_floor().min(pane.width / 2));
    let h = Layout::horizontal([Constraint::Length(width), Constraint::Min(0)])
        .spacing(1)
        .split(pane);
    (h[0], h[1])
}

/// The narrowest the `notes` tree is drawn where pane 2 has the room: its
/// mode line in either mode, plus the pane's border and padding on each
/// side. Measured from the line itself, so new words move the floor with
/// them rather than cutting the `(m)` off.
fn notes_tree_floor() -> u16 {
    let line = [false, true]
        .map(|all| notes_mode_line_for(all).width())
        .into_iter()
        .max()
        .unwrap_or(0);
    u16::try_from(line).unwrap_or(u16::MAX) + 2 * crate::ui::PANE_CONTENT_INSET
}

/// Rows pane 2's body keeps under a pinned header, or the header is not
/// pinned. On a short pane the history is what you came for, and a header
/// taking half the height to repeat what the frame's own title already says
/// is the worse trade.
const MIN_BODY_ROWS: u16 = 6;

/// Columns past which the repo list is only drawing longer dots: two borders,
/// two columns of padding, the marker and kind glyph, a generous name, the
/// dot fill, and the widest badge run a row can carry (`1PR 1 3↓ 2↑ 5!`).
const REPO_LIST_MAX_COLS: u16 = 52;

/// PR numbers shown inline on the header's remote row before the rest are
/// summarised with a `+`. Two keeps the row short enough that CI still fits
/// beside it on a normal pane.
const PR_NUMBERS_SHOWN: usize = 2;

/// The detail pane's line under the header for a repo the last GitHub poll
/// did not return (`App::is_not_in_last_poll`): its CI and PRs are kept, not
/// cleared, since a blank lamp would read as "no CI". Provisional words (O4).
const EARLIER_REFRESH_NOTE: &str = "GitHub data from an earlier refresh: not in the last poll";

/// Most changed files listed in the inspector before the rest are collapsed
/// into a "… and N more" line -- keeps a huge commit from turning the pane
/// into an endless scroll.
const INSPECTOR_FILE_CAP: usize = 30;

/// Interior width from which the inspector shows the changed files in a lane
/// beside the message rather than under it. Half of this leaves each lane
/// about fifty columns, enough for a subject line or a stat plus a path.
pub const INSPECTOR_TWO_LANE_WIDTH: u16 = 100;

/// The row a capped list ends on: how many entries it left out. The one
/// overflow marker the panes use (the inspector's file list, the slot
/// digest's lists).
pub fn more_line(hidden: usize) -> Line<'static> {
    Line::from(Span::styled(
        format!("… and {hidden} more"),
        Style::default().fg(colors::OVERLAY),
    ))
}

/// Render the detail view: the repo list on the left (35% of the width by
/// default, capped at `REPO_LIST_MAX_COLS`; see `split_main`) and the detail
/// pane on the right, with the inspector docked across the full width below
/// them.
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
    reader_area: Option<Rect>,
    app: &App,
) {
    render_repo_list(frame, list_area, app);
    render_detail_pane(frame, detail_area, app);
    // The `notes` layout's preview, inside pane 2 beside the tree (FZ-67).
    if let Some(reader_area) = reader_area {
        render_reader(frame, reader_area, app);
    }
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

    let dim_pending = app.watch_mode && !app.blink_on;

    // The rows come from `App::list_rows`, the list the cursor walks and
    // clicks resolve against, so the three cannot disagree about what sits
    // where. While searching it is one ranked set under "results"; otherwise
    // the sections (github activity, local changes, also active), each headed
    // once another is present, which `App::section_rank` orders the cursor to
    // match. The cursor's row, a section header included (FZ-45), is told to
    // ListState so that exact row is kept in view.
    let visible = app.visible_repos();
    let total = visible.len();
    let rows_drawn = app.list_rows();
    let cursor = app.cursor_row(&rows_drawn);
    for (i, row) in rows_drawn.iter().enumerate() {
        let on_cursor = cursor == Some(i);
        items.push(match *row {
            ListRow::Results => section_header_item("results"),
            ListRow::Gap => ListItem::new(Line::from("")),
            ListRow::Header(section) if on_cursor => selected_header_item(section.label()),
            ListRow::Header(section) => section_header_item(section.label()),
            ListRow::Repo(pos) => {
                let repo = visible[pos].1;
                repo_list_item(
                    repo,
                    on_cursor,
                    col_width,
                    dim_pending,
                    app.is_not_in_last_poll(repo),
                    app.search_match_field(repo).and_then(|f| f.mark()),
                )
            }
        });
    }
    let selected_display_idx = cursor;

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
    // The padding keeps the pane 2 margin past the cursor, shrunk by ratatui
    // in a short pane as `scrolloff_for` shrinks it (FZ-55).
    let list = List::new(items).scroll_padding(usize::from(crate::app::SCROLLOFF));
    let mut state = ListState::default();
    state.select(selected_display_idx);
    frame.render_stateful_widget(list, rows[2], &mut state);
    // Where the rows landed, for clicks and hovers (FZ-45).
    app.list_hits.set(crate::app::ListHits {
        area: rows[2],
        offset: state.offset(),
    });
}

/// A section header the list cursor stands on (FZ-45): the marker repo rows
/// wear, and the label lit, so the stop reads as the cursor's.
fn selected_header_item(label: &str) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::raw("▸ "),
        Span::styled(
            label.to_string(),
            Style::default()
                .fg(colors::MAUVE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ──", Style::default().fg(colors::SURFACE)),
    ]))
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
    not_in_last_poll: bool,
    match_mark: Option<&'static str>,
) -> ListItem<'static> {
    ListItem::new(Line::from(repo_row_spans(
        repo,
        is_selected,
        col_width,
        dim_pending,
        not_in_last_poll,
        match_mark,
    )))
}

/// Widest a repo row is drawn: the repo list's column cap less its two
/// borders and two padding columns. The slot digest (FZ-45) draws its rows'
/// shared columns at this width, as pane 1 does at its default size.
pub const REPO_ROW_MAX_W: usize = REPO_LIST_MAX_COLS as usize - 4;

/// A repo row's spans, `col_width` wide: marker, kind glyph, name, dot fill
/// and the badge group flush right. The one grammar a repo row has: the repo
/// list draws it, and so do the slot digest's lines (FZ-45), so every
/// indicator keeps its place and its colour in both.
pub fn repo_row_spans(
    repo: &RepoStatus,
    is_selected: bool,
    col_width: usize,
    dim_pending: bool,
    not_in_last_poll: bool,
    match_mark: Option<&'static str>,
) -> Vec<Span<'static>> {
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
    let (badge_spans, badge_w) = badges::badge_spans_marked(repo, dim_pending, not_in_last_poll);
    let right_len = badge_w + 1; // +1 for space after dots
    // A search hit on the owner or an author says so after the name (FZ-56),
    // in the filter strip's dim, so it does not read as a name match.
    let mark = match_mark.map(|m| format!(" {m}")).unwrap_or_default();
    let mark_w = mark.chars().count();
    let reserved = badges::MARKER_W + badges::GLYPH_W + 1 + right_len + mark_w;
    let name_budget = col_width.saturating_sub(reserved).max(3);
    let name = crate::ui::truncate(&repo.name, name_budget);

    let left_len = badges::MARKER_W + badges::GLYPH_W + name.chars().count() + mark_w + 1; // +1 for space before dots
    let dot_count = col_width.saturating_sub(left_len + right_len).max(1);
    let dots = "·".repeat(dot_count);

    let mut spans = vec![
        Span::raw(marker.to_string()),
        badges::glyph_span(repo),
        Span::styled(name, name_style),
    ];
    if !mark.is_empty() {
        spans.push(Span::styled(mark, Style::default().fg(colors::OVERLAY)));
    }
    // The cursor's row wears the history lists' band (FZ-66). The dots'
    // resting colour, `SURFACE`, is lighter than the band, so they step up to
    // `OVERLAY` and bold to stay seen; `·` is one cell either way, so no
    // column moves.
    let dot_style = if is_selected {
        Style::default()
            .fg(colors::OVERLAY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors::SURFACE)
    };
    spans.push(Span::styled(format!(" {dots} "), dot_style));
    spans.extend(badge_spans);
    if is_selected { wash(spans) } else { spans }
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

    if let Some(glyph) = badges::kind_icon(repo.repo_kind) {
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

/// One local branch's rows: the branch itself, then any worktrees checked
/// out on it, indented beneath.
///
/// Shared by `info` and `branches` so the two cannot drift into drawing the
/// same branch differently -- the failure `split_inspector` exists to prevent,
/// one level up in this same file. The reasons these rows read the way they
/// do are in the comments below and in `docs/ideas.md`; a second copy would
/// lose them.
///
/// `shown` collects the worktree paths this consumed, so the caller can still
/// list the orphans afterwards.
fn branch_row_lines(
    b: &crate::model::BranchStatus,
    worktrees: &[crate::model::Worktree],
    shown: &mut std::collections::HashSet<String>,
) -> Vec<Line<'static>> {
    let mut rows: Vec<Line<'static>> = Vec::new();
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
    // Only when it differs from the branch's own name. A branch is
    // free to track a ref it is not named after, and with the upstream
    // left implicit those two shapes drew identically -- so a count
    // against `origin/wip` looked like a count against a branch of the
    // same name, and the row could not be reconciled with the remote.
    // Shown for the odd one out rather than for every row, because
    // repeating `origin/x` after `x` down the whole list is noise that
    // would bury exactly the case worth seeing.
    if let Some(upstream) = &b.upstream
        && upstream
            .split_once('/')
            .map_or(upstream.as_str(), |(_, n)| n)
            != b.name
    {
        branch_spans.push(Span::styled(
            format!(" -> {upstream}"),
            Style::default().fg(colors::OVERLAY),
        ));
    }
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
    rows.push(Line::from(branch_spans));

    for wt in worktrees.iter().filter(|wt| wt.branch == b.name) {
        shown.insert(wt.path.clone());
        rows.push(Line::from(Span::styled(
            // The word, not a glyph: `⎇` is the Alt key symbol that
            // shell prompts use for *branch*, so it read as "branch"
            // in a list of branches. Categorical things get words.
            format!("    worktree {}", wt.path),
            Style::default().fg(colors::OVERLAY),
        )));
    }
    rows
}

fn render_detail_pane(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.focused_panel == FocusedPanel::Detail;
    let border_color = if focused {
        colors::LAVENDER
    } else {
        colors::SURFACE
    };
    let pane = area;
    let panel = pane2_block(border_color);

    // On a section header pane 2 is that section's digest (FZ-45). The repo
    // views keep their state underneath and come back when the cursor
    // leaves the header.
    if let Some(digest) = app.digest(chrono::Local::now()) {
        *app.detail_hits.borrow_mut() = DetailHits::default();
        crate::ui::digest::render_pane(frame, area, panel, app, &digest, focused);
        return;
    }

    let repo = match app.selected_repo() {
        Some(r) => r,
        None => {
            *app.detail_hits.borrow_mut() = DetailHits::default();
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
    // above them. The right title carries the view switcher instead.
    let status = repo.short_status();
    // The number span comes from `panel_title` so its colors have one source;
    // a copy here once drew the unfocused number darker than every other pane.
    let number = crate::ui::panel_title(2, "", focused).spans.swap_remove(0);
    let left_title = Line::from(vec![
        number,
        Span::styled(
            repo.name.clone(),
            Style::default()
                .fg(colors::SAPPHIRE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {status} "), Style::default().fg(colors::OVERLAY)),
    ]);
    // Two corners and a column of border either side of the two titles, so
    // they cannot meet in the middle and overprint.
    let budget = (area.width as usize)
        .saturating_sub(2)
        .saturating_sub(span_width(&left_title.spans) + 2);

    let panel = panel
        .title_top(left_title.left_aligned())
        .title_top(view_switcher(app.history_view, focused, budget));
    let inner = panel.inner(area);
    frame.render_widget(panel, area);

    let rows = pane_rows(inner);
    render_selected_commit_readout(frame, rows[0], app);
    let area = rows[1];
    let width = area.width as usize;

    // FZ-67: in the `notes` layout the header pins across the whole pane and
    // the body under it is the tree's, beside the preview.
    let notes_split = match app.notes_layout() {
        true => notes_pane_body(pane, app),
        false => None,
    };
    let mut lines: Vec<Line> = header_lines(app, repo, width);
    let (area, width) = match notes_split {
        Some((header, body)) => {
            let (tree, _) = split_notes_pane(body);
            match header {
                Some(header) => frame.render_widget(Paragraph::new(lines.split_off(0)), header),
                // Unpinned on a short pane: the header scrolls with the tree,
                // laid out at the tree's width.
                None => lines = header_lines(app, repo, tree.width as usize),
            }
            (tree, tree.width as usize)
        }
        None => (area, width),
    };

    // Everything above this point identifies the repo -- what state it is in,
    // where it lives, which branch is out, and which of its views you are in.
    // It is the answer to "what am I looking at", and scrolling a two-hundred
    // row history is exactly when that question comes back. Held out of the
    // scrolling body below.
    let header_rows = lines.len();
    // How far the pinned region reaches once the branches block has been laid
    // out. Starts at the identity header and grows to take the local branches
    // in; every view that draws no branches keeps the header's own count.
    let mut pinned_rows = header_rows;
    // The first line and line count of `info`'s remote-only names, when `B`
    // has them expanded, for the reveal below (FZ-55).
    let mut expanded_remotes: Option<(usize, usize)> = None;

    // Branches. Info lists worktrees in a section of their own; the branches
    // view still draws each inline under the branch it has checked out.
    let view = app.history_view;

    // How old the local work is. Only local-only repos carry this: they are
    // the ones with no `pushedAt` to be dated by, which is what put them in
    // "local changes". The absolute date answers "when", the age answers "how
    // long", and the age uses `format_age` so it reads like the commit rows
    // further down rather than inventing a second vocabulary for time.
    if view == HistoryView::Info
        && let Some(at) = repo.last_local
    {
        lines.push(Line::from(vec![
            Span::styled("last touched ", Style::default().fg(colors::OVERLAY)),
            Span::styled(
                at.format("%Y-%m-%d").to_string(),
                Style::default().fg(colors::TEXT),
            ),
            Span::styled(
                format!(" ({})", crate::ui::format_age(at)),
                Style::default().fg(colors::OVERLAY),
            ),
        ]));
        lines.push(Line::from(""));
    }
    let mut shown_worktrees = std::collections::HashSet::new();
    if view == HistoryView::Info
        && (!repo.branches.is_empty() || !repo.remote_only_branches.is_empty())
    {
        lines.push(Line::from(section_label("branches")));
        for b in &repo.branches {
            // No worktrees: Info's `worktrees` section below holds the path,
            // with its state, so an inline row here would say it twice.
            lines.extend(branch_row_lines(b, &[], &mut shown_worktrees));
        }

        // The pinned region ends here, with the local branches and not the
        // list below them. What a repo is behind on is bounded by the branches
        // you actually have; the remote-only list is bounded by what everyone
        // else has pushed, and on a busy repo it runs to dozens of rows that
        // would take the whole pane if they were pinned too.
        pinned_rows = lines.len();

        // Remote branches with no local counterpart -- never fetched/checked
        // out, or checked out once and deleted locally after merging. Shown so
        // "what am I missing locally" is visible at a glance.
        //
        // Collapsed to a count by default. Unlike a PR number or a changed
        // path, a remote-only branch name is not identifying: three of
        // twenty-three feature branches a colleague pushed say nothing the
        // count does not, while costing three rows of the commit list below --
        // which used to start below the fold, so the selection scrolled this
        // whole block away the moment the cursor moved. `B` expands it in
        // place; the `branches` view lists them all with their state.
        if !repo.remote_only_branches.is_empty() {
            if app.expand_remote_branches {
                expanded_remotes = Some((lines.len(), repo.remote_only_branches.len()));
                for name in &repo.remote_only_branches {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {name}"), Style::default().fg(colors::OVERLAY)),
                        Span::styled(" not fetched", Style::default().fg(colors::SUBTEXT)),
                    ]));
                }
            } else {
                let n = repo.remote_only_branches.len();
                lines.push(Line::from(Span::styled(
                    format!(
                        "  {n} remote branch{} not fetched",
                        if n == 1 { "" } else { "es" }
                    ),
                    Style::default().fg(colors::OVERLAY),
                )));
            }
        }
        lines.push(Line::from(""));
    }

    // CI at a glance, from what is in hand: the default branch's check state
    // and any runs a visit to `ci` left held. Drawing it polls nothing; the
    // full list is one `]` away. No section for a row with no GitHub
    // identity.
    if view == HistoryView::Info
        && let Some(rows) = crate::ui::ci::info_rows(app, repo, width, chrono::Local::now())
    {
        lines.push(Line::from(section_label("ci")));
        lines.extend(rows);
        lines.push(Line::from(""));
    }

    // Not pinned: the local branches are, because the behind count is their
    // sum; this list is bounded by the checkouts you made and sits just below.
    if view == HistoryView::Info {
        let worktrees = worktree_rows(repo, width);
        if !worktrees.is_empty() {
            lines.extend(worktrees);
            lines.push(Line::from(""));
        }
    }

    // The compact graph list: git's topology in a column, then hash, author,
    // decorations and subject. `info` shows the newest few as a glance;
    // `commits` shows the lot.
    // `info` shows the newest few, `commits` the lot, through one renderer so
    // the two cannot drift into drawing the same rows differently.
    // The selected block's inclusive row range. The expanded graph's blocks
    // run to twenty rows and wrap on top of that, so bringing "the selection"
    // into view means bringing a range into view, not a row. The compact
    // graph and the reflog report a one-row range and behave as before.
    let mut selected_block: Option<(usize, usize)> = None;
    // Where every selectable entry begins, in list order. Handed to the app so
    // a scroll can keep the cursor on screen; only this pass knows the rows,
    // since they sit under however many sections this repo drew.
    let mut block_tops: Vec<usize> = Vec::new();
    if matches!(view, HistoryView::Info | HistoryView::Commits) && !repo.graph.is_empty() {
        let limit = (view == HistoryView::Info).then_some(RECENT_COMMIT_COUNT);
        // `info` still labels this section, because it is one of several it
        // draws. `commits` does not: the pinned title above already says so,
        // and the list is the only thing under it.
        if view == HistoryView::Info {
            lines.push(Line::from(section_label("recent commits")));
        }
        // One line per commit, in list order, so the rows are the run this
        // section occupies.
        let base = lines.len();
        let mut list_block: Option<(usize, usize)> = None;
        lines.extend(graph_list_lines(
            repo,
            width,
            app.detail_selected(),
            limit,
            &mut list_block,
        ));
        block_tops.extend(base..lines.len());
        // Offset, because `graph_list_lines` counts from its own first row and
        // the pane counts from the top of the body. Unoffset, the follow step
        // scrolled to a row `base` above the selection -- so the highlight sat
        // that far below the pane's bottom edge -- and for a commit whose
        // index fell inside the pinned header the subtraction below nulled the
        // block out and the pane did not follow at all. The reflog reported
        // `lines.len()` and was right, which is why only the lists drifted.
        if let Some((top, bottom)) = list_block {
            selected_block = Some((base + top, base + bottom));
        }
        lines.push(Line::from(""));
    }

    // The expanded graph: git's own `log --graph` layout, one block per
    // commit with Merge/Author/Date and the full message. Reproduced rather
    // than recomposed -- the value is that it looks like what you get in a
    // terminal, so the renderer colours the lines and changes nothing else.
    if view == HistoryView::Graph && !repo.graph_expanded.is_empty() {
        let selected_hash = app.inspected_hash().unwrap_or_default().to_string();
        let mut open: Option<usize> = None;
        for raw in &repo.graph_expanded {
            // A block runs from the line naming its commit to the line before
            // the next one names another, so the end is found by watching for
            // the next `commit ` line rather than by counting rows.
            if commit_hash_at(raw).is_some() {
                if let Some(start) = open.take() {
                    selected_block = Some((start, lines.len().saturating_sub(1)));
                }
                // One predicate for the tops, the block ends and the marker,
                // so they cannot disagree about where a commit starts.
                block_tops.push(lines.len());
            }
            // The block the selection names starts here, so scrolling to it
            // puts the whole commit on screen rather than its last line, and
            // so the marker lands on the row that names the commit rather
            // than on some line of its message.
            if opens_commit(raw, &selected_hash) {
                open = Some(lines.len());
            }
            lines.extend(expanded_graph_lines(raw, &selected_hash, width));
        }
        // The selected block was the last one, so it runs to the end.
        if let Some(start) = open {
            selected_block = Some((start, lines.len().saturating_sub(1)));
        }
        lines.push(Line::from(""));
    }

    // Reflog -- recent HEAD movements, for a "what did I just do" glance.
    if view == HistoryView::Reflog && !repo.reflog.is_empty() {
        for (ri, entry) in repo.reflog.iter().enumerate() {
            let prefix_len = 2 + entry.hash.chars().count();
            let action_len = if entry.action.is_empty() {
                0
            } else {
                1 + entry.action.chars().count()
            };
            let msg_budget = width.saturating_sub(prefix_len + action_len + 1).max(10);
            let description = crate::ui::truncate(&entry.description, msg_budget);

            let is_sel = ri == app.detail_selected();
            if is_sel {
                selected_block = Some((lines.len(), lines.len()));
            }
            block_tops.push(lines.len());
            // The same caret and wash the commit lists use. A bolded hash was
            // all this had, which marks one token and leaves the eye to find
            // which -- the work the marker exists to save.
            let mut reflog_spans = vec![
                Span::styled(
                    if is_sel { "▸ " } else { "  " },
                    Style::default().fg(colors::SAPPHIRE),
                ),
                Span::styled(entry.hash.clone(), hash_style(is_sel)),
            ];
            if !entry.action.is_empty() {
                reflog_spans.push(Span::styled(
                    format!(" {}", entry.action),
                    Style::default().fg(colors::MAUVE),
                ));
            }
            reflog_spans.push(Span::raw(" "));
            reflog_spans.push(Span::styled(description, Style::default().fg(colors::TEXT)));
            if is_sel {
                reflog_spans = wash(pad_to(reflog_spans, width));
            }
            lines.push(Line::from(reflog_spans));
        }
        lines.push(Line::from(""));
    }

    // The branches view: every local branch, then every remote. Read-only --
    // deleting stays in the `b` overlay, which already has the merged/protected
    // knowledge and the confirm step for it.
    if view == HistoryView::Branches {
        lines.push(Line::from(section_label("local branches")));
        for (bi, b) in repo.branches.iter().enumerate() {
            let is_sel = bi == app.detail_selected();
            // The same renderer `info` uses, so the two cannot draw the same
            // branch differently.
            let rows = branch_row_lines(b, &repo.worktrees, &mut shown_worktrees);
            if is_sel {
                selected_block = Some((lines.len(), lines.len() + rows.len() - 1));
            }
            block_tops.push(lines.len());
            for (i, row) in rows.into_iter().enumerate() {
                // The caret is a column of its own, beside git's `*`: one says
                // where the cursor is, the other where HEAD is, and a row can
                // be both or neither.
                let mut spans = vec![Span::styled(
                    if is_sel && i == 0 { "▸ " } else { "  " },
                    Style::default().fg(colors::SAPPHIRE),
                )];
                spans.extend(row.spans);
                if is_sel && i == 0 {
                    spans = wash(pad_to(spans, width));
                }
                lines.push(Line::from(spans));
            }
        }
        lines.push(Line::from(""));

        lines.push(Line::from(section_label("remotes")));
        // Every remote, not only the unclaimed ones: "not fetched" is a marker
        // on a row, not a reason to leave the row out.
        let local_count = repo.branches.len();
        for (ri, r) in repo.remote_branches.iter().enumerate() {
            let is_sel = local_count + ri == app.detail_selected();
            if is_sel {
                selected_block = Some((lines.len(), lines.len()));
            }
            block_tops.push(lines.len());
            let mut spans = vec![
                Span::styled(
                    if is_sel { "▸ " } else { "  " },
                    Style::default().fg(colors::SAPPHIRE),
                ),
                Span::styled(r.name.clone(), Style::default().fg(colors::TEXT)),
            ];
            if repo.remote_only_branches.contains(&r.name) {
                spans.push(Span::styled(
                    " not fetched",
                    Style::default().fg(colors::SUBTEXT),
                ));
            }
            if is_sel {
                spans = wash(pad_to(spans, width));
            }
            lines.push(Line::from(spans));
        }
        lines.push(Line::from(""));
    }

    // The `ci` view: the freshness line and, while runs are listed, the
    // headline naming their columns, both pinned with the header so they
    // stay in sight while the runs scroll; then one row per held run.
    if view == HistoryView::Ci {
        lines.push(crate::ui::ci::first_line(app, chrono::Local::now()));
        lines.extend(crate::ui::ci::headline(app, width, chrono::Local::now()));
        pinned_rows = lines.len();
        let count = app.selected_commit_count();
        let selected = app.detail_selected().min(count.saturating_sub(1));
        let base = lines.len();
        let rows = crate::ui::ci::run_rows(app, width, chrono::Local::now(), selected, focused);
        if !rows.is_empty() {
            selected_block = Some((base + selected, base + selected));
        }
        block_tops.extend(base..base + rows.len());
        lines.extend(rows);
    }

    // The `notes` view: which tree is drawn, pinned with the header, then
    // the tree, one cursor stop per file.
    if view == HistoryView::Notes {
        lines.push(notes_mode_line(app));
        pinned_rows = lines.len();
        let base = lines.len();
        let (rows, tops, selected) = notes_rows(app, repo, width);
        if let Some(row) = selected {
            selected_block = Some((base + row, base + row));
        }
        block_tops.extend(tops.into_iter().map(|t| base + t));
        lines.extend(rows);
    }

    // PRs
    // Gated on `-p`: the header row already names the open PRs by number, so
    // this section is the opt-in detail rather than the news itself.
    if view == HistoryView::Info && app.show_pr_detail && !repo.prs.is_empty() {
        // The query lists a few PRs but counts them all, so name both when the
        // list is the shorter. The noun follows the total: it says how many are
        // open. A list longer than the count (should not happen) keeps today's
        // plain "N open PRs" rather than printing "8 of 5".
        let listed = repo.prs.len();
        let total = repo.pr_count as usize;
        let pr_label = if total.max(listed) == 1 { "PR" } else { "PRs" };
        let heading = if listed < total {
            format!("{listed} of {total} open {pr_label}")
        } else {
            format!("{listed} open {pr_label}")
        };
        lines.push(Line::from(section_label(&heading)));
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
    // Two rungs, widest first. The branches block is the only thing that
    // explains the repo's behind count, and it was the first thing to scroll
    // away: stepping the commit cursor pushed it off the top and left the
    // "not fetched" list reading like the whole story. Pinning it needs a
    // fallback rather than a bigger constant, because a repo with many local
    // branches would otherwise fail the one test and lose the identity header
    // its pin too -- trading a real regression for the fix.
    let pinned = [pinned_rows, header_rows]
        .into_iter()
        .find(|rows| area.height >= *rows as u16 + MIN_BODY_ROWS)
        .unwrap_or(0);
    let pin_header = pinned > 0;
    let header_h = pinned as u16;

    let (header_area, area) = match pin_header {
        true => {
            let split =
                Layout::vertical([Constraint::Length(header_h), Constraint::Min(1)]).split(area);
            (Some(split[0]), split[1])
        }
        false => (None, area),
    };
    let mut lines = match pin_header {
        true => {
            let body = lines.split_off(pinned);
            frame.render_widget(Paragraph::new(lines), header_area.expect("split"));
            body
        }
        false => lines,
    };
    // Rows above the split are no longer in the scrolling body, so every row
    // index the sections recorded has to come down with them.
    let selected_block = match pin_header {
        // Both ends come down together, and a block whose top is inside the
        // pinned header is no longer in the scrolling body at all.
        true => selected_block
            .and_then(|(t, b)| Some((t.checked_sub(pinned)?, b.saturating_sub(pinned)))),
        false => selected_block,
    };
    // Written every frame, empty list included: a stale set of rows from the
    // view or the repo you just left would drag the cursor to the wrong entry.
    //
    // Rows above the split left the scrolling body with the header, so the
    // tops come down by the same amount the selection did -- and only when the
    // header was actually split off.
    let shed = if pin_header { pinned } else { 0 };
    let body_tops: Vec<Option<usize>> = block_tops.iter().map(|t| t.checked_sub(shed)).collect();
    *app.detail_row_tops.borrow_mut() = block_tops
        .into_iter()
        .map(|t| t.saturating_sub(shed) as u16)
        .collect();

    let max_scroll = wrapped_row_count(&lines, area.width).saturating_sub(area.height);
    app.detail_max_scroll.set(max_scroll);
    // The scrolloff arithmetic needs the body's height, and this is the only
    // place the pane's height and the pinned header's are both known.
    app.detail_view_height.set(area.height);
    let mut scroll = app.detail_scroll().min(max_scroll);

    // Follow the cursor. This is the only place that knows which row the
    // selection landed on, since the row depends on how many sections were
    // drawn above it.
    //
    // Without it the cursor walked rows that were off screen and the pane sat
    // still, so `j` looked like it did nothing at all -- survivable at six
    // commits, and the whole view at two hundred.
    if app.follow_selection.get()
        && let Some((top, bottom)) = selected_block
    {
        scroll = bring_into_view((top as u16, bottom as u16), scroll, area.height);
        scroll = scroll.min(max_scroll);
        app.set_detail_scroll(scroll);
        app.follow_selection.set(false);
    }

    // Reveal on expand: on the frame `B` opens the remote-only names, scroll
    // just far enough to show them (`scroll::reveal_offset`). Only the frame
    // the flag flips, so scrolling back up afterwards is left alone, and only
    // when the expand opened rows in the scrolling body.
    let opened = app.expand_remote_branches && !app.remotes_drawn_expanded.get();
    app.remotes_drawn_expanded.set(app.expand_remote_branches);
    if opened && let Some((first, count)) = expanded_remotes {
        let first = first.saturating_sub(shed);
        let row = |range: std::ops::Range<usize>| -> u16 {
            let rows: usize = lines[range].iter().map(|l| line_rows(l, area.width)).sum();
            rows.min(u16::MAX as usize) as u16
        };
        let folder = row(0..first);
        let rows = row(first..first + count);
        scroll =
            crate::ui::scroll::reveal_offset(rows, folder, scroll, area.height).min(max_scroll);
        app.set_detail_scroll(scroll);
    }

    // The graph's cursor is a row, so the wash goes on after the block layout
    // is settled: a source line can wrap into several rows, and the cursor
    // names one of those rows rather than one of the lines that made them.
    if view == HistoryView::Graph {
        mark_cursor_row(&mut lines, app.graph_cursor, area.width as usize);
    }

    *app.detail_hits.borrow_mut() = DetailHits::measure(
        &lines,
        &body_tops,
        |entry| app.entry_hash(entry),
        area,
        scroll,
        view == HistoryView::Graph,
    );

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Pane 2 of the `notes` layout under its frame and readout row (FZ-67):
/// the rows its header pins across the whole pane, `None` when the pane is
/// too short to spare them, and the body under it that `split_notes_pane`
/// splits into the tree and the preview. `None` when pane 2 shows no tree
/// (a section's digest, or no repo). Shared by `render_detail_pane` and
/// `ui::panel_rects`, so the preview is hit-tested where it is drawn.
pub fn notes_pane_body(pane: Rect, app: &App) -> Option<(Option<Rect>, Rect)> {
    let repo = app.selected_repo()?;
    let below = pane_rows(pane2_block(colors::SURFACE).inner(pane))[1];
    let header_rows =
        u16::try_from(header_lines(app, repo, below.width as usize).len()).unwrap_or(u16::MAX);
    if below.height < header_rows.saturating_add(MIN_BODY_ROWS) {
        return Some((None, below));
    }
    let split =
        Layout::vertical([Constraint::Length(header_rows), Constraint::Min(1)]).split(below);
    Some((Some(split[0]), split[1]))
}

/// Pane 2's frame, every view's: rounded, a column of padding either side
/// and a row above the readout.
fn pane2_block(border_color: ratatui::style::Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::new(1, 1, 1, 0))
}

/// Pane 2 inside its frame: one pinned row directly under the title, then
/// the scrolling body. The readout has to sit outside the paragraph: the
/// whole body scrolls as a unit, so a line inside it would scroll away
/// exactly when it is needed, which is when the commit list has been
/// scrolled past.
fn pane_rows(inner: Rect) -> std::rc::Rc<[Rect]> {
    Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(inner)
}

/// Pane 2's identity header, pinned above every repo view: how the repo is
/// doing, where it lives, what is out, then the view's headliner.
fn header_lines(app: &App, repo: &RepoStatus, width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

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

    // Under row 1, while the last fetch failed: the counts beside it are
    // against the remote refs of the last fetch that worked, and when that
    // was. Then git's own reason, already stripped of credentials.
    let mut fetch_rows: Vec<Line> = Vec::new();
    if let Some(text) = crate::ui::fetch_failure_line(&repo.fetch, chrono::Local::now()) {
        fetch_rows.push(Line::from(Span::styled(
            format!("  {text}"),
            Style::default().fg(colors::PEACH),
        )));
        if let Some(detail) = repo.fetch.failure().and_then(|f| f.detail.as_deref()) {
            fetch_rows.push(Line::from(Span::styled(
                format!("    {detail}"),
                Style::default().fg(colors::OVERLAY),
            )));
        }
    }

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
    // them here the way it learned to find them there. Neither is drawn in
    // OVERLAY: that is for paths and furniture, and at that brightness real
    // news reads as decoration you have already dismissed.
    // Each carries the colour its badge uses (one colour per concept): the
    // stash count is STASH_CLR and the worktree count is WORKTREE_CLR, so the
    // header fact is recognised as the same thing the list row flags.
    let mut local_extra: Vec<(String, ratatui::style::Color)> = Vec::new();
    if repo.stash_count > 0 {
        let noun = if repo.stash_count == 1 {
            "stash"
        } else {
            "stashes"
        };
        local_extra.push((
            format!("{} {} {noun}", badges::stash_icon(), repo.stash_count),
            colors::STASH_CLR,
        ));
    }
    if !repo.worktrees.is_empty() {
        let noun = if repo.worktrees.len() == 1 {
            "worktree"
        } else {
            "worktrees"
        };
        local_extra.push((
            format!("{} {noun}", repo.worktrees.len()),
            colors::WORKTREE_CLR,
        ));
    }
    for (extra, color) in local_extra {
        if row3_left.is_empty() {
            row3_left.push(Span::raw("  "));
        } else {
            row3_left.push(Span::styled(" · ", Style::default().fg(colors::SURFACE)));
        }
        row3_left.push(Span::styled(extra, Style::default().fg(color)));
    }

    // Row 3 right: facts that live on GitHub. From an earlier refresh when
    // the last poll did not return this repo: the same values, in the list's
    // grey, the lamp held still.
    let not_in_last_poll = app.is_not_in_last_poll(repo);
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
        let pr_color = if not_in_last_poll {
            badges::EARLIER_REFRESH
        } else {
            colors::PR_CLR
        };
        row3_right.push(Span::styled(text, Style::default().fg(pr_color)));
    }
    if repo.ci_status != CiStatus::Unknown {
        let (ci_sym, ci_color, ci_label) =
            badges::ci_display(&repo.ci_status, app.watch_mode && !app.blink_on);
        let ci_color = if not_in_last_poll {
            badges::EARLIER_REFRESH
        } else {
            ci_color
        };
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
    // Drawn without userinfo, so a token kept in an HTTPS remote never
    // reaches the screen. Display only: matching and `owner_display` read
    // the stored URL.
    let (remote_loc, remote_loc_w) = match repo.remote_url.as_deref() {
        Some(url) => {
            let shown = crate::git::redact_userinfo(url);
            let shown_w = shown.chars().count();
            (
                vec![Span::styled(shown, Style::default().fg(colors::OVERLAY))],
                shown_w,
            )
        }
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
    lines.extend(fetch_rows);
    lines.push(Line::from(row2));
    if !row3.is_empty() {
        lines.push(Line::from(row3));
    }
    if not_in_last_poll {
        lines.push(Line::from(Span::styled(
            format!("  {EARLIER_REFRESH_NOTE}"),
            Style::default().fg(badges::EARLIER_REFRESH),
        )));
    }
    lines.push(Line::from(""));
    // The view's own title, in the colour its tab wears up in the frame. The
    // tab is what you aim at; this is what confirms you landed, and saying it
    // twice in two colours would have made the pair read as two things.
    lines.extend(headliner(app.history_view, width));
    lines
}

/// Where the detail pane's scrolling body drew its rows on the last frame,
/// and which columns of each entry's row hold its hash. Built from the lines
/// the paragraph is handed, with the row arithmetic the scroll already uses
/// (`line_rows`), so a click is resolved against what is on screen rather
/// than against a second copy of the layout.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DetailHits {
    /// The scrolling body on screen, below any pinned header.
    pub body: Rect,
    /// Rows scrolled off the top of `body`.
    pub scroll: u16,
    /// First wrapped row of each body line, then the total row count.
    pub line_starts: Vec<u16>,
    /// Each selectable entry, in list order: the body line it starts on and
    /// the columns of that line's first row its hash occupies.
    pub entries: Vec<EntryHit>,
    /// Every line is a cursor stop (the graph's row cursor), not just an
    /// entry's.
    pub row_cursor: bool,
}

/// One selectable entry of the detail pane's body.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryHit {
    /// The entry's index in the view's list, as the cursor counts it.
    pub entry: usize,
    pub line: usize,
    /// The hash as drawn, when it is drawn whole on the line's first row.
    /// Kept so a click copies what was on screen, even if the list changed
    /// between the paint and the click.
    pub hash: Option<String>,
    /// Start and end (exclusive) column of `hash` within the body.
    pub hash_cols: Option<(u16, u16)>,
}

/// What a click in the detail pane's body landed on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailClick {
    /// The body line clicked, which is the graph's cursor unit.
    pub line: u16,
    /// The index of the entry that line starts, if any.
    pub entry: Option<usize>,
    /// The hash drawn under the click, when the click fell on one.
    pub hash: Option<String>,
}

impl DetailHits {
    fn measure<'a>(
        lines: &[Line],
        tops: &[Option<usize>],
        hash_of: impl Fn(usize) -> Option<&'a str>,
        body: Rect,
        scroll: u16,
        row_cursor: bool,
    ) -> Self {
        let mut line_starts = Vec::with_capacity(lines.len() + 1);
        let mut row = 0usize;
        for line in lines {
            line_starts.push(row.min(u16::MAX as usize) as u16);
            row += line_rows(line, body.width);
        }
        line_starts.push(row.min(u16::MAX as usize) as u16);

        let entries = tops
            .iter()
            .enumerate()
            .filter_map(|(entry, top)| {
                let line = (*top)?;
                let text: String = lines
                    .get(line)?
                    .spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect();
                let drawn = hash_of(entry).filter(|h| !h.is_empty()).and_then(|h| {
                    let at = text.find(h)?;
                    let start = text[..at].chars().count();
                    let end = start + h.chars().count();
                    (end <= body.width as usize).then_some((h, (start as u16, end as u16)))
                });
                Some(EntryHit {
                    entry,
                    line,
                    hash: drawn.map(|(h, _)| h.to_string()),
                    hash_cols: drawn.map(|(_, cols)| cols),
                })
            })
            .collect();

        Self {
            body,
            scroll,
            line_starts,
            entries,
            row_cursor,
        }
    }

    /// The row and entry under screen cell (`col`, `row`), or `None` outside
    /// the body, below the last line, or on a line that is no cursor stop.
    pub fn at(&self, col: u16, row: u16) -> Option<DetailClick> {
        let b = self.body;
        if col < b.x || col >= b.x + b.width || row < b.y || row >= b.y + b.height {
            return None;
        }
        let body_row = row - b.y + self.scroll;
        let x = col - b.x;
        let line = self
            .line_starts
            .windows(2)
            .position(|w| w[0] <= body_row && body_row < w[1])?;
        let hit = self.entries.iter().find(|e| e.line == line);
        if hit.is_none() && !self.row_cursor {
            return None;
        }
        let hash = hit
            .filter(|e| {
                e.hash_cols.is_some_and(|(start, end)| {
                    body_row == self.line_starts[line] && start <= x && x < end
                })
            })
            .and_then(|e| e.hash.clone());
        Some(DetailClick {
            line: line as u16,
            entry: hit.map(|e| e.entry),
            hash,
        })
    }
}

/// Put the caret and the selection wash on one row of the graph, clamped to
/// the rows there are: the cursor outlives the repo it was chosen against, and
/// a shorter history must move it rather than lose it.
fn mark_cursor_row(lines: &mut [Line<'static>], cursor: u16, width: usize) {
    let Some(row) = lines
        .len()
        .checked_sub(1)
        .map(|last| (cursor as usize).min(last))
    else {
        return;
    };
    let spans = std::mem::take(&mut lines[row].spans);
    let mut spans = match spans.split_first() {
        // Replace the caret column rather than prepending one, so the row
        // keeps the width every other row has.
        Some((_, rest)) => {
            let mut v = vec![Span::styled(
                "\u{25b8} ".to_string(),
                Style::default().fg(colors::SAPPHIRE),
            )];
            v.extend(rest.iter().cloned());
            v
        }
        None => Vec::new(),
    };
    spans = wash(pad_to(spans, width));
    lines[row] = Line::from(spans);
}

/// The `notes` tree's two modes in the words the mode line, the empty states
/// and the help overlay's `m` row all use (FZ-57). `markdown` rather than
/// `all markdown`: the help row has 26 columns at 80x24, and the shorter
/// word still says what the second mode adds.
const NOTES_MODES: [&str; 2] = ["notez", "markdown"];

/// The `notes` view's first line: both modes as a two-state switch, the
/// active one bracketed and lit as the tab strip lights its view, the other
/// dim, and the key that flips them (FZ-57): `notes: [notez] markdown  (m)`.
fn notes_mode_line(app: &App) -> Line<'static> {
    notes_mode_line_for(app.notes_all_markdown)
}

/// `notes_mode_line` for either mode, so the tree's width floor can measure
/// the line it has to hold (`notes_tree_floor`).
fn notes_mode_line_for(all_markdown: bool) -> Line<'static> {
    let on = colors::history_view_color(HistoryView::Notes);
    let active = usize::from(all_markdown);
    let mut spans = vec![Span::styled(
        "notes: ",
        Style::default().fg(colors::OVERLAY),
    )];
    for (i, mode) in NOTES_MODES.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(match i == active {
            true => Span::styled(
                format!("[{mode}]"),
                Style::default().fg(on).add_modifier(Modifier::BOLD),
            ),
            false => Span::styled(mode.to_string(), Style::default().fg(colors::SUBTEXT)),
        });
    }
    spans.push(Span::styled("  (", Style::default().fg(colors::OVERLAY)));
    spans.push(Span::styled(
        "m",
        Style::default()
            .fg(colors::TEXT)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(")", Style::default().fg(colors::OVERLAY)));
    Line::from(spans)
}

/// The `notes` view's tree for `repo` (FZ-53): its lines, the line each file
/// starts on, in cursor order, and the line the cursor's file is on. A row
/// with nothing to list says why in one line.
fn notes_rows(
    app: &App,
    repo: &RepoStatus,
    width: usize,
) -> (Vec<Line<'static>>, Vec<usize>, Option<usize>) {
    let say = |text: String| {
        let line = Line::from(Span::styled(text, Style::default().fg(colors::OVERLAY)));
        (vec![line], Vec::new(), None)
    };
    if repo.path.is_none() {
        return say("not cloned: no notes to read".to_string());
    }
    let notes = match app
        .selected_notes()
        .and_then(|(_, state)| state?.tree.as_ref())
    {
        None => return say("reading notes…".to_string()),
        Some(Err(e)) => return say(format!("could not read the repo: {e}")),
        Some(Ok(notes)) => notes,
    };
    let rows = notes.rows(app.notes_all_markdown);
    if rows.is_empty() {
        // Named by the mode it belongs to, in the mode line's words.
        return say(match app.notes_all_markdown {
            false => format!("{}: no notez/ or .notez/ directory", NOTES_MODES[0]),
            true => format!("{}: no markdown files", NOTES_MODES[1]),
        });
    }
    let files = rows
        .iter()
        .filter(|r| matches!(r, crate::notes::Row::File { .. }))
        .count();
    let cursor = app.detail_selected().min(files.saturating_sub(1));
    let mut lines = Vec::with_capacity(rows.len());
    let mut tops = Vec::with_capacity(files);
    let mut selected = None;
    for row in rows {
        match row {
            crate::notes::Row::Section(label) => {
                if !lines.is_empty() {
                    lines.push(Line::from(""));
                }
                lines.push(Line::from(section_label(&label)));
            }
            crate::notes::Row::Dir { depth, name } => {
                lines.push(Line::from(Span::styled(
                    format!("  {}{name}/", "  ".repeat(depth)),
                    Style::default().fg(colors::OVERLAY),
                )));
            }
            crate::notes::Row::File { depth, name, file } => {
                let is_sel = tops.len() == cursor;
                if is_sel {
                    selected = Some(lines.len());
                }
                tops.push(lines.len());
                // The kind's glyph before the name (FZ-62), dim so the name
                // stays what the eye reads.
                let glyph = notes
                    .files
                    .get(file)
                    .map_or("", |f| badges::note_kind_icon(f.kind()));
                let mut spans = vec![
                    Span::styled(
                        if is_sel { "▸ " } else { "  " },
                        Style::default().fg(colors::SAPPHIRE),
                    ),
                    Span::raw("  ".repeat(depth)),
                    Span::styled(format!("{glyph} "), Style::default().fg(colors::OVERLAY)),
                    Span::styled(name, Style::default().fg(colors::TEXT)),
                ];
                if is_sel {
                    spans = wash(pad_to(spans, width));
                }
                lines.push(Line::from(spans));
            }
        }
    }
    (lines, tops, selected)
}

/// The card's title in the `notes` layout, folded or open: it describes the
/// file, so it is named for it rather than for the commit inspector it
/// replaces there.
const NOTES_CARD_LABEL: &str = "file";

/// The width of the card's label column: the longest label and a space.
const CARD_LABEL_W: usize = 12;

/// The highlighted note's body as last read, when it is that file's.
fn held_note_body(app: &App) -> Option<&crate::app::NoteBody> {
    let path = app.notes_highlighted()?;
    app.note_body.as_ref().filter(|b| b.path == path)
}

/// A file time as the card writes it, in local time, to the minute.
fn card_time(at: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(at)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

/// git's `%cI` date as the card writes it, the way `card_time` writes a file
/// time; git's own spelling when it does not parse.
fn card_commit_time(iso: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(iso) {
        Ok(at) => at
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        Err(_) => iso.to_string(),
    }
}

/// The card's lines for the highlighted file (FZ-62): one labelled line per
/// fact. Size, times and the last commit wait for the background read
/// (`spawn_note_read`); created is left out where the filesystem keeps no
/// birth time.
fn notes_card_lines(app: &App) -> Vec<Line<'static>> {
    let dim = |text: &str| {
        Line::from(Span::styled(
            text.to_string(),
            Style::default().fg(colors::OVERLAY),
        ))
    };
    let Some(file) = app.notes_highlighted_file() else {
        return vec![dim("no file selected")];
    };
    let row = |label: &str, value: String, color: ratatui::style::Color| {
        Line::from(vec![
            Span::styled(
                format!("{label:<CARD_LABEL_W$}"),
                Style::default().fg(colors::OVERLAY),
            ),
            Span::styled(value, Style::default().fg(color)),
        ])
    };
    let name = file
        .rel
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let project = app
        .selected_repo()
        .map(|r| r.name.clone())
        .unwrap_or_default();
    let lives = match &file.notez {
        Some(dir) => format!("{}/", dir.display()),
        None => "the repo".to_string(),
    };
    let mut lines = vec![
        row("name", name, colors::TEXT),
        row("path", file.rel.display().to_string(), colors::TEXT),
        row("project", project, colors::SAPPHIRE),
        row("kind", file.kind().label().to_string(), colors::TEXT),
        row("lives in", lives, colors::TEXT),
    ];
    let held = held_note_body(app);
    let Some(facts) = held.and_then(|b| b.facts.as_ref()) else {
        lines.push(dim("reading…"));
        return lines;
    };
    if let Some(size) = facts.size {
        lines.push(row("size", crate::notes::size_label(size), colors::TEXT));
    }
    if let Some(at) = facts.modified {
        lines.push(row("modified", card_time(at), colors::TEXT));
    }
    if let Some(at) = facts.created {
        lines.push(row("created", card_time(at), colors::TEXT));
    }
    // git answers after the body (`AppEvent::NoteCommit`).
    use crate::notes::LastCommit;
    lines.push(match held.and_then(|b| b.last_commit.as_ref()) {
        Some(LastCommit::At(iso)) => row("last commit", card_commit_time(iso), colors::TEXT),
        Some(LastCommit::Untracked) => row("last commit", "untracked".to_string(), colors::OVERLAY),
        None => row("last commit", "reading…".to_string(), colors::OVERLAY),
    });
    lines
}

/// Draw `lines` wrapped into `inner` from `scroll`, clamped to what this
/// frame measured, and report the largest useful offset through `max`.
fn render_scrolled(
    frame: &mut Frame,
    inner: Rect,
    lines: &[Line<'static>],
    scroll: u16,
    max: &std::cell::Cell<u16>,
) {
    let wrapped: Vec<Line<'static>> = lines
        .iter()
        .flat_map(|l| wrap_line(l, inner.width as usize))
        .collect();
    render_rows(frame, inner, &wrapped, scroll, max);
}

/// Draw `rows`, one per screen row as given, into `inner` from `scroll`,
/// clamped to what this frame measured, and report the largest useful
/// offset through `max`. Only the rows on screen are copied.
fn render_rows(
    frame: &mut Frame,
    inner: Rect,
    rows: &[Line<'static>],
    scroll: u16,
    max: &std::cell::Cell<u16>,
) {
    let max_scroll = u16::try_from(rows.len())
        .unwrap_or(u16::MAX)
        .saturating_sub(inner.height);
    max.set(max_scroll);
    let visible: Vec<Line> = rows
        .iter()
        .skip(scroll.min(max_scroll) as usize)
        .take(inner.height as usize)
        .cloned()
        .collect();
    frame.render_widget(Paragraph::new(visible), inner);
}

/// The card where the inspector was, under pane 1 in the `notes` layout
/// (FZ-62): the highlighted file's facts, scrolled by the inspector's own
/// offset, since it is that pane (`[3]`, its size and its fold).
fn render_notes_card(frame: &mut Frame, area: Rect, app: &App) {
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
        .padding(Padding::new(1, 1, 0, 0))
        .title_top(crate::ui::panel_title(
            crate::ui::INSPECTOR_NUM,
            NOTES_CARD_LABEL,
            focused,
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    render_scrolled(
        frame,
        inner,
        &notes_card_lines(app),
        app.inspector_scroll,
        &app.inspector_max_scroll,
    );
}

/// The preview's left border title, inside pane 2 (FZ-67).
const READER_LABEL: &str = " notes preview ";

/// The reader's right border title: the file's name, kind and size, then for a
/// markdown file whether it is drawn rendered or raw, as in
/// `README.md · markdown · 4.2 KiB · rendered`. The size waits for the read.
fn reader_title(app: &App) -> Option<String> {
    let file = app.notes_highlighted_file()?;
    let name = file.rel.file_name()?.to_string_lossy().into_owned();
    let mut title = format!(" {name} · {}", file.kind().label());
    if let Some(size) = held_note_body(app)
        .and_then(|b| b.facts.as_ref())
        .and_then(|f| f.size)
    {
        title.push_str(&format!(" · {}", crate::notes::size_label(size)));
    }
    if file.kind().is_markdown() {
        title.push_str(match app.reader_raw {
            true => " · raw",
            false => " · rendered",
        });
    }
    title.push(' ');
    Some(title)
}

/// The held body's markdown render, when the reader draws one: a markdown
/// file, `p` not on raw, and a render landed (FZ-59). Until one lands the
/// reader shows the source.
fn reader_render(app: &App) -> Option<&crate::app::RenderedNote> {
    let markdown = app
        .notes_highlighted_file()
        .is_some_and(|f| f.kind().is_markdown());
    if !markdown || app.reader_raw {
        return None;
    }
    let body = held_note_body(app)?;
    matches!(body.text, Some(Ok(_)))
        .then_some(body.rendered.as_ref())
        .flatten()
}

/// The `notes` layout's reader, right of the tree in pane 2 (FZ-62): the
/// highlighted file's body, wrapped to the pane, with its own scroll
/// (`App::reader_scroll`). A markdown file is drawn rendered
/// (`ui::markdown`, FZ-59), its rows laid out off the UI thread at the
/// width this pass records; `p` shows its source, which every other kind
/// always shows, raw as `notes::read_body` delivers it. A cut body says so
/// on its last line, as the read leaves it.
fn render_reader(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.focused_panel == FocusedPanel::Reader;
    let border_color = if focused {
        colors::LAVENDER
    } else {
        colors::SURFACE
    };
    let title_style = match focused {
        true => Style::default()
            .fg(colors::LAVENDER)
            .add_modifier(Modifier::BOLD),
        false => Style::default().fg(colors::OVERLAY),
    };
    let label = Line::from(Span::styled(READER_LABEL, title_style));
    let label_width = label.width();
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::new(1, 1, 0, 0))
        .title_top(label.left_aligned());
    // The file's facts on the right of the same border, dropped whole when
    // the border cannot hold both with a column between them (FZ-67).
    let room = (area.width as usize).saturating_sub(2);
    if let Some(facts) = reader_title(app).map(|t| Line::from(Span::styled(t, title_style)))
        && label_width + 1 + facts.width() <= room
    {
        block = block.title_top(facts.right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    // One `H`/`L` page and the render width, which only this pass can
    // measure.
    app.reader_view_height.set(inner.height);
    app.reader_width.set(inner.width);
    if let Some(rendered) = reader_render(app) {
        let rows: std::borrow::Cow<[Line<'static>]> = match rendered.width == inner.width {
            true => std::borrow::Cow::Borrowed(&rendered.lines),
            // Laid out for the width before a resize, until the render
            // at this one lands (`App::claim_note_render`).
            false => std::borrow::Cow::Owned(
                rendered
                    .lines
                    .iter()
                    .flat_map(|l| wrap_line(l, inner.width as usize))
                    .collect(),
            ),
        };
        render_rows(
            frame,
            inner,
            &rows,
            app.reader_scroll,
            &app.reader_max_scroll,
        );
        return;
    }
    let dim = |text: String| Line::from(Span::styled(text, Style::default().fg(colors::OVERLAY)));
    let lines: Vec<Line<'static>> = match app.notes_highlighted_file() {
        None => vec![dim("no file selected".to_string())],
        Some(_) => match held_note_body(app).and_then(|b| b.text.as_ref()) {
            None => vec![dim("reading…".to_string())],
            Some(Err(e)) => vec![Line::from(Span::styled(
                format!("could not read: {e}"),
                Style::default().fg(colors::PEACH),
            ))],
            Some(Ok(text)) => text
                .lines()
                .map(|l| {
                    Line::from(Span::styled(
                        l.to_string(),
                        Style::default().fg(colors::TEXT),
                    ))
                })
                .collect(),
        },
    };
    render_scrolled(
        frame,
        inner,
        &lines,
        app.reader_scroll,
        &app.reader_max_scroll,
    );
}

/// Render the `[3] inspector` pane: the full details of the commit the detail
/// pane's cursor is on -- a two-line masthead (hash, then author · date), the
/// full message (subject bold, body untruncated), and the per-file diffstat --
/// so you can read a commit in full without leaving the dashboard. The block's
/// padding insets every line uniformly, so wrapped prose keeps its left edge
/// instead of falling back to column zero.
/// What the inspector says when it has a selection it cannot draw.
///
/// Three different facts, and none of them may be told as silence: nothing is
/// selected, the commit is real but was not loaded, or the commit is real and
/// simply is not reachable from here.
///
/// The reflog reaches back further than the commit list does, so an old entry
/// names a commit that was not loaded -- a range problem. A remote-only branch
/// tip is a different thing: `git::commits` and `git::graph` walk the union of
/// the *local* branches, so that commit is not outside the range, it is off
/// the refs fleetz reads at all. Saying "range" there would send someone
/// looking for a limit to raise.
fn inspector_empty_text(app: &App) -> String {
    let Some(hash) = app.inspected_hash() else {
        return "no commit selected".to_string();
    };
    match app.history_view {
        HistoryView::Branches => format!("{hash} is not on any local branch"),
        _ => format!("{hash} is outside the loaded commit range"),
    }
}

fn render_inspector(frame: &mut Frame, area: Rect, app: &App) {
    if app.inspector_folded {
        let label = match app.notes_layout() {
            true => NOTES_CARD_LABEL,
            false => "inspector",
        };
        crate::ui::render_folded(
            frame,
            area,
            crate::ui::INSPECTOR_NUM,
            label,
            app.focused_panel == FocusedPanel::Inspector,
        );
        return;
    }
    // A digest line's detail (FZ-45), ahead of the `ci` view's run, which
    // the digest hides.
    if let Some(digest) = app.digest(chrono::Local::now()) {
        crate::ui::digest::render_inspector(frame, area, app, &digest);
        return;
    }
    if app.inspects_ci_run() {
        render_ci_inspector(frame, area, app);
        return;
    }
    if app.notes_wanted() {
        render_notes_card(frame, area, app);
        return;
    }
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
        .title_top(crate::ui::panel_title(
            crate::ui::INSPECTOR_NUM,
            "inspector",
            focused,
        ));
    let inner = block.inner(area);

    let Some(c) = app.inspected_commit() else {
        // Distinguish "nothing selected" from "selected something this pane
        // cannot show". The reflog reaches back further than the commit list
        // does, so picking an old entry names a real commit that simply was
        // not loaded -- saying "no commit selected" there would be a lie.
        let text = inspector_empty_text(app);
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
    let mut file_lines: Vec<Line> = Vec::new();
    if !c.files.is_empty() {
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
        file_lines.push(Line::from(header));

        for f in c.files.iter().take(INSPECTOR_FILE_CAP) {
            file_lines.push(file_stat_line(f));
        }
        if c.files.len() > INSPECTOR_FILE_CAP {
            file_lines.push(more_line(c.files.len() - INSPECTOR_FILE_CAP));
        }
    }

    // Wide enough, the files get a lane of their own beside the message, so
    // a long body no longer pushes them below the fold and a long file list
    // no longer buries the prose. Narrower, they stack as one column.
    let lanes: Vec<(Rect, Vec<Line>)> =
        if inner.width >= INSPECTOR_TWO_LANE_WIDTH && !file_lines.is_empty() {
            let h = Layout::horizontal([Constraint::Percentage(50), Constraint::Min(0)])
                .spacing(2)
                .split(inner);
            vec![(h[0], lines), (h[1], file_lines)]
        } else {
            if !file_lines.is_empty() {
                lines.push(Line::from(""));
                lines.extend(file_lines);
            }
            vec![(inner, lines)]
        };

    // Clamp against the *wrapped* row count, not the raw line count -- long
    // prose wraps to several rows, so a raw count would stop scrolling short of
    // the real end.
    // Recorded back so `App::scroll_inspector` clamps against it, rather than
    // letting the offset run past the end and stall the way back.
    // Wrapped here rather than by the `Paragraph`, so one line is one rendered
    // row and the offset below is an exact row index. See `wrap_line`.
    //
    // One offset scrolls every lane, clamped to the tallest, so the end of
    // either lane is always reachable.
    let wrapped: Vec<(Rect, Vec<Line>)> = lanes
        .into_iter()
        .map(|(rect, lines)| {
            let lines = lines
                .iter()
                .flat_map(|l| wrap_line(l, rect.width as usize))
                .collect();
            (rect, lines)
        })
        .collect();
    let max_scroll = wrapped
        .iter()
        .map(|(rect, lines)| (lines.len() as u16).saturating_sub(rect.height))
        .max()
        .unwrap_or(0);
    app.inspector_max_scroll.set(max_scroll);
    let scroll = app.inspector_scroll.min(max_scroll);
    frame.render_widget(block, area);
    for (rect, lines) in wrapped {
        frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), rect);
    }
}

/// The inspector's body tabs on a run (FZ-64), `[/] JOBS · LOG · SCRIPT`,
/// right of a title that no longer names the body. The shown one wears the
/// `ci` tab's colour, the view whose run it details.
fn ci_body_switcher(app: &App, focused: bool, budget: usize) -> Line<'static> {
    let labels: Vec<String> = crate::app::CiBody::ALL
        .iter()
        .map(|b| b.label().to_uppercase())
        .collect();
    tab_switcher(
        &labels,
        app.ci_body.index(),
        colors::history_view_color(HistoryView::Ci),
        focused,
        budget,
    )
}

/// The inspector while pane 2 shows `ci` (FZ-30f): the highlighted run in
/// the shown body (`ui::ci::inspector_body`), wrapped here so one line is
/// one row, and scrolled by that body's own offset, clamped to what this
/// frame measured.
fn render_ci_inspector(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.focused_panel == FocusedPanel::Inspector;
    let border_color = if focused {
        colors::LAVENDER
    } else {
        colors::SURFACE
    };
    let title = crate::ui::panel_title(crate::ui::INSPECTOR_NUM, "inspector", focused);
    // Two corners and a column of border either side, as pane 2 spends.
    let mut budget = (area.width as usize)
        .saturating_sub(2)
        .saturating_sub(span_width(&title.spans) + 2);
    // The resize grip is painted over the middle of this border after the
    // panes, so the tabs keep to the cells right of it, a column of rule
    // between, short of the corner.
    if let Some((_, grip_right)) = crate::ui::grips::inspector_grip_span(area) {
        let corner = (area.x + area.width).saturating_sub(1);
        budget = budget.min(corner.saturating_sub(grip_right + 1) as usize);
    }
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::new(2, 2, 1, 0))
        .title_top(title);
    // Only a highlighted run has bodies to step (`App::press_bracket`'s own
    // condition); loading, empty or offline, the brackets step pane 2.
    if app.ci_highlighted_run().is_some() {
        block = block.title_top(ci_body_switcher(app, focused, budget));
    }
    let inner = block.inner(area);
    let (body, marked) = crate::ui::ci::inspector_lines(app, chrono::Local::now());
    // Each body line's first wrapped row, and one past the last, so the
    // marked job's lines map to the rows they take at this width.
    let mut row_starts = Vec::with_capacity(body.len() + 1);
    let mut head: Vec<Line> = Vec::new();
    for line in &body {
        row_starts.push(head.len());
        head.extend(wrap_line(line, inner.width as usize));
    }
    row_starts.push(head.len());
    // A held whole log is wrapped once per job and width and kept, and only
    // the rows on screen are handed to the paragraph: up to 5000 lines
    // (`ci::LOG_LINE_CAP`) were re-wrapped and copied on every frame.
    let mut cache = app.ci_log_wrap.borrow_mut();
    let log: &[Line<'static>] = match crate::ui::ci::shown_log(app) {
        Some((job_id, log)) => {
            let fresh = cache
                .as_ref()
                .is_some_and(|w| w.job_id == job_id && w.width == inner.width);
            if !fresh {
                let lines = log
                    .iter()
                    .flat_map(|l| wrap_line(&crate::ui::ci::log_line(l), inner.width as usize))
                    .collect();
                *cache = Some(crate::app::CiLogWrap {
                    job_id,
                    width: inner.width,
                    lines,
                });
            }
            cache.as_ref().map_or(&[][..], |w| &w.lines[..])
        }
        None => &[],
    };
    let total = head.len() + log.len();
    let max_scroll = u16::try_from(total)
        .unwrap_or(u16::MAX)
        .saturating_sub(inner.height);
    app.inspector_max_scroll.set(max_scroll);
    if let Some(lines) = marked
        && app.ci_job_follow.get() == crate::app::JobFollow::Due
    {
        let rows = row_starts[lines.start]..row_starts[lines.end];
        let followed = follow_rows(app.ci_body_scroll(), rows, inner.height).min(max_scroll);
        app.ci_job_follow.set(crate::app::JobFollow::At(followed));
    }
    let scroll = app.ci_body_scroll().min(max_scroll);
    let visible: Vec<Line> = head
        .iter()
        .chain(log.iter())
        .skip(scroll as usize)
        .take(inner.height as usize)
        .cloned()
        .collect();
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(visible), inner);
}

/// The least scroll from `scroll` that shows `rows` in a pane `height` rows
/// tall, or its first row when they are taller than the pane.
fn follow_rows(scroll: u16, rows: std::ops::Range<usize>, height: u16) -> u16 {
    let top = u16::try_from(rows.start).unwrap_or(u16::MAX);
    let bottom = u16::try_from(rows.end).unwrap_or(u16::MAX);
    if top < scroll {
        top
    } else if bottom > scroll.saturating_add(height) {
        bottom.saturating_sub(height).min(top)
    } else {
        scroll
    }
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
/// The scroll offset that brings `block` (an inclusive row range) into view,
/// moving as little as possible.
///
/// Replaces the old rule, which pinned the selected commit to the top of the
/// pane on every selection change. That kept an oversized block's header on
/// screen but made stepping one commit move the entire view, so the pane
/// never felt scrolled, only jumped.
///
/// A block taller than the pane still resolves to its top: the header is what
/// names the commit, and the tail of a long message with no header above it
/// says nothing about whose message it is.
fn bring_into_view(block: (u16, u16), scroll: u16, height: u16) -> u16 {
    let (top, bottom) = block;
    // The same margin the cursor keeps when it walks: a block brought to the
    // very edge is a block you cannot see the next one past.
    let pad = crate::app::scrolloff_for(height);
    // scroll must be at most this for the top to clear the upper margin,
    // and at least this for the bottom to clear the lower one.
    let highest = top.saturating_sub(pad);
    let lowest = (bottom + 1 + pad).saturating_sub(height);
    if scroll > highest {
        return highest;
    }
    if scroll < lowest {
        // A block taller than the pane cannot clear both margins, so its top
        // wins: scrolling to the bottom of an oversized block leaves you
        // looking at the tail of a message with nothing to say whose it is.
        return lowest.min(highest);
    }
    scroll
}

/// `Paragraph` wraps long lines, so a raw line count under-counts. Guards a
/// zero width so a degenerate pane can't divide by zero.
fn wrapped_row_count(lines: &[Line], width: u16) -> u16 {
    let rows: usize = lines.iter().map(|line| line_rows(line, width)).sum();
    rows.min(u16::MAX as usize) as u16
}

/// Rows one line takes in a wrapping paragraph `width` columns wide: the
/// arithmetic `wrapped_row_count` and the click map share.
fn line_rows(line: &Line, width: u16) -> usize {
    let w = width.max(1) as usize;
    let len: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
    if len == 0 { 1 } else { len.div_ceil(w) }
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
///
/// Shared with `App::selected_commit_count`, which bounds the cursor: a
/// second copy of this number let the cursor stop one row short of the list
/// -- or one row past it -- depending on which of the two was edited.
pub(crate) const RECENT_COMMIT_COUNT: usize = 6;

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
    selected_block: &mut Option<(usize, usize)>,
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
    // The two churn columns. `+2` and `+338` have to share a right edge, and
    // so do their deletions. Right-aligning the pair as one blob -- which is
    // what the tail did -- puts every row's `+` in a different column, so the
    // one part of the row that is read by scanning down was the one part left
    // ragged. Same argument as `lane_w` and `hash_w` above, and measured the
    // same way.
    //
    // Over the rows this call will actually draw, not the whole history:
    // `info` shows six of a hundred and fifty, and measuring all of them would
    // reserve width for churn that is not on screen.
    let (add_w, del_w) = repo
        .graph
        .iter()
        .take(limit.unwrap_or(usize::MAX))
        .filter_map(|g| repo.commits.iter().find(|c| c.hash == g.hash))
        .fold((0usize, 0usize), |(add, del), c| {
            (
                add.max(stat_part_width(c.insertions)),
                del.max(stat_part_width(c.deletions)),
            )
        });
    let stat_len = stat_tail_width(add_w, del_w);

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
            // Relative to this function's own output, which the caller offsets
            // to the pane: these lines are a slice of the body, not the body.
            *selected_block = Some((row, row));
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
        //
        // The width comes from the columns rather than from this row, so a row
        // with no churn still reserves them. That is what makes the leader dots
        // stop where the `+` column begins on every row instead of running
        // under it on the rows that happen to have nothing to show.
        let churn = repo.commits.iter().find(|c| c.hash == g.hash);
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
        // The second of the two spaces the budget reserved, between the
        // subject and whatever follows it. It used to be emitted only
        // alongside the dots, so a row whose subject filled its whole budget
        // -- leaving no dots to draw -- came out one column short and slid its
        // churn tail left by one, breaking the column against the rows that
        // did have dots. Which rows truncate depends on the pane width, which
        // is why the tail looked aligned at one size and ragged at another.
        //
        // Withheld only when the row is already over budget, the case
        // `dot_count`'s saturation absorbs: a graph deep enough to overrun the
        // pane should not be handed another column to overrun it with.
        let fits = used + msg_len + 2 + right <= width;

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
        if fits {
            row.push(Span::raw(" "));
        }
        if dot_count > 0 {
            row.push(Span::styled(
                "·".repeat(dot_count),
                Style::default().fg(colors::SURFACE),
            ));
        }
        if stat_len > 0 {
            row.push(Span::raw(" "));
            row.extend(stat_spans(churn, add_w, del_w));
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
pub(crate) fn wash(spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
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
        // Nodes are plain text, not lane-coloured: a commit belongs to the
        // history rather than to whichever lane it landed in, and colouring
        // them made the node column change hue every time a lane was reused.
        let glyph_color = match cell.is_node() {
            true => colors::TEXT,
            false => colors::graph_lane_color(cell.style as usize),
        };
        spans.push(Span::styled(
            cell.glyph.to_string(),
            Style::default().fg(glyph_color),
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
            let color = match ch {
                ' ' => colors::SURFACE,
                // `git log --graph` marks every commit with `*`. A node is
                // part of the history, not part of whichever lane it landed
                // in, so it keeps the plain text colour whatever its column.
                // The compact graph has always done this (`Cell::is_node`);
                // this is the expanded renderer catching up.
                '*' => colors::TEXT,
                _ => {
                    // git writes two columns per lane: the glyph (|, \, /, _) and
                    // a horizontal run beside it (space or connector). Palette must
                    // be indexed by lane, not column, or adjacent lanes land two
                    // apart and the first six lanes only use three of six colours.
                    colors::graph_lane_color(i / 2)
                }
            };
            Span::styled(ch.to_string(), Style::default().fg(color))
        })
        .collect()
}

/// `+12 -3` split so additions and deletions keep their own colours, each half
/// right-aligned in its own column.
///
/// A half the row does not have is drawn as blanks rather than skipped: the
/// blanks are what hold the other half's column when one row has deletions and
/// the row above it does not. `None` -- a graph row deeper than the commit
/// window -- is the same case with both halves missing.
///
/// No trailing space: the caller budgeted the row against exactly
/// `stat_tail_width`, so an extra column here pushes the tail onto a line of
/// its own.
fn stat_spans(
    commit: Option<&crate::model::Commit>,
    add_w: usize,
    del_w: usize,
) -> Vec<Span<'static>> {
    let (insertions, deletions) = commit.map_or((0, 0), |c| (c.insertions, c.deletions));
    let mut spans = Vec::new();
    if add_w > 0 {
        let text = match insertions {
            0 => String::new(),
            n => format!("+{n}"),
        };
        spans.push(Span::styled(
            format!("{text:>add_w$}"),
            Style::default().fg(colors::DIFF_ADD),
        ));
    }
    if del_w > 0 {
        if add_w > 0 {
            spans.push(Span::raw(" "));
        }
        let text = match deletions {
            0 => String::new(),
            n => format!("-{n}"),
        };
        spans.push(Span::styled(
            format!("{text:>del_w$}"),
            Style::default().fg(colors::DIFF_DEL),
        ));
    }
    spans
}

/// Columns a `+N` or `-N` token occupies, zero when the count is zero and the
/// token is not drawn at all.
fn stat_part_width(n: u32) -> usize {
    match n {
        0 => 0,
        n => 1 + n.to_string().chars().count(),
    }
}

/// Columns the churn tail occupies, separator included. Zero when no row in
/// the list has any churn, so such a list spends no width on it.
fn stat_tail_width(add_w: usize, del_w: usize) -> usize {
    match (add_w, del_w) {
        (0, 0) => 0,
        (only, 0) | (0, only) => only,
        (add, del) => add + 1 + del,
    }
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
/// Whether this line of `git log --graph`'s expanded output is the `commit
/// <hash>` line that opens `hash`'s block.
fn opens_commit(raw: &str, hash: &str) -> bool {
    !hash.is_empty() && commit_hash_at(raw) == Some(hash)
}

/// The characters git draws the lanes with. Space is one of them, so the
/// split below also eats a message body's indentation.
const GRAPH_CHARS: [char; 6] = ['*', '|', '\\', '/', '_', ' '];

/// Split a raw `git log --graph` row into its lane prefix and the text after
/// it. One function so the marker, the block bounds and the renderer cannot
/// disagree about where the drawing stops and the text starts.
fn split_graph_prefix(raw: &str) -> (&str, &str) {
    let split = raw
        .char_indices()
        .find(|(_, c)| !GRAPH_CHARS.contains(c))
        .map_or(raw.len(), |(i, _)| i);
    raw.split_at(split)
}

/// The hash of the commit this row opens, or `None` for every other row.
///
/// The hash is validated, not just located: a substring test for "commit "
/// matched message bodies, and this repo's own log says "that commit had the
/// mechanism" and "one wheel notch jumped a whole commit block". Every one of
/// those opened a phantom block, so the graph reported thirty block starts for
/// twenty-five commits and the marker indexed past the commit it was on.
fn commit_hash_at(raw: &str) -> Option<&str> {
    let word = split_graph_prefix(raw)
        .1
        .strip_prefix("commit ")?
        .split_whitespace()
        .next()?;
    // Decorations follow after a space, so the first word is the bare hash.
    (word.len() >= 4 && word.chars().all(|c| c.is_ascii_hexdigit())).then_some(word)
}

/// Hold a row open to `width` so a selection wash covers the line rather than
/// stopping where its text does, which reads as a highlight on the words
/// instead of a cursor on the row.
fn pad_to(mut spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let used = span_width(&spans);
    if used < width {
        spans.push(Span::raw(" ".repeat(width - used)));
    }
    spans
}

/// Split `text` into chunks that each fit `budget` columns.
///
/// Breaks at spaces where it can and mid-token where it cannot, because a
/// space-only break never terminates on a token wider than the pane, and a
/// long URL in a commit message is the common case. Always returns at least
/// one chunk: git's blocks use blank lines as spacing, and dropping them
/// would close the gaps between commits.
fn wrap_text(text: &str, budget: usize) -> Vec<String> {
    if budget == 0 {
        return vec![String::new()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut len = 0usize;
    for word in text.split_inclusive(' ') {
        let wlen = word.chars().count();
        if len > 0 && len + wlen > budget {
            out.push(std::mem::take(&mut line));
            len = 0;
        }
        if wlen > budget {
            for ch in word.chars() {
                if len == budget {
                    out.push(std::mem::take(&mut line));
                    len = 0;
                }
                line.push(ch);
                len += 1;
            }
        } else {
            line.push_str(word);
            len += wlen;
        }
    }
    out.push(line);
    out
}

/// Rewrite git's `Date:` row as an age measured now, or `None` for any other
/// row.
///
/// `git::graph_expanded` asks for `--date=iso-strict` so this can: a relative
/// date is frozen at the moment git rendered it, and that output is only
/// refetched when the repo's WorkKey moves. An idle repo sat on "3 minutes
/// ago" all night while `commits` and `reflog` beside it correctly said "16h
/// ago", because those two carry a timestamp and format it per frame. This is
/// the same trade `format_age` documents, applied to the one surface that
/// still had git do the formatting.
fn refresh_date_line(rest: &str) -> Option<String> {
    let stamp = rest.strip_prefix("Date:")?.trim();
    let parsed = chrono::DateTime::parse_from_rfc3339(stamp).ok()?;
    // Git's own column: `Date:` and three spaces, so the value stays under
    // the one on the `Author:` row above it.
    Some(format!(
        "Date:   {}",
        format_age(parsed.with_timezone(&chrono::Utc))
    ))
}

/// Re-flow one line's spans to `width`, keeping each chunk's styling.
///
/// The same word-aware break `wrap_text` gives the graph, applied to a line
/// that is already several spans. Wrapping in the builder rather than leaving
/// it to `Paragraph` makes one line one rendered row, which is what lets the
/// scroll offset be an exact row index: `wrapped_row_count` counts
/// `ceil(len / width)` while `Paragraph` breaks at spaces, so it under-reports
/// every wrapped paragraph and the tail of a long commit body could not be
/// scrolled to.
pub fn wrap_line<'a>(line: &Line<'a>, width: usize) -> Vec<Line<'a>> {
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    if width == 0 || text.chars().count() <= width {
        return vec![line.clone()];
    }
    // One entry per character, so a chunk can be cut back into styled spans.
    // `wrap_text` neither adds nor drops characters, so the offsets line up.
    let styled: Vec<(char, Style)> = line
        .spans
        .iter()
        .flat_map(|s| s.content.chars().map(|c| (c, s.style)))
        .collect();

    let mut out = Vec::new();
    let mut at = 0usize;
    for chunk in wrap_text(&text, width) {
        let n = chunk.chars().count();
        let mut spans: Vec<Span<'a>> = Vec::new();
        for &(ch, style) in &styled[at.min(styled.len())..(at + n).min(styled.len())] {
            match spans.last_mut() {
                // Runs of one style stay one span, so a wrapped line does not
                // become one span per character.
                Some(last) if last.style == style => last.content.to_mut().push(ch),
                _ => spans.push(Span::styled(ch.to_string(), style)),
            }
        }
        at += n;
        out.push(Line::from(spans));
    }
    out
}

/// One raw `git log --graph` line, as the rows it occupies at this width.
///
/// Returns a `Vec` because a wrapped line is several rows and the caller
/// needs the true count: the pane's scroll offset is a row index, so one
/// source line has to stay one rendered row. Letting the `Paragraph` wrap
/// instead would split row index from line index, which is exactly the split
/// the computed graph exists to avoid. Wrapping earlier avoids the problem
/// that wrapping later creates.
fn expanded_graph_lines(raw: &str, selected_hash: &str, width: usize) -> Vec<Line<'static>> {
    let (prefix, rest) = split_graph_prefix(raw);
    // Git's `Date:` row, recomputed as of now. See `refresh_date_line`.
    let refreshed = refresh_date_line(rest);
    let rest = refreshed.as_deref().unwrap_or(rest);

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

    let budget = width.saturating_sub(2 + prefix.chars().count());
    wrap_text(rest, budget)
        .into_iter()
        .map(|chunk| {
            // The caret column is held open on every row: `mark_cursor_row`
            // fills it in on the one row the cursor is on, and a column that
            // appeared only there would shift the whole drawing sideways.
            let mut spans = vec![Span::styled(
                "  ".to_string(),
                Style::default().fg(colors::SAPPHIRE),
            )];
            spans.extend(graph_prefix_spans_raw(prefix));
            spans.push(Span::styled(chunk, style));
            Line::from(spans)
        })
        .collect()
}

/// The pane's headliner: the view's name, and a divider under it that fades
/// out of the name's own colour.
///
/// Two rows, and pinned ones, so it costs the scrolling body one line at every
/// pane height. Worth it because the headliner is the thing the whole body
/// below it is an answer to, and as a single line of bold text sitting straight
/// on that body it read as the body's first line rather than as its title.
///
/// No box and no leading bar. Both were tried. A box drew two lines whose only
/// job was to meet each other, and cost a third pinned row to do it; a leading
/// bar then indented the name by two columns, so the one line that heads the
/// body did not line up with any line of it. The divider alone says the same
/// thing and leaves the name where the eye already is.
///
/// The divider starts in the view's own colour -- the same one its tab wears up
/// in the border, so the two agree about which view you are in -- and fades
/// into `SURFACE` across the pane: under the name it is part of the label, by
/// the far edge it is a divider, and structure is what that colour is for.
fn headliner(view: HistoryView, width: usize) -> Vec<Line<'static>> {
    let on = colors::history_view_color(view);
    // Per cell, through the same `blend` the spinner walks its palette with.
    // That one interpolates over time and this one over distance, but it is the
    // same arithmetic and there is no reason to have two of it. A cell each is
    // a couple of hundred spans on a wide pane, which is nothing next to the
    // commit rows underneath.
    let divider: Vec<Span<'static>> = (0..width)
        .map(|i| {
            Span::styled(
                "─".to_string(),
                Style::default().fg(crate::ui::blend(
                    on,
                    colors::SURFACE,
                    i as u32 + 1,
                    width as u32,
                )),
            )
        })
        .collect();

    vec![
        Line::from(Span::styled(
            view_name(view),
            Style::default().fg(on).add_modifier(Modifier::BOLD),
        )),
        Line::from(divider),
    ]
}

/// The view's name as both the tab strip and the pane's headliner draw it:
/// capitals, one cell per letter.
///
/// One form for both, so the tab you aim at and the title that confirms you
/// landed are the same string as well as the same colour. Capitals cost the
/// strip nothing -- the label is the same number of cells -- so its fit budget,
/// which drops the inactive tabs when the labels outgrow the border, is
/// untouched. Letterspacing was tried and dropped: it read as decoration in a
/// pane whose every other line is dense, and the box around the headliner
/// already does the work of setting it apart.
fn view_name(view: HistoryView) -> String {
    view.label().to_uppercase()
}

/// A bold section header inside the detail pane (e.g. "branches", "history").
/// The view switcher, drawn in the pane's top border rather than in its body.
///
/// It belongs in the frame because it answers "what am I looking at", and the
/// body scrolls: a strip at the top of the body is gone by the second page,
/// which is exactly when the question gets asked. The doc comment this
/// replaces claimed it was pinned. It never was.
///
/// The keys ride along with it, since a switcher that does not say how to
/// switch answers half the question. `[` and `]` are drawn as two lit keys
/// either side of a dimmed `/`, so the pair reads as "either bracket" rather
/// than as the single `/` key -- which is bound to search.
///
/// `budget` is the border width left over after the pane's own title. Too
/// tight for all four and it shows the active one alone, which is the half
/// that cannot be dropped.
fn view_switcher(active: HistoryView, focused: bool, budget: usize) -> Line<'static> {
    let labels: Vec<String> = HistoryView::ALL.iter().map(|v| view_name(*v)).collect();
    let index = HistoryView::ALL
        .iter()
        .position(|v| *v == active)
        .unwrap_or(0);
    tab_switcher(
        &labels,
        index,
        colors::history_view_color(active),
        focused,
        budget,
    )
}

/// A border's `[/]` tab line: pane 2's views (`view_switcher`) and the CI
/// inspector's bodies (FZ-64) share it, so the two read and collapse alike.
/// `labels` are drawn as given, `labels[active]` bold in `on`; too tight for
/// all of them within `budget` and it shows the active one alone.
fn tab_switcher(
    labels: &[String],
    active: usize,
    on: ratatui::style::Color,
    focused: bool,
    budget: usize,
) -> Line<'static> {
    // The active tab wears its own view's colour in either state: which mode
    // the pane is in is worth reading from across the screen, and it is the
    // one thing here that a dimmed switcher must still say. Everything around
    // it -- the keys, the separators, the inactive tabs -- drops back when the
    // pane is not focused, so a bright switcher never pulls the eye to a pane
    // you are not in.
    let (key, slash, dot, off) = if focused {
        (
            colors::TEXT,
            colors::OVERLAY,
            colors::SURFACE,
            colors::SUBTEXT,
        )
    } else {
        (
            colors::OVERLAY,
            colors::SURFACE,
            colors::SURFACE,
            colors::SURFACE,
        )
    };
    let bold = |c| Style::default().fg(c).add_modifier(Modifier::BOLD);

    let mut spans = vec![
        Span::raw(" "),
        Span::styled("[", bold(key)),
        Span::styled("/", Style::default().fg(slash)),
        Span::styled("]", bold(key)),
        Span::raw(" "),
    ];

    // " [/] " + every label + " · " between each + a trailing space.
    let width: usize = labels.iter().map(|l| l.chars().count()).sum();
    let full = 5 + width + 3 * labels.len().saturating_sub(1) + 1;

    if full <= budget {
        for (i, label) in labels.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" · ", Style::default().fg(dot)));
            }
            spans.push(match i == active {
                true => Span::styled(label.clone(), bold(on)),
                false => Span::styled(label.clone(), Style::default().fg(off)),
            });
        }
    } else if let Some(label) = labels.get(active) {
        spans.push(Span::styled(label.clone(), bold(on)));
    }
    spans.push(Span::raw(" "));
    Line::from(spans).right_aligned()
}

/// A worktree's path as the `worktrees` section draws it: relative to the
/// repo when it lives inside it (`.claude/worktrees/issue-45`), `.` when it
/// is the repo, otherwise unchanged. Both are the `~`-collapsed spellings the
/// pane already prints, so this is a comparison of strings, never of disks.
/// A sibling that merely shares the prefix (`fleetz-hotfix` beside `fleetz`)
/// is outside.
fn path_within_repo(path: &str, repo_path: &str) -> String {
    let repo = repo_path.trim_end_matches('/');
    if repo.is_empty() {
        return path.to_string();
    }
    if path == repo {
        return ".".to_string();
    }
    match path
        .strip_prefix(repo)
        .and_then(|rest| rest.strip_prefix('/'))
    {
        Some(rest) if !rest.is_empty() => rest.to_string(),
        _ => path.to_string(),
    }
}

/// Widest a worktree's branch column grows before the name is cut.
const WORKTREE_NAME_MAX: usize = 32;

/// The Info view's `worktrees` section: the label, then one row per linked
/// worktree with its branch (or `detached @ <sha>`), dirty count, ahead and
/// behind, the age of its last commit, and its path. Empty when the repo has
/// no linked worktree.
///
/// Ahead and behind come from the main checkout's `branches`: a linked
/// worktree's branch is a local branch of the same repository. A detached
/// worktree has nothing to compare, so that column reads `·`.
///
/// Every counter is spelled by `model::counters_text` and coloured by
/// `badges::counter_color`, the helpers the repo list's status token is
/// built from, so a worktree's `3!` is the repo list's `3!` in the repo
/// list's colour and cannot drift from it.
///
/// Each row fits `width`, the pane's: the path is cut to what the columns
/// before it leave, and a row whose columns alone are wider is clipped
/// (`clip_spans`), so one row is always one rendered row in the wrapping
/// body.
fn worktree_rows(repo: &RepoStatus, width: usize) -> Vec<Line<'static>> {
    if repo.worktrees.is_empty() {
        return Vec::new();
    }
    let token = |behind: u32, ahead: u32, dirty: u32| -> Span<'static> {
        Span::styled(
            crate::model::counters_text(behind, ahead, dirty),
            Style::default().fg(badges::counter_color(behind, ahead, dirty)),
        )
    };
    let unknown = |text: &str| Span::styled(text.to_string(), Style::default().fg(colors::OVERLAY));
    let repo_path = repo.path.as_deref().unwrap_or_default();

    struct Row {
        name: String,
        dirty: Option<Span<'static>>,
        sync: Option<Span<'static>>,
        age: String,
        path: String,
    }
    let rows: Vec<Row> = repo
        .worktrees
        .iter()
        .map(|wt| {
            let branch = (!wt.detached)
                .then(|| repo.branches.iter().find(|b| b.name == wt.branch))
                .flatten();
            let level =
                branch.is_some_and(|b| b.upstream.is_some() && b.behind == 0 && b.ahead == 0);
            let dirty = match wt.dirty {
                None => Some(unknown("?")),
                Some(n) if n > 0 => Some(token(0, 0, n)),
                Some(_) if wt.detached => Some(unknown("·")),
                Some(_) if level => Some(token(0, 0, 0)),
                Some(_) => None,
            };
            let sync = match branch {
                None => Some(unknown("·")),
                Some(b) if b.upstream.is_none() => Some(unknown("no upstream")),
                Some(b) if b.behind > 0 || b.ahead > 0 => Some(token(b.behind, b.ahead, 0)),
                Some(_) => None,
            };
            let name = if wt.detached {
                format!("detached @ {}", wt.head)
            } else {
                wt.branch.clone()
            };
            Row {
                name: crate::ui::truncate(&name, WORKTREE_NAME_MAX),
                dirty,
                sync,
                age: wt.head_at.map_or_else(|| "·".to_string(), format_age),
                path: path_within_repo(&wt.path, repo_path),
            }
        })
        .collect();

    let width_of =
        |span: &Option<Span<'static>>| span.as_ref().map_or(0, |s| s.content.chars().count());
    let name_w = rows
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(0);
    let dirty_w = rows.iter().map(|r| width_of(&r.dirty)).max().unwrap_or(0);
    let sync_w = rows.iter().map(|r| width_of(&r.sync)).max().unwrap_or(0);
    let age_w = rows
        .iter()
        .map(|r| r.age.chars().count())
        .max()
        .unwrap_or(0);

    let mut lines = vec![Line::from(section_label("worktrees"))];
    for row in rows {
        let mut spans = vec![Span::raw("  ")];
        spans.extend(pad_to(
            vec![Span::styled(row.name, Style::default().fg(colors::TEXT))],
            name_w,
        ));
        for (cell, w) in [(row.dirty, dirty_w), (row.sync, sync_w)] {
            if w == 0 {
                continue;
            }
            spans.push(Span::raw("  "));
            spans.extend(pad_to(cell.into_iter().collect(), w));
        }
        spans.push(Span::styled(
            format!("  {:>age_w$}", row.age),
            Style::default().fg(colors::OVERLAY),
        ));
        let path_budget = width.saturating_sub(span_width(&spans) + 2);
        if path_budget > 0 {
            spans.push(Span::styled(
                format!("  {}", crate::ui::truncate(&row.path, path_budget)),
                Style::default().fg(colors::OVERLAY),
            ));
        }
        lines.push(Line::from(clip_spans(spans, width)));
    }
    lines
}

/// `spans` cut to `width` columns, the span that crosses the edge ending in
/// the pane's ellipsis and everything after it dropped. For a row in the
/// wrapping body that must stay one rendered row: the commit list's row tops
/// are counted in source lines, so a row that wrapped would put every commit
/// below it lower than recorded.
pub(crate) fn clip_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let mut left = width;
    let mut out = Vec::new();
    for span in spans {
        let w = span.content.chars().count();
        if w <= left {
            left -= w;
            out.push(span);
            continue;
        }
        if left > 0 {
            out.push(Span::styled(
                crate::ui::truncate(&span.content, left),
                span.style,
            ));
        }
        break;
    }
    out
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
    /// Flatten a rendered row to its text, so a test can assert on what the
    /// eye sees rather than on span structure.
    fn row_text(row: &Line<'static>) -> String {
        row.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn branch_rows(b: &crate::model::BranchStatus) -> Vec<Line<'static>> {
        let mut shown = std::collections::HashSet::new();
        branch_row_lines(b, &[], &mut shown)
    }

    fn branch_fixture(
        name: &str,
        upstream: Option<&str>,
        behind: u32,
        is_head: bool,
    ) -> crate::model::BranchStatus {
        crate::model::BranchStatus {
            name: name.to_string(),
            upstream: upstream.map(|u| u.to_string()),
            behind,
            ahead: 0,
            is_head,
            tip: "a1b2c3d".to_string(),
        }
    }

    /// The failure this closes: `info` and `branches` would each grow their
    /// own copy of this row, and two copies of a row vocabulary drift exactly
    /// the way two copies of layout arithmetic do. One function, called by
    /// both.
    #[test]
    fn a_branch_row_says_head_upstream_and_divergence_once() {
        let b = branch_fixture("feature/who-goes-first", Some("origin/wip"), 19, true);
        let text = row_text(&branch_rows(&b)[0]);
        assert!(text.starts_with("* "), "HEAD marker missing: {text:?}");
        assert!(text.contains("-> origin/wip"), "upstream missing: {text:?}");
        assert!(text.contains("19 behind"), "divergence missing: {text:?}");
    }

    /// The arrow is for the odd one out. Repeating `origin/x` after every `x`
    /// down the whole list is noise that would bury the case worth seeing.
    #[test]
    fn a_branch_tracking_its_own_name_stays_bare() {
        let b = branch_fixture("main", Some("origin/main"), 0, false);
        let text = row_text(&branch_rows(&b)[0]);
        assert!(
            !text.contains("->"),
            "arrow drawn for a matching upstream: {text:?}"
        );
        assert!(text.contains('✓'), "level branch lost its tick: {text:?}");
    }

    /// An untracked branch rendering bare reads as "fine" when it means "not
    /// on any remote" -- the same trap as a not-cloned repo whose counters are
    /// all zero.
    #[test]
    fn a_branch_with_no_upstream_says_so() {
        let b = branch_fixture("beta", None, 0, false);
        let text = row_text(&branch_rows(&b)[0]);
        assert!(
            text.contains("no upstream"),
            "silence stood in for a state: {text:?}"
        );
    }

    fn busy_repo() -> RepoStatus {
        let mut r = cloned_repo();
        r.stash_count = 2;
        r.branches = vec![crate::model::BranchStatus {
            name: "main".to_string(),
            upstream: Some("origin/main".to_string()),
            behind: 0,
            ahead: 0,
            is_head: true,
            tip: "a1b2c3d".to_string(),
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

    /// `pane_text` for the `ci` view, with the recorded runs fixture landed
    /// for the fixture repo.
    fn ci_view_text(width: u16) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), width, 44);
        app.history_view = HistoryView::Ci;
        let (key, owner, name) = crate::ci::target(&app.repos[0]).expect("a GitHub repo");
        let request = app.claim_ci_poll(key.clone(), owner, name);
        let runs =
            crate::ci::parse_runs(include_str!("../ci/fixtures/runs.json")).expect("fixture");
        app.apply_ci(
            crate::ci::CiAnswer {
                key,
                attempt: request.attempt,
                runs: Some(crate::ci::Fetched::Fresh {
                    data: runs,
                    etag: None,
                }),
                jobs: None,
                log: None,
                full_log: None,
                failure: None,
                budget: None,
            },
            chrono::Local::now(),
        );
        let mut term = Terminal::new(TestBackend::new(width, 30)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        (0..30u16)
            .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
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

    /// The color the `2` of the detail pane's ` [2] ` title is drawn in.
    fn detail_number_color(focused_panel: FocusedPanel) -> Option<ratatui::style::Color> {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let width = 92;
        let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), width, 44);
        app.focused_panel = focused_panel;
        let mut term = Terminal::new(TestBackend::new(width, 30)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        let row: String = (0..width).map(|x| buf[(x, 0)].symbol()).collect();
        let x = row.find("[2]").expect("pane number not drawn") as u16 + 1;
        buf[(x, 0)].style().fg
    }

    /// The detail pane builds its own title and drew its unfocused number in
    /// SURFACE, a shade darker than every other pane's, which is exactly
    /// when the number is the key you need to read.
    #[test]
    fn the_unfocused_detail_number_matches_the_other_panes() {
        let number_fg = |focused| {
            crate::ui::panel_title(2, "detail", focused).spans[0]
                .style
                .fg
        };
        assert_eq!(
            detail_number_color(FocusedPanel::RepoList),
            number_fg(false),
            "unfocused number differs from panel_title's"
        );
        assert_eq!(detail_number_color(FocusedPanel::Detail), number_fg(true));
    }

    /// The failure this closes: a remote-only branch's tip is not in the union
    /// of local branches that `git::commits` walks, so the inspector had
    /// nothing to draw. Reporting it as a *range* problem would send someone
    /// looking for a limit to raise; it is a reachability problem.
    #[test]
    fn an_unreachable_branch_tip_says_it_is_not_on_a_local_branch() {
        let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 92, 44);
        app.history_view = HistoryView::Branches;
        app.repos[0].branches = vec![branch_fixture("gone", None, 0, false)];
        app.repos[0].branches[0].tip = "beefbee".to_string();
        app.set_detail_selected(0);
        assert_eq!(
            inspector_empty_text(&app),
            "beefbee is not on any local branch"
        );
    }

    /// The reflog keeps its own wording: there the commit really is outside a
    /// range that could be widened.
    #[test]
    fn an_unloaded_reflog_commit_still_reports_a_range() {
        let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 92, 44);
        app.history_view = HistoryView::Reflog;
        app.repos[0].reflog = vec![crate::model::ReflogEntry {
            hash: "beefbee".to_string(),
            action: "commit".to_string(),
            description: "x".to_string(),
        }];
        app.set_detail_selected(0);
        assert_eq!(
            inspector_empty_text(&app),
            "beefbee is outside the loaded commit range"
        );
    }

    /// A repo with two local branches and three remotes, one of which nothing
    /// local claims, rendered in the branches view.
    fn branches_text(selected: usize) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut r = busy_repo();
        r.branches = vec![
            branch_fixture("main", Some("origin/main"), 0, true),
            branch_fixture("beta", None, 0, false),
        ];
        r.remote_branches = ["origin/main", "origin/beta", "origin/nobody"]
            .iter()
            .map(|n| crate::model::RemoteBranch {
                name: (*n).to_string(),
                tip: "a1b2c3d".to_string(),
            })
            .collect();
        r.remote_only_branches = vec!["origin/nobody".to_string()];

        let width = 92u16;
        let mut app = crate::app::App::new(vec![r], "t".to_string(), width, 44);
        app.history_view = HistoryView::Branches;
        app.set_detail_selected(selected);
        let mut term = Terminal::new(TestBackend::new(width, 30)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        (0..30u16)
            .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The branches view lists every remote, not only the unfetched ones: that
    /// distinction is a marker on a row, not a reason to omit the row.
    #[test]
    fn the_branches_view_lists_local_and_remote_sections() {
        let out = branches_text(0);
        assert!(out.contains("local branches"), "no local section:\n{out}");
        assert!(out.contains("remotes"), "no remotes section:\n{out}");
        assert!(out.contains("main"), "a local branch was dropped:\n{out}");
        assert!(
            out.contains("origin/beta"),
            "a fetched remote was dropped:\n{out}"
        );
        assert!(
            out.contains("origin/nobody"),
            "an unfetched remote was dropped:\n{out}"
        );
    }

    /// A remote nothing local claims is marked; one a local branch claims is
    /// not. Silence must not stand in for either state.
    #[test]
    fn only_unclaimed_remotes_are_marked_not_fetched() {
        let out = branches_text(0);
        let marked: Vec<&str> = out.lines().filter(|l| l.contains("not fetched")).collect();
        assert_eq!(
            marked.len(),
            1,
            "expected exactly one marked row: {marked:?}"
        );
        assert!(
            marked[0].contains("origin/nobody"),
            "a claimed remote was marked not fetched: {marked:?}"
        );
    }

    /// The cursor walks past the local branches into the remotes: the two
    /// sections are one selectable list, in the order they are drawn.
    #[test]
    fn the_cursor_walks_from_local_branches_into_remotes() {
        let out = branches_text(2);
        let carets: Vec<&str> = out.lines().filter(|l| l.contains("▸ ")).collect();
        assert_eq!(carets.len(), 1, "expected one caret: {carets:?}");
        assert!(
            carets[0].contains("origin/main"),
            "index 2 should be the first remote: {carets:?}"
        );
    }

    /// The commit rows of the `commits` view, border stripped, for a list with
    /// the given per-commit churn.
    fn commit_rows(churn: &[(u32, u32)]) -> Vec<String> {
        commit_rows_at(churn, 100, |i| format!("chore: an ordinary subject {i}"))
    }

    /// The same at a chosen width, with the subject under the caller's control
    /// so a test can force the truncation case.
    fn commit_rows_at(
        churn: &[(u32, u32)],
        width: u16,
        subject: impl Fn(usize) -> String,
    ) -> Vec<String> {
        let mut r = cloned_repo();
        r.graph = (0..churn.len())
            .map(|i| crate::model::GraphLine {
                lanes: vec![crate::graph::commit_cell()],
                hash: format!("c{i:06}"),
                is_merge: false,
                refs: vec![],
                author: "Gaurgle".to_string(),
                message: subject(i),
            })
            .collect();
        r.commits = churn
            .iter()
            .enumerate()
            .map(|(i, (insertions, deletions))| crate::model::Commit {
                hash: format!("c{i:06}"),
                author: "Gaurgle".to_string(),
                date: Some(chrono::Utc::now()),
                message: subject(i),
                body: String::new(),
                insertions: *insertions,
                deletions: *deletions,
                files: vec![],
            })
            .collect();

        let height = 24u16;
        let mut app = crate::app::App::new(vec![r], "t".to_string(), width, height);
        app.history_view = HistoryView::Commits;
        let app = app;
        let mut term =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        (0..height)
            .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .filter(|row| row.contains(" Gau "))
            .collect()
    }

    /// Column positions, counted in characters. The pane is full of multibyte
    /// box-drawing and the leader dot, so `str::find`'s byte offsets do not
    /// answer "which column", which is the only question these tests ask.
    fn columns(row: &str) -> Vec<char> {
        row.chars().collect()
    }

    /// The column a right-aligned `+N` / `-N` token ends at, which is the edge
    /// that has to agree between rows. Their *starts* differ by design: that is
    /// what right-aligning them means.
    fn token_end(row: &str, sign: char) -> Option<usize> {
        let cols = columns(row);
        let start = cols.iter().position(|c| *c == sign)?;
        let len = cols[start + 1..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        Some(start + 1 + len)
    }

    /// `+2` and `+338` share a right edge, and so do their deletions. Aligning
    /// the pair as one right-aligned blob -- which is what it did -- put every
    /// row's `+` in a different column, leaving the one part of the row that is
    /// read by scanning down the page the one part left ragged.
    #[test]
    fn the_churn_columns_share_a_right_edge() {
        let rows = commit_rows(&[(2, 2), (338, 21), (104, 8), (7, 3)]);
        assert_eq!(rows.len(), 4, "fixture did not draw four commit rows");

        let adds: Vec<usize> = rows.iter().filter_map(|r| token_end(r, '+')).collect();
        let dels: Vec<usize> = rows.iter().filter_map(|r| token_end(r, '-')).collect();
        assert_eq!(adds.len(), 4, "a row lost its insertions");
        assert_eq!(dels.len(), 4, "a row lost its deletions");
        assert!(
            adds.windows(2).all(|w| w[0] == w[1]),
            "insertions ragged across rows: {adds:?}"
        );
        assert!(
            dels.windows(2).all(|w| w[0] == w[1]),
            "deletions ragged across rows: {dels:?}"
        );
    }

    /// A row with no churn still spends the columns, so the leader dots stop
    /// where the `+` column begins rather than running under it on whichever
    /// rows happen to have nothing to show.
    #[test]
    fn a_row_without_churn_still_holds_the_columns() {
        let rows = commit_rows(&[(338, 21), (0, 0)]);
        let dots_end = |r: &str| {
            columns(r)
                .iter()
                .rposition(|c| *c == '·')
                .expect("row drew no leader dots")
        };
        assert_eq!(
            dots_end(&rows[0]),
            dots_end(&rows[1]),
            "the churn-less row let its dots run into the columns"
        );
    }

    /// One-sided churn keeps the other column's blanks, which is what holds
    /// the alignment when a delete-only commit sits under an add-only one.
    #[test]
    fn one_sided_churn_keeps_the_other_column() {
        let rows = commit_rows(&[(338, 21), (7, 0), (0, 12)]);
        let adds: Vec<usize> = rows.iter().filter_map(|r| token_end(r, '+')).collect();
        let dels: Vec<usize> = rows.iter().filter_map(|r| token_end(r, '-')).collect();
        assert_eq!(adds.len(), 2, "expected two rows with insertions");
        assert_eq!(dels.len(), 2, "expected two rows with deletions");
        assert_eq!(adds[0], adds[1], "insertions moved: {adds:?}");
        assert_eq!(dels[0], dels[1], "deletions moved: {dels:?}");
    }

    /// The bug the columns alone did not fix. The row budget reserves a space
    /// between the subject and what follows it, but it used to be drawn only
    /// alongside the leader dots -- so a subject that filled its whole budget,
    /// leaving no dots, came out a column short and slid its churn left by one.
    /// Whether a subject truncates depends on the pane width, which is why the
    /// tail looked aligned at one size and ragged at another.
    #[test]
    fn a_truncated_subject_keeps_the_churn_columns() {
        // Long enough to truncate at this width, alternating with short ones
        // so both shapes sit in the same list.
        let subject = |i: usize| match i % 2 {
            0 => format!("chore: release v0.3.{i}"),
            _ => format!("fix: make the behind count explicable where it is shown {i}"),
        };
        let rows = commit_rows_at(&[(2, 2), (338, 21), (2, 2), (104, 8)], 64, subject);
        assert_eq!(rows.len(), 4, "fixture did not draw four commit rows");
        assert!(
            rows.iter().any(|r| r.contains('…')),
            "no subject truncated, so this width does not exercise the case"
        );

        let adds: Vec<usize> = rows.iter().filter_map(|r| token_end(r, '+')).collect();
        let dels: Vec<usize> = rows.iter().filter_map(|r| token_end(r, '-')).collect();
        assert!(
            adds.windows(2).all(|w| w[0] == w[1]),
            "insertions ragged across truncated rows: {adds:?}"
        );
        assert!(
            dels.windows(2).all(|w| w[0] == w[1]),
            "deletions ragged across truncated rows: {dels:?}"
        );
    }

    /// The same thing stated as the invariant underneath it: every commit row
    /// draws to exactly the pane's width, dots or no dots.
    #[test]
    fn every_commit_row_reaches_the_pane_edge() {
        let subject = |i: usize| match i % 2 {
            0 => format!("chore: release v0.3.{i}"),
            _ => format!("fix: make the behind count explicable where it is shown {i}"),
        };
        for width in [64u16, 72, 80, 92] {
            let rows = commit_rows_at(&[(2, 2), (338, 21), (2, 2), (104, 8)], width, subject);
            let ends: Vec<usize> = rows
                .iter()
                .map(|r| {
                    columns(r)
                        .iter()
                        .rposition(|c| !c.is_whitespace() && *c != '│')
                        .expect("row drew nothing")
                })
                .collect();
            assert!(
                ends.windows(2).all(|w| w[0] == w[1]),
                "rows end at different columns at width {width}: {ends:?}"
            );
        }
    }

    #[test]
    fn a_zero_count_draws_no_token_at_all() {
        assert_eq!(stat_part_width(0), 0);
        assert_eq!(stat_part_width(9), 2);
        assert_eq!(stat_part_width(338), 4);
    }

    /// The separator is only spent when both columns exist, so a list with
    /// nothing but insertions does not reserve a blank for deletions.
    #[test]
    fn the_churn_tail_spends_a_separator_only_between_two_columns() {
        assert_eq!(stat_tail_width(0, 0), 0);
        assert_eq!(stat_tail_width(4, 0), 4);
        assert_eq!(stat_tail_width(0, 3), 3);
        assert_eq!(stat_tail_width(4, 3), 8);
    }

    /// A repo shaped like the one that exposed this: several local branches
    /// behind by different amounts, one of them tracking a ref it is not named
    /// after, a long remote-only list under them, and enough history to scroll.
    fn spread_repo() -> RepoStatus {
        let mut r = busy_repo();
        r.behind = 40;
        r.branches = vec![
            crate::model::BranchStatus {
                name: "main".to_string(),
                upstream: Some("origin/main".to_string()),
                behind: 0,
                ahead: 0,
                is_head: true,
                tip: String::new(),
            },
            crate::model::BranchStatus {
                name: "beta".to_string(),
                upstream: Some("origin/beta".to_string()),
                behind: 21,
                ahead: 0,
                is_head: false,
                tip: String::new(),
            },
            crate::model::BranchStatus {
                name: "feature/who-goes-first".to_string(),
                upstream: Some("origin/wip".to_string()),
                behind: 19,
                ahead: 0,
                is_head: false,
                tip: String::new(),
            },
        ];
        r.remote_only_branches = (0..12)
            .map(|i| format!("origin/feature/spare-{i}"))
            .collect();
        // `info` draws its commit list from the graph rather than from
        // `commits`, so this is the field that decides whether the pane has
        // anything to scroll.
        r.graph = (0..30)
            .map(|i| crate::model::GraphLine {
                lanes: vec![crate::graph::commit_cell()],
                hash: format!("c{i:06}"),
                is_merge: false,
                refs: vec![],
                author: "Gaurgle".to_string(),
                message: format!("feat: commit {i}"),
            })
            .collect();
        r
    }

    /// The `info` pane's visible text at a given size, scrolled as far as its
    /// content allows. The offset is clamped during render, so a large value
    /// parks it at the bottom whatever the content turns out to measure.
    fn info_text_at_bottom(repo: RepoStatus, width: u16, height: u16) -> String {
        info_text_at_bottom_expanded(repo, width, height, false)
    }

    fn info_text_at_bottom_expanded(
        repo: RepoStatus,
        width: u16,
        height: u16,
        expand: bool,
    ) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::app::App::new(vec![repo], "t".to_string(), width, height);
        app.history_view = HistoryView::Info;
        app.expand_remote_branches = expand;
        app.set_detail_scroll(u16::MAX);
        let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        (0..height)
            .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The branches block is the only thing that explains the repo's behind
    /// count, so it has to survive the scrolling that reading the history
    /// requires. Unpinned, stepping the commit cursor pushed it off the top
    /// and left the "not fetched" list reading like the whole story.
    #[test]
    fn the_branches_block_survives_a_scroll_to_the_bottom() {
        // 20 rows is the shortest pane that still pins the branches;
        // `the_branches_pin_holds_down_to_a_twenty_row_pane` pins that
        // threshold directly.
        let text = info_text_at_bottom(spread_repo(), 92, 20);
        assert!(
            text.contains("branches"),
            "the branches label scrolled away"
        );
        assert!(text.contains("21 behind"), "beta's count scrolled away");
        assert!(
            text.contains("19 behind"),
            "the feature branch's count scrolled away"
        );
    }

    /// The remote-only list is bounded by what everyone else has pushed, not
    /// by what you have, so it stays in the scrolling body. Pinning it too
    /// would cost the pane its history on exactly the busy repos that need it.
    ///
    /// Tested expanded, because collapsed there are no names for the pin to
    /// hold and the assertion would pass for the wrong reason.
    #[test]
    fn the_remote_only_list_is_not_pinned() {
        let text = info_text_at_bottom_expanded(spread_repo(), 92, 20, true);
        assert!(
            text.contains("feat: commit"),
            "the history is unreachable at the bottom of the scroll"
        );
        assert!(
            !text.contains("spare-0 "),
            "the first remote-only row is still on screen at the bottom"
        );
    }

    /// The reported bug: twenty-plus "not fetched" rows sat between the pinned
    /// local branches and the commit list, so `recent commits` started below
    /// the fold and the selection scrolled the whole block away as soon as the
    /// cursor moved.
    #[test]
    fn info_keeps_the_commit_list_on_the_first_screen_with_many_remotes() {
        let text = info_text_at_bottom_expanded(spread_repo(), 92, 30, false);
        assert!(
            text.contains("12 remote branches not fetched"),
            "the list did not collapse:\n{text}"
        );
        assert!(
            text.contains("recent commits"),
            "the commit list is still below the fold:\n{text}"
        );
    }

    /// Collapsed is not a dead end: the names are still reachable in place.
    #[test]
    fn expanding_the_summary_draws_every_remote_name() {
        let text = info_text_at_bottom_expanded(spread_repo(), 92, 44, true);
        assert!(
            !text.contains("remote branches not fetched"),
            "still collapsed:\n{text}"
        );
        assert!(
            text.contains("spare-11"),
            "names missing when expanded:\n{text}"
        );
    }

    /// One branch is not "1 remote branches".
    #[test]
    fn a_single_unfetched_remote_reads_as_one_branch() {
        let mut r = spread_repo();
        r.remote_only_branches = vec!["origin/only".to_string()];
        let text = info_text_at_bottom_expanded(r, 92, 30, false);
        assert!(
            text.contains("1 remote branch not fetched"),
            "plural leaked into the singular:\n{text}"
        );
    }

    /// Where the branches block stops being pinned, stated so the next row
    /// added to the pinned region is a deliberate decision rather than a
    /// surprise. The headliner's divider is one such row and it landed inside
    /// the slack `MIN_BODY_ROWS` already had, so this threshold is where it was
    /// before the headliner grew. A boxed version, briefly tried, was three
    /// rows and did push it to 21.
    ///
    /// Below the threshold the ladder still keeps the identity header pinned;
    /// it is the branches block alone that goes.
    #[test]
    fn the_branches_pin_holds_down_to_a_twenty_row_pane() {
        let pinned = |height| info_text_at_bottom(spread_repo(), 92, height).contains("21 behind");
        assert!(pinned(20), "20 rows should hold the branches");
        assert!(!pinned(19), "19 rows should not");
        assert!(
            info_text_at_bottom(spread_repo(), 92, 19).contains("~/repos/fleetz"),
            "the identity header lost its pin along with the branches block"
        );
    }

    /// On a pane too short for both, the header keeps its pin rather than the
    /// pair losing it together -- the fallback is the whole reason the pinned
    /// count is chosen from a ladder instead of a single test.
    #[test]
    fn a_short_pane_falls_back_to_pinning_the_header_alone() {
        // 17 rows is the window where the header alone clears MIN_BODY_ROWS
        // and the header plus the branches block does not.
        let text = info_text_at_bottom(spread_repo(), 92, 17);
        assert!(
            text.contains("~/repos/fleetz"),
            "the identity header lost its pin along with the branches block"
        );
    }

    /// A branch tracking a ref it is not named after says so. Left implicit,
    /// its row is indistinguishable from one tracking its own name, and the
    /// count against it cannot be reconciled with the remote.
    #[test]
    fn a_branch_names_an_upstream_that_differs_from_its_own_name() {
        let text = info_text_at_bottom(spread_repo(), 120, 22);
        assert!(
            text.contains("feature/who-goes-first -> origin/wip"),
            "the differing upstream is not shown"
        );
        // The ordinary case stays bare: repeating `origin/beta` after `beta`
        // on every row would bury the one row worth reading.
        assert!(
            !text.contains("beta -> origin/beta"),
            "a same-named upstream is being repeated as noise"
        );
    }

    /// Whether a rendered pane shows `name` as its headliner. The headliner
    /// row trims to exactly the view's name; the tab strip in the border
    /// carries the same string but never trims to it alone, so this is what
    /// tells the two apart.
    fn has_headliner(text: &str, name: &str) -> bool {
        text.lines()
            .any(|l| l.trim().trim_matches('│').trim() == name)
    }

    /// The colours of the headliner's divider: its first cell and its last.
    fn rule_ends(view: HistoryView) -> (ratatui::style::Color, ratatui::style::Color) {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let width = 92u16;
        let mut app = crate::app::App::new(vec![spread_repo()], "t".to_string(), width, 24);
        app.history_view = view;
        let app = app;
        let mut term = Terminal::new(TestBackend::new(width, 24)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();

        // The divider is the row directly under the name.
        let name = view_name(view);
        let title_y = (1..24u16)
            .find(|y| {
                let row: String = (0..width).map(|x| buf[(x, *y)].symbol()).collect();
                row.trim().trim_matches('│').trim() == name
            })
            .expect("no headliner drawn");
        let y = title_y + 1;
        let cells: Vec<u16> = (0..width)
            .filter(|x| buf[(*x, y)].symbol() == "─")
            .collect();
        assert!(cells.len() > 2, "no divider under the headliner");
        (
            buf[(cells[0], y)].fg,
            buf[(*cells.last().expect("last cell"), y)].fg,
        )
    }

    /// The divider fades out of the name's own colour rather than starting at
    /// structure-grey. Starting there made it read as a separate dim line that
    /// happened to sit under the name, instead of as the name's own underline
    /// running out.
    #[test]
    fn the_headliner_divider_fades_from_the_name_colour_into_surface() {
        for view in HistoryView::ALL {
            let on = colors::history_view_color(view);
            let (near, far) = rule_ends(view);
            assert_ne!(
                near,
                colors::SURFACE,
                "{} divider starts at structure-grey, so it is not fading",
                view.label()
            );
            assert_eq!(
                far,
                colors::SURFACE,
                "{} divider does not arrive at structure-grey",
                view.label()
            );
            assert_ne!(near, on, "{} divider starts un-faded", view.label());
        }
    }

    /// The near end is nearer the name's colour than the far end is, in every
    /// channel
    /// that separates the two colours. States the direction of the fade, which
    /// the endpoints alone do not.
    #[test]
    fn the_headliner_divider_fades_towards_the_far_end() {
        let channels = |c| match c {
            ratatui::style::Color::Rgb(r, g, b) => (r as i32, g as i32, b as i32),
            other => panic!("expected an RGB colour, got {other:?}"),
        };
        for view in HistoryView::ALL {
            let on = channels(colors::history_view_color(view));
            let surface = channels(colors::SURFACE);
            let (near, far) = rule_ends(view);
            let (near, far) = (channels(near), channels(far));
            for (i, (o, s)) in [(on.0, surface.0), (on.1, surface.1), (on.2, surface.2)]
                .into_iter()
                .enumerate()
            {
                if o == s {
                    continue;
                }
                let (n, f) = match i {
                    0 => (near.0, far.0),
                    1 => (near.1, far.1),
                    _ => (near.2, far.2),
                };
                assert!(
                    (n - o).abs() <= (f - o).abs(),
                    "{} channel {i}: near end is not closer to the name colour",
                    view.label()
                );
            }
        }
    }

    /// The formatting rule itself. The render tests ask the helper what to
    /// expect, so without this nothing states what it should produce.
    #[test]
    fn a_view_name_is_drawn_in_capitals() {
        assert_eq!(view_name(HistoryView::Info), "INFO");
        assert_eq!(view_name(HistoryView::Commits), "COMMITS");
        assert_eq!(view_name(HistoryView::Graph), "GRAPH");
    }

    /// Capitals cost the strip nothing, which is what lets the tab and the
    /// headliner share one form: the switcher runs on a fit budget that drops
    /// the inactive tabs when the labels outgrow the border it shares with the
    /// pane title, and a wider name would collapse it to the active tab alone
    /// on panes where all four fit today.
    #[test]
    fn capitalising_a_tab_does_not_change_its_width() {
        for view in HistoryView::ALL {
            assert_eq!(
                view_name(view).chars().count(),
                view.label().chars().count(),
                "{} changed width in the strip",
                view.label()
            );
        }
    }

    /// One CI indicator per pane. The repo row carries a lamp and the detail
    /// header carries another; a third in the pane's body made three of them
    /// blinking the same fact in step, which reads as three things happening.
    ///
    /// The workflow name and duration went with it. They were the only thing
    /// that line added, and a name and a stopwatch are not worth a third lamp.
    #[test]
    fn the_info_body_does_not_repeat_the_ci_state() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut repo = busy_repo();
        repo.ci_status = crate::model::CiStatus::Passed;
        repo.ci_workflow = Some("build-and-test".to_string());
        repo.ci_duration = Some("12s".to_string());

        let (w, h) = (100u16, 30u16);
        let mut app = crate::app::App::new(vec![repo], "t".to_string(), w, h);
        app.history_view = HistoryView::Info;
        let app = app;
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        let text: String = (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");

        assert_eq!(
            text.matches("CI ◉").count(),
            1,
            "the pane draws CI more than once:\n{text}"
        );
        assert!(
            !text.contains("build-and-test"),
            "the workflow name is still drawn"
        );
    }

    /// The header's worktree count is the same concept as the list badge, so
    /// it wears the badge's colour; a repo with no linked worktree draws none.
    #[test]
    fn the_header_worktree_count_wears_the_badge_colour() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (w, h) = (100u16, 30u16);
        let draw = |repo: RepoStatus| {
            let mut app = crate::app::App::new(vec![repo], "t".to_string(), w, h);
            app.history_view = HistoryView::Info;
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| render_detail_pane(f, f.area(), &app))
                .unwrap();
            term.backend().buffer().clone()
        };
        let row_text = |buf: &ratatui::buffer::Buffer, y: u16| -> Vec<String> {
            (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect()
        };

        let mut with_one = busy_repo();
        with_one.worktrees = vec![crate::model::Worktree {
            path: "/tmp/wt".to_string(),
            branch: "feature".to_string(),
            ..Default::default()
        }];
        let buf = draw(with_one);
        let found = (0..h).find_map(|y| {
            let cells = row_text(&buf, y);
            let line = cells.concat();
            line.find("1 worktree").map(|byte| {
                let x = line[..byte].chars().count() as u16;
                (x, y)
            })
        });
        let (x, y) = found.expect("the header has no `1 worktree` text");
        assert_eq!(buf[(x, y)].fg, colors::WORKTREE_CLR);

        let buf = draw(busy_repo());
        let text: String = (0..h).flat_map(|y| row_text(&buf, y)).collect();
        assert!(
            !text.contains("worktree"),
            "a repo with no worktrees draws a worktree count"
        );
    }

    /// `busy_repo` with a second local branch `feature`, two ahead of its
    /// upstream, checked out in a linked worktree inside the repo with three
    /// uncommitted changes, last committed to two days ago.
    fn repo_with_a_worktree() -> RepoStatus {
        let mut r = busy_repo();
        r.branches.push(crate::model::BranchStatus {
            name: "feature".to_string(),
            upstream: Some("origin/feature".to_string()),
            behind: 0,
            ahead: 2,
            is_head: false,
            tip: "b2c3d4e".to_string(),
        });
        r.worktrees = vec![crate::model::Worktree {
            path: "~/repos/fleetz/.claude/worktrees/feat".to_string(),
            branch: "feature".to_string(),
            detached: false,
            head: "b2c3d4e".to_string(),
            dirty: Some(3),
            head_at: Some(chrono::Utc::now() - chrono::Duration::days(2)),
        }];
        r
    }

    /// The detail pane drawn in `view`, as the buffer and its rows of text,
    /// the side borders blanked so a row trims to its content. One cell per
    /// character, so a column in the text is a column in the buffer.
    fn drawn(repo: RepoStatus, view: HistoryView) -> (ratatui::buffer::Buffer, Vec<String>) {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let (w, h) = (100u16, 40u16);
        let mut app = crate::app::App::new(vec![repo], "t".to_string(), w, h);
        app.history_view = view;
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        let rows = (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| match buf[(x, y)].symbol() {
                        "│" => " ",
                        s => s,
                    })
                    .collect::<String>()
            })
            .collect();
        (buf, rows)
    }

    /// The colour of the first cell of `needle` on row `y`.
    fn fg_at(
        buf: &ratatui::buffer::Buffer,
        rows: &[String],
        y: usize,
        needle: &str,
    ) -> ratatui::style::Color {
        let byte = rows[y]
            .find(needle)
            .unwrap_or_else(|| panic!("`{needle}` not on row {y}: {}", rows[y]));
        let x = rows[y][..byte].chars().count() as u16;
        buf[(x, y as u16)].fg
    }

    /// Info's `worktrees` section answers the glance: which branch, dirty,
    /// ahead or behind, how long since the last commit, and where, relative
    /// to the repo when it lives inside it. The counters wear the repo list's
    /// colours. The branch row above no longer repeats the path, and the old
    /// `other worktrees` section is gone.
    #[test]
    fn info_draws_a_worktrees_section_with_per_worktree_state() {
        let (buf, rows) = drawn(repo_with_a_worktree(), HistoryView::Info);
        let text = rows.join("\n");

        let label = rows
            .iter()
            .position(|r| r.trim() == "worktrees")
            .unwrap_or_else(|| panic!("no `worktrees` label:\n{text}"));
        let branches = rows
            .iter()
            .position(|r| r.trim() == "branches")
            .expect("the branches block");
        let recent = rows
            .iter()
            .position(|r| r.trim() == "recent commits")
            .expect("the recent commits block");
        assert!(
            branches < label && label < recent,
            "after branches, before recent commits:\n{text}"
        );

        let row = label + 1;
        let line = &rows[row];
        assert!(line.contains("feature"), "the branch: {line}");
        assert!(line.contains("3!"), "the dirty count: {line}");
        assert!(line.contains("2↑"), "the ahead count: {line}");
        assert!(line.contains("2d ago"), "the age: {line}");
        assert!(
            line.contains(" .claude/worktrees/feat"),
            "the path, relative to the repo: {line}"
        );
        assert!(
            !line.contains("~/repos/fleetz"),
            "not the full path: {line}"
        );

        let mut dirty_only = busy_repo();
        dirty_only.dirty = 3;
        let mut ahead_only = busy_repo();
        ahead_only.ahead = 2;
        assert_eq!(
            fg_at(&buf, &rows, row, "3!"),
            badges::status_color(&dirty_only)
        );
        assert_eq!(
            fg_at(&buf, &rows, row, "2↑"),
            badges::status_color(&ahead_only)
        );
        assert_eq!(fg_at(&buf, &rows, row, "2d ago"), colors::OVERLAY);
        assert_eq!(fg_at(&buf, &rows, row, ".claude"), colors::OVERLAY);
        assert_eq!(fg_at(&buf, &rows, row, "feature"), colors::TEXT);

        assert!(
            !rows.iter().any(|r| r.trim_start().starts_with("worktree ")),
            "Info still draws the inline worktree row:\n{text}"
        );
        assert!(
            !text.contains("other worktrees"),
            "the old section is still drawn:\n{text}"
        );
    }

    /// A clean worktree level with its upstream reads `✓` in the synced
    /// colour; one with no upstream says so in words, as a branch row does;
    /// a dirty probe that failed is `?`, never clean.
    #[test]
    fn a_worktree_row_says_clean_unknown_and_no_upstream() {
        let mut r = repo_with_a_worktree();
        r.branches[1].ahead = 0;
        r.worktrees[0].dirty = Some(0);
        let (buf, rows) = drawn(r.clone(), HistoryView::Info);
        let row = rows
            .iter()
            .position(|l| l.trim() == "worktrees")
            .expect("label")
            + 1;
        assert!(rows[row].contains('✓'), "clean and level: {}", rows[row]);
        assert_eq!(
            fg_at(&buf, &rows, row, "✓"),
            badges::status_color(&busy_repo())
        );

        r.branches[1].upstream = None;
        r.worktrees[0].dirty = None;
        let (buf, rows) = drawn(r, HistoryView::Info);
        let row = rows
            .iter()
            .position(|l| l.trim() == "worktrees")
            .expect("label")
            + 1;
        assert!(rows[row].contains("no upstream"), "{}", rows[row]);
        assert!(
            !rows[row].contains('✓'),
            "unknown is not clean: {}",
            rows[row]
        );
        assert_eq!(fg_at(&buf, &rows, row, "no upstream"), colors::OVERLAY);
        assert_eq!(fg_at(&buf, &rows, row, "?"), colors::OVERLAY);
    }

    /// A detached worktree is named by its HEAD and has no branch to compare,
    /// so its counter columns read `·`; a path outside the repo keeps `~`.
    #[test]
    fn a_detached_worktree_is_named_by_its_head() {
        let mut r = busy_repo();
        r.worktrees = vec![crate::model::Worktree {
            path: "~/scratch/bisect".to_string(),
            branch: String::new(),
            detached: true,
            head: "9a2eb79".to_string(),
            dirty: Some(0),
            head_at: None,
        }];
        let (_, rows) = drawn(r, HistoryView::Info);
        let row = rows
            .iter()
            .position(|l| l.trim() == "worktrees")
            .expect("label")
            + 1;
        let line = &rows[row];
        assert!(line.contains("detached @ 9a2eb79"), "{line}");
        assert!(line.contains("~/scratch/bisect"), "{line}");
        let after_name = &line[line.find("9a2eb79").unwrap() + 7..line.find("~/").unwrap()];
        assert_eq!(
            after_name.split_whitespace().collect::<Vec<_>>(),
            ["·", "·", "·"],
            "dirty, ahead/behind and age all read `·`: {line}"
        );
    }

    /// The body wraps, while the commit list's row tops are counted in source
    /// lines. A worktree row wider than the pane wrapped and put every commit
    /// below it a row lower than recorded, so the cursor follow misjudged the
    /// selection. A row is bounded to the pane, the path cut to what is left.
    #[test]
    fn a_wide_worktree_row_stays_one_row_and_the_commit_tops_hold() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut r = repo_with_a_worktree();
        r.branches[1].name = "feat/a-very-long-branch-name-for-the-worktree".to_string();
        r.branches[1].upstream = None;
        r.worktrees[0].branch = r.branches[1].name.clone();
        r.worktrees[0].path = "~/repos/fleetz-worktrees/a/deeply/nested/checkout/path".to_string();

        let (w, h) = (50u16, 40u16);
        let mut app = crate::app::App::new(vec![r], "t".to_string(), w, h);
        app.history_view = HistoryView::Info;
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        let rows: Vec<String> = (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| match buf[(x, y)].symbol() {
                        "│" => " ",
                        s => s,
                    })
                    .collect()
            })
            .collect();
        let text = rows.join("\n");

        let label = rows
            .iter()
            .position(|l| l.trim() == "worktrees")
            .unwrap_or_else(|| panic!("no label:\n{text}"));
        assert!(rows[label + 1].contains("feat/a-very"), "{text}");
        assert!(
            rows[label + 1].contains('…'),
            "the row is cut at the pane's edge:\n{text}"
        );
        assert_eq!(
            rows[label + 2].trim(),
            "",
            "the worktree row wrapped onto a second row:\n{text}"
        );

        // The pinned block ends with the last local branch; the body starts
        // on the blank row below it, and the first commit must sit where its
        // recorded top says.
        let branches = rows
            .iter()
            .position(|l| l.trim() == "branches")
            .unwrap_or_else(|| panic!("no branches block:\n{text}"));
        let body_top = branches
            + rows[branches..]
                .iter()
                .position(|l| l.trim().is_empty())
                .expect("a blank row closes the branches block");
        let first_top = *app
            .detail_row_tops
            .borrow()
            .first()
            .expect("the commit list recorded its tops") as usize;
        let commit = rows
            .iter()
            .position(|l| l.contains("a1b2c3d") && !l.contains('['))
            .unwrap_or_else(|| panic!("no commit row:\n{text}"));
        assert_eq!(
            commit,
            body_top + first_top,
            "the commit is not where its top says:\n{text}"
        );

        // Where the columns leave room, it is the path that gives way.
        let r = app.selected_repo().expect("the repo").clone();
        let row: String = worktree_rows(&r, 70)[1]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(row.chars().count(), 70, "{row}");
        assert!(row.ends_with('…'), "{row}");
        assert!(row.contains("2d ago  ~/repos/"), "{row}");
    }

    /// A branch longer than the column's cap is cut at `WORKTREE_NAME_MAX`
    /// with the pane's ellipsis.
    #[test]
    fn a_long_worktree_branch_is_cut_at_the_cap() {
        let mut r = repo_with_a_worktree();
        let long = "b".repeat(WORKTREE_NAME_MAX + 10);
        r.branches[1].name = long.clone();
        r.worktrees[0].branch = long;
        let lines = worktree_rows(&r, 200);
        let row: String = lines[1].spans.iter().map(|s| s.content.as_ref()).collect();
        let expected = format!("{}…", "b".repeat(WORKTREE_NAME_MAX - 1));
        assert!(row.contains(&expected), "{row}");
        assert!(!row.contains(&"b".repeat(WORKTREE_NAME_MAX)), "{row}");
    }

    #[test]
    fn info_with_no_linked_worktree_draws_no_worktrees_label() {
        let (_, rows) = drawn(busy_repo(), HistoryView::Info);
        assert!(!rows.iter().any(|r| r.trim() == "worktrees"));
    }

    /// The Branches view has no worktrees section, so its inline row is still
    /// the place it says "checked out elsewhere".
    #[test]
    fn the_branches_view_keeps_the_inline_worktree_row() {
        let (_, rows) = drawn(repo_with_a_worktree(), HistoryView::Branches);
        assert!(
            rows.iter()
                .any(|r| r.contains("worktree ~/repos/fleetz/.claude/worktrees/feat")),
            "{}",
            rows.join("\n")
        );
        assert!(!rows.iter().any(|r| r.trim() == "worktrees"));
    }

    #[test]
    fn a_worktree_path_is_shortened_only_under_the_repo() {
        assert_eq!(
            path_within_repo("~/repos/fleetz/.claude/worktrees/x", "~/repos/fleetz"),
            ".claude/worktrees/x"
        );
        assert_eq!(
            path_within_repo("~/repos/fleetz-hotfix", "~/repos/fleetz"),
            "~/repos/fleetz-hotfix",
            "a sibling sharing the prefix is outside"
        );
        assert_eq!(path_within_repo("/tmp/wt", "~/repos/fleetz"), "/tmp/wt");
        assert_eq!(path_within_repo("~/repos/fleetz", "~/repos/fleetz"), ".");
        assert_eq!(
            path_within_repo("~/repos/fleetz/wt", "~/repos/fleetz/"),
            "wt",
            "a trailing separator on the repo path"
        );
    }

    /// Each view draws its own section and none of the others'. The whole
    /// point is that they take turns rather than queueing for one scroll
    /// offset, so a section leaking across views defeats it.
    #[test]
    fn each_view_shows_only_its_own_section() {
        // `info` draws a named section of its own; the other three are named
        // by the pane's headliner, matched in its boxed form. The bare name
        // will not do: the tab strip carries all four in every view, and the
        // strip and the headliner draw the same string, so `contains("GRAPH")`
        // is true in every view and would assert nothing.
        // `info` draws a named section of its own; the other three are named
        // by the pane's headliner. The bare name will not do for those: the
        // tab strip carries all four in every view and draws the same string,
        // so `contains("GRAPH")` is true everywhere and asserts nothing.
        for view in HistoryView::ALL {
            let text = pane_text(view, 92);
            match view {
                HistoryView::Info => assert!(
                    text.contains("recent commits"),
                    "info is missing its own section"
                ),
                _ => assert!(
                    has_headliner(&text, &view_name(view)),
                    "{} is missing its headliner",
                    view.label()
                ),
            }
            for other in HistoryView::ALL {
                if other == view {
                    continue;
                }
                assert!(
                    !has_headliner(&text, &view_name(other)),
                    "{} leaked the {} headliner",
                    view.label(),
                    other.label()
                );
            }
            if view != HistoryView::Info {
                assert!(
                    !text.contains("recent commits"),
                    "{} leaked info's section",
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
        let text: String = expanded_graph_lines("* commit a1b2c3d", "", 80)
            .iter()
            .flat_map(|l| l.spans.iter())
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
                    text.contains(&view_name(tab)),
                    "{} is missing the {} tab",
                    view.label(),
                    tab.label()
                );
            }
        }
    }

    /// The switcher moved into the border because the body scrolls. The doc
    /// comment on the old one claimed it was pinned above the scrolling
    /// content; it was the scrolling content's first row, so paging down took
    /// it off screen -- exactly when "what am I looking at" gets asked.
    #[test]
    fn the_view_switcher_is_in_the_border_and_says_which_keys_move_it() {
        for view in HistoryView::ALL {
            let border = pane_text(view, 92)
                .lines()
                .next()
                .unwrap_or_default()
                .to_string();
            for tab in HistoryView::ALL {
                assert!(
                    border.contains(&view_name(tab)),
                    "{} view: the border is missing the {} tab: {border:?}",
                    view.label(),
                    tab.label()
                );
            }
            assert!(
                border.contains("[/]"),
                "{} view: the border does not say which keys switch: {border:?}",
                view.label()
            );
        }
    }

    /// The regression itself: scroll the body to the bottom and the switcher
    /// is still there, because it is not in the body.
    #[test]
    fn scrolling_to_the_bottom_does_not_lose_the_view_switcher() {
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
        r.reflog = (0..200)
            .map(|i| crate::model::ReflogEntry {
                hash: format!("r{i:06}"),
                action: "commit".to_string(),
                description: format!("entry {i}"),
            })
            .collect();

        for view in HistoryView::ALL {
            let mut app = crate::app::App::new(vec![r.clone()], "t".to_string(), 92, 30);
            app.history_view = view;
            app.set_detail_scroll(150);
            let mut term = Terminal::new(TestBackend::new(92, 30)).unwrap();
            term.draw(|f| render_detail_pane(f, f.area(), &app))
                .unwrap();
            let buf = term.backend().buffer().clone();
            let border: String = (0..92u16).map(|x| buf[(x, 0)].symbol()).collect();
            assert!(
                border.contains(&view_name(view)),
                "{} view: scrolled away: {border:?}",
                view.label()
            );
        }
    }

    /// One selection marker across all four views. Reflog and the expanded
    /// graph bolded the hash and nothing else, which marks a single token and
    /// leaves the eye to find which token it was -- in a two-hundred row list
    /// that search is the work the caret exists to save.
    #[test]
    fn every_view_marks_its_selection_the_same_way() {
        for view in HistoryView::ALL {
            // `ci` lists runs, so it is drawn with the recorded runs held.
            let text = match view {
                HistoryView::Ci => ci_view_text(92),
                HistoryView::Notes => {
                    notes_pane_text(&notes_view_app(&[("notez/a.md", Some("notez"))]))
                }
                _ => pane_text(view, 92),
            };
            assert!(
                text.contains("▸ "),
                "{} view has no selection caret:\n{text}",
                view.label()
            );
        }
    }

    /// A percentage of a very wide terminal is more room than a repo row can
    /// use: past about fifty columns the list is only drawing longer dots.
    /// The right column has the opposite appetite, so the surplus goes there.
    #[test]
    fn the_repo_list_stops_growing_once_the_rows_have_what_they_need() {
        use ratatui::layout::Rect;
        let at = |w: u16| {
            let (list, right) =
                split_main(Rect::new(0, 0, w, 40), crate::app::DEFAULT_REPO_LIST_PCT);
            (list.width, right.width)
        };

        // Below the cap nothing changes: the percentage is still the answer.
        let (list, _) = at(120);
        assert_eq!(list, 120 * crate::app::DEFAULT_REPO_LIST_PCT / 100);

        let (narrow_list, narrow_right) = at(150);
        let (wide_list, wide_right) = at(300);
        assert_eq!(wide_list, REPO_LIST_MAX_COLS, "the list kept growing");
        assert!(
            wide_right - narrow_right >= 150,
            "the surplus did not reach the right column: {narrow_right} -> {wide_right}"
        );
        assert!(narrow_list <= REPO_LIST_MAX_COLS);
    }

    /// Only the default yields to the cap. Once the divider has been dragged
    /// the number is the user's answer to this question, and a cap that
    /// overrode it would make the drag look broken.
    #[test]
    fn a_dragged_divider_is_honoured_past_the_cap() {
        use ratatui::layout::Rect;
        let dragged = crate::app::DEFAULT_REPO_LIST_PCT + 5;
        let (list, _) = split_main(Rect::new(0, 0, 300, 40), dragged);
        assert_eq!(list.width, 300 * dragged / 100);
        assert!(list.width > REPO_LIST_MAX_COLS);
    }

    /// The standing invariant: `panel_rects` must report the rectangles
    /// `render` draws, and both reach the split through `split_main`. A cap
    /// added to one side only is exactly the drift that function exists to
    /// prevent, and it fails silently -- clicks land on the wrong pane.
    #[test]
    fn the_capped_split_is_the_one_hit_testing_uses() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 300, 40);
        let mut term = Terminal::new(TestBackend::new(300, 40)).unwrap();
        let mut drawn = None;
        term.draw(|f| {
            drawn = Some(crate::ui::panel_rects(f.area(), &app));
            crate::ui::render(f, &app);
        })
        .unwrap();
        let rects = drawn.expect("panel_rects");
        assert_eq!(
            rects.repo_list.width, REPO_LIST_MAX_COLS,
            "hit testing still thinks the list is the uncapped width"
        );
    }

    /// The repo header identifies what you are looking at, and scrolling two
    /// hundred rows of history is exactly when that question comes back. It
    /// sits outside the scrolling body now, like the pinned readout above it.
    #[test]
    fn the_repo_header_survives_scrolling() {
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

        let mut app = crate::app::App::new(vec![r], "t".to_string(), 92, 30);
        app.history_view = HistoryView::Commits;
        app.set_detail_scroll(40);
        let mut term = Terminal::new(TestBackend::new(92, 24)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        let text: String = (0..24u16)
            .map(|y| (0..92u16).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            text.contains("~/repos/fleetz"),
            "header scrolled away:\n{text}"
        );
        assert!(text.contains("synced"), "status scrolled away");
        assert!(
            text.contains("h000040"),
            "the body did not scroll with the header pinned"
        );
    }

    /// A short pane spends its rows on the content you came for. The header
    /// repeats what the frame's own title already says, so it is the half that
    /// gives way rather than taking half the height.
    #[test]
    fn a_short_pane_lets_the_header_scroll_rather_than_eat_the_body() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut r = busy_repo();
        r.graph = (0..40)
            .map(|i| crate::model::GraphLine {
                lanes: vec![crate::graph::commit_cell()],
                hash: format!("h{i:06}"),
                is_merge: false,
                refs: vec![],
                author: "Gaurgle".to_string(),
                message: format!("subject {i}"),
            })
            .collect();
        let mut app = crate::app::App::new(vec![r], "t".to_string(), 92, 10);
        app.history_view = HistoryView::Commits;
        app.set_detail_scroll(30);
        let mut term = Terminal::new(TestBackend::new(92, 10)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        let text: String = (0..10u16)
            .map(|y| (0..92u16).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("h0000"),
            "a short pane showed no history at all:\n{text}"
        );
    }

    /// Only the active tab is coloured, and it wears the colour its own view's
    /// content uses. One lit tab reads instantly; four lit ones are a palette.
    #[test]
    fn only_the_active_tab_wears_its_view_colour() {
        for view in HistoryView::ALL {
            let line = view_switcher(view, true, 200);
            let label = |v: HistoryView| {
                line.spans
                    .iter()
                    .find(|s| s.content == view_name(v))
                    .unwrap_or_else(|| panic!("{} tab missing", v.label()))
            };
            assert_eq!(
                label(view).style.fg,
                Some(colors::history_view_color(view)),
                "the {} tab is not lit while it is active",
                view.label()
            );
            for other in HistoryView::ALL.iter().filter(|v| **v != view) {
                assert_eq!(
                    label(*other).style.fg,
                    Some(colors::SUBTEXT),
                    "the {} tab is lit while {} is active",
                    other.label(),
                    view.label()
                );
            }
        }
    }

    /// The colours have to be four, not one repeated: the point is telling
    /// them apart at a glance from across the screen.
    #[test]
    fn the_four_view_colours_are_distinct() {
        let mut seen = Vec::new();
        for view in HistoryView::ALL {
            let c = colors::history_view_color(view);
            assert!(!seen.contains(&c), "{} shares a colour", view.label());
            seen.push(c);
        }
    }

    /// Arriving in a view shows its page, not one of its commits. Switching
    /// used to follow the selection, and the expanded graph anchors its
    /// selection to the top of the pane -- so asking for the tab scrolled its
    /// own header off and landed you on a commit.
    #[test]
    fn switching_views_does_not_scroll_the_one_you_arrive_in() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut r = busy_repo();
        r.graph_expanded = (0..30)
            .flat_map(|i| {
                vec![
                    format!("* commit e{i:06} (HEAD -> main)"),
                    "| Author: Gaurgle <g@example.com>".to_string(),
                    "|".to_string(),
                    format!("|     expanded subject {i}"),
                ]
            })
            .collect();

        let mut app = crate::app::App::new(vec![r], "t".to_string(), 92, 30);
        while app.history_view != HistoryView::Graph {
            app.cycle_history_view(false);
        }
        assert_eq!(app.detail_scroll(), 0, "arriving already moved the offset");

        let mut term = Terminal::new(TestBackend::new(92, 24)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        assert_eq!(
            app.detail_scroll(),
            0,
            "the first frame after arriving scrolled the page"
        );
        let buf = term.backend().buffer().clone();
        let text: String = (0..24u16)
            .map(|y| (0..92u16).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            has_headliner(&text, &view_name(HistoryView::Graph)),
            "the view title scrolled off"
        );
        assert!(
            text.contains("e000000"),
            "the first commit is not on screen"
        );
    }

    /// The tab in the frame is what you aim at; the title inside the pane is
    /// what confirms you landed. Same word, same colour, one source -- said
    /// twice in two colours they read as two different things.
    #[test]
    fn the_pinned_title_matches_the_tab_it_belongs_to() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        for view in HistoryView::ALL {
            let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 92, 30);
            app.history_view = view;
            let mut term = Terminal::new(TestBackend::new(92, 24)).unwrap();
            term.draw(|f| render_detail_pane(f, f.area(), &app))
                .unwrap();
            let buf = term.backend().buffer().clone();

            // The title row is the one inside the pane whose text is exactly
            // the view's headliner -- the frame's tab strip carries all four
            // on row 0, so start below it.
            let title = view_name(view);
            let first_letter = title[0..1].to_string();
            let want = colors::history_view_color(view);
            let mut found = false;
            for y in 1..24u16 {
                let row: String = (0..92u16).map(|x| buf[(x, y)].symbol()).collect();
                // The row has to *be* the name: the strip draws the same
                // string, so a substring match would find the border row as
                // readily as the headliner.
                if row.trim().trim_matches('│').trim() != title {
                    continue;
                }
                let x = (0..92u16)
                    .find(|x| buf[(*x, y)].symbol() == first_letter)
                    .expect("first letter");
                assert_eq!(
                    buf[(x, y)].fg,
                    want,
                    "the {} title is not in its tab's colour",
                    view.label()
                );
                found = true;
                break;
            }
            assert!(found, "{} has no title row of its own", view.label());
        }
    }

    /// A node is a commit, not a branch. Colouring it by lane made the node
    /// column change hue whenever a lane was reused, which is a signal that
    /// means nothing -- the lanes either side already say what the topology
    /// is.
    #[test]
    fn graph_nodes_are_plain_text_not_lane_coloured() {
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

        let rows = crate::graph::layout(&commits);
        let mut nodes = 0usize;
        let mut lanes = 0usize;
        for cells in &rows {
            for span in lane_spans(cells) {
                let glyph = span.content.chars().next().unwrap_or(' ');
                if glyph == crate::graph::COMMIT_NODE || glyph == crate::graph::MERGE_NODE {
                    nodes += 1;
                    assert_eq!(span.style.fg, Some(colors::TEXT), "a node is lane-coloured");
                } else if glyph == '\u{2502}' {
                    lanes += 1;
                    assert_ne!(span.style.fg, Some(colors::TEXT), "a lane went plain");
                }
            }
        }
        assert!(nodes >= 4, "fixture drew no nodes");
        assert!(lanes >= 1, "fixture drew no lanes to contrast against");
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
            // A run is not a commit; FZ-30f details runs.
            let expected = match view {
                HistoryView::Ci | HistoryView::Notes => None,
                _ => Some("a1b2c3d"),
            };
            assert_eq!(
                app.inspected_hash(),
                expected,
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
            !rows[0].contains("CI ◉"),
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

    /// Every row of the detail pane under `-p`, for the PR section's heading.
    fn pane_rows_with_pr_detail(total: u32, listed: Vec<u32>) -> Vec<String> {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut repo = cloned_repo();
        repo.pr_count = total;
        repo.prs = listed.into_iter().map(pr_numbered).collect();
        let mut app = crate::app::App::new(vec![repo], "test".to_string(), 120, 60);
        app.show_pr_detail = true;
        let mut terminal = Terminal::new(TestBackend::new(120, 60)).unwrap();
        terminal
            .draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..60)
            .map(|y| {
                (0..120)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    /// CV14: five listed of eight open. Fails if the heading goes back to
    /// counting the list alone ("5 open PRs").
    #[test]
    fn pr_section_says_five_of_eight_when_the_list_is_capped() {
        let rows = pane_rows_with_pr_detail(8, vec![13, 9, 7, 4, 2]);
        assert!(
            rows.iter().any(|r| r.contains("5 of 8 open PRs")),
            "got {rows:?}"
        );
        assert!(!rows.iter().any(|r| r.contains(" 5 open PRs")));
    }

    /// Fails if the equal case grows an "of" or loses the singular.
    #[test]
    fn pr_section_wording_is_unchanged_when_every_open_pr_is_listed() {
        let five = pane_rows_with_pr_detail(5, vec![13, 9, 7, 4, 2]);
        assert!(five.iter().any(|r| r.contains("5 open PRs")));
        assert!(!five.iter().any(|r| r.contains(" of 5 open")));

        let one = pane_rows_with_pr_detail(1, vec![13]);
        assert!(one.iter().any(|r| r.contains("1 open PR")));
        assert!(!one.iter().any(|r| r.contains("1 open PRs")));
        assert!(!one.iter().any(|r| r.contains("1 of 1")));
    }

    /// The noun follows the total. Fails if it follows the listed count
    /// ("1 of 3 open PR").
    #[test]
    fn pr_section_pluralises_by_the_total_when_one_of_three_is_listed() {
        let rows = pane_rows_with_pr_detail(3, vec![13]);
        assert!(
            rows.iter().any(|r| r.contains("1 of 3 open PRs")),
            "got {rows:?}"
        );
    }

    /// A list longer than the count must not print "8 of 5". Fails if the
    /// comparison is dropped.
    #[test]
    fn pr_section_never_prints_a_listed_count_above_the_total() {
        let rows = pane_rows_with_pr_detail(2, vec![13, 9, 7]);
        assert!(rows.iter().any(|r| r.contains("3 open PRs")));
        assert!(!rows.iter().any(|r| r.contains(" of 2 open")));
    }

    /// The header row already uses the total and must keep doing so beside
    /// the section's new wording.
    #[test]
    fn header_row_keeps_the_total_when_the_pr_section_is_capped() {
        let mut r = cloned_repo();
        r.pr_count = 8;
        r.prs = vec![13, 9, 7, 4, 2].into_iter().map(pr_numbered).collect();
        let facts = header_rows(r, 120).2;
        assert!(facts.contains("8 open PRs #13 #9+"), "got {facts:?}");
        assert!(!facts.contains(" of 8"), "got {facts:?}");
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
            daily_key: None,
            last_local: None,
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
            remote_branches: vec![],
            work_key: None,
            fetch: crate::model::FetchFacts::default(),
            authors: Default::default(),
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
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
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

    /// Every row of the detail pane as drawn, joined by newlines.
    fn header_text(repo: RepoStatus, width: u16) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let app = crate::app::App::new(vec![repo], "test".to_string(), width, 40);
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        terminal
            .draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..24)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Rule 8: an HTTPS remote can carry a token in its userinfo, and the
    /// remote row drew `remote_url` exactly as stored. Display only: the
    /// owner is still read from the stored URL.
    #[test]
    fn a_remote_url_with_a_credential_is_drawn_without_it() {
        let mut r = cloned_repo();
        r.owner = None;
        r.remote_url = Some("https://user:secret@github.com/Gaurgle/fleetz.git".to_string());
        assert_eq!(r.owner_display().as_deref(), Some("Gaurgle"));
        let text = header_text(r, 100);
        assert!(!text.contains("secret"), "credential on screen:\n{text}");
        assert!(!text.contains("user"), "userinfo on screen:\n{text}");
        assert!(
            text.contains("https://github.com/Gaurgle/fleetz.git"),
            "{text}"
        );
    }

    /// Rule 5 and rule 8 in the pane: under the counts, that the fetch
    /// failed, why and since when, then git's line with no credential in it.
    #[test]
    fn a_failed_fetch_is_spelled_out_under_the_counts() {
        let mut r = cloned_repo();
        r.behind = 2;
        let at = chrono::Local::now();
        r.fetch.last_success = Some(at);
        let failure = crate::git::FetchFailure {
            kind: crate::git::FetchFailureKind::Unreachable,
            detail: Some("fatal: unable to access 'https://host.invalid/o/r.git/'".to_string()),
        };
        r.fetch
            .record(&crate::git::FetchOutcome::Failed(failure), at);
        let text = header_text(r, 100);
        let rows: Vec<&str> = text.lines().collect();
        assert!(rows[3].contains("2 behind"), "{text}");
        assert!(
            rows[4].contains(&format!(
                "⚠ fetch failed: remote unreachable, as of {}",
                at.format("%H:%M")
            )),
            "{text}"
        );
        assert!(rows[5].contains("fatal: unable to access"), "{text}");
    }

    /// Nothing extra in the pane while fetches work.
    #[test]
    fn a_working_fetch_adds_no_row_to_the_pane() {
        let mut r = cloned_repo();
        r.fetch
            .record(&crate::git::FetchOutcome::Succeeded, chrono::Local::now());
        let text = header_text(r, 100);
        assert!(!text.contains("fetch"), "{text}");
    }

    /// git accepts a remote pasted with smart quotes and returns it as
    /// written. Drawing it must neither panic nor show the credential.
    #[test]
    fn a_smart_quoted_remote_url_draws_without_panicking_or_its_credential() {
        let mut r = cloned_repo();
        r.remote_url = Some("\u{2018}https://u:secret@github.com/o/r.git\u{2019}".to_string());
        let text = header_text(r, 100);
        assert!(!text.contains("secret"), "credential on screen:\n{text}");
        assert!(
            text.contains("\u{2018}https://github.com/o/r.git\u{2019}"),
            "{text}"
        );
    }

    /// The pinned readout row (row 2), border stripped.
    fn readout_row(repo: RepoStatus, selected: usize, width: u16) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::app::App::new(vec![repo], "test".to_string(), width, 40);
        app.set_detail_selected(selected);
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
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

    /// A repo whose two history lists agree, which is the ordinary case: every
    /// local branch is merged into HEAD, so `git log HEAD` and `git log
    /// <every branch>` return the same commits in the same order. The readout
    /// tests below are about the row's contents, not about the lists
    /// diverging; `readout_names_the_row_the_cursor_is_on` covers that.
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
        r.graph = r
            .commits
            .iter()
            .map(|c| crate::model::GraphLine {
                lanes: vec![crate::graph::commit_cell()],
                hash: c.hash.clone(),
                is_merge: false,
                refs: vec![],
                author: c.author.clone(),
                message: c.message.clone(),
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
        // Tall enough that the commit list fits without the pane scrolling to
        // follow the cursor: a scrolled body slides the header rows this test
        // measures off the top, and it is their columns that are under test.
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
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

    /// The regression: `info` draws the graph but the readout and the
    /// inspector used to index `commits`, and the two are different `git log`
    /// runs -- `commits` walks HEAD, the graph walks the union of the local
    /// branches. One commit living only on a side branch therefore sat in the
    /// drawn list and not in the indexed one, and every row below it reported
    /// its neighbour's hash: the caret was on `36fc171d` while the readout and
    /// the inspector below both said `0a7d35e6`.
    #[test]
    fn readout_names_the_row_the_cursor_is_on() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let width = 72;
        let mut r = repo_with_commits(4);
        // Reachable from a side branch but not from HEAD, so it is in the
        // graph and not in `commits`, exactly as `git log` reports the two.
        r.graph.insert(
            1,
            crate::model::GraphLine {
                lanes: vec![crate::graph::commit_cell()],
                hash: "sidebr1".to_string(),
                is_merge: false,
                refs: vec![],
                author: "Gaurgle".to_string(),
                message: "docs: on a branch".to_string(),
            },
        );

        let mut app = crate::app::App::new(vec![r], "test".to_string(), width, 40);
        app.history_view = HistoryView::Info;
        app.set_detail_selected(2);
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        terminal
            .draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..24u16)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect();

        let caret_row = rows
            .iter()
            .find(|row| row.contains("\u{25b8} "))
            .expect("the cursor row is drawn");
        // Row 2: the readout sits directly under the pinned panel title, as
        // `readout_ends_in_the_same_column_as_the_owner_and_name_rows` pins.
        let readout = &rows[2];
        // Which hash the caret is on, taken from the drawn row rather than
        // from the model: the whole failure was the two disagreeing.
        let hash = caret_row
            .split('\u{25b8}')
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .expect("the cursor row names a hash");
        // Third graph row: the side-branch commit displaced it by one, which
        // is precisely what `commits` does not know about.
        assert_eq!(hash, "1abc123", "the caret sits on the third graph row");
        // 5 rows, not 4: the readout counts the list it draws, side branch
        // included, so the ordinal and the total come from the same place.
        assert!(
            readout.contains(&format!("3/5 \u{b7} {hash}")),
            "readout {readout:?} names a different commit than the caret row {caret_row:?}"
        );
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

    /// `git log --graph` writes `*` for every commit, and the node used to take
    /// the lane's hue, so the same commit changed colour depending on which
    /// column its branch happened to occupy. The compact graph already paints
    /// nodes in the plain text colour (`graph::Cell::is_node`): a commit belongs
    /// to the history, not to whichever lane it sits in.
    #[test]
    fn graph_nodes_are_not_lane_coloured() {
        let spans = graph_prefix_spans_raw("| | * |");
        let colour_of = |i: usize| spans[i].style.fg.expect("every span is styled");

        assert_eq!(colour_of(4), colors::TEXT, "the node took a lane colour");
        assert_eq!(
            colour_of(0),
            colors::graph_lane_color(0),
            "pipes must keep their lane hue"
        );
        assert_ne!(
            colour_of(0),
            colour_of(6),
            "two pipes in different lanes must stay distinguishable"
        );
    }

    /// A node is a node in every column, which is the whole point: the old rule
    /// made column 0 and column 4 different colours for the same kind of thing.
    #[test]
    fn a_node_is_the_same_colour_in_every_column() {
        let first = graph_prefix_spans_raw("* ");
        let fifth = graph_prefix_spans_raw("| | * ");
        assert_eq!(
            first[0].style.fg.unwrap(),
            fifth[4].style.fg.unwrap(),
            "node colour still depends on the column"
        );
    }

    /// git writes two columns per lane (glyph and horizontal run), so the
    /// palette must be indexed by lane not by column. If this reverts to raw
    /// column indices, adjacent lanes land two palette entries apart and six
    /// lanes only use three of six colours. This guards against that regression.
    #[test]
    fn pipes_are_coloured_by_lane_not_by_column() {
        let spans = graph_prefix_spans_raw("| | | | ");
        let colour_of = |i: usize| spans[i].style.fg.expect("every span is styled");

        // Columns 0, 2, 4, 6 are lanes 0, 1, 2, 3 respectively. Each should
        // get a different palette entry if indexed by lane (i / 2), or land on
        // the same entry if mistakenly indexed by raw column (i).
        assert_eq!(
            colour_of(0),
            colors::graph_lane_color(0),
            "column 0 is lane 0"
        );
        assert_eq!(
            colour_of(2),
            colors::graph_lane_color(1),
            "column 2 is lane 1"
        );
        assert_eq!(
            colour_of(4),
            colors::graph_lane_color(2),
            "column 4 is lane 2"
        );
        assert_eq!(
            colour_of(6),
            colors::graph_lane_color(3),
            "column 6 is lane 3"
        );

        // Adjacent lanes must use different palette entries.
        assert_ne!(
            colour_of(0),
            colour_of(2),
            "lanes 0 and 1 must be distinguishable"
        );
    }

    /// The graph view truncated every line to the pane width, so a long
    /// subject or a long decoration list was simply unreadable. Wrapping
    /// happens in the builder rather than in the `Paragraph` so one source
    /// line stays one rendered row, which the scroll arithmetic depends on.
    #[test]
    fn a_long_graph_line_wraps_instead_of_truncating() {
        let raw = "|     feat: rotate the contribution window's from date past GitHub's cache";
        let rows = expanded_graph_lines(raw, "", 40);
        assert!(rows.len() > 1, "the line was truncated, not wrapped");
        for row in &rows {
            let w: usize = row.spans.iter().map(|s| s.content.chars().count()).sum();
            assert!(w <= 40, "a wrapped row is {w} wide, past the pane's 40");
        }
    }

    /// Every row carries the lane prefix. A gap in the pipes reads as a
    /// topology change that did not happen.
    #[test]
    fn wrapped_rows_repeat_the_lane_prefix() {
        let raw = "| | |     a subject long enough to need more than one row here";
        let rows = expanded_graph_lines(raw, "", 30);
        assert!(rows.len() > 1, "fixture did not wrap");
        for (i, row) in rows.iter().enumerate() {
            let text: String = row.spans.iter().map(|s| s.content.as_ref()).collect();
            assert!(
                text.starts_with("  | | |"),
                "row {i} lost the lane prefix: {text:?}"
            );
        }
    }

    /// "Dynamic" means it re-wraps to whatever the pane is now, so a narrower
    /// pane must produce more rows from the same input.
    #[test]
    fn a_narrower_pane_produces_more_rows() {
        let raw = "|     a subject long enough to wrap at several different widths";
        let wide = expanded_graph_lines(raw, "", 60).len();
        let narrow = expanded_graph_lines(raw, "", 30).len();
        assert!(
            narrow > wide,
            "wrapping did not follow the width: {narrow} vs {wide}"
        );
    }

    /// A single token longer than the pane must still render, and must
    /// terminate. Breaking only at spaces loops forever on a long URL.
    #[test]
    fn a_token_longer_than_the_pane_still_renders() {
        let long = "x".repeat(120);
        let rows = wrap_text(&long, 20);
        assert_eq!(rows.len(), 6, "expected 120 chars to fill six 20-wide rows");
        for row in &rows {
            assert!(row.chars().count() <= 20);
        }
        assert_eq!(rows.concat(), long, "characters were dropped");
    }

    /// Empty input is one empty row, not zero: the blank lines between git's
    /// blocks are load-bearing spacing.
    #[test]
    fn empty_text_wraps_to_a_single_empty_row() {
        assert_eq!(wrap_text("", 20), vec![String::new()]);
    }

    /// The graph view pinned the selected commit to the top of the pane on
    /// every selection change, so stepping one commit moved the whole view.
    /// A block already clear of both margins must not move it at all.
    ///
    /// All four cases below run at height 20, where `scrolloff_for` is 4.
    #[test]
    fn a_visible_block_does_not_move_the_view() {
        assert_eq!(bring_into_view((16, 21), 10, 20), 10);
    }

    /// A block above the viewport scrolls up until it clears the top margin,
    /// not until it touches the top edge: an entry brought hard against the
    /// edge is one you cannot see what precedes.
    #[test]
    fn a_block_above_the_viewport_stops_at_the_margin() {
        assert_eq!(bring_into_view((14, 19), 20, 20), 10);
    }

    /// A block just inside the margin still moves. This is the case that made
    /// the margin worth having: the old rule called it visible and left the
    /// selection pinned to the pane's first row.
    #[test]
    fn a_block_inside_the_top_margin_still_scrolls() {
        assert_eq!(bring_into_view((12, 17), 10, 20), 8);
    }

    /// A block below the viewport scrolls down by the least that clears the
    /// bottom margin.
    #[test]
    fn a_block_below_the_viewport_scrolls_the_minimum() {
        assert_eq!(bring_into_view((30, 34), 10, 20), 19);
    }

    /// A block taller than the pane cannot clear both margins, so its top
    /// wins: scrolling to the bottom of an oversized block leaves you looking
    /// at the tail of a message with nothing to say whose it is. This is the
    /// one thing the old top-pinning got right.
    #[test]
    fn an_oversized_block_shows_its_top() {
        assert_eq!(bring_into_view((30, 60), 10, 20), 26);
    }

    /// The margin never eats more than half a short pane, or the two overlap
    /// and the cursor is pinned to the middle instead of walking.
    #[test]
    fn a_short_pane_shrinks_the_margin_rather_than_seizing_up() {
        // height 5 -> pad 2, so a block at row 10 sits two rows down.
        assert_eq!(bring_into_view((10, 10), 0, 5), 8);
        // height 1 -> pad 0, and the row simply becomes the pane.
        assert_eq!(bring_into_view((10, 10), 0, 1), 10);
    }

    /// The staleness bug: git rendered "3 minutes ago" once, the expanded
    /// graph was only refetched when the repo moved, and an idle repo kept
    /// showing it for hours while the commit list beside it said "16h ago".
    #[test]
    fn the_graph_block_date_is_aged_at_draw_time() {
        let then = chrono::Utc::now() - chrono::Duration::hours(16);
        let line = format!("Date:   {}", then.to_rfc3339());
        assert_eq!(
            refresh_date_line(&line).as_deref(),
            Some("Date:   16h ago"),
            "the frozen relative date survived to the screen"
        );
    }

    /// Only the `Date:` row is rewritten. Everything else is git's output and
    /// is reproduced verbatim, which is the whole point of this view.
    #[test]
    fn other_graph_rows_are_left_exactly_as_git_wrote_them() {
        assert_eq!(refresh_date_line("Author: someone <a@b.c>"), None);
        assert_eq!(refresh_date_line("commit 1c51c6e"), None);
        assert_eq!(refresh_date_line("    a subject line"), None);
        assert_eq!(
            refresh_date_line("Date:   Mon Aug 25 09:12:33 2026 +0200"),
            None,
            "an unparseable stamp must pass through, not vanish"
        );
    }

    /// End to end: the rewritten row reaches the drawn line.
    #[test]
    fn the_drawn_graph_block_shows_a_current_age() {
        let then = chrono::Utc::now() - chrono::Duration::days(3);
        let raw = format!("| | Date:   {}", then.to_rfc3339());
        let rows = expanded_graph_lines(&raw, "", 80);
        let text: String = rows[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert!(text.contains("3d ago"), "drew {text:?}");
    }

    /// Phantom blocks: a substring test for "commit " matched message bodies,
    /// so this repo's own log reported thirty block starts for twenty-five
    /// commits and the marker indexed past the commit it was on.
    #[test]
    fn only_gits_commit_row_opens_a_block() {
        assert_eq!(commit_hash_at("* commit 7b1f067"), Some("7b1f067"));
        assert_eq!(
            commit_hash_at("| | commit 7b1f067 (HEAD -> main, origin/main)"),
            Some("7b1f067")
        );
        assert_eq!(
            commit_hash_at("|     notch jumped a whole commit block, which is"),
            None,
            "a message body opened a phantom block"
        );
        assert_eq!(
            commit_hash_at("|     is the same symptom. That commit had the mechanism"),
            None
        );
        assert_eq!(
            commit_hash_at("|     commit blocks are tall"),
            None,
            "a body starting with the word is still not a commit row"
        );
        assert_eq!(commit_hash_at("| Author: Gaurgle <a@b.c>"), None);
    }

    /// The highlight is a row, and it must be a *drawn* row at every offset:
    /// a commit-granular marker could only sit on a `commit <hash>` row, so it
    /// spent whole blocks off screen and the graph looked like it had no
    /// cursor at all.
    #[test]
    fn the_graph_highlight_is_a_drawn_row_at_every_offset() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (w, h) = (120u16, 40u16);
        let mut repo = busy_repo();
        let hashes: Vec<String> = (0..12).map(|i| format!("a1b2c{i:02x}")).collect();
        repo.graph_expanded = hashes
            .iter()
            .flat_map(|hash| {
                let mut block = vec![
                    format!("* commit {hash}"),
                    "| Author: Gaurgle <a@b.c>".to_string(),
                    "| Date:   2026-08-24T16:52:11+02:00".to_string(),
                    "|".to_string(),
                ];
                // A tall body, which is the case that broke: the commit row
                // leaves the top of the pane and does not come back.
                block.extend((0..16).map(|n| format!("|     body line {n}")));
                block.push("|".to_string());
                block
            })
            .collect();
        repo.graph = hashes
            .iter()
            .map(|hash| crate::model::GraphLine {
                hash: hash.clone(),
                refs: vec![],
                author: "Gaurgle".to_string(),
                message: "m".to_string(),
                lanes: vec![],
                is_merge: false,
            })
            .collect();

        let mut app = crate::app::App::new(vec![repo], "h".to_string(), w, h);
        app.view = crate::app::View::Detail;
        app.history_view = crate::app::HistoryView::Graph;
        app.focused_panel = crate::app::FocusedPanel::Detail;

        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| crate::ui::render(f, &app)).unwrap();
        assert!(app.detail_max_scroll.get() > 0, "nothing to scroll");
        let pane = crate::ui::panel_rects(term.get_frame().area(), &app)
            .detail
            .expect("the graph view draws a detail pane");

        let mut rows_seen = std::collections::BTreeSet::new();
        for _ in 0..app.detail_max_scroll.get() {
            app.scroll_detail(1);
            term.draw(|f| crate::ui::render(f, &app)).unwrap();
            let buf = term.backend().buffer().clone();
            // Only inside the detail pane: the repo list draws a caret too.
            let marked: Vec<u16> = (pane.y..pane.y + pane.height)
                .filter(|y| {
                    (pane.x..pane.x + pane.width).any(|x| buf[(x, *y)].symbol() == "\u{25b8}")
                })
                .collect();
            assert_eq!(
                marked.len(),
                1,
                "expected one highlighted row at offset {}, got {marked:?}",
                app.detail_scroll()
            );
            rows_seen.insert(marked[0]);
        }
        assert!(
            rows_seen.len() > 10,
            "the highlight only ever sat on {} rows, so it is not walking",
            rows_seen.len()
        );
    }

    /// The wrap breaks at spaces and keeps every span's styling, so a wrapped
    /// paragraph is not one span per character and no word is cut in half.
    #[test]
    fn wrapping_a_line_keeps_its_words_and_its_styling() {
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let plain = Style::default().fg(colors::TEXT);
        let line = Line::from(vec![
            Span::styled("hash1234 ".to_string(), bold),
            Span::styled("the quick brown fox jumps".to_string(), plain),
        ]);

        let rows = wrap_line(&line, 14);
        assert!(rows.len() > 1, "nothing wrapped");
        for row in &rows {
            let text: String = row.spans.iter().map(|s| s.content.as_ref()).collect();
            assert!(
                text.chars().count() <= 14,
                "row ran past the pane: {text:?}"
            );
            assert!(
                row.spans.len() <= 2,
                "one span per character: {:?}",
                row.spans.len()
            );
        }
        let rejoined: String = rows
            .iter()
            .flat_map(|r| r.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(rejoined, "hash1234 the quick brown fox jumps");
        assert_eq!(rows[0].spans[0].style, bold, "the hash lost its weight");
    }

    /// A word wider than the pane still renders rather than looping, and its
    /// characters are not lost.
    #[test]
    fn wrapping_a_line_survives_a_word_wider_than_the_pane() {
        let line = Line::from(Span::raw("a".repeat(30)));
        let rows = wrap_line(&line, 8);
        assert_eq!(rows.len(), 4);
        let rejoined: String = rows
            .iter()
            .flat_map(|r| r.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(rejoined.chars().count(), 30);
    }

    /// The inspector's scroll must reach the last row of a long body.
    ///
    /// It clamped against `wrapped_row_count`, which counts `ceil(len/width)`
    /// while `Paragraph` broke at spaces: every wrapped paragraph reported
    /// fewer rows than it drew, so the tail of a commit message was
    /// unreachable. The lines are wrapped in the builder now, so the count is
    /// the count.
    #[test]
    fn the_inspector_scrolls_to_the_end_of_a_long_body() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (w, h) = (120u16, 30u16);
        let mut repo = list_repo(3);
        // One long paragraph of long words. Broken at spaces every word takes a
        // row of its own and most of that row's width goes unused, so a
        // `ceil(len / width)` count reports far fewer rows than are drawn --
        // and it is that shortfall that stranded the tail.
        let mut body = "z".repeat(50);
        for _ in 0..40 {
            body.push(' ');
            body.push_str(&"z".repeat(50));
        }
        body.push_str(" TAILMARKER");
        repo.commits[0].body = body;

        let mut app = crate::app::App::new(vec![repo], "h".to_string(), w, h);
        app.view = crate::app::View::Detail;
        app.history_view = crate::app::HistoryView::Commits;

        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| crate::ui::render(f, &app)).unwrap();
        let max = app.inspector_max_scroll.get();
        assert!(max > 0, "the body did not overflow the inspector");

        app.scroll_inspector(max as i32);
        term.draw(|f| crate::ui::render(f, &app)).unwrap();
        let buf = term.backend().buffer().clone();
        let screen: String = (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect();
        assert!(
            screen.contains("TAILMARKER"),
            "the end of the body was past the end of the scroll"
        );
    }

    fn inspector_screen(w: u16, h: u16) -> Vec<String> {
        inspector_screen_at(w, h, crate::app::DEFAULT_INSPECTOR_PCT)
    }

    fn inspector_screen_at(w: u16, h: u16, inspector_pct: u16) -> Vec<String> {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut repo = list_repo(3);
        repo.commits[0].body = "why it changed".to_string();
        repo.commits[0].files = vec![crate::model::FileStat {
            path: "src/lib.rs".to_string(),
            added: 3,
            removed: 1,
        }];
        let mut app = crate::app::App::new(vec![repo], "h".to_string(), w, h);
        app.view = crate::app::View::Detail;
        app.history_view = crate::app::HistoryView::Commits;
        app.contributions_folded = true;
        app.inspector_pct = inspector_pct;
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| crate::ui::render(f, &app)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    fn row_of(screen: &[String], needle: &str) -> usize {
        screen
            .iter()
            .position(|r| r.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} not on screen:\n{}", screen.join("\n")))
    }

    /// A wide inspector puts the changed files in a lane of their own to the
    /// right of the message, so neither has to wait for the other to scroll
    /// past. The files header lands level with the masthead: the body sits
    /// five rows under the hash (hash, author, blank, subject, blank), so it
    /// is the one line only the inspector draws that fixes where that is.
    #[test]
    fn a_wide_inspector_lays_the_files_beside_the_message() {
        let screen = inspector_screen(160, 40);
        let files_row = row_of(&screen, "changed files");
        assert_eq!(
            files_row,
            row_of(&screen, "why it changed") - 5,
            "files lane should start level with the masthead"
        );
        assert!(
            screen[files_row].find("changed files").unwrap() > 80,
            "files on the right half: {:?}",
            screen[files_row]
        );
    }

    /// Below the width threshold the lanes stack as one column, as before.
    #[test]
    fn a_narrow_inspector_stacks_the_files_under_the_message() {
        // Draws at the pre-FZ-88 inspector size (40%) it was written for.
        let screen = inspector_screen_at(90, 40, 40);
        assert!(
            row_of(&screen, "changed files") > row_of(&screen, "why it changed"),
            "files should follow the body"
        );
    }

    /// Stepping the cursor with `j` must keep it on the pane in every view.
    ///
    /// `graph_list_lines` reported the selected row in its own coordinates
    /// while the reflog reported the pane's, so the follow step scrolled to a
    /// row `base` above the selection and the highlight sat that far below the
    /// bottom edge -- and a commit whose index fell inside the pinned header
    /// had its block nulled out by the header subtraction, so the pane did not
    /// follow at all. Only the lists drifted, which is why the reflog looked
    /// fine.
    #[test]
    fn stepping_the_cursor_never_pushes_it_off_the_pane() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (w, h) = (120u16, 30u16);
        let repo = list_repo(40);

        for view in crate::app::HistoryView::ALL {
            let mut app = crate::app::App::new(vec![repo.clone()], "h".to_string(), w, h);
            app.view = crate::app::View::Detail;
            app.history_view = view;
            app.focused_panel = crate::app::FocusedPanel::Detail;
            app.contributions_folded = true;

            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| crate::ui::render(f, &app)).unwrap();
            let pane = crate::ui::panel_rects(term.get_frame().area(), &app)
                .detail
                .expect("a detail pane");
            let caret_row = |term: &Terminal<TestBackend>| {
                let buf = term.backend().buffer().clone();
                (pane.y..pane.y + pane.height).find(|y| {
                    (pane.x..pane.x + pane.width).any(|x| buf[(x, *y)].symbol() == "\u{25b8}")
                })
            };

            for step in 0..app.selected_commit_count() {
                app.detail_nav_down();
                term.draw(|f| crate::ui::render(f, &app)).unwrap();
                assert!(
                    caret_row(&term).is_some(),
                    "{} pushed its cursor off the pane stepping down to {step}",
                    view.label()
                );
            }
            for step in 0..app.selected_commit_count() {
                app.detail_nav_up();
                term.draw(|f| crate::ui::render(f, &app)).unwrap();
                assert!(
                    caret_row(&term).is_some(),
                    "{} pushed its cursor off the pane stepping up to {step}",
                    view.label()
                );
            }
        }
    }

    /// The other views step commit to commit, so scrolling moves the view out
    /// from under their cursor rather than moving the cursor. It used to leave
    /// the highlight off the pane entirely, and `j` then stepped a commit you
    /// could not see. Asserted end to end, at the pane, in every view that
    /// draws a list.
    /// A repo whose every history view has a list long enough to scroll.
    fn list_repo(n: usize) -> RepoStatus {
        let mut repo = busy_repo();
        repo.commits = (0..n)
            .map(|i| crate::model::Commit {
                hash: format!("c0ffee{i:02}"),
                author: "Gaurgle".to_string(),
                date: Some(chrono::Utc::now()),
                message: format!("feat: thing {i}"),
                body: String::new(),
                insertions: 1,
                deletions: 0,
                files: vec![],
            })
            .collect();
        repo.graph = repo
            .commits
            .iter()
            .map(|c| crate::model::GraphLine {
                hash: c.hash.clone(),
                refs: vec![],
                author: c.author.clone(),
                message: c.message.clone(),
                lanes: vec![],
                is_merge: false,
            })
            .collect();
        repo.reflog = repo
            .commits
            .iter()
            .map(|c| crate::model::ReflogEntry {
                hash: c.hash.clone(),
                action: "commit".to_string(),
                description: c.message.clone(),
            })
            .collect();
        repo
    }

    /// Scrolling leaves the selection where it is, so the marker is free to
    /// travel off the pane with the content it belongs to -- that is the
    /// natural behaviour and the whole point of the change. What has to hold
    /// is that a single step brings it back, at the edge being looked at.
    ///
    /// The old contract was the opposite: every wheel notch dragged the
    /// selection to the nearest visible entry, so the wheel silently
    /// reassigned which commit the inspector was reading out.
    #[test]
    fn a_step_after_scrolling_brings_the_selection_back_onto_the_pane() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (w, h) = (120u16, 30u16);
        let repo = list_repo(40);

        // `info` is excluded: its commit list is one section among several, and
        // scrolling past it into the branches and stashes below is the point.
        for view in [
            crate::app::HistoryView::Commits,
            crate::app::HistoryView::Reflog,
        ] {
            let mut app = crate::app::App::new(vec![repo.clone()], "h".to_string(), w, h);
            app.view = crate::app::View::Detail;
            app.history_view = view;
            app.focused_panel = crate::app::FocusedPanel::Detail;

            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| crate::ui::render(f, &app)).unwrap();
            assert!(
                app.detail_max_scroll.get() > 0,
                "{} has nothing to scroll",
                view.label()
            );
            let pane = crate::ui::panel_rects(term.get_frame().area(), &app)
                .detail
                .expect("a detail pane");
            let marker_on_pane = |term: &Terminal<TestBackend>| {
                let buf = term.backend().buffer().clone();
                (pane.y..pane.y + pane.height).any(|y| {
                    (pane.x..pane.x + pane.width).any(|x| buf[(x, y)].symbol() == "\u{25b8}")
                })
            };

            let before = app.detail_selected();
            for _ in 0..app.detail_max_scroll.get() {
                app.scroll_detail(1);
            }
            term.draw(|f| crate::ui::render(f, &app)).unwrap();
            assert_eq!(
                app.detail_selected(),
                before,
                "{} let the wheel move its selection",
                view.label()
            );

            app.detail_nav_down();
            term.draw(|f| crate::ui::render(f, &app)).unwrap();
            assert!(
                marker_on_pane(&term),
                "{}: one step did not bring the selection back onto the pane",
                view.label()
            );
            assert_ne!(
                app.detail_selected(),
                before,
                "{}: the step did not re-enter at all",
                view.label()
            );
        }
    }

    /// The end-to-end version of the marker drag: render the graph view, then
    /// scroll past the second block's first row and check the selection
    /// followed. The app-side tests hand-feed the block tops; this one proves
    /// the renderer actually reports them, which is the half that cannot be
    /// faked.
    #[test]
    fn rendering_the_graph_view_reports_where_each_block_starts() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut repo = busy_repo();
        repo.graph_expanded = (0..20)
            .flat_map(|i| {
                [
                    format!("* commit {i:07}"),
                    "| Author: someone <a@b.c>".to_string(),
                    "|".to_string(),
                    format!("|     subject {i}"),
                    "|".to_string(),
                ]
            })
            .collect();

        let mut app = crate::app::App::new(vec![repo], "h".to_string(), 100, 40);
        app.view = crate::app::View::Detail;
        app.history_view = crate::app::HistoryView::Graph;
        let mut term = Terminal::new(TestBackend::new(100, 40)).unwrap();
        term.draw(|f| crate::ui::render(f, &app)).unwrap();

        let tops = app.detail_row_tops.borrow().clone();
        assert_eq!(tops.len(), 20, "one top per commit block");
        assert!(tops.windows(2).all(|w| w[0] < w[1]), "tops must ascend");

        // Walk the cursor onto the second block's first row.
        app.scroll_detail(tops[1] as i32);
        assert_eq!(app.graph_cursor, tops[1]);
        assert_eq!(
            app.detail_selected(),
            1,
            "the selection did not follow the cursor"
        );
    }

    /// Scrolling the graph view must not panic at any pane size or offset.
    /// The wrap and the bring-into-view arithmetic both do width and height
    /// subtraction, and a deep lane prefix in a narrow pane is where those
    /// run out of room.
    #[test]
    fn the_graph_view_survives_every_scroll_offset() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut repo = busy_repo();
        repo.graph_expanded = (0..40)
            .map(|i| format!("| | | | * commit {i:030} a very long trailing subject line indeed"))
            .collect();

        for (w, h) in [(40u16, 10u16), (80, 30), (24, 6), (200, 50), (16, 5)] {
            for view in crate::app::HistoryView::ALL {
                for focus in [
                    crate::app::FocusedPanel::Detail,
                    crate::app::FocusedPanel::Inspector,
                ] {
                    let mut app = crate::app::App::new(vec![repo.clone()], "h".to_string(), w, h);
                    app.view = crate::app::View::Detail;
                    app.history_view = view;
                    app.focused_panel = focus;
                    for off in [0i32, 1, 5, 50, 500, 5000, -5000] {
                        app.scroll_detail(off);
                        app.detail_nav_down();
                        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                        term.draw(|f| crate::ui::render(f, &app)).unwrap();
                    }
                }
            }
        }
    }

    /// The invariant CLAUDE.md names: the cursor walks `App::visible_repos`
    /// while the eye walks what `render_repo_list` draws. Nothing checked
    /// that the two agreed about section order, so reordering one and
    /// forgetting the other misrouted every click below the first section
    /// with nothing failing. This reads the drawn headers back off the
    /// buffer and holds them against the order the cursor uses.
    #[test]
    fn drawn_section_order_matches_the_order_the_cursor_walks() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut repos = vec![
            repo("elsewhere-one", RepoKind::Public, None),
            repo("local-one", RepoKind::Public, None),
            repo("primary-one", RepoKind::Public, None),
        ];
        repos[0].location = RepoLocation::Elsewhere;
        repos[1].location = RepoLocation::LocalOnly;
        repos[2].location = RepoLocation::Primary;

        let app = crate::app::App::new(repos, "t".to_string(), 120, 40);

        let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
        term.draw(|f| render_repo_list(f, f.area(), &app)).unwrap();
        let buf = term.backend().buffer().clone();
        let screen: Vec<String> = (0..40u16)
            .map(|y| (0..120u16).map(|x| buf[(x, y)].symbol()).collect())
            .collect();

        // Headers as drawn, top to bottom.
        let drawn: Vec<&str> = screen
            .iter()
            .filter_map(|row| {
                ["github activity", "local changes", "also active"]
                    .into_iter()
                    .find(|label| row.contains(label))
            })
            .collect();
        assert_eq!(
            drawn,
            vec!["github activity", "local changes", "also active"],
            "drew: {screen:#?}"
        );

        // And the same order out of the cursor's own list, so the two cannot
        // drift apart without this failing.
        let walked: Vec<&str> = app
            .visible_repos()
            .iter()
            .map(|(_, r)| match r.location {
                RepoLocation::Primary => "github activity",
                RepoLocation::LocalOnly => "local changes",
                RepoLocation::Elsewhere | RepoLocation::NotCloned => "also active",
            })
            .collect();
        assert_eq!(walked, drawn, "cursor order disagrees with drawn order");
    }

    /// FZ-56: the repo list drawn for a ranked owner and author search, as
    /// screen rows, with the app that drew it.
    fn draw_people_search(query: &str) -> (crate::app::App, Vec<String>) {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut tools = repo("tools-kestrel", RepoKind::Public, Some("Gaurgle"));
        tools.commits = vec![crate::model::Commit {
            hash: "a1".to_string(),
            message: "subject".to_string(),
            body: String::new(),
            author: "Andreas Roos".to_string(),
            date: None,
            insertions: 0,
            deletions: 0,
            files: vec![],
        }];
        let web = repo("tools-web", RepoKind::Org, Some("kestrel-labs"));
        let mut scratch = repo("tools-scratch", RepoKind::Unknown, None);
        scratch.location = RepoLocation::LocalOnly;
        scratch.commits = tools.commits.clone();
        scratch.commits[0].author = "Kestrel Hansen".to_string();

        let mut app = crate::app::App::new(vec![scratch, web, tools], "t".to_string(), 120, 40);
        app.search_query = query.to_string();
        let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
        term.draw(|f| render_repo_list(f, f.area(), &app)).unwrap();
        let buf = term.backend().buffer().clone();
        let screen = (0..40u16)
            .map(|y| (0..120u16).map(|x| buf[(x, y)].symbol()).collect())
            .collect();
        (app, screen)
    }

    /// The ranked order drawn for an owner query is the order the cursor
    /// walks, with the name hit first, then the organisation, then the
    /// contributor: one list, so a click lands on the row it names.
    #[test]
    fn an_owner_search_draws_the_order_the_cursor_walks() {
        let (app, screen) = draw_people_search("kestrel");
        let names = ["tools-kestrel", "tools-web", "tools-scratch"];
        let drawn: Vec<&str> = screen
            .iter()
            .filter_map(|row| names.into_iter().find(|n| row.contains(n)))
            .collect();
        let walked: Vec<&str> = app
            .visible_repos()
            .iter()
            .map(|(_, r)| r.name.as_str())
            .collect();
        assert_eq!(drawn, names, "drew: {screen:#?}");
        assert_eq!(walked, drawn, "cursor order disagrees with drawn order");
    }

    /// A hit on the owner, organisation or an author is marked after the
    /// name, dimmed; a name hit carries no mark.
    #[test]
    fn a_non_name_search_hit_is_marked_with_its_field() {
        let row = |screen: &[String], name: &str| {
            screen
                .iter()
                .find(|row| row.contains(name))
                .cloned()
                .unwrap_or_else(|| panic!("{name} not drawn: {screen:#?}"))
        };
        let (_, screen) = draw_people_search("kestrel");
        assert!(
            row(&screen, "tools-kestrel").contains("tools-kestrel ·"),
            "a name hit is unmarked"
        );
        assert!(row(&screen, "tools-web").contains("tools-web org ·"));
        assert!(row(&screen, "tools-scratch").contains("tools-scratch author ·"));

        let (_, screen) = draw_people_search("gaurgle");
        assert!(row(&screen, "tools-kestrel").contains("tools-kestrel owner ·"));

        // Dimmed in the strip's own grey, not a new color.
        let (app, _) = draw_people_search("gaurgle");
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
        term.draw(|f| render_repo_list(f, f.area(), &app)).unwrap();
        let buf = term.backend().buffer().clone();
        let y = (0..40u16)
            .find(|&y| {
                (0..120u16)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .contains("owner")
            })
            .unwrap();
        let line: String = (0..120u16).map(|x| buf[(x, y)].symbol()).collect();
        let x = line.find("owner").unwrap();
        let x = line[..x].chars().count() as u16;
        assert_eq!(buf[(x, y)].fg, colors::OVERLAY);
    }

    /// A local-only repo has no `pushedAt` to date it by -- that is what puts
    /// it in "local changes" in the first place -- so `info` is the only
    /// place its age is legible. It is also the number the time window uses
    /// to decide whether the repo keeps its row, so seeing it explains an
    /// eviction rather than leaving it a mystery.
    #[test]
    fn info_dates_a_local_only_repo() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let draw = |last_local: Option<chrono::DateTime<chrono::Utc>>| {
            let mut r = busy_repo();
            r.location = RepoLocation::LocalOnly;
            r.last_local = last_local;
            let mut app = crate::app::App::new(vec![r], "t".to_string(), 96, 44);
            app.history_view = HistoryView::Info;
            let mut term = Terminal::new(TestBackend::new(96, 60)).unwrap();
            term.draw(|f| render_detail_pane(f, f.area(), &app))
                .unwrap();
            let buf = term.backend().buffer().clone();
            (0..60u16)
                .map(|y| (0..96u16).map(|x| buf[(x, y)].symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        };

        let at = chrono::Utc::now() - chrono::Duration::days(95);
        let shown = draw(Some(at));
        assert!(shown.contains("last touched"), "no row drawn: {shown}");
        assert!(
            shown.contains(&at.format("%Y-%m-%d").to_string()),
            "the date itself must be there, not just an age: {shown}"
        );
        assert!(
            shown.contains("3mo ago"),
            "and the age beside it, as the commit rows read: {shown}"
        );

        // A repo git could not date at all says nothing rather than lying
        // about it with a zero or an epoch.
        assert!(!draw(None).contains("last touched"));
    }

    // --- FZ-09d: GitHub values from an earlier refresh (O4) ---

    use ratatui::style::Color as TermColor;

    /// Two `Pending` rows with two PRs each, `stale` and `fresh`, under
    /// watch, with `stale` not in the last poll.
    fn app_with_a_stale_row() -> crate::app::App {
        let mut repos = vec![
            repo("stale", RepoKind::Public, Some("alice")),
            repo("fresh", RepoKind::Public, Some("alice")),
        ];
        for r in &mut repos {
            r.ci_status = CiStatus::Pending;
            r.pr_count = 2;
        }
        let mut app = crate::app::App::new(repos, "t".to_string(), 120, 40);
        app.watch_mode = true;
        app.not_in_last_poll
            .insert((Some("alice".to_string()), "stale".to_string()));
        app
    }

    /// The colour of the CI lamp and of the PR count on the list row naming
    /// `name`, as drawn.
    fn list_colors(app: &crate::app::App, name: &str) -> (TermColor, TermColor) {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut term = Terminal::new(TestBackend::new(60, 12)).unwrap();
        term.draw(|f| render_repo_list(f, f.area(), app)).unwrap();
        let buf = term.backend().buffer().clone();
        let y = (0..12u16)
            .find(|&y| {
                let row: String = (0..60u16).map(|x| buf[(x, y)].symbol()).collect();
                row.contains(&format!(" {name} "))
            })
            .expect("row drawn");
        let at = |symbol: &str| {
            (0..60u16)
                .find(|&x| buf[(x, y)].symbol() == symbol)
                .map(|x| buf[(x, y)].fg)
                .expect("cell drawn")
        };
        (at("◉"), at("P"))
    }

    #[test]
    fn a_row_not_in_the_last_poll_is_drawn_dimmed_and_its_lamp_holds_still() {
        let mut app = app_with_a_stale_row();
        for blink_on in [true, false] {
            app.blink_on = blink_on;
            assert_eq!(
                list_colors(&app, "stale"),
                (badges::EARLIER_REFRESH, badges::EARLIER_REFRESH),
                "blink_on {blink_on}"
            );
        }
    }

    #[test]
    fn a_row_in_the_last_poll_is_drawn_as_before() {
        let mut app = app_with_a_stale_row();
        app.blink_on = true;
        assert_eq!(list_colors(&app, "fresh"), (colors::PEACH, colors::PR_CLR));
        app.blink_on = false;
        assert_eq!(
            list_colors(&app, "fresh"),
            (colors::SURFACE, colors::PR_CLR)
        );
    }

    // --- FZ-66: the cursor's row in pane 1 wears a band ---

    /// Two repos with two PRs each, so every row carries a badge group.
    fn two_repo_app() -> crate::app::App {
        let mut repos = vec![
            repo("alpha", RepoKind::Public, Some("alice")),
            repo("beta", RepoKind::Public, Some("alice")),
        ];
        for r in &mut repos {
            r.pr_count = 2;
        }
        crate::app::App::new(repos, "t".to_string(), 120, 40)
    }

    #[test]
    fn the_cursor_row_wears_the_band_and_bold_dots_and_others_do_not() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let app = two_repo_app();
        let (w, h) = (60u16, 12u16);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render_repo_list(f, f.area(), &app)).unwrap();
        let buf = term.backend().buffer().clone();
        // Inside the border and the one column of padding on each side.
        let inner = 2..w - 2;
        let row_of = |name: &str| {
            (0..h)
                .find(|&y| {
                    let row: String = (0..w).map(|x| buf[(x, y)].symbol()).collect();
                    row.contains(&format!(" {name} "))
                })
                .expect("row drawn")
        };
        let (a, b) = (row_of("alpha"), row_of("beta"));
        let (on, off) = if buf[(2, a)].symbol() == "▸" {
            (a, b)
        } else {
            (b, a)
        };
        assert_eq!(buf[(2, on)].symbol(), "▸", "one row is the cursor's");
        assert_eq!(buf[(2, off)].symbol(), " ");

        for x in inner.clone() {
            assert_eq!(buf[(x, on)].bg, colors::SELECTED_BG, "band at x {x}");
            assert_eq!(buf[(x, off)].bg, TermColor::Reset, "no band at x {x}");
        }
        let dots = |y: u16| -> Vec<u16> {
            inner
                .clone()
                .filter(|&x| buf[(x, y)].symbol() == "·")
                .collect()
        };
        assert!(!dots(on).is_empty() && !dots(off).is_empty());
        for x in dots(on) {
            let cell = &buf[(x, on)];
            assert_eq!(cell.fg, colors::OVERLAY, "cursor dot at x {x}");
            assert!(cell.modifier.contains(Modifier::BOLD), "bold at x {x}");
        }
        for x in dots(off) {
            let cell = &buf[(x, off)];
            assert_eq!(cell.fg, colors::SURFACE, "dot at x {x}");
            assert!(!cell.modifier.contains(Modifier::BOLD), "plain at x {x}");
        }
    }

    #[test]
    fn the_band_and_bold_dots_move_no_column() {
        use ratatui::buffer::Buffer;
        use ratatui::widgets::Widget;

        let app = two_repo_app();
        let r = &app.repos[0];
        let width = 40u16;
        let drawn = |selected: bool| {
            let line = Line::from(repo_row_spans(
                r,
                selected,
                width as usize,
                false,
                false,
                None,
            ));
            let cells = line.width();
            let area = Rect::new(0, 0, width + 10, 1);
            let mut buf = Buffer::empty(area);
            line.render(area, &mut buf);
            let symbols: Vec<String> = (0..area.width)
                .map(|x| buf[(x, 0)].symbol().to_string())
                .collect();
            (cells, symbols)
        };
        let (on_w, on) = drawn(true);
        let (off_w, off) = drawn(false);
        assert_eq!(on_w, width as usize, "the cursor's row fills its width");
        assert_eq!(on_w, off_w, "same number of cells");
        let badge_at = |s: &[String]| s.iter().position(|c| c == "P").expect("PR badge");
        assert_eq!(badge_at(&on), badge_at(&off), "badges in the same column");
        assert_eq!(on[2..], off[2..], "only the marker differs");
    }

    /// The detail pane of the selected repo, as text, and the colour of its
    /// CI lamp.
    fn detail_of(app: &crate::app::App) -> (String, TermColor) {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (w, h) = (100u16, 24u16);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), app)).unwrap();
        let buf = term.backend().buffer().clone();
        let text = (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        let lamp = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .find(|&(x, y)| buf[(x, y)].symbol() == "◉")
            .map(|(x, y)| buf[(x, y)].fg)
            .expect("lamp drawn");
        (text, lamp)
    }

    #[test]
    fn the_detail_pane_says_a_repo_not_in_the_last_poll_is_from_an_earlier_refresh() {
        let mut app = app_with_a_stale_row();
        app.blink_on = true;
        // The first row, `stale`, is selected.
        let (text, lamp) = detail_of(&app);
        assert!(text.contains(EARLIER_REFRESH_NOTE), "{text}");
        assert_eq!(lamp, badges::EARLIER_REFRESH);

        app.not_in_last_poll.clear();
        let (text, lamp) = detail_of(&app);
        assert!(!text.contains(EARLIER_REFRESH_NOTE), "{text}");
        assert_eq!(lamp, colors::PEACH);
    }

    /// FZ-37 review: a hit carries the entry's own list index and the hash
    /// as drawn, so a top shed into the pinned header cannot shift the index
    /// and a later list change cannot change what a click copies.
    #[test]
    fn a_hit_carries_the_drawn_hash_and_the_entry_index() {
        let lines = vec![
            Line::from("  aaa1111 first"),
            Line::from("  bbb2222 second"),
        ];
        // Entry 0 sat in the pinned header, so it has no body line.
        let tops = [None, Some(0), Some(1)];
        let hash_of = |i: usize| ["zzz9999", "aaa1111", "bbb2222"].get(i).copied();
        let body = Rect::new(10, 5, 40, 10);
        let hits = DetailHits::measure(&lines, &tops, hash_of, body, 0, false);

        let on_hash = hits.at(12, 6).expect("a row");
        assert_eq!(on_hash.entry, Some(2));
        assert_eq!(on_hash.hash.as_deref(), Some("bbb2222"));
        assert_eq!(on_hash.line, 1);

        let off_hash = hits.at(12 + 8, 6).expect("a row");
        assert_eq!((off_hash.entry, off_hash.hash), (Some(2), None));

        assert_eq!(hits.at(12, 7), None, "below the last line");
    }

    // --- FZ-55: a scroll margin on every list, reveal on expand ---

    /// Pane 1 drawn `height` rows tall: the `visible_repos` index of each repo
    /// row on screen, top to bottom, and the index of the selected one.
    fn drawn_repo_window(app: &crate::app::App, height: u16) -> (Vec<usize>, usize) {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let names: Vec<String> = app
            .visible_repos()
            .iter()
            .map(|(_, r)| format!(" {} ", r.name))
            .collect();
        let mut term = Terminal::new(TestBackend::new(60, height)).unwrap();
        term.draw(|f| render_repo_list(f, f.area(), app)).unwrap();
        let buf = term.backend().buffer().clone();
        let drawn = (0..height)
            .filter_map(|y| {
                let row: String = (0..60u16).map(|x| buf[(x, y)].symbol()).collect();
                names.iter().position(|n| row.contains(n.as_str()))
            })
            .collect();
        let selected = format!(" {} ", app.selected_repo().expect("a selection").name);
        let selected = names.iter().position(|n| *n == selected).expect("listed");
        (drawn, selected)
    }

    /// Walks pane 1 down to the last repo and back, checking the drawn
    /// window after every step: `margin` rows stay past the cursor on the
    /// side it heads for, fewer only where the list itself runs out.
    fn walk_repo_list(height: u16, margin: usize) {
        let repos = (0..40)
            .map(|i| repo(&format!("list-{i:02}"), RepoKind::Public, None))
            .collect();
        let mut app = crate::app::App::new(repos, "t".to_string(), 60, height);
        let last = app.visible_repos().len() - 1;
        let mut parked = false;
        let check = |app: &crate::app::App, down: bool, parked: &mut bool| {
            let (drawn, selected) = drawn_repo_window(app, height);
            let pos = drawn
                .iter()
                .position(|&i| i == selected)
                .expect("on screen");
            let (above, below) = (pos, drawn.len() - 1 - pos);
            if down {
                assert!(
                    below >= margin.min(last - selected),
                    "cursor {selected} has {below} rows below it: {drawn:?}"
                );
                *parked |= drawn[0] > 0 && below == margin;
            } else {
                assert!(
                    above >= margin.min(selected),
                    "cursor {selected} has {above} rows above it: {drawn:?}"
                );
            }
        };
        for _ in 0..last {
            app.move_down();
            check(&app, true, &mut parked);
        }
        assert!(parked, "the list never scrolled with the margin held");
        for _ in 0..last {
            app.move_up();
            check(&app, false, &mut parked);
        }
    }

    /// Twenty rows less the border and the filter strip: sixteen list rows,
    /// room for the full margin pane 2 keeps.
    #[test]
    fn the_repo_list_keeps_the_shared_margin_past_the_cursor() {
        assert_eq!(crate::app::scrolloff_for(16), crate::app::SCROLLOFF);
        walk_repo_list(20, usize::from(crate::app::SCROLLOFF));
    }

    /// Eight list rows cannot hold the full margin on each side of the
    /// cursor, so it shrinks to (8 - 1) / 2 = 3 rather than pinning it.
    #[test]
    fn a_short_repo_list_shrinks_the_margin() {
        assert_eq!(crate::app::scrolloff_for(8), 3);
        walk_repo_list(12, 3);
    }

    /// A repo whose commits, reflog and local branches each run to `n`
    /// one-row entries, so every one of those views overflows the pane.
    fn long_lists_repo(n: usize) -> RepoStatus {
        let mut r = busy_repo();
        r.graph = (0..n)
            .map(|i| crate::model::GraphLine {
                lanes: vec![crate::graph::commit_cell()],
                hash: format!("h{i:06}"),
                is_merge: false,
                refs: vec![],
                author: "Gaurgle".to_string(),
                message: format!("subject {i}"),
            })
            .collect();
        r.reflog = (0..n)
            .map(|i| crate::model::ReflogEntry {
                hash: format!("h{i:06}"),
                action: "commit".to_string(),
                description: format!("subject {i}"),
            })
            .collect();
        r.branches = (0..n)
            .map(|i| crate::model::BranchStatus {
                name: format!("branch-{i:02}"),
                upstream: Some(format!("origin/branch-{i:02}")),
                behind: 0,
                ahead: 0,
                is_head: i == 0,
                tip: String::new(),
            })
            .collect();
        r
    }

    /// Draws pane 2 `height` rows tall and returns the cursor's rows of
    /// margin above and below it on screen, and the rows of content past it
    /// either way, as the renderer measured them.
    fn detail_cursor_margins(app: &crate::app::App, height: u16) -> (u16, u16, u16, u16) {
        let body = draw_detail(app, height);
        let top = app.detail_row_tops.borrow()[app.detail_selected()];
        let pos = top - app.detail_scroll();
        let content = app.detail_max_scroll.get() + body;
        (pos, body - 1 - pos, top, content - 1 - top)
    }

    /// Draws pane 2 `height` rows tall and returns its scrolling body's
    /// height, as the renderer measured it.
    fn draw_detail(app: &crate::app::App, height: u16) -> u16 {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut term = Terminal::new(TestBackend::new(96, height)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), app)).unwrap();
        app.detail_view_height.get()
    }

    /// Steps pane 2's cursor down `steps` entries and back with `j` and `k`,
    /// checking the margin after every draw.
    fn walk_detail(view: HistoryView, height: u16, steps: usize) {
        let mut app = crate::app::App::new(vec![long_lists_repo(80)], "t".to_string(), 96, 40);
        app.history_view = view;
        app.focused_panel = crate::app::FocusedPanel::Detail;
        let margin = crate::app::scrolloff_for(draw_detail(&app, height));
        let mut parked = false;
        for _ in 0..steps {
            app.detail_nav_down();
            let (_, below, _, past) = detail_cursor_margins(&app, height);
            assert!(below >= margin.min(past), "{view:?}: {below} rows below");
            parked |= app.detail_scroll() > 0 && below == margin;
        }
        assert!(parked, "{view:?}: never scrolled with the margin held");
        for _ in 0..steps {
            app.detail_nav_up();
            let (above, _, before, _) = detail_cursor_margins(&app, height);
            assert!(above >= margin.min(before), "{view:?}: {above} rows above");
        }
    }

    /// `commits`, the longest list, through the cursor-follow path `j` and
    /// `k` take: the full `SCROLLOFF` past the cursor in a tall pane, the
    /// shrunken margin in a short one.
    #[test]
    fn the_commit_list_keeps_the_shared_margin_past_the_cursor() {
        let mut app = crate::app::App::new(vec![long_lists_repo(80)], "t".to_string(), 96, 40);
        app.history_view = HistoryView::Commits;
        let tall = draw_detail(&app, 24);
        assert_eq!(
            crate::app::scrolloff_for(tall),
            crate::app::SCROLLOFF,
            "the pane is too short for the full margin: body {tall}"
        );
        walk_detail(HistoryView::Commits, 24, 60);

        let short = draw_detail(&app, 10);
        assert!(
            crate::app::scrolloff_for(short) < crate::app::SCROLLOFF,
            "body {short}"
        );
        walk_detail(HistoryView::Commits, 10, 60);
    }

    /// One margin for every view the cursor walks: each parks its cursor the
    /// same `scrolloff_for` rows off the bottom, so a change to that function
    /// moves all of them.
    #[test]
    fn every_history_view_parks_its_cursor_on_the_shared_margin() {
        for view in [
            HistoryView::Commits,
            HistoryView::Reflog,
            HistoryView::Branches,
        ] {
            let mut app = crate::app::App::new(vec![long_lists_repo(80)], "t".to_string(), 96, 40);
            app.history_view = view;
            app.focused_panel = crate::app::FocusedPanel::Detail;
            draw_detail(&app, 24);
            for _ in 0..40 {
                app.detail_nav_down();
                draw_detail(&app, 24);
            }
            let (_, below, _, _) = detail_cursor_margins(&app, 24);
            let body = app.detail_view_height.get();
            assert_eq!(below, crate::app::scrolloff_for(body), "{view:?}");
            walk_detail(view, 24, 50);
        }
    }

    /// Draws `info` `height` rows tall and returns the screen text.
    fn info_text(app: &crate::app::App, height: u16) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut term = Terminal::new(TestBackend::new(92, height)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), app)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..height)
            .map(|y| (0..92u16).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Short enough that the branches block scrolls with the body (nine
    /// rows, a margin of four), so the remote-only summary sits low in it
    /// and the names `B` opens run past the bottom edge.
    const REVEAL_HEIGHT: u16 = 18;

    /// The body row, counted from the pane's top edge, holding `needle`.
    fn text_row_of(text: &str, needle: &str) -> Option<usize> {
        text.lines().position(|l| l.contains(needle))
    }

    /// `B` on a remotes list that overflows the pane scrolls it into view:
    /// five names open one row past the bottom edge, and the reveal brings
    /// the last onto the bottom row.
    #[test]
    fn expanding_the_remotes_reveals_the_rows_it_opened() {
        let mut r = spread_repo();
        r.remote_only_branches.truncate(5);
        let mut app = crate::app::App::new(vec![r], "t".to_string(), 92, 40);
        app.history_view = HistoryView::Info;
        let before = info_text(&app, REVEAL_HEIGHT);
        assert!(before.contains("5 remote branches not fetched"), "{before}");
        assert_eq!(app.detail_scroll(), 0);

        app.expand_remote_branches = true;
        let after = info_text(&app, REVEAL_HEIGHT);
        assert_eq!(app.detail_scroll(), 1, "the pane did not move:\n{after}");
        // The last text row sits above the bottom border.
        assert_eq!(
            text_row_of(&after, "spare-4 "),
            Some(usize::from(REVEAL_HEIGHT) - 2),
            "the last opened row is not on the bottom row:\n{after}"
        );
        assert!(after.contains("spare-0 "), "{after}");
    }

    /// Twelve names cannot all show in a nine-row body, so the reveal stops
    /// with the first of them on the top margin rather than scrolling it off.
    #[test]
    fn a_reveal_taller_than_the_pane_stops_at_the_top_margin() {
        let mut app = crate::app::App::new(vec![spread_repo()], "t".to_string(), 92, 40);
        app.history_view = HistoryView::Info;
        info_text(&app, REVEAL_HEIGHT);
        app.expand_remote_branches = true;
        let after = info_text(&app, REVEAL_HEIGHT);
        assert!(app.detail_scroll() > 0, "the pane did not move:\n{after}");
        let body_top = usize::from(REVEAL_HEIGHT - app.detail_view_height.get() - 1);
        let margin = usize::from(crate::app::scrolloff_for(app.detail_view_height.get()));
        assert_eq!(
            text_row_of(&after, "spare-0 "),
            Some(body_top + margin),
            "the expanded row is not on the top margin:\n{after}"
        );
    }

    /// Without the reveal the names open below the fold, which is the bug:
    /// the same draw with the expand already recorded leaves the offset at 0.
    /// Also covers "only on the frame it flips": scrolling back up after the
    /// reveal is left alone.
    #[test]
    fn the_reveal_fires_once_per_expand() {
        let mut app = crate::app::App::new(vec![spread_repo()], "t".to_string(), 92, 40);
        app.history_view = HistoryView::Info;
        info_text(&app, REVEAL_HEIGHT);
        app.expand_remote_branches = true;
        info_text(&app, REVEAL_HEIGHT);
        assert!(app.detail_scroll() > 0);

        app.set_detail_scroll(0);
        let text = info_text(&app, REVEAL_HEIGHT);
        assert_eq!(app.detail_scroll(), 0, "the reveal ran again:\n{text}");

        // Collapsing never scrolls, and expanding again reveals again.
        app.expand_remote_branches = false;
        info_text(&app, REVEAL_HEIGHT);
        assert_eq!(app.detail_scroll(), 0);
        app.expand_remote_branches = true;
        info_text(&app, REVEAL_HEIGHT);
        assert!(app.detail_scroll() > 0, "a second expand did not reveal");
    }

    /// An expand that opens no rows (no remote-only branches, or a view that
    /// does not draw them) leaves the pane where it was.
    #[test]
    fn an_expand_that_opens_nothing_does_not_scroll() {
        let mut r = spread_repo();
        r.remote_only_branches.clear();
        let mut app = crate::app::App::new(vec![r], "t".to_string(), 92, 40);
        app.history_view = HistoryView::Info;
        info_text(&app, REVEAL_HEIGHT);
        app.expand_remote_branches = true;
        info_text(&app, REVEAL_HEIGHT);
        assert_eq!(app.detail_scroll(), 0);

        // `B` pressed in `commits`, then back to `info`: nothing opened on
        // the frame it flipped, so nothing is revealed later.
        let mut app = crate::app::App::new(vec![spread_repo()], "t".to_string(), 92, 40);
        app.history_view = HistoryView::Commits;
        info_text(&app, REVEAL_HEIGHT);
        app.expand_remote_branches = true;
        info_text(&app, REVEAL_HEIGHT);
        app.history_view = HistoryView::Info;
        app.set_detail_scroll(0);
        info_text(&app, REVEAL_HEIGHT);
        assert_eq!(app.detail_scroll(), 0);
    }

    // --- The `notes` view (FZ-53) ---

    /// An app on `busy_repo` showing `notes`, with a walk of `files` landed.
    fn notes_view_app(files: &[(&str, Option<&str>)]) -> crate::app::App {
        let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 100, 44);
        app.view = crate::app::View::Detail;
        app.history_view = HistoryView::Notes;
        app.inspector_folded = false;
        let key = app.repos[0].path.clone().expect("cloned");
        let notes = crate::notes::Notes {
            files: files
                .iter()
                .map(|(rel, notez)| crate::notes::NoteFile {
                    rel: (*rel).into(),
                    notez: notez.map(Into::into),
                })
                .collect(),
        };
        let walk = crate::app::NotesWalk {
            root: key.clone().into(),
            key,
            generation: 0,
        };
        app.apply_notes_walk(walk, Ok(notes));
        app
    }

    /// Pane 2 in the `notes` layout, 120 columns wide: the tree takes its
    /// share beside the preview (FZ-67), and at this width that share holds
    /// its longest message, `notez: no notez/ or .notez/ directory`, on one
    /// row.
    fn notes_pane_text(app: &crate::app::App) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), app)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..30u16)
            .map(|y| {
                (0..120u16)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The tree draws each notes directory as a section, nested directories
    /// indented, and the caret on the cursor's file; the cursor walks files
    /// only, so its second stop is the nested file, not the directory.
    #[test]
    fn the_notes_view_draws_sections_and_marks_the_file() {
        let mut app = notes_view_app(&[
            ("notez/a.md", Some("notez")),
            ("notez/sub/b.md", Some("notez")),
            ("crates/x/notez/c.md", Some("crates/x/notez")),
            ("docs/guide.md", None),
        ]);
        app.select_commit_down();
        let text = notes_pane_text(&app);
        for want in [
            "notes: [notez] markdown  (m)",
            "notez/",
            "  sub/",
            "crates/x/notez/",
        ] {
            assert!(text.contains(want), "{want} missing:\n{text}");
        }
        let glyph = badges::note_kind_icon(crate::notes::NoteKind::Markdown);
        assert!(
            text.contains(&format!("▸   {glyph} b.md")),
            "caret on the nested file:\n{text}"
        );
        assert!(!text.contains("guide.md"), "docs only under the toggle");
        assert_eq!(app.detail_row_tops.borrow().len(), 3, "one stop per file");

        app.toggle_notes_markdown();
        let text = notes_pane_text(&app);
        assert!(
            text.contains("docs/") && text.contains("guide.md"),
            "{text}"
        );
    }

    /// Each empty case says what it is in one line.
    #[test]
    fn the_notes_view_says_why_it_lists_nothing() {
        let app = notes_view_app(&[("docs/guide.md", None)]);
        assert!(notes_pane_text(&app).contains("no notez/ or .notez/ directory"));

        let mut app = notes_view_app(&[]);
        app.notes.clear();
        assert!(notes_pane_text(&app).contains("reading notes…"));

        app.repos[0].path = None;
        assert!(notes_pane_text(&app).contains("not cloned: no notes to read"));
    }

    /// The reader (FZ-62, formerly the inspector) reads the highlighted file:
    /// its name in the border, then its body, or the read's error in place.
    #[test]
    fn the_notes_inspector_shows_the_body_or_the_error() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let draw = |app: &crate::app::App| {
            let mut term = Terminal::new(TestBackend::new(60, 12)).unwrap();
            term.draw(|f| render_reader(f, f.area(), app)).unwrap();
            let buf = term.backend().buffer().clone();
            (0..12u16)
                .map(|y| (0..60u16).map(|x| buf[(x, y)].symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
        let read = app.claim_note_read().expect("a read");
        assert!(draw(&app).contains("reading…"));

        app.apply_note_read(read.clone(), Ok("# Plan\nship it".to_string()));
        let text = draw(&app);
        for want in ["a.md · markdown", "# Plan", "ship it"] {
            assert!(text.contains(want), "{want} missing:\n{text}");
        }

        app.note_body.as_mut().unwrap().text = None;
        app.apply_note_read(read, Err("permission denied".to_string()));
        assert!(draw(&app).contains("could not read: permission denied"));
    }

    // --- The `notes` reader and card (FZ-62), the mode line (FZ-57) ---

    /// Draw `paint` into a `w` by `h` buffer and return its rows.
    fn drawn_rows(w: u16, h: u16, paint: impl FnOnce(&mut Frame)) -> Vec<String> {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(paint).unwrap();
        let buf = term.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect()
    }

    /// The first line names both modes, the active one bracketed, and `m`
    /// moves the brackets; each empty state names the mode it belongs to.
    #[test]
    fn the_mode_line_marks_the_active_mode_and_m_moves_it() {
        let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
        let text = notes_pane_text(&app);
        assert!(text.contains("notes: [notez] markdown  (m)"), "{text}");

        app.toggle_notes_markdown();
        let text = notes_pane_text(&app);
        assert!(text.contains("notes: notez [markdown]  (m)"), "{text}");

        let mut app = notes_view_app(&[]);
        assert!(notes_pane_text(&app).contains("notez: no notez/ or .notez/ directory"));
        app.toggle_notes_markdown();
        assert!(notes_pane_text(&app).contains("markdown: no markdown files"));
    }

    /// Each file wears its kind's glyph before its name, the Nerd Font icon
    /// or, under `icons = "plain"`, its one-letter stand-in.
    #[test]
    fn the_tree_draws_each_files_kind_glyph_in_both_sets() {
        use crate::notes::NoteKind;
        let files = [
            ("notez/TODO.md", NoteKind::Todo),
            ("notez/diagram.png", NoteKind::Other),
            ("notez/notes.txt", NoteKind::Text),
            ("notez/plan.md", NoteKind::Markdown),
        ];
        let walk: Vec<(&str, Option<&str>)> =
            files.iter().map(|(f, _)| (*f, Some("notez"))).collect();
        for icons in [crate::app::Icons::Nerd, crate::app::Icons::Plain] {
            badges::set_active_icons(icons);
            let app = notes_view_app(&walk);
            let text = notes_pane_text(&app);
            for (rel, kind) in files {
                let name = rel.trim_start_matches("notez/");
                let want = format!("{} {name}", badges::note_kind_icon(kind));
                assert!(text.contains(&want), "{icons:?}: {want} missing:\n{text}");
            }
        }
        assert!(notes_pane_text(&notes_view_app(&walk)).contains("T TODO.md"));
    }

    /// The reader's border names the file, its kind and size, and its body
    /// scrolls by its own offset: the tree's cursor stays where it was, and
    /// a scrolled body starts further down.
    #[test]
    fn the_reader_titles_the_file_and_scrolls_its_body() {
        let mut app = notes_view_app(&[("notez/README.md", Some("notez"))]);
        app.focused_panel = FocusedPanel::Reader;
        let read = app.claim_note_read().expect("a read");
        let body: Vec<String> = (1..=40).map(|i| format!("line {i}")).collect();
        let facts = crate::notes::NoteFacts {
            size: Some(4300),
            ..Default::default()
        };
        app.apply_note_facts(&read, facts);
        app.apply_note_read(read, Ok(body.join("\n")));

        // Wide enough for `notes preview` and the facts on one border (FZ-67).
        let draw = |app: &crate::app::App| drawn_rows(70, 12, |f| render_reader(f, f.area(), app));
        let rows = draw(&app);
        assert!(
            rows[0].contains("README.md · markdown · 4.2 KiB"),
            "{}",
            rows[0]
        );
        assert!(rows[1].contains("line 1 "), "{}", rows[1]);

        assert_eq!(app.reader_view_height.get(), 10, "one H/L page measured");
        app.scroll_reader(10);
        let rows = draw(&app);
        assert!(rows[1].contains("line 11"), "{}", rows.join("\n"));
        assert!(!rows.join("\n").contains("line 1 "), "{}", rows.join("\n"));
        assert_eq!(app.detail_selected(), 0, "the tree's cursor did not move");

        // Past the end it parks on the last page, as the inspector does.
        app.scroll_reader(1000);
        let rows = draw(&app);
        assert!(rows[10].contains("line 40"), "{}", rows.join("\n"));
    }

    // --- Rendered markdown in the reader (FZ-59) ---

    const MARKDOWN_FIXTURE: &str = "# Plan\n\n- ship *it* now\n- run `cargo test`\n\n\
                                    See [the docs](https://x.io).";

    /// Draw the reader into a 60 by 12 buffer.
    fn reader_buffer(app: &crate::app::App) -> ratatui::buffer::Buffer {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut term = Terminal::new(TestBackend::new(60, 12)).unwrap();
        term.draw(|f| render_reader(f, f.area(), app)).unwrap();
        term.backend().buffer().clone()
    }

    fn buffer_rows(buf: &ratatui::buffer::Buffer) -> Vec<String> {
        let area = buf.area;
        (0..area.height)
            .map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    /// The style of the cell where `needle` starts on screen.
    fn style_at(buf: &ratatui::buffer::Buffer, needle: &str) -> Style {
        for (y, row) in buffer_rows(buf).iter().enumerate() {
            let cells: Vec<char> = row.chars().collect();
            let want: Vec<char> = needle.chars().collect();
            if let Some(x) = cells.windows(want.len()).position(|w| w == want) {
                return buf[(x as u16, y as u16)].style();
            }
        }
        panic!("{needle} not on screen:\n{}", buffer_rows(buf).join("\n"));
    }

    /// Land `text` as the reader's body the way the event loop does: the
    /// read claimed at the width the reader last drew, rendered with it.
    fn land_rendered(app: &mut crate::app::App, text: &str) {
        reader_buffer(app);
        let read = app.claim_note_read().expect("a read");
        let width = read.render_width.expect("a markdown read renders");
        let lines = crate::ui::markdown::render_markdown(text, width);
        let rendered = crate::app::RenderedNote { width, lines };
        app.apply_note_rendered(&read.path, read.generation, rendered);
        app.apply_note_read(read, Ok(text.to_string()));
    }

    /// FZ-59: a markdown file draws rendered by default, through the
    /// `colors.rs` tokens: the heading in the title colour, bold, without
    /// its `#`; list bullets; emphasis italic; code in the code colour; a
    /// link's text in the link colour beside its url. The border says
    /// `rendered`; `p` shows the source unchanged and the border says `raw`.
    #[test]
    fn the_reader_renders_markdown_and_r_shows_the_source() {
        let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
        land_rendered(&mut app, MARKDOWN_FIXTURE);
        let buf = reader_buffer(&app);
        let text = buffer_rows(&buf).join("\n");
        for want in [
            "a.md · markdown · rendered",
            "Plan",
            "• ship it now",
            "• run cargo test",
            "See the docs (https://x.io).",
        ] {
            assert!(text.contains(want), "{want} missing:\n{text}");
        }
        assert!(!text.contains("# Plan"), "the heading's # is gone:\n{text}");
        let heading = style_at(&buf, "Plan");
        assert_eq!(heading.fg, Some(colors::MAUVE), "the title colour");
        assert!(heading.add_modifier.contains(Modifier::BOLD));
        assert!(
            style_at(&buf, "it now")
                .add_modifier
                .contains(Modifier::ITALIC)
        );
        assert_eq!(style_at(&buf, "cargo test").fg, Some(colors::PEACH));
        assert_eq!(style_at(&buf, "the docs").fg, Some(colors::SAPPHIRE));
        assert_eq!(style_at(&buf, "(https").fg, Some(colors::OVERLAY));

        app.toggle_reader_raw();
        let text = buffer_rows(&reader_buffer(&app)).join("\n");
        assert!(text.contains("a.md · markdown · raw"), "{text}");
        for line in MARKDOWN_FIXTURE.lines().filter(|l| !l.is_empty()) {
            assert!(text.contains(line), "source line {line:?} missing:\n{text}");
        }
    }

    /// FZ-59: the scroll bound counts rendered rows, not source lines: 40
    /// source lines that render as one paragraph of a few rows do not
    /// scroll, and raw they scroll by the source's 40 rows.
    #[test]
    fn the_rendered_reader_scrolls_by_rendered_rows() {
        let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
        let body: Vec<String> = (1..=40).map(|i| format!("line {i}")).collect();
        let body = body.join("\n");
        land_rendered(&mut app, &body);
        let rows = crate::ui::markdown::render_markdown(&body, app.reader_width.get()).len();
        reader_buffer(&app);
        let height = app.reader_view_height.get();
        let want = u16::try_from(rows).unwrap().saturating_sub(height);
        assert_eq!(app.reader_max_scroll.get(), want, "{rows} rendered rows");
        assert!(rows < 40, "one paragraph, {rows} rows");

        app.toggle_reader_raw();
        reader_buffer(&app);
        assert_eq!(app.reader_max_scroll.get(), 40 - height, "raw rows");
    }

    /// FZ-59: a text file is always raw: no render is asked for, its border
    /// names no mode, and `p` does not change what it shows.
    #[test]
    fn a_text_file_stays_raw_whatever_the_toggle() {
        let mut app = notes_view_app(&[("notez/notes.txt", Some("notez"))]);
        reader_buffer(&app);
        let read = app.claim_note_read().expect("a read");
        assert_eq!(read.render_width, None, "no render for text");
        app.apply_note_read(read, Ok("# not a heading".to_string()));
        let before = buffer_rows(&reader_buffer(&app)).join("\n");
        assert!(before.contains("notes.txt · text "), "{before}");
        assert!(!before.contains("rendered") && !before.contains("raw"));
        assert!(before.contains("# not a heading"), "{before}");
        app.toggle_reader_raw();
        assert_eq!(buffer_rows(&reader_buffer(&app)).join("\n"), before);
    }

    /// The card says every fact for a file: name, path, project, kind,
    /// where it lives, size, modified, created and the last commit, which
    /// is "untracked" when git knew nothing of the file.
    #[test]
    fn the_card_renders_every_fact_and_untracked() {
        let mut app = notes_view_app(&[(".notez/scratch.md", Some(".notez"))]);
        let read = app.claim_note_read().expect("a read");
        let at = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000);
        app.apply_note_facts(
            &read,
            crate::notes::NoteFacts {
                size: Some(812),
                modified: Some(at),
                created: Some(at),
            },
        );
        let draw = |app: &crate::app::App| {
            drawn_rows(50, 14, |f| render_inspector(f, f.area(), app)).join("\n")
        };
        // Before git answers the date says it is still being read.
        assert!(draw(&app).contains("last commit reading…"));
        app.apply_note_commit(&read, crate::notes::LastCommit::Untracked);
        let text = draw(&app);
        let project = app.repos[0].name.clone();
        let stamp = card_time(at);
        for want in [
            "[3] file",
            "name        scratch.md",
            "path        .notez/scratch.md",
            &format!("project     {project}"),
            "kind        markdown",
            "lives in    .notez/",
            "size        812 B",
            &format!("modified    {stamp}"),
            &format!("created     {stamp}"),
            "last commit untracked",
        ] {
            assert!(text.contains(want), "{want} missing:\n{text}");
        }

        app.apply_note_commit(
            &read,
            crate::notes::LastCommit::At("2026-10-29T10:50:00+01:00".to_string()),
        );
        let text = draw(&app);
        let committed = card_commit_time("2026-10-29T10:50:00+01:00");
        assert!(text.contains(&format!("last commit {committed}")), "{text}");

        // Folded, it keeps its one title row.
        app.inspector_folded = true;
        let rows = drawn_rows(50, 1, |f| render_inspector(f, f.area(), &app));
        assert!(rows[0].contains("[3] file"), "{}", rows[0]);
    }

    /// The `notes` layout as painted and as hit-tested agree: a click inside
    /// the preview, on a tree row and inside the card lands on each, each
    /// pane's border is drawn where its rectangle says, pane 2 runs the full
    /// height beside pane 1 and the card with the preview inside it under
    /// the pinned header (FZ-67), and the card is as wide as pane 1.
    #[test]
    fn notes_clicks_land_on_the_tree_the_reader_and_the_card() {
        let (w, h) = (140u16, 44u16);
        let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
        app.contributions_folded = true;
        let read = app.claim_note_read().expect("a read");
        app.apply_note_read(read, Ok("hello".to_string()));
        let area = Rect::new(0, 0, w, h);
        let rows = drawn_rows(w, h, |f| crate::ui::render(f, &app));
        let rects = crate::ui::panel_rects(area, &app);
        let pane2 = rects.detail.expect("pane 2");
        let reader = rects.reader.expect("reader");
        let card = rects.inspector.expect("card");

        assert_eq!(
            (card.x, card.width),
            (rects.repo_list.x, rects.repo_list.width)
        );
        assert_eq!(card.y, rects.repo_list.y + rects.repo_list.height);
        assert_eq!(pane2.y, rects.repo_list.y, "pane 2 starts at the top");
        assert_eq!(pane2.bottom(), card.bottom(), "and runs the full height");
        // Inside pane 2's border, under its title, readout and pinned
        // header, and in its right part, which leaves the tree the left.
        assert!(
            reader.x > pane2.x + 1 && reader.right() < pane2.right(),
            "the reader is inside pane 2's width: {reader:?} in {pane2:?}"
        );
        assert!(
            reader.y > pane2.y + 2 && reader.bottom() < pane2.bottom(),
            "the reader is under pane 2's header: {reader:?} in {pane2:?}"
        );
        assert!(
            reader.x > pane2.x + pane2.width / 4,
            "the tree keeps the left of pane 2"
        );

        let cell = |x: u16, y: u16| rows[y as usize].chars().nth(x as usize).unwrap();
        for (rect, panel, title) in [
            (pane2, FocusedPanel::Detail, "[2]"),
            (reader, FocusedPanel::Reader, "notes preview"),
            (reader, FocusedPanel::Reader, "a.md · markdown"),
            (card, FocusedPanel::Inspector, "[3] file"),
        ] {
            assert_eq!(cell(rect.x, rect.y), '╭', "{panel:?} corner");
            assert_eq!(cell(rect.right() - 1, rect.bottom() - 1), '╯', "{panel:?}");
            let top: String = rows[rect.y as usize]
                .chars()
                .skip(rect.x as usize)
                .take(rect.width as usize)
                .collect();
            assert!(top.contains(title), "{panel:?} title: {top}");
            assert_eq!(
                rects.at(rect.x, rect.y),
                Some(panel),
                "click corner {panel:?}"
            );
        }
        for (rect, panel) in [
            (reader, FocusedPanel::Reader),
            (card, FocusedPanel::Inspector),
        ] {
            let (mx, my) = (rect.x + rect.width / 2, rect.y + rect.height / 2);
            assert_eq!(rects.at(mx, my), Some(panel), "click mid {panel:?}");
        }
        // Pane 2's middle is the preview's now; the tree's file row is
        // pane 2's own.
        let (fy, fx) = rows
            .iter()
            .enumerate()
            .find_map(|(y, r)| {
                let x = r
                    .chars()
                    .take(reader.x as usize)
                    .collect::<String>()
                    .find("a.md")?;
                Some((y as u16, r[..x].chars().count() as u16))
            })
            .expect("the tree draws a.md left of the preview");
        assert!(pane2.contains(ratatui::layout::Position { x: fx, y: fy }));
        assert_eq!(
            rects.at(fx, fy),
            Some(FocusedPanel::Detail),
            "click a tree row"
        );
        assert!(
            rows.iter().any(|r| r.contains("hello")),
            "the body is drawn"
        );
        // The card's top edge grabs the divider; the same row in pane 2 does not.
        assert_eq!(
            rects.divider_at(card.x + 2, card.y),
            Some(crate::ui::Divider::Horizontal)
        );
        assert_eq!(rects.divider_at(reader.x + 2, card.y), None);
    }

    /// FZ-67: pane 2 keeps one outer border in the `notes` layout, its
    /// title and view tabs across the whole pane, and the preview is a box
    /// of its own inside it, titled `notes preview`, right of the tree.
    /// Fails while the reader is a sibling pane with its top on pane 2's.
    #[test]
    fn the_notes_preview_is_a_box_inside_pane_2() {
        let (w, h) = (140u16, 44u16);
        let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
        app.contributions_folded = true;
        let rows = drawn_rows(w, h, |f| crate::ui::render(f, &app));
        let rects = crate::ui::panel_rects(Rect::new(0, 0, w, h), &app);
        let pane2 = rects.detail.expect("pane 2");
        let reader = rects.reader.expect("preview");
        let span = |y: u16, from: u16, to: u16| -> String {
            rows[y as usize]
                .chars()
                .skip(from as usize)
                .take((to - from) as usize)
                .collect()
        };

        // One border on pane 2's top row: its two corners and nothing
        // between them that opens a second box.
        let top = span(pane2.y, pane2.x, pane2.right());
        assert!(top.starts_with('╭') && top.ends_with('╮'), "{top}");
        assert_eq!(top.matches('╭').count(), 1, "one outer border: {top}");
        assert!(top.contains("[2]"), "the pane's title: {top}");
        assert!(top.contains("NOTES"), "the view tabs: {top}");
        let bottom = span(pane2.bottom() - 1, pane2.x, pane2.right());
        assert!(bottom.starts_with('╰') && bottom.ends_with('╯'), "{bottom}");

        // The preview's own box, inside pane 2 and titled.
        let preview_top = span(reader.y, reader.x, reader.right());
        assert!(preview_top.starts_with('╭'), "{preview_top}");
        assert!(preview_top.contains("notes preview"), "{preview_top}");
        assert!(
            pane2.contains(ratatui::layout::Position {
                x: reader.x,
                y: reader.y
            }) && reader.right() < pane2.right()
                && reader.bottom() < pane2.bottom(),
            "{reader:?} in {pane2:?}"
        );

        // The tree on the left of the same body: its mode line on the
        // preview's top row, before the preview's corner.
        let left = span(reader.y, pane2.x, reader.x);
        assert!(left.contains("notes: [notez]"), "the tree: {left}");
    }

    /// FZ-67: the preview's border carries the file's facts on its right
    /// where it can hold them beside `notes preview`, and only `notes
    /// preview` where it cannot. Fails while the facts are the left title.
    #[test]
    fn the_preview_border_drops_the_facts_whole_when_narrow() {
        let mut app = notes_view_app(&[("notez/README.md", Some("notez"))]);
        let read = app.claim_note_read().expect("a read");
        let facts = crate::notes::NoteFacts {
            size: Some(4300),
            ..Default::default()
        };
        app.apply_note_facts(&read, facts);
        app.apply_note_read(read, Ok("body".to_string()));
        let facts = "README.md · markdown · 4.2 KiB · rendered";

        let wide = drawn_rows(70, 6, |f| render_reader(f, f.area(), &app));
        assert!(wide[0].starts_with("╭ notes preview "), "{}", wide[0]);
        assert!(wide[0].contains(&format!(" {facts} ╮")), "{}", wide[0]);

        let narrow = drawn_rows(50, 6, |f| render_reader(f, f.area(), &app));
        assert!(narrow[0].starts_with("╭ notes preview "), "{}", narrow[0]);
        assert!(
            !narrow[0].contains("README"),
            "dropped whole: {}",
            narrow[0]
        );
        assert!(!narrow[0].contains("·"), "no fragment: {}", narrow[0]);
    }

    /// FZ-67: in the `notes` layout `detail` is all of pane 2 and `reader`
    /// the preview inside it, so `at` answers the preview inside it and
    /// pane 2 on a tree row. A section's digest and an empty fleet draw no
    /// preview, and `panel_rects` says so.
    #[test]
    fn panel_rects_nest_the_preview_inside_pane_2() {
        let area = Rect::new(0, 0, 140, 44);
        let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
        let rects = crate::ui::panel_rects(area, &app);
        let pane2 = rects.detail.expect("pane 2");
        let reader = rects.reader.expect("preview");
        assert_eq!(pane2.intersection(reader), reader, "reader inside detail");
        assert!(pane2.width > reader.width && pane2.height > reader.height);
        let (cx, cy) = (reader.x + reader.width / 2, reader.y + reader.height / 2);
        assert_eq!(rects.at(cx, cy), Some(FocusedPanel::Reader));
        assert_eq!(
            rects.at(reader.right() - 1, reader.bottom() - 1),
            Some(FocusedPanel::Reader)
        );
        // A tree row: left of the preview, on its rows.
        assert_eq!(
            rects.at(pane2.x + 3, reader.y + 2),
            Some(FocusedPanel::Detail)
        );
        // Pane 2's header, above the preview, is pane 2's too.
        assert_eq!(rects.at(cx, pane2.y + 1), Some(FocusedPanel::Detail));

        // A local-only repo's section always draws its header.
        app.repos[0].location = crate::model::RepoLocation::LocalOnly;
        let section = crate::digest::Section::of(&app.repos[0]);
        app.land_on(crate::app::ListRow::Header(section));
        assert!(app.digest(chrono::Local::now()).is_some(), "on the digest");
        let rects = crate::ui::panel_rects(area, &app);
        assert_eq!(rects.reader, None, "the digest draws no preview");
        let rows = drawn_rows(140, 44, |f| crate::ui::render(f, &app));
        assert!(!rows.join("\n").contains("notes preview"));

        let mut empty = crate::app::App::new(Vec::new(), "t".to_string(), 140, 44);
        empty.view = crate::app::View::Detail;
        empty.history_view = HistoryView::Notes;
        assert_eq!(crate::ui::panel_rects(area, &empty).reader, None);
        let rows = drawn_rows(140, 44, |f| crate::ui::render(f, &empty));
        assert!(!rows.join("\n").contains("notes preview"));
    }

    /// FZ-67: a click on the tree's second file selects it with the tree
    /// inside pane 2's body: the hit map is built from the tree's rect, so
    /// the row under the pointer is the file drawn there.
    #[test]
    fn a_click_on_the_second_file_selects_it_inside_pane_2() {
        let (w, h) = (140u16, 44u16);
        let mut app =
            notes_view_app(&[("notez/a.md", Some("notez")), ("notez/b.md", Some("notez"))]);
        app.contributions_folded = true;
        let rows = drawn_rows(w, h, |f| crate::ui::render(f, &app));
        let reader = crate::ui::panel_rects(Rect::new(0, 0, w, h), &app)
            .reader
            .expect("preview");
        let (y, x) = rows
            .iter()
            .enumerate()
            .find_map(|(y, r)| {
                let left: String = r.chars().take(reader.x as usize).collect();
                let x = left.find("b.md")?;
                Some((y as u16, left[..x].chars().count() as u16))
            })
            .expect("b.md drawn in the tree");
        assert_eq!(app.detail_selected(), 0);
        let body = app.detail_hits.borrow().body;
        assert!(
            body.right() < reader.x,
            "the hit map is the tree's, left of the preview: {body:?}, {reader:?}"
        );
        let click = app.detail_hits.borrow().at(x, y).expect("a hit on b.md");
        app.click_detail(&click);
        assert_eq!(app.detail_selected(), 1, "the second file is selected");
        assert_eq!(
            app.notes_highlighted_file().map(|f| f.rel.clone()),
            Some("notez/b.md".into())
        );
    }

    /// FZ-67: on a pane too short to pin the header over the tree and the
    /// preview, the header scrolls with the tree in the tree's column, laid
    /// out at the tree's width: its rows do not wrap, so the mode line sits
    /// right under them, and the preview keeps its box beside it. Fails
    /// while the fallback keeps the header laid out for the whole pane.
    #[test]
    fn a_short_notes_pane_scrolls_the_header_with_the_tree() {
        let (w, h) = (140u16, 18u16);
        let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
        app.contributions_folded = true;
        let pane2 = crate::ui::panel_rects(Rect::new(0, 0, w, h), &app)
            .detail
            .expect("pane 2");
        let (header, body) = notes_pane_body(pane2, &app).expect("a tree");
        assert_eq!(header, None, "too short to pin the header");
        let (tree, _) = split_notes_pane(body);

        let rows = drawn_rows(w, h, |f| crate::ui::render(f, &app));
        let reader = crate::ui::panel_rects(Rect::new(0, 0, w, h), &app)
            .reader
            .expect("preview");
        let cell = |x: u16, y: u16| rows[y as usize].chars().nth(x as usize).unwrap();
        assert_eq!(cell(reader.x, reader.y), '╭', "the preview's corner");
        let left = |y: u16| -> String {
            rows[y as usize]
                .chars()
                .skip(tree.x as usize)
                .take((reader.x - tree.x) as usize)
                .collect()
        };

        // The header's first row, the repo's state, opens the tree column.
        let repo = app.selected_repo().expect("a repo");
        let state = repo.long_status();
        assert!(left(tree.y).contains(&state), "{}", left(tree.y));
        // Laid out at the tree's width, one row per line: the mode line is
        // the row after the last header line.
        let header_rows = header_lines(&app, repo, tree.width as usize).len() as u16;
        let mode = left(tree.y + header_rows);
        assert!(mode.contains("notes: [notez]"), "{}", rows.join("\n"));

        let body = app.detail_hits.borrow().body;
        assert!(body.right() < reader.x, "{body:?}, {reader:?}");
    }

    /// The tree is wide enough for its mode line whole, `(m)` included, in
    /// both modes on a whole rendered frame: at 140 columns the 30-column
    /// floor left 27 text columns and drew `(m` (FZ-62 review F3).
    #[test]
    fn the_tree_holds_its_mode_line_whole_on_a_140_column_frame() {
        for w in [120u16, 140, 150] {
            let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
            for want in [
                "notes: [notez] markdown  (m)",
                "notes: notez [markdown]  (m)",
            ] {
                let rows = drawn_rows(w, 44, |f| crate::ui::render(f, &app));
                assert!(
                    rows.iter().any(|r| r.contains(want)),
                    "{w} columns: {want} cut:\n{}",
                    rows.join("\n")
                );
                app.toggle_notes_markdown();
            }
        }
    }

    /// Off the `notes` view the usual layout holds: no reader, the
    /// inspector across the full width.
    #[test]
    fn the_notes_layout_is_the_notes_views_alone() {
        let area = Rect::new(0, 0, 140, 44);
        let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
        assert!(crate::ui::panel_rects(area, &app).reader.is_some());
        app.history_view = HistoryView::Info;
        let rects = crate::ui::panel_rects(area, &app);
        assert_eq!(rects.reader, None);
        assert_eq!(rects.inspector.map(|r| r.width), Some(140));
    }
}
