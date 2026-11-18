//! The slot digest and the repo list's section stops (FZ-45): the window,
//! the digest cursor, the held `git log --since` reads, the work hours, the
//! pending hover, and the `App` methods that walk, land and read them
//! (`app.digest`). Not the domain model, which is `crate::digest` (sections,
//! windows, `build`); this is the front end's state over it.
//!
//! Moved out of `app.rs` unchanged but for the field paths (FZ-121). The
//! methods stay on `App`: each reads the repo list, the view, the focused
//! panel or the inspector's scroll. Design: `docs/app-split-design.md`,
//! "Owner types" (the per-view row), D5 and D7, owner item O-E.

use super::{App, FocusedPanel, View};
use crate::model::RepoStatus;
use chrono::{DateTime, Local};

/// The digest's state and the list's section stops, moved off `App`
/// unchanged (FZ-121).
pub(crate) struct DigestView {
    /// The section header the list cursor stands on (FZ-45), lifted off the
    /// section's first repo, which `selected` keeps pointing at. Read only
    /// through `on_header`, which also checks the header is drawn.
    pub(crate) list_stop: Option<crate::digest::Section>,
    /// The window the digest counts over, cycled by `t` on a header.
    pub(crate) window: crate::digest::DigestWindow,
    /// The digest line pane 2's cursor is on.
    pub(crate) selected: usize,
    /// The newest `git log --since` read per row path, or the one running
    /// (`digest::HeldLog`). One per path, so it stays the size of the list.
    pub(crate) logs: std::collections::HashMap<String, crate::digest::HeldLog>,
    /// The work hours slot boundaries are cut at, copied from `Config` at
    /// startup as the GitHub poll interval is.
    pub(crate) work_hours: (u8, u8),
    /// The section header under the resting pointer and since when; the
    /// tick lands on it after `HOVER_DWELL` (`settle_hover`).
    pub(crate) hover_pending: Option<(crate::digest::Section, std::time::Instant)>,
}

impl DigestView {
    pub(crate) fn new() -> Self {
        Self {
            list_stop: None,
            window: crate::digest::DigestWindow::default(),
            selected: 0,
            logs: std::collections::HashMap::new(),
            work_hours: (9, 18),
            hover_pending: None,
        }
    }
}

/// How long the pointer rests on a section header before the cursor lands
/// there: the CI view's dwell, the pause that tells a rest from a pass.
pub const HOVER_DWELL: std::time::Duration = crate::ci::DWELL;

/// The repo list's drawn rows as the renderer left them: the list area and
/// the index of `App::list_rows` at its top.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListHits {
    pub area: ratatui::layout::Rect,
    pub offset: usize,
}

impl ListHits {
    /// The `App::list_rows` index drawn at (`col`, `row`), if any. Every list
    /// row is one line high.
    pub fn at(&self, col: u16, row: u16) -> Option<usize> {
        let a = self.area;
        (col >= a.x && col < a.x + a.width && row >= a.y && row < a.y + a.height)
            .then(|| self.offset + (row - a.y) as usize)
    }
}

/// One row the repo list draws, in drawn order (`App::list_rows`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListRow {
    /// A section header, a cursor stop in the detail view (FZ-45).
    Header(crate::digest::Section),
    /// The "results" header a free-text query draws; not a stop.
    Results,
    /// The blank row between two sections; not a stop.
    Gap,
    /// A repo, by its position in `App::visible_repos`.
    Repo(usize),
}

/// Section header stops and the slot digest (FZ-45). The header rows are a
/// row kind of `list_rows`, built from `visible_repos`, so the cursor, the
/// drawing and the hit-testing walk one list.
impl App {
    /// Every row the repo list draws, in drawn order. A free-text query draws
    /// one ranked list under "results"; otherwise the sections, each headed
    /// once another section is present, the second and third set off by a
    /// blank row.
    pub fn list_rows(&self) -> Vec<ListRow> {
        use crate::digest::Section;
        let visible = self.visible_repos();
        if self.has_text_query() {
            let mut rows = vec![ListRow::Results];
            rows.extend((0..visible.len()).map(ListRow::Repo));
            return rows;
        }
        let mixed = visible
            .iter()
            .any(|(_, r)| Section::of(r) != Section::GithubActivity);
        let mut rows = Vec::new();
        let mut current = None;
        for (pos, (_, repo)) in visible.iter().enumerate() {
            let section = Section::of(repo);
            if current != Some(section) {
                current = Some(section);
                if section != Section::GithubActivity {
                    if !rows.is_empty() {
                        rows.push(ListRow::Gap);
                    }
                    rows.push(ListRow::Header(section));
                } else if mixed {
                    rows.push(ListRow::Header(section));
                }
            }
            rows.push(ListRow::Repo(pos));
        }
        rows
    }

