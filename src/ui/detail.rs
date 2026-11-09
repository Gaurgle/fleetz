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

/// Split pane 2's inner area of the `notes` layout, its full height, into
/// the tree's column and the column right of it (FZ-62, FZ-68), whose
/// first column is the divider (`notes_pane_body`). Shared by `render` and
/// `ui::panel_rects`, as the other splits are, so a click lands in the pane
/// drawn under it.
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
    // The `notes` layout's preview, right of pane 2's divider (FZ-68).
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
                let shown = app.ci_shown(repo);
                let ci = badges::RowCi {
                    status: &shown.status,
                    side_run: app.ci_side_run(repo),
                };
                ListItem::new(Line::from(repo_row_spans_ci(
                    repo,
                    ci,
                    on_cursor,
                    col_width,
                    dim_pending,
                    app.is_not_in_last_poll(repo),
                    app.search_match_field(repo).and_then(|f| f.mark()),
                )))
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
    let mut title = crate::ui::panel_title(1, "repos", focused);
    // The time window on this border, between the title and the count
    // (FZ-95, Andreas 2026-11-09).
    let count_w = format!(" {total} ").chars().count();
    let budget = (area.width as usize).saturating_sub(2 + span_width(&title.spans) + count_w + 2);
    title.spans.extend(list_title_window(app, budget));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::horizontal(1))
        .title_top(title)
        .title_top(
            Line::from(Span::styled(
                format!(" {total} "),
                Style::default().fg(colors::OVERLAY),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Filter strip + divider, then the repo list, then the fleet's summary
    // box at the foot (FZ-95).
    let summary = summary_box_lines(app, inner.width as usize);
    let (inner, summary_area) = split_summary_box(inner, summary.len());
    if let Some(box_area) = summary_area {
        let mut lines = vec![crate::ui::divider_line(box_area.width as usize)];
        lines.extend(summary);
        frame.render_widget(Paragraph::new(lines), box_area);
        // The footer, drawn after, leaves the summary and the message out.
        app.fleet_drawn.set(true);
    }
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

/// The rows the repo list keeps above pane 1's summary box before the box
/// gives way: a list that cannot show a handful of repos is worse than no
/// box, and the cursor's scroll margin needs the rows.
const MIN_LIST_ROWS_BESIDE_SUMMARY: u16 = 8;

/// Pane 1's inner area split into the filter strip and list above, and the
/// summary box at the foot: a divider row and `lines` rows. `None` for the
/// box where the list would be left too short; the footer then carries the
/// fleet's state. The list's clicks resolve against where it is drawn
/// (`ListHits`), so this split needs no copy in `panel_rects`.
fn split_summary_box(inner: Rect, lines: usize) -> (Rect, Option<Rect>) {
    let box_h = u16::try_from(lines).unwrap_or(u16::MAX).saturating_add(1);
    // The filter strip and its divider sit above the list.
    if inner.height < 2 + MIN_LIST_ROWS_BESIDE_SUMMARY + box_h {
        return (inner, None);
    }
    let v = Layout::vertical([Constraint::Min(1), Constraint::Length(box_h)]).split(inner);
    (v[0], Some(v[1]))
}

/// The time window on pane 1's top border (FZ-95), `· ◷ since 1 week [t] `,
/// in the short form (`· ◷ 1w [t] `) where `budget` columns cannot hold
/// it, and nothing where they cannot hold that either. Before a load names
/// the window, `· ◷ [t] `. Pane 2's sections never read this width.
fn list_title_window(app: &App, budget: usize) -> Vec<Span<'static>> {
    let key = Style::default()
        .fg(colors::TEXT)
        .add_modifier(Modifier::BOLD);
    let label = Style::default()
        .fg(colors::TEAL)
        .add_modifier(Modifier::BOLD);
    [window_label(app), window_short_label(&app.header)]
        .into_iter()
        .map(|text| {
            vec![
                Span::styled("· ", Style::default().fg(colors::SURFACE)),
                Span::styled(
                    match text.is_empty() {
                        true => WINDOW_MARK.to_string(),
                        false => format!("{WINDOW_MARK} {text}"),
                    },
                    label,
                ),
                Span::raw(" "),
                Span::styled("[t]", key),
                Span::raw(" "),
            ]
        })
        .find(|spans| span_width(spans) <= budget)
        .unwrap_or_default()
}

/// The most rows the summary takes in words before it switches to glyphs
/// and counts (Andreas, 2026-11-06: "when wide screen: write it out ...
/// but when narrow: 2!, 2PRs").
const SUMMARY_WORD_ROWS: usize = 2;

/// Pane 1's summary box (FZ-95), under its divider: the active group
/// (`[work]`) and the fleet's counters, wrapped at the pane's width between
/// counters, in words (`3↓ · 1! · 17 synced · 2 not cloned · 8 PRs`) while
/// they fit `SUMMARY_WORD_ROWS` rows and as glyphs and counts
/// (`3↓ 1! 17✓ 2- 8 PRs`) where they do not. Then the activity row: a
/// manual refresh's spinner and the last action's message, blank when
/// there is none. The activity row is always there, so a message moves
/// nothing.
fn summary_box_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let p = crate::ui::fleet_pieces(app, chrono::Local::now());
    let group: Vec<Vec<Span<'static>>> = p.group.into_iter().map(|g| vec![g]).collect();
    let mut words: Vec<Span<'static>> = Vec::new();
    for g in group.iter().flatten() {
        words.push(g.clone());
        words.push(Span::raw(" · "));
    }
    words.extend(p.summary);
    let mut lines = wrap_counters(words, width);
    if lines.len() > SUMMARY_WORD_ROWS {
        let mut glyphs = group;
        glyphs.extend(glyph_counters(&app.summary()));
        lines = wrap_groups(glyphs, Span::raw(" "), width);
    }
    if lines.is_empty() {
        lines.push(Line::from(""));
    }
    let mut activity: Vec<Span<'static>> = Vec::new();
    if let Some(spinner) = p.refreshing {
        activity.extend(
            spinner
                .into_iter()
                .map(|s| Span::styled(s.content.trim().to_string(), s.style)),
        );
        activity.push(Span::raw("  "));
    }
    if let Some(message) = p.message {
        activity.push(message);
    }
    lines.push(Line::from(fit_spans(activity, width)));
    lines
}

/// The summary's counters wrapped at `width`, breaking only at the ` · `
/// between two counters, so a counter is never split.
fn wrap_counters(spans: Vec<Span<'static>>, width: usize) -> Vec<Line<'static>> {
    let dot = Span::styled(" · ", Style::default().fg(colors::SURFACE));
    wrap_groups(counter_groups(spans), dot, width)
}

/// The summary's spans split into its counters at the ` · ` between them.
fn counter_groups(spans: Vec<Span<'static>>) -> Vec<Vec<Span<'static>>> {
    let mut groups: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    for span in spans {
        if span.content.trim() == "·" {
            groups.push(Vec::new());
        } else if let Some(group) = groups.last_mut() {
            group.push(span);
        }
    }
    groups.retain(|g| !g.is_empty());
    groups
}

/// `groups` laid out at `width`, `sep` between two on a row, a new row
/// where the next does not fit; a group wider than a row is cut with `…`.
fn wrap_groups(
    groups: Vec<Vec<Span<'static>>>,
    sep: Span<'static>,
    width: usize,
) -> Vec<Line<'static>> {
    let sep_w = sep.content.chars().count();
    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
    for group in groups {
        match lines.last_mut() {
            Some(line) if span_width(line) + sep_w + span_width(&group) <= width => {
                line.push(sep.clone());
                line.extend(group);
            }
            _ => lines.push(fit_spans(group, width)),
        }
    }
    lines.into_iter().map(Line::from).collect()
}

/// The summary's narrow form (Andreas, 2026-11-06): every counter a count
/// and a glyph, the count first as the list's counters write it (`3↓ 1!`),
/// synced a green `✓`, not cloned a grey `-`, the PRs kept in words. From
/// the one table the words form reads (`footer::summary_counters`).
fn glyph_counters(s: &crate::model::Summary) -> Vec<Vec<Span<'static>>> {
    crate::ui::footer::summary_counters(s)
        .into_iter()
        .map(|c| vec![Span::styled(c.glyphs, Style::default().fg(c.color))])
        .collect()
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

/// Right-aligned readout of which commit the pane's cursor is on, on the
/// pinned header's view title row (FZ-93; its own row under the panel title
/// before): `[3/10 · a1b2c3d]` in grey.
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
    // Whole or not at all: a cut readout names a hash that is not the one.
    if text.chars().count() > area.width as usize {
        return;
    }
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

/// Widest a repo row is drawn: the repo list's column cap less its two
/// borders and two padding columns. The slot digest (FZ-45) draws its rows'
/// shared columns at this width, as pane 1 does at its default size.
pub const REPO_ROW_MAX_W: usize = REPO_LIST_MAX_COLS as usize - 4;

/// A repo row's spans, `col_width` wide: marker, kind glyph, name, dot fill
/// and the badge group flush right. The one grammar a repo row has: the repo
/// list draws it, and so do the slot digest's lines (FZ-45), so every
/// indicator keeps its place and its colour in both. Drawn with the fleet
/// poll's lamp; the screens draw `repo_row_spans_ci` (FZ-83), so this one
/// is kept for the tests.
#[cfg(test)]
pub fn repo_row_spans(
    repo: &RepoStatus,
    is_selected: bool,
    col_width: usize,
    dim_pending: bool,
    not_in_last_poll: bool,
    match_mark: Option<&'static str>,
) -> Vec<Span<'static>> {
    let ci = badges::RowCi {
        status: &repo.ci_status,
        side_run: false,
    };
    repo_row_spans_ci(
        repo,
        ci,
        is_selected,
        col_width,
        dim_pending,
        not_in_last_poll,
        match_mark,
    )
}

/// `repo_row_spans` with the CI part as `ci` says (FZ-83): the repo list
/// passes `App::ci_shown` and `App::ci_side_run`.
pub fn repo_row_spans_ci(
    repo: &RepoStatus,
    ci: badges::RowCi,
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
    let (badge_spans, badge_w) = badges::badge_spans_ci(repo, ci, dim_pending, not_in_last_poll);
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
    // FZ-68: in the `notes` layout a divider splits pane 2 under its top
    // border, the tree's column left of it and the preview right of it.
    let notes_split = match app.notes_layout() {
        true => notes_pane_body(pane, app),
        false => None,
    };
    // The top border is one segment in `notes` too, so it and its titles
    // light while either side has focus; `render_notes_frame` gives each
    // side its own outline.
    let top_lit = focused || (notes_split.is_some() && app.focused_panel == FocusedPanel::Reader);
    let panel = match top_lit {
        true => pane2_block(colors::LAVENDER),
        false => panel,
    };
    // The number span comes from `panel_title` so its colors have one source;
    // a copy here once drew the unfocused number darker than every other pane.
    let number = crate::ui::panel_title(2, "", top_lit).spans.swap_remove(0);
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
    let budget =
        (area.width.saturating_sub(2) as usize).saturating_sub(span_width(&left_title.spans) + 2);
    let switcher = view_switcher(app.history_view, top_lit, budget);

    let inner = panel.inner(area);
    frame.render_widget(
        panel
            .title_top(left_title.left_aligned())
            .title_top(switcher),
        area,
    );
    if let Some(notes) = notes_split {
        render_notes_frame(frame, pane, &notes, app);
    }
    let inner = notes_split.map_or(inner, |notes| notes.column);

    // No readout row of its own: the counter rides the view's title row
    // once the header is pinned (below), so the header starts a row higher.
    let area = inner;
    let width = area.width as usize;

    let mut lines: Vec<Line> = header_lines(app, repo, width);
    let (area, width) = match notes_split {
        Some(notes) => {
            // Pinned, or on a short pane scrolling with the tree; laid out
            // at the column's width either way.
            if let Some(header) = notes.header {
                frame.render_widget(Paragraph::new(lines.split_off(0)), header);
                render_notes_heads(frame, pane, &notes, app);
            }
            if let Some(foot) = notes.foot {
                render_notes_foot(frame, foot, app);
            }
            (notes.body, notes.body.width as usize)
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

    // The long notes whole, where the sectioned header could only cut them
    // on its banner (FZ-95).
    if view == HistoryView::Info && header_sections(app, width).is_some() {
        lines.extend(long_note_lines(&long_notes(app, repo)));
    }

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
        // One row, as the tree's are (`notes_rows`): on a pane too short to
        // pin it, a wrapped mode line put the highlight a row below the
        // follow's count (FZ-65).
        lines.push(Line::from(clip_spans(notes_mode_line(app).spans, width)));
        pinned_rows = lines.len();
        let base = lines.len();
        let (rows, tops, selected) = notes_rows(app, repo, width);
        if let Some(row) = selected {
            selected_block = Some((base + row, base + row));
        }
        block_tops.extend(tops.into_iter().map(|t| base + t));
        lines.extend(rows);
    }

    // The `worktrees` view (FZ-77): one cursor stop per worktree, the main
    // checkout first.
    if view == HistoryView::Worktrees {
        let base = lines.len();
        let (rows, selected) = worktrees_view_rows(app, repo, width);
        if let Some(row) = selected {
            selected_block = Some((base + row, base + row));
            block_tops.extend(base..base + app.worktrees_listed().len());
        }
        lines.extend(rows);
    }

    // The `changes` view (FZ-60): which base the second section uses,
    // pinned with the header, then both sections, one cursor stop per file.
    if view == HistoryView::Changes {
        lines.push(changes_mode_line(app));
        pinned_rows = lines.len();
        let base = lines.len();
        let (rows, tops, selected) = changes_rows(app, repo, width);
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
            let header_area = header_area.expect("split");
            frame.render_widget(Paragraph::new(lines), header_area);
            // The sections' subtitles on the pane's top padding, over the
            // block they name (FZ-95). Only while pinned: over a scrolled
            // body they would name whatever rows had moved under them.
            if let Some(sections) = header_sections(app, header_area.width as usize)
                && header_area.y > pane.y + 1
            {
                frame.render_widget(
                    Paragraph::new(subtitle_row(sections)),
                    Rect::new(header_area.x, header_area.y - 1, header_area.width, 1),
                );
            }
            // The counter flush right on the view's title row, the first of
            // the headliner's two (his look, 2026-11-05). Only while that row
            // is pinned: a counter scrolled away with the body would vanish
            // exactly when the list has been scrolled past.
            // Right of the title only, so a narrow pane never covers it.
            if let Some(title_y) = (header_rows >= 2)
                .then(|| header_area.y + header_rows as u16 - 2)
                .filter(|y| *y < header_area.bottom())
            {
                let title_w = view_name(app.history_view).chars().count() as u16 + 1;
                render_selected_commit_readout(
                    frame,
                    Rect::new(
                        header_area.x + title_w.min(header_area.width),
                        title_y,
                        header_area.width.saturating_sub(title_w),
                        1,
                    ),
                    app,
                );
            }
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

/// Pane 2 of the `notes` layout, split by its divider (FZ-68).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotesPane {
    /// The tree's column, inside the frame, its full inner height: the
    /// header, the tree and the file's facts divider at its foot.
    pub column: Rect,
    /// The rows the header pins in that column, `None` when the pane is too
    /// short to spare them.
    pub header: Option<Rect>,
    /// The tree, under the header.
    pub body: Rect,
    /// The divider's screen column, from under the top border to the bottom
    /// one.
    pub divider: u16,
    /// Right of the divider on the header's rows above the rule: blank but
    /// for `notes preview` on the last of them, level with `NOTES`; the
    /// window sits on pane 1's border (FZ-95). `None` when the header is
    /// not pinned.
    pub facts: Option<Rect>,
    /// The tree column's last row: a divider carrying the highlighted
    /// file's facts (FZ-93). `None` when the tree has no row to spare.
    pub foot: Option<Rect>,
    /// The row of the `NOTES` rule, which runs pane 2's inner width and
    /// crosses the divider. `None` when the header is not pinned.
    pub rule: Option<u16>,
    /// The preview's text: one column in from the divider, on the tree's
    /// rows under the rule, or the column's full inner height when the
    /// header is not pinned.
    pub reader: Rect,
}

/// Pane 2 of the `notes` layout inside its frame (FZ-67, FZ-68): the tree's
/// column left of the divider, holding the pinned header, the tree and the
/// file's facts on a divider at its foot (FZ-93); right of it the fleet's
/// state beside the header and the preview beside the tree, the `NOTES` rule between them across the whole pane
/// (FZ-68 round 2). `None` when pane 2 shows no tree
/// (a section's digest, or no repo). Shared by `render_detail_pane` and
/// `ui::panel_rects`, so the preview is hit-tested where it is drawn.
pub fn notes_pane_body(pane: Rect, app: &App) -> Option<NotesPane> {
    let repo = app.selected_repo()?;
    let (column, right) = split_notes_pane(pane2_block(colors::SURFACE).inner(pane));
    // A column of padding either side of the divider, as the frame keeps
    // one inside its border.
    let text = Rect {
        x: right.x.saturating_add(2).min(right.right()),
        width: right.width.saturating_sub(2),
        ..right
    };
    let below = column;
    let header_rows =
        u16::try_from(header_lines(app, repo, below.width as usize).len()).unwrap_or(u16::MAX);
    let (header, body) = if below.height < header_rows.saturating_add(MIN_BODY_ROWS) {
        (None, below)
    } else {
        let split =
            Layout::vertical([Constraint::Length(header_rows), Constraint::Min(1)]).split(below);
        (Some(split[0]), split[1])
    };
    // The file's facts divider takes the tree's last row while the tree
    // keeps its minimum; the tree's clicks and wheel follow `body`.
    let (body, foot) = if body.height > MIN_BODY_ROWS {
        let split = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(body);
        (split[0], Some(split[1]))
    } else {
        (body, None)
    };
    // The header ends on the headliner's two rows, `NOTES` and its rule:
    // the facts take the rows above the rule, the preview the rows below.
    let (facts, rule, reader) = match header {
        Some(h) if h.height >= 2 => (
            Some(Rect {
                y: h.y,
                height: h.height - 1,
                ..text
            }),
            Some(h.bottom() - 1),
            Rect {
                y: h.bottom(),
                height: text.bottom().saturating_sub(h.bottom()),
                ..text
            },
        ),
        _ => (None, None, text),
    };
    Some(NotesPane {
        column,
        header,
        body,
        divider: right.x,
        facts,
        foot,
        rule,
        reader,
    })
}

/// Pane 2's frame in the `notes` layout (FZ-68), drawn over the one
/// `render_detail_pane` has rendered with its whole top border: the
/// divider from under that border to `┴`, and each side's outline below
/// the top border in its own focus, the tree's left and the preview's
/// right. The top border keeps the pane's colour, lit for either side.
fn render_notes_frame(frame: &mut Frame, pane: Rect, notes: &NotesPane, app: &App) {
    let lit = |on: bool| Style::default().fg(notes_lit(on));
    let (top, bottom) = (pane.y, pane.bottom().saturating_sub(1));
    let x = notes.divider;
    let below_top = top.saturating_add(1);
    let buf = frame.buffer_mut();
    let tree = lit(app.focused_panel == FocusedPanel::Detail);
    let preview = lit(app.focused_panel == FocusedPanel::Reader);
    let right_edge = pane.right().saturating_sub(1);
    for (rect, style) in [
        (Rect::new(pane.x, below_top, 1, pane.height), tree),
        (Rect::new(pane.x, bottom, x.saturating_sub(pane.x), 1), tree),
        (Rect::new(right_edge, below_top, 1, pane.height), preview),
        (
            Rect::new(x, bottom, pane.right().saturating_sub(x), 1),
            preview,
        ),
    ] {
        buf.set_style(rect.intersection(pane), style);
    }
    let divider = lit(notes_divider_lit(app));
    for y in below_top..=bottom {
        let symbol = match y == bottom {
            true => "┴",
            false => "│",
        };
        // Off the buffer on a pane squeezed to a column or two.
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_symbol(symbol).set_style(divider);
        }
    }
}

/// The `notes` frame's border colour, lit or not.
fn notes_lit(on: bool) -> ratatui::style::Color {
    match on {
        true => colors::LAVENDER,
        false => colors::SURFACE,
    }
}

/// Whether the `notes` divider is lit: while either side has focus.
fn notes_divider_lit(app: &App) -> bool {
    matches!(
        app.focused_panel,
        FocusedPanel::Detail | FocusedPanel::Reader
    )
}

/// Above the preview in the `notes` layout (FZ-68 round 2), drawn after the
/// pinned header: the file's facts beside the repo header, `notes preview`
/// level with `NOTES` in the view's colour, and the `NOTES` rule across
/// pane 2's whole inner width, crossing the divider with `┼`. Nothing when
/// the header is not pinned.
fn render_notes_heads(frame: &mut Frame, pane: Rect, notes: &NotesPane, app: &App) {
    let (Some(facts), Some(rule)) = (notes.facts, notes.rule) else {
        return;
    };
    let inner = pane2_block(colors::SURFACE).inner(pane);
    // The headliner's own rule at the pane's width, over the column-wide one
    // the header drew, so the fade runs the whole pane.
    if let Some(line) = headliner(app.history_view, inner.width as usize).pop() {
        frame.render_widget(
            Paragraph::new(line),
            Rect::new(inner.x, rule, inner.width, 1),
        );
    }
    if let Some(cell) = frame.buffer_mut().cell_mut((notes.divider, rule)) {
        cell.set_symbol("┼")
            .set_style(Style::default().fg(notes_lit(notes_divider_lit(app))));
    }
    if facts.height == 0 {
        return;
    }
    // Only `notes preview` here: the window sits on pane 1's border (FZ-95).
    let label_row = facts.bottom() - 1;
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            READER_LABEL,
            Style::default()
                .fg(colors::history_view_color(HistoryView::Notes))
                .add_modifier(Modifier::BOLD),
        ))),
        Rect {
            y: label_row,
            height: 1,
            ..facts
        },
    );
}

