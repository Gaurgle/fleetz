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
    /// The contribution strip. Unlike the other three it exists in Compact
    /// view too, and only while the pane is open, so both `focus_panel` and
    /// `toggle_panel_focus` special-case it.
    Contributions,
}

/// Which rendering of a repo's history the pane body is showing.
///
/// `Info` is the reference view -- branches, worktrees, stashes -- and the
/// default, so the pane opens on the facts you glance at rather than on a
/// list you have to read. The other three are history, each dense because it
/// does one job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistoryView {
    #[default]
    Info,
    Commits,
    Reflog,
    Graph,
}

impl HistoryView {
    /// Every view, in the order the tab strip draws them and `V` cycles them.
    pub const ALL: [Self; 4] = [Self::Info, Self::Commits, Self::Reflog, Self::Graph];
    pub const COUNT: usize = Self::ALL.len();

    /// Index into the per-view scroll and selection arrays.
    pub fn index(self) -> usize {
        match self {
            Self::Info => 0,
            Self::Commits => 1,
            Self::Reflog => 2,
            Self::Graph => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Commits => "commits",
            Self::Reflog => "reflog",
            Self::Graph => "graph",
        }
    }
}

/// Whether the contribution strip is showing, and from which source.
/// Persisted to `config.toml` as one key, so the source survives a restart
/// along with the open state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContributionsMode {
    #[default]
    Off,
    Github,
    Local,
}

/// What the strip does when `config.toml` says nothing: show GitHub's own
/// calendar. Explicit `contributions = "off"` still closes it.
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

    pub fn is_open(self) -> bool {
        self != Self::Off
    }
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

/// A search-bar query split into its three parts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedQuery {
    /// The free-text portion, fuzzy-matched against repo names.
    pub text: String,
    /// Quick filters activated by `#tag` words.
    pub filters: QuickFilters,
    /// `#tag` words that matched no known filter, kept so the strip can flag
    /// them instead of letting a typo silently narrow nothing.
    pub unknown_tags: Vec<String>,
}

/// Split a search-bar query into free text, quick filters, and unrecognized
/// tags.
///
/// A `#name` word is always consumed as filter syntax and never falls back to
/// literal text, so tags stay unambiguous; an unrecognized one is reported in
/// `unknown_tags` rather than dropped on the floor. Prefix-matched against
/// `FILTER_TAGS`, so `#d` alone resolves to Dirty.
///
/// A bare `#` is deliberately inert. It used to activate every filter, which
/// AND'd to an almost always empty list, and since `#` is the first keystroke
/// of every tag the list blanked and repopulated on each one.
///
/// Filters live in the same text as the search query rather than on number
/// keys, so they combine the same way and never occupy a keybinding a
/// panel-focus jump could use instead.
pub fn parse_query(query: &str) -> ParsedQuery {
    let mut parsed = ParsedQuery::default();
    let mut text_words = Vec::new();

    for word in query.split_whitespace() {
        let Some(name) = word.strip_prefix('#') else {
            text_words.push(word);
            continue;
        };
        let name = name.to_lowercase();
        if name.is_empty() {
            continue;
        }
        let mut matched = false;
        for &(kind, tag) in &FILTER_TAGS {
            if tag.starts_with(&name) {
                set_filter(&mut parsed.filters, kind);
                matched = true;
            }
        }
        if !matched {
            parsed.unknown_tags.push(name);
        }
    }

    parsed.text = text_words.join(" ");
    parsed
}

/// Step a percentage by `delta` resize steps, clamped to `[min, max]`.
fn step_pct(current: u16, delta: i16, min: u16, max: u16) -> u16 {
    let next = current as i32 + delta as i32 * RESIZE_STEP as i32;
    next.clamp(min as i32, max as i32) as u16
}

/// Which section of the repo list a repo belongs to, as a sort key: github
/// activity, then local changes, then also active.
///
/// "local changes" outranks "also active" because it is a worklist -- work
/// sitting on this machine and nowhere else -- while "also active" is repos
/// you happen to have a copy of. `render_repo_list` draws them in this order
/// too; the two must not disagree.
fn section_rank(repo: &RepoStatus) -> u8 {
    match repo.location {
        RepoLocation::Primary => 0,
        RepoLocation::LocalOnly => 1,
        RepoLocation::Elsewhere | RepoLocation::NotCloned => 2,
    }
}

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
    Intro,
}

/// The settings rows in display order.
pub const SETTING_FIELDS: [SettingField; 9] = [
    SettingField::RefreshInterval,
    SettingField::GithubPollInterval,
    SettingField::Watch,
    SettingField::View,
    SettingField::WorkStart,
    SettingField::WorkEnd,
    SettingField::PersistPanes,
    SettingField::Contributions,
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
            Self::Intro => "intro animation",
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
            Self::Intro => "intro",
        }
    }

    pub fn applies(self) -> Applies {
        match self {
            Self::RefreshInterval
            | Self::GithubPollInterval
            | Self::View
            | Self::PersistPanes
            | Self::Contributions => Applies::Live,
            // Only read when the app starts; `w` is how you change it now.
            Self::Watch => Applies::Restart,
            // The wave plays once, on the first screen of a launch. By the
            // time this row can be reached it has already been and gone, so
            // there is nothing for a change to take effect on until next time.
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

/// Repo list width as a percentage of the main area, and the inspector's share
/// of the right column. These are the sizes every launch starts from.
pub const DEFAULT_REPO_LIST_PCT: u16 = 35;
pub const DEFAULT_INSPECTOR_PCT: u16 = 40;

/// Bounds the resize keys clamp to. Both panes stay wide/tall enough to be
/// worth drawing, so a held key can never collapse one to nothing.
pub const REPO_LIST_PCT_MIN: u16 = 20;
pub const REPO_LIST_PCT_MAX: u16 = 60;
pub const INSPECTOR_PCT_MIN: u16 = 15;
pub const INSPECTOR_PCT_MAX: u16 = 70;

/// Percentage points one resize keypress moves a pane edge.
const RESIZE_STEP: u16 = 5;

/// Ticks per pending-CI blink phase. Expressed in ticks rather than
/// milliseconds so the blink stays at its own ~500ms half-period when the tick
/// rate changes to speed up the spinner. Keep this at `500ms / TICK_INTERVAL`.
pub const BLINK_TICKS: usize = 5;

/// Rows of context the detail pane keeps past its cursor at either edge.
///
/// The cursor walks freely through the middle of the pane and parks this far
/// from the edge, from where the text scrolls under it instead. Four is what
/// vim's `scrolloff` and less's default both settle near: enough to see where
/// you are going, little enough that the cursor still travels.
///
/// Applied in every history view, not only the graph. The margin collapses at
/// the ends of the content, where there is nothing left to keep in reserve.
pub const SCROLLOFF: u16 = 4;

/// The scrolloff margin for a pane of `height` rows: never more than half of
/// it, or the two margins overlap and the cursor is pinned to the middle of a
/// short pane instead of walking it.
pub fn scrolloff_for(height: u16) -> u16 {
    SCROLLOFF.min(height.saturating_sub(1) / 2)
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
        intro: bool,
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
            SettingField::Intro => bool_label(self.intro),
        }
    }
}

#[cfg(test)]
mod settings_intro_tests {
    use super::*;

    fn overlay(intro: bool) -> SettingsOverlay {
        SettingsOverlay::new(
            60,
            30,
            true,
            None,
            9,
            18,
            false,
            ContributionsMode::Off,
            intro,
        )
    }

    /// The row exists, is a toggle, and writes the key the loader reads. A row
    /// whose `config_key` did not match `Config`'s field would save happily and
    /// change nothing.
    #[test]
    fn the_intro_row_toggles_and_saves_under_the_key_config_reads() {
        assert!(SETTING_FIELDS.contains(&SettingField::Intro));
        assert_eq!(SettingField::Intro.config_key(), "intro");

        let mut so = overlay(true);
        so.cursor = SETTING_FIELDS
            .iter()
            .position(|f| *f == SettingField::Intro)
            .expect("the intro row");
        assert_eq!(so.value_label(SettingField::Intro), bool_label(true));
        so.step(1);
        assert!(!so.intro, "the row did not toggle");
        assert_eq!(so.value_label(SettingField::Intro), bool_label(false));

        let saved = so
            .config_updates()
            .into_iter()
            .find(|(key, _)| *key == "intro")
            .expect("intro is not written to the file");
        assert!(
            matches!(saved.1, crate::config::SettingValue::Bool(false)),
            "the saved value is not the toggled one"
        );
    }

    /// It is marked restart-only, and honestly so: the wave plays on the first
    /// screen of a launch, so by the time this row can be reached it has
    /// already been and gone.
    #[test]
    fn the_intro_row_says_it_needs_a_restart() {
        assert_eq!(SettingField::Intro.applies(), Applies::Restart);
    }

