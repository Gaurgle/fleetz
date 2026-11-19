//! Pane layout and focus: which panes are drawn and folded, the resize keys
//! and their bounds, the inspector's scroll, the shifted scroll into the next
//! pane, the view toggle and the focus cycle.
//!
//! Moved out of `app.rs` unchanged (FZ-124). The methods stay on `App` and
//! the fields they touch stay plain on it. Design:
//! `docs/app-split-design.md`, "Readability at the end" and D7.

use super::{
    App, CiBody, DEFAULT_INSPECTOR_PCT, DEFAULT_REPO_LIST_PCT, FocusedPanel, HistoryView,
    JobFollow, View,
};

/// Step a percentage by `delta` resize steps, clamped to `[min, max]`.
fn step_pct(current: u16, delta: i16, min: u16, max: u16) -> u16 {
    let next = current as i32 + delta as i32 * RESIZE_STEP as i32;
    next.clamp(min as i32, max as i32) as u16
}

/// Bounds the resize keys clamp to. Both panes stay wide/tall enough to be
/// worth drawing, so a held key can never collapse one to nothing.
pub const REPO_LIST_PCT_MIN: u16 = 20;
pub const REPO_LIST_PCT_MAX: u16 = 60;
pub const INSPECTOR_PCT_MIN: u16 = 15;
pub const INSPECTOR_PCT_MAX: u16 = 70;

/// Percentage points one resize keypress moves a pane edge.
const RESIZE_STEP: u16 = 5;

impl App {
    /// Whether the inspector is drawn at all.
    ///
    /// The graph view's blocks already carry the hash, author, date and full
    /// message, so an inspector there repeats the block while taking the rows
    /// the block needs. Read by both `ui::panel_rects` and `ui::render`
    /// through `detail::split_inspector`, so the two cannot disagree about
    /// whether the pane exists.
    ///
    /// The `notes` layout docks none either: the file's facts sit in pane 2
    /// beside the header (FZ-68), and `ui::panel_rects` gives pane 1 the rows.
    pub fn shows_inspector(&self) -> bool {
        self.history_view != HistoryView::Graph && !self.notes_layout()
    }

    /// Whether the inspector is drawn with its content, not folded to an
    /// empty frame. Shift-scroll aims only at a pane whose rows it can see.
    pub(super) fn inspector_is_expanded(&self) -> bool {
        self.shows_inspector() && !self.inspector_folded
    }

    /// The fold flag of a foldable pane; None for the two that never fold,
    /// the repo list and the detail pane, which are the dashboard itself.
    fn fold_flag(&mut self, panel: FocusedPanel) -> Option<&mut bool> {
        match panel {
            FocusedPanel::Inspector => Some(&mut self.inspector_folded),
            FocusedPanel::Contributions => Some(&mut self.contributions_folded),
            FocusedPanel::RepoList | FocusedPanel::Detail | FocusedPanel::Reader => None,
        }
    }

    /// A pane's number key (`1`-`4`). On a pane that is not focused it
    /// focuses it, unfolding it if folded. On the focused pane it folds or
    /// unfolds it in place, keeping focus, so the same key both reaches a
    /// pane and gets it out of the way.
    pub fn press_panel_key(&mut self, panel: FocusedPanel) {
        let focused = self.focused_panel == panel;
        self.focus_panel(panel);
        // Focus refused: the pane is not on screen (the graph view's
        // inspector), so there is nothing to fold either.
        if self.focused_panel != panel {
            return;
        }
        if let Some(folded) = self.fold_flag(panel) {
            *folded = focused && !*folded;
        }
        self.contributions_unfolded(panel, chrono::Local::now().date_naive());
        self.needs_redraw = true;
    }

    /// Fold `panel` to an empty frame, or unfold it: the click on a pane's
    /// `[3]`/`[4]` tag. Unfolding focuses the pane, since opening it is
    /// asking to look at it; folding leaves focus where it is, the same as
    /// the number key does.
    pub fn toggle_fold(&mut self, panel: FocusedPanel) {
        let Some(folded) = self.fold_flag(panel) else {
            return;
        };
        *folded = !*folded;
        if !*folded {
            self.focus_panel(panel);
        }
        self.contributions_unfolded(panel, chrono::Local::now().date_naive());
        self.needs_redraw = true;
    }

