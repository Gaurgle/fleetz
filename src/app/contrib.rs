//! The contribution strip (`app.contrib`): which source it shows, the GitHub
//! calendar from the last poll, the local source's author filter, the day's
//! icon size, the day cursor and an in-progress pan, and the `App` methods
//! that open, walk and scroll it. Not the domain model, which is
//! `crate::contributions` (`Calendar`, `WEEKS`); this is the front end's
//! state over it. The scroll and the visible width stay in `drawn`, the fold
//! flag on `App`.
//!
//! Moved out of `app.rs` unchanged but for the field paths (FZ-122). The
//! methods stay on `App`: they read the fold flag, `drawn` and the visible
//! repos. Design: `docs/app-split-design.md`, "Owner types" (the per-view
//! row), D5 and D7, owner item O-E.

use super::App;

/// The strip's state, moved off `App` unchanged (FZ-122).
pub(crate) struct ContribStrip {
    /// Which source the contribution strip shows. Never `Off` here: the
    /// strip is always on screen, and the setting's "off" folds it instead
    /// (`apply_contributions_setting`).
    pub(crate) mode: ContributionsMode,
    /// The account-wide calendar from the last GitHub poll. None until the
    /// first poll lands, or when the payload predates the field.
    pub(crate) calendar: Option<Box<crate::contributions::Calendar>>,
    /// Git author regex the local contribution source filters by, from the
    /// `contributions_author` config key. None resolves each repo's own
    /// `user.email` instead. Lives on `App` rather than being read from
    /// `Config` at each use, because the sweep is started from call sites
    /// that hold an `App` but not a `Config`.
    pub(crate) author: Option<String>,
    /// Which square icon the strip draws a day as, from the
    /// `contributions_cell_size` config key and the settings row.
    pub(crate) cell: CellSize,
    /// Grid slot the day cursor sits on. The strip stands on today once it
    /// is opened or unfolded (#66), so the title reads today's count. None
    /// remains only for an `App` whose strip was never shown: there the first
    /// move seeds the cursor on today, and the title falls back to
    /// `ui::contributions::period_total`.
    pub(crate) cursor: Option<usize>,
    /// Screen column the in-progress contribution-strip pan is measured from,
    /// or None when no drag is underway. Separate from `dragging`, which owns
    /// pane resizing: a pan moves content inside a pane rather than moving
    /// the boundary between two.
    pub(crate) drag: Option<u16>,
}

impl ContribStrip {
    pub(crate) fn new() -> Self {
        Self {
            mode: ContributionsMode::Github,
            calendar: None,
            author: None,
            cell: CellSize::default(),
            cursor: None,
            drag: None,
        }
    }
}

/// Which source the contribution strip shows. Persisted to `config.toml` as
/// one key, where `Off` starts the strip folded: the strip itself is always on
/// screen, so at runtime `App::contrib.mode` is never `Off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContributionsMode {
    #[default]
    Off,
    Github,
    Local,
}

/// What the strip does when `config.toml` says nothing: show GitHub's own
/// calendar. Explicit `contributions = "off"` starts it folded.
pub const DEFAULT_CONTRIBUTIONS: &str = "github";

impl ContributionsMode {
    pub fn from_config(value: &str) -> Self {
        match value {
            "github" => Self::Github,
            "local" => Self::Local,
            _ => Self::Off,
        }
    }

    pub fn config_value(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Github => "github",
            Self::Local => "local",
        }
    }

    /// Label for the settings row and the pane title.
    pub fn label(self) -> &'static str {
        self.config_value()
    }
}