    /// Every row's key round-trips through a real save and load, which is the
    /// only thing that proves the overlay and the config agree about names.
    #[test]
    fn toggling_the_row_off_survives_a_save_and_load() {
        let dir = std::env::temp_dir().join(format!("fleetz-intro-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("config.toml");

        let mut so = overlay(true);
        so.intro = false;
        crate::config::save_settings(&path, &so.config_updates()).expect("save");
        let loaded = crate::config::Config::load_from(&path).expect("load");
        assert!(!loaded.intro, "the toggle did not survive the round trip");

        let _ = std::fs::remove_dir_all(&dir);
    }
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
    pub watch_mode: bool,
    pub refresh_interval: u64,
    pub last_refreshed: Option<DateTime<Local>>,
    pub group_names: Vec<String>,
    pub active_group: Option<usize>,
    pub focused_panel: FocusedPanel,
    /// Which rendering of history the repo pane's body is showing.
    ///
    /// The pane used to stack branches, commits, the graph, the reflog,
    /// worktrees and stashes into one scrolling paragraph, all competing for
    /// a single offset: reaching the graph meant scrolling past the commits.
    /// They are four answers to different questions, so they take turns
    /// instead of queueing.
    pub history_view: HistoryView,
    /// Scroll offset per history view, indexed by `HistoryView::index`.
    ///
    /// Per view rather than shared, because a shared offset is wrong the
    /// moment you switch back to something you had already scrolled.
    ///
    /// `Cell` for the same reason `contrib_scroll` is one: the render pass is
    /// the only place that knows which screen row the selection landed on, so
    /// it is the only place that can scroll to keep it visible. Walking a
    /// cursor through two hundred rows the pane never followed is what made
    /// the view look frozen.
    pub detail_scrolls: [std::cell::Cell<u16>; HistoryView::COUNT],
    /// Index of the row highlighted in each history view, whose commit the
    /// inspector shows. Reset to 0 when the selected repo changes.
    ///
    /// Per view for the same reason as the scroll offset, and because the
    /// lists have different lengths: an index valid in the reflog is
    /// meaningless against the commit list.
    pub detail_selections: [usize; HistoryView::COUNT],
    /// Scroll offset for the inspector pane (long commit bodies).
    pub inspector_scroll: u16,
    /// Largest useful scroll offset for each pane, measured by the renderer,
    /// which is the only place content length and pane height are both known.
    /// The scroll handlers clamp against these instead of a fixed cap.
    ///
    /// `Cell` because rendering takes `&App`: this is the render pass reporting
    /// a measurement back, not state the input layer owns. Both start at 0 and
    /// are written on the first frame, which is always drawn before any input
    /// is processed.
    pub detail_max_scroll: std::cell::Cell<u16>,
    /// Set when the cursor moves, cleared once the renderer has scrolled to
    /// show it.
    ///
    /// Without it the renderer re-centred on the selection every frame, which
    /// fought free scrolling to a standstill: `k` decremented the offset and
    /// the next frame put it straight back, so the pane looked frozen. The
    /// pane should chase the cursor when the cursor moves, and otherwise go
    /// where it is told.
    pub follow_selection: std::cell::Cell<bool>,
    /// Body row of every selectable entry in the *active* history view, in
    /// list order, recorded by the renderer.
    ///
    /// The graph's entries are its commit blocks, whose height depends on the
    /// message, the pane width and the wrap; the other views' are their list
    /// rows, which sit under however many sections the repo happened to draw.
    /// Either way the render pass is the only place that knows, which is why
    /// this is a `RefCell` for the same reason the scroll offsets are `Cell`s:
    /// the renderer reporting a measurement back, not state the input layer
    /// owns. Only the active view is drawn, so only its rows are held.
    pub detail_row_tops: std::cell::RefCell<Vec<u16>>,
    /// Body row the graph view highlights. The graph's cursor is a *row*, not
    /// a commit: its blocks run twenty rows and more, so a commit-granular
    /// cursor could only jump a whole block at a time and left the highlight
    /// nowhere while you read through one.
    pub graph_cursor: u16,
    /// Height of the detail pane's scrolling body, measured by the renderer.
    /// The scrolloff arithmetic needs it, and the renderer is the only place
    /// the pane's height and the header's are both known.
    pub detail_view_height: std::cell::Cell<u16>,
    pub inspector_max_scroll: std::cell::Cell<u16>,
    /// Repaint on the next loop pass. Set by every state change; ticks also
    /// force a paint every 4th frame so the clock and spinner stay live.
    pub needs_redraw: bool,
    /// Advances every 250ms tick; drives the loading/sweep spinner.
    pub spinner_frame: usize,
    /// Flips every 500ms; drives the pending-CI blink.
    pub blink_on: bool,
    /// The intro wave, while one is running. See `ui::intro`.
    pub intro: Option<crate::ui::intro::Intro>,
    /// The noise field shown while the first load is still running, so the
    /// wait is spent on the surface the wave will later wash away rather than
    /// on an empty frame.
    pub loading_field: Option<crate::ui::intro::Loading>,
    /// Set when the first screenful of real content is ready, and cleared by
    /// the paint that captures it. The wave resolves *into* a finished screen,
    /// so it cannot start until there is one to resolve into.
    pub intro_pending: bool,
    /// Whether the wave has already been armed this launch. A sweep delivers
    /// `Loaded` more than once; without this the dashboard would dissolve into
    /// noise again every time one landed.
    pub intro_armed: bool,
    /// True once the user pressed v; stops resize from auto-switching views.
    pub view_overridden: bool,
    /// Width of the repo list as a percentage of the main area, and the
    /// inspector's share of the right column. Nudged with the resize keys.
    /// Defaults are restored on every launch unless `persist_pane_sizes` is
    /// set, so a layout tweaked for one repo doesn't silently become the norm.
    pub repo_list_pct: u16,
    pub inspector_pct: u16,
    /// Whether the contribution strip is open, and from which source.
    pub contributions: ContributionsMode,
    /// The last source the strip was on, so closing and reopening returns to
    /// it instead of always snapping back to github.
    last_contributions_source: ContributionsMode,
    /// The account-wide calendar from the last GitHub poll. None until the
    /// first poll lands, or when the payload predates the field.
    pub github_calendar: Option<Box<crate::contributions::Calendar>>,
    /// Git author regex the local contribution source filters by, from the
    /// `contributions_author` config key. None resolves each repo's own
    /// `user.email` instead. Lives on `App` rather than being read from
    /// `Config` at each use, because the sweep is started from call sites
    /// that hold an `App` but not a `Config`.
    pub contrib_author: Option<String>,
    /// Grid slot the day cursor sits on, or None while the user has not
    /// travelled the grid yet. None is the resting state on purpose: a cell
    /// highlighted before anyone asked for it reads as a selection you did
    /// not make, and the title shows the year's total instead.
    pub contrib_cursor: Option<usize>,
    /// Leftmost visible week. The grid is wider than any terminal at the
    /// square size we draw, so it scrolls instead of shrinking. Written by
    /// the renderer (which is the only place the visible width is known) as
    /// well as by input, hence the `Cell`.
    pub contrib_scroll: std::cell::Cell<usize>,
    /// Weeks the last frame could actually fit, measured by the renderer so
    /// the scroll keys and the cursor have something real to clamp against.
    pub contrib_visible_weeks: std::cell::Cell<usize>,
    /// Mirror of the `persist_pane_sizes` config key: write the two above back
    /// on exit and restore them next launch.
    pub persist_pane_sizes: bool,
    /// The divider currently held by the mouse, if any. Set on a press that
    /// lands on a boundary and cleared on release, so drag events in between
    /// resize instead of doing whatever the panel under the cursor does.
    /// Whether the repo pane draws the detailed PR section under the header.
    ///
    /// Set from `-p` after construction, like the other launch options, so it
    /// stays out of `App::new`'s signature. It gates *drawing* only: the PR
    /// data rides every poll regardless, since a smaller page would not cost
    /// any less.
    pub show_pr_detail: bool,
    pub dragging: Option<crate::ui::Divider>,
    /// Screen column the in-progress contribution-strip pan is measured from,
    /// or None when no drag is underway. Separate from `dragging`, which owns
    /// pane resizing: a pan moves content inside a pane rather than moving
    /// the boundary between two.
    pub contrib_drag: Option<u16>,
    /// True while the initial load or an R reload is in flight.
    pub loading: bool,
    /// When the in-flight watch sweep started, or None when idle. Doubles as
    /// the "one sweep at a time" lock and as the stuck-sweep timer.
    pub sweep_started: Option<std::time::Instant>,
    /// Whether the in-flight sweep was asked for by a keypress rather than by
    /// the watch timer. Only a manual one announces itself in the header: a
    /// background refresh firing on an interval should not repaint a big
    /// colored indicator over a dashboard you are reading.
    ///
    /// Only meaningful while `sweep_started` is `Some`.
    pub sweep_manual: bool,
    /// When GitHub was last re-polled in watch mode.
    pub last_github_poll: Option<DateTime<Local>>,
    /// True while a GitHub re-poll is in flight, so ticks do not stack them.
    pub github_poll_inflight: bool,
    /// When the in-flight GitHub poll started, so a request that never returns
    /// can be given up on. Without this, one hung call leaves
    /// `github_poll_inflight` true forever and CI status silently stops
    /// updating for the rest of the session, with nothing on screen to say so.
    pub github_poll_started: Option<std::time::Instant>,
    /// True while the most recent GitHub poll came back an error, cleared by
    /// the first one that succeeds.
    ///
    /// The failure used to live only in a status message, which the next
    /// keypress clears. A poll that had been failing for hours therefore
    /// looked exactly like one that had just succeeded: the calendar and the
    /// CI lamps kept their last good values and nothing on screen said they
    /// had stopped moving. Rate limits are the common case and they last an
    /// hour, so this needs to survive a keypress.
    pub github_poll_failing: bool,
    /// Why the last full load failed, while it has left nothing to draw.
    ///
    /// A failed initial load used to set only a status message, which the
    /// next keypress clears. Since the repo list is derived from the GitHub
    /// dataset, the result was a completely blank dashboard whose one line of
    /// explanation vanished on the first keystroke -- indistinguishable from
    /// a fleet that genuinely has no repos in the window. Cleared by the next
    /// load that succeeds.
    pub load_error: Option<String>,
    /// Set when a sweep sees a repo's unpushed count fall to zero, which means
    /// you just pushed. That is the one moment CI is about to change and
    /// nothing is `Pending` yet to poll harder on, so it forces the next poll
    /// rather than waiting out the interval.
    pub push_detected: bool,
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
    /// Progress of an in-flight window apply, or None when idle. Drives the
    /// footer gauge; set when the apply is spawned, cleared when it lands.
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
    pub fn new(repos: Vec<RepoStatus>, header: String, term_width: u16, term_height: u16) -> Self {
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
            watch_mode: false,
            refresh_interval: 60,
            last_refreshed: Some(Local::now()),
            group_names: Vec::new(),
            active_group: None,
            focused_panel: FocusedPanel::RepoList,
            history_view: HistoryView::default(),
            detail_scrolls: std::array::from_fn(|_| std::cell::Cell::new(0)),
            detail_selections: [0; HistoryView::COUNT],
            inspector_scroll: 0,
            detail_max_scroll: std::cell::Cell::new(0),
            follow_selection: std::cell::Cell::new(false),
            detail_row_tops: std::cell::RefCell::new(Vec::new()),
            graph_cursor: 0,
            detail_view_height: std::cell::Cell::new(0),
            inspector_max_scroll: std::cell::Cell::new(0),
            needs_redraw: true,
            spinner_frame: 0,
            blink_on: true,
            intro: None,
            loading_field: None,
            intro_pending: false,
            intro_armed: false,
            view_overridden: false,
            repo_list_pct: DEFAULT_REPO_LIST_PCT,
            inspector_pct: DEFAULT_INSPECTOR_PCT,
            contributions: ContributionsMode::Off,
            last_contributions_source: ContributionsMode::Github,
            github_calendar: None,
            contrib_author: None,
            contrib_cursor: None,
            // Out of range on purpose: the renderer clamps it down to
            // "newest week at the right edge", which is where the grid opens.
            contrib_scroll: std::cell::Cell::new(crate::contributions::WEEKS),
            contrib_visible_weeks: std::cell::Cell::new(crate::contributions::WEEKS),
            persist_pane_sizes: false,
            show_pr_detail: false,
            dragging: None,
            contrib_drag: None,
            loading: false,
            sweep_started: None,
            sweep_manual: false,
            last_github_poll: None,
            github_poll_inflight: false,
            github_poll_started: None,
            github_poll_failing: false,
            load_error: None,
            push_detected: false,
            dataset: Vec::new(),
            window: None,
            dirty_mode: false,
            apply_progress: None,
        }
    }

    /// Called on every tick from the event loop. The spinner advances one
    /// frame per tick, so the tick interval is the spin rate; the blink flips
    /// every `BLINK_TICKS` so it keeps pulsing at 1Hz whatever that rate is.
    pub fn on_tick(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
        if self.spinner_frame.is_multiple_of(BLINK_TICKS) {
            self.blink_on = !self.blink_on;
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
        self.set_detail_scroll(0);
        self.set_detail_selected(0);
        self.inspector_scroll = 0;
    }

    /// Repos filtered by the current search query, active group, and quick
    /// filters -- the latter two both parsed out of `search_query`'s `#tag`
    /// words (see `parse_query`), so there's a single source of truth for
    /// "what's typed" rather than a separate toggled-by-keypress field.
    /// Whether the main area should give way to the load-failure panel.
    ///
    /// Both halves matter. Without `load_error` an empty fleet is just an
    /// empty fleet. Without `repos.is_empty()` a failed *reload* would replace
    /// a dashboard full of good rows with an error panel, which is worse than
    /// the stale data it removed: that case keeps its rows and reports the
    /// failure in the status line and the header instead.
    pub fn shows_load_error(&self) -> bool {
        self.load_error.is_some() && self.repos.is_empty()
    }

    /// Whether the inspector is drawn at all.
    ///
    /// The graph view's blocks already carry the hash, author, date and full
    /// message, so an inspector there repeats the block while taking the rows
    /// the block needs. Read by both `ui::panel_rects` and `ui::render`
    /// through `detail::split_inspector`, so the two cannot disagree about
    /// whether the pane exists.
    pub fn shows_inspector(&self) -> bool {
        self.history_view != HistoryView::Graph
    }

    pub fn visible_repos(&self) -> Vec<(usize, &RepoStatus)> {
        let active_group = self.active_group.and_then(|i| self.group_names.get(i));
        let parsed = parse_query(&self.search_query);

        let mut visible: Vec<(usize, &RepoStatus, i32)> = self
            .repos
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                // Group filter
                if let Some(group) = active_group
                    && !r.groups.contains(group)
                {
                    return None;
                }
                // Quick filters (dirty/behind/ahead/CI/PRs/stashes)
                if !parsed.filters.matches(r) {
                    return None;
                }
                // Fuzzy search: None means the name isn't a match at all.
                let score = crate::fuzzy::score(&parsed.text, &r.name)?;
                Some((i, r, score))
            })
            .collect();

        if parsed.text.is_empty() {
            // Navigation order must match what the repo list draws, which
            // groups by location (github activity, then local changes, then
            // also active). Without this the cursor walks assembly order and
            // appears to jump between sections. sort_by_key is stable, so
            // order within a section is untouched.
            visible.sort_by_key(|(_, r, _)| section_rank(r));
        } else {
            // While searching the list is one ranked result set, best match
            // first, so the top row is what Enter should act on. The repo list
            // drops its section headers to match (see `render_repo_list`);
            // both sides key off the same "is there text to rank by" test.
            visible.sort_by_key(|(_, _, score)| std::cmp::Reverse(*score));
        }

        visible.into_iter().map(|(i, r, _)| (i, r)).collect()
    }

    /// True when the query has a free-text portion to rank by, as opposed to
    /// being empty or nothing but `#tag` words. Decides both the sort order
    /// above and whether the repo list draws sections.
    pub fn has_text_query(&self) -> bool {
        !parse_query(&self.search_query).text.is_empty()
    }

    /// Move the vertical divider between the repo list and the right column.
    /// Positive widens the repo list.
    pub fn resize_repo_list(&mut self, delta: i16) {
        self.repo_list_pct = step_pct(
            self.repo_list_pct,
            delta,
            REPO_LIST_PCT_MIN,
            REPO_LIST_PCT_MAX,
        );
        self.needs_redraw = true;
    }

    /// Move the horizontal divider between the detail pane and the inspector.
    /// Positive grows the inspector.
    pub fn resize_inspector(&mut self, delta: i16) {
        self.inspector_pct = step_pct(
            self.inspector_pct,
            delta,
            INSPECTOR_PCT_MIN,
            INSPECTOR_PCT_MAX,
        );
        self.needs_redraw = true;
    }

    /// Set the repo list width directly, as a percentage. For mouse drags,
    /// where the pointer names an absolute position rather than a step. The
    /// clamp is the same one the keys use, so a drag past the edge parks the
    /// divider at the limit instead of collapsing the pane.
    pub fn set_repo_list_pct(&mut self, pct: u16) {
        let clamped = pct.clamp(REPO_LIST_PCT_MIN, REPO_LIST_PCT_MAX);
        if clamped != self.repo_list_pct {
            self.repo_list_pct = clamped;
            self.needs_redraw = true;
        }
    }

