//! The `changes` view: its read per repo, the base it compares with, the
//! highlighted file's diff, and the `App` methods that claim, land and read
//! them (`app.changes`).
//!
//! Moved out of `app.rs` unchanged but for the field paths (FZ-120). The
//! methods stay on `App`: each reads the repo list, the view or the focused
//! panel. Design: `docs/app-split-design.md`, "Owner types" (the per-view
//! row), D5 and D7, owner item O-E.

use super::{App, HistoryView, View};

/// The `changes` view's state, moved off `App` unchanged (FZ-120).
pub(crate) struct ChangesView {
    /// The `changes` view's read per repo, keyed by the row's path, held for
    /// the session and never written to disk (FZ-60). Read only while the
    /// view is on screen (`changes_wanted`).
    pub(crate) states: std::collections::HashMap<String, ChangesState>,
    /// Bumped when a sweep lands or a suspend returns: a read or a diff from
    /// an older generation is read again the next time it is shown.
    pub(crate) generation: u64,
    /// `u`: what the second section compares `HEAD` with.
    pub(crate) base: crate::git::ChangesBase,
    /// The row path the cursor rests on in the `changes` view and since
    /// when, for the dwell (`ci::DWELL`) before a read starts.
    pub(crate) cursor: Option<(Option<String>, std::time::Instant)>,
    /// The highlighted file's diff for the inspector: one file at a time.
    pub(crate) diff: Option<ChangeDiff>,
}

impl ChangesView {
    pub(crate) fn new() -> Self {
        Self {
            states: std::collections::HashMap::new(),
            generation: 0,
            base: crate::git::ChangesBase::default(),
            cursor: None,
            diff: None,
        }
    }
}

/// The `changes` view's read of one repo (FZ-60).
#[derive(Debug, Clone, Default)]
pub struct ChangesState {
    /// The last read's answer, kept on screen while a newer one runs.
    pub held: Option<Result<crate::git::Changes, String>>,
    /// A read is running.
    pub reading: bool,
    /// The generation and base `held` was read in.
    pub generation: u64,
    pub base: crate::git::ChangesBase,
}

/// A claimed read of one repo's `changes`, for `git::changes` off the UI
/// thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangesRead {
    pub key: String,
    pub root: std::path::PathBuf,
    pub generation: u64,
    pub base: crate::git::ChangesBase,
}

/// Which section of the `changes` view a file is listed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeSection {
    Uncommitted,
    Outgoing,
}

/// A claimed diff of the highlighted `changes` file, for
/// `git::changed_file_diff`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRead {
    pub root: std::path::PathBuf,
    pub section: ChangeSection,
    pub file: crate::git::ChangedFile,
    /// The outgoing section's base ref; `None` for an uncommitted file.
    pub base: Option<String>,
    pub generation: u64,
}

/// The highlighted file's diff, as `read` asked for it.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeDiff {
    pub read: DiffRead,
    /// `None` while the read runs.
    pub text: Option<Result<String, String>>,
}

impl App {
    /// Whether the `changes` view may read a repo or a diff: pane 2 is on
    /// screen showing it. The one gate both reads pass (FZ-60).
    pub fn changes_wanted(&self) -> bool {
        self.view == View::Detail && self.history_view == HistoryView::Changes
    }

    /// The selected row's read, by its path; `None` for a row with nothing
    /// on disk to read, and a `None` state while nothing was read yet.
    pub fn selected_changes(&self) -> Option<(&str, Option<&ChangesState>)> {
        let path = self.selected_repo()?.path.as_deref()?;
        Some((path, self.changes.states.get(path)))
    }

    /// The selected row's sections as last read, when the read succeeded.
    pub fn changes_held(&self) -> Option<&crate::git::Changes> {
        self.selected_changes()?.1?.held.as_ref()?.as_ref().ok()
    }

    /// The files the `changes` view lists, uncommitted then outgoing, in
    /// drawing order: what its cursor walks.
    pub fn changes_listed(&self) -> Vec<(ChangeSection, &crate::git::ChangedFile)> {
        let Some(changes) = self.changes_held() else {
            return Vec::new();
        };
        let uncommitted = changes
            .uncommitted
            .iter()
            .map(|f| (ChangeSection::Uncommitted, f));
        let outgoing = changes
            .outgoing
            .files()
            .iter()
            .map(|f| (ChangeSection::Outgoing, f));
        uncommitted.chain(outgoing).collect()
    }

    /// The file the `changes` view's cursor is on, clamped to the files
    /// there are.
    pub fn changes_highlighted(&self) -> Option<(ChangeSection, &crate::git::ChangedFile)> {
        let listed = self.changes_listed();
        let last = listed.len().checked_sub(1)?;
        Some(listed[self.detail_selections[HistoryView::Changes.index()].min(last)])
    }

