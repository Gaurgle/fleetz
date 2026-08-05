use crate::model::{CiStatus, RepoLocation, RepoStatus, Summary};
use crate::time::{TimeWindow, WINDOW_N_MIN, WindowUnit};
use chrono::{DateTime, Local};

/// Which TUI view is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Detail,
    Compact,
}

/// Which panel has keyboard focus in `Detail` view -- only meaningful there,
/// since `Compact` has a single flat grid. Reachable directly with 1/2/3 or
/// cycled with Tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPanel {
    RepoList,
    /// The detail pane. j/k here moves the commit-selection cursor that
    /// drives the inspector, rather than scrolling.
    Detail,
    /// The inspector pane (full detail of the selected commit). j/k here
    /// scrolls its content.
    Inspector,
}

/// Detail-pane scroll offset is capped well above any realistic content
/// length (recent commits/reflog/graph are all bounded to small constants in
/// main.rs) so an over-eager scroll-down can't grow unbounded.
const DETAIL_SCROLL_CAP: u16 = 300;

/// One quick-filter a repo can be toggled against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    Dirty,
    Behind,
    Ahead,
    CiFailed,
    HasPrs,
    HasStashes,
}

/// Combinable quick filters for the repo list (AND'd together when several are active).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QuickFilters {
    pub dirty: bool,
    pub behind: bool,
    pub ahead: bool,
    pub ci_failed: bool,
    pub has_prs: bool,
    pub has_stashes: bool,
}

impl QuickFilters {
    pub fn is_active(&self, kind: FilterKind) -> bool {
        match kind {
            FilterKind::Dirty => self.dirty,
            FilterKind::Behind => self.behind,
            FilterKind::Ahead => self.ahead,
            FilterKind::CiFailed => self.ci_failed,
            FilterKind::HasPrs => self.has_prs,
            FilterKind::HasStashes => self.has_stashes,
        }
    }

    pub fn matches(&self, repo: &RepoStatus) -> bool {
        (!self.dirty || repo.dirty > 0)
            && (!self.behind || repo.behind > 0)
            && (!self.ahead || repo.ahead > 0)
            && (!self.ci_failed || repo.ci_status == CiStatus::Failed)
            && (!self.has_prs || repo.pr_count > 0)
            && (!self.has_stashes || repo.stash_count > 0)
    }
}

/// Short names quick filters answer to as `#tag` tokens in the search bar,
/// prefix-matched case-insensitively (so `#d` alone resolves to Dirty) --
/// mirrors notez/todoz's own `#tag` filter syntax.
pub const FILTER_TAGS: [(FilterKind, &str); 6] = [
    (FilterKind::Dirty, "dirty"),
    (FilterKind::Behind, "behind"),
    (FilterKind::Ahead, "ahead"),
    (FilterKind::CiFailed, "ci"),
    (FilterKind::HasPrs, "prs"),
    (FilterKind::HasStashes, "stash"),
];

fn set_filter(filters: &mut QuickFilters, kind: FilterKind) {
    match kind {
        FilterKind::Dirty => filters.dirty = true,
        FilterKind::Behind => filters.behind = true,
        FilterKind::Ahead => filters.ahead = true,
        FilterKind::CiFailed => filters.ci_failed = true,
        FilterKind::HasPrs => filters.has_prs = true,
        FilterKind::HasStashes => filters.has_stashes = true,
    }
}

/// Split a search-bar query into its free-text portion and the quick filters
/// it activates. A `#name` word is always consumed as filter syntax -- never
/// falls back to literal text, even if the prefix matches nothing -- and is
/// prefix-matched against `FILTER_TAGS` (a bare `#` activates every filter).
/// Replaces binding quick filters directly to number keys: filters live in
/// the same search text as the substring query, combine the same way, and
/// never occupy a keybinding that a panel-focus jump could use instead.
pub fn parse_query(query: &str) -> (String, QuickFilters) {
    let mut filters = QuickFilters::default();
    let mut text_words = Vec::new();

    for word in query.split_whitespace() {
        let Some(name) = word.strip_prefix('#') else {
            text_words.push(word);
            continue;
        };
        let name = name.to_lowercase();
        if name.is_empty() {
            for &(kind, _) in &FILTER_TAGS {
                set_filter(&mut filters, kind);
            }
            continue;
        }
        for &(kind, tag) in &FILTER_TAGS {
            if tag.starts_with(&name) {
                set_filter(&mut filters, kind);
            }
        }
    }

    (text_words.join(" "), filters)
}

/// Overlay shown on top of the main view.
#[derive(Debug, Clone)]
pub enum Overlay {
    None,
    Help,
    Branches(BranchOverlay),
    TimeWindow(TimeWindowOverlay),
}