    /// Set the inspector height directly, as a percentage of the right column.
    pub fn set_inspector_pct(&mut self, pct: u16) {
        let clamped = pct.clamp(INSPECTOR_PCT_MIN, INSPECTOR_PCT_MAX);
        if clamped != self.inspector_pct {
            self.inspector_pct = clamped;
            self.needs_redraw = true;
        }
    }

    /// Restore both panes to their launch defaults.
    pub fn reset_pane_sizes(&mut self) {
        self.repo_list_pct = DEFAULT_REPO_LIST_PCT;
        self.inspector_pct = DEFAULT_INSPECTOR_PCT;
        self.needs_redraw = true;
    }

    /// Strip any `#tag` filter words from the search query, keeping free
    /// text intact.
    pub fn clear_filters(&mut self) {
        self.search_query = parse_query(&self.search_query).text;
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
        self.set_detail_scroll(0);
        // Before the selection, which in the graph view reads it back.
        self.graph_cursor = 0;
        self.set_detail_selected(0);
        self.inspector_scroll = 0;
    }

    /// Rows the cursor can walk in the *active* history view.
    ///
    /// Not `commits.len()`: the cursor walks whichever list is on screen, and
    /// an index valid against the commit list is meaningless against the
    /// reflog -- or against the graph, which `info` and `commits` both draw.
    pub fn selected_commit_count(&self) -> usize {
        let Some(repo) = self.selected_repo() else {
            return 0;
        };
        // One row per commit: the graph is computed from parent hashes rather
        // than read out of `git log --graph`, so there are no connector rows
        // to skip past. See `crate::graph`.
        let graph_commits = || repo.graph.len();
        match self.history_view {
            // Info draws the newest few *graph* rows, not the newest few of
            // `commits`. The two are different `git log` runs: `commits` walks
            // HEAD, the graph walks the union of the local branches, so a
            // commit living only on a side branch sits in one and not the
            // other. Counting the wrong one put the cursor's ordinal and the
            // hash it resolved on different rows.
            HistoryView::Info => graph_commits().min(crate::ui::detail::RECENT_COMMIT_COUNT),
            HistoryView::Commits => graph_commits(),
            HistoryView::Reflog => repo.reflog.len(),
            HistoryView::Graph => graph_commits(),
        }
    }

    /// Move the inspector's commit cursor within the selected repo's commits.
    pub fn select_commit_up(&mut self) {
        if self.detail_selected() > 0 {
            self.set_detail_selected(self.detail_selected() - 1);
            self.inspector_scroll = 0;
        }
    }

    pub fn select_commit_down(&mut self) {
        let max = self.selected_commit_count().saturating_sub(1);
        if self.detail_selected() < max {
            self.set_detail_selected(self.detail_selected() + 1);
            self.inspector_scroll = 0;
        }
    }

    /// Detail-pane down-navigation: step the commit cursor through the recent
    /// commits, then -- once past the last one -- scroll the pane so the
    /// history/reflog/CI sections below the commits become reachable.
    pub fn detail_nav_down(&mut self) {
        // Spent on the re-entry when the selection is off screen: see
        // `reenter_selection`.
        if self.reenter_selection() {
            return;
        }
        if self.detail_selected() + 1 < self.selected_commit_count() {
            self.select_commit_down();
        } else {
            // Past the last row there is still content below it -- branches,
            // stashes -- so keep scrolling rather than stopping dead.
            self.scroll_detail(1);
        }
    }

    /// Detail-pane up-navigation: step the commit cursor, and let the
    /// renderer bring it into view.
    ///
    /// It used to unwind the content scroll first. Once the renderer began
    /// following the selection the two fought: this decremented the offset
    /// and the next frame restored it, so the pane never moved.
    pub fn detail_nav_up(&mut self) {
        // Spent on the re-entry when the selection is off screen: see
        // `reenter_selection`.
        if self.reenter_selection() {
            return;
        }
        if self.detail_selected() > 0 {
            self.select_commit_up();
        } else {
            // At the top of the list there is still the header above it.
            self.scroll_detail(-1);
        }
    }

    /// The commit the inspector is currently showing, if any.
    /// The hash the active history view has selected.
    ///
    /// Every view draws a list of its own, and they are lists of different
    /// things: `info` the newest graph rows, `commits` and `graph` all of
    /// them, `reflog` the HEAD movements. Which is why this returns a hash
    /// rather than an index -- an index into one means nothing in another.
    pub fn inspected_hash(&self) -> Option<&str> {
        let repo = self.selected_repo()?;
        // Clamped, not bounds-checked: the index survives a repo change and a
        // view switch, so it can outlive the list it was chosen against. A
        // stale one resolves to the last row rather than to nothing, which is
        // what keeps the readout drawn instead of vanishing.
        let i = self
            .detail_selected()
            .min(self.selected_commit_count().saturating_sub(1));
        let graph_nth = |n: usize| repo.graph.get(n).map(|g| g.hash.as_str());
        match self.history_view {
            HistoryView::Info | HistoryView::Commits | HistoryView::Graph => graph_nth(i),
            HistoryView::Reflog => repo.reflog.get(i).map(|e| e.hash.as_str()),
        }
    }

    /// The commit the inspector should show: whatever the active view
    /// selected, resolved against the loaded commit list.
    ///
    /// Resolved by hash rather than by index, because the reflog and the
    /// graph are different lists of different lengths -- an index into one
    /// means nothing in another. Returns None when the selection names a
    /// commit outside the loaded window, which the reflog can easily do:
    /// it reaches back further than the commit list does.
    pub fn inspected_commit(&self) -> Option<&crate::model::Commit> {
        let repo = self.selected_repo()?;
        let hash = self.inspected_hash()?;
        repo.commits.iter().find(|c| c.hash == hash)
    }

    /// Scroll the inspector by `delta` lines, clamped to the content the last
    /// frame actually measured.
    pub fn scroll_inspector(&mut self, delta: i32) {
        let max = self.inspector_max_scroll.get() as i32;
        let current = self.inspector_scroll as i32;
        self.inspector_scroll = (current + delta).clamp(0, max) as u16;
    }

    /// The pane `Shift+J`/`K` (and `Shift+↑`/`↓`) drives: the one *after* the
    /// focused pane, so you can stand in one pane and pull more into view in
    /// the next without leaving where you are.
    ///
    /// The strip is never a target. It is a fixed-height horizontal timeline
    /// with nothing below the fold, so scrolling it vertically has no meaning;
    /// it is skipped rather than left in the cycle as a no-op, which would have
    /// made the key do nothing at all from whichever pane sat before it.
    ///
    /// Compact has one flat grid and no next pane, so there is nothing to aim
    /// at and `None` is the honest answer.
    fn shift_scroll_target(&self) -> Option<FocusedPanel> {
        if self.view != View::Detail {
            return None;
        }
        Some(match self.focused_panel {
            FocusedPanel::RepoList => FocusedPanel::Detail,
            FocusedPanel::Detail if self.shows_inspector() => FocusedPanel::Inspector,
            // The graph docks no inspector, so there is no next pane to reach
            // into -- and it is the one view whose rows are finer than its
            // commits, so there is a gesture worth having here instead. The
            // key stays on this pane and walks its rows one at a time, where
            // plain `j`/`k` jumps whole commit blocks. Wrapping to the repo
            // list instead would spend the only line-by-line gesture the graph
            // has on moving to a different repo.
            FocusedPanel::Detail => FocusedPanel::Detail,
            FocusedPanel::Inspector | FocusedPanel::Contributions => FocusedPanel::RepoList,
        })
    }

    /// Scroll the pane after the focused one. See `shift_scroll_target`.
    ///
    /// Aimed at the detail pane it *steps that pane's cursor* rather than
    /// scrolling it: stepping is what makes the inspector read out each commit
    /// in turn, which is the reason to drive window 2 from window 1 at all.
    /// Past the last commit `detail_nav_*` falls through to scrolling, so the
    /// sections below a short list stay reachable. Aimed at the repo list it
    /// walks the selection, which is that list's equivalent.
    pub fn scroll_next_panel(&mut self, delta: i32) {
        let Some(target) = self.shift_scroll_target() else {
            return;
        };
        match target {
            FocusedPanel::RepoList => {
                for _ in 0..delta.unsigned_abs() {
                    match delta > 0 {
                        true => self.move_down(),
                        false => self.move_up(),
                    }
                }
            }
            // Aimed at the pane it is already in: only the graph does that,
            // and only to walk its rows. `move_graph_cursor` is a row cursor,
            // so this is the line-by-line step.
            FocusedPanel::Detail if self.focused_panel == FocusedPanel::Detail => {
                self.move_graph_cursor(delta);
            }
            FocusedPanel::Detail => {
                for _ in 0..delta.unsigned_abs() {
                    match delta > 0 {
                        true => self.detail_nav_down(),
                        false => self.detail_nav_up(),
                    }
                }
            }
            FocusedPanel::Inspector => self.scroll_inspector(delta),
            // Never selected as a target, but the match has to be total.
            FocusedPanel::Contributions => {}
        }
    }

    /// Flip between the detail and compact views. Marks the view as
    /// user-chosen so a later terminal resize stops re-picking it.
    pub fn toggle_view(&mut self) {
        self.view_overridden = true;
        self.view = match self.view {
            View::Detail => View::Compact,
            View::Compact => View::Detail,
        };
        // Compact has no detail pane to focus, and switching views should
        // always land somewhere predictable rather than carry over whatever
        // was focused/scrolled before.
        self.focused_panel = FocusedPanel::RepoList;
        self.set_detail_scroll(0);
    }

    /// Cycle keyboard focus: repo list -> detail -> inspector -> repo list.
    /// Only meaningful in `Detail` view.
    pub fn toggle_panel_focus(&mut self) {
        let strip = self.contributions.is_open();
        if self.view != View::Detail {
            // Compact has one flat grid, but the strip is still a real target.
            if strip {
                self.focused_panel = match self.focused_panel {
                    FocusedPanel::Contributions => FocusedPanel::RepoList,
                    _ => FocusedPanel::Contributions,
                };
            }
            return;
        }
        self.focused_panel = match self.focused_panel {
            FocusedPanel::RepoList => FocusedPanel::Detail,
            FocusedPanel::Detail if self.shows_inspector() => FocusedPanel::Inspector,
            FocusedPanel::Detail if strip => FocusedPanel::Contributions,
            FocusedPanel::Detail => FocusedPanel::RepoList,
            FocusedPanel::Inspector if strip => FocusedPanel::Contributions,
            FocusedPanel::Inspector | FocusedPanel::Contributions => FocusedPanel::RepoList,
        };
    }

    /// Jump keyboard focus directly to `panel` (the `1`/`2` bindings).
    /// Only meaningful in `Detail` view.
    pub fn focus_panel(&mut self, panel: FocusedPanel) {
        // The strip is the one panel that exists in both views, and it is
        // only a target while it is open.
        if panel == FocusedPanel::Contributions {
            if self.contributions.is_open() {
                self.focused_panel = panel;
            }
            return;
        }
        if self.view != View::Detail {
            return;
        }
        // The graph view draws no inspector, and focus on a pane that is not
        // on screen swallows every later keypress.
        if panel == FocusedPanel::Inspector && !self.shows_inspector() {
            return;
        }
        self.focused_panel = panel;
    }

    /// `c`: open or close the strip. Reopening returns to whichever source it
    /// was last on.
    pub fn toggle_contributions(&mut self) {
        if self.contributions.is_open() {
            self.last_contributions_source = self.contributions;
            self.contributions = ContributionsMode::Off;
            // Focus cannot be left pointing at a panel that is no longer on
            // screen: every key routed to it would silently do nothing.
            if self.focused_panel == FocusedPanel::Contributions {
                self.focused_panel = FocusedPanel::RepoList;
            }
        } else {
            self.open_contributions(self.last_contributions_source);
        }
        self.needs_redraw = true;
    }

    /// `C`: switch which source fills the strip, opening it if it was closed.
    /// Opening from `Off` lands on the remembered source rather than always
    /// snapping to github, so `c` and `C` agree about where "open" means.
    pub fn cycle_contributions_source(&mut self) {
        let next = match self.contributions {
            ContributionsMode::Off => self.last_contributions_source,
            ContributionsMode::Github => ContributionsMode::Local,
            ContributionsMode::Local => ContributionsMode::Github,
        };
        self.open_contributions(next);
        self.needs_redraw = true;
    }

    /// Open the strip on `mode`, with no day marked and the grid scrolled to
    /// the newest weeks. Shared by `c` and `C` so both land in the same state.
    fn open_contributions(&mut self, mode: ContributionsMode) {
        self.contributions = mode;
        self.last_contributions_source = mode;
        self.contrib_cursor = None;
        // Out of range on purpose, exactly as at startup: the renderer is the
        // only place the visible width is known, and it clamps this down to
        // "newest week at the right edge". Computing it here from the last
        // measured width would anchor on the oldest weeks the first time,
        // before any frame has been drawn.
        self.contrib_scroll.set(crate::contributions::WEEKS);
    }