/// The tree's foot in the `notes` layout (FZ-93): a static divider with
/// the highlighted file's facts (`notes_foot_spans`). Not a pane: no focus,
/// no clicks.
fn render_notes_foot(frame: &mut Frame, foot: Rect, app: &App) {
    let spans = notes_foot_spans(app, foot.width as usize);
    frame.render_widget(Paragraph::new(Line::from(spans)), foot);
}

/// Pane 2's frame, every view's: rounded, a column of padding either side
/// and a row above the header.
fn pane2_block(border_color: ratatui::style::Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::new(1, 1, 1, 0))
}

/// The header's CI word (FZ-83): `CI ◉ passed` while the default branch's
/// name is not known, `CI ◉ main passed` once it is, and the culprit after a
/// state that is not green, `CI ◉ main failed: Vercel`.
fn ci_header_text(sym: &str, label: &str, shown: &crate::app::CiShown) -> String {
    let mut text = match &shown.branch {
        Some(branch) => format!("CI {sym} {branch} {label}"),
        None => format!("CI {sym} {label}"),
    };
    if let Some(culprit) = &shown.culprit {
        text.push_str(": ");
        text.push_str(culprit);
    }
    text
}

/// Columns of the divider between two sections of pane 2's header.
const SECTION_DIVIDER: &str = " │ ";

