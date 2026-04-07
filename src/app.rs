use crate::model::{RepoLocation, RepoStatus, Summary};

/// Which TUI view is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Detail,
    Compact,
}

/// Overlay shown on top of the main view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    Help,
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
        }
    }

    /// Repos filtered by the current search query.
    pub fn visible_repos(&self) -> Vec<(usize, &RepoStatus)> {
        if self.search_query.is_empty() {
            self.repos.iter().enumerate().collect()
        } else {
            let q = self.search_query.to_lowercase();
            self.repos
                .iter()
                .enumerate()
                .filter(|(_, r)| r.name.to_lowercase().contains(&q))
                .collect()
        }
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
            Overlay::None => Overlay::Help,
            Overlay::Help => Overlay::None,
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
                pushed_at: None,
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
}
