//! The `notes` view and its reader: the walk per repo, the highlighted
//! file's body, the reader's scroll and mode, and the `App` methods that
//! claim, land and read them (`app.notes`).
//!
//! Moved out of `app.rs` unchanged but for the field paths (FZ-119). The
//! methods stay on `App`: each reads the repo list, the view, the focused
//! panel or the drawn reader. Design: `docs/app-split-design.md`, "Owner
//! types" (the per-view row), D5 and D7, owner item O-E.

use super::{App, FocusedPanel, HistoryView, View};

/// The `notes` view's state, moved off `App` unchanged (FZ-119).
pub(crate) struct NotesView {
    /// The `notes` view's walk per repo, keyed by the row's path, held for
    /// the session and never written to disk (FZ-53). Walked only while the
    /// view is on screen (`notes_wanted`).
    pub(crate) states: std::collections::HashMap<String, NotesState>,
    /// Bumped when a sweep lands or a suspend returns: a walk or a body read
    /// from an older generation is read again the next time it is shown.
    pub(crate) generation: u64,
    /// `m`: the tree lists every markdown file, not only the notes
    /// directories. The walk collected both; this changes only the drawing.
    pub(crate) all_markdown: bool,
    /// The row path the cursor rests on in the `notes` view and since when,
    /// for the dwell (`ci::DWELL`) before a walk starts.
    pub(crate) cursor: Option<(Option<String>, std::time::Instant)>,
    /// The highlighted file's body and facts, for the reader and the facts
    /// beside pane 2's header: one file at a time.
    pub(crate) body: Option<NoteBody>,
    /// The footer has said once this session that a board went to
    /// `$EDITOR` because `todoz` is not on PATH (FZ-63).
    pub(crate) todoz_fallback_said: bool,
    /// The reader's scroll offset (FZ-62), its own rather than the
    /// inspector's. Reset when the file changes.
    pub(crate) reader_scroll: u16,
    /// `p` in the `notes` layout: the reader shows markdown as its source.
    /// One flag for the session, not per file; other kinds are always raw.
    pub(crate) reader_raw: bool,
}

impl NotesView {
    pub(crate) fn new() -> Self {
        Self {
            states: std::collections::HashMap::new(),
            generation: 0,
            all_markdown: false,
            cursor: None,
            body: None,
            todoz_fallback_said: false,
            reader_scroll: 0,
            reader_raw: false,
        }
    }
}

/// The `notes` view's walk of one repo (FZ-53).
#[derive(Debug, Clone, Default)]
pub struct NotesState {
    /// The last walk's answer, kept on screen while a newer one runs.
    pub tree: Option<Result<crate::notes::Notes, String>>,
    /// A walk is running.
    pub walking: bool,
    /// The generation `tree` was walked in (`NotesView::generation`).
    pub generation: u64,
}

/// The body and facts of one file in the `notes` view: the reader draws the
/// text, the facts beside pane 2's header the facts.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteBody {
    pub path: std::path::PathBuf,
    pub generation: u64,
    /// `None` while the read runs.
    pub text: Option<Result<String, String>>,
    /// `None` while the read runs; read in the same blocking read as `text`.
    pub facts: Option<crate::notes::NoteFacts>,
    /// `None` until git answers, which lands after the body.
    pub last_commit: Option<crate::notes::LastCommit>,
    /// A markdown body rendered for the reader (FZ-59), at the width it
    /// was asked for; `None` until a render lands.
    pub rendered: Option<RenderedNote>,
    /// The width of the render in flight, so one is asked for only once.
    pub render_asked: Option<u16>,
}

/// A markdown body as `ui::markdown::render_markdown` laid it out: one
/// entry per reader row at `width`.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedNote {
    pub width: u16,
    pub lines: Vec<ratatui::text::Line<'static>>,
}

/// A claimed render of the held markdown body at a new reader width
/// (`App::claim_note_render`), for a blocking task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteRender {
    pub path: std::path::PathBuf,
    pub generation: u64,
    pub width: u16,
    pub text: String,
}

/// A claimed walk of one repo, for `notes::walk` off the UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotesWalk {
    pub key: String,
    pub root: std::path::PathBuf,
    pub generation: u64,
}