/// The narrowest the local section of pane 2's header may be before the
/// header gives way to its plain two-sided rows. The remote section, the
/// other half, is at most a column narrower.
const MIN_LOCAL_SECTION: usize = 20;

/// `spans` cut to `width` columns with `…`, or as they are when they fit.
fn fit_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    if span_width(&spans) <= width {
        spans
    } else {
        crate::ui::markdown::clip_spans(spans, width)
    }
}

/// `spans` padded with blanks to `width` columns.
fn pad_spans(mut spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let w = span_width(&spans);
    if w < width {
        spans.push(Span::raw(" ".repeat(width - w)));
    }
    spans
}

/// `spans` cut to `width` and pushed flush right in it.
fn right_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let spans = fit_spans(spans, width);
    let mut out = vec![Span::raw(
        " ".repeat(width.saturating_sub(span_width(&spans))),
    )];
    out.extend(spans);
    out
}

/// The time window's mark on pane 1's border.
const WINDOW_MARK: &str = "◷";

/// The window's label as pane 1's border draws it: the header's own words,
/// empty before a load names it.
fn window_label(app: &App) -> String {
    app.header.clone()
}

/// The window's short form for a narrow border (FZ-95): `since 4 weeks` as
/// `4w`, `since 2 hours` as `2h`; a slot window by its slot (`Work`);
/// anything else cut to `SHORT_WINDOW_MAX` columns with `…`. A coverage
/// note after the label is dropped with the rest.
fn window_short_label(header: &str) -> String {
    let words: Vec<&str> = header.split_whitespace().collect();
    if let ["since", n, unit, ..] = words.as_slice()
        && n.parse::<u32>().is_ok()
    {
        let short = match unit.trim_end_matches('s') {
            "hour" => Some("h"),
            "day" => Some("d"),
            "week" => Some("w"),
            "month" => Some("mo"),
            "year" => Some("y"),
            _ => None,
        };
        if let Some(short) = short {
            return format!("{n}{short}");
        }
    }
    if let Some(slot) = words
        .iter()
        .find(|w| matches!(**w, "Work" | "Evening" | "Night"))
    {
        return (*slot).to_string();
    }
    let spans = fit_spans(vec![Span::raw(header.to_string())], SHORT_WINDOW_MAX);
    spans.iter().map(|s| s.content.as_ref()).collect()
}