/// Which Nerd Font square the contribution strip draws a day as. Three
/// icons of measured size rather than one icon scaled: a terminal cannot
/// scale a glyph, so "size" can only mean "a different glyph". Persisted as
/// `contributions_cell_size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl CellSize {
    pub fn from_config(value: &str) -> Self {
        match value {
            "small" => Self::Small,
            "large" => Self::Large,
            _ => Self::Medium,
        }
    }

    pub fn config_value(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    /// Label for the settings row.
    pub fn label(self) -> &'static str {
        self.config_value()
    }

    /// The icon, from JetBrainsMono Nerd Font's glyph boxes: `nf-fa-stop` at
    /// 693 units, `nf-md-square` at 750 and `nf-fa-square` at 808, all
    /// centred on the same line as a capital letter. Measured, not eyeballed:
    /// the three looked interchangeable in a list and are clearly stepped on
    /// screen.
    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Small => "\u{f04d}",
            Self::Medium => "\u{f0764}",
            Self::Large => "\u{f0c8}",
        }
    }

    /// small -> medium -> large, wrapping; `delta` is -1 or 1.
    pub fn step(self, delta: i32) -> Self {
        match (self, delta > 0) {
            (Self::Small, true) => Self::Medium,
            (Self::Medium, true) => Self::Large,
            (Self::Large, true) => Self::Small,
            (Self::Small, false) => Self::Large,
            (Self::Medium, false) => Self::Small,
            (Self::Large, false) => Self::Medium,
        }
    }
}

impl App {
    /// Advance the GitHub calendar to the local date, called on every tick
    /// with the clock. The poll stamps the calendar's `today`, so without
    /// this the strip drew yesterday as its last live square after midnight
    /// until the next poll (#78). When the grid shifts by whole weeks, the
    /// cursor and the scroll follow so they stay on the same dates. A view
    /// pinned to the newest edge stays pinned, or the new Sunday's column
    /// would open just off the right edge.
    pub fn observe_day(&mut self, today: chrono::NaiveDate) {
        let Some(calendar) = self.contrib.calendar.as_deref_mut() else {
            return;
        };
        let before = calendar.start();
        if !calendar.advance_to(today) {
            return;
        }
        let weeks = ((calendar.start() - before).num_days() / 7) as usize;
        if weeks > 0 {
            self.contrib.cursor = self.contrib.cursor.map(|c| c.saturating_sub(7 * weeks));
            let newest_edge = crate::contributions::WEEKS
                .saturating_sub(self.drawn.contrib_visible_weeks.get().max(1));
            let scroll = self.drawn.contrib_scroll.get();
            if scroll < newest_edge {
                self.drawn.contrib_scroll.set(scroll.saturating_sub(weeks));
            }
        }
        self.needs_redraw = true;
    }

    /// Apply the `contributions` setting, at launch or when the settings
    /// overlay saves it. The strip is always on screen, so "off" folds it
    /// rather than removing it, and keeps whichever source it is on.
    pub fn apply_contributions_setting(&mut self, mode: ContributionsMode) {
        match mode {
            ContributionsMode::Off => self.contributions_folded = true,
            mode => self.open_contributions(mode, chrono::Local::now().date_naive()),
        }
        self.needs_redraw = true;
    }

    /// `c`: switch which source fills the strip. Only while it is unfolded:
    /// a folded strip shows neither source, so there is nothing to switch.
    pub fn cycle_contributions_source(&mut self) {
        if self.contributions_folded {
            return;
        }
        let next = match self.contrib.mode {
            ContributionsMode::Github => ContributionsMode::Local,
            ContributionsMode::Local | ContributionsMode::Off => ContributionsMode::Github,
        };
        self.open_contributions(next, chrono::Local::now().date_naive());
        self.needs_redraw = true;
    }

    /// Show the strip unfolded on `mode`, standing on `today` with the grid
    /// scrolled to the newest weeks, so the first look reads today's count
    /// and the first arrow key is a real step (#66).
    ///
    /// `today` is a parameter so tests can pin the date; the public callers
    /// read the local clock, as the renderer does.
    pub(super) fn open_contributions(&mut self, mode: ContributionsMode, today: chrono::NaiveDate) {
        self.contrib.mode = mode;
        self.contributions_folded = false;
        self.contrib.cursor = Some(self.contrib_last_slot(today));
        // Out of range on purpose, exactly as at startup: the renderer is the
        // only place the visible width is known, and it clamps this down to
        // "newest week at the right edge". Computing it here from the last
        // measured width would anchor on the oldest weeks the first time,
        // before any frame has been drawn.
        self.drawn.contrib_scroll.set(crate::contributions::WEEKS);
    }

    /// The last selectable grid slot for `today`: the grid stops at today,
    /// not at the end of today's week.
    pub fn contrib_last_slot(&self, today: chrono::NaiveDate) -> usize {
        crate::contributions::Calendar::new(today, crate::contributions::Source::Local).today_slot()
    }

