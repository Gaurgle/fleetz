//! The `worktrees` view: the newest commits per worktree, and the `App`
//! methods that list, claim, land and read them (`app.worktrees`).
//!
//! Moved out of `app.rs` unchanged but for the field paths (FZ-120). The
//! methods stay on `App`: each reads the repo list, the view or the focused
//! panel. Design: `docs/app-split-design.md`, "Owner types" (the per-view
//! row), D5 and D7, owner item O-E.

use super::{App, FocusedPanel, HistoryView, View};

/// The `worktrees` view's state, moved off `App` unchanged (FZ-120).
pub(crate) struct WorktreesView {
    /// The `worktrees` view's newest commits per worktree, keyed by the
    /// worktree's path as the row holds it, for the session (FZ-77). Read
    /// only while the view is on screen (`worktrees_wanted`).
    pub(crate) logs: std::collections::HashMap<String, WorktreeLog>,
    /// Bumped when a sweep lands: a held worktree log from an older
    /// generation is read again the next time it is shown.
    pub(crate) generation: u64,
    /// The row path the cursor rests on in the `worktrees` view and since
    /// when, for the dwell (`ci::DWELL`) before a log read starts.
    pub(crate) cursor: Option<(Option<String>, std::time::Instant)>,
}

impl WorktreesView {
    pub(crate) fn new() -> Self {
        Self {
            logs: std::collections::HashMap::new(),
            generation: 0,
            cursor: None,
        }
    }
}

/// One worktree's newest commits as the `worktrees` view's inspector draws
/// them (FZ-77): `None` until a read lands.
#[derive(Debug, Clone, Default)]
pub struct WorktreeLog {
    pub commits: Option<Vec<crate::model::Commit>>,
    /// The generation `commits` was read in (`WorktreesView::generation`).
    pub generation: u64,
    /// A read is in flight; one per worktree at a time.
    pub reading: bool,
}

/// How many commits the `worktrees` inspector lists for a worktree.
pub const WORKTREE_LOG_COUNT: u32 = 6;

/// A claimed log read of one worktree, for `git::branch_commits` on `HEAD`
/// off the UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeLogRead {
    /// The worktree's path as its row holds it, the `WorktreesView::logs` key.
    pub key: String,
    pub dir: std::path::PathBuf,
    pub generation: u64,
}

impl App {
    /// Whether the `worktrees` view is on screen (FZ-77): the one gate its
    /// log read passes.
    pub fn worktrees_wanted(&self) -> bool {
        self.view == View::Detail && self.history_view == HistoryView::Worktrees
    }

    /// The worktrees the `worktrees` view lists for the selected repo, in
    /// drawing order: the main checkout first, then the linked ones as
    /// `git worktree list` gave them. Empty for a row with nothing on disk.
    pub fn worktrees_listed(&self) -> Vec<crate::model::Worktree> {
        let Some(repo) = self.selected_repo() else {
            return Vec::new();
        };
        let Some(main) = repo.main_worktree() else {
            return Vec::new();
        };
        std::iter::once(main)
            .chain(repo.worktrees.iter().cloned())
            .collect()
    }

    /// The worktree the `worktrees` view's cursor is on, clamped to the
    /// rows there are.
    pub fn worktree_highlighted(&self) -> Option<crate::model::Worktree> {
        let mut listed = self.worktrees_listed();
        let last = listed.len().checked_sub(1)?;
        let at = self.detail_selections[HistoryView::Worktrees.index()].min(last);
        Some(listed.swap_remove(at))
    }

    /// The directory `Enter`, `e` and `o` act in, `~` expanded: the
    /// highlighted worktree's while pane 2 has focus on the `worktrees`
    /// view (FZ-77), else the selected repo's. `None` for a row with
    /// nothing on disk.
    pub fn action_dir(&self) -> Option<String> {
        let worktree = (self.worktrees_wanted() && self.focused_panel == FocusedPanel::Detail)
            .then(|| self.worktree_highlighted())
            .flatten();
        let path = match worktree {
            Some(wt) => wt.path,
            None => self.selected_repo()?.path.clone()?,
        };
        Some(shellexpand::tilde(&path).to_string())
    }

    /// The held log of the highlighted worktree, `None` while none landed.
    pub fn worktree_log(&self) -> Option<&[crate::model::Commit]> {
        let wt = self.worktree_highlighted()?;
        self.worktrees.logs.get(&wt.path)?.commits.as_deref()
    }

    /// Claim the log read of the highlighted worktree at `instant`, or
    /// `None`: only while the `worktrees` view is on screen with the
    /// inspector open, once the cursor has rested on the repo for
    /// `ci::DWELL` (as `claim_notes_walk` waits), and only when the log is
    /// missing or from an older generation. One read per worktree at a time.
    pub fn claim_worktree_log(&mut self, instant: std::time::Instant) -> Option<WorktreeLogRead> {
        if !self.worktrees_wanted() || !self.inspector_is_expanded() {
            return None;
        }
        let repo = self.selected_repo().and_then(|r| r.path.clone());
        let settled_for = match &self.worktrees.cursor {
            Some((held, since)) if *held == repo => instant.duration_since(*since),
            _ => {
                self.worktrees.cursor = Some((repo, instant));
                std::time::Duration::ZERO
            }
        };
        if settled_for < crate::ci::DWELL {
            return None;
        }
        let key = self.worktree_highlighted()?.path;
        let generation = self.worktrees.generation;
        let state = self.worktrees.logs.entry(key.clone()).or_default();
        let fresh = state.commits.is_some() && state.generation == generation;
        if state.reading || fresh {
            return None;
        }
        state.reading = true;
        let dir = std::path::PathBuf::from(shellexpand::tilde(&key).as_ref());
        Some(WorktreeLogRead {
            key,
            dir,
            generation,
        })
    }

    /// Land a log read (`AppEvent::WorktreeLogRead`) and ask for a redraw.
    pub fn apply_worktree_log(
        &mut self,
        read: WorktreeLogRead,
        commits: Vec<crate::model::Commit>,
    ) {
        let state = self.worktrees.logs.entry(read.key).or_default();
        state.reading = false;
        state.commits = Some(commits);
        state.generation = read.generation;
        self.needs_redraw = true;
    }
}