/// The widest a short window label is drawn.
const SHORT_WINDOW_MAX: usize = 8;

/// Pane 2's header sections at one width (FZ-95).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sections {
    local: usize,
    remote: usize,
}

/// The widths of pane 2's header sections at `width`, from the pane's
/// width alone, never from what a repo puts in them or from the window's
/// label on pane 1's border, so the divider stands in the same column for
/// every repo and every window (FZ-95). The divider splits the row in the
/// middle. `None` where the local half would fall under its floor; the
/// header then draws its plain two-sided rows.
fn section_widths(width: usize) -> Option<Sections> {
    let both = width.saturating_sub(SECTION_DIVIDER.chars().count());
    let (local, remote) = (both.div_ceil(2), both / 2);
    (local >= MIN_LOCAL_SECTION).then_some(Sections { local, remote })
}

/// The sections pane 2's header is drawn in for this frame, or `None` for
/// its plain two-sided rows: the `notes` layout, whose header sits left of
/// its divider, and a pane too narrow for them.
fn header_sections(app: &App, width: usize) -> Option<Sections> {
    (!app.notes_layout())
        .then(|| section_widths(width))
        .flatten()
}

fn section_divider() -> Span<'static> {
    Span::styled(SECTION_DIVIDER, Style::default().fg(colors::SURFACE))
}

/// Pane 2's header in fixed sections (FZ-95): the repo's local facts
/// leaning left, its remote ones right-aligned against the pane's edge.
/// Every row draws the divider.
fn sectioned_rows(
    rows: Vec<(Vec<Span<'static>>, Vec<Span<'static>>)>,
    sections: Sections,
) -> Vec<Line<'static>> {
    rows.into_iter()
        .map(|(left, right)| {
            let mut line = pad_spans(fit_spans(left, sections.local), sections.local);
            line.push(section_divider());
            line.extend(right_spans(right, sections.remote));
            Line::from(line)
        })
        .collect()
}

/// The row above pane 2's sectioned header, on the pane's top padding: a
/// small subtitle over each section, `local` and `remote` (Andreas,
/// 2026-11-06), with the divider through it.
fn subtitle_row(sections: Sections) -> Line<'static> {
    let style = Style::default().fg(colors::OVERLAY);
    let mut line = pad_spans(
        vec![Span::styled(HEADER_SUBTITLES[0], style)],
        sections.local,
    );
    line.push(section_divider());
    line.extend(right_spans(
        vec![Span::styled(HEADER_SUBTITLES[1], style)],
        sections.remote,
    ));
    Line::from(line)
}

/// The local and remote sections' subtitles.
const HEADER_SUBTITLES: [&str; 2] = ["local", "remote"];

/// The long notes a repo can carry (FZ-95): a failed fetch, git's reason
/// for it, the earlier-refresh note. Too long for a section, so the
/// sectioned header draws them on one banner row under its block
/// (`banner_line`) and `info` spells them out whole (`long_note_lines`).
fn long_notes(app: &App, repo: &RepoStatus) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(text) = crate::ui::fetch_failure_line(&repo.fetch, chrono::Local::now()) {
        notes.push(text);
        if let Some(detail) = repo.fetch.failure().and_then(|f| f.detail.as_deref()) {
            notes.push(detail.to_string());
        }
    }
    if app.is_not_in_last_poll(repo) {
        notes.push(EARLIER_REFRESH_NOTE.to_string());
    }
    notes
}

/// The long notes on one orange row under the header's block (Andreas,
/// 2026-11-06: "a banner of sorts to put longer information like this"),
/// joined by ` · ` and cut with `…` at the pane's edge. No divider crosses
/// it. `None` when there are none.
fn banner_line(notes: &[String], width: usize) -> Option<Line<'static>> {
    if notes.is_empty() {
        return None;
    }
    let text = format!("  {}", notes.join(" · "));
    Some(Line::from(fit_spans(
        vec![Span::styled(text, Style::default().fg(colors::PEACH))],
        width,
    )))
}

/// The long notes whole, each on its own row, at the top of `info`'s body
/// while the header draws them cut on its banner: the full text stays on
/// screen somewhere.
fn long_note_lines(notes: &[String]) -> Vec<Line<'static>> {
    if notes.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<Line<'static>> = notes
        .iter()
        .map(|note| {
            Line::from(Span::styled(
                note.clone(),
                Style::default().fg(colors::PEACH),
            ))
        })
        .collect();
    lines.push(Line::from(""));
    lines
}

