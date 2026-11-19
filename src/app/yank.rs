//! The yank (`y` chords) and its copy flash: `YankState` holds the pending
//! chord and the flash; the types, helpers and `App` methods build what a
//! yank copies and say so on the status line.
//!
//! Moved out of `app.rs` unchanged but for the field paths (FZ-123). The
//! methods stay on `App`: they read the focused pane, the views, the
//! selection and the status line. Design: `docs/app-split-design.md`,
//! "Owner types", D5 and D7, owner item O-E.

use super::{App, CiBody, FocusedPanel, HistoryView, View};
use crate::model::{RepoLocation, RepoStatus};

/// The yank's state, moved off `App` unchanged (FZ-123).
pub(crate) struct YankState {
    /// A `y` was pressed and the next key finishes the yank chord (FZ-74):
    /// `y` copies the thing under the cursor, `p` its path, anything else
    /// copies nothing. Cleared by the next key whatever it is.
    pub(crate) pending: bool,
    /// The row a successful `yy` or `yp` copied from flashes once (FZ-81).
    pub(crate) flash: YankFlash,
}

impl YankState {
    pub(crate) fn new() -> Self {
        Self {
            pending: false,
            flash: YankFlash::default(),
        }
    }
}

/// Which half of the yank chord (FZ-74): `yy` copies the thing under the
/// cursor, `yp` its path or address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YankKind {
    Thing,
    Path,
}

/// How long the copy flash (FZ-81) takes to sweep over the cursor row: `yy`
/// from the left edge, `yp` from the right. notez's timing.
pub const YANK_SWEEP: std::time::Duration = std::time::Duration::from_millis(100);
/// How long the swept row then takes to fade back to the band.
pub const YANK_FADE: std::time::Duration = std::time::Duration::from_millis(200);
/// The whole copy flash: the sweep, then the fade.
pub const YANK_FLASH: std::time::Duration = YANK_SWEEP.saturating_add(YANK_FADE);
/// The longest the event loop waits while the flash is live: about 8
/// frames over the sweep.
pub const YANK_FRAME: std::time::Duration = std::time::Duration::from_millis(12);

/// Which row a copy flash belongs to: the pane, pane 2's view and the cursor
/// there. The flash paints only while the app still points at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlashSource {
    pub pane: FocusedPanel,
    pub view: HistoryView,
    pub row: usize,
}

/// The one flash after a successful `yy` or `yp` (FZ-81), as notez draws
/// it: a timestamp read at draw time, no thread and no timer task. While it
/// lasts the event loop cuts its wait to `YankFlash::wait` and redraws
/// when `YankFlash::tick` says so.
#[derive(Debug, Default, Clone, Copy)]
pub struct YankFlash {
    start: Option<(std::time::Instant, FlashSource)>,
    from_right: bool,
}

impl YankFlash {
    /// Start the flash at `now` on `source`, sweeping from the right edge
    /// when `from_right`.
    pub fn show(&mut self, source: FlashSource, from_right: bool, now: std::time::Instant) {
        self.start = Some((now, source));
        self.from_right = from_right;
    }

    pub fn clear(&mut self) {
        self.start = None;
    }

    /// Whether a flash is started and not yet ended by `tick` or `clear`.
    pub fn is_active(&self) -> bool {
        self.start.is_some()
    }

    /// The row the flash belongs to, while there is one.
    pub fn source(&self) -> Option<FlashSource> {
        self.start.map(|(_, source)| source)
    }

    /// Time into a live flash at `now`, or `None` once it is over.
    fn elapsed(&self, now: std::time::Instant) -> Option<std::time::Duration> {
        let elapsed = now.saturating_duration_since(self.start?.0);
        (elapsed < YANK_FLASH).then_some(elapsed)
    }

    /// The background at `now` of cell `col` of a flashed row `width` cells
    /// wide: `YANKED_BG` where the sweep has reached and the band
    /// (`SELECTED_BG`) ahead of it, then the whole row fading from
    /// `YANKED_BG` back to the band; the band before a copy and after.
    pub fn cell_bg(&self, now: std::time::Instant, col: u16, width: u16) -> ratatui::style::Color {
        use crate::ui::colors::{SELECTED_BG, YANKED_BG, mix};
        let Some(elapsed) = self.elapsed(now) else {
            return SELECTED_BG;
        };
        if elapsed >= YANK_SWEEP {
            let faded = (elapsed - YANK_SWEEP).as_secs_f64() / YANK_FADE.as_secs_f64();
            return mix(YANKED_BG, SELECTED_BG, faded);
        }
        let reached = elapsed.as_secs_f64() / YANK_SWEEP.as_secs_f64();
        let reached = ((reached * f64::from(width)).floor() as u16).min(width);
        let swept = match self.from_right {
            true => col >= width - reached,
            false => col < reached,
        };
        if swept { YANKED_BG } else { SELECTED_BG }
    }