    /// The last selectable grid slot for `today`: the grid stops at today,
    /// not at the end of today's week.
    pub fn contrib_last_slot(&self, today: chrono::NaiveDate) -> usize {
        crate::contributions::Calendar::new(today, crate::contributions::Source::Local).today_slot()
    }

    /// Move the day cursor `dweeks` columns and `ddays` rows, which is how
    /// the grid actually reads: a week is a column, a weekday is a row.
    ///
    /// The first move seeds the cursor on today rather than jumping from
    /// nowhere, so pressing any direction key starts from where the eye is.
    /// Both axes clamp independently, so walking up a column never spills
    /// into the neighbouring week, and the final column stops at today.
    pub fn move_contrib_cursor(&mut self, dweeks: i32, ddays: i32, today: chrono::NaiveDate) {
        let last = self.contrib_last_slot(today);
        let (last_week, last_weekday) = (last / 7, last % 7);

        let Some(current) = self.contrib_cursor else {
            // First keypress only marks today; it does not also move.
            self.contrib_cursor = Some(last);
            self.scroll_cursor_into_view(last_week);
            self.needs_redraw = true;
            return;
        };

        let week = ((current / 7) as i32 + dweeks).clamp(0, last_week as i32) as usize;
        // The newest column is short: it ends on today's weekday.
        let max_weekday = if week == last_week { last_weekday } else { 6 };
        let weekday = ((current % 7) as i32 + ddays).clamp(0, max_weekday as i32) as usize;

        self.contrib_cursor = Some(week * 7 + weekday);
        self.scroll_cursor_into_view(week);
        self.needs_redraw = true;
    }

    /// Jump the cursor to the oldest or newest column, keeping its weekday.
    pub fn jump_contrib_cursor(&mut self, to_end: bool, today: chrono::NaiveDate) {
        let last = self.contrib_last_slot(today);
        let weekday = self.contrib_cursor.map_or(last % 7, |c| c % 7);
        let week = if to_end { last / 7 } else { 0 };
        let max_weekday = if week == last / 7 { last % 7 } else { 6 };
        self.contrib_cursor = Some(week * 7 + weekday.min(max_weekday));
        self.scroll_cursor_into_view(week);
        self.needs_redraw = true;
    }

    /// Scroll the grid the minimum amount that brings `week` back on screen.
    fn scroll_cursor_into_view(&self, week: usize) {
        let visible = self.contrib_visible_weeks.get().max(1);
        let first = self.contrib_scroll.get();
        if week < first {
            self.contrib_scroll.set(week);
        } else if week >= first + visible {
            self.contrib_scroll.set(week + 1 - visible);
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
        let visible = self.contrib_visible_weeks.get().max(1);
        let max = crate::contributions::WEEKS.saturating_sub(visible);
        let next = (self.contrib_scroll.get() as i32 + delta).clamp(0, max as i32) as usize;
        self.contrib_scroll.set(next);
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

    /// Scroll the detail pane by `delta` lines (negative scrolls up), clamped
    /// to the content the last frame actually measured.
    ///
    /// This used to clamp to a fixed `DETAIL_SCROLL_CAP` of 300 and let the
    /// renderer clamp again for display. The offset therefore kept climbing
    /// long after the view stopped moving, and scrolling back needed one
    /// no-op keypress for every step of overshoot before anything moved --
    /// which read as the pane being stuck.
    pub fn scroll_detail(&mut self, delta: i32) {
        // The graph view scrolls by moving its row cursor and letting the view
        // follow, so every gesture that scrolls it also moves the highlight.
        if self.history_view == HistoryView::Graph {
            return self.move_graph_cursor(delta);
        }
        let max = self.detail_max_scroll.get() as i32;
        let current = self.detail_scroll() as i32;
        self.set_detail_scroll((current + delta).clamp(0, max) as u16);
    }

    /// Bring the selection back inside the pane, and report whether it moved.
    ///
    /// Called when a step is asked for, **not** while scrolling. Scrolling
    /// used to drag the selection along on every wheel notch, which meant the
    /// wheel silently reassigned which commit the inspector was reading out:
    /// the highlight crept down the pane on its own and the panel below it
    /// changed under you. Scrolling now leaves the selection with the content
    /// it is attached to, exactly as it leaves every other row, and it is the
    /// first `j`/`k` afterwards that re-enters -- from the edge you are
    /// looking at rather than from a row that is no longer on screen.
    ///
    /// That first step is spent on the re-entry and does not also advance, so
    /// the highlight appears where the eye is before it starts moving again.
    ///
    /// The selection lands on the nearest entry that clears the margin, and
    /// stays put when there is no such entry: `info` draws branches, stashes
    /// and worktrees below its commit list, and those rows must stay
    /// reachable by scrolling past the last commit.
    ///
    /// Deliberately does not set `follow_selection`: that would ask the next
    /// frame to scroll the entry into view, undoing the scroll that chose it.
    fn reenter_selection(&mut self) -> bool {
        let height = self.detail_view_height.get();
        if height == 0 {
            return false;
        }
        let scroll = self.detail_scroll();
        let pad = scrolloff_for(height);
        // The margin collapses at the ends of the content: there is nothing
        // beyond them to keep in reserve, and holding it there would make the
        // first and last entries unreachable.
        let first = scroll + if scroll > 0 { pad } else { 0 };
        let last = scroll + height
            - 1
            - if scroll < self.detail_max_scroll.get() {
                pad
            } else {
                0
            };

        let tops = self.detail_row_tops.borrow();
        let Some(&row) = tops.get(self.detail_selected()) else {
            return false;
        };
        let next = if row < first {
            tops.iter().position(|&t| t >= first)
        } else if row > last {
            tops.iter().rposition(|&t| t <= last)
        } else {
            None
        };
        drop(tops);
        match next {
            Some(i) if i != self.detail_selected() => {
                self.detail_selections[self.history_view.index()] = i;
                self.inspector_scroll = 0;
                true
            }
            _ => false,
        }
    }

    /// Move the graph view's row cursor by `delta` rows and let the view
    /// follow it, keeping `SCROLLOFF` rows of context past the cursor at
    /// whichever edge it is heading for.
    ///
    /// The highlight therefore walks down the pane until it is that far from
    /// the bottom, and from there stays put while the text runs under it --
    /// the way a pager or an editor moves. A commit-granular marker could not
    /// do this: blocks run past twenty rows, so it stood still through a whole
    /// commit and then jumped.
    pub fn move_graph_cursor(&mut self, delta: i32) {
        let last = self.graph_body_rows().saturating_sub(1) as i32;
        self.graph_cursor = (self.graph_cursor as i32 + delta).clamp(0, last) as u16;
        self.keep_graph_cursor_in_view();
        self.sync_graph_selection();
    }

    /// Rows in the detail pane's scrolling body, as the last frame measured
    /// it: the largest useful offset plus the height it leaves on screen.
    fn graph_body_rows(&self) -> u16 {
        self.detail_max_scroll.get() + self.detail_view_height.get()
    }

    /// Nudge the scroll offset so the cursor sits at least `SCROLLOFF`
    /// rows inside the pane, and otherwise leave it exactly where it is --
    /// that "otherwise" is what makes the highlight, not the text, move
    /// through the middle of the pane.
    fn keep_graph_cursor_in_view(&mut self) {
        let height = self.detail_view_height.get();
        if height == 0 {
            return;
        }
        // Never more than half the pane, or the two margins overlap and the
        // cursor is pinned to the middle of a short one.
        let pad = scrolloff_for(height);
        let cursor = self.graph_cursor;
        // cursor - scroll must land in [pad, height - 1 - pad].
        let lowest = cursor.saturating_sub(height - 1 - pad);
        let highest = cursor.saturating_sub(pad);
        let scroll = self
            .detail_scroll()
            .clamp(lowest, highest)
            .min(self.detail_max_scroll.get());
        self.set_detail_scroll(scroll);
    }

    /// Bring the cursor's row into view for a commit step, from the top.
    ///
    /// A commit step lands on a block's first row, and the minimal nudge
    /// `keep_graph_cursor_in_view` makes would leave that row in the *bottom*
    /// margin: four rows of the commit you just stepped to, and the rest of it
    /// below the fold. A row already comfortably on screen is left alone, so
    /// stepping through commits that share a screen does not jerk the view.
    fn show_graph_cursor_from_top(&mut self) {
        let height = self.detail_view_height.get();
        if height == 0 {
            return;
        }
        let pad = scrolloff_for(height);
        let scroll = self.detail_scroll();
        let cursor = self.graph_cursor;
        if cursor < scroll + pad || cursor > scroll + height - 1 - pad {
            let top = cursor.saturating_sub(pad).min(self.detail_max_scroll.get());
            self.set_detail_scroll(top);
        }
    }

    /// The commit the row cursor is inside, which is what the hash emphasis
    /// and `inspected_hash` key off.
    ///
    /// Deliberately does not set `follow_selection`: that would ask the next
    /// frame to scroll the block into view, undoing the scroll that chose it.
    fn sync_graph_selection(&mut self) {
        let cursor = self.graph_cursor;
        let block = self
            .detail_row_tops
            .borrow()
            .iter()
            .rposition(|&top| top <= cursor);
        if let Some(i) = block {
            self.detail_selections[HistoryView::Graph.index()] = i;
        }
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

    /// Footer counts for what the repo list is currently drawing, not for the
    /// whole fleet. The list walks `visible_repos`, so folding `self.repos`
    /// here left the footer describing repos a quick filter or a search had
    /// already hidden.
    /// Scroll offset and selection for the *active* history view. Every
    /// caller wants the current one; the arrays exist so switching views does
    /// not lose where you were in the one you left.
    pub fn detail_scroll(&self) -> u16 {
        self.detail_scrolls[self.history_view.index()].get()
    }

    /// Takes `&self` because the renderer sets it too -- see the field.
    pub fn set_detail_scroll(&self, value: u16) {
        self.detail_scrolls[self.history_view.index()].set(value);
    }

    pub fn detail_selected(&self) -> usize {
        self.detail_selections[self.history_view.index()]
    }

    pub fn set_detail_selected(&mut self, value: usize) {
        self.detail_selections[self.history_view.index()] = value;
        // Stepping a commit in the graph view moves the row cursor to that
        // commit's own row, and the view is nudged around it here rather than
        // by `follow_selection`: bring-into-view puts the row hard against an
        // edge, which the very next scroll would undo by pulling the scrolloff
        // margin back.
        if self.history_view == HistoryView::Graph {
            let top = self.detail_row_tops.borrow().get(value).copied();
            if let Some(top) = top {
                self.graph_cursor = top;
                self.show_graph_cursor_from_top();
                return;
            }
        }
        // Any cursor move asks the pane to follow. Scrolling does not, which
        // is what keeps the two from fighting each other to a standstill.
        self.follow_selection.set(true);
    }

    /// Step to the next history view, wrapping, or the previous one.
    ///
    /// `[` and `]`, which is lazygit's own binding for the same gesture and
    /// leaves `1`-`4` to go on focusing panes.
    pub fn cycle_history_view(&mut self, back: bool) {
        let n = HistoryView::COUNT;
        let i = self.history_view.index();
        let next = if back { (i + n - 1) % n } else { (i + 1) % n };
        self.history_view = HistoryView::ALL[next];
        // The inspector may have just stopped being drawn; focus cannot stay
        // on a pane that is not on screen.
        if self.focused_panel == FocusedPanel::Inspector && !self.shows_inspector() {
            self.focused_panel = FocusedPanel::Detail;
        }
        // Deliberately does not follow the selection. Each view keeps its own
        // scroll offset, so arriving restores where you left that view --
        // which is where its cursor is anyway, since the two were left in
        // step. Following on arrival meant the expanded graph, which anchors
        // its selection to the top of the pane, scrolled its own header off
        // the moment you switched to it: you asked for the tab and got a
        // commit, not a page.
        self.needs_redraw = true;
    }

    pub fn summary(&self) -> Summary {
        Summary::from_refs(self.visible_repos().into_iter().map(|(_, r)| r))
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

    fn commit(hash: &str) -> crate::model::Commit {
        crate::model::Commit {
            hash: hash.to_string(),
            author: "Gaurgle".to_string(),
            date: None,
            message: format!("subject {hash}"),
            body: String::new(),
            insertions: 0,
            deletions: 0,
            files: vec![],
        }
    }

    fn make_app(n: usize) -> App {
        let repos: Vec<RepoStatus> = (0..n)
            .map(|i| RepoStatus {
                daily: None,
                last_local: None,
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
                graph_expanded: Vec::new(),
                repo_kind: crate::model::RepoKind::Unknown,
                owner: None,
                remote_url: None,
                remote_only_branches: vec![],
                work_key: None,
            })
            .collect();
        App::new(repos, "test".to_string(), 120, 40)
    }

    /// Regression: the footer folded `self.repos` while the repo list drew
    /// `visible_repos`, so with a filter on it reported repos that were not
    /// on screen. The two walk the same list now.
    /// Regression: the wheel could only ever move the grid one way. The strip
    /// opens with `contrib_scroll` pinned to the newest week, so scrolling
    /// "newer" was already clamped and did nothing, and the arm that should
    /// have scrolled older had been wired to a cursor move instead. The
    /// symptom was a strip where the only thing the mouse could do was walk
    /// the selected day upward.
    #[test]
    fn the_wheel_scrolls_the_grid_both_ways() {
        let mut app = make_app(1);
        app.contrib_visible_weeks.set(20);
        let max = crate::contributions::WEEKS - 20;
        app.contrib_scroll.set(max);

        // Newer, from the right edge: already there, nothing to give.
        app.scroll_contributions(3);
        assert_eq!(app.contrib_scroll.get(), max, "clamped at the newest week");

        // Older must actually move, which is the half that was missing.
        app.scroll_contributions(-3);
        assert_eq!(app.contrib_scroll.get(), max - 3);
        app.scroll_contributions(3);
        assert_eq!(app.contrib_scroll.get(), max, "and back again");

        // Neither direction disturbs the day cursor.
        assert_eq!(app.contrib_cursor, None);
    }

    /// Dragging pulls the grid along with the pointer, and a drag too short
    /// to be worth a whole week banks its columns instead of discarding
    /// them -- otherwise a slow drag rounds to zero forever and never moves.
    #[test]
    fn dragging_pans_the_grid_and_banks_the_remainder() {
        let mut app = make_app(1);
        app.contrib_visible_weeks.set(20);
        let start = crate::contributions::WEEKS - 20;
        app.contrib_scroll.set(start);

        // One column is less than a week: nothing moves, anchor unchanged.
        let anchor = app.pan_contributions(50, 51);
        assert_eq!(anchor, 50, "sub-week drag keeps its anchor");
        assert_eq!(app.contrib_scroll.get(), start);

        // Dragging right pulls older weeks in from the left.
        let anchor = app.pan_contributions(50, 58);
        assert_eq!(app.contrib_scroll.get(), start - 4, "8 columns = 4 weeks");
        assert_eq!(anchor, 58, "anchor advances by what was consumed");

        // And back the other way.
        app.pan_contributions(58, 50);
        assert_eq!(app.contrib_scroll.get(), start);
    }

    /// One key steps through every view and comes back where it started.
    #[test]
    fn cycling_history_views_wraps_through_all_of_them() {
        let mut app = make_app(1);
        assert_eq!(app.history_view, HistoryView::Info, "opens on the facts");

        let mut seen = vec![app.history_view];
        for _ in 1..HistoryView::COUNT {
            app.cycle_history_view(false);
            seen.push(app.history_view);
        }
        assert_eq!(seen, HistoryView::ALL.to_vec(), "in tab-strip order");

        app.cycle_history_view(false);
        assert_eq!(app.history_view, HistoryView::Info, "wraps forward");

        // And backwards, which a shifted letter could not have given us.
        app.cycle_history_view(true);
        assert_eq!(app.history_view, HistoryView::Graph, "wraps backward");
        app.cycle_history_view(true);
        assert_eq!(app.history_view, HistoryView::Reflog);
    }

    /// Scroll and selection are per view. A shared offset is wrong the moment
    /// you switch back to something you had already scrolled -- you would
    /// land wherever the other list happened to leave you.
    #[test]
    fn each_view_keeps_its_own_place() {
        let mut app = make_app(1);

        app.history_view = HistoryView::Commits;
        app.set_detail_scroll(12);
        app.set_detail_selected(3);

        app.history_view = HistoryView::Reflog;
        assert_eq!(app.detail_scroll(), 0, "a fresh view starts at the top");
        assert_eq!(app.detail_selected(), 0);
        app.set_detail_scroll(40);
        app.set_detail_selected(7);

        app.history_view = HistoryView::Commits;
        assert_eq!(app.detail_scroll(), 12, "commits kept its place");
        assert_eq!(app.detail_selected(), 3);

        app.history_view = HistoryView::Reflog;
        assert_eq!(app.detail_scroll(), 40, "and so did the reflog");
        assert_eq!(app.detail_selected(), 7);
    }

    /// The cursor walks whichever list is on screen, so its bound is that
    /// list's length. `info` and `commits` both draw the graph; only the cap
    /// differs.
    #[test]
    fn the_cursor_is_bounded_by_the_active_views_list() {
        let mut app = make_app(1);
        app.repos[0].commits = vec![commit("aaa"), commit("bbb")];
        app.repos[0].reflog = vec![
            crate::model::ReflogEntry {
                hash: "aaa".to_string(),
                action: "commit".to_string(),
                description: "one".to_string(),
            },
            crate::model::ReflogEntry {
                hash: "bbb".to_string(),
                action: "checkout".to_string(),
                description: "two".to_string(),
            },
            crate::model::ReflogEntry {
                hash: "ccc".to_string(),
                action: "pull".to_string(),
                description: "three".to_string(),
            },
        ];

        // Both history lists walk the graph rows, not the diffstat list: the
        // diffstat list is `git log HEAD` and the graph is the union of the
        // local branches, so they are not the same commits in the same order.
        app.repos[0].graph = vec![crate::model::GraphLine {
            lanes: vec![crate::graph::commit_cell()],
            hash: "aaa".to_string(),
            is_merge: false,
            refs: vec![],
            author: "Gaurgle".to_string(),
            message: "one".to_string(),
        }];
        app.history_view = HistoryView::Info;
        assert_eq!(app.selected_commit_count(), 1, "graph rows, not commits");

        app.history_view = HistoryView::Commits;
        assert_eq!(app.selected_commit_count(), 1, "graph rows, not commits");

        app.history_view = HistoryView::Reflog;
        assert_eq!(app.selected_commit_count(), 3, "a longer list of its own");
    }

    #[test]
    fn the_summary_describes_only_what_the_list_is_drawing() {
        let mut app = make_app(4);
        app.repos[0].dirty = 3;
        app.repos[1].dirty = 1;
        app.repos[2].behind = 2;
        app.repos[3].location = RepoLocation::NotCloned;

        let all = app.summary();
        assert_eq!(all.dirty_count, 2, "unfiltered, the whole fleet counts");
        assert_eq!(all.behind_count, 1);
        assert_eq!(all.not_cloned_count, 1);

        // Narrow to a single repo by name; the footer must follow the list.
        app.search_query = "repo-0".to_string();
        let filtered = app.summary();
        assert_eq!(app.visible_repos().len(), 1, "test premise");
        assert_eq!(filtered.dirty_count, 1);
        assert_eq!(filtered.behind_count, 0, "repo-2 is filtered out of view");
        assert_eq!(filtered.not_cloned_count, 0, "repo-3 is filtered out too");
    }

    #[test]
    fn visible_repos_are_grouped_in_display_order() {
        // Assembly order deliberately interleaves the three sections; the
        // repo list always draws them grouped, so navigation must too.
        let mut app = make_app(5);
        app.repos[0].location = RepoLocation::LocalOnly;
        app.repos[1].location = RepoLocation::Primary;
        app.repos[2].location = RepoLocation::NotCloned;
        app.repos[3].location = RepoLocation::LocalOnly;
        app.repos[4].location = RepoLocation::Primary;

        let order: Vec<RepoLocation> = app
            .visible_repos()
            .iter()
            .map(|(_, r)| r.location.clone())
            .collect();
        assert_eq!(
            order,
            vec![
                RepoLocation::Primary,
                RepoLocation::Primary,
                RepoLocation::LocalOnly,
                RepoLocation::LocalOnly,
                RepoLocation::NotCloned,
            ]
        );

        // Stability: within a section, assembly order is preserved.
        let names: Vec<&str> = app
            .visible_repos()
            .iter()
            .map(|(_, r)| r.name.as_str())
            .collect();
        assert_eq!(names[0], "repo-1");
        assert_eq!(names[1], "repo-4");
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
        let parsed = parse_query("repo #dirty");
        assert_eq!(parsed.text, "repo");
        assert!(parsed.filters.dirty);
        assert!(!parsed.filters.behind);
        assert!(parsed.unknown_tags.is_empty());
    }

    #[test]
    fn parse_query_bare_hash_is_inert() {
        // It used to activate all six filters, which AND'd to an empty list --
        // and since "#" is the first keystroke of every tag, the list blanked
        // on the way to typing any of them.
        let parsed = parse_query("#");
        assert_eq!(parsed.text, "");
        assert_eq!(parsed.filters, QuickFilters::default());
        assert!(parsed.unknown_tags.is_empty(), "incomplete, not wrong");
    }

    #[test]
    fn parse_query_reports_an_unknown_tag_instead_of_swallowing_it() {
        let parsed = parse_query("#nonsense hello");
        assert_eq!(parsed.text, "hello", "still not treated as literal text");
        assert_eq!(parsed.filters, QuickFilters::default());
        assert_eq!(parsed.unknown_tags, vec!["nonsense".to_string()]);
    }

    #[test]
    fn fuzzy_search_matches_a_non_substring_query() {
        let mut app = make_app(2);
        app.repos[0].name = "fleetz".to_string();
        app.repos[1].name = "notez2".to_string();
        app.search_query = "flz".to_string();

        let visible = app.visible_repos();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].1.name, "fleetz");
    }

    #[test]
    fn search_results_are_ranked_best_match_first() {
        // The weaker match sits earlier in `repos` and in an earlier section,
        // so only scoring can put the exact hit on top.
        let mut app = make_app(2);
        app.repos[0].name = "pinz-board-archive".to_string();
        app.repos[1].name = "pinz".to_string();
        app.repos[1].location = RepoLocation::LocalOnly;
        app.search_query = "pinz".to_string();

        let visible = app.visible_repos();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].1.name, "pinz", "exact match ranks first");
    }