    /// Move the day cursor `dweeks` columns and `ddays` rows, which is how
    /// the grid actually reads: a week is a column, a weekday is a row.
    ///
    /// Opening or unfolding the strip already stands on today. A cursor can
    /// still be unset on an `App` whose strip was never shown; there the
    /// first move seeds it on today rather than jumping from nowhere.
    /// Both axes clamp independently, so walking up a column never spills
    /// into the neighbouring week, and the final column stops at today.
    pub fn move_contrib_cursor(&mut self, dweeks: i32, ddays: i32, today: chrono::NaiveDate) {
        let last = self.contrib_last_slot(today);
        let (last_week, last_weekday) = (last / 7, last % 7);

        let Some(current) = self.contrib.cursor else {
            // First keypress only marks today; it does not also move.
            self.contrib.cursor = Some(last);
            self.scroll_cursor_into_view(last_week);
            self.needs_redraw = true;
            return;
        };

        let week = ((current / 7) as i32 + dweeks).clamp(0, last_week as i32) as usize;
        // The newest column is short: it ends on today's weekday.
        let max_weekday = if week == last_week { last_weekday } else { 6 };
        let weekday = ((current % 7) as i32 + ddays).clamp(0, max_weekday as i32) as usize;

        self.contrib.cursor = Some(week * 7 + weekday);
        self.scroll_cursor_into_view(week);
        self.needs_redraw = true;
    }

    /// Jump the cursor to the oldest or newest column, keeping its weekday.
    pub fn jump_contrib_cursor(&mut self, to_end: bool, today: chrono::NaiveDate) {
        let last = self.contrib_last_slot(today);
        let weekday = self.contrib.cursor.map_or(last % 7, |c| c % 7);
        let week = if to_end { last / 7 } else { 0 };
        let max_weekday = if week == last / 7 { last % 7 } else { 6 };
        self.contrib.cursor = Some(week * 7 + weekday.min(max_weekday));
        self.scroll_cursor_into_view(week);
        self.needs_redraw = true;
    }

    /// Scroll the grid the minimum amount that brings `week` back on screen.
    fn scroll_cursor_into_view(&self, week: usize) {
        let visible = self.drawn.contrib_visible_weeks.get().max(1);
        let first = self.drawn.contrib_scroll.get();
        if week < first {
            self.drawn.contrib_scroll.set(week);
        } else if week >= first + visible {
            self.drawn.contrib_scroll.set(week + 1 - visible);
        }
    }

    /// Pan the grid by a drag from screen column `from` to `to`, and report
    /// the anchor the next sample should measure against.
    ///
    /// The content follows the hand, the way grabbing a map works: dragging
    /// right pulls older weeks in from the left. The returned anchor lags
    /// `to` by whatever part of the drag did not add up to a whole week, so
    /// a slow drag accumulates instead of rounding to zero on every sample
    /// and never moving at all.
    pub fn pan_contributions(&mut self, from: u16, to: u16) -> u16 {
        let weeks = crate::ui::contributions::weeks_dragged(to as i32 - from as i32);
        if weeks == 0 {
            return from;
        }
        self.scroll_contributions(-weeks);
        let consumed = crate::ui::contributions::cols_for_weeks(weeks);
        (from as i32 + consumed).max(0) as u16
    }

    /// Scroll the grid by whole weeks without moving the cursor.
    pub fn scroll_contributions(&mut self, delta: i32) {
        let visible = self.drawn.contrib_visible_weeks.get().max(1);
        let max = crate::contributions::WEEKS.saturating_sub(visible);
        let next = (self.drawn.contrib_scroll.get() as i32 + delta).clamp(0, max as i32) as usize;
        self.drawn.contrib_scroll.set(next);
        self.needs_redraw = true;
    }

    /// The local calendar: every visible repo's histogram summed. Pure
    /// arithmetic over data the sweep already gathered, so this is safe to
    /// call on every frame and every keystroke.
    pub fn local_calendar(&self, today: chrono::NaiveDate) -> crate::contributions::Calendar {
        let mut calendar =
            crate::contributions::Calendar::new(today, crate::contributions::Source::Local);
        for (_, repo) in self.visible_repos() {
            if let Some(daily) = &repo.daily {
                calendar.add_daily(daily);
            }
        }
        calendar
    }
}

#[cfg(test)]
mod tests;
