use super::{CellSize, ContributionsMode};
use crate::config::{Icons, View};
use crate::time::{TimeWindow, WINDOW_N_MIN, WindowUnit};

/// Overlay shown on top of the main view.
#[derive(Debug, Clone)]
pub enum Overlay {
    None,
    Help,
    Branches(BranchOverlay),
    TimeWindow(TimeWindowOverlay),
    Settings(SettingsOverlay),
}

/// When a setting takes effect once applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applies {
    /// Mutates app state directly; visible on the next frame or tick.
    Live,
    /// Only consumed while building the view at startup, so the saved value
    /// waits for the next launch. Shown in the overlay so a field that looks
    /// inert is understood rather than reported as a bug.
    Restart,
}

/// One editable row in the settings overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingField {
    RefreshInterval,
    GithubPollInterval,
    Watch,
    View,
    WorkStart,
    WorkEnd,
    PersistPanes,
    Contributions,
    ContribCellSize,
    Icons,
    Intro,
}

/// The settings rows in display order.
pub const SETTING_FIELDS: [SettingField; 11] = [
    SettingField::RefreshInterval,
    SettingField::GithubPollInterval,
    SettingField::Watch,
    SettingField::View,
    SettingField::WorkStart,
    SettingField::WorkEnd,
    SettingField::PersistPanes,
    SettingField::Contributions,
    SettingField::ContribCellSize,
    SettingField::Icons,
    SettingField::Intro,
];

impl SettingField {
    pub fn label(self) -> &'static str {
        match self {
            Self::RefreshInterval => "sweep interval",
            Self::GithubPollInterval => "github poll",
            Self::Watch => "start watching",
            Self::View => "startup view",
            Self::WorkStart => "work starts",
            Self::WorkEnd => "work ends",
            Self::PersistPanes => "remember pane sizes",
            Self::Contributions => "contributions",
            Self::ContribCellSize => "contrib cell size",
            Self::Icons => "icons",
            Self::Intro => "loading screen",
        }
    }

    /// The `config.toml` key this row writes to.
    pub fn config_key(self) -> &'static str {
        match self {
            Self::RefreshInterval => "refresh_interval",
            Self::GithubPollInterval => "github_poll_interval",
            Self::Watch => "watch",
            Self::View => "view",
            Self::WorkStart => "work_start",
            Self::WorkEnd => "work_end",
            Self::PersistPanes => "persist_pane_sizes",
            Self::Contributions => "contributions",
            Self::ContribCellSize => "contributions_cell_size",
            Self::Icons => "icons",
            Self::Intro => "intro",
        }
    }

    pub fn applies(self) -> Applies {
        match self {
            Self::RefreshInterval
            | Self::GithubPollInterval
            | Self::View
            | Self::PersistPanes
            | Self::Contributions
            | Self::ContribCellSize
            | Self::Icons => Applies::Live,
            // Only read when the app starts; `w` is how you change it now.
            Self::Watch => Applies::Restart,
            // The loading screen only covers the first load of a launch. By
            // the time this row can be reached it has already been and gone,
            // so there is nothing for a change to take effect on until next
            // time.
            Self::Intro => Applies::Restart,
            // Both only feed `time::` when the cutoff and header are computed,
            // which happens once per load against the `Config` the main loop
            // owns by reference.
            Self::WorkStart | Self::WorkEnd => Applies::Restart,
        }
    }

    /// True for rows that accept typed digits.
    fn numeric(self) -> bool {
        matches!(
            self,
            Self::RefreshInterval | Self::GithubPollInterval | Self::WorkStart | Self::WorkEnd
        )
    }
}

/// Bounds for `refresh_interval`, in seconds. The floor keeps a stray keypress
/// from turning the dashboard into a fetch loop.
const INTERVAL_MIN: u64 = 5;
const INTERVAL_MAX: u64 = 3600;
const INTERVAL_STEP: u64 = 5;

/// Floor for the GitHub poll, mirroring `main::GITHUB_POLL_MIN`. Lower than
/// the sweep floor because a poll is one GraphQL call, not a fetch per repo.
const POLL_MIN: u64 = 15;

fn bool_label(v: bool) -> String {
    if v { "yes" } else { "no" }.to_string()
}