/// Progress of an in-flight time-window apply, shown as a footer gauge.
/// `Statting` is the determinate phase over the GitHub-matched repos;
/// `Discovering` and `Finishing` bracket it (their counts are not known).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyProgress {
    Discovering,
    Statting { done: usize, total: usize },
    Finishing,
}

impl ApplyProgress {
    /// Fill fraction for the gauge, in 0.0..=1.0.
    pub fn ratio(&self) -> f64 {
        match self {
            ApplyProgress::Discovering => 0.0,
            ApplyProgress::Statting { done, total } => {
                if *total == 0 {
                    0.0
                } else {
                    (*done as f64 / *total as f64).clamp(0.0, 1.0)
                }
            }
            ApplyProgress::Finishing => 1.0,
        }
    }

    /// Gauge label text.
    pub fn label(&self) -> String {
        match self {
            ApplyProgress::Discovering => "discovering…".to_string(),
            ApplyProgress::Statting { done, total } => format!("{done}/{total} repos"),
            ApplyProgress::Finishing => "finishing…".to_string(),
        }
    }
}

/// State for the branch overlay (switch + cleanup + preview).
#[derive(Debug, Clone)]
pub struct BranchOverlay {
    pub repo_name: String,
    pub repo_path: String,
    pub branches: Vec<BranchEntry>,
    pub selected: usize,
    pub confirm_delete: bool,
    /// Read-only "what happened here" preview: recent commits per branch,
    /// fetched lazily as branches are highlighted (see `main::ensure_preview`).
    pub previews: std::collections::HashMap<String, Vec<crate::model::Commit>>,
}

impl BranchOverlay {
    /// The name of the currently highlighted branch, if any.
    pub fn selected_branch(&self) -> Option<&str> {
        self.branches.get(self.selected).map(|b| b.name.as_str())
    }
}

/// A branch entry in the cleanup overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchEntry {
    pub name: String,
    pub is_merged: bool,
    pub is_current: bool,
    pub is_protected: bool,
    pub selected_for_delete: bool,
}

impl BranchEntry {
    pub fn can_delete(&self) -> bool {
        !self.is_current && !self.is_protected
    }
}

impl BranchOverlay {
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        let max = self.branches.len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
    }

    pub fn toggle_selected(&mut self) {
        if let Some(b) = self.branches.get_mut(self.selected)
            && b.can_delete()
        {
            b.selected_for_delete = !b.selected_for_delete;
        }
    }

    pub fn selected_count(&self) -> usize {
        self.branches
            .iter()
            .filter(|b| b.selected_for_delete)
            .count()
    }

    pub fn selected_branch_names(&self) -> Vec<String> {
        self.branches
            .iter()
            .filter(|b| b.selected_for_delete)
            .map(|b| b.name.clone())
            .collect()
    }
}

/// Which field of the time-window spinner has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowField {
    Number,
    Unit,
}

/// Cycle order for the unit field.
const WINDOW_UNITS: [WindowUnit; 6] = [
    WindowUnit::Slot,
    WindowUnit::Hours,
    WindowUnit::Days,
    WindowUnit::Weeks,
    WindowUnit::Months,
    WindowUnit::Years,
];

/// State for the time-window overlay: a two-field spinner (count + unit).
/// Fully modal -- all input routes here while open, so its hjkl bindings
/// can't collide with list navigation.
#[derive(Debug, Clone)]
pub struct TimeWindowOverlay {
    pub n: u8,
    pub unit: WindowUnit,
    pub field: WindowField,
    /// Rolling buffer of the last typed digits, so "2" then "9" reads as
    /// 29 rather than two separate entries.
    digits: String,
}

impl TimeWindowOverlay {
    pub fn new(current: TimeWindow) -> Self {
        // Slot has no meaningful count, so focus lands on the unit field.
        let field = if current.unit == WindowUnit::Slot {
            WindowField::Unit
        } else {
            WindowField::Number
        };
        Self {
            n: current.n.clamp(WINDOW_N_MIN, current.unit.max_count()),
            unit: current.unit,
            field,
            digits: String::new(),
        }
    }

    pub fn toggle_field(&mut self) {
        self.digits.clear();
        // The number field is dimmed while Slot is selected; focus stays put.
        if self.unit == WindowUnit::Slot {
            self.field = WindowField::Unit;
            return;
        }
        self.field = match self.field {
            WindowField::Number => WindowField::Unit,
            WindowField::Unit => WindowField::Number,
        };
    }

    fn unit_index(&self) -> usize {
        WINDOW_UNITS
            .iter()
            .position(|u| *u == self.unit)
            .unwrap_or(0)
    }

    /// k / Up: number +1 (wrapping), or previous unit.
    pub fn step_up(&mut self) {
        self.digits.clear();
        match self.field {
            WindowField::Number => {
                let max = self.unit.max_count();
                self.n = if self.n >= max {
                    WINDOW_N_MIN
                } else {
                    self.n + 1
                };
            }
            WindowField::Unit => {
                let len = WINDOW_UNITS.len();
                self.unit = WINDOW_UNITS[(self.unit_index() + len - 1) % len];
                self.after_unit_change();
            }
        }
    }

