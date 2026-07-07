use chrono::{DateTime, Local};
use crate::model::{self, CiStatus, RepoLocation, RepoStatus, Summary};

/// Which TUI view is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Detail,
    Compact,
}

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
    pub fn toggle(&mut self, kind: FilterKind) {
        let flag = match kind {
            FilterKind::Dirty => &mut self.dirty,
            FilterKind::Behind => &mut self.behind,
            FilterKind::Ahead => &mut self.ahead,
            FilterKind::CiFailed => &mut self.ci_failed,
            FilterKind::HasPrs => &mut self.has_prs,
            FilterKind::HasStashes => &mut self.has_stashes,
        };
        *flag = !*flag;
    }

    pub fn matches(&self, repo: &RepoStatus) -> bool {
        (!self.dirty || repo.dirty > 0)
            && (!self.behind || repo.behind > 0)
            && (!self.ahead || repo.ahead > 0)
            && (!self.ci_failed || repo.ci_status == CiStatus::Failed)
            && (!self.has_prs || repo.pr_count > 0)
            && (!self.has_stashes || repo.stash_count > 0)
    }

    /// Short labels for currently active filters, for the header tag.
    pub fn active_labels(&self) -> Vec<&'static str> {
        let mut labels = Vec::new();
        if self.dirty {
            labels.push("dirty");
        }
        if self.behind {
            labels.push("behind");
        }
        if self.ahead {
            labels.push("ahead");
        }
        if self.ci_failed {
            labels.push("CI✗");
        }
        if self.has_prs {
            labels.push("PRs");
        }
        if self.has_stashes {
            labels.push("stash");
        }
        labels
    }
}

/// Overlay shown on top of the main view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    None,
    Help,
    Branches(BranchOverlay),
}

/// State for the branch cleanup overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchOverlay {
    pub repo_name: String,
    pub repo_path: String,
    pub branches: Vec<BranchEntry>,
    pub selected: usize,
    pub confirm_delete: bool,
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
        if let Some(b) = self.branches.get_mut(self.selected) {
            if b.can_delete() {
                b.selected_for_delete = !b.selected_for_delete;
            }
        }
    }

    pub fn selected_count(&self) -> usize {
        self.branches.iter().filter(|b| b.selected_for_delete).count()
    }

    pub fn selected_branch_names(&self) -> Vec<String> {
        self.branches
            .iter()
            .filter(|b| b.selected_for_delete)
            .map(|b| b.name.clone())
            .collect()
    }
}

/// Status message shown temporarily in the footer.
#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub text: String,
    pub is_error: bool,
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
    pub show_stat: bool,
    pub watch_mode: bool,
    pub refresh_interval: u64,
    pub last_refreshed: Option<DateTime<Local>>,
    pub group_names: Vec<String>,
    pub active_group: Option<usize>,
    pub filters: QuickFilters,
}

impl App {
    pub fn new(repos: Vec<RepoStatus>, header: String, show_prs: bool, show_stat: bool) -> Self {
        Self {
            repos,
            view: View::Detail,
            overlay: Overlay::None,
            selected: 0,
            search_query: String::new(),
            search_active: false,
            status_message: None,
            header,
            should_quit: false,
            show_prs,
            show_stat,
            watch_mode: false,
            refresh_interval: 300,
            last_refreshed: Some(Local::now()),
            group_names: Vec::new(),
            active_group: None,
            filters: QuickFilters::default(),
        }
    }

    /// Repos filtered by the current search query, active group, and quick filters.
    pub fn visible_repos(&self) -> Vec<(usize, &RepoStatus)> {
        let active_group = self.active_group.and_then(|i| self.group_names.get(i));

        self.repos
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                // Group filter
                if let Some(group) = active_group {
                    if !r.groups.contains(group) {
                        return false;
                    }
                }
                // Search filter
                if !self.search_query.is_empty() {
                    let q = self.search_query.to_lowercase();
                    if !r.name.to_lowercase().contains(&q) {
                        return false;
                    }
                }
                // Quick filters (dirty/behind/ahead/CI/PRs/stashes)
                if !self.filters.matches(r) {
                    return false;
                }
                true
            })
            .collect()
    }

    /// Toggle one quick filter on/off; filters combine with AND.
    pub fn toggle_filter(&mut self, kind: FilterKind) {
        self.filters.toggle(kind);
        self.selected = 0;
    }

    /// Clear all active quick filters.
    pub fn clear_filters(&mut self) {
        self.filters = QuickFilters::default();
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
        }
    }

    pub fn move_down(&mut self) {
        let max = self.visible_repos().len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
    }

    pub fn toggle_view(&mut self) {
        self.view = match self.view {
            View::Detail => View::Compact,
            View::Compact => View::Detail,
        };
    }

    pub fn toggle_help(&mut self) {
        self.overlay = match self.overlay {
            Overlay::Help => Overlay::None,
            _ => Overlay::Help,
        };
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
        self.status_message = Some(StatusMessage { text, is_error });
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
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
                ci_status: model::CiStatus::Unknown,
                ci_workflow: None,
                ci_duration: None,
                pushed_at: None,
                groups: vec![],
                stash_count: 0,
                stash_entries: vec![],
                worktrees: vec![],
            })
            .collect();
        App::new(repos, "test".to_string(), false, false)
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
        app.toggle_filter(FilterKind::Dirty);
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

        app.toggle_filter(FilterKind::Dirty);
        app.toggle_filter(FilterKind::Behind);

        let visible = app.visible_repos();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].1.name, "repo-1");
    }

    #[test]
    fn toggling_same_filter_twice_clears_it() {
        let mut app = make_app(3);
        app.repos[1].dirty = 1;
        app.toggle_filter(FilterKind::Dirty);
        app.toggle_filter(FilterKind::Dirty);
        assert_eq!(app.visible_repos().len(), 3);
    }

    #[test]
    fn clear_filters_shows_all() {
        let mut app = make_app(3);
        app.repos[1].dirty = 1;
        app.toggle_filter(FilterKind::Dirty);
        assert_eq!(app.visible_repos().len(), 1);
        app.clear_filters();
        assert_eq!(app.visible_repos().len(), 3);
    }

    #[test]
    fn ci_failed_filter_matches_only_failed_ci() {
        let mut app = make_app(2);
        app.repos[0].ci_status = model::CiStatus::Passed;
        app.repos[1].ci_status = model::CiStatus::Failed;
        app.toggle_filter(FilterKind::CiFailed);
        let visible = app.visible_repos();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].1.name, "repo-1");
    }
}