    /// Whether the list cursor can stand on `row`. Headers only in the detail
    /// view, whose pane 2 draws their digest; the compact grid has none.
    pub fn is_stop(&self, row: ListRow) -> bool {
        match row {
            ListRow::Repo(_) => true,
            ListRow::Header(_) => self.view == View::Detail,
            ListRow::Results | ListRow::Gap => false,
        }
    }

    /// The section whose header the cursor stands on. The stop is lifted off
    /// the section's first repo, so anything that moved `selected` since (a
    /// search, a reload, a group) has also left the header.
    pub fn on_header(&self) -> Option<crate::digest::Section> {
        let section = self.digest.list_stop?;
        if self.view != View::Detail {
            return None;
        }
        let rows = self.list_rows();
        let at = rows.iter().position(|r| *r == ListRow::Header(section))?;
        (rows.get(at + 1) == Some(&ListRow::Repo(self.selected))).then_some(section)
    }

    /// The `list_rows` index the cursor is drawn on.
    pub fn cursor_row(&self, rows: &[ListRow]) -> Option<usize> {
        let at = match self.on_header() {
            Some(section) => ListRow::Header(section),
            None => ListRow::Repo(self.selected),
        };
        rows.iter().position(|r| *r == at)
    }

    /// Step the cursor `delta` stops through `list_rows`. False when the
    /// cursor is on no row, so the caller's own clamp applies.
    pub(super) fn step_list(&mut self, delta: i32) -> bool {
        let rows = self.list_rows();
        let Some(current) = self.cursor_row(&rows) else {
            return false;
        };
        let stops: Vec<usize> = (0..rows.len()).filter(|&i| self.is_stop(rows[i])).collect();
        let Some(at) = stops.iter().position(|&i| i == current) else {
            return false;
        };
        let next = (at as i32 + delta).clamp(0, stops.len() as i32 - 1) as usize;
        if next != at {
            self.land_on(rows[stops[next]]);
        }
        true
    }

    /// Put the cursor on `row`. Landing on a header keeps pane 2's view and
    /// its place, so stepping back onto the repo restores them; a different
    /// repo starts its detail from the top, as `move_down` does.
    pub fn land_on(&mut self, row: ListRow) {
        // A cursor moved by a key or a click outranks a pending hover.
        self.digest.hover_pending = None;
        match row {
            ListRow::Header(section) => {
                if self.on_header() == Some(section) || !self.is_stop(row) {
                    return;
                }
                let rows = self.list_rows();
                let Some(at) = rows.iter().position(|r| *r == row) else {
                    return;
                };
                let Some(ListRow::Repo(first)) = rows.get(at + 1).copied() else {
                    return;
                };
                if self.selected != first {
                    self.selected = first;
                    self.reset_detail_view();
                }
                self.digest.list_stop = Some(section);
                self.digest.selected = 0;
                self.inspector_scroll = 0;
                // On a header the `notes` layout gives way to the digest and
                // no reader is drawn; focus cannot stay on it (FZ-62).
                if self.focused_panel == FocusedPanel::Reader {
                    self.focused_panel = FocusedPanel::Detail;
                }
            }
            ListRow::Repo(pos) => {
                self.digest.list_stop = None;
                if self.selected != pos {
                    self.selected = pos;
                    self.reset_detail_view();
                }
                // Off a header onto a repo in `notes` the layout returns and
                // docks no inspector; focus cannot stay on it (FZ-68).
                if self.focused_panel == FocusedPanel::Inspector && !self.shows_inspector() {
                    self.focused_panel = FocusedPanel::Detail;
                }
            }
            ListRow::Results | ListRow::Gap => return,
        }
        self.needs_redraw = true;
    }