    /// The one unfold entry point the `4` key and the `[4]` tag click share:
    /// a strip on screen with no day marked yet (launched with the setting
    /// off, so never opened) stands on `today`, with today's column in view,
    /// exactly as opening it does (#66). A strip that was travelled keeps its
    /// day: folding is getting it out of the way, not resetting it.
    fn contributions_unfolded(&mut self, panel: FocusedPanel, today: chrono::NaiveDate) {
        if panel != FocusedPanel::Contributions
            || self.contributions_folded
            || self.contrib.cursor.is_some()
        {
            return;
        }
        self.contrib.cursor = Some(self.contrib_last_slot(today));
        // Out of range on purpose, as in `open_contributions`: the renderer
        // clamps it to "newest week at the right edge", which holds today.
        self.drawn.contrib_scroll.set(crate::contributions::WEEKS);
    }

    /// Move the vertical divider between the repo list and the right column.
    /// Positive widens the repo list.
    ///
    /// The step starts from the width on screen (FZ-69). Where the default
    /// yields to the cap in `split_main`, the percentage is not what is
    /// drawn, and stepping it blind made `<` widen the list.
    pub fn resize_repo_list(&mut self, delta: i16) {
        use crate::ui::detail::repo_list_width;
        let area = self.drawn.main_area_width.get();
        let pct = self.repo_list_pct;
        let drawn = repo_list_width(area, pct);
        // The percentage that draws `drawn`, rounded up as a drag converts
        // a column, so the start is the list on screen.
        let uncapped = u32::from(area) * u32::from(pct) / 100;
        let from = if area > 0 && u32::from(drawn) < uncapped {
            (u32::from(drawn) * 100).div_ceil(u32::from(area)) as u16
        } else {
            pct
        };
        let mut next = step_pct(from, delta, REPO_LIST_PCT_MIN, REPO_LIST_PCT_MAX);
        // Landing back on the default would draw the capped width again.
        if delta > 0
            && area > 0
            && next == DEFAULT_REPO_LIST_PCT
            && repo_list_width(area, next) <= drawn
        {
            next = (next + 1).min(REPO_LIST_PCT_MAX);
        }
        // Already narrower than the floor draws: `<` has nowhere to go.
        if delta < 0 && area > 0 && repo_list_width(area, next) > drawn {
            return;
        }
        self.repo_list_pct = next;
        self.needs_redraw = true;
    }

    /// Move the horizontal divider between the detail pane and the inspector.
    /// Positive grows the inspector.
    pub fn resize_inspector(&mut self, delta: i16) {
        self.inspector_pct = step_pct(
            self.inspector_pct,
            delta,
            INSPECTOR_PCT_MIN,
            INSPECTOR_PCT_MAX,
        );
        self.needs_redraw = true;
    }

    /// Set the repo list width directly, as a percentage. For mouse drags,
    /// where the pointer names an absolute position rather than a step. The
    /// clamp is the same one the keys use, so a drag past the edge parks the
    /// divider at the limit instead of collapsing the pane.
    pub fn set_repo_list_pct(&mut self, pct: u16) {
        let clamped = pct.clamp(REPO_LIST_PCT_MIN, REPO_LIST_PCT_MAX);
        if clamped != self.repo_list_pct {
            self.repo_list_pct = clamped;
            self.needs_redraw = true;
        }
    }

    /// Set the inspector height directly, as a percentage of the right column.
    pub fn set_inspector_pct(&mut self, pct: u16) {
        let clamped = pct.clamp(INSPECTOR_PCT_MIN, INSPECTOR_PCT_MAX);
        if clamped != self.inspector_pct {
            self.inspector_pct = clamped;
            self.needs_redraw = true;
        }
    }