    /// Claim the read of the selected repo the `changes` view needs at
    /// `instant`, or `None`. Nothing unless the view is on screen, the
    /// cursor has rested on the row for `ci::DWELL`, and the row's read is
    /// missing, from an older generation or for the other base; one read
    /// per repo at a time.
    pub fn claim_changes_read(&mut self, instant: std::time::Instant) -> Option<ChangesRead> {
        if !self.changes_wanted() {
            return None;
        }
        let key = self.selected_repo().and_then(|r| r.path.clone());
        let settled_for = match &self.changes.cursor {
            Some((held, since)) if *held == key => instant.duration_since(*since),
            _ => {
                self.changes.cursor = Some((key.clone(), instant));
                std::time::Duration::ZERO
            }
        };
        let key = key?;
        if settled_for < crate::ci::DWELL {
            return None;
        }
        let (generation, base) = (self.changes.generation, self.changes.base);
        let state = self.changes.states.entry(key.clone()).or_default();
        let fresh = state.held.is_some() && state.generation == generation && state.base == base;
        if state.reading || fresh {
            return None;
        }
        state.reading = true;
        let root = std::path::PathBuf::from(shellexpand::tilde(&key).as_ref());
        Some(ChangesRead {
            key,
            root,
            generation,
            base,
        })
    }

    /// Land a read (`AppEvent::ChangesRead`) and ask for a redraw.
    pub fn apply_changes_read(
        &mut self,
        read: ChangesRead,
        result: Result<crate::git::Changes, String>,
    ) {
        let state = self.changes.states.entry(read.key).or_default();
        state.reading = false;
        state.held = Some(result);
        state.generation = read.generation;
        state.base = read.base;
        self.needs_redraw = true;
    }

    /// Claim the diff of the highlighted file, or `None`: only while the
    /// `changes` view shows with the inspector open, and only when the diff
    /// held is another file's, base's or generation's. One file at a time,
    /// whatever the cursor passed over since the last tick. A new file
    /// starts the inspector at its top.
    pub fn claim_change_diff(&mut self) -> Option<DiffRead> {
        if !self.changes_wanted() || self.inspector_folded {
            return None;
        }
        let root = self.changes_root()?;
        let base = self
            .changes_held()
            .and_then(|c| c.outgoing.base_ref())
            .map(str::to_string);
        let (section, file) = self.changes_highlighted()?;
        let read = DiffRead {
            root,
            section,
            file: file.clone(),
            base: match section {
                ChangeSection::Uncommitted => None,
                ChangeSection::Outgoing => base,
            },
            generation: self.changes.generation,
        };
        let held = self.changes.diff.as_ref().map(|d| &d.read);
        if held == Some(&read) {
            return None;
        }
        if held.is_none_or(|h| {
            h.root != read.root || h.file.path != read.file.path || h.section != read.section
        }) {
            self.inspector_scroll = 0;
        }
        self.changes.diff = Some(ChangeDiff {
            read: read.clone(),
            text: None,
        });
        Some(read)
    }

    /// Land a diff (`AppEvent::ChangeDiff`); one no longer wanted is dropped.
    pub fn apply_change_diff(&mut self, read: DiffRead, text: Result<String, String>) {
        if let Some(held) = &mut self.changes.diff
            && held.read == read
        {
            held.text = Some(text);
            self.needs_redraw = true;
        }
    }

    /// The held diff, when it is the highlighted file's in the selected
    /// repo: a same-named file in another repo is not it.
    pub fn highlighted_change_diff(&self) -> Option<&ChangeDiff> {
        let root = self.changes_root()?;
        let (section, file) = self.changes_highlighted()?;
        self.changes.diff.as_ref().filter(|d| {
            d.read.root == root && d.read.section == section && d.read.file.path == file.path
        })
    }

    /// The selected row's directory, `~` expanded, as a diff read names it.
    fn changes_root(&self) -> Option<std::path::PathBuf> {
        let (path, _) = self.selected_changes()?;
        Some(std::path::PathBuf::from(shellexpand::tilde(path).as_ref()))
    }

    /// A sweep landed or a suspend returned: the held reads and the diff
    /// are read again the next time they are shown.
    pub fn invalidate_changes(&mut self) {
        self.changes.generation += 1;
        self.needs_redraw = true;
    }

    /// `u` in the `changes` view: compare `HEAD` with the default branch
    /// instead of the upstream, or back. Nothing elsewhere.
    pub fn toggle_changes_base(&mut self) {
        if !self.changes_wanted() {
            return;
        }
        use crate::git::ChangesBase;
        self.changes.base = match self.changes.base {
            ChangesBase::Upstream => ChangesBase::DefaultBranch,
            ChangesBase::DefaultBranch => ChangesBase::Upstream,
        };
        self.needs_redraw = true;
    }

    /// The highlighted `changes` file as an absolute path, for `Enter`.
    pub fn change_to_open(&self) -> Option<std::path::PathBuf> {
        let root = self.selected_changes()?.0;
        let (_, file) = self.changes_highlighted()?;
        Some(std::path::PathBuf::from(shellexpand::tilde(root).as_ref()).join(&file.path))
    }

    /// `y` in the `changes` view: copy the highlighted file's repo-relative
    /// path through `copy`.
    pub fn yank_change_path(&mut self, copy: impl FnOnce(&str) -> Result<(), String>) {
        let Some(path) = self.changes_highlighted().map(|(_, f)| f.path.clone()) else {
            self.set_status("nothing to copy".to_string(), false);
            return;
        };
        self.copy_hash(&path, copy);
    }
}

#[cfg(test)]
mod tests;