/// A remote as the header names it (FZ-95): `owner/name` for GitHub, the
/// host before it for any other, never userinfo. Anything that is not a
/// host and exactly `owner/name` (a local path, a GitLab subgroup, an ssh
/// host alias) keeps its redacted URL (review 1, N2). Display only:
/// matching and `owner_display` read the stored URL.
fn remote_display(url: &str) -> String {
    let shown = crate::git::redact_userinfo(url);
    let Some(remote) = crate::model::RemoteRef::parse(&shown) else {
        return shown;
    };
    let host = remote.host.to_ascii_lowercase();
    // `RemoteRef::parse` reads the first two path segments and ignores the
    // rest, so a deeper path ends in something else; a host with no dot is
    // an ssh alias, not a place.
    let tail = shown.trim().trim_end_matches('/');
    let tail = tail.strip_suffix(".git").unwrap_or(tail);
    let pair = format!("{}/{}", remote.owner, remote.name);
    // What is left before `/owner/name` must be the host itself, with at
    // most a scheme, a user and a port around it.
    let authority = tail
        .strip_suffix(pair.as_str())
        .and_then(|before| before.strip_suffix(['/', ':']))
        .map(|before| before.rsplit_once("://").map_or(before, |(_, a)| a))
        .map(|a| a.rsplit_once('@').map_or(a, |(_, h)| h))
        .map(|a| a.split_once(':').map_or(a, |(h, _)| h));
    if host.is_empty() || !host.contains('.') || authority != Some(remote.host.as_str()) {
        return shown;
    }
    if host == "github.com" || host == "www.github.com" {
        format!("{}/{}", remote.owner, remote.name)
    } else {
        format!("{}/{}/{}", remote.host, remote.owner, remote.name)
    }
}

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
    //   * main · 1 stash          2 open PRs #13 · CI ◉ main passed
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
    let shown = app.ci_shown(repo);
    if shown.status != CiStatus::Unknown {
        let (ci_sym, ci_color, ci_label) =
            badges::ci_display(&shown.status, app.watch_mode && !app.blink_on);
        let ci_color = if not_in_last_poll {
            badges::EARLIER_REFRESH
        } else {
            ci_color
        };
        if !row3_right.is_empty() {
            row3_right.push(Span::styled(" · ", Style::default().fg(colors::SURFACE)));
        }
        row3_right.push(Span::styled(
            ci_header_text(ci_sym, ci_label, &shown),
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
            let shown = remote_display(url);
            let shown_w = shown.chars().count();
            (
                vec![Span::styled(shown, Style::default().fg(colors::OVERLAY))],
                shown_w,
            )
        }
        None => name_spans(repo),
    };
    let row3_right_w = span_width(&row3_right);

    let note = not_in_last_poll.then(|| {
        vec![Span::styled(
            format!("  {EARLIER_REFRESH_NOTE}"),
            Style::default().fg(badges::EARLIER_REFRESH),
        )]
    });
    // Outside the `notes` layout, whose header sits left of its divider,
    // fixed sections (FZ-95): local and remote.
    if let Some(sections) = header_sections(app, width) {
        let mut rows = vec![(row1_left, owner_group), (row2_left, remote_loc)];
        if !row3_left.is_empty() || !row3_right.is_empty() {
            rows.push((row3_left, row3_right));
        }
        lines.extend(sectioned_rows(rows, sections));
        // The fetch failure, its git detail and the earlier-refresh note do
        // not fit a section: one banner row under the block, cut at the
        // pane's edge, no divider on it; `info` has them whole (Andreas,
        // 2026-11-06, replacing "whole, under the block").
        lines.extend(banner_line(&long_notes(app, repo), width));
        lines.push(Line::from(""));
    } else {
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
        lines.extend(note.map(Line::from));
        lines.push(Line::from(""));
    }
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

/// A note file's name in the tree, and in the facts beside the header.
const NOTE_NAME_COLOR: ratatui::style::Color = colors::TEXT;

/// A note's kind glyph in the tree, and its kind word in the facts.
const NOTE_GLYPH_COLOR: ratatui::style::Color = colors::OVERLAY;

/// The lit mark of a two-state switch in `notes`: the mode line's active
/// mode, and the facts' `rendered` or `raw`.
fn notes_mode_mark_style() -> Style {
    Style::default()
        .fg(colors::history_view_color(HistoryView::Notes))
        .add_modifier(Modifier::BOLD)
}

/// The `notes` view's first line: both modes as a two-state switch, the
/// active one bracketed and lit as the tab strip lights its view, the other
/// dim, and the key that flips them (FZ-57): `notes: [notez] markdown  (m)`.
fn notes_mode_line(app: &App) -> Line<'static> {
    notes_mode_line_for(app.notes_all_markdown)
}

/// `notes_mode_line` for either mode, so the tree's width floor can measure
/// the line it has to hold (`notes_tree_floor`).
fn notes_mode_line_for(all_markdown: bool) -> Line<'static> {
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
            true => Span::styled(format!("[{mode}]"), notes_mode_mark_style()),
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
    // Every row is cut to one rendered row (`clip_spans`): the tops are
    // counted in lines, and a long name the paragraph wrapped put the
    // highlight that many rows below where the follow kept it (FZ-65).
    for row in rows {
        match row {
            crate::notes::Row::Section(label) => {
                if !lines.is_empty() {
                    lines.push(Line::from(""));
                }
                lines.push(Line::from(clip_spans(vec![section_label(&label)], width)));
            }
            crate::notes::Row::Dir { depth, name } => {
                let dir = Span::styled(
                    format!("  {}{name}/", "  ".repeat(depth)),
                    Style::default().fg(colors::OVERLAY),
                );
                lines.push(Line::from(clip_spans(vec![dir], width)));
            }
            crate::notes::Row::File { depth, name, file } => {
                let is_sel = tops.len() == cursor;
                if is_sel {
                    selected = Some(lines.len());
                }
                tops.push(lines.len());
                // The kind's glyph before the name (FZ-62), dim so the name
                // stays what the eye reads.
                let entry = notes.files.get(file);
                let glyph = entry.map_or("", |f| badges::note_kind_icon(f.kind()));
                let mut spans = vec![
                    Span::styled(
                        if is_sel { "▸ " } else { "  " },
                        Style::default().fg(colors::SAPPHIRE),
                    ),
                    Span::raw("  ".repeat(depth)),
                    Span::styled(format!("{glyph} "), Style::default().fg(NOTE_GLYPH_COLOR)),
                    Span::styled(name, Style::default().fg(NOTE_NAME_COLOR)),
                ];
                // Every board is named `TODO.md` under `todos` (FZ-63); its
                // place, dim, tells them apart.
                if let Some(f) = entry.filter(|f| f.kind() == crate::notes::NoteKind::Todo) {
                    spans.push(Span::styled(
                        format!("  {}", f.place()),
                        Style::default().fg(colors::OVERLAY),
                    ));
                }
                spans = clip_spans(spans, width);
                if is_sel {
                    spans = wash(pad_to(spans, width));
                }
                lines.push(Line::from(spans));
            }
        }
    }
    (lines, tops, selected)
}

/// The highlighted note's body as last read, when it is that file's.
fn held_note_body(app: &App) -> Option<&crate::app::NoteBody> {
    let path = app.notes_highlighted()?;
    app.note_body.as_ref().filter(|b| b.path == path)
}

/// A file time as the facts write it, the local date: the facts share a
/// row, and the sketch the owner accepted drew dates (FZ-68 round 2).
fn card_time(at: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(at)
        .format("%Y-%m-%d")
        .to_string()
}

/// git's `%cI` date as the facts write it, the way `card_time` writes a
/// file time; git's own spelling when it does not parse.
fn card_commit_time(iso: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(iso) {
        Ok(at) => at
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string(),
        Err(_) => iso.to_string(),
    }
}