    /// The list row drawn at (`col`, `row`) by the last frame, if a stop.
    /// Only the detail view draws the list `list_hits` records; the compact
    /// grid never writes it, so there a stale detail frame must not answer.
    fn list_stop_at(&self, col: u16, row: u16) -> Option<ListRow> {
        if self.view != View::Detail {
            return None;
        }
        let i = self.drawn.list_hits.get().at(col, row)?;
        let hit = self.list_rows().get(i).copied()?;
        self.is_stop(hit).then_some(hit)
    }

    /// A click in the repo list lands the cursor on the row under it.
    pub fn click_list(&mut self, col: u16, row: u16) {
        if let Some(hit) = self.list_stop_at(col, row) {
            self.land_on(hit);
        }
    }

    /// The pointer moved to (`col`, `row`) at `now`. Over a section header it
    /// starts, or keeps, the dwell `settle_hover` lands on; anywhere else it
    /// cancels it, so a pointer passing over a header on its way elsewhere
    /// lands nowhere.
    pub fn hover_list(&mut self, col: u16, row: u16, now: std::time::Instant) {
        self.digest.hover_pending = match self.list_stop_at(col, row) {
            Some(ListRow::Header(section)) => match self.digest.hover_pending {
                Some((held, since)) if held == section => Some((held, since)),
                _ => Some((section, now)),
            },
            _ => None,
        };
    }

    /// The pointer left the repo list: no hover is pending.
    pub fn clear_hover(&mut self) {
        self.digest.hover_pending = None;
    }

    /// On the tick: land on the header the pointer has rested on for
    /// `HOVER_DWELL`.
    pub fn settle_hover(&mut self, now: std::time::Instant) {
        if let Some((section, since)) = self.digest.hover_pending
            && now.saturating_duration_since(since) >= HOVER_DWELL
        {
            self.land_on(ListRow::Header(section));
            self.digest.hover_pending = None;
        }
    }

    /// GitHub's push time for the remote `repo` carries, from the dataset
    /// (`carries`, the predicate the poll follows).
    fn pushed_at(&self, repo: &RepoStatus) -> Option<DateTime<chrono::Utc>> {
        self.dataset
            .iter()
            .find(|e| crate::engine::carries(repo, e))
            .map(|e| e.pushed_at)
    }

    /// The digest of the header the cursor stands on, at `now`.
    pub fn digest(&self, now: DateTime<Local>) -> Option<crate::digest::Digest> {
        use crate::digest::{Row, Section, build, needs_log};
        let section = self.on_header()?;
        let (start, end) = self.digest.work_hours;
        let (cutoff, label) = self.digest.window.bounds(now, start, end);
        let visible = self.visible_repos();
        let rows: Vec<Row> = visible
            .iter()
            .filter(|(_, r)| Section::of(r) == section)
            .map(|(index, repo)| {
                let pushed_at = self.pushed_at(repo);
                let log = needs_log(section, repo, pushed_at, cutoff).and_then(|key| {
                    let held = self.digest.logs.get(repo.path.as_deref()?)?;
                    if held.key == key {
                        held.commits.as_deref()
                    } else {
                        None
                    }
                });
                Row {
                    index: *index,
                    repo,
                    pushed_at,
                    log,
                }
            })
            .collect();
        Some(build(section, self.digest.window, cutoff, label, &rows))
    }

