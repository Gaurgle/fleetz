use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::changes::{changes_mode_line, changes_rows};
use super::commits::{
    RECENT_COMMIT_COUNT, commit_hash_at, expanded_graph_lines, graph_list_lines, hash_style,
    opens_commit,
};
use super::header::{header_lines, header_sections, long_note_lines, long_notes, subtitle_row};
use super::layout::MIN_BODY_ROWS;
use super::notes::{
    notes_mode_line, notes_pane_body, notes_rows, render_notes_foot, render_notes_frame,
    render_notes_heads,
};
use super::worktrees::{worktree_rows, worktrees_view_rows};
use super::{
    bring_into_view, clip_spans, line_rows, pad_to, pane2_block, section_label, span_width,
    view_name, view_switcher, wash, wrapped_row_count,
};
use crate::app::{App, FocusedPanel, HistoryView};
use crate::ui::colors;
use crate::ui::format_age;

#[cfg(test)]
mod tests;

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

pub(super) fn render_detail_pane(frame: &mut Frame, area: Rect, app: &App) {
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