    /// True when the view should redraw at `now`: the flash is live, or its
    /// time just ran out, which ends it.
    pub fn tick(&mut self, now: std::time::Instant) -> bool {
        if self.start.is_none() {
            return false;
        }
        if self.elapsed(now).is_none() {
            self.start = None;
        }
        true
    }

    /// `wait`, or no longer than the next frame while the flash lasts, or no
    /// wait at all once its time is up but `tick` has not ended it yet, so a
    /// late frame does not leave the row a shade off.
    pub fn wait(&self, wait: std::time::Duration, now: std::time::Instant) -> std::time::Duration {
        match self.elapsed(now) {
            Some(elapsed) => wait.min(YANK_FRAME).min(YANK_FLASH - elapsed),
            None if self.start.is_some() => std::time::Duration::ZERO,
            None => wait,
        }
    }
}

/// How the status line names what a yank copied: a short value by itself
/// (a hash, a branch, a run id), a url or a path by kind, a body by size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum YankWord {
    Value,
    Url,
    Path,
    Size,
}

/// The repo's page on GitHub, `https://github.com/<owner>/<repo>`: the
/// carrier's owner and name as GitHub spells them, else an `origin` on
/// github.com. `None` for a row with neither.
fn github_web_url(repo: &RepoStatus) -> Option<String> {
    let origin = repo
        .remote_url
        .as_deref()
        .and_then(crate::model::RemoteRef::parse);
    let (owner, name) = match (&repo.owner, &repo.location) {
        (Some(owner), RepoLocation::LocalOnly) => (owner.clone(), origin?.name),
        (Some(owner), _) => (owner.clone(), repo.name.clone()),
        (None, _) => {
            let origin = origin.filter(|o| o.host.eq_ignore_ascii_case("github.com"))?;
            (origin.owner, origin.name)
        }
    };
    Some(format!("https://github.com/{owner}/{name}"))
}

/// A note's text as the reader reads it, up to `notes::BODY_CAP`, without
/// the reader's cut marker, which is not the file's. `None` for a file that
/// cannot be read as text.
fn note_text(path: &std::path::Path) -> Option<String> {
    let body = crate::notes::read_body(path).ok()?;
    let marker = format!("\n… cut at {} KiB", crate::notes::BODY_CAP / 1024);
    Some(match body.strip_suffix(&marker) {
        Some(text) => text.to_string(),
        None => body,
    })
}

impl App {
    /// `y`: copy the inspected commit's hash through `copy` and say so on the
    /// status line. `copy` is the clipboard, passed in so a test can stand in
    /// for it.
    pub fn yank_with(&mut self, copy: impl FnOnce(&str) -> Result<(), String>) {
        let Some(hash) = self.inspected_hash().map(str::to_string) else {
            self.set_status("nothing to copy".to_string(), false);
            return;
        };
        self.copy_hash(&hash, copy);
    }

    /// Feed one key press to the yank chord (FZ-74) and say whether the
    /// chord took it. `key` is the character pressed, `None` for any key
    /// that is not a plain character. A `y` arms the chord; the next key
    /// finishes it: `y` copies the thing under the cursor, `p` its path,
    /// anything else ends it, copies nothing and is not taken, so it does
    /// what it does on its own. On a commit list in pane 2 the first `y`
    /// already copies the hash, as `y` always did there, and still arms
    /// the chord.
    pub fn press_yank_key(
        &mut self,
        key: Option<char>,
        copy: impl FnOnce(&str) -> Result<(), String>,
    ) -> bool {
        if std::mem::take(&mut self.yank.pending) {
            let kind = match key {
                Some('y') => YankKind::Thing,
                Some('p') => YankKind::Path,
                _ => return false,
            };
            self.yank(kind, copy);
            return true;
        }
        if key != Some('y') {
            return false;
        }
        self.yank.pending = true;
        if self.on_commit_list() {
            self.yank_with(copy);
        } else if self.on_changed_file() {
            self.yank_change_path(copy);
        }
        true
    }