    /// j / Down: number -1 (wrapping), or next unit.
    pub fn step_down(&mut self) {
        self.digits.clear();
        match self.field {
            WindowField::Number => {
                let max = self.unit.max_count();
                self.n = if self.n <= WINDOW_N_MIN {
                    max
                } else {
                    self.n - 1
                };
            }
            WindowField::Unit => {
                self.unit = WINDOW_UNITS[(self.unit_index() + 1) % WINDOW_UNITS.len()];
                self.after_unit_change();
            }
        }
    }

    fn after_unit_change(&mut self) {
        if self.unit == WindowUnit::Slot {
            // Slot ignores the count, so clamping to its nominal max of 1
            // here would wipe the user's number just for cycling past it.
            self.field = WindowField::Unit;
            return;
        }
        self.n = self.n.clamp(WINDOW_N_MIN, self.unit.max_count());
    }

    /// Typed digits set the count directly. Two digits combine ("2" "9" ->
    /// 29); a third starts over. Clamped so typed values can't escape the
    /// valid range.
    pub fn push_digit(&mut self, d: u8) {
        if self.field != WindowField::Number || self.unit == WindowUnit::Slot {
            return;
        }
        if self.digits.len() >= 2 {
            self.digits.clear();
        }
        self.digits.push((b'0' + d) as char);
        let parsed: u8 = self.digits.parse().unwrap_or(WINDOW_N_MIN);
        self.n = parsed.clamp(WINDOW_N_MIN, self.unit.max_count());
    }

    /// The window this overlay currently describes.
    pub fn window(&self) -> TimeWindow {
        TimeWindow::clamped(self.n, self.unit)
    }
}

/// Status message shown temporarily in the footer.
#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub text: String,
    pub is_error: bool,
    /// When the message was set, so the event loop can expire it after a
    /// short TTL instead of it sitting frozen until the next keypress.
    pub created: std::time::Instant,
}

/// Main application state.
pub struct App {
    pub repos: Vec<RepoStatus>,
    pub view: View,
    pub overlay: Overlay,
    pub selected: usize,
    pub search_query: String,
    pub search_active: bool,
    pub status_message: Option<StatusMessage>,
    pub header: String,
    pub should_quit: bool,
    pub show_prs: bool,
    pub watch_mode: bool,
    pub refresh_interval: u64,
    pub last_refreshed: Option<DateTime<Local>>,
    pub group_names: Vec<String>,
    pub active_group: Option<usize>,
    pub focused_panel: FocusedPanel,
    pub detail_scroll: u16,
    /// Index of the commit highlighted in the detail pane's recent-commits
    /// list, which the inspector shows in full. Reset to 0 when the selected
    /// repo changes.
    pub detail_selected: usize,
    /// Scroll offset for the inspector pane (long commit bodies).
    pub inspector_scroll: u16,
    /// Set by the `R` key; the event loop consumes it to re-run the full
    /// `load_repos` (GitHub fetch + discovery), rather than only re-statting
    /// the existing list. Lets a just-pushed or newly-dirty repo appear
    /// without restarting fleetz.
    pub reload_requested: bool,
    /// The last unfiltered GitHub fetch (top repo_limit repos by push date).
    /// The time-window overlay re-filters this in memory, so applying a new
    /// window needs no network round trip.
    pub dataset: Vec<crate::github::GithubRepo>,
    /// The active time window. None means "follow the CLI flags", which is
    /// the state until the user opens the selector (or a saved window was
    /// loaded at startup).
    pub window: Option<crate::time::TimeWindow>,
    /// True when launched with -d: there is no GitHub dataset, so the
    /// time-window selector is unavailable.
    pub dirty_mode: bool,
    /// Set by Enter in the time-window overlay; the event loop consumes it
    /// to re-assemble the view (it needs args/config/repo_dir context the
    /// key handler doesn't have). Mirrors `reload_requested`.
    pub window_apply_requested: bool,
    /// Progress of an in-flight window apply, or None when idle. Drives the
    /// footer gauge; set/cleared by apply_time_window's animate loop.
    pub apply_progress: Option<ApplyProgress>,
}

/// Below this width or height, `Detail`'s list+detail split doesn't have
/// enough room to be useful, so `Compact`'s dense single-line-per-repo grid
/// is the better default. `v` still lets the user override it either way.
const SMALL_WIDTH: u16 = 70;
const SMALL_HEIGHT: u16 = 20;

/// Pick the view a fresh terminal size should start in.
pub fn initial_view(term_width: u16, term_height: u16) -> View {
    if term_width < SMALL_WIDTH || term_height < SMALL_HEIGHT {
        View::Compact
    } else {
        View::Detail
    }
}