    #[test]
    fn section_order_returns_once_the_query_is_cleared() {
        let mut app = make_app(2);
        app.repos[0].location = RepoLocation::LocalOnly;
        app.repos[1].location = RepoLocation::Primary;

        assert!(!app.has_text_query());
        let visible = app.visible_repos();
        assert_eq!(
            visible[0].1.location,
            RepoLocation::Primary,
            "github activity leads when nothing is typed"
        );
    }

    /// "local changes" is a worklist -- uncommitted work on this machine --
    /// while "also active" is other people's repos you happen to have. The
    /// worklist reads first.
    #[test]
    fn local_changes_sorts_above_also_active() {
        let mut app = make_app(3);
        app.repos[0].location = RepoLocation::Elsewhere;
        app.repos[1].location = RepoLocation::LocalOnly;
        app.repos[2].location = RepoLocation::Primary;

        let order: Vec<RepoLocation> = app
            .visible_repos()
            .iter()
            .map(|(_, r)| r.location.clone())
            .collect();
        assert_eq!(
            order,
            vec![
                RepoLocation::Primary,
                RepoLocation::LocalOnly,
                RepoLocation::Elsewhere,
            ]
        );
    }

    /// A not-cloned repo shares "also active" with `Elsewhere`, so it has to
    /// move with it. Ranking them apart would split one drawn section in two.
    #[test]
    fn not_cloned_stays_with_also_active() {
        let mut app = make_app(2);
        app.repos[0].location = RepoLocation::NotCloned;
        app.repos[1].location = RepoLocation::LocalOnly;

        let order: Vec<RepoLocation> = app
            .visible_repos()
            .iter()
            .map(|(_, r)| r.location.clone())
            .collect();
        assert_eq!(
            order,
            vec![RepoLocation::LocalOnly, RepoLocation::NotCloned]
        );
    }

    #[test]
    fn tag_only_query_keeps_section_order() {
        // "#dirty" has no free text to rank by, so the list must not switch
        // into ranked mode (which would also drop the section headers).
        let mut app = make_app(2);
        app.repos[0].dirty = 1;
        app.repos[1].dirty = 1;
        app.search_query = "#dirty".to_string();
        assert!(!app.has_text_query());
        assert_eq!(app.visible_repos().len(), 2);
    }

    fn settings() -> SettingsOverlay {
        SettingsOverlay::new(
            60,
            30,
            true,
            None,
            9,
            18,
            false,
            ContributionsMode::Off,
            true,
        )
    }

    /// Put the cursor on a field by name rather than by number, so inserting a
    /// row does not silently retarget every test that follows it.
    fn focus(s: &mut SettingsOverlay, field: SettingField) {
        s.cursor = SETTING_FIELDS
            .iter()
            .position(|f| *f == field)
            .expect("field is in SETTING_FIELDS");
        assert_eq!(s.field(), field);
    }

    #[test]
    fn settings_cursor_wraps_both_ways() {
        // The last row is derived rather than named: this test is about the
        // wrap, and adding a settings row should not break it.
        let last = *SETTING_FIELDS.last().expect("at least one setting");
        let mut s = settings();
        assert_eq!(s.field(), SettingField::RefreshInterval);
        s.move_cursor(-1);
        assert_eq!(s.field(), last, "wraps to the end");
        s.move_cursor(1);
        assert_eq!(s.field(), SettingField::RefreshInterval);
    }

    #[test]
    fn settings_interval_clamps_to_its_range() {
        let mut s = settings();
        for _ in 0..100 {
            s.step(-1);
        }
        assert_eq!(s.refresh_interval, 5, "never reaches zero");
        for _ in 0..2000 {
            s.step(1);
        }
        assert_eq!(s.refresh_interval, 3600);
    }