/// The highlighted file's facts in the `notes` layout, for the divider at
/// the tree's foot (FZ-93; beside the repo header from FZ-68 round 2, the
/// `[3] file` card's before it), in display order, each with the rank it
/// gives way at when the foot is narrow, higher first: where it lives (6),
/// the markdown mode (5), the kind (4), the last commit (3), modified (2),
/// the size (1); the name (0) never. The name and kind wear the tree's
/// colours for the file and its glyph, the mode the mode line's mark,
/// sizes and dates are dim. Size and times wait for the background read
/// (`spawn_note_read`), the last commit for git (`AppEvent::NoteCommit`).
fn notes_fact_pieces(app: &App) -> Vec<(u8, Vec<Span<'static>>)> {
    let dim = |text: String| Span::styled(text, Style::default().fg(colors::OVERLAY));
    let Some(file) = app.notes_highlighted_file() else {
        return vec![(0, vec![dim("no file selected".to_string())])];
    };
    let name = file
        .rel
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let held = held_note_body(app);
    let facts = held.and_then(|b| b.facts.as_ref());

    let mut pieces = vec![
        (
            0,
            vec![Span::styled(name, Style::default().fg(NOTE_NAME_COLOR))],
        ),
        (
            4,
            vec![Span::styled(
                file.kind().label().to_string(),
                Style::default().fg(NOTE_GLYPH_COLOR),
            )],
        ),
    ];
    // A board counts what is open, unchecked and half done (FZ-63).
    if let Some(board) = held_board(app) {
        pieces.push((4, vec![dim(format!("{} open", board.open()))]));
    }
    if let Some(size) = facts.and_then(|f| f.size) {
        pieces.push((1, vec![dim(crate::notes::size_label(size))]));
    }
    // A board is drawn as one whatever `p` says, so it names no mode.
    if file.kind().is_markdown() && file.kind() != crate::notes::NoteKind::Todo {
        let mode = match app.reader_raw {
            true => "raw",
            false => "rendered",
        };
        pieces.push((5, vec![Span::styled(mode, notes_mode_mark_style())]));
    }
    let lives = match &file.notez {
        Some(dir) => format!("{}/", dir.display()),
        None if file.is_personal() => "the personal store".to_string(),
        None => "the repo".to_string(),
    };
    pieces.push((
        6,
        vec![Span::styled(lives, Style::default().fg(colors::TEXT))],
    ));
    match facts {
        None => pieces.push((1, vec![dim("reading…".to_string())])),
        Some(facts) => {
            if let Some(at) = facts.modified {
                pieces.push((2, vec![dim(format!("modified {}", card_time(at)))]));
            }
            use crate::notes::LastCommit;
            let commit = match held.and_then(|b| b.last_commit.as_ref()) {
                Some(LastCommit::At(iso)) => card_commit_time(iso),
                Some(LastCommit::Untracked) => "untracked".to_string(),
                None => "reading…".to_string(),
            };
            pieces.push((3, vec![dim(format!("last commit {commit}"))]));
        }
    }
    pieces
}

/// The tree's foot as one row of `width`: a rule carrying the file's facts
/// (`notes_fact_pieces`), drawn as the compact view's status line is. The
/// facts that do not fit give way in their rank's order, so the row never
/// wraps and the name always stays; a name wider than the row is cut.
fn notes_foot_spans(app: &App, width: usize) -> Vec<Span<'static>> {
    let rule = |n: usize| Span::styled("─".repeat(n), Style::default().fg(colors::SURFACE));
    let pieces = notes_fact_pieces(app);
    let line = |max_rank: u8| {
        let mut spans = vec![rule(1), Span::raw(" ")];
        for (i, (_, piece)) in pieces.iter().filter(|(r, _)| *r <= max_rank).enumerate() {
            if i > 0 {
                spans.push(Span::styled(" · ", Style::default().fg(colors::SURFACE)));
            }
            spans.extend(piece.iter().cloned());
        }
        spans.push(Span::raw(" "));
        spans
    };
    let spans = (0..=6u8)
        .rev()
        .map(line)
        .find(|spans| span_width(spans) <= width)
        .unwrap_or_else(|| fit_spans(line(0), width));
    let rest = width.saturating_sub(span_width(&spans));
    let mut spans = spans;
    spans.push(rule(rest));
    spans
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

/// The preview's headliner word, right of the divider level with `NOTES`
/// (FZ-68 round 2): the preview is a page of the `notes` view.
const READER_LABEL: &str = "notes preview";

/// Where `render_notes_heads` draws the reader's `notes preview` title in
/// pane 2, for the copy flash (FZ-81). `None` when the header is not pinned
/// and the title is not drawn.
pub fn reader_label_rect(pane: Rect, app: &App) -> Option<Rect> {
    let facts = notes_pane_body(pane, app)?.facts?;
    (facts.height > 0).then(|| Rect {
        y: facts.bottom() - 1,
        width: (READER_LABEL.len() as u16).min(facts.width),
        height: 1,
        ..facts
    })
}

/// The highlighted file's todoz board, parsed from the body already read,
/// here on the UI thread: measured cheap for a capped file (FZ-63). `None`
/// for any other kind, and until the read lands.
fn held_board(app: &App) -> Option<crate::ui::todo::Board> {
    app.notes_highlighted_file()
        .filter(|f| f.kind() == crate::notes::NoteKind::Todo)?;
    let text = held_note_body(app)?.text.as_ref()?.as_ref().ok()?;
    Some(crate::ui::todo::Board::parse(text))
}