    /// Restore both panes to their launch defaults.
    pub fn reset_pane_sizes(&mut self) {
        self.repo_list_pct = DEFAULT_REPO_LIST_PCT;
        self.inspector_pct = DEFAULT_INSPECTOR_PCT;
        self.needs_redraw = true;
    }

    /// Scroll the inspector by `delta` lines, clamped to the content the last
    /// frame actually measured.
    /// On a run (`inspects_ci_run`) it scrolls the shown body's own offset.
    pub fn scroll_inspector(&mut self, delta: i32) {
        let max = self.drawn.inspector_max_scroll.get() as i32;
        if self.inspects_ci_run() {
            self.sync_ci_inspector(std::time::Instant::now());
            let from = i32::from(self.ci_body_scroll());
            self.ci.body_scrolls[self.ci.body.index()] = (from + delta).clamp(0, max) as u16;
            if self.ci.body == CiBody::Jobs {
                self.drawn.ci_job_follow.set(JobFollow::Free);
            }
            return;
        }
        let current = self.inspector_scroll as i32;
        self.inspector_scroll = (current + delta).clamp(0, max) as u16;
    }

    /// The pane `Shift+J`/`K` (and `Shift+↑`/`↓`) drives: the one *after* the
    /// focused pane, so you can stand in one pane and pull more into view in
    /// the next without leaving where you are.
    ///
    /// The strip is never a target. It is a fixed-height horizontal timeline
    /// with nothing below the fold, so scrolling it vertically has no meaning;
    /// it is skipped rather than left in the cycle as a no-op, which would have
    /// made the key do nothing at all from whichever pane sat before it.
    ///
    /// Compact has one flat grid and no next pane, so there is nothing to aim
    /// at and `None` is the honest answer.
    pub(super) fn shift_scroll_target(&self) -> Option<FocusedPanel> {
        if self.view != View::Detail {
            return None;
        }
        Some(match self.focused_panel {
            FocusedPanel::RepoList => FocusedPanel::Detail,
            // In the `notes` layout the pane after the tree is the reader,
            // as Tab goes.
            FocusedPanel::Detail if self.notes_layout() => FocusedPanel::Reader,
            FocusedPanel::Detail if self.inspector_is_expanded() => FocusedPanel::Inspector,
            // The graph docks no inspector, so there is no next pane to reach
            // into -- and it is the one view whose rows are finer than its
            // commits, so there is a gesture worth having here instead. The
            // key stays on this pane and walks its rows one at a time, where
            // plain `j`/`k` jumps whole commit blocks. Wrapping to the repo
            // list instead would spend the only line-by-line gesture the graph
            // has on moving to a different repo.
            FocusedPanel::Detail => FocusedPanel::Detail,
            // The reader keeps the keys: in the `notes` layout the shifted
            // keys scroll it from the tree and from the reader alike
            // (Andreas, 2026-10-29), as the graph keeps them on its rows.
            FocusedPanel::Reader => FocusedPanel::Reader,
            FocusedPanel::Inspector | FocusedPanel::Contributions => FocusedPanel::RepoList,
        })
    }

    /// Scroll the pane after the focused one. See `shift_scroll_target`.
    ///
    /// Aimed at the detail pane it *steps that pane's cursor* rather than
    /// scrolling it: stepping is what makes the inspector read out each commit
    /// in turn, which is the reason to drive window 2 from window 1 at all.
    /// Past the last commit `detail_nav_*` falls through to scrolling, so the
    /// sections below a short list stay reachable. Aimed at the repo list it
    /// walks the selection, which is that list's equivalent.
    pub fn scroll_next_panel(&mut self, delta: i32) {
        let Some(target) = self.shift_scroll_target() else {
            return;
        };
        match target {
            FocusedPanel::RepoList => {
                for _ in 0..delta.unsigned_abs() {
                    match delta > 0 {
                        true => self.move_down(),
                        false => self.move_up(),
                    }
                }
            }
            // Aimed at the pane it is already in: only the graph does that,
            // and only to walk its rows. `move_graph_cursor` is a row cursor,
            // so this is the line-by-line step.
            FocusedPanel::Detail if self.focused_panel == FocusedPanel::Detail => {
                self.move_graph_cursor(delta);
            }
            FocusedPanel::Detail => {
                for _ in 0..delta.unsigned_abs() {
                    match delta > 0 {
                        true => self.detail_nav_down(),
                        false => self.detail_nav_up(),
                    }
                }
            }
            FocusedPanel::Inspector => self.scroll_inspector(delta),
            FocusedPanel::Reader => self.scroll_reader(delta),
            // Never selected as a target, but the match has to be total.
            FocusedPanel::Contributions => {}
        }
    }