impl App {
    pub fn new(
        repos: Vec<RepoStatus>,
        header: String,
        show_prs: bool,
        term_width: u16,
        term_height: u16,
    ) -> Self {
        Self {
            repos,
            view: initial_view(term_width, term_height),
            overlay: Overlay::None,
            selected: 0,
            search_query: String::new(),
            search_active: false,
            status_message: None,
            header,
            should_quit: false,
            show_prs,
            watch_mode: false,
            refresh_interval: 300,
            last_refreshed: Some(Local::now()),
            group_names: Vec::new(),
            active_group: None,
            focused_panel: FocusedPanel::RepoList,
            detail_scroll: 0,
            detail_selected: 0,
            inspector_scroll: 0,
            reload_requested: false,
            dataset: Vec::new(),
            window: None,
            dirty_mode: false,
            window_apply_requested: false,
            apply_progress: None,
        }
    }

    /// After the repo list is replaced by a reload, keep the selection in
    /// range and reset the detail/inspector panes so nothing points at a
    /// commit that no longer exists.
    pub fn clamp_after_reload(&mut self) {
        let len = self.visible_repos().len();
        if self.selected >= len {
            self.selected = len.saturating_sub(1);
        }
        self.detail_scroll = 0;
        self.detail_selected = 0;
        self.inspector_scroll = 0;
    }