/// A claimed read of the highlighted file, for `notes::read_body` and the
/// file's facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteRead {
    pub path: std::path::PathBuf,
    /// The repo the file sits in, where git is asked for its last commit.
    pub root: std::path::PathBuf,
    pub generation: u64,
    /// The reader width to render a markdown body at with the read
    /// (FZ-59); `None` for another kind or before the reader has drawn.
    pub render_width: Option<u16>,
}

impl App {
    /// Whether the `notes` view may walk a repo or read a file: pane 2 is on
    /// screen showing it. The one gate the walk and the body read pass.
    pub fn notes_wanted(&self) -> bool {
        self.view == View::Detail && self.history_view == HistoryView::Notes
    }

    /// The selected row's walk, by its path; `None` for a row with nothing
    /// on disk to walk, and a `None` state while nothing was walked yet.
    pub fn selected_notes(&self) -> Option<(&str, Option<&NotesState>)> {
        let path = self.selected_repo()?.path.as_deref()?;
        Some((path, self.notes.states.get(path)))
    }

    /// The files the `notes` view lists for the selected repo, in drawing
    /// order: what its cursor walks.
    pub fn notes_listed(&self) -> Vec<&crate::notes::NoteFile> {
        match self.selected_notes() {
            Some((
                _,
                Some(NotesState {
                    tree: Some(Ok(notes)),
                    ..
                }),
            )) => notes.listed(self.notes.all_markdown),
            _ => Vec::new(),
        }
    }

    /// The file the `notes` view's cursor is on, as an absolute path: read
    /// from the view's own selection slot, clamped to the files there are.
    pub fn notes_highlighted(&self) -> Option<std::path::PathBuf> {
        let root = self.selected_notes()?.0;
        let file = self.notes_highlighted_file()?;
        Some(std::path::PathBuf::from(shellexpand::tilde(root).as_ref()).join(&file.rel))
    }

    /// The walk's entry for the file the cursor is on: what the facts and
    /// the reader describe (FZ-62, FZ-68).
    pub fn notes_highlighted_file(&self) -> Option<&crate::notes::NoteFile> {
        let listed = self.notes_listed();
        let last = listed.len().checked_sub(1)?;
        Some(listed[self.detail_selections[HistoryView::Notes.index()].min(last)])
    }

    /// Claim the walk of the selected repo the `notes` view needs at
    /// `instant`, or `None`. Nothing unless the view is on screen, the
    /// cursor has rested on the row for `ci::DWELL`, and the row's walk is
    /// missing or from an older generation; one walk per repo at a time.
    pub fn claim_notes_walk(&mut self, instant: std::time::Instant) -> Option<NotesWalk> {
        if !self.notes_wanted() {
            return None;
        }
        let key = self.selected_repo().and_then(|r| r.path.clone());
        let settled_for = match &self.notes.cursor {
            Some((held, since)) if *held == key => instant.duration_since(*since),
            _ => {
                self.notes.cursor = Some((key.clone(), instant));
                std::time::Duration::ZERO
            }
        };
        let key = key?;
        if settled_for < crate::ci::DWELL {
            return None;
        }
        let generation = self.notes.generation;
        let state = self.notes.states.entry(key.clone()).or_default();
        let fresh = state.tree.is_some() && state.generation == generation;
        if state.walking || fresh {
            return None;
        }
        state.walking = true;
        let root = std::path::PathBuf::from(shellexpand::tilde(&key).as_ref());
        Some(NotesWalk {
            key,
            root,
            generation,
        })
    }

    /// Land a walk (`AppEvent::NotesWalked`) and ask for a redraw.
    pub fn apply_notes_walk(
        &mut self,
        walk: NotesWalk,
        result: Result<crate::notes::Notes, String>,
    ) {
        let state = self.notes.states.entry(walk.key).or_default();
        state.walking = false;
        state.tree = Some(result);
        state.generation = walk.generation;
        self.needs_redraw = true;
    }