    /// Flip between the detail and compact views. Marks the view as
    /// user-chosen so a later terminal resize stops re-picking it.
    pub fn toggle_view(&mut self) {
        self.view_overridden = true;
        self.view = match self.view {
            View::Detail => View::Compact,
            View::Compact => View::Detail,
        };
        // Compact has no detail pane to focus, and switching views should
        // always land somewhere predictable rather than carry over whatever
        // was focused/scrolled before.
        self.focused_panel = FocusedPanel::RepoList;
        self.set_detail_scroll(0);
    }

    /// Cycle keyboard focus: repo list -> detail -> inspector -> strip ->
    /// repo list.
    /// Only meaningful in `Detail` view.
    pub fn toggle_panel_focus(&mut self) {
        if self.view != View::Detail {
            // Compact has one flat grid, but the strip is still a real target.
            self.focused_panel = match self.focused_panel {
                FocusedPanel::Contributions => FocusedPanel::RepoList,
                _ => FocusedPanel::Contributions,
            };
            return;
        }
        // The `notes` layout puts its reader after pane 2, and docks no
        // inspector after it (FZ-68).
        self.focused_panel = match self.focused_panel {
            FocusedPanel::RepoList => FocusedPanel::Detail,
            FocusedPanel::Detail if self.notes_layout() => FocusedPanel::Reader,
            FocusedPanel::Detail | FocusedPanel::Reader if self.shows_inspector() => {
                FocusedPanel::Inspector
            }
            FocusedPanel::Detail | FocusedPanel::Reader | FocusedPanel::Inspector => {
                FocusedPanel::Contributions
            }
            FocusedPanel::Contributions => FocusedPanel::RepoList,
        };
    }

    /// `toggle_panel_focus` in reverse (Shift-Tab): repo list -> strip ->
    /// inspector -> detail -> repo list, skipping a hidden inspector the same
    /// way.
    pub fn toggle_panel_focus_back(&mut self) {
        if self.view != View::Detail {
            // Two targets, so the reverse is the same toggle.
            self.toggle_panel_focus();
            return;
        }
        self.focused_panel = match self.focused_panel {
            FocusedPanel::RepoList => FocusedPanel::Contributions,
            FocusedPanel::Contributions if self.shows_inspector() => FocusedPanel::Inspector,
            FocusedPanel::Contributions | FocusedPanel::Inspector if self.notes_layout() => {
                FocusedPanel::Reader
            }
            FocusedPanel::Contributions | FocusedPanel::Inspector | FocusedPanel::Reader => {
                FocusedPanel::Detail
            }
            FocusedPanel::Detail => FocusedPanel::RepoList,
        };
    }

    /// Jump keyboard focus directly to `panel` (the `1`/`2` bindings).
    /// Only meaningful in `Detail` view.
    pub fn focus_panel(&mut self, panel: FocusedPanel) {
        // The strip is the one panel that exists in both views.
        if panel == FocusedPanel::Contributions {
            self.focused_panel = panel;
            return;
        }
        if self.view != View::Detail {
            return;
        }
        // The graph view draws no inspector, and focus on a pane that is not
        // on screen swallows every later keypress.
        if panel == FocusedPanel::Inspector && !self.shows_inspector() {
            return;
        }
        // The reader exists only in the `notes` layout.
        if panel == FocusedPanel::Reader && !self.notes_layout() {
            return;
        }
        self.focused_panel = panel;
    }
}
