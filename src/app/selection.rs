//! The selection: the repo cursor and its reorder across reloads, the detail
//! pane's per-view cursor and scroll, the hash the inspector reads, the
//! graph's row cursor and the history view cycle.
//!
//! Moved out of `app.rs` unchanged (FZ-124). The methods stay on `App` and
//! the fields they touch stay plain on it. Design:
//! `docs/app-split-design.md`, "Readability at the end" and D7.

use super::{App, FocusedPanel, HistoryView, scrolloff_for};
use crate::model::RepoStatus;

impl App {
    /// After the repo list is replaced by a reload, keep the selection in
    /// range and reset the detail/inspector panes so nothing points at a
    /// commit that no longer exists.
    pub fn clamp_after_reload(&mut self) {
        let len = self.visible_repos().len();
        if self.selected >= len {
            self.selected = len.saturating_sub(1);
        }
        self.set_detail_scroll(0);
        self.set_detail_selected(0);
        self.inspector_scroll = 0;
    }

    /// Stable-sort `repos` by `keys` (one per row, in row order), keeping the
    /// cursor on the repo it stood on (FZ-86). The caller supplies the keys,
    /// so the order has one source (`main.rs::order_rows_by_activity`).
    ///
    /// A search in progress is left alone: its ranked list breaks ties by row
    /// order, so moving rows under it would reshuffle what is being read. The
    /// next update after the search ends applies the order. False, with no
    /// redraw asked, when nothing moved.
    pub fn reorder_repos<K: Ord>(&mut self, keys: Vec<K>) -> bool {
        if self.has_text_query() || keys.len() != self.repos.len() {
            return false;
        }
        let mut order: Vec<usize> = (0..keys.len()).collect();
        order.sort_by(|&a, &b| keys[a].cmp(&keys[b]));
        if order.iter().enumerate().all(|(new, &old)| new == old) {
            return false;
        }
        // None on a section header, and there the cursor needs no help: the
        // sections are by location, which no re-sort changes, so each one
        // starts at the same visible position before and after.
        let held = self.selected_repo_index();

        let mut rows: Vec<Option<RepoStatus>> = std::mem::take(&mut self.repos)
            .into_iter()
            .map(Some)
            .collect();
        self.repos = order.iter().filter_map(|&old| rows[old].take()).collect();

        if let Some(held) = held {
            let moved_to = order.iter().position(|&old| old == held);
            if let Some(pos) = self
                .visible_repos()
                .iter()
                .position(|(i, _)| Some(*i) == moved_to)
            {
                self.selected = pos;
            }
        }
        self.needs_redraw = true;
        true
    }

    /// The currently selected repo, if any.
    /// None while the cursor stands on a section header (`on_header`).
    pub fn selected_repo(&self) -> Option<&RepoStatus> {
        if self.on_header().is_some() {
            return None;
        }
        let visible = self.visible_repos();
        visible.get(self.selected).map(|(_, r)| *r)
    }

    /// Index into self.repos for the currently selected visible repo.
    pub fn selected_repo_index(&self) -> Option<usize> {
        if self.on_header().is_some() {
            return None;
        }
        let visible = self.visible_repos();
        visible.get(self.selected).map(|(i, _)| *i)
    }

    pub fn move_up(&mut self) {
        if self.step_list(-1) {
            return;
        }
        if self.selected > 0 {
            self.selected -= 1;
            self.reset_detail_view();
        }
    }