    #[test]
    fn settings_view_cycles_through_auto() {
        let mut s = settings();
        assert_eq!(s.view, None);
        focus(&mut s, SettingField::View);
        s.step(1);
        assert_eq!(s.view, Some(View::Detail));
        s.step(1);
        assert_eq!(s.view, Some(View::Compact));
        s.step(1);
        assert_eq!(s.view, None, "wraps back to auto");
        s.step(-1);
        assert_eq!(s.view, Some(View::Compact), "and cycles backwards");
    }

    #[test]
    fn settings_work_hours_stay_ordered() {
        // The pair must never invert or collapse: an empty work slot would
        // make the time-slot labels nonsense.
        let mut s = settings();
        focus(&mut s, SettingField::WorkStart);
        for _ in 0..40 {
            s.step(1);
        }
        assert!(s.work_start < s.work_end);
        assert_eq!(s.work_start, 17, "stops one below work_end");

        focus(&mut s, SettingField::WorkEnd);
        for _ in 0..40 {
            s.step(-1);
        }
        assert!(s.work_start < s.work_end);
        assert_eq!(s.work_end, 18, "stops one above work_start");
    }

    #[test]
    fn settings_digits_type_into_numeric_fields_only() {
        let mut s = settings();
        s.push_digit(3);
        s.push_digit(0);
        assert_eq!(s.refresh_interval, 30, "'3' then '0' reads as 30");

        focus(&mut s, SettingField::View); // not numeric
        let before = s.view;
        s.push_digit(7);
        assert_eq!(
            s.view, before,
            "digits are ignored, not swallowed as a step"
        );
    }

    #[test]
    fn settings_normalize_lifts_a_half_typed_value_to_its_floor() {
        // Typing "30" passes through "3", which is below INTERVAL_MIN. The
        // intermediate state is allowed; applying it is not.
        let mut s = settings();
        s.push_digit(3);
        assert_eq!(s.refresh_interval, 3);
        s.normalize();
        assert_eq!(s.refresh_interval, 5);
    }

    #[test]
    fn settings_poll_interval_has_its_own_floor() {
        // Lower than the sweep floor on purpose: a poll is one GraphQL call,
        // a sweep is a fetch against every remote.
        let mut s = settings();
        focus(&mut s, SettingField::GithubPollInterval);
        for _ in 0..100 {
            s.step(-1);
        }
        assert_eq!(s.github_poll_interval, POLL_MIN);
        // And the sweep floor is untouched by it: two separate limits, because
        // the two operations cost very different things.
        assert_eq!(s.refresh_interval, 60);
    }

    #[test]
    fn settings_watch_is_a_restart_field() {
        // `w` changes the live state; this row only sets what the next launch
        // starts with, and the overlay says so rather than looking inert.
        assert_eq!(SettingField::Watch.applies(), Applies::Restart);
        let mut s = settings();
        focus(&mut s, SettingField::Watch);
        let before = s.watch;
        s.step(1);
        assert_ne!(s.watch, before, "toggles");
    }