    /// Claim the read of the highlighted file, or `None`: only while the
    /// `notes` view shows, and only when the body held is another file's or
    /// from an older generation. One file at a time, whatever the cursor
    /// passed over since the last tick. A folded inspector still reads: the
    /// reader beside the tree shows the body (FZ-62). A new file starts the
    /// reader at its top; a re-read of the same one keeps its place.
    pub fn claim_note_read(&mut self) -> Option<NoteRead> {
        if !self.notes_wanted() {
            return None;
        }
        let path = self.notes_highlighted()?;
        let root = self
            .selected_notes()
            .map(|(p, _)| std::path::PathBuf::from(shellexpand::tilde(p).as_ref()))?;
        let generation = self.notes.generation;
        let held = self.notes.body.as_ref();
        if held.is_some_and(|b| b.path == path && b.generation == generation) {
            return None;
        }
        if held.is_none_or(|b| b.path != path) {
            self.notes.reader_scroll = 0;
        }
        let render_width = self.markdown_render_width();
        self.notes.body = Some(NoteBody {
            path: path.clone(),
            generation,
            text: None,
            facts: None,
            last_commit: None,
            rendered: None,
            render_asked: render_width,
        });
        Some(NoteRead {
            path,
            root,
            generation,
            render_width,
        })
    }

    /// The width to render the highlighted file at: the reader's last
    /// drawn width, when the file is markdown and the reader has drawn.
    /// Rendered whatever `notes.reader_raw` says, so `p` flips at once.
    fn markdown_render_width(&self) -> Option<u16> {
        let width = self.drawn.reader_width.get();
        let markdown = self
            .notes_highlighted_file()
            .is_some_and(|f| f.kind().is_markdown());
        (markdown && width > 0).then_some(width)
    }

    /// Claim a render of the held markdown body at the reader's current
    /// width, or `None`: only in the `notes` layout, once the body has
    /// landed, when no render at that width is held or in flight. The read
    /// renders at the width it was claimed with; this follows a resize or a
    /// fold, and a body read before the reader first drew.
    pub fn claim_note_render(&mut self) -> Option<NoteRender> {
        if !self.notes_layout() {
            return None;
        }
        let width = self.markdown_render_width()?;
        let path = self.notes_highlighted()?;
        let body = self.notes.body.as_mut().filter(|b| b.path == path)?;
        let Some(Ok(text)) = &body.text else {
            return None;
        };
        let held = body.rendered.as_ref().map(|r| r.width);
        if held == Some(width) || body.render_asked == Some(width) {
            return None;
        }
        body.render_asked = Some(width);
        Some(NoteRender {
            path,
            generation: body.generation,
            width,
            text: text.clone(),
        })
    }

    /// Land a render (`AppEvent::NoteRead` or `AppEvent::NoteRendered`) on
    /// the body it was made from; one for a file or a generation no longer
    /// held is dropped.
    pub fn apply_note_rendered(
        &mut self,
        path: &std::path::Path,
        generation: u64,
        rendered: RenderedNote,
    ) {
        if let Some(body) = &mut self.notes.body
            && body.path == path
            && body.generation == generation
        {
            if body.render_asked == Some(rendered.width) {
                body.render_asked = None;
            }
            body.rendered = Some(rendered);
            self.needs_redraw = true;
        }
    }

    /// `p` in the `notes` layout: flip the reader between rendered markdown
    /// and its source, for every markdown file this session.
    pub fn toggle_reader_raw(&mut self) {
        if !self.notes_layout() {
            return;
        }
        self.notes.reader_raw = !self.notes.reader_raw;
        self.needs_redraw = true;
    }

    /// Land a read's metadata beside its body (`AppEvent::NoteRead`), on the
    /// same terms as `apply_note_read`.
    pub fn apply_note_facts(&mut self, read: &NoteRead, facts: crate::notes::NoteFacts) {
        if let Some(body) = self.held_note_mut(read) {
            body.facts = Some(facts);
            self.needs_redraw = true;
        }
    }

    /// Land git's answer for a read's last commit (`AppEvent::NoteCommit`),
    /// which follows the body; one for a file or a generation no longer
    /// wanted is dropped.
    pub fn apply_note_commit(&mut self, read: &NoteRead, last: crate::notes::LastCommit) {
        if let Some(body) = self.held_note_mut(read) {
            body.last_commit = Some(last);
            self.needs_redraw = true;
        }
    }