/// The held body's markdown render, when the reader draws one: a markdown
/// file, `p` not on raw, and a render landed (FZ-59). Until one lands the
/// reader shows the source. A `TODO.md` draws as a board instead
/// (`held_board`, FZ-63).
fn reader_render(app: &App) -> Option<&crate::app::RenderedNote> {
    let markdown = app
        .notes_highlighted_file()
        .is_some_and(|f| f.kind().is_markdown() && f.kind() != crate::notes::NoteKind::Todo);
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
fn render_reader(frame: &mut Frame, inner: Rect, app: &App) {
    // No frame of its own: pane 2's border and divider hold the column and
    // carry its titles (`render_notes_frame`, FZ-68).
    // One `H`/`L` page and the render width, which only this pass can
    // measure.
    let board = held_board(app);
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
        Some(_) if board.is_some() => board
            .as_ref()
            .map_or_else(Vec::new, |b| b.lines(inner.width as usize)),
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
        crate::ui::render_folded(
            frame,
            area,
            crate::ui::INSPECTOR_NUM,
            "inspector",
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
    if app.worktrees_wanted() {
        render_worktree_inspector(frame, area, app);
        return;
    }
    if app.changes_wanted() {
        render_changes_inspector(frame, area, app);
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

/// The inspector while pane 2 shows `worktrees` (FZ-77): the highlighted
/// worktree's facts, then its newest commits as the read landed them.
fn render_worktree_inspector(frame: &mut Frame, area: Rect, app: &App) {
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
    let lines: Vec<Line> = worktree_inspector_lines(app)
        .iter()
        .flat_map(|l| wrap_line(l, inner.width as usize))
        .collect();
    let max_scroll = (lines.len() as u16).saturating_sub(inner.height);
    app.inspector_max_scroll.set(max_scroll);
    let scroll = app.inspector_scroll.min(max_scroll);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
}

/// What the `worktrees` inspector draws for the highlighted worktree: its
/// path, branch, HEAD, dirty count, ahead and behind against its upstream,
/// the age of its last commit, then its newest commits in the branches
/// preview's row form. Ahead and behind come from the main checkout's
/// `branches`, as `worktree_table`'s column does.
fn worktree_inspector_lines(app: &App) -> Vec<Line<'static>> {
    let (Some(repo), Some(wt)) = (app.selected_repo(), app.worktree_highlighted()) else {
        return vec![Line::from(Span::styled(
            "no worktree selected",
            Style::default().fg(colors::OVERLAY),
        ))];
    };
    let label =
        |text: &str| Span::styled(format!("{text:<8}"), Style::default().fg(colors::OVERLAY));
    let plain = |text: String, color| Span::styled(text, Style::default().fg(color));
    let log = app.worktree_log();
    let mut lines = vec![Line::from(Span::styled(
        wt.path.clone(),
        Style::default()
            .fg(colors::SAPPHIRE)
            .add_modifier(Modifier::BOLD),
    ))];

    let branch = match wt.detached {
        true => plain(format!("detached @ {}", wt.head), colors::PEACH),
        false => plain(wt.branch.clone(), colors::TEXT),
    };
    lines.push(Line::from(vec![label("branch"), branch]));

    let subject = log
        .and_then(|commits| commits.first())
        .filter(|c| {
            !wt.head.is_empty() && (c.hash.starts_with(&wt.head) || wt.head.starts_with(&c.hash))
        })
        .map(|c| c.message.clone());
    let mut head = vec![label("HEAD"), plain(wt.head.clone(), colors::HASH_CLR)];
    if let Some(subject) = subject {
        head.push(plain(format!(" {subject}"), colors::TEXT));
    }
    lines.push(Line::from(head));

    let dirty = match wt.dirty {
        None => plain("unknown".to_string(), colors::OVERLAY),
        Some(0) => plain("clean".to_string(), colors::OVERLAY),
        Some(n) => Span::styled(
            format!("{} ({n} changed)", crate::model::counters_text(0, 0, n)),
            Style::default().fg(badges::counter_color(0, 0, n)),
        ),
    };
    lines.push(Line::from(vec![label("dirty"), dirty]));

    let tracked = (!wt.detached)
        .then(|| repo.branches.iter().find(|b| b.name == wt.branch))
        .flatten();
    let sync = match tracked {
        None => plain("·".to_string(), colors::OVERLAY),
        Some(b) => match &b.upstream {
            None => plain("no upstream".to_string(), colors::OVERLAY),
            Some(up) if b.behind == 0 && b.ahead == 0 => {
                plain(format!("level with {up}"), colors::OVERLAY)
            }
            Some(up) => Span::styled(
                format!(
                    "{} against {up}",
                    crate::model::counters_text(b.behind, b.ahead, 0)
                ),
                Style::default().fg(badges::counter_color(b.behind, b.ahead, 0)),
            ),
        },
    };
    lines.push(Line::from(vec![label("sync"), sync]));

    let age = wt.head_at.map_or_else(|| "·".to_string(), format_age);
    lines.push(Line::from(vec![label("last"), plain(age, colors::OVERLAY)]));

    lines.push(Line::from(""));
    lines.push(Line::from(section_label("recent commits")));
    match log {
        None => lines.push(Line::from(plain("  reading…".to_string(), colors::OVERLAY))),
        Some([]) => lines.push(Line::from(plain(
            "  (no commits)".to_string(),
            colors::OVERLAY,
        ))),
        Some(commits) => {
            for c in commits {
                lines.push(Line::from(vec![
                    plain(format!("  {}", c.hash), colors::HASH_CLR),
                    Span::styled(
                        format!(" {} ", crate::ui::author_code(&c.author)),
                        Style::default().fg(colors::author_color(&c.author)),
                    ),
                    plain(c.message.clone(), colors::TEXT),
                    plain(format!("  {}", commit_age(c)), colors::OVERLAY),
                ]));
            }
        }
    }
    lines
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

/// The `changes` view's two bases, in the mode line's and the help row's
/// words (FZ-60).
const CHANGES_BASES: [&str; 2] = ["upstream", "default"];

/// The `changes` view's first line: both bases as a two-state switch, drawn
/// as the `notes` mode line draws its modes: `vs: [upstream] default  (u)`.
fn changes_mode_line(app: &App) -> Line<'static> {
    let on = colors::history_view_color(HistoryView::Changes);
    let active = usize::from(app.changes_base == crate::git::ChangesBase::DefaultBranch);
    let mut spans = vec![Span::styled("vs: ", Style::default().fg(colors::OVERLAY))];
    for (i, base) in CHANGES_BASES.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(match i == active {
            true => Span::styled(
                format!("[{base}]"),
                Style::default().fg(on).add_modifier(Modifier::BOLD),
            ),
            false => Span::styled(base.to_string(), Style::default().fg(colors::SUBTEXT)),
        });
    }
    spans.push(Span::styled("  (", Style::default().fg(colors::OVERLAY)));
    spans.push(Span::styled(
        "u",
        Style::default()
            .fg(colors::TEXT)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(")", Style::default().fg(colors::OVERLAY)));
    Line::from(spans)
}

/// A file's status columns: porcelain's staged column in green and its
/// unstaged one in the dirty counter's yellow, `??` dim; the other
/// section's one letter in the churn colors when it adds or deletes.
fn change_status_spans(status: &str) -> Vec<Span<'static>> {
    let styled = |text: String, color| Span::styled(text, Style::default().fg(color));
    if status == "??" {
        return vec![styled("??".to_string(), colors::OVERLAY)];
    }
    let mut chars = status.chars();
    let (x, y) = (chars.next().unwrap_or(' '), chars.next());
    match y {
        Some(y) => vec![
            styled(x.to_string(), colors::GREEN),
            styled(y.to_string(), colors::YELLOW),
        ],
        None => {
            let color = match x {
                'A' => colors::DIFF_ADD,
                'D' => colors::DIFF_DEL,
                _ => colors::TEXT,
            };
            vec![styled(format!("{x} "), color)]
        }
    }
}

/// The churn column of one changed file, in the widths its section uses.
fn churn_spans(churn: Option<(u32, u32)>, add_w: usize, del_w: usize) -> Vec<Span<'static>> {
    let (added, removed) = churn.unwrap_or((0, 0));
    let token = |n: u32, sign: char, w: usize| match n {
        0 => " ".repeat(w),
        n => format!("{:>w$}", format!("{sign}{n}")),
    };
    let mut spans = Vec::new();
    if add_w > 0 {
        spans.push(Span::styled(
            token(added, '+', add_w),
            Style::default().fg(colors::DIFF_ADD),
        ));
    }
    if del_w > 0 {
        if add_w > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(
            token(removed, '-', del_w),
            Style::default().fg(colors::DIFF_DEL),
        ));
    }
    spans
}

/// The `changes` view's lists for `repo` (FZ-60): its lines, the line each
/// file is on, in cursor order, and the line the cursor's file is on. A row
/// with nothing to list says why in one line.
fn changes_rows(
    app: &App,
    repo: &RepoStatus,
    width: usize,
) -> (Vec<Line<'static>>, Vec<usize>, Option<usize>) {
    use crate::git::Outgoing;
    let dim = |text: String| Line::from(Span::styled(text, Style::default().fg(colors::OVERLAY)));
    let say = |text: String| (vec![dim(text)], Vec::new(), None);
    if repo.path.is_none() {
        return say("not cloned: nothing to read".to_string());
    }
    let changes = match app.selected_changes().and_then(|(_, s)| s?.held.as_ref()) {
        None => return say("reading changes…".to_string()),
        Some(Err(e)) => return say(format!("could not read the repo: {e}")),
        Some(Ok(changes)) => changes,
    };
    if let Outgoing::Unpushed {
        upstream, ahead: 0, ..
    } = &changes.outgoing
        && changes.uncommitted.is_empty()
    {
        return say(format!("clean and pushed to {upstream}"));
    }
    let total = changes.uncommitted.len() + changes.outgoing.files().len();
    let cursor = app.detail_selected().min(total.saturating_sub(1));
    let mut lines = Vec::new();
    let mut tops = Vec::with_capacity(total);
    let mut selected = None;
    let mut list = |lines: &mut Vec<Line<'static>>, files: &[crate::git::ChangedFile]| {
        let add_w = files
            .iter()
            .map(|f| stat_part_width(f.churn.map_or(0, |c| c.0)))
            .max()
            .unwrap_or(0);
        let del_w = files
            .iter()
            .map(|f| stat_part_width(f.churn.map_or(0, |c| c.1)))
            .max()
            .unwrap_or(0);
        let tail = stat_tail_width(add_w, del_w);
        let path_w = width.saturating_sub(2 + 2 + 2 + tail + usize::from(tail > 0) * 2);
        for file in files {
            let is_sel = tops.len() == cursor;
            if is_sel {
                selected = Some(lines.len());
            }
            tops.push(lines.len());
            let path = crate::ui::truncate(&file.path, path_w);
            let pad = path_w.saturating_sub(path.chars().count());
            let mut spans = vec![Span::styled(
                if is_sel { "▸ " } else { "  " },
                Style::default().fg(colors::SAPPHIRE),
            )];
            spans.extend(change_status_spans(&file.status));
            spans.push(Span::raw("  "));
            spans.push(Span::styled(path, Style::default().fg(colors::TEXT)));
            if tail > 0 {
                spans.push(Span::raw(" ".repeat(pad + 2)));
                spans.extend(churn_spans(file.churn, add_w, del_w));
            }
            if is_sel {
                spans = wash(pad_to(spans, width));
            }
            lines.push(Line::from(spans));
        }
    };
    match changes.uncommitted.len() {
        0 => lines.push(dim("nothing uncommitted".to_string())),
        n => {
            lines.push(Line::from(section_label(&format!("uncommitted ({n})"))));
            list(&mut lines, &changes.uncommitted);
        }
    }
    lines.push(Line::from(""));
    match &changes.outgoing {
        Outgoing::Unpushed {
            upstream, ahead: 0, ..
        } => lines.push(dim(format!("nothing unpushed to {upstream}"))),
        Outgoing::Unpushed {
            upstream,
            ahead,
            files,
        } => {
            let heading = format!("unpushed ({ahead} ahead of {upstream})");
            lines.push(Line::from(section_label(&heading)));
            list(&mut lines, files);
        }
        Outgoing::VsDefault { base, files } => {
            let heading = format!("on branch vs {base} ({})", files.len());
            lines.push(Line::from(section_label(&heading)));
            list(&mut lines, files);
        }
        Outgoing::NoUpstream { branch: Some(b) } => {
            lines.push(dim(format!("no upstream: {b} tracks no remote branch")))
        }
        Outgoing::NoUpstream { branch: None } => {
            lines.push(dim("no upstream: HEAD is detached".to_string()))
        }
        Outgoing::NoDefault => lines.push(dim("no default branch to compare with".to_string())),
    }
    (lines, tops, selected)
}