    /// The `git log --since` reads the shown digest needs and nothing holds
    /// or runs under the same key: one per touched repo per cutoff and refs,
    /// so a sweep that moves no ref runs none. Marked running here, so the
    /// next tick does not ask again.
    pub fn claim_digest_logs(&mut self, now: DateTime<Local>) -> Vec<crate::digest::LogRequest> {
        use crate::digest::{HeldLog, LogRequest, Section, needs_log};
        let Some(section) = self.on_header() else {
            return Vec::new();
        };
        let (start, end) = self.digest.work_hours;
        let (cutoff, _) = self.digest.window.bounds(now, start, end);
        let wanted: Vec<(String, crate::digest::LogKey)> = self
            .visible_repos()
            .iter()
            .filter(|(_, r)| Section::of(r) == section)
            .filter_map(|(_, repo)| {
                let key = needs_log(section, repo, self.pushed_at(repo), cutoff)?;
                let path = repo.path.clone()?;
                let held = self.digest.logs.get(&path).is_some_and(|h| h.key == key);
                (!held).then_some((path, key))
            })
            .collect();
        wanted
            .into_iter()
            .map(|(path, key)| {
                self.digest.logs.insert(
                    path.clone(),
                    HeldLog {
                        key: key.clone(),
                        commits: None,
                    },
                );
                LogRequest {
                    dir: std::path::PathBuf::from(shellexpand::tilde(&path).to_string()),
                    path,
                    key,
                }
            })
            .collect()
    }

    /// Land a `git log --since` read. One read under a key the row has since
    /// left (a newer window, moved refs) is dropped.
    pub fn apply_digest_log(
        &mut self,
        path: &str,
        key: &crate::digest::LogKey,
        commits: Vec<crate::model::Commit>,
    ) {
        if let Some(held) = self.digest.logs.get_mut(path)
            && held.key == *key
        {
            held.commits = Some(commits);
            self.needs_redraw = true;
        }
    }

    /// The digest line pane 2's cursor is on, clamped to the lines there are.
    pub fn digest_line(&self, now: DateTime<Local>) -> Option<crate::digest::DigestLine> {
        let digest = self.digest(now)?;
        let last = digest.lines.len().checked_sub(1)?;
        digest.lines.into_iter().nth(self.digest.selected.min(last))
    }

    /// Move pane 2's digest cursor.
    pub fn digest_nav(&mut self, delta: i32) {
        let Some(digest) = self.digest(Local::now()) else {
            return;
        };
        let last = digest.lines.len().saturating_sub(1) as i32;
        let next = (self.digest.selected.min(last as usize) as i32 + delta).clamp(0, last) as usize;
        if next != self.digest.selected {
            self.digest.selected = next;
            self.inspector_scroll = 0;
        }
        self.needs_redraw = true;
    }

    /// Scroll the inspector's digest line detail. Not `scroll_inspector`,
    /// which in the `ci` view scrolls a run body the digest hides.
    pub fn scroll_digest_inspector(&mut self, delta: i32) {
        let max = self.drawn.inspector_max_scroll.get() as i32;
        self.inspector_scroll = (self.inspector_scroll as i32 + delta).clamp(0, max) as u16;
        self.needs_redraw = true;
    }

    /// `J`/`K` on a header: drive the next pane as `scroll_next_panel` does,
    /// but pane 2 and the inspector are the digest's, so their hidden repo
    /// views keep the place a return to the repo restores.
    pub fn digest_scroll_next_panel(&mut self, delta: i32) {
        match self.shift_scroll_target() {
            Some(FocusedPanel::Detail) => self.digest_nav(delta),
            Some(FocusedPanel::Inspector) => self.scroll_digest_inspector(delta),
            Some(FocusedPanel::RepoList) => self.scroll_next_panel(delta),
            // No reader on a header: the `notes` layout gives way there.
            Some(FocusedPanel::Contributions | FocusedPanel::Reader) | None => {}
        }
    }

    /// `t` on a header: the next window.
    pub fn cycle_digest_window(&mut self) {
        self.digest.window = self.digest.window.next();
        self.digest.selected = 0;
        self.inspector_scroll = 0;
        self.needs_redraw = true;
    }

    /// `Enter` on a header: land on the repo of the digest line under pane
    /// 2's cursor, in pane 1.
    pub fn jump_to_digest_line(&mut self, now: DateTime<Local>) {
        let Some(line) = self.digest_line(now) else {
            return;
        };
        let Some(pos) = self
            .visible_repos()
            .iter()
            .position(|(i, _)| *i == line.repo)
        else {
            return;
        };
        self.land_on(ListRow::Repo(pos));
        self.focus_panel(FocusedPanel::RepoList);
    }
}

/// FZ-45: the digest's `git log --since` reads are claimed once per touched
/// repo per cutoff and refs, and a read under a key the row has left is
/// dropped.
#[cfg(test)]
mod tests;