    /// The held body `read` was claimed for, if it still is that file's and
    /// generation's.
    fn held_note_mut(&mut self, read: &NoteRead) -> Option<&mut NoteBody> {
        self.notes
            .body
            .as_mut()
            .filter(|b| b.path == read.path && b.generation == read.generation)
    }

    /// Whether the main area takes the `notes` layout (FZ-62): pane 2 at
    /// full height split into tree and reader, the facts beside its header,
    /// and no inspector under pane 1 (FZ-68). Not on
    /// a section header, where pane 2 is the digest and the usual layout
    /// returns. Read by `ui::panel_rects`, so render and hit-testing agree.
    pub fn notes_layout(&self) -> bool {
        self.notes_wanted() && self.on_header().is_none()
    }

    /// Scroll the reader by `delta` lines, clamped to what the last frame
    /// measured.
    pub fn scroll_reader(&mut self, delta: i32) {
        let max = i32::from(self.drawn.reader_max_scroll.get());
        self.notes.reader_scroll =
            (i32::from(self.notes.reader_scroll) + delta).clamp(0, max) as u16;
        self.needs_redraw = true;
    }

    /// Whether `H`/`L` page the reader: in the `notes` layout with the tree
    /// or the reader focused (Andreas, 2026-10-29). Plain `h`/`l` stay free
    /// in the tree for its folds (FZ-58).
    pub fn reader_pages(&self) -> bool {
        self.notes_layout()
            && matches!(
                self.focused_panel,
                FocusedPanel::Detail | FocusedPanel::Reader
            )
    }

    /// `H` (`-1`) or `L` (`1`): scroll the reader one page of the height its
    /// last frame drew, focus unmoved.
    pub fn page_reader(&mut self, pages: i32) {
        let page = i32::from(self.drawn.reader_view_height.get().max(1));
        self.scroll_reader(pages * page);
    }

    /// Land a body read (`AppEvent::NoteRead`); one for a file or a
    /// generation no longer wanted is dropped.
    pub fn apply_note_read(&mut self, read: NoteRead, text: Result<String, String>) {
        if let Some(body) = &mut self.notes.body
            && body.path == read.path
            && body.generation == read.generation
        {
            body.text = Some(text);
            self.needs_redraw = true;
        }
    }

    /// A sweep landed or a suspend returned: every held walk and the body
    /// are read again the next time they are shown.
    pub fn invalidate_notes(&mut self) {
        self.notes.generation += 1;
        self.needs_redraw = true;
    }

    /// `m` in the `notes` view: list every markdown file, or the notes
    /// directories only. The cursor stays on its file when the other tree
    /// lists it, else goes to the first.
    pub fn toggle_notes_markdown(&mut self) {
        if !self.notes_wanted() {
            return;
        }
        let held = self.notes_highlighted();
        self.notes.all_markdown = !self.notes.all_markdown;
        let root = self
            .selected_notes()
            .map(|(p, _)| std::path::PathBuf::from(shellexpand::tilde(p).as_ref()));
        let at = self
            .notes_listed()
            .iter()
            .position(|f| root.as_ref().map(|r| r.join(&f.rel)) == held)
            .unwrap_or(0);
        self.set_detail_selected(at);
        self.inspector_scroll = 0;
        self.needs_redraw = true;
    }

    /// One wheel event over the `notes` tree: step its cursor a file, and
    /// let the renderer bring it into view with the scrolloff margin; past
    /// either end, scroll the header or the tail into view instead (FZ-65).
    ///
    /// Not `detail_nav_down`/`up`: their re-entry measures the cursor
    /// against the last frame, and his terminal sends two events a notch
    /// before the next frame is drawn. The first stepped the cursor past the
    /// margin the old frame kept, the second re-entered it back, and the
    /// notch moved nothing.
    pub fn wheel_notes_tree(&mut self, down: bool) {
        match down {
            true if self.detail_selected() + 1 < self.selected_commit_count() => {
                self.select_commit_down()
            }
            false if self.detail_selected() > 0 => self.select_commit_up(),
            true => self.scroll_detail(1),
            false => self.scroll_detail(-1),
        }
    }
}