/// One line of a changed file's diff, in the churn column's green and red;
/// hunk heads in the section labels' mauve, file headers dim. Tabs widened
/// so the terminal draws them.
fn diff_line(line: &str) -> Line<'static> {
    let color = if line.starts_with("+++") || line.starts_with("---") {
        colors::OVERLAY
    } else if line.starts_with('+') {
        colors::DIFF_ADD
    } else if line.starts_with('-') {
        colors::DIFF_DEL
    } else if line.starts_with("@@") {
        colors::MAUVE
    } else if line.starts_with(' ') || line.is_empty() {
        colors::TEXT
    } else {
        colors::OVERLAY
    };
    Line::from(Span::styled(
        line.replace('\t', "    "),
        Style::default().fg(color),
    ))
}

/// The inspector's lines in the `changes` view: the highlighted file's
/// path, then its diff (an untracked file's text plain), or why there is
/// none.
fn changes_inspector_lines(app: &App) -> Vec<Line<'static>> {
    let dim = |text: &str| {
        Line::from(Span::styled(
            text.to_string(),
            Style::default().fg(colors::OVERLAY),
        ))
    };
    let Some((_, file)) = app.changes_highlighted() else {
        return vec![dim("no file selected")];
    };
    let mut lines = vec![
        Line::from(Span::styled(
            file.path.clone(),
            Style::default()
                .fg(colors::TEXT)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    match app.highlighted_change_diff().and_then(|d| d.text.as_ref()) {
        None => lines.push(dim("reading…")),
        Some(Err(e)) => lines.push(Line::from(Span::styled(
            format!("could not read: {e}"),
            Style::default().fg(colors::RED),
        ))),
        Some(Ok(text)) if file.status == "??" => lines.extend(text.lines().map(|l| {
            Line::from(Span::styled(
                l.replace('\t', "    "),
                Style::default().fg(colors::TEXT),
            ))
        })),
        Some(Ok(text)) => lines.extend(text.lines().map(diff_line)),
    }
    lines
}

/// The inspector while pane 2 shows `changes` (FZ-60). Lines are clipped,
/// not wrapped, as a diff reads, so one line is one row and only the rows
/// on screen are handed to the paragraph; scrolled by the inspector's
/// offset, clamped to what this frame measured.
fn render_changes_inspector(frame: &mut Frame, area: Rect, app: &App) {
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
    let lines = changes_inspector_lines(app);
    let max_scroll = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_sub(inner.height);
    app.inspector_max_scroll.set(max_scroll);
    let scroll = app.inspector_scroll.min(max_scroll);
    let visible: Vec<Line> = lines
        .into_iter()
        .skip(scroll as usize)
        .take(inner.height as usize)
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
/// `budget` is the border width left over after the pane's own title, and
/// the strip takes the widest of three forms that fits it: every tab with
/// its full name; every tab cut to its first three letters (`INF · COM`),
/// same separators, colours and keys; and, when even that does not fit,
/// `[X/N]` alone, the active view's place in the cycle, its digits in the
/// view's colour and no key hint beside it.
fn view_switcher(active: HistoryView, focused: bool, budget: usize) -> Line<'static> {
    // " [/] " + every label + " · " between each + a trailing space, as
    // `tab_switcher` measures it.
    let strip_width = |labels: &[String]| -> usize {
        let cells: usize = labels.iter().map(|l| l.chars().count()).sum();
        5 + cells + 3 * (labels.len() - 1) + 1
    };
    let on = colors::history_view_color(active);
    let full: Vec<String> = HistoryView::ALL.iter().map(|v| view_name(*v)).collect();
    let short: Vec<String> = full.iter().map(|l| l.chars().take(3).collect()).collect();
    let labels = if strip_width(&full) <= budget {
        full
    } else if strip_width(&short) <= budget {
        short
    } else {
        let key = if focused {
            colors::TEXT
        } else {
            colors::OVERLAY
        };
        let bold = |c| Style::default().fg(c).add_modifier(Modifier::BOLD);
        let position = format!("{}/{}", active.index() + 1, HistoryView::COUNT);
        return Line::from(vec![
            Span::raw(" "),
            Span::styled("[", bold(key)),
            Span::styled(position, bold(on)),
            Span::styled("]", bold(key)),
            Span::raw(" "),
        ])
        .right_aligned();
    };
    tab_switcher(&labels, active.index(), on, focused, budget)
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
    let mut lines = vec![Line::from(section_label("worktrees"))];
    lines.extend(worktree_table(repo, &repo.worktrees, width));
    lines
}

/// The `worktrees` view's list (FZ-77): one row per worktree, the main
/// checkout first and marked `*` as the checked-out branch is, each row
/// `worktree_table`'s behind the cursor's caret column. Returns the lines
/// and the selected row; a row with nothing on disk says `not cloned`, and
/// a repo with no linked worktree says so under its main checkout.
fn worktrees_view_rows(
    app: &App,
    repo: &RepoStatus,
    width: usize,
) -> (Vec<Line<'static>>, Option<usize>) {
    let dim = |text: &str| {
        Line::from(Span::styled(
            format!("  {text}"),
            Style::default().fg(colors::OVERLAY),
        ))
    };
    let listed = app.worktrees_listed();
    if listed.is_empty() {
        let text = match repo.is_not_cloned() {
            true => "not cloned",
            false => "no worktrees",
        };
        return (vec![dim(text)], None);
    }
    let selected = app.detail_selected().min(listed.len() - 1);
    let head_color = colors::ref_color(crate::model::RefKind::Head);
    let mut lines = Vec::with_capacity(listed.len() + 1);
    for (i, row) in worktree_table(repo, &listed, width.saturating_sub(2))
        .into_iter()
        .enumerate()
    {
        let is_sel = i == selected;
        let mut spans = vec![Span::styled(
            if is_sel { "▸ " } else { "  " },
            Style::default().fg(colors::SAPPHIRE),
        )];
        let mut cells = row.spans;
        if i == 0 && !cells.is_empty() {
            cells[0] = Span::styled("* ", Style::default().fg(head_color));
        }
        spans.extend(cells);
        if is_sel {
            spans = wash(pad_to(spans, width));
        }
        lines.push(Line::from(spans));
    }
    if listed.len() == 1 {
        lines.push(dim("no worktrees besides the main one"));
    }
    (lines, Some(selected))
}

/// One row per entry of `worktrees`, the columns `worktree_rows` draws:
/// that section and the `worktrees` view (FZ-77) both lay their rows out
/// here, so the two cannot drift. Each row starts with two blank columns,
/// the indent the view's main checkout marks.
fn worktree_table(
    repo: &RepoStatus,
    worktrees: &[crate::model::Worktree],
    width: usize,
) -> Vec<Line<'static>> {
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
    let rows: Vec<Row> = worktrees
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

    let mut lines = Vec::with_capacity(rows.len());
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
mod tests;