    /// Whether pane 2 has focus on a list of commits, where a plain `y`
    /// copies the hash at once.
    fn on_commit_list(&self) -> bool {
        self.view == View::Detail
            && self.focused_panel == FocusedPanel::Detail
            && self.on_header().is_none()
            && matches!(
                self.history_view,
                HistoryView::Info | HistoryView::Commits | HistoryView::Reflog | HistoryView::Graph
            )
    }

    /// Whether pane 2 or the inspector has focus on the `changes` view,
    /// where a plain `y` copies the highlighted file's path at once (FZ-60),
    /// as it copies the hash on a commit list.
    fn on_changed_file(&self) -> bool {
        self.changes_wanted()
            && matches!(
                self.focused_panel,
                FocusedPanel::Detail | FocusedPanel::Inspector
            )
    }

    /// `yy` or `yp`: copy what `kind` names under the focused cursor through
    /// `copy` and say so on the status line, `nothing to copy` where there
    /// is nothing (`yank_target`).
    /// A successful copy starts the row's flash (FZ-81), `yp` sweeping from
    /// the right; a failed one and `nothing to copy` end any flash.
    pub fn yank(&mut self, kind: YankKind, copy: impl FnOnce(&str) -> Result<(), String>) {
        self.yank.flash.clear();
        let Some((text, word)) = self.yank_target(kind) else {
            self.set_status("nothing to copy".to_string(), false);
            return;
        };
        let said = match word {
            YankWord::Value => format!("copied {text}"),
            YankWord::Url => "copied url".to_string(),
            YankWord::Path => "copied path".to_string(),
            // Bytes under a KB, where `0.0 KB` would read as empty.
            YankWord::Size => match text.len() {
                n if n < 1024 => format!("copied {n} B"),
                n => format!("copied {:.1} KB", n as f64 / 1024.0),
            },
        };
        match copy(&text) {
            Ok(()) => {
                self.set_status(said, false);
                let from_right = kind == YankKind::Path;
                let source = self.flash_source();
                self.yank
                    .flash
                    .show(source, from_right, std::time::Instant::now());
            }
            Err(reason) => self.set_status(format!("copy failed: {reason}"), true),
        }
    }

    /// The row a copy made now would flash (FZ-81): the focused pane, pane
    /// 2's view and the cursor there. The inspector and the reader have no
    /// cursor row; their flash is on their border label.
    pub fn flash_source(&self) -> FlashSource {
        let row = match self.focused_panel {
            FocusedPanel::RepoList => self.selected,
            FocusedPanel::Detail if self.history_view == HistoryView::Graph => {
                usize::from(self.graph_cursor)
            }
            FocusedPanel::Detail => self.detail_selected(),
            _ => 0,
        };
        FlashSource {
            pane: self.focused_panel,
            view: self.history_view,
            row,
        }
    }