/// Editable copy of the settings, live until Enter writes them back.
#[derive(Debug, Clone)]
pub struct SettingsOverlay {
    pub cursor: usize,
    pub refresh_interval: u64,
    pub github_poll_interval: u64,
    pub watch: bool,
    /// None means "auto": decide the view from the terminal size.
    pub view: Option<View>,
    pub work_start: u8,
    pub work_end: u8,
    pub persist_panes: bool,
    pub contributions: ContributionsMode,
    pub contrib_cell: CellSize,
    /// Seeded from the set the app is drawing with, so a save that only
    /// meant to change another row writes back the set already in force.
    pub icons: Icons,
    /// The set in force when the overlay opened, or when it last saved: what
    /// Esc puts back, because stepping the icons row previews at once.
    pub icons_in_force: Icons,
    pub intro: bool,
    /// Result of the last save attempt, shown as a footer line.
    pub status: Option<String>,
    /// Rolling buffer of typed digits, so "3" then "0" reads as 30.
    digits: String,
}

impl SettingsOverlay {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        refresh_interval: u64,
        github_poll_interval: u64,
        watch: bool,
        view: Option<View>,
        work_start: u8,
        work_end: u8,
        persist_panes: bool,
        contributions: ContributionsMode,
        contrib_cell: CellSize,
        intro: bool,
        icons: Icons,
    ) -> Self {
        Self {
            cursor: 0,
            refresh_interval: refresh_interval.clamp(INTERVAL_MIN, INTERVAL_MAX),
            github_poll_interval: github_poll_interval.clamp(POLL_MIN, INTERVAL_MAX),
            watch,
            view,
            work_start,
            work_end,
            persist_panes,
            contributions,
            contrib_cell,
            icons,
            icons_in_force: icons,
            intro,
            status: None,
            digits: String::new(),
        }
    }

    pub fn field(&self) -> SettingField {
        SETTING_FIELDS[self.cursor.min(SETTING_FIELDS.len() - 1)]
    }

    pub fn move_cursor(&mut self, delta: i32) {
        self.digits.clear();
        self.status = None;
        let len = SETTING_FIELDS.len() as i32;
        let next = (self.cursor as i32 + delta).rem_euclid(len);
        self.cursor = next as usize;
    }

    /// Nudge the focused field. `delta` is -1 or 1; enum-valued fields cycle.
    pub fn step(&mut self, delta: i32) {
        self.digits.clear();
        self.status = None;
        match self.field() {
            SettingField::RefreshInterval => {
                let next = self.refresh_interval as i64 + delta as i64 * INTERVAL_STEP as i64;
                self.refresh_interval = next.clamp(INTERVAL_MIN as i64, INTERVAL_MAX as i64) as u64;
            }
            SettingField::GithubPollInterval => {
                let next = self.github_poll_interval as i64 + delta as i64 * INTERVAL_STEP as i64;
                self.github_poll_interval = next.clamp(POLL_MIN as i64, INTERVAL_MAX as i64) as u64;
            }
            SettingField::Watch => self.watch = !self.watch,
            SettingField::View => {
                // auto -> detail -> compact -> auto
                self.view = match (self.view, delta > 0) {
                    (None, true) => Some(View::Detail),
                    (Some(View::Detail), true) => Some(View::Compact),
                    (Some(View::Compact), true) => None,
                    (None, false) => Some(View::Compact),
                    (Some(View::Compact), false) => Some(View::Detail),
                    (Some(View::Detail), false) => None,
                };
            }
            // Kept strictly ordered as they are edited, so the pair can never
            // reach a state where the work slot is empty or inverted.
            SettingField::WorkStart => {
                let next = self.work_start as i32 + delta;
                self.work_start = next.clamp(0, self.work_end as i32 - 1) as u8;
            }
            SettingField::WorkEnd => {
                let next = self.work_end as i32 + delta;
                self.work_end = next.clamp(self.work_start as i32 + 1, 23) as u8;
            }
            SettingField::PersistPanes => self.persist_panes = !self.persist_panes,
            SettingField::Intro => self.intro = !self.intro,
            SettingField::ContribCellSize => self.contrib_cell = self.contrib_cell.step(delta),
            SettingField::Icons => {
                self.icons = match self.icons {
                    Icons::Nerd => Icons::Plain,
                    Icons::Plain => Icons::Nerd,
                }
            }
            // off -> github -> local -> off
            SettingField::Contributions => {
                self.contributions = match (self.contributions, delta > 0) {
                    (ContributionsMode::Off, true) => ContributionsMode::Github,
                    (ContributionsMode::Github, true) => ContributionsMode::Local,
                    (ContributionsMode::Local, true) => ContributionsMode::Off,
                    (ContributionsMode::Off, false) => ContributionsMode::Local,
                    (ContributionsMode::Github, false) => ContributionsMode::Off,
                    (ContributionsMode::Local, false) => ContributionsMode::Github,
                };
            }
        }
    }

    /// Type a digit into the focused numeric field. Non-numeric fields ignore
    /// it rather than swallowing the key.
    pub fn push_digit(&mut self, d: u8) {
        let field = self.field();
        if !field.numeric() {
            return;
        }
        self.status = None;
        self.digits.push((b'0' + d) as char);
        // Keep only what fits the field's range, so typing past it restarts
        // rather than saturating at the maximum.
        let parsed: u64 = self.digits.parse().unwrap_or(0);
        match field {
            SettingField::RefreshInterval => {
                if parsed > INTERVAL_MAX {
                    self.digits = ((b'0' + d) as char).to_string();
                    self.refresh_interval = d as u64;
                } else {
                    self.refresh_interval = parsed;
                }
            }
            SettingField::GithubPollInterval => {
                if parsed > INTERVAL_MAX {
                    self.digits = ((b'0' + d) as char).to_string();
                    self.github_poll_interval = d as u64;
                } else {
                    self.github_poll_interval = parsed;
                }
            }
            SettingField::WorkStart => {
                let capped = (self.work_end as u64).saturating_sub(1);
                if parsed > capped {
                    self.digits = ((b'0' + d) as char).to_string();
                    self.work_start = (d as u64).min(capped) as u8;
                } else {
                    self.work_start = parsed as u8;
                }
            }
            SettingField::WorkEnd => {
                if parsed > 23 || parsed <= self.work_start as u64 {
                    self.digits = ((b'0' + d) as char).to_string();
                    self.work_end = (d as u64).clamp(self.work_start as u64 + 1, 23) as u8;
                } else {
                    self.work_end = parsed as u8;
                }
            }
            _ => {}
        }
    }

    /// Clamp anything a digit entry could have left below its floor. Called
    /// before applying, since `push_digit` lets a value pass through an
    /// intermediate state (typing "30" goes through "3").
    pub fn normalize(&mut self) {
        self.refresh_interval = self.refresh_interval.clamp(INTERVAL_MIN, INTERVAL_MAX);
        self.github_poll_interval = self.github_poll_interval.clamp(POLL_MIN, INTERVAL_MAX);
        self.work_end = self.work_end.clamp(1, 23);
        self.work_start = self.work_start.min(self.work_end - 1);
    }

    /// The value each row writes to `config.toml`, in field order.
    pub fn config_updates(&self) -> Vec<(&'static str, crate::config::SettingValue)> {
        use crate::config::SettingValue;
        vec![
            (
                SettingField::RefreshInterval.config_key(),
                SettingValue::Int(self.refresh_interval as i64),
            ),
            (
                SettingField::GithubPollInterval.config_key(),
                SettingValue::Int(self.github_poll_interval as i64),
            ),
            (
                SettingField::Watch.config_key(),
                SettingValue::Bool(self.watch),
            ),
            (
                SettingField::View.config_key(),
                SettingValue::Str(match self.view {
                    None => "auto",
                    Some(View::Detail) => "detail",
                    Some(View::Compact) => "compact",
                }),
            ),
            (
                SettingField::Contributions.config_key(),
                SettingValue::Str(self.contributions.config_value()),
            ),
            (
                SettingField::ContribCellSize.config_key(),
                SettingValue::Str(self.contrib_cell.config_value()),
            ),
            (
                SettingField::Icons.config_key(),
                SettingValue::Str(self.icons.config_value()),
            ),
            (
                SettingField::Intro.config_key(),
                SettingValue::Bool(self.intro),
            ),
            (
                SettingField::WorkStart.config_key(),
                SettingValue::Int(self.work_start as i64),
            ),
            (
                SettingField::WorkEnd.config_key(),
                SettingValue::Int(self.work_end as i64),
            ),
            (
                SettingField::PersistPanes.config_key(),
                SettingValue::Bool(self.persist_panes),
            ),
        ]
    }

    /// How the focused field's value renders in the overlay.
    pub fn value_label(&self, field: SettingField) -> String {
        match field {
            SettingField::RefreshInterval => format!("{}s", self.refresh_interval),
            SettingField::GithubPollInterval => format!("{}s", self.github_poll_interval),
            SettingField::Watch => bool_label(self.watch),
            SettingField::View => match self.view {
                None => "auto".to_string(),
                Some(View::Detail) => "detail".to_string(),
                Some(View::Compact) => "compact".to_string(),
            },
            SettingField::WorkStart => format!("{:02}:00", self.work_start),
            SettingField::WorkEnd => format!("{:02}:00", self.work_end),
            SettingField::PersistPanes => bool_label(self.persist_panes),
            SettingField::Contributions => self.contributions.label().to_string(),
            SettingField::ContribCellSize => self.contrib_cell.label().to_string(),
            SettingField::Icons => self.icons.config_value().to_string(),
            SettingField::Intro => bool_label(self.intro),
        }
    }
}

#[cfg(test)]
mod settings_intro_tests;

#[cfg(test)]
mod settings_icons_tests;

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

#[cfg(test)]
mod tests;