    #[test]
    fn settings_write_every_field_back() {
        use crate::config::SettingValue;
        let mut s = settings();
        s.refresh_interval = 90;
        s.view = Some(View::Compact);
        s.persist_panes = true;

        let updates = s.config_updates();
        let by_key = |k: &str| {
            updates
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(by_key("refresh_interval"), Some(SettingValue::Int(90)));
        assert_eq!(by_key("github_poll_interval"), Some(SettingValue::Int(30)));
        assert_eq!(by_key("watch"), Some(SettingValue::Bool(true)));
        assert_eq!(by_key("view"), Some(SettingValue::Str("compact")));
        assert_eq!(by_key("work_start"), Some(SettingValue::Int(9)));
        assert_eq!(by_key("persist_pane_sizes"), Some(SettingValue::Bool(true)));
    }

    #[test]
    fn the_blink_actually_flips_between_frames() {
        // Guards a parity bug rather than the flip itself: repaints for
        // watch-mode animation happen every SLOW_REPAINT_TICKS, and if that is
        // an even multiple of BLINK_TICKS the blink is drawn in the same phase
        // every time and never appears to blink. Anything odd-multiple, or
        // equal, is fine.
        let repaint_every = crate::SLOW_REPAINT_TICKS;
        let flips_per_repaint = repaint_every / BLINK_TICKS;
        assert_eq!(
            repaint_every % BLINK_TICKS,
            0,
            "repaints should land on a blink boundary"
        );
        assert!(
            flips_per_repaint % 2 == 1,
            "an even number of flips between repaints draws the same phase forever \
             (repaint every {repaint_every} ticks, blink every {BLINK_TICKS})"
        );
    }

    #[test]
    fn on_tick_flips_the_blink_on_its_period() {
        let mut app = make_app(1);
        let start = app.blink_on;
        for _ in 0..BLINK_TICKS {
            app.on_tick();
        }
        assert_ne!(app.blink_on, start, "flips after one period");
        for _ in 0..BLINK_TICKS {
            app.on_tick();
        }
        assert_eq!(app.blink_on, start, "and back after two");
    }

    #[test]
    fn scrolling_stops_at_the_measured_end_of_content() {
        // The overshoot bug: the offset used to climb to a fixed cap of 300
        // regardless of content, so scrolling back needed one dead keypress
        // per step of overshoot before the view moved at all.
        let mut app = make_app(1);
        app.detail_max_scroll.set(12);

        for _ in 0..100 {
            app.scroll_detail(1);
        }
        assert_eq!(app.detail_scroll(), 12, "never runs past the content");

        // The very next scroll back must move the view, not burn off phantom
        // offset.
        app.scroll_detail(-1);
        assert_eq!(app.detail_scroll(), 11);
    }

    #[test]
    fn scrolling_up_stops_at_the_top() {
        let mut app = make_app(1);
        app.inspector_max_scroll.set(5);
        app.scroll_inspector(3);
        assert_eq!(app.inspector_scroll, 3);

        for _ in 0..50 {
            app.scroll_inspector(-1);
        }
        assert_eq!(app.inspector_scroll, 0);
        app.scroll_inspector(1);
        assert_eq!(app.inspector_scroll, 1, "moves again immediately");
    }

    #[test]
    fn a_pane_with_no_overflow_does_not_scroll() {
        // Content shorter than the pane: max is 0, so every scroll is a no-op
        // rather than silently accumulating offset.
        let mut app = make_app(1);
        app.detail_max_scroll.set(0);
        app.scroll_detail(10);
        assert_eq!(app.detail_scroll(), 0);
    }

    #[test]
    fn page_scrolling_clamps_the_same_way() {
        let mut app = make_app(1);
        app.detail_max_scroll.set(4);
        app.scroll_detail(10); // one PageDown
        assert_eq!(app.detail_scroll(), 4);
        app.scroll_detail(-10); // one PageUp
        assert_eq!(app.detail_scroll(), 0);
    }

    #[test]
    fn pane_resize_clamps_and_resets() {
        let mut app = make_app(1);
        assert_eq!(app.repo_list_pct, DEFAULT_REPO_LIST_PCT);

        for _ in 0..50 {
            app.resize_repo_list(-1);
        }
        assert_eq!(app.repo_list_pct, REPO_LIST_PCT_MIN, "never collapses");
        for _ in 0..50 {
            app.resize_repo_list(1);
        }
        assert_eq!(app.repo_list_pct, REPO_LIST_PCT_MAX);

        for _ in 0..50 {
            app.resize_inspector(1);
        }
        assert_eq!(app.inspector_pct, INSPECTOR_PCT_MAX);

        app.reset_pane_sizes();
        assert_eq!(app.repo_list_pct, DEFAULT_REPO_LIST_PCT);
        assert_eq!(app.inspector_pct, DEFAULT_INSPECTOR_PCT);
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

    fn today() -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str("2026-08-18", "%Y-%m-%d").unwrap()
    }

    #[test]
    fn c_toggles_the_pane_and_remembers_the_source() {
        let mut app = make_app(1);
        assert_eq!(app.contributions, ContributionsMode::Off);
        app.toggle_contributions();
        assert_eq!(
            app.contributions,
            ContributionsMode::Github,
            "defaults to github"
        );
        app.cycle_contributions_source();
        assert_eq!(app.contributions, ContributionsMode::Local);
        app.toggle_contributions();
        assert_eq!(app.contributions, ContributionsMode::Off);
        app.toggle_contributions();
        assert_eq!(
            app.contributions,
            ContributionsMode::Local,
            "reopens where it closed"
        );
    }

    #[test]
    fn cycling_the_source_on_a_closed_pane_opens_it() {
        let mut app = make_app(1);
        app.cycle_contributions_source();
        assert!(app.contributions.is_open());
    }

    #[test]
    fn closing_the_pane_moves_focus_off_it() {
        let mut app = make_app(1);
        app.toggle_contributions();
        app.focus_panel(FocusedPanel::Contributions);
        assert_eq!(app.focused_panel, FocusedPanel::Contributions);
        app.toggle_contributions();
        assert_eq!(
            app.focused_panel,
            FocusedPanel::RepoList,
            "focus cannot strand"
        );
    }

    #[test]
    fn the_pane_is_only_focusable_while_open() {
        let mut app = make_app(1);
        app.focus_panel(FocusedPanel::Contributions);
        assert_eq!(app.focused_panel, FocusedPanel::RepoList, "closed: refused");
        app.toggle_contributions();
        app.focus_panel(FocusedPanel::Contributions);
        assert_eq!(app.focused_panel, FocusedPanel::Contributions);
    }

    /// Unlike the other three panels, this one exists in Compact view, so the
    /// Detail-only guard on `focus_panel` must not apply to it.
    #[test]
    fn the_pane_is_focusable_in_compact_view() {
        let mut app = make_app(1);
        app.view = View::Compact;
        app.toggle_contributions();
        app.focus_panel(FocusedPanel::Contributions);
        assert_eq!(app.focused_panel, FocusedPanel::Contributions);
        app.focus_panel(FocusedPanel::Detail);
        assert_eq!(
            app.focused_panel,
            FocusedPanel::Contributions,
            "detail is still unreachable in compact"
        );
    }

    #[test]
    fn tab_includes_the_pane_only_while_open() {
        let mut app = make_app(1);
        app.view = View::Detail;
        app.toggle_panel_focus();
        app.toggle_panel_focus();
        app.toggle_panel_focus();
        assert_eq!(
            app.focused_panel,
            FocusedPanel::RepoList,
            "closed: three panels"
        );

        app.toggle_contributions();
        app.focused_panel = FocusedPanel::Inspector;
        app.toggle_panel_focus();
        assert_eq!(app.focused_panel, FocusedPanel::Contributions);
        app.toggle_panel_focus();
        assert_eq!(app.focused_panel, FocusedPanel::RepoList);
    }

    #[test]
    fn tab_in_compact_toggles_between_the_list_and_the_pane() {
        let mut app = make_app(1);
        app.view = View::Compact;
        app.toggle_panel_focus();
        assert_eq!(app.focused_panel, FocusedPanel::RepoList, "closed: no-op");
        app.toggle_contributions();
        app.toggle_panel_focus();
        assert_eq!(app.focused_panel, FocusedPanel::Contributions);
        app.toggle_panel_focus();
        assert_eq!(app.focused_panel, FocusedPanel::RepoList);
    }

    /// The grid reads as columns of weeks and rows of weekdays, so the keys
    /// must move that way: left/right change the week, up/down change the
    /// weekday. They were wired the other way round at first, which moved the
    /// cursor at right angles to what the eye expected.
    #[test]
    fn left_right_walks_weeks_and_up_down_walks_weekdays() {
        let mut app = make_app(1);
        let last = app.contrib_last_slot(today());
        app.contrib_cursor = Some(last);

        app.move_contrib_cursor(-1, 0, today());
        assert_eq!(
            app.contrib_cursor,
            Some(last - 7),
            "left is one column back, same weekday"
        );

        app.move_contrib_cursor(0, -1, today());
        assert_eq!(
            app.contrib_cursor,
            Some(last - 8),
            "up is one row, same column"
        );

        app.move_contrib_cursor(1, 0, today());
        assert_eq!(app.contrib_cursor, Some(last - 1), "right returns");
    }

    /// Nothing is marked until the user actually travels the grid: a
    /// highlighted cell nobody asked for reads as a selection you did not
    /// make.
    #[test]
    fn no_day_is_marked_until_the_user_moves() {
        let mut app = make_app(1);
        app.toggle_contributions();
        assert_eq!(app.contrib_cursor, None, "opening marks nothing");
        app.focus_panel(FocusedPanel::Contributions);
        assert_eq!(app.contrib_cursor, None, "focusing marks nothing");

        // The first keypress lands on today rather than jumping from nowhere.
        app.move_contrib_cursor(-1, 0, today());
        assert_eq!(app.contrib_cursor, Some(app.contrib_last_slot(today())));

        // And it goes back to unmarked when the pane is reopened.
        app.toggle_contributions();
        app.toggle_contributions();
        assert_eq!(app.contrib_cursor, None);
    }

    /// Walking up a column must not spill into the neighbouring week, and
    /// walking right must not walk into the future.
    #[test]
    fn each_axis_clamps_without_spilling_into_the_other() {
        let mut app = make_app(1);
        let last = app.contrib_last_slot(today());
        let (last_week, last_weekday) = (last / 7, last % 7);

        // Top of a column: further up is a no-op, not a jump to last week.
        app.contrib_cursor = Some(last_week * 7);
        app.move_contrib_cursor(0, -1, today());
        assert_eq!(app.contrib_cursor, Some(last_week * 7));

        // 2026-08-18 is a Tuesday, so the newest column ends there.
        app.contrib_cursor = Some(last);
        app.move_contrib_cursor(0, 1, today());
        assert_eq!(
            app.contrib_cursor,
            Some(last),
            "cannot walk into the future"
        );

        app.move_contrib_cursor(1, 0, today());
        assert_eq!(app.contrib_cursor, Some(last), "no column after this one");

        // Entering the short final column from a later weekday slides up to
        // today rather than landing on a cell that does not exist.
        app.contrib_cursor = Some((last_week - 1) * 7 + 6);
        app.move_contrib_cursor(1, 0, today());
        assert_eq!(app.contrib_cursor, Some(last_week * 7 + last_weekday));

        // The oldest column is a hard left edge.
        app.contrib_cursor = Some(3);
        app.move_contrib_cursor(-1, 0, today());
        assert_eq!(app.contrib_cursor, Some(3));
    }

    #[test]
    fn home_and_end_jump_to_the_ends_keeping_the_weekday() {
        let mut app = make_app(1);
        let last = app.contrib_last_slot(today());
        app.contrib_cursor = Some(20 * 7 + 4); // a Thursday mid-year

        app.jump_contrib_cursor(false, today());
        assert_eq!(app.contrib_cursor, Some(4), "oldest column, same weekday");

        app.jump_contrib_cursor(true, today());
        assert_eq!(
            app.contrib_cursor,
            Some(last),
            "newest column clamps to today"
        );
    }

    /// The grid is wider than the terminal, so moving off the visible window
    /// must bring it back rather than losing the cursor off-screen.
    #[test]
    fn moving_off_the_window_scrolls_the_grid() {
        let mut app = make_app(1);
        app.contrib_visible_weeks.set(10);
        app.contrib_scroll.set(40);
        app.contrib_cursor = Some(40 * 7);

        app.move_contrib_cursor(-1, 0, today());
        assert_eq!(app.contrib_scroll.get(), 39, "scrolled one column left");

        app.contrib_scroll.set(0);
        app.contrib_cursor = Some(9 * 7);
        app.move_contrib_cursor(1, 0, today());
        assert_eq!(app.contrib_scroll.get(), 1, "scrolled one column right");
    }

    #[test]
    fn scrolling_clamps_to_the_ends_of_the_year() {
        let mut app = make_app(1);
        app.contrib_visible_weeks.set(10);
        app.contrib_scroll.set(0);

        app.scroll_contributions(-5);
        assert_eq!(app.contrib_scroll.get(), 0, "cannot scroll before the year");

        app.scroll_contributions(1000);
        assert_eq!(
            app.contrib_scroll.get(),
            crate::contributions::WEEKS - 10,
            "cannot scroll past the newest week"
        );
    }

    /// Opening the strip pins the newest weeks to the right edge: the grid
    /// should open on now, not on last August. The request is deliberately
    /// past the end, because only the renderer knows how many weeks fit; it
    /// clamps this down on the next frame.
    #[test]
    fn opening_the_strip_asks_for_the_newest_weeks() {
        let mut app = make_app(1);
        app.contrib_scroll.set(0);
        app.toggle_contributions();
        assert!(
            app.contrib_scroll.get() >= crate::contributions::WEEKS,
            "an out-of-range request is what anchors the grid right"
        );
    }

    #[test]
    fn the_local_calendar_sums_only_the_visible_repos() {
        let mut app = make_app(0);
        let mut a = crate::contributions::DailyCounts::default();
        a.0[0] = 3;
        let mut b = crate::contributions::DailyCounts::default();
        b.0[0] = 4;
        let mut alpha = make_app(1).repos.remove(0);
        alpha.name = "alpha".into();
        alpha.daily = Some(Box::new(a));
        let mut beta = make_app(1).repos.remove(0);
        beta.name = "beta".into();
        beta.daily = Some(Box::new(b));
        app.repos = vec![alpha, beta];
        assert_eq!(app.local_calendar(today()).total(), 7, "both visible");

        app.search_query = "alpha".to_string();
        assert_eq!(
            app.local_calendar(today()).total(),
            3,
            "a search narrows the heatmap the same way it narrows the list"
        );
    }

    #[test]
    fn a_repo_with_no_histogram_contributes_nothing() {
        let app = make_app(1);
        assert_eq!(app.local_calendar(today()).total(), 0);
        assert_eq!(
            app.local_calendar(today()).source(),
            crate::contributions::Source::Local
        );
    }

    #[test]
    fn the_settings_row_cycles_the_three_modes() {
        let mut overlay = SettingsOverlay::new(
            60,
            60,
            true,
            None,
            9,
            18,
            false,
            ContributionsMode::Off,
            true,
        );
        overlay.cursor = SETTING_FIELDS
            .iter()
            .position(|f| *f == SettingField::Contributions)
            .unwrap();
        assert_eq!(overlay.value_label(SettingField::Contributions), "off");
        overlay.step(1);
        assert_eq!(overlay.value_label(SettingField::Contributions), "github");
        overlay.step(1);
        assert_eq!(overlay.value_label(SettingField::Contributions), "local");
        overlay.step(1);
        assert_eq!(
            overlay.value_label(SettingField::Contributions),
            "off",
            "wraps"
        );
        overlay.step(-1);
        assert_eq!(
            overlay.value_label(SettingField::Contributions),
            "local",
            "wraps back"
        );
    }

    #[test]
    fn the_settings_row_writes_the_contributions_key() {
        assert_eq!(SettingField::Contributions.config_key(), "contributions");
        assert_eq!(SettingField::Contributions.applies(), Applies::Live);
        let overlay = SettingsOverlay::new(
            60,
            60,
            true,
            None,
            9,
            18,
            false,
            ContributionsMode::Local,
            true,
        );
        let updates = overlay.config_updates();
        assert!(updates.iter().any(|(k, v)| *k == "contributions"
            && matches!(v, crate::config::SettingValue::Str("local"))));
    }

    /// A failed initial load used to set only a status message, which the
    /// next keypress clears. The repo list is derived from the GitHub dataset,
    /// so the result was a blank dashboard with no stated cause -- identical
    /// to a fleet with nothing in the window.
    #[test]
    fn an_empty_fleet_with_a_failed_load_explains_itself() {
        let mut app = make_app(0);
        assert!(!app.shows_load_error(), "nothing has failed yet");
        app.load_error = Some("rate limited".to_string());
        assert!(app.shows_load_error());
    }

    /// The guard's other half: a reload that fails while rows are on screen
    /// must keep them. Replacing live data with an error panel loses more
    /// than it explains.
    #[test]
    fn a_failed_reload_keeps_a_populated_fleet_on_screen() {
        let mut app = make_app(3);
        app.load_error = Some("rate limited".to_string());
        assert!(
            !app.shows_load_error(),
            "the error panel would have replaced three good rows"
        );
    }

    /// Scrolling and the cursor are independent: the wheel used to call
    /// `detail_nav_down`, so it moved the selection instead of the view and
    /// the pane could only jump from commit to commit.
    ///
    /// Asserted against `commits` rather than `graph`: the graph view now
    /// drags its marker with the view deliberately (see below), and it is the
    /// only view that does.
    #[test]
    fn scrolling_the_detail_pane_leaves_the_selection_alone() {
        let mut app = make_app(3);
        app.history_view = HistoryView::Commits;
        app.detail_max_scroll.set(20);
        app.set_detail_selected(2);
        let before = app.detail_selected();
        app.scroll_detail(5);
        app.scroll_detail(-2);
        assert_eq!(app.detail_selected(), before, "scrolling moved the cursor");
    }

    /// The graph view is the exception, and its cursor is a *row*. Blocks run
    /// past twenty rows, so a commit-granular marker stood still through a
    /// whole commit and then jumped -- the highlight was nowhere while you
    /// read one. The cursor walks the pane a row at a time instead.
    fn graph_app() -> App {
        let mut app = make_app(1);
        app.history_view = HistoryView::Graph;
        app.detail_view_height.set(20);
        app.detail_max_scroll.set(100);
        *app.detail_row_tops.borrow_mut() = vec![0, 10, 25, 60];
        app
    }

    /// Scrolloff: the highlight travels down the pane while the text holds
    /// still, and once it is `SCROLLOFF` from the bottom the text starts
    /// moving under it instead.
    #[test]
    fn the_graph_cursor_walks_the_pane_before_the_pane_moves() {
        let mut app = graph_app();

        for _ in 0..15 {
            app.scroll_detail(1);
        }
        assert_eq!(app.graph_cursor, 15, "one row per press");
        assert_eq!(
            app.detail_scroll(),
            0,
            "the text moved while the highlight still had room"
        );

        app.scroll_detail(1);
        assert_eq!(app.graph_cursor, 16);
        assert_eq!(app.detail_scroll(), 1, "now the text moves under it");

        for _ in 0..5 {
            app.scroll_detail(1);
        }
        assert_eq!((app.graph_cursor, app.detail_scroll()), (21, 6));
        assert_eq!(
            app.graph_cursor - app.detail_scroll(),
            15,
            "the highlight is parked four rows off the bottom"
        );
    }

    /// The same margin at the other edge, walking back up.
    #[test]
    fn the_graph_cursor_keeps_its_margin_going_up() {
        let mut app = graph_app();
        for _ in 0..21 {
            app.scroll_detail(1);
        }
        assert_eq!((app.graph_cursor, app.detail_scroll()), (21, 6));

        for _ in 0..11 {
            app.scroll_detail(-1);
        }
        assert_eq!(app.graph_cursor, 10);
        assert_eq!(
            app.detail_scroll(),
            6,
            "the highlight came up, not the text"
        );

        app.scroll_detail(-1);
        assert_eq!((app.graph_cursor, app.detail_scroll()), (9, 5));
    }

    /// The cursor cannot walk off the end of the content, and does not bank
    /// phantom offset there either -- one press back must move it.
    #[test]
    fn the_graph_cursor_stops_at_the_last_row() {
        let mut app = graph_app();
        app.scroll_detail(10_000);
        // max_scroll + height - 1: the last row the pane can show.
        assert_eq!(app.graph_cursor, 119);
        app.scroll_detail(-1);
        assert_eq!(app.graph_cursor, 118, "moves again immediately");
    }

    /// The commit the cursor is inside is what the view calls selected, so the
    /// hash emphasis and `inspected_hash` follow the highlight through a block
    /// rather than waiting for its last row.
    #[test]
    fn the_cursor_selects_the_commit_it_is_inside() {
        let mut app = graph_app();
        app.scroll_detail(9);
        assert_eq!(app.detail_selected(), 0);
        app.scroll_detail(1);
        assert_eq!(app.detail_selected(), 1, "row 10 opens the second commit");
        app.scroll_detail(20);
        assert_eq!(app.detail_selected(), 2);
    }

    /// Stepping a commit shows its block from the top. Bringing the row into
    /// view by the smallest nudge would leave it in the *bottom* margin: four
    /// rows of the commit you stepped to, and the rest of it below the fold.
    #[test]
    fn stepping_a_commit_shows_its_block_from_the_top() {
        let mut app = graph_app();

        app.set_detail_selected(2);
        assert_eq!(app.graph_cursor, 25, "the cursor moved to the commit's row");
        assert_eq!(
            app.detail_scroll(),
            21,
            "which sits four rows down the pane"
        );

        // Already comfortably on screen: stepping must not jerk the view.
        let before = app.detail_scroll();
        app.set_detail_selected(2);
        assert_eq!(app.detail_scroll(), before);
    }

    /// Moving the cursor must not ask the pane to follow it: the next frame
    /// would bring the whole block into view and undo the scrolloff the cursor
    /// just established.
    #[test]
    fn moving_the_graph_cursor_does_not_request_follow() {
        let mut app = graph_app();
        app.follow_selection.set(false);
        app.scroll_detail(12);
        assert_eq!(app.detail_selected(), 1);
        assert!(
            !app.follow_selection.get(),
            "the cursor asked the renderer to follow, which would undo it"
        );
    }

    /// From the repo list, Shift+J/K drives window 2's cursor rather than its
    /// scroll offset: `j`/`k` there belong to the repo list, so this is the
    /// only way to step commits without leaving window 1, and stepping the
    /// cursor is what makes the inspector read each one out. It used to scroll
    /// the pane, which moved the text past a cursor that stayed put and told
    /// the inspector nothing.
    #[test]
    fn shift_scroll_from_the_repo_list_steps_the_detail_cursor() {
        let mut app = make_app(1);
        // The graph, because that is the list `info` draws and therefore the
        // one its cursor is bounded by.
        app.repos[0].graph = (0..6)
            .map(|i| crate::model::GraphLine {
                lanes: vec![crate::graph::commit_cell()],
                hash: format!("c0ffee{i}"),
                is_merge: false,
                refs: vec![],
                author: "Gaurgle".to_string(),
                message: format!("feat: thing {i}"),
            })
            .collect();
        app.view = View::Detail;
        app.focused_panel = FocusedPanel::RepoList;
        app.history_view = HistoryView::Info;
        app.detail_view_height.set(20);
        app.detail_max_scroll.set(100);
        // Six commits, as `info` shows.
        *app.detail_row_tops.borrow_mut() = (0..6).collect();

        app.scroll_next_panel(1);
        assert_eq!(app.detail_selected(), 1, "the inspector was told nothing");
        app.scroll_next_panel(1);
        assert_eq!(app.detail_selected(), 2);
        app.scroll_next_panel(-1);
        assert_eq!(app.detail_selected(), 1);
    }

    /// Past the last commit it falls through to scrolling, so the branches and
    /// stashes below a short list stay reachable from the repo list too.
    #[test]
    fn shift_scroll_past_the_last_commit_scrolls_instead() {
        let mut app = make_app(1);
        app.view = View::Detail;
        app.focused_panel = FocusedPanel::RepoList;
        app.history_view = HistoryView::Info;
        app.detail_view_height.set(20);
        app.detail_max_scroll.set(100);
        *app.detail_row_tops.borrow_mut() = (0..6).collect();

        // `make_app` builds repos with no commits, so the cursor is already at
        // the end of the list and every step is a scroll.
        assert_eq!(app.selected_commit_count(), 0);
        app.scroll_next_panel(1);
        assert_eq!(app.detail_scroll(), 1);
    }

    /// From the detail pane Shift+scroll reaches past it into the inspector,
    /// so the commit under the cursor can be read further down without leaving
    /// the pane that chose it.
    ///
    /// It used to scroll the detail pane itself. `PageUp`/`PageDown` still do
    /// that, so the gesture was not lost, only moved off a key that now has
    /// somewhere better to point.
    #[test]
    fn shift_scroll_from_the_detail_pane_drives_the_inspector() {
        let mut app = make_app(1);
        app.view = View::Detail;
        app.focused_panel = FocusedPanel::Detail;
        app.history_view = HistoryView::Commits;
        app.detail_view_height.set(20);
        app.detail_max_scroll.set(100);
        *app.detail_row_tops.borrow_mut() = (0..40).collect();
        assert!(app.shows_inspector(), "no inspector to aim at");
        assert_eq!(
            app.shift_scroll_target(),
            Some(FocusedPanel::Inspector),
            "the detail pane does not aim at the inspector"
        );

        // The inspector's own offset is clamped against its content, so give
        // it something to scroll before asking whether it moved.
        app.inspector_max_scroll.set(10);
        let scroll_before = app.detail_scroll();
        app.scroll_next_panel(1);
        assert_eq!(app.inspector_scroll, 1, "the inspector did not scroll");
        assert_eq!(
            app.detail_scroll(),
            scroll_before,
            "the detail pane scrolled itself instead of the pane after it"
        );
    }

    /// In the graph the shifted keys stay on the pane and walk its *rows*,
    /// one line at a time, where plain `j`/`k` jumps whole commit blocks. The
    /// graph is the only view whose rows are finer than its commits, so it is
    /// the only one with a line-by-line step to offer.
    #[test]
    fn shift_scroll_walks_graph_rows_one_line_at_a_time() {
        let mut app = make_app(1);
        app.view = View::Detail;
        app.focused_panel = FocusedPanel::Detail;
        app.history_view = HistoryView::Graph;
        app.detail_view_height.set(20);
        app.detail_max_scroll.set(100);

        assert_eq!(
            app.shift_scroll_target(),
            Some(FocusedPanel::Detail),
            "the graph aimed somewhere other than itself"
        );

        assert_eq!(app.graph_cursor, 0);
        app.scroll_next_panel(1);
        assert_eq!(app.graph_cursor, 1, "did not step a single row");
        app.scroll_next_panel(1);
        assert_eq!(app.graph_cursor, 2);
        app.scroll_next_panel(-1);
        assert_eq!(app.graph_cursor, 1, "did not step back a single row");
    }

    /// ...and it does not reach pane 1 to do it. The graph has no inspector to
    /// pass the gesture on to, and wrapping to the repo list would have spent
    /// the only line-by-line step the graph has on changing repo.
    #[test]
    fn shift_scroll_in_the_graph_leaves_the_repo_list_alone() {
        let mut app = make_app(3);
        app.view = View::Detail;
        app.focused_panel = FocusedPanel::Detail;
        app.history_view = HistoryView::Graph;
        app.detail_view_height.set(20);
        app.detail_max_scroll.set(100);

        let repo_before = app.selected;
        app.scroll_next_panel(1);
        app.scroll_next_panel(1);
        assert_eq!(
            app.selected, repo_before,
            "the graph's shifted step changed which repo is selected"
        );
    }

    /// The strip is never a target: it is a fixed-height horizontal timeline
    /// with nothing below the fold, so it is skipped rather than left in the
    /// cycle as a stop where the key would do nothing.
    #[test]
    fn shift_scroll_never_aims_at_the_contributions_strip() {
        let mut app = make_app(1);
        app.view = View::Detail;
        app.open_contributions(ContributionsMode::Github);
        for focused in [
            FocusedPanel::RepoList,
            FocusedPanel::Detail,
            FocusedPanel::Inspector,
            FocusedPanel::Contributions,
        ] {
            app.focused_panel = focused;
            assert_ne!(
                app.shift_scroll_target(),
                Some(FocusedPanel::Contributions),
                "{focused:?} aimed at the strip"
            );
        }
    }

    /// Compact has one flat grid and no pane after the list, so the key has
    /// nothing to aim at rather than quietly driving a pane that is not drawn.
    #[test]
    fn shift_scroll_has_no_target_in_compact() {
        let mut app = make_app(1);
        app.view = View::Compact;
        app.focused_panel = FocusedPanel::RepoList;
        assert_eq!(app.shift_scroll_target(), None);
    }

    /// Scrolling leaves the selection with the content it is attached to.
    ///
    /// It used to drag the selection to the nearest visible entry on every
    /// wheel notch, which meant scrolling silently reassigned which commit the
    /// inspector was reading out: the highlight crept down the pane on its own
    /// and the panel below it changed under you. Deliberately reversed --
    /// scrolling is a view gesture, not a selection one.
    #[test]
    fn scrolling_does_not_move_the_selection() {
        let mut app = make_app(1);
        app.history_view = HistoryView::Commits;
        app.detail_view_height.set(20);
        app.detail_max_scroll.set(100);
        // One row per commit, as the list draws them.
        *app.detail_row_tops.borrow_mut() = (0..40).collect();

        let before = app.detail_selected();
        app.scroll_detail(10);
        assert_eq!(
            app.detail_selected(),
            before,
            "the wheel moved the selection"
        );
        app.scroll_detail(20);
        assert_eq!(
            app.detail_selected(),
            before,
            "the wheel moved the selection"
        );
        app.scroll_detail(-5);
        assert_eq!(
            app.detail_selected(),
            before,
            "the wheel moved the selection"
        );
    }

    /// ...and the first step afterwards re-enters at the edge you are looking
    /// at, rather than carrying on from a row that is no longer on screen.
    /// That step is spent on the re-entry, so the highlight appears where the
    /// eye already is before it starts moving again.
    #[test]
    fn the_first_step_after_a_scroll_re_enters_the_pane() {
        let mut app = make_app(1);
        app.history_view = HistoryView::Commits;
        app.detail_view_height.set(20);
        app.detail_max_scroll.set(100);
        *app.detail_row_tops.borrow_mut() = (0..40).collect();

        app.scroll_detail(10);
        assert_eq!(app.detail_selected(), 0, "the wheel moved the selection");

        app.detail_nav_down();
        assert_eq!(
            app.detail_selected(),
            14,
            "did not re-enter at the top edge of the view"
        );
    }

    /// Scrolling back up re-enters from the bottom edge instead, so the
    /// re-entry always lands on the side the selection went out of.
    #[test]
    fn a_step_after_scrolling_up_re_enters_from_the_bottom() {
        let mut app = make_app(1);
        app.history_view = HistoryView::Commits;
        app.detail_view_height.set(20);
        app.detail_max_scroll.set(100);
        *app.detail_row_tops.borrow_mut() = (0..40).collect();

        app.set_detail_selected(30);
        app.scroll_detail(0_i32.saturating_sub(0));
        app.set_detail_scroll(0);
        app.detail_nav_up();
        assert_eq!(
            app.detail_selected(),
            15,
            "did not re-enter at the bottom edge of the view"
        );
    }

    /// A selection already on screen is left alone, so stepping through
    /// commits that share a screen does not stutter on a phantom re-entry.
    #[test]
    fn a_visible_selection_steps_without_re_entering() {
        let mut app = make_app(1);
        app.history_view = HistoryView::Commits;
        app.detail_view_height.set(20);
        app.detail_max_scroll.set(100);
        *app.detail_row_tops.borrow_mut() = (0..40).collect();

        // `Commits` counts graph rows, and a step past the last one falls
        // through to scrolling, so the list has to be long enough to step in.
        app.repos[0].graph = (0..40)
            .map(|i| crate::model::GraphLine {
                hash: format!("c{i:06}"),
                refs: vec![],
                author: "Gaurgle".to_string(),
                message: format!("feat: thing {i}"),
                lanes: vec![],
                is_merge: false,
            })
            .collect();
        app.set_detail_scroll(10);
        app.set_detail_selected(16);
        app.detail_nav_down();
        assert_eq!(
            app.detail_selected(),
            17,
            "a visible selection did not step"
        );
    }

    /// The margin collapses at the ends of the content: held there, the first
    /// and last entries could never be selected.
    #[test]
    fn the_first_entry_is_reachable_at_the_top_of_the_content() {
        let mut app = make_app(1);
        app.history_view = HistoryView::Commits;
        app.detail_view_height.set(20);
        app.detail_max_scroll.set(100);
        *app.detail_row_tops.borrow_mut() = (0..40).collect();

        app.set_detail_selected(0);
        app.scroll_detail(0);
        assert_eq!(app.detail_selected(), 0, "row 0 was pushed off its own row");
    }

    /// `info` draws branches, worktrees and stashes below its commit list, and
    /// scrolling past the last commit to read them is the point. With no entry
    /// left inside the pane the selection stays where it is rather than being
    /// dragged to an edge.
    #[test]
    fn scrolling_past_a_list_leaves_its_selection_alone() {
        let mut app = make_app(1);
        app.history_view = HistoryView::Info;
        app.detail_view_height.set(20);
        app.detail_max_scroll.set(100);
        // Six commits at the top, then sections that own no commit.
        *app.detail_row_tops.borrow_mut() = (0..6).collect();
        app.set_detail_selected(5);

        app.scroll_detail(60);
        assert_eq!(
            app.detail_scroll(),
            60,
            "the sections below stayed reachable"
        );
        assert_eq!(app.detail_selected(), 5, "and the cursor was not dragged");
    }

    /// The graph block already carries hash, author, date and message, so an
    /// inspector there repeats it while taking the rows the block needs.
    #[test]
    fn the_graph_view_hides_the_inspector() {
        let mut app = make_app(1);
        app.history_view = HistoryView::Commits;
        assert!(app.shows_inspector());
        app.history_view = HistoryView::Graph;
        assert!(!app.shows_inspector());
    }

    /// Tab must not park focus on a pane that is not drawn: every later
    /// keypress would go somewhere invisible.
    #[test]
    fn focus_cycling_skips_a_hidden_inspector() {
        let mut app = make_app(1);
        app.history_view = HistoryView::Graph;
        app.view = View::Detail;
        app.focused_panel = FocusedPanel::Detail;
        app.toggle_panel_focus();
        assert_ne!(
            app.focused_panel,
            FocusedPanel::Inspector,
            "focus landed on a pane the graph view does not draw"
        );
    }

    /// The same trap by another door: `3` jumps straight to the inspector.
    #[test]
    fn jumping_to_a_hidden_inspector_is_refused() {
        let mut app = make_app(1);
        app.history_view = HistoryView::Graph;
        app.view = View::Detail;
        app.focused_panel = FocusedPanel::Detail;
        app.focus_panel(FocusedPanel::Inspector);
        assert_eq!(
            app.focused_panel,
            FocusedPanel::Detail,
            "focus went to a hidden pane"
        );
    }

    /// And by the third: switching into the graph view while the inspector
    /// holds focus has to hand it back.
    #[test]
    fn entering_the_graph_view_moves_focus_off_the_inspector() {
        let mut app = make_app(1);
        app.view = View::Detail;
        app.history_view = HistoryView::Reflog;
        app.focused_panel = FocusedPanel::Inspector;
        // HistoryView::ALL is [Info, Commits, Reflog, Graph], so Reflog steps
        // forward into Graph.
        app.cycle_history_view(false);
        assert_eq!(app.history_view, HistoryView::Graph, "fixture assumption");
        assert_eq!(
            app.focused_panel,
            FocusedPanel::Detail,
            "focus left on a hidden pane"
        );
    }
}