    /// What `yy` (`Thing`) or `yp` (`Path`) copies from the focused pane,
    /// and the word the status line says it in. Nothing on the compact
    /// grid, the strip, a section header or a row with nothing to name.
    fn yank_target(&self, kind: YankKind) -> Option<(String, YankWord)> {
        use YankKind::{Path, Thing};
        if self.view != View::Detail || self.on_header().is_some() {
            return None;
        }
        let repo = self.selected_repo()?;
        match (self.focused_panel, self.history_view) {
            (FocusedPanel::Contributions, _) => None,
            (FocusedPanel::RepoList, _) => match kind {
                Thing => Some((github_web_url(repo)?, YankWord::Url)),
                Path => Some((
                    shellexpand::tilde(repo.path.as_deref()?).into_owned(),
                    YankWord::Path,
                )),
            },
            (_, HistoryView::Notes) => {
                let path = self.notes_highlighted()?;
                match kind {
                    Thing => Some((note_text(&path)?, YankWord::Size)),
                    Path => Some((path.to_string_lossy().into_owned(), YankWord::Path)),
                }
            }
            (FocusedPanel::Reader, _) => None,
            // A changed file's path in the repo and its absolute path
            // (FZ-60), from pane 2 or the inspector showing its diff.
            (_, HistoryView::Changes) => {
                let (_, file) = self.changes_highlighted()?;
                match kind {
                    Thing => Some((file.path.clone(), YankWord::Value)),
                    Path => Some((
                        self.change_to_open()?.to_string_lossy().into_owned(),
                        YankWord::Path,
                    )),
                }
            }
            (FocusedPanel::Detail, HistoryView::Branches) => {
                let (name, on_remote) = self.highlighted_branch(repo)?;
                match kind {
                    Thing => Some((name.to_string(), YankWord::Value)),
                    Path => {
                        let branch = match on_remote {
                            true => name.split_once('/').map_or(name, |(_, b)| b),
                            false => name,
                        };
                        let url = github_web_url(repo)?;
                        Some((format!("{url}/tree/{branch}"), YankWord::Url))
                    }
                }
            }
            // A worktree's branch and its directory; a detached one has no
            // branch to name (FZ-77 on FZ-74).
            (FocusedPanel::Detail, HistoryView::Worktrees) => {
                let wt = self.worktree_highlighted()?;
                match kind {
                    Thing => (!wt.branch.is_empty()).then_some((wt.branch, YankWord::Value)),
                    Path => Some((shellexpand::tilde(&wt.path).into_owned(), YankWord::Path)),
                }
            }
            (FocusedPanel::Detail, HistoryView::Ci) => {
                let run = self.ci_highlighted_run()?;
                match kind {
                    Thing => Some((run.url.clone(), YankWord::Url)),
                    Path => Some((run.id.to_string(), YankWord::Value)),
                }
            }
            (FocusedPanel::Detail, _) => {
                let hash = self.inspected_hash()?;
                match kind {
                    Thing => Some((hash.to_string(), YankWord::Value)),
                    Path => {
                        let url = github_web_url(repo)?;
                        Some((format!("{url}/commit/{hash}"), YankWord::Url))
                    }
                }
            }
            (FocusedPanel::Inspector, HistoryView::Ci) => {
                if !self.inspects_ci_run() {
                    return None;
                }
                match kind {
                    Thing => Some((self.ci_shown_text()?, YankWord::Size)),
                    Path => Some((self.ci_highlighted_run()?.url.clone(), YankWord::Url)),
                }
            }
            (FocusedPanel::Inspector, _) => match kind {
                Thing => {
                    let commit = self.inspected_commit()?;
                    let message = match commit.body.trim().is_empty() {
                        true => commit.message.clone(),
                        false => format!("{}\n\n{}", commit.message, commit.body.trim_end()),
                    };
                    Some((message, YankWord::Size))
                }
                Path => Some((self.inspected_hash()?.to_string(), YankWord::Value)),
            },
        }
    }

    /// The `branches` view's highlighted entry: its name as the view lists
    /// it, and whether it is a remote-tracking branch. Locals come first.
    fn highlighted_branch<'a>(&self, repo: &'a RepoStatus) -> Option<(&'a str, bool)> {
        let locals = repo.branches.len();
        let last = (locals + repo.remote_branches.len()).checked_sub(1)?;
        let i = self.detail_selected().min(last);
        match i < locals {
            true => repo.branches.get(i).map(|b| (b.name.as_str(), false)),
            false => repo
                .remote_branches
                .get(i - locals)
                .map(|r| (r.name.as_str(), true)),
        }
    }

    /// The text of the run body the inspector shows: the marked job's log
    /// tail in `jobs`, its whole held log in `log`, the workflow file in
    /// `script`. `None` while the body holds no text.
    fn ci_shown_text(&self) -> Option<String> {
        let run = self.ci_highlighted_run()?;
        let state = self.selected_ci()?.1?;
        match self.ci.body {
            CiBody::Jobs => match state.logs.get(&self.ci_marked_job()?.id)? {
                crate::ci::LogTail::Lines(tail) => Some(tail.join("\n")),
                crate::ci::LogTail::Unavailable(_) => None,
            },
            CiBody::Log => Some(crate::ui::ci::shown_log(self)?.1.join("\n")),
            CiBody::Script => {
                let key = (run.head_sha.clone()?, run.path.clone()?);
                match state.scripts.get(&key)? {
                    crate::ci::Detail::Ready(text) => Some(text.clone()),
                    _ => None,
                }
            }
        }
    }

    /// Copy `hash` through `copy` and say so on the status line, in the
    /// words `y` uses. A click passes the hash it saw drawn.
    pub fn copy_hash(&mut self, hash: &str, copy: impl FnOnce(&str) -> Result<(), String>) {
        match copy(hash) {
            Ok(()) => self.set_status(format!("copied {hash}"), false),
            Err(reason) => self.set_status(format!("copy failed: {reason}"), true),
        }
    }
}

#[cfg(test)]
mod tests;