    /// Repos filtered by the current search query, active group, and quick
    /// filters -- the latter two both parsed out of `search_query`'s `#tag`
    /// words (see `parse_query`), so there's a single source of truth for
    /// "what's typed" rather than a separate toggled-by-keypress field.
    pub fn visible_repos(&self) -> Vec<(usize, &RepoStatus)> {
        let active_group = self.active_group.and_then(|i| self.group_names.get(i));
        let (text_query, filters) = parse_query(&self.search_query);

        self.repos
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                // Group filter
                if let Some(group) = active_group
                    && !r.groups.contains(group)
                {
                    return false;
                }
                // Search filter
                if !text_query.is_empty() {
                    let q = text_query.to_lowercase();
                    if !r.name.to_lowercase().contains(&q) {
                        return false;
                    }
                }
                // Quick filters (dirty/behind/ahead/CI/PRs/stashes)
                if !filters.matches(r) {
                    return false;
                }
                true
            })
            .collect()
    }

    /// The quick filters currently active per the search query's `#tag`
    /// words -- for the filter-strip dots to light up against.
    pub fn active_filters(&self) -> QuickFilters {
        parse_query(&self.search_query).1
    }

    /// Strip any `#tag` filter words from the search query, keeping free
    /// text intact.
    pub fn clear_filters(&mut self) {
        let (text, _) = parse_query(&self.search_query);
        self.search_query = text;
        self.selected = 0;
    }

    /// Cycle to the next group. None -> first -> second -> ... -> None.
    pub fn cycle_group(&mut self) {
        if self.group_names.is_empty() {
            return;
        }
        self.active_group = match self.active_group {
            None => Some(0),
            Some(i) if i + 1 < self.group_names.len() => Some(i + 1),
            Some(_) => None,
        };
        self.selected = 0;
    }

    /// Clear group filter (show all).
    pub fn clear_group(&mut self) {
        self.active_group = None;
        self.selected = 0;
    }

    /// Label for the active group, if any.
    pub fn active_group_label(&self) -> Option<&str> {
        self.active_group
            .and_then(|i| self.group_names.get(i))
            .map(|s| s.as_str())
    }

    /// The currently selected repo, if any.
    pub fn selected_repo(&self) -> Option<&RepoStatus> {
        let visible = self.visible_repos();
        visible.get(self.selected).map(|(_, r)| *r)
    }

    /// Index into self.repos for the currently selected visible repo.
    pub fn selected_repo_index(&self) -> Option<usize> {
        let visible = self.visible_repos();
        visible.get(self.selected).map(|(i, _)| *i)
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.reset_detail_view();
        }
    }

    pub fn move_down(&mut self) {
        let max = self.visible_repos().len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
            self.reset_detail_view();
        }
    }

    /// Reset the detail pane's scroll + commit selection + inspector scroll,
    /// so moving to a different repo always starts its detail from the top
    /// with the first commit selected rather than inheriting stale offsets.
    fn reset_detail_view(&mut self) {
        self.detail_scroll = 0;
        self.detail_selected = 0;
        self.inspector_scroll = 0;
    }

    /// Number of commits in the recent-commits list of the selected repo --
    /// the range the inspector's commit cursor moves over.
    pub fn selected_commit_count(&self) -> usize {
        self.selected_repo().map_or(0, |r| r.commits.len())
    }

    /// Move the inspector's commit cursor within the selected repo's commits.
    pub fn select_commit_up(&mut self) {
        if self.detail_selected > 0 {
            self.detail_selected -= 1;
            self.inspector_scroll = 0;
        }
    }

    pub fn select_commit_down(&mut self) {
        let max = self.selected_commit_count().saturating_sub(1);
        if self.detail_selected < max {
            self.detail_selected += 1;
            self.inspector_scroll = 0;
        }
    }

    /// Detail-pane down-navigation: step the commit cursor through the recent
    /// commits, then -- once past the last one -- scroll the pane so the
    /// history/reflog/CI sections below the commits become reachable.
    pub fn detail_nav_down(&mut self) {
        if self.detail_selected + 1 < self.selected_commit_count() {
            self.select_commit_down();
        } else {
            self.scroll_detail(1);
        }
    }

    /// Detail-pane up-navigation: unwind any content scroll first, then step
    /// the commit cursor back up -- the mirror of `detail_nav_down`.
    pub fn detail_nav_up(&mut self) {
        if self.detail_scroll > 0 {
            self.scroll_detail(-1);
        } else {
            self.select_commit_up();
        }
    }

    /// The commit the inspector is currently showing, if any.
    pub fn inspected_commit(&self) -> Option<&crate::model::Commit> {
        self.selected_repo()
            .and_then(|r| r.commits.get(self.detail_selected))
    }

    pub fn scroll_inspector(&mut self, delta: i32) {
        let current = self.inspector_scroll as i32;
        self.inspector_scroll = (current + delta).clamp(0, DETAIL_SCROLL_CAP as i32) as u16;
    }

    /// Scroll the focused pane's content by `delta` lines (Shift+scroll)
    /// without moving the selection cursor. From the repo list it drives the
    /// detail pane (window 2), so you can skim the selected repo's history
    /// while browsing the list; from the inspector it scrolls the inspector.
    pub fn scroll_focused(&mut self, delta: i32) {
        match self.focused_panel {
            FocusedPanel::RepoList | FocusedPanel::Detail => self.scroll_detail(delta),
            FocusedPanel::Inspector => self.scroll_inspector(delta),
        }
    }

    pub fn toggle_view(&mut self) {
        self.view = match self.view {
            View::Detail => View::Compact,
            View::Compact => View::Detail,
        };
        // Compact has no detail pane to focus, and switching views should
        // always land somewhere predictable rather than carry over whatever
        // was focused/scrolled before.
        self.focused_panel = FocusedPanel::RepoList;
        self.detail_scroll = 0;
    }

    /// Cycle keyboard focus: repo list -> detail -> inspector -> repo list.
    /// Only meaningful in `Detail` view.
    pub fn toggle_panel_focus(&mut self) {
        if self.view != View::Detail {
            return;
        }
        self.focused_panel = match self.focused_panel {
            FocusedPanel::RepoList => FocusedPanel::Detail,
            FocusedPanel::Detail => FocusedPanel::Inspector,
            FocusedPanel::Inspector => FocusedPanel::RepoList,
        };
    }

    /// Jump keyboard focus directly to `panel` (the `1`/`2` bindings).
    /// Only meaningful in `Detail` view.
    pub fn focus_panel(&mut self, panel: FocusedPanel) {
        if self.view != View::Detail {
            return;
        }
        self.focused_panel = panel;
    }

    /// Scroll the detail pane by `delta` lines (negative scrolls up),
    /// clamped to `[0, DETAIL_SCROLL_CAP]`. The renderer clamps further
    /// against actual content length, so overshoot here just means a couple
    /// of extra keypresses before movement is visible again -- not a stuck
    /// scroll, since detail content is always small (bounded by the commit/
    /// reflog/graph limits in main.rs).
    pub fn scroll_detail(&mut self, delta: i32) {
        let current = self.detail_scroll as i32;
        self.detail_scroll = (current + delta).clamp(0, DETAIL_SCROLL_CAP as i32) as u16;
    }

    pub fn toggle_help(&mut self) {
        self.overlay = match self.overlay {
            Overlay::Help => Overlay::None,
            _ => Overlay::Help,
        };
    }

    /// Open the time-window overlay, prefilled with the active window (or
    /// the default auto slot when none has been set).
    pub fn open_time_window(&mut self) {
        let current = self.window.unwrap_or_default();
        self.overlay = Overlay::TimeWindow(TimeWindowOverlay::new(current));
    }

    pub fn start_search(&mut self) {
        self.search_active = true;
        self.search_query.clear();
        self.selected = 0;
    }

    pub fn search_push_char(&mut self, c: char) {
        self.search_query.push(c);
        self.selected = 0; // reset selection when query changes
    }

    pub fn search_pop_char(&mut self) {
        self.search_query.pop();
        self.selected = 0;
    }

    pub fn end_search(&mut self) {
        self.search_active = false;
    }

    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.search_active = false;
        self.selected = 0;
    }

    pub fn set_status(&mut self, text: String, is_error: bool) {
        self.status_message = Some(StatusMessage {
            text,
            is_error,
            created: std::time::Instant::now(),
        });
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    /// Whether the current status message (if any) has been showing for at
    /// least `ttl` and should be auto-dismissed.
    pub fn status_expired(&self, ttl: std::time::Duration) -> bool {
        self.status_message
            .as_ref()
            .is_some_and(|m| m.created.elapsed() >= ttl)
    }

    pub fn summary(&self) -> Summary {
        Summary::from_repos(&self.repos)
    }

    /// Repos grouped by location for the repo list pane.
    pub fn primary_repos(&self) -> Vec<(usize, &RepoStatus)> {
        self.visible_repos()
            .into_iter()
            .filter(|(_, r)| r.location == RepoLocation::Primary)
            .collect()
    }

    pub fn elsewhere_repos(&self) -> Vec<(usize, &RepoStatus)> {
        self.visible_repos()
            .into_iter()
            .filter(|(_, r)| {
                r.location == RepoLocation::Elsewhere || r.location == RepoLocation::NotCloned
            })
            .collect()
    }

    pub fn local_only_repos(&self) -> Vec<(usize, &RepoStatus)> {
        self.visible_repos()
            .into_iter()
            .filter(|(_, r)| r.location == RepoLocation::LocalOnly)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app(n: usize) -> App {
        let repos: Vec<RepoStatus> = (0..n)
            .map(|i| RepoStatus {
                name: format!("repo-{i}"),
                path: Some(format!("~/repos/repo-{i}")),
                location: RepoLocation::Primary,
                branches: vec![],
                commits: vec![],
                behind: 0,
                ahead: 0,
                dirty: 0,
                pr_count: 0,
                prs: vec![],
                ci_status: crate::model::CiStatus::Unknown,
                ci_workflow: None,
                ci_duration: None,
                groups: vec![],
                stash_count: 0,
                stash_entries: vec![],
                worktrees: vec![],
                reflog: vec![],
                graph: vec![],
                repo_kind: crate::model::RepoKind::Unknown,
                remote_url: None,
                remote_only_branches: vec![],
            })
            .collect();
        App::new(repos, "test".to_string(), false, 120, 40)
    }

    #[test]
    fn navigation_up_down() {
        let mut app = make_app(5);
        assert_eq!(app.selected, 0);
        app.move_down();
        assert_eq!(app.selected, 1);
        app.move_down();
        assert_eq!(app.selected, 2);
        app.move_up();
        assert_eq!(app.selected, 1);
        // Can't go above 0
        app.move_up();
        app.move_up();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn navigation_clamps_at_end() {
        let mut app = make_app(3);
        for _ in 0..10 {
            app.move_down();
        }
        assert_eq!(app.selected, 2);
    }

    #[test]
    fn initial_view_small_terminal_is_compact() {
        assert_eq!(initial_view(SMALL_WIDTH - 1, 40), View::Compact);
        assert_eq!(initial_view(120, SMALL_HEIGHT - 1), View::Compact);
    }

    #[test]
    fn initial_view_roomy_terminal_is_detail() {
        assert_eq!(initial_view(SMALL_WIDTH, SMALL_HEIGHT), View::Detail);
    }

    #[test]
    fn toggle_view() {
        let mut app = make_app(1);
        assert_eq!(app.view, View::Detail);
        app.toggle_view();
        assert_eq!(app.view, View::Compact);
        app.toggle_view();
        assert_eq!(app.view, View::Detail);
    }

    #[test]
    fn search_filters_repos() {
        let mut app = make_app(5);
        app.search_push_char('1');
        let visible = app.visible_repos();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].1.name, "repo-1");
    }

    #[test]
    fn search_resets_selection() {
        let mut app = make_app(5);
        app.move_down();
        app.move_down();
        assert_eq!(app.selected, 2);
        app.search_push_char('0');
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn clear_search_shows_all() {
        let mut app = make_app(5);
        app.search_push_char('1');
        assert_eq!(app.visible_repos().len(), 1);
        app.clear_search();
        assert_eq!(app.visible_repos().len(), 5);
    }

    #[test]
    fn selected_repo_returns_correct_repo() {
        let mut app = make_app(3);
        app.move_down();
        let r = app.selected_repo().unwrap();
        assert_eq!(r.name, "repo-1");
    }

    #[test]
    fn dirty_filter_shows_only_dirty_repos() {
        let mut app = make_app(3);
        app.repos[1].dirty = 2;
        app.search_query = "#dirty".to_string();
        let visible = app.visible_repos();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].1.name, "repo-1");
    }

    #[test]
    fn quick_filters_combine_with_and() {
        let mut app = make_app(3);
        app.repos[0].dirty = 1; // dirty only
        app.repos[1].dirty = 1;
        app.repos[1].behind = 1; // dirty AND behind
        app.repos[2].behind = 1; // behind only

        app.search_query = "#dirty #behind".to_string();

        let visible = app.visible_repos();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].1.name, "repo-1");
    }

    #[test]
    fn filter_tag_prefix_matches() {
        let mut app = make_app(2);
        app.repos[1].dirty = 1;
        app.search_query = "#d".to_string(); // prefix of "dirty"
        let visible = app.visible_repos();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].1.name, "repo-1");
    }

    #[test]
    fn clear_filters_strips_tags_but_keeps_text() {
        let mut app = make_app(3);
        app.repos[1].dirty = 1;
        app.repos[1].name = "repo-1-target".to_string();
        app.search_query = "target #dirty".to_string();
        assert_eq!(app.visible_repos().len(), 1);
        app.clear_filters();
        assert_eq!(app.search_query, "target");
        assert_eq!(app.visible_repos().len(), 1); // text query alone still narrows it
    }

    #[test]
    fn ci_failed_filter_matches_only_failed_ci() {
        let mut app = make_app(2);
        app.repos[0].ci_status = crate::model::CiStatus::Passed;
        app.repos[1].ci_status = crate::model::CiStatus::Failed;
        app.search_query = "#ci".to_string();
        let visible = app.visible_repos();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].1.name, "repo-1");
    }

    #[test]
    fn parse_query_extracts_tag_and_text() {
        let (text, filters) = parse_query("repo #dirty");
        assert_eq!(text, "repo");
        assert!(filters.dirty);
        assert!(!filters.behind);
    }

    #[test]
    fn parse_query_bare_hash_activates_all() {
        let (text, filters) = parse_query("#");
        assert_eq!(text, "");
        assert!(
            filters.dirty
                && filters.behind
                && filters.ahead
                && filters.ci_failed
                && filters.has_prs
                && filters.has_stashes
        );
    }

    #[test]
    fn parse_query_unmatched_tag_is_dropped_not_kept_as_text() {
        let (text, filters) = parse_query("#nonsense hello");
        assert_eq!(text, "hello");
        assert_eq!(filters, QuickFilters::default());
    }

    #[test]
    fn window_overlay_opens_on_number_field() {
        let o = TimeWindowOverlay::new(TimeWindow {
            n: 2,
            unit: WindowUnit::Weeks,
        });
        assert_eq!(o.field, WindowField::Number);
        assert_eq!(o.n, 2);
    }

    #[test]
    fn window_overlay_slot_forces_unit_field() {
        let o = TimeWindowOverlay::new(TimeWindow::default());
        assert_eq!(o.field, WindowField::Unit);
        let mut o = o;
        o.toggle_field();
        assert_eq!(o.field, WindowField::Unit); // can't focus the dimmed number
    }

    #[test]
    fn window_overlay_toggle_field_flips() {
        let mut o = TimeWindowOverlay::new(TimeWindow {
            n: 1,
            unit: WindowUnit::Days,
        });
        assert_eq!(o.field, WindowField::Number);
        o.toggle_field();
        assert_eq!(o.field, WindowField::Unit);
        o.toggle_field();
        assert_eq!(o.field, WindowField::Number);
    }

    #[test]
    fn window_overlay_number_wraps_both_ways() {
        let mut o = TimeWindowOverlay::new(TimeWindow {
            n: 31,
            unit: WindowUnit::Days,
        });
        o.step_up();
        assert_eq!(o.n, 1);
        o.step_down();
        assert_eq!(o.n, 31);
    }

    #[test]
    fn window_overlay_unit_cycles_forward_and_back() {
        let mut o = TimeWindowOverlay::new(TimeWindow {
            n: 1,
            unit: WindowUnit::Days,
        });
        o.toggle_field(); // focus unit
        o.step_down(); // Days -> Weeks
        assert_eq!(o.unit, WindowUnit::Weeks);
        o.step_up(); // Weeks -> Days
        assert_eq!(o.unit, WindowUnit::Days);
    }

    #[test]
    fn window_overlay_unit_wraps_and_slot_grabs_focus() {
        let mut o = TimeWindowOverlay::new(TimeWindow {
            n: 1,
            unit: WindowUnit::Years,
        });
        o.toggle_field(); // focus unit
        o.step_down(); // Years wraps to Slot
        assert_eq!(o.unit, WindowUnit::Slot);
        assert_eq!(o.field, WindowField::Unit);
    }

    #[test]
    fn window_overlay_digit_entry_two_digit_and_clamp() {
        let mut o = TimeWindowOverlay::new(TimeWindow {
            n: 1,
            unit: WindowUnit::Days,
        });
        o.push_digit(2);
        assert_eq!(o.n, 2);
        o.push_digit(9);
        assert_eq!(o.n, 29); // "2" then "9" = 29
        o.push_digit(5);
        assert_eq!(o.n, 5); // third digit starts a fresh entry
        let mut o = TimeWindowOverlay::new(TimeWindow {
            n: 1,
            unit: WindowUnit::Days,
        });
        o.push_digit(3);
        o.push_digit(5);
        assert_eq!(o.n, 31); // 35 clamps to max
        let mut o = TimeWindowOverlay::new(TimeWindow {
            n: 5,
            unit: WindowUnit::Days,
        });
        o.push_digit(0);
        assert_eq!(o.n, 1); // 0 clamps to min
    }

    #[test]
    fn window_overlay_digits_ignored_on_unit_field() {
        let mut o = TimeWindowOverlay::new(TimeWindow {
            n: 4,
            unit: WindowUnit::Days,
        });
        o.toggle_field();
        o.push_digit(9);
        assert_eq!(o.n, 4);
    }

    #[test]
    fn window_overlay_window_returns_clamped() {
        let o = TimeWindowOverlay::new(TimeWindow {
            n: 3,
            unit: WindowUnit::Months,
        });
        assert_eq!(
            o.window(),
            TimeWindow {
                n: 3,
                unit: WindowUnit::Months
            }
        );
    }

    #[test]
    fn open_time_window_prefills_from_app_window() {
        let mut app = make_app(1);
        app.window = Some(TimeWindow {
            n: 2,
            unit: WindowUnit::Weeks,
        });
        app.open_time_window();
        match &app.overlay {
            Overlay::TimeWindow(o) => {
                assert_eq!(o.n, 2);
                assert_eq!(o.unit, WindowUnit::Weeks);
            }
            _ => panic!("expected time window overlay"),
        }
    }

    #[test]
    fn window_overlay_number_wraps_at_unit_max() {
        // Hours cap is 48, so 48 -> step_up wraps to 1.
        let mut o = TimeWindowOverlay::new(TimeWindow {
            n: 48,
            unit: WindowUnit::Hours,
        });
        o.step_up();
        assert_eq!(o.n, 1);
        o.step_down();
        assert_eq!(o.n, 48);
    }

    #[test]
    fn window_overlay_digit_clamps_to_unit_max() {
        // Years cap is 10: typing 4 then 0 (=40) clamps to 10.
        let mut o = TimeWindowOverlay::new(TimeWindow {
            n: 1,
            unit: WindowUnit::Years,
        });
        o.push_digit(4);
        o.push_digit(0);
        assert_eq!(o.n, 10);
    }

    #[test]
    fn window_overlay_unit_change_clamps_n_down() {
        // 52 weeks, switch forward to Years (max 10): n must clamp to 10.
        let mut o = TimeWindowOverlay::new(TimeWindow {
            n: 52,
            unit: WindowUnit::Weeks,
        });
        o.toggle_field(); // focus unit
        o.step_down(); // Weeks -> Months (max 24): 52 clamps to 24
        assert_eq!(o.unit, WindowUnit::Months);
        assert_eq!(o.n, 24);
        o.step_down(); // Months -> Years (max 10): 24 clamps to 10
        assert_eq!(o.unit, WindowUnit::Years);
        assert_eq!(o.n, 10);
    }

    #[test]
    fn window_overlay_cycling_through_slot_preserves_count() {
        // Slot ignores the count, so passing through it must not squash n.
        let mut o = TimeWindowOverlay::new(TimeWindow {
            n: 14,
            unit: WindowUnit::Days,
        });
        o.toggle_field(); // focus unit
        o.step_up(); // Days -> Hours (max 48): 14 fits
        assert_eq!(o.unit, WindowUnit::Hours);
        assert_eq!(o.n, 14);
        o.step_up(); // Hours -> Slot: count must be preserved, not clamped to 1
        assert_eq!(o.unit, WindowUnit::Slot);
        assert_eq!(o.n, 14);
        o.step_up(); // Slot -> Years (max 10): NOW it clamps, to 10
        assert_eq!(o.unit, WindowUnit::Years);
        assert_eq!(o.n, 10);
    }

    #[test]
    fn apply_progress_ratio_and_label() {
        assert_eq!(ApplyProgress::Discovering.ratio(), 0.0);
        assert_eq!(ApplyProgress::Discovering.label(), "discovering…");
        let s = ApplyProgress::Statting {
            done: 12,
            total: 48,
        };
        assert!((s.ratio() - 0.25).abs() < 1e-9);
        assert_eq!(s.label(), "12/48 repos");
        // total 0 must not divide by zero.
        let z = ApplyProgress::Statting { done: 0, total: 0 };
        assert_eq!(z.ratio(), 0.0);
        assert_eq!(ApplyProgress::Finishing.ratio(), 1.0);
        assert_eq!(ApplyProgress::Finishing.label(), "finishing…");
    }
}