    pub fn move_down(&mut self) {
        if self.step_list(1) {
            return;
        }
        let max = self.visible_repos().len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
            self.reset_detail_view();
        }
    }

    /// Reset the detail pane's scroll + commit selection + inspector scroll,
    /// so moving to a different repo always starts its detail from the top
    /// with the first commit selected rather than inheriting stale offsets.
    pub(super) fn reset_detail_view(&mut self) {
        self.set_detail_scroll(0);
        // Before the selection, which in the graph view reads it back.
        self.graph_cursor = 0;
        self.set_detail_selected(0);
        self.inspector_scroll = 0;
    }

    /// Rows the cursor can walk in the *active* history view.
    ///
    /// Not `commits.len()`: the cursor walks whichever list is on screen, and
    /// an index valid against the commit list is meaningless against the
    /// reflog -- or against the graph, which `info` and `commits` both draw.
    pub fn selected_commit_count(&self) -> usize {
        let Some(repo) = self.selected_repo() else {
            return 0;
        };
        // One row per commit: the graph is computed from parent hashes rather
        // than read out of `git log --graph`, so there are no connector rows
        // to skip past. See `crate::graph`.
        let graph_commits = || repo.graph.len();
        match self.history_view {
            // Info draws the newest few *graph* rows, not the newest few of
            // `commits`. The two are different `git log` runs: `commits` walks
            // HEAD, the graph walks the union of the local branches, so a
            // commit living only on a side branch sits in one and not the
            // other. Counting the wrong one put the cursor's ordinal and the
            // hash it resolved on different rows.
            HistoryView::Info => graph_commits().min(crate::ui::detail::RECENT_COMMIT_COUNT),
            HistoryView::Commits => graph_commits(),
            HistoryView::Reflog => repo.reflog.len(),
            HistoryView::Graph => graph_commits(),
            // Local branches then remotes, in the order the view draws them.
            HistoryView::Branches => repo.branches.len() + repo.remote_branches.len(),
            // The runs held for the repo and the tip's other contexts among
            // them (FZ-83), as the view draws them.
            HistoryView::Ci => self.ci_entries().len(),
            // The files the tree lists, in its current mode.
            HistoryView::Notes => self.notes_listed().len(),
            // The main checkout and the linked worktrees.
            HistoryView::Worktrees => self.worktrees_listed().len(),
            // Both sections' files, uncommitted first.
            HistoryView::Changes => self.changes_listed().len(),
        }
    }

    /// Move the inspector's commit cursor within the selected repo's commits.
    pub fn select_commit_up(&mut self) {
        if self.detail_selected() > 0 {
            self.set_detail_selected(self.detail_selected() - 1);
            self.inspector_scroll = 0;
        }
    }

    pub fn select_commit_down(&mut self) {
        let max = self.selected_commit_count().saturating_sub(1);
        if self.detail_selected() < max {
            self.set_detail_selected(self.detail_selected() + 1);
            self.inspector_scroll = 0;
        }
    }

    /// Detail-pane down-navigation: step the commit cursor through the recent
    /// commits, then -- once past the last one -- scroll the pane so the
    /// history/reflog/CI sections below the commits become reachable.
    pub fn detail_nav_down(&mut self) {
        // Spent on the re-entry when the selection is off screen: see
        // `reenter_selection`.
        if self.reenter_selection() {
            return;
        }
        if self.detail_selected() + 1 < self.selected_commit_count() {
            self.select_commit_down();
        } else {
            // Past the last row there is still content below it -- branches,
            // stashes -- so keep scrolling rather than stopping dead.
            self.scroll_detail(1);
        }
    }

    /// Detail-pane up-navigation: step the commit cursor, and let the
    /// renderer bring it into view.
    ///
    /// It used to unwind the content scroll first. Once the renderer began
    /// following the selection the two fought: this decremented the offset
    /// and the next frame restored it, so the pane never moved.
    pub fn detail_nav_up(&mut self) {
        // Spent on the re-entry when the selection is off screen: see
        // `reenter_selection`.
        if self.reenter_selection() {
            return;
        }
        if self.detail_selected() > 0 {
            self.select_commit_up();
        } else {
            // At the top of the list there is still the header above it.
            self.scroll_detail(-1);
        }
    }

    /// The commit the inspector is currently showing, if any.
    /// The hash the active history view has selected.
    ///
    /// Every view draws a list of its own, and they are lists of different
    /// things: `info` the newest graph rows, `commits` and `graph` all of
    /// them, `reflog` the HEAD movements. Which is why this returns a hash
    /// rather than an index -- an index into one means nothing in another.
    pub fn inspected_hash(&self) -> Option<&str> {
        // Clamped, not bounds-checked: the index survives a repo change and a
        // view switch, so it can outlive the list it was chosen against. A
        // stale one resolves to the last row rather than to nothing, which is
        // what keeps the readout drawn instead of vanishing.
        let i = self
            .detail_selected()
            .min(self.selected_commit_count().saturating_sub(1));
        self.entry_hash(i)
    }

    /// The hash entry `i` of the active history view names, the one
    /// `inspected_hash` returns when the cursor is on it.
    pub fn entry_hash(&self, i: usize) -> Option<&str> {
        let repo = self.selected_repo()?;
        let graph_nth = |n: usize| repo.graph.get(n).map(|g| g.hash.as_str());
        match self.history_view {
            HistoryView::Info | HistoryView::Commits | HistoryView::Graph => graph_nth(i),
            HistoryView::Reflog => repo.reflog.get(i).map(|e| e.hash.as_str()),
            // A branch resolves to its own tip. Locals come first, so an index
            // past them names a remote. An empty tip is no answer at all: a
            // repo with no commits has branches but no objects to name.
            HistoryView::Branches => {
                let n = repo.branches.len();
                match i < n {
                    true => repo.branches.get(i).map(|b| b.tip.as_str()),
                    false => repo.remote_branches.get(i - n).map(|r| r.tip.as_str()),
                }
                .filter(|h| !h.is_empty())
            }
            // A run is not a commit of the loaded history; the inspector
            // details the run itself (`ci_highlighted_run`).
            HistoryView::Ci => None,
            // A file is not a commit; the inspector reads the file itself.
            HistoryView::Notes => None,
            // A worktree names its HEAD, as a branch names its tip; the main
            // checkout first, as the view draws it. Borrowed from the rows
            // the hash came from, not from a list built here.
            HistoryView::Worktrees => match i {
                0 => repo.main_worktree().and_then(|_| {
                    repo.branches
                        .iter()
                        .find(|b| b.is_head)
                        .map(|b| b.tip.as_str())
                        .filter(|t| !t.is_empty())
                        // Detached: the 7-digit form the row's
                        // `detached @ <sha>` names (`main_worktree`).
                        .or(repo
                            .work_key
                            .as_ref()
                            .map(|k| k.head_sha.get(..7).unwrap_or(&k.head_sha)))
                }),
                _ => repo.worktrees.get(i - 1).map(|w| w.head.as_str()),
            }
            .filter(|h| !h.is_empty()),
            // Nor is a changed file; the inspector reads its diff.
            HistoryView::Changes => None,
        }
    }

    /// The commit the inspector should show: whatever the active view
    /// selected, resolved against the loaded commit list.
    ///
    /// Resolved by hash rather than by index, because the reflog and the
    /// graph are different lists of different lengths -- an index into one
    /// means nothing in another. Returns None when the selection names a
    /// commit outside the loaded window, which the reflog can easily do:
    /// it reaches back further than the commit list does.
    pub fn inspected_commit(&self) -> Option<&crate::model::Commit> {
        let repo = self.selected_repo()?;
        let hash = self.inspected_hash()?;
        repo.commits.iter().find(|c| c.hash == hash)
    }

    /// A left click the detail pane's hit map resolved: move the cursor to
    /// the clicked row the way `j`/`k` would. The graph's cursor is a row, so
    /// any row of it is a stop; the other views move only onto an entry.
    pub fn click_detail(&mut self, click: &crate::ui::detail::DetailClick) {
        if self.history_view == HistoryView::Graph {
            self.graph_cursor = click.line;
            self.keep_graph_cursor_in_view();
            self.sync_graph_selection();
        } else if let Some(entry) = click.entry {
            self.set_detail_selected(entry);
            self.inspector_scroll = 0;
        }
        self.needs_redraw = true;
    }

    /// Scroll the detail pane by `delta` lines (negative scrolls up), clamped
    /// to the content the last frame actually measured.
    ///
    /// This used to clamp to a fixed `DETAIL_SCROLL_CAP` of 300 and let the
    /// renderer clamp again for display. The offset therefore kept climbing
    /// long after the view stopped moving, and scrolling back needed one
    /// no-op keypress for every step of overshoot before anything moved --
    /// which read as the pane being stuck.
    pub fn scroll_detail(&mut self, delta: i32) {
        // The graph view scrolls by moving its row cursor and letting the view
        // follow, so every gesture that scrolls it also moves the highlight.
        if self.history_view == HistoryView::Graph {
            return self.move_graph_cursor(delta);
        }
        let max = self.drawn.detail_max_scroll.get() as i32;
        let current = self.detail_scroll() as i32;
        self.set_detail_scroll((current + delta).clamp(0, max) as u16);
    }

    /// Bring the selection back inside the pane, and report whether it moved.
    ///
    /// Called when a step is asked for, **not** while scrolling. Scrolling
    /// used to drag the selection along on every wheel notch, which meant the
    /// wheel silently reassigned which commit the inspector was reading out:
    /// the highlight crept down the pane on its own and the panel below it
    /// changed under you. Scrolling now leaves the selection with the content
    /// it is attached to, exactly as it leaves every other row, and it is the
    /// first `j`/`k` afterwards that re-enters -- from the edge you are
    /// looking at rather than from a row that is no longer on screen.
    ///
    /// That first step is spent on the re-entry and does not also advance, so
    /// the highlight appears where the eye is before it starts moving again.
    ///
    /// The selection lands on the nearest entry that clears the margin, and
    /// stays put when there is no such entry: `info` draws branches, stashes
    /// and worktrees below its commit list, and those rows must stay
    /// reachable by scrolling past the last commit.
    ///
    /// Deliberately does not set `follow_selection`: that would ask the next
    /// frame to scroll the entry into view, undoing the scroll that chose it.
    fn reenter_selection(&mut self) -> bool {
        let height = self.drawn.detail_view_height.get();
        if height == 0 {
            return false;
        }
        let scroll = self.detail_scroll();
        let pad = scrolloff_for(height);
        // The margin collapses at the ends of the content: there is nothing
        // beyond them to keep in reserve, and holding it there would make the
        // first and last entries unreachable.
        let first = scroll + if scroll > 0 { pad } else { 0 };
        let last = scroll + height
            - 1
            - if scroll < self.drawn.detail_max_scroll.get() {
                pad
            } else {
                0
            };

        let tops = self.drawn.detail_row_tops.borrow();
        let Some(&row) = tops.get(self.detail_selected()) else {
            return false;
        };
        let next = if row < first {
            tops.iter().position(|&t| t >= first)
        } else if row > last {
            tops.iter().rposition(|&t| t <= last)
        } else {
            None
        };
        drop(tops);
        match next {
            Some(i) if i != self.detail_selected() => {
                self.detail_selections[self.history_view.index()] = i;
                self.inspector_scroll = 0;
                true
            }
            _ => false,
        }
    }

    /// Move the graph view's row cursor by `delta` rows and let the view
    /// follow it, keeping `SCROLLOFF` rows of context past the cursor at
    /// whichever edge it is heading for.
    ///
    /// The highlight therefore walks down the pane until it is that far from
    /// the bottom, and from there stays put while the text runs under it --
    /// the way a pager or an editor moves. A commit-granular marker could not
    /// do this: blocks run past twenty rows, so it stood still through a whole
    /// commit and then jumped.
    pub fn move_graph_cursor(&mut self, delta: i32) {
        let last = self.graph_body_rows().saturating_sub(1) as i32;
        self.graph_cursor = (self.graph_cursor as i32 + delta).clamp(0, last) as u16;
        self.keep_graph_cursor_in_view();
        self.sync_graph_selection();
    }

    /// Rows in the detail pane's scrolling body, as the last frame measured
    /// it: the largest useful offset plus the height it leaves on screen.
    fn graph_body_rows(&self) -> u16 {
        self.drawn.detail_max_scroll.get() + self.drawn.detail_view_height.get()
    }

    /// Nudge the scroll offset so the cursor sits at least `SCROLLOFF`
    /// rows inside the pane, and otherwise leave it exactly where it is --
    /// that "otherwise" is what makes the highlight, not the text, move
    /// through the middle of the pane.
    fn keep_graph_cursor_in_view(&mut self) {
        let height = self.drawn.detail_view_height.get();
        if height == 0 {
            return;
        }
        // Never more than half the pane, or the two margins overlap and the
        // cursor is pinned to the middle of a short one.
        let pad = scrolloff_for(height);
        let cursor = self.graph_cursor;
        // cursor - scroll must land in [pad, height - 1 - pad].
        let lowest = cursor.saturating_sub(height - 1 - pad);
        let highest = cursor.saturating_sub(pad);
        let scroll = self
            .detail_scroll()
            .clamp(lowest, highest)
            .min(self.drawn.detail_max_scroll.get());
        self.set_detail_scroll(scroll);
    }

    /// Bring the cursor's row into view for a commit step, from the top.
    ///
    /// A commit step lands on a block's first row, and the minimal nudge
    /// `keep_graph_cursor_in_view` makes would leave that row in the *bottom*
    /// margin: four rows of the commit you just stepped to, and the rest of it
    /// below the fold. A row already comfortably on screen is left alone, so
    /// stepping through commits that share a screen does not jerk the view.
    fn show_graph_cursor_from_top(&mut self) {
        let height = self.drawn.detail_view_height.get();
        if height == 0 {
            return;
        }
        let pad = scrolloff_for(height);
        let scroll = self.detail_scroll();
        let cursor = self.graph_cursor;
        if cursor < scroll + pad || cursor > scroll + height - 1 - pad {
            let top = cursor
                .saturating_sub(pad)
                .min(self.drawn.detail_max_scroll.get());
            self.set_detail_scroll(top);
        }
    }

    /// The commit the row cursor is inside, which is what the hash emphasis
    /// and `inspected_hash` key off.
    ///
    /// Deliberately does not set `follow_selection`: that would ask the next
    /// frame to scroll the block into view, undoing the scroll that chose it.
    fn sync_graph_selection(&mut self) {
        let cursor = self.graph_cursor;
        let block = self
            .drawn
            .detail_row_tops
            .borrow()
            .iter()
            .rposition(|&top| top <= cursor);
        if let Some(i) = block {
            self.detail_selections[HistoryView::Graph.index()] = i;
        }
    }

    /// Scroll offset and selection for the *active* history view. Every
    /// caller wants the current one; the arrays exist so switching views does
    /// not lose where you were in the one you left.
    pub fn detail_scroll(&self) -> u16 {
        self.drawn.detail_scrolls[self.history_view.index()].get()
    }

    /// Takes `&self` because the renderer sets it too -- see the field.
    pub fn set_detail_scroll(&self, value: u16) {
        self.drawn.detail_scrolls[self.history_view.index()].set(value);
    }

    pub fn detail_selected(&self) -> usize {
        self.detail_selections[self.history_view.index()]
    }

    pub fn set_detail_selected(&mut self, value: usize) {
        self.detail_selections[self.history_view.index()] = value;
        // Every way the highlighted run can change passes here (a run step,
        // a click, a repo change), so the inspector's offsets cannot outlive
        // their run and come back when the cursor returns to it.
        self.sync_ci_inspector(std::time::Instant::now());
        // Stepping a commit in the graph view moves the row cursor to that
        // commit's own row, and the view is nudged around it here rather than
        // by `follow_selection`: bring-into-view puts the row hard against an
        // edge, which the very next scroll would undo by pulling the scrolloff
        // margin back.
        if self.history_view == HistoryView::Graph {
            let top = self.drawn.detail_row_tops.borrow().get(value).copied();
            if let Some(top) = top {
                self.graph_cursor = top;
                self.show_graph_cursor_from_top();
                return;
            }
        }
        // Any cursor move asks the pane to follow. Scrolling does not, which
        // is what keeps the two from fighting each other to a standstill.
        self.drawn.follow_selection.set(true);
    }

    /// Step to the next history view, wrapping, or the previous one.
    ///
    /// `[` and `]`, which is lazygit's own binding for the same gesture and
    /// leaves `1`-`4` to go on focusing panes.
    pub fn cycle_history_view(&mut self, back: bool) {
        let n = HistoryView::COUNT;
        let i = self.history_view.index();
        let next = if back { (i + n - 1) % n } else { (i + 1) % n };
        self.history_view = HistoryView::ALL[next];
        // The inspector may have just stopped being drawn; focus cannot stay
        // on a pane that is not on screen.
        if self.focused_panel == FocusedPanel::Inspector && !self.shows_inspector() {
            self.focused_panel = FocusedPanel::Detail;
        }
        // Likewise the reader, once pane 2 leaves `notes`.
        if self.focused_panel == FocusedPanel::Reader && !self.notes_layout() {
            self.focused_panel = FocusedPanel::Detail;
        }
        // Deliberately does not follow the selection. Each view keeps its own
        // scroll offset, so arriving restores where you left that view --
        // which is where its cursor is anyway, since the two were left in
        // step. Following on arrival meant the expanded graph, which anchors
        // its selection to the top of the pane, scrolled its own header off
        // the moment you switched to it: you asked for the tab and got a
        // commit, not a page.
        self.needs_redraw = true;
    }
}
