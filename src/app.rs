use crate::model::{CiStatus, RepoLocation, RepoStatus, Summary};
use crate::time::{TimeWindow, WINDOW_N_MIN, WindowUnit};
use chrono::{DateTime, Local};

pub use crate::engine::{ApplyProgress, DatasetSource, FetchClaims, FetchReport, FetchTally};

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
    /// The `notes` view's reader, beside the tree in pane 2 (FZ-62). On
    /// screen only in that layout (`App::notes_layout`); j/k scroll it.
    Reader,
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
    /// The selected repo's worktrees, the main checkout first, the
    /// highlighted one's facts and newest commits in the inspector (FZ-77).
    /// Reads a worktree's log only while on screen
    /// (`App::worktrees_wanted`). Beside `branches`, whose checkouts these
    /// are, and before it: `branches`, `ci` and `notes` keep their order.
    Worktrees,
    Branches,
    /// The selected repo's recent workflow runs (FZ-30d). The only view that
    /// polls: the CI data layer asks GitHub only while this one is on
    /// screen (`App::ci_wanted`).
    Ci,
    /// The selected repo's notes as a tree, the highlighted file in the
    /// inspector (FZ-53). Walks the repo only while on screen
    /// (`App::notes_wanted`).
    Notes,
    /// The selected repo's files that have not left this machine:
    /// uncommitted, then unpushed, the highlighted file's diff in the
    /// inspector (FZ-60). Read only while on screen (`App::changes_wanted`).
    Changes,
}

impl HistoryView {
    /// Every view, in the order the tab strip draws them and `V` cycles them.
    pub const ALL: [Self; 9] = [
        Self::Info,
        Self::Commits,
        Self::Reflog,
        Self::Graph,
        Self::Worktrees,
        Self::Branches,
        Self::Ci,
        Self::Notes,
        Self::Changes,
    ];
    pub const COUNT: usize = Self::ALL.len();

    /// Index into the per-view scroll and selection arrays.
    pub fn index(self) -> usize {
        match self {
            Self::Info => 0,
            Self::Commits => 1,
            Self::Reflog => 2,
            Self::Graph => 3,
            Self::Worktrees => 4,
            Self::Branches => 5,
            Self::Ci => 6,
            Self::Notes => 7,
            Self::Changes => 8,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Commits => "commits",
            Self::Reflog => "reflog",
            Self::Graph => "graph",
            Self::Branches => "branches",
            Self::Ci => "ci",
            Self::Notes => "notes",
            Self::Worktrees => "worktrees",
            Self::Changes => "changes",
        }
    }
}

/// Which source the contribution strip shows. Persisted to `config.toml` as
/// one key, where `Off` starts the strip folded: the strip itself is always on
/// screen, so at runtime `App::contributions` is never `Off`.
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

/// Which glyph set the repo kind, stash and watch marks are drawn from,
/// from the `icons` config key. `Nerd` is the default and draws the Nerd
/// Font icons; `Plain` draws ASCII stand-ins every monospace font has, for a
/// terminal without a Nerd Font, where those icons are tofu. The contribution
/// strip's day squares are not covered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Icons {
    #[default]
    Nerd,
    Plain,
}

impl Icons {
    /// "plain" selects `Plain`; anything else, a typo included, keeps the
    /// default, as the other enum-like keys do.
    pub fn from_config(value: &str) -> Self {
        match value {
            "plain" => Self::Plain,
            _ => Self::Nerd,
        }
    }

    /// The string `from_config` reads back as this set: what the settings
    /// overlay writes and shows.
    pub fn config_value(self) -> &'static str {
        match self {
            Self::Nerd => "nerd",
            Self::Plain => "plain",
        }
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
    /// The free-text portion, matched against repo names, owners and
    /// authors (`text_match`).
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

/// Which field of a row a free-text search matched (FZ-56), declared in
/// rank order: a name hit outranks an owner or organisation hit, which
/// outranks a contributor hit, whatever the fuzzy scores say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchField {
    Name,
    Owner,
    /// The owner, when GitHub reports it as an organisation.
    Org,
    /// An author of the row's loaded commits, or of one of its open PRs.
    Author,
}

impl MatchField {
    /// The rank tier: lower sorts first. Owner and organisation share one,
    /// being the same login.
    fn tier(self) -> u8 {
        match self {
            MatchField::Name => 0,
            MatchField::Owner | MatchField::Org => 1,
            MatchField::Author => 2,
        }
    }

    /// The dimmed word the ranked list draws after a non-name hit, so a
    /// match on a person does not read as a mistaken name match.
    pub fn mark(self) -> Option<&'static str> {
        match self {
            MatchField::Name => None,
            MatchField::Owner => Some("owner"),
            MatchField::Org => Some("org"),
            MatchField::Author => Some("author"),
        }
    }
}

/// Score `text` against an owner or author string, or None when it does
/// not match.
///
/// Stricter than the name match: every query word must start a word of the
/// field (`roos` and `and ro` find "Andreas Roos", `dr` does not). A loose
/// subsequence made nearly any two letters match a full name, so a short
/// name query listed every row its author ever touched. Word order is free,
/// so the phrase and the words one by one find the same rows; the fuzzy
/// score then orders the hits, phrase first.
fn person_score(text: &str, field: &str) -> Option<i32> {
    fn words(s: &str) -> impl Iterator<Item = &str> {
        s.split(|c: char| matches!(c, '-' | '_' | '.' | '/') || c.is_whitespace())
            .filter(|w| !w.is_empty())
    }
    let field_lower = field.to_lowercase();
    let text_lower = text.to_lowercase();
    let field_words: Vec<&str> = words(&field_lower).collect();
    let text_words: Vec<&str> = words(&text_lower).collect();
    // Separators alone give no words, and zero words would match anyone.
    if text_words.is_empty() {
        return None;
    }
    let mut by_word = 0;
    for word in text_words {
        if !field_words.iter().any(|w| w.starts_with(word)) {
            return None;
        }
        by_word += crate::fuzzy::score(word, field).unwrap_or(0);
    }
    Some(crate::fuzzy::score(text, field).unwrap_or(by_word))
}

/// How a row answers a free-text query: the best field it matched and that
/// field's score, or None when it matched nothing. The name is tried first
/// and wins outright, so a name search ranks exactly as it did before
/// owners and authors were searched.
pub fn text_match(text: &str, repo: &RepoStatus) -> Option<(MatchField, i32)> {
    if let Some(score) = crate::fuzzy::score(text, &repo.name) {
        return Some((MatchField::Name, score));
    }
    if let Some(owner) = repo.owner_display()
        && let Some(score) = person_score(text, &owner)
    {
        let field = if repo.repo_kind == crate::model::RepoKind::Org {
            MatchField::Org
        } else {
            MatchField::Owner
        };
        return Some((field, score));
    }
    repo.commit_authors()
        .iter()
        .map(String::as_str)
        .chain(repo.prs.iter().map(|pr| pr.author.as_str()))
        .filter_map(|author| person_score(text, author))
        .max()
        .map(|score| (MatchField::Author, score))
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
    crate::digest::Section::of(repo) as u8
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

/// Repo list width as a percentage of the main area, and the inspector's share
/// of the right column. These are the sizes every launch starts from.
pub const DEFAULT_REPO_LIST_PCT: u16 = 35;
pub const DEFAULT_INSPECTOR_PCT: u16 = 25;

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
/// from the edge, from where the text scrolls under it instead, as vim's
/// `scrolloff`: enough to see where you are going, little enough that the
/// cursor still travels. Four, shared by pane 1 and pane 2 (FZ-55).
///
/// Applied in every history view, not only the graph, and as pane 1's
/// `List::scroll_padding`. The margin collapses at the ends of the content,
/// where there is nothing left to keep in reserve.
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

/// Status message shown temporarily in the footer.
#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub text: String,
    pub is_error: bool,
    /// When the message was set, so the event loop can expire it after a
    /// short TTL instead of it sitting frozen until the next keypress.
    pub created: std::time::Instant,
}

/// The two halves of a manual `r`, named apart in the header because they
/// cost very different amounts: the local sweep lands in well under a second,
/// the GitHub poll takes 6-7s before its fetches even start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshPhase {
    /// Re-reading the local clones.
    Local,
    /// Waiting on the GitHub poll, then on the fetches it set off.
    Remote,
}

/// How old the GitHub data on screen is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataTime {
    /// Nothing from GitHub has been shown yet.
    NoData,
    /// Data is shown, but when it left GitHub could not be read. It stays
    /// unknown: launch time is never used in its place.
    Unknown,
    /// When the data on screen left GitHub.
    At(DateTime<Local>),
}

/// How the most recent attempt to reach GitHub ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    NotAttempted,
    Success,
    Failed(crate::github::Failure),
}

/// The three GitHub freshness facts of `docs/freshness-contract.md`: when the
/// data on screen left GitHub, when fleetz last tried, and how that went.
///
/// They used to be one timestamp and one flag, `last_github_poll` and
/// `github_poll_failing`, and an offline poll answered from the cache set the
/// one and cleared the other, so offline looked exactly like healthy. Kept
/// apart, each transition below is a pure function of the event and `now`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubFreshness {
    pub data_time: DataTime,
    /// When the most recent attempt finished or was given up on. Drives the
    /// retry clock and the budget's pace, and dates a failure in the header
    /// (`offline at 12:02`, #79); a cache read never sets it.
    pub last_attempt: Option<DateTime<Local>>,
    pub outcome: Outcome,
    /// The id handed to the newest background poll started.
    started: u64,
    /// The id of the newest background poll whose answer was applied.
    applied: u64,
}

impl Default for GithubFreshness {
    fn default() -> Self {
        Self {
            data_time: DataTime::NoData,
            last_attempt: None,
            outcome: Outcome::NotAttempted,
            started: 0,
            applied: 0,
        }
    }
}

impl GithubFreshness {
    /// A load landed. A cache read is not an attempt (rule 1), so it sets the
    /// data time and leaves the retry clock unset: the poll is due at once.
    ///
    /// A load that went to GitHub (`R`, or a launch with no usable cache) is
    /// an attempt that rebuilt every row, so it supersedes every poll started
    /// before it lands: an answer from one of those cannot be ordered against
    /// the load and must not overwrite it (rule 10). A cache read and a load
    /// with no GitHub fetch behind it supersede nothing, since they are older
    /// than any poll still out, which is about to bring newer data.
    pub fn loaded(&mut self, source: &DatasetSource, now: DateTime<Local>) {
        match source {
            DatasetSource::Local => {}
            DatasetSource::Cache { written } => self.data_time = Self::file_time(*written),
            DatasetSource::Network => {
                self.supersede_started();
                self.succeeded(now);
            }
            DatasetSource::Fallback { failure, written } => {
                self.supersede_started();
                self.failed(failure.clone(), now);
                // The rows were rebuilt from the cache, so this is what is on
                // screen now.
                self.data_time = Self::file_time(*written);
            }
        }
    }

    /// An attempt came back with data (rule 7).
    pub fn succeeded(&mut self, now: DateTime<Local>) {
        self.data_time = DataTime::At(now);
        self.last_attempt = Some(now);
        self.outcome = Outcome::Success;
    }

    /// An attempt failed. The data time stays where it was (rule 3); the
    /// retry clock advances (rule 6).
    pub fn failed(&mut self, failure: crate::github::Failure, now: DateTime<Local>) {
        self.last_attempt = Some(now);
        self.outcome = Outcome::Failed(failure);
    }

    /// A background poll outlived `GITHUB_POLL_STUCK_AFTER` (decision D4).
    pub fn gave_up(&mut self, after: std::time::Duration, now: DateTime<Local>) {
        let failure = crate::github::Failure::new(
            crate::github::FailureKind::TimedOut,
            format!("no answer from GitHub after {}s", after.as_secs()),
        );
        self.failed(failure, now);
    }

    /// Hand out the id for a background poll about to start.
    pub fn begin_attempt(&mut self) -> u64 {
        self.started += 1;
        self.started
    }

    /// Whether `attempt` is the newest poll started, the one the in-flight
    /// lock belongs to.
    pub fn is_latest(&self, attempt: u64) -> bool {
        attempt == self.started
    }

    /// Record that `attempt`'s answer is being applied. False when a newer
    /// attempt's answer was applied already: the caller must discard it, or
    /// the data time could go backwards (rule 10).
    pub fn accept_answer(&mut self, attempt: u64) -> bool {
        if attempt <= self.applied {
            return false;
        }
        self.applied = attempt;
        true
    }

    /// Whether one interval of `every_secs` has passed since the last attempt.
    /// No attempt yet means due.
    pub fn retry_due(&self, now: DateTime<Local>, every_secs: i64) -> bool {
        self.last_attempt
            .is_none_or(|t| (now - t).num_seconds() >= every_secs)
    }

    pub fn is_failing(&self) -> bool {
        matches!(self.outcome, Outcome::Failed(_))
    }

    /// Data is on screen and nothing has tried to confirm it yet: the state a
    /// cache-served launch leaves, owed one attempt even with watch off
    /// (rule 8).
    pub fn awaits_first_attempt(&self) -> bool {
        self.outcome == Outcome::NotAttempted && self.data_time != DataTime::NoData
    }

    /// Treat every poll started so far as answered, so their answers are
    /// discarded when they arrive.
    fn supersede_started(&mut self) {
        self.applied = self.started;
    }

    fn file_time(written: Option<DateTime<Local>>) -> DataTime {
        written.map_or(DataTime::Unknown, DataTime::At)
    }
}

/// Where the randomness in the retry policy comes from, injected so tests can
/// plant it. See `docs/api-budget-design.md` 4.3 and 4.4: instances that fail
/// together would otherwise retry together.
pub trait Jitter {
    /// A factor for the backoff ladder from step 2 on, in 0.8 to 1.2.
    fn backoff_factor(&mut self) -> f64;
    /// Whole seconds added to a resume after a reset, in 0 to 30.
    fn resume_secs(&mut self) -> i64;
}

/// The production jitter: a xorshift seeded from the clock and the process id,
/// so two instances started together still draw apart. Not for anything that
/// needs to be unpredictable, only spread.
pub struct ClockJitter {
    state: u64,
}

impl ClockJitter {
    pub fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        let seed = nanos ^ (u64::from(std::process::id()) << 32) ^ 0x9E37_79B9_7F4A_7C15;
        Self { state: seed.max(1) }
    }

    /// A draw in 0.0 to 1.0.
    fn unit(&mut self) -> f64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

impl Default for ClockJitter {
    fn default() -> Self {
        Self::new()
    }
}

impl Jitter for ClockJitter {
    fn backoff_factor(&mut self) -> f64 {
        BACKOFF_JITTER_MIN + (BACKOFF_JITTER_MAX - BACKOFF_JITTER_MIN) * self.unit()
    }

    fn resume_secs(&mut self) -> i64 {
        ((self.unit() * (RESUME_JITTER_MAX_SECS + 1) as f64) as i64).min(RESUME_JITTER_MAX_SECS)
    }
}

/// The share of the hourly limit pacing leaves untouched for `r`, `R`, the
/// board and other clients on the token (A2).
pub const BUDGET_RESERVE_PERCENT: u64 = 10;
/// Where the backoff ladder stops doubling, before its jitter (A3).
pub const BACKOFF_CAP_SECS: i64 = 600;
/// GitHub's documented minimum wait for a secondary limit that names none.
pub const SECONDARY_LIMIT_MIN_SECS: i64 = 60;
const BACKOFF_JITTER_MIN: f64 = 0.8;
const BACKOFF_JITTER_MAX: f64 = 1.2;
const RESUME_JITTER_MAX_SECS: i64 = 30;
/// A rate-limit window lasts at most an hour, which bounds a reset's delay.
const RESET_WINDOW_SECS: i64 = 3600;

/// The newest budget reading, anchored to the local clock on arrival. Memory
/// only: a budget is per account and per moment (`docs/api-budget-design.md`
/// 4.1). Unknown until a reading arrives, and unknown again once its reset
/// has passed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ApiBudget {
    reading: Option<crate::github::BudgetReading>,
    /// The reading's reset as a local instant (`anchor_reset`).
    reset_at: Option<DateTime<Local>>,
    /// Drawn once per window, so a pause and a limit in the same window resume
    /// at the same moment, and every check of it agrees.
    resume_jitter_secs: i64,
}

/// What `ApiBudget::observe` did with a reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observed {
    /// Older than the one held, or from a discarded answer it may not apply.
    Ignored,
    /// Replaced the held reading on the same token, or was the first.
    Updated,
    /// An applied answer on another token replaced the held reading.
    Switched,
}

/// How the budget spaces automatic polls (A2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    /// No reading, or its window is over: the configured interval applies.
    Unknown,
    /// At most one automatic poll per this many seconds.
    Every(i64),
    /// At or below the reserve: no automatic poll before `until`, the reset
    /// plus its jitter.
    Paused { until: DateTime<Local> },
}

/// `x-ratelimit-reset` as a local instant. The delay to the reset is taken in
/// the server's own time (`reset - Date`) and added to the local arrival, so a
/// skewed local clock neither pauses too long nor resumes into a refused
/// window; without a `Date` the local clock stands in. Clamped to one window.
pub fn anchor_reset(
    reading: &crate::github::BudgetReading,
    arrival: DateTime<Local>,
) -> DateTime<Local> {
    let server_now = reading.date.map_or(arrival.timestamp(), |d| d.timestamp());
    let delay = (reading.reset - server_now).clamp(0, RESET_WINDOW_SECS);
    arrival + chrono::Duration::seconds(delay)
}

impl ApiBudget {
    /// Take a reading under the order of `docs/api-budget-design.md` 4.1:
    /// only an applied answer may switch the token; on the same token a later
    /// reset is a newer window, and within one window the lower `remaining` is
    /// the newer reading. `applied` is false for an answer `accept_answer`
    /// discarded.
    pub fn observe(
        &mut self,
        reading: &crate::github::BudgetReading,
        applied: bool,
        arrival: DateTime<Local>,
        jitter: &mut dyn Jitter,
    ) -> Observed {
        let Some(held) = &self.reading else {
            if !applied {
                return Observed::Ignored;
            }
            self.replace(reading, arrival, jitter);
            return Observed::Updated;
        };
        if held.token_id != reading.token_id {
            if !applied {
                return Observed::Ignored;
            }
            self.replace(reading, arrival, jitter);
            return Observed::Switched;
        }
        match reading.reset.cmp(&held.reset) {
            std::cmp::Ordering::Greater => {
                self.replace(reading, arrival, jitter);
                Observed::Updated
            }
            std::cmp::Ordering::Equal if reading.remaining < held.remaining => {
                // The same window: its reset and jitter stand.
                self.reading = Some(reading.clone());
                Observed::Updated
            }
            _ => Observed::Ignored,
        }
    }

    fn replace(
        &mut self,
        reading: &crate::github::BudgetReading,
        arrival: DateTime<Local>,
        jitter: &mut dyn Jitter,
    ) {
        self.reset_at = Some(anchor_reset(reading, arrival));
        self.resume_jitter_secs = jitter.resume_secs().clamp(0, RESUME_JITTER_MAX_SECS);
        self.reading = Some(reading.clone());
    }

    /// The held reading, whether or not its window is over. Its token decides
    /// which later readings may apply.
    #[cfg(test)]
    pub fn reading(&self) -> Option<&crate::github::BudgetReading> {
        self.reading.as_ref()
    }

    /// The reading while its window lasts at `at`; `None` is unknown.
    pub fn known(&self, at: DateTime<Local>) -> Option<&crate::github::BudgetReading> {
        let reset_at = self.reset_at?;
        (reset_at > at).then_some(self.reading.as_ref()?)
    }

    /// The reset as a local instant.
    #[cfg(test)]
    pub fn reset_at(&self) -> Option<DateTime<Local>> {
        self.reset_at
    }

    /// When polling resumes after this window: the reset plus its jitter.
    pub fn resume_at(&self) -> Option<DateTime<Local>> {
        self.reset_at
            .map(|t| t + chrono::Duration::seconds(self.resume_jitter_secs))
    }

    /// Whether the next request would be refused at `at`: fewer points left
    /// than one poll costs. Treated as a known primary limit until the reset.
    pub fn exhausted(&self, cost: u32, at: DateTime<Local>) -> bool {
        self.known(at).is_some_and(|r| r.remaining < cost)
    }

    /// How far apart automatic polls of `cost` points go, judged at `at` (the
    /// last attempt): what is left above the reserve spread over the time to
    /// the reset, never past the resume. At or below the reserve, or with too
    /// little left for one poll, paused until the resume.
    pub fn pace(&self, cost: u32, at: DateTime<Local>) -> Pace {
        let (Some(reading), Some(resume)) = (self.known(at), self.resume_at()) else {
            return Pace::Unknown;
        };
        let reserve = u64::from(reading.limit) * BUDGET_RESERVE_PERCENT / 100;
        let remaining = u64::from(reading.remaining);
        if remaining <= reserve || reading.remaining < cost {
            return Pace::Paused { until: resume };
        }
        let to_reset = self.reset_at.map_or(0, |t| (t - at).num_seconds().max(0)) as u64;
        let spare = remaining - reserve;
        let secs = (u64::from(cost) * to_reset).div_ceil(spare) as i64;
        Pace::Every(secs.min((resume - at).num_seconds().max(0)))
    }
}

/// When the next automatic attempt may go at the earliest, beside the rule 6
/// interval. See `GithubBackoff`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Hold {
    /// A fixed instant: a known reset plus its jitter, or a `retry-after`.
    At(DateTime<Local>),
    /// A step of the ladder, counted from the failure at `from`. Resolved
    /// against the configured interval when asked, which `App` does not hold.
    Ladder {
        from: DateTime<Local>,
        step: u32,
        factor: f64,
        at_least_secs: i64,
    },
}

/// The not-before and consecutive-failure count of `docs/api-budget-design.md`
/// 4.3. Kept beside `GithubFreshness`, not inside it, so `retry_due` keeps its
/// meaning: the not-before is a second condition, checked next to it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GithubBackoff {
    failures: u32,
    hold: Option<Hold>,
    /// Whether `hold` is a rate limit's, which a token change drops and which
    /// holds back `r` and `R`.
    hold_is_limit: bool,
}

/// Seconds the ladder waits after the `step`th consecutive failure: exactly
/// the interval at step 1, then doubling up to the cap, times the jitter
/// `factor` (applied after the cap), never under the interval.
pub fn backoff_secs(step: u32, interval_secs: i64, factor: f64) -> i64 {
    if step <= 1 {
        return interval_secs;
    }
    let doublings = (step - 1).min(32);
    let base = interval_secs
        .saturating_mul(1i64 << doublings)
        .min(BACKOFF_CAP_SECS);
    let factor = factor.clamp(BACKOFF_JITTER_MIN, BACKOFF_JITTER_MAX);
    ((base as f64 * factor).round() as i64).max(interval_secs)
}

impl GithubBackoff {
    /// The consecutive-failure count.
    #[cfg(test)]
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// A success clears both the count and the not-before (transition 3).
    pub fn succeeded(&mut self) {
        *self = Self::default();
    }

    /// An applied failure at `now`. `budget` already holds the reading that
    /// came with it. `Offline` and `Auth` say nothing about GitHub and leave
    /// everything as it was (transition 4); every other kind counts and
    /// replaces the not-before (transition 2).
    pub fn failed(
        &mut self,
        failure: &crate::github::Failure,
        now: DateTime<Local>,
        budget: &ApiBudget,
        jitter: &mut dyn Jitter,
    ) {
        use crate::github::{FailureKind, RateLimit};
        if matches!(failure.kind, FailureKind::Offline | FailureKind::Auth) {
            return;
        }
        self.failures = self.failures.saturating_add(1);
        let step = self.failures;
        let mut ladder = |at_least_secs| Hold::Ladder {
            from: now,
            step,
            // Step 1 is exactly the interval: no draw.
            factor: if step >= 2 {
                jitter.backoff_factor()
            } else {
                1.0
            },
            at_least_secs,
        };
        let (hold, is_limit) = match failure.limit {
            None => (ladder(0), false),
            Some(RateLimit::Primary) => match budget.known(now).and(budget.resume_at()) {
                Some(resume) => (Hold::At(resume), true),
                None => (ladder(0), true),
            },
            Some(RateLimit::Secondary {
                retry_after: Some(wait),
            }) => {
                let wait = chrono::Duration::from_std(wait)
                    .unwrap_or_else(|_| chrono::Duration::seconds(RESET_WINDOW_SECS));
                (Hold::At(now + wait), true)
            }
            Some(RateLimit::Secondary { retry_after: None }) => {
                (ladder(SECONDARY_LIMIT_MIN_SECS), true)
            }
        };
        self.hold = Some(hold);
        self.hold_is_limit = is_limit;
    }

    /// An applied answer came on another token: a rate limit belonged to the
    /// old credential and goes with it. The count describes GitHub and stays.
    pub fn account_switched(&mut self) {
        if self.hold_is_limit {
            self.hold = None;
            self.hold_is_limit = false;
        }
    }

    /// The not-before, resolved against the configured interval.
    pub fn not_before(&self, interval_secs: i64) -> Option<DateTime<Local>> {
        Some(match self.hold? {
            Hold::At(t) => t,
            Hold::Ladder {
                from,
                step,
                factor,
                at_least_secs,
            } => {
                let secs = backoff_secs(step, interval_secs, factor).max(at_least_secs);
                from + chrono::Duration::seconds(secs)
            }
        })
    }

    /// The not-before when it is a rate limit's.
    pub fn limit_until(&self, interval_secs: i64) -> Option<DateTime<Local>> {
        self.not_before(interval_secs)
            .filter(|_| self.hold_is_limit)
    }
}

/// The per-checkout hold after fetches that timed out (FZ-34, #83; amendment
/// 2 of `docs/fetch-outcome-contract.md`). A repo too large to fetch within
/// FZ-34's wall clock used to start the same download on every sweep and
/// leave a temp pack behind each time; since FZ-35a a timeout is a fetch
/// that stalled for `git::FETCH_STALL_LIMIT`.
///
/// Keyed by the row's path, as the fetch facts are. After the `n`th
/// consecutive `TimedOut` fetch the path is held for
/// `backoff_secs(n + 1, interval_secs, 1.0)` seconds from that timeout: two
/// intervals, then four, doubling up to `BACKOFF_CAP_SECS`, with no jitter
/// (one clone has nothing to desynchronise from). Only the automatic fetches
/// honour it (the watch sweep, the poll's moved remotes, the recovery
/// refetch); `f` and `F` fetch as before and their outcome counts like any.
/// A success clears the path; `r` and `R` clear every path. Any other failure
/// kind fails fast, costs nothing, and leaves the count as it was. That
/// includes `TooBig` (FZ-35b): a fetch cut at the cap holds its repo
/// explicitly, until `f` or a success (`model::FetchFacts::size_hold`), not
/// on a doubling timer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchBackoff {
    /// Consecutive timeouts and when the last one landed, per path.
    timeouts: std::collections::HashMap<String, (u32, DateTime<Local>)>,
}

impl FetchBackoff {
    /// A fetch for `path` finished at `at`. A timeout older than the one
    /// already counted (a load's fetch landing after a sweep's) is ignored,
    /// so one timeout is never counted twice.
    pub fn record(&mut self, path: &str, outcome: &crate::git::FetchOutcome, at: DateTime<Local>) {
        use crate::git::{FetchFailureKind, FetchOutcome};
        match outcome {
            FetchOutcome::Succeeded => {
                self.timeouts.remove(path);
            }
            FetchOutcome::Failed(f) if f.kind == FetchFailureKind::TimedOut => {
                let entry = self.timeouts.entry(path.to_string()).or_insert((0, at));
                if entry.0 == 0 || at > entry.1 {
                    *entry = (entry.0.saturating_add(1), at);
                }
            }
            FetchOutcome::Failed(_) | FetchOutcome::NotAttempted => {}
        }
    }

    /// Whether an automatic fetch of `path` must wait at `now`.
    pub fn holds(&self, path: &str, now: DateTime<Local>, interval_secs: i64) -> bool {
        self.timeouts.get(path).is_some_and(|&(n, at)| {
            now < at + chrono::Duration::seconds(backoff_secs(n + 1, interval_secs, 1.0))
        })
    }

    /// A manual `r` or `R`: every path may fetch again.
    pub fn clear(&mut self) {
        self.timeouts.clear();
    }

    /// The consecutive timeouts counted for `path`.
    #[cfg(test)]
    pub fn timeouts(&self, path: &str) -> u32 {
        self.timeouts.get(path).map_or(0, |&(n, _)| n)
    }
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
    /// Whether this frame drew the fleet's summary and activity in pane 1's
    /// summary box (FZ-95; pane 2's fleet column in FZ-93). Reset
    /// at the start of every frame and read by the footer, drawn after the
    /// panes, which carries the summary and the message itself when nothing
    /// else did (a pane 1 too short for the box), so an action never ends
    /// in silence.
    pub fleet_drawn: std::cell::Cell<bool>,
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
    /// Where the detail pane's last frame drew its rows and their hashes, so
    /// a click resolves against what is on screen (FZ-37). Recorded by the
    /// renderer for the reason `detail_row_tops` is.
    pub detail_hits: std::cell::RefCell<crate::ui::detail::DetailHits>,
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
    /// force a paint while a spinner is visible, and every
    /// `SLOW_REPAINT_TICKS` ticks in watch mode or while applying, so the
    /// clock and blink stay live.
    pub needs_redraw: bool,
    /// Advances every 100ms tick; drives the loading/sweep spinner.
    pub spinner_frame: usize,
    /// Flips every 500ms; drives the pending-CI blink.
    pub blink_on: bool,
    /// The noise field shown while the first load is still running, so the
    /// wait is spent on a loading screen rather than on an empty frame. See
    /// `ui::intro`.
    pub loading_field: Option<crate::ui::intro::Loading>,
    /// True once the user pressed v; stops resize from auto-switching views.
    pub view_overridden: bool,
    /// Width of the repo list as a percentage of the main area, and the
    /// inspector's share of the right column. Nudged with the resize keys.
    /// Defaults are restored on every launch unless `persist_pane_sizes` is
    /// set, so a layout tweaked for one repo doesn't silently become the norm.
    pub repo_list_pct: u16,
    pub inspector_pct: u16,
    /// Which source the contribution strip shows. Never `Off` here: the
    /// strip is always on screen, and the setting's "off" folds it instead
    /// (`apply_contributions_setting`).
    pub contributions: ContributionsMode,
    /// Whether the inspector and the strip are folded to an empty frame. A
    /// folded pane stays on screen and can hold focus, so its `[3]`/`[4]` tag
    /// is still there to click and its number still reaches it. Neither is
    /// ever removed outright. Only these two fold: the repo list and the
    /// detail pane are the dashboard itself.
    pub inspector_folded: bool,
    pub contributions_folded: bool,
    /// The account-wide calendar from the last GitHub poll. None until the
    /// first poll lands, or when the payload predates the field.
    pub github_calendar: Option<Box<crate::contributions::Calendar>>,
    /// Git author regex the local contribution source filters by, from the
    /// `contributions_author` config key. None resolves each repo's own
    /// `user.email` instead. Lives on `App` rather than being read from
    /// `Config` at each use, because the sweep is started from call sites
    /// that hold an `App` but not a `Config`.
    pub contrib_author: Option<String>,
    /// Which square icon the strip draws a day as, from the
    /// `contributions_cell_size` config key and the settings row.
    pub contrib_cell: CellSize,
    /// Grid slot the day cursor sits on. The strip stands on today once it
    /// is opened or unfolded (#66), so the title reads today's count. None
    /// remains only for an `App` whose strip was never shown: there the first
    /// move seeds the cursor on today, and the title falls back to
    /// `ui::contributions::period_total`.
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

    /// Whether `info` draws the remote-branch names or just their count.
    ///
    /// App-level and session-lived rather than per repo: it is a preference
    /// about how you want `info` to read, not a fact about one repo. Resets on
    /// launch, like the pane sizes.
    pub expand_remote_branches: bool,
    /// `expand_remote_branches` as the detail pane last drew it, so the frame
    /// that sees it flip on is the one that reveals the opened rows (FZ-55).
    /// A `Cell` because the renderer records it, as `detail_view_height`.
    pub remotes_drawn_expanded: std::cell::Cell<bool>,
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
    /// colored indicator over a dashboard you are reading. An `r` pressed
    /// while a watch sweep runs marks that sweep manual: it starts none of
    /// its own, so the running one's rows are the ones it waits for.
    ///
    /// Only meaningful while `sweep_started` is `Some`.
    pub sweep_manual: bool,
    /// True while a manual `r` waits on its GitHub poll. The local sweep is
    /// only half of a refresh: the poll lands later and fetches the remotes
    /// that moved, so a spinner keyed on the sweep alone stopped while rows
    /// were still changing under it.
    pub refresh_awaits_poll: bool,
    /// Display paths of the fetches that poll set off for a manual `r`, each
    /// cleared by that repo's fetched update. See `is_refreshing`.
    pub refresh_fetches: std::collections::HashSet<String>,
    /// When `refresh_fetches` was filled, or one of its fetches last made
    /// progress (`apply_fetch_progress`), so a fetch that never returns
    /// cannot leave the spinner running for the rest of the session while
    /// one still downloading is waited for.
    pub refresh_fetches_started: Option<std::time::Instant>,
    /// How current the GitHub data is: data time, last attempt and outcome.
    ///
    /// A failure has to survive a keypress. It used to live only in a status
    /// message, which the next key clears, so a poll that had been failing
    /// for hours looked exactly like one that had just succeeded: the calendar
    /// and the CI lamps kept their last good values and nothing on screen said
    /// they had stopped moving. Rate limits are the common case and they last
    /// an hour. The header reads this.
    pub github_freshness: GithubFreshness,
    /// The account's GitHub budget as the newest reading stated it.
    pub api_budget: ApiBudget,
    /// The retry ladder and not-before beside `github_freshness`'s clock.
    pub github_backoff: GithubBackoff,
    /// Paths the automatic fetches skip after a timeout (`FetchBackoff`).
    pub fetch_backoff: FetchBackoff,
    /// The automatic-fetch cap in bytes, `None` for no cap (`fetch_max_mb`,
    /// FZ-35b). Copied from `Config` at startup, as `github_poll_secs` is.
    pub fetch_max_bytes: Option<u64>,
    /// Paths with a fetch running or queued: one fetch per repo at a time
    /// (FZ-34, R5). Every fetch selection, automatic or manual, skips a path
    /// listed here and only sweeps it (`App::claim_fetch`). Two fetches of
    /// one repo each wrote their own temp pack. Sweeps reserve here and the
    /// fetch tasks take the claim at their permit; a launch or `R` load
    /// claims its fetches here too (`FetchClaims`, FZ-35a).
    pub fetches_in_flight: FetchClaims,
    /// The configured GitHub poll interval in seconds, floored, and what one
    /// poll costs in points (`github::poll_cost` of `repo_limit`). Copied from
    /// `Config` at startup, as `contrib_author` is, because the header judges
    /// the pace and holds an `App`, not a `Config`. Neither changes while
    /// fleetz runs.
    pub github_poll_secs: i64,
    pub github_poll_cost: u32,
    /// Where the backoff and resume jitter come from; planted in tests.
    pub jitter: Box<dyn Jitter>,
    /// The fetches a keypress is waiting on (`f`, `F`, and the ones `r`'s
    /// poll sets off), reported as one status line when the last lands.
    pub manual_fetches: FetchTally,
    /// True while a GitHub re-poll is in flight, so ticks do not stack them.
    pub github_poll_inflight: bool,
    /// When the in-flight GitHub poll started, so a request that never returns
    /// can be given up on. Without this, one hung call leaves
    /// `github_poll_inflight` true forever and CI status silently stops
    /// updating for the rest of the session, with nothing on screen to say so.
    pub github_poll_started: Option<std::time::Instant>,
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
    /// What the fetch behind `dataset` learned besides the repos: its
    /// coverage and account. Replaced with `dataset` by a load and by a
    /// successful poll; a window apply keeps it, so its header note is drawn
    /// from the same coverage as the dataset it re-filters.
    pub github_meta: crate::github::FetchMeta,
    /// Carrier rows' remote keys (`carrier_key`) that the last applied
    /// successful poll did not return: their GitHub fields are from an
    /// earlier refresh. A poll that returns a key removes it, a failed poll
    /// changes nothing (freshness rule 4), and every load that lands rows
    /// clears it, since those rows all come from one dataset
    /// (`docs/coverage-cache-design.md` 4.7). A set here rather than a
    /// `RepoStatus` field, so no row literal changes.
    pub not_in_last_poll: std::collections::HashSet<RemoteKey>,
    /// An applied poll answered on another account than the rows were built
    /// from (or the rows' account was unknown): the loop rebuilds them from
    /// memory once nothing else is loading (`main.rs::start_account_rebuild`).
    pub account_rebuild_due: bool,
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
    /// The `ci` view's data per remote identity, held for the session and
    /// never written to disk (FZ-30b). See `ci.rs`. Polled only while the
    /// view is on screen (`ci_wanted`); what an earlier visit read stays.
    pub ci: std::collections::HashMap<RemoteKey, CiState>,
    /// The `core` budget the CI calls bill, apart from `api_budget`, which
    /// is the GraphQL one the fleet poll paces on. One per session: the
    /// allowance is the account's, whichever repo spends it.
    pub ci_budget: ApiBudget,
    /// The CI target the cursor rests on (`None` for a row without one) and
    /// since when, for the dwell (`ci::DWELL`).
    pub ci_cursor: Option<(Option<RemoteKey>, std::time::Instant)>,
    /// The inspector body shown for the highlighted run (FZ-30f).
    pub ci_body: CiBody,
    /// Scroll offset per body, indexed by `CiBody::index`; reset when the
    /// highlighted run changes.
    pub ci_body_scrolls: [u16; 3],
    /// The job the `jobs` body marks, which the `log` body shows, once `j`
    /// or `k` moved it; `None` is the first failed job, else the first.
    pub ci_job_mark: Option<usize>,
    /// Whether the `jobs` body's next frame brings the marked job into
    /// view, and the offset it chose (`JobFollow`). Written by the renderer,
    /// which alone knows where the job's rows fall.
    pub ci_job_follow: std::cell::Cell<JobFollow>,
    /// `[`/`]` pressed on the focused inspector cycled pane 2's views, and
    /// no other key came since: the streak keeps cycling views on arriving
    /// in `ci` rather than stepping the run bodies there.
    pub brackets_cycling_views: bool,
    /// A `y` was pressed and the next key finishes the yank chord (FZ-74):
    /// `y` copies the thing under the cursor, `p` its path, anything else
    /// copies nothing. Cleared by the next key whatever it is.
    pub pending_yank: bool,
    /// The row a successful `yy` or `yp` copied from flashes once (FZ-81).
    pub yank_flash: YankFlash,
    /// The run the scrolls and the mark belong to, and since when it is
    /// highlighted, for the dwell before the inspector fetches anything.
    pub ci_inspected: Option<(u64, std::time::Instant)>,
    /// The `log` body's whole log as last wrapped, written by the renderer
    /// (`RefCell` for the reason `detail_row_tops` is one). A held log never
    /// changes, so its job id and the width are the whole key.
    pub ci_log_wrap: std::cell::RefCell<Option<CiLogWrap>>,
    /// The `worktrees` view's newest commits per worktree, keyed by the
    /// worktree's path as the row holds it, for the session (FZ-77). Read
    /// only while the view is on screen (`worktrees_wanted`).
    pub worktree_logs: std::collections::HashMap<String, WorktreeLog>,
    /// Bumped when a sweep lands: a held worktree log from an older
    /// generation is read again the next time it is shown.
    pub worktree_logs_generation: u64,
    /// The row path the cursor rests on in the `worktrees` view and since
    /// when, for the dwell (`ci::DWELL`) before a log read starts.
    pub worktrees_cursor: Option<(Option<String>, std::time::Instant)>,
    /// The `notes` view's walk per repo, keyed by the row's path, held for
    /// the session and never written to disk (FZ-53). Walked only while the
    /// view is on screen (`notes_wanted`).
    pub notes: std::collections::HashMap<String, NotesState>,
    /// Bumped when a sweep lands or a suspend returns: a walk or a body read
    /// from an older generation is read again the next time it is shown.
    pub notes_generation: u64,
    /// `m`: the tree lists every markdown file, not only the notes
    /// directories. The walk collected both; this changes only the drawing.
    pub notes_all_markdown: bool,
    /// The row path the cursor rests on in the `notes` view and since when,
    /// for the dwell (`ci::DWELL`) before a walk starts.
    pub notes_cursor: Option<(Option<String>, std::time::Instant)>,
    /// The highlighted file's body and facts, for the reader and the facts
    /// beside pane 2's header: one file at a time.
    pub note_body: Option<NoteBody>,
    /// The footer has said once this session that a board went to
    /// `$EDITOR` because `todoz` is not on PATH (FZ-63).
    pub todoz_fallback_said: bool,
    /// The reader's scroll offset (FZ-62), its own rather than the
    /// inspector's. Reset when the file changes.
    pub reader_scroll: u16,
    /// Largest useful reader offset, measured by the renderer for the
    /// reason `inspector_max_scroll` is.
    pub reader_max_scroll: std::cell::Cell<u16>,
    /// Rows the reader's body drew on the last frame: one `H`/`L` page.
    pub reader_view_height: std::cell::Cell<u16>,
    /// Columns the reader's body drew on the last frame: the width a
    /// markdown body is rendered at (FZ-59), off the UI thread.
    pub reader_width: std::cell::Cell<u16>,
    /// Columns of the main area the repo list and the right column split,
    /// as the key handler last measured it; 0 before it has. `<` and `>`
    /// step from the list width this draws (FZ-69), which differs from
    /// `repo_list_pct` where the default yields to the cap.
    pub main_area_width: std::cell::Cell<u16>,
    /// `p` in the `notes` layout: the reader shows markdown as its source.
    /// One flag for the session, not per file; other kinds are always raw.
    pub reader_raw: bool,
    /// The section header the list cursor stands on (FZ-45), lifted off the
    /// section's first repo, which `selected` keeps pointing at. Read only
    /// through `on_header`, which also checks the header is drawn.
    pub list_stop: Option<crate::digest::Section>,
    /// The window the digest counts over, cycled by `t` on a header.
    pub digest_window: crate::digest::DigestWindow,
    /// The digest line pane 2's cursor is on.
    pub digest_selected: usize,
    /// The newest `git log --since` read per row path, or the one running
    /// (`digest::HeldLog`). One per path, so it stays the size of the list.
    pub digest_logs: std::collections::HashMap<String, crate::digest::HeldLog>,
    /// The work hours slot boundaries are cut at, copied from `Config` at
    /// startup as `github_poll_secs` is.
    pub work_hours: (u8, u8),
    /// Where the repo list's last frame drew its rows, so a click or a hover
    /// resolves against what is on screen. Written by the renderer for the
    /// reason `detail_hits` is.
    pub list_hits: std::cell::Cell<ListHits>,
    /// The section header under the resting pointer and since when; the
    /// tick lands on it after `HOVER_DWELL` (`settle_hover`).
    pub hover_pending: Option<(crate::digest::Section, std::time::Instant)>,
    /// The `changes` view's read per repo, keyed by the row's path, held for
    /// the session and never written to disk (FZ-60). Read only while the
    /// view is on screen (`changes_wanted`).
    pub changes: std::collections::HashMap<String, ChangesState>,
    /// Bumped when a sweep lands or a suspend returns: a read or a diff from
    /// an older generation is read again the next time it is shown.
    pub changes_generation: u64,
    /// `u`: what the second section compares `HEAD` with.
    pub changes_base: crate::git::ChangesBase,
    /// The row path the cursor rests on in the `changes` view and since
    /// when, for the dwell (`ci::DWELL`) before a read starts.
    pub changes_cursor: Option<(Option<String>, std::time::Instant)>,
    /// The highlighted file's diff for the inspector: one file at a time.
    pub change_diff: Option<ChangeDiff>,
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

/// The `notes` view's walk of one repo (FZ-53).
#[derive(Debug, Clone, Default)]
pub struct NotesState {
    /// The last walk's answer, kept on screen while a newer one runs.
    pub tree: Option<Result<crate::notes::Notes, String>>,
    /// A walk is running.
    pub walking: bool,
    /// The generation `tree` was walked in (`App::notes_generation`).
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

/// One worktree's newest commits as the `worktrees` view's inspector draws
/// them (FZ-77): `None` until a read lands.
#[derive(Debug, Clone, Default)]
pub struct WorktreeLog {
    pub commits: Option<Vec<crate::model::Commit>>,
    /// The generation `commits` was read in (`App::worktree_logs_generation`).
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
    /// The worktree's path as its row holds it, the `worktree_logs` key.
    pub key: String,
    pub dir: std::path::PathBuf,
    pub generation: u64,
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

/// A whole job log wrapped for one pane width, so a 5000-line log is not
/// re-wrapped on every redraw (FZ-30f).
#[derive(Debug, Clone)]
pub struct CiLogWrap {
    pub job_id: u64,
    pub width: u16,
    pub lines: Vec<ratatui::text::Line<'static>>,
}

/// The `ci` view's data for one repo (FZ-30b).
#[derive(Debug, Clone, Default)]
pub struct CiState {
    /// The newest list, newest run first.
    pub runs: Vec<crate::ci::WorkflowRun>,
    /// The list's `ETag`, sent back as `If-None-Match`.
    pub runs_etag: Option<String>,
    /// The run whose jobs are read on each poll: the highlighted run while
    /// the `ci` view shows and the inspector is open
    /// (`App::sync_ci_inspector`).
    pub expanded: Option<u64>,
    /// Each read run's jobs with their `ETag`, by run id.
    pub jobs: std::collections::HashMap<u64, CiJobs>,
    /// Runs whose `/jobs` answered 404, dropped by the next list.
    pub gone: std::collections::HashSet<u64>,
    /// Failed jobs' log tails by job id, each read once.
    pub logs: std::collections::HashMap<u64, crate::ci::LogTail>,
    /// Whole job logs by job id for the inspector's `log` body, each read
    /// once, by the poll that reads a failed job's tail or by the body
    /// (FZ-30f).
    pub full_logs: std::collections::HashMap<u64, crate::ci::Detail<Vec<String>>>,
    /// Workflow files by commit and path for the `script` body, each read
    /// once.
    pub scripts: std::collections::HashMap<(String, String), crate::ci::Detail<String>>,
    /// The inspector's own `/jobs` fetch per run, for a highlighted run no
    /// poll has read yet. The jobs land in `jobs`; this holds the fetch's
    /// state until they do, and its failure after.
    pub jobs_fetch: std::collections::HashMap<u64, crate::ci::Detail<()>>,
    /// The three freshness facts, for this repo's CI alone. A 304 is a
    /// successful attempt; the fleet header never reads this.
    pub freshness: GithubFreshness,
    /// A poll for this repo is out.
    pub inflight: bool,
    /// When the poll that holds `inflight` was claimed, so one that never
    /// answers can be given up on (`App::expire_stuck_ci_polls`).
    pub started: Option<std::time::Instant>,
    /// This repo's retry ladder and not-before, apart from the fleet's and
    /// from every other repo's, so one repo's failures never delay another
    /// repo's first poll.
    pub backoff: GithubBackoff,
    /// The default branch's tip and its contexts (FZ-83), read with the runs
    /// while the `ci` view shows this repo. Memory only.
    pub tip: Option<CiTip>,
}

/// The default branch's tip as the `ci` poll last read it (FZ-83).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CiTip {
    pub sha: String,
    pub status_etag: Option<String>,
    pub statuses: Vec<crate::ci::Context>,
    pub statuses_complete: bool,
    pub checks_etag: Option<String>,
    /// `None` until the check runs of this sha answered.
    pub checks: Option<crate::ci::TipChecks>,
    /// When both reads last answered for this sha, fresh or 304: the data
    /// time `App::ci_shown` weighs against the fleet poll's.
    pub confirmed: Option<DateTime<Local>>,
    /// Every state change of the tip's commit statuses, which dates their
    /// rows in the `ci` list.
    pub history: Vec<crate::ci::StatusEvent>,
    pub history_etag: Option<String>,
}

impl CiTip {
    /// Every context of the tip is held: all its statuses and check runs.
    pub fn complete(&self) -> bool {
        self.statuses_complete && self.checks.as_ref().is_some_and(|c| c.complete)
    }

    /// The statuses, then the check runs.
    pub fn contexts(&self) -> impl Iterator<Item = &crate::ci::Context> {
        self.statuses
            .iter()
            .chain(self.checks.iter().flat_map(|c| c.runs.iter()))
    }

    /// Land the tip reads' answer at `now`. A new sha starts over: the old
    /// tip's contexts say nothing about it.
    fn land(held: &mut Option<Self>, answer: crate::ci::TipAnswer, now: DateTime<Local>) {
        use crate::ci::Fetched;
        if let Fetched::Fresh { data, etag } = answer.status {
            if held.as_ref().is_none_or(|t| t.sha != data.sha) {
                *held = Some(Self {
                    sha: data.sha.clone(),
                    ..Self::default()
                });
            }
            if let Some(tip) = held.as_mut() {
                tip.status_etag = etag;
                tip.statuses = data.statuses;
                tip.statuses_complete = data.complete;
            }
        }
        let Some(tip) = held.as_mut() else {
            return;
        };
        if let Some((sha, checks)) = answer.checks
            && tip.sha == sha
        {
            if let Fetched::Fresh { data, etag } = checks {
                tip.checks = Some(data);
                tip.checks_etag = etag;
            }
            if tip.checks.is_some() {
                tip.confirmed = Some(now);
            }
        }
        if let Some((sha, Fetched::Fresh { data, etag })) = answer.history
            && tip.sha == sha
        {
            tip.history = data;
            tip.history_etag = etag;
        }
    }

    /// The tip's contexts the run list does not already show, as rows of
    /// it: every commit status, from its history, and every check run of
    /// an app other than GitHub Actions.
    pub fn context_runs(&self) -> Vec<crate::ci::ContextRun> {
        let statuses = self
            .statuses
            .iter()
            .map(|s| crate::ci::status_run(s, &self.history));
        let checks = self
            .checks
            .iter()
            .flat_map(|c| c.runs.iter())
            .filter(|c| !c.actions)
            .map(crate::ci::check_run);
        statuses.chain(checks).collect()
    }
}

/// One row of the `ci` view's list (FZ-83): a workflow run, or another
/// context of the default branch's tip, sorted together by time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiEntry<'a> {
    Run(&'a crate::ci::WorkflowRun),
    Context(crate::ci::ContextRun),
}

/// The CI state to show for a repo (FZ-83): what the row lamp, pane 2's
/// header and the `info` line all draw, from `App::ci_shown`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiShown {
    pub status: CiStatus,
    /// The default branch's name, once a held run on its tip names it.
    pub branch: Option<String>,
    /// The context to blame while not passed (`Vercel`, `Vercel +2`), known
    /// only when the verdict came from the tip's own contexts.
    pub culprit: Option<String>,
}

/// One run's jobs as last read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CiJobs {
    pub etag: Option<String>,
    pub jobs: Vec<crate::ci::Job>,
}

impl CiState {
    /// What one poll of this repo costs on `core`: the list, the tip's
    /// status and check runs (FZ-83), its status history while it has
    /// commit statuses, plus the expanded run's jobs.
    pub fn poll_cost(&self) -> u32 {
        let history = self.tip.as_ref().is_some_and(|t| !t.statuses.is_empty());
        crate::ci::CALL_COST * (3 + u32::from(history) + u32::from(self.expanded.is_some()))
    }

    /// Land an applied answer's data at `now`. The outcome is
    /// `App::apply_ci`'s.
    fn land(&mut self, mut answer: crate::ci::CiAnswer, now: DateTime<Local>) {
        use crate::ci::{Fetched, JobsAnswer};
        if let Some(tip) = answer.tip.take() {
            CiTip::land(&mut self.tip, tip, now);
        }
        if let Some(Fetched::Fresh { data, etag }) = answer.runs {
            let gone = std::mem::take(&mut self.gone);
            self.runs = data.into_iter().filter(|r| !gone.contains(&r.id)).collect();
            self.runs_etag = etag;
            let listed: std::collections::HashSet<u64> = self.runs.iter().map(|r| r.id).collect();
            self.jobs.retain(|id, _| listed.contains(id));
            if self.expanded.is_some_and(|id| !listed.contains(&id)) {
                self.expanded = None;
            }
        }
        match answer.jobs {
            Some((run_id, JobsAnswer::Fetched(Fetched::Fresh { data, etag }))) => {
                self.jobs.insert(run_id, CiJobs { etag, jobs: data });
            }
            Some((run_id, JobsAnswer::Gone)) => {
                self.jobs.remove(&run_id);
                self.gone.insert(run_id);
            }
            Some((_, JobsAnswer::Fetched(Fetched::NotModified))) | None => {}
        }
        if let Some((job_id, tail)) = answer.log {
            self.logs.insert(job_id, tail);
        }
        if let Some((job_id, lines)) = answer.full_log {
            self.full_logs
                .insert(job_id, crate::ci::Detail::Ready(lines));
        }
    }

    /// Land an inspector fetch's answer at `now` (FZ-30f).
    fn land_detail(&mut self, got: crate::ci::DetailGot, now: DateTime<Local>) {
        use crate::ci::{Detail, DetailGot, Fetched, JobsAnswer};
        match got {
            DetailGot::Jobs { run_id, result } => match result {
                Ok(JobsAnswer::Fetched(Fetched::Fresh { data, etag })) => {
                    self.jobs_fetch.remove(&run_id);
                    self.jobs.insert(run_id, CiJobs { etag, jobs: data });
                }
                Ok(JobsAnswer::Fetched(Fetched::NotModified)) => {
                    self.jobs_fetch.remove(&run_id);
                }
                Ok(JobsAnswer::Gone) => {
                    self.jobs_fetch.remove(&run_id);
                    self.jobs.remove(&run_id);
                    self.gone.insert(run_id);
                }
                Err(failure) => {
                    let held = Detail::from_result(Err::<(), _>(failure), now);
                    self.jobs_fetch.insert(run_id, held);
                }
            },
            DetailGot::Log {
                job_id,
                result,
                tail,
            } => {
                if let Some(tail) = tail {
                    self.logs.entry(job_id).or_insert(tail);
                }
                self.full_logs
                    .insert(job_id, Detail::from_result(result, now));
            }
            DetailGot::Script { sha, path, result } => {
                self.scripts
                    .insert((sha, path), Detail::from_result(result, now));
            }
        }
    }
}

/// The three bodies the inspector shows for the highlighted run while pane 2
/// shows `ci` (FZ-30f), cycled with `[` and `]` while the inspector has
/// focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CiBody {
    /// Each job with its lamp, duration and steps; a failed job's log tail.
    #[default]
    Jobs,
    /// The marked job's whole log.
    Log,
    /// The workflow file the run ran, the failing step highlighted.
    Script,
}

impl CiBody {
    pub const ALL: [Self; 3] = [Self::Jobs, Self::Log, Self::Script];

    /// Index into `App::ci_body_scrolls`.
    pub fn index(self) -> usize {
        match self {
            Self::Jobs => 0,
            Self::Log => 1,
            Self::Script => 2,
        }
    }

    /// The word the inspector's title names it by.
    pub fn label(self) -> &'static str {
        match self {
            Self::Jobs => "jobs",
            Self::Log => "log",
            Self::Script => "script",
        }
    }
}

/// The `jobs` body's scroll against its job cursor: `j`/`k` and a new run
/// ask the next frame to bring the marked job into view (`Due`), the frame
/// records the offset it chose (`At`), and a scroll by the wheel or a page
/// key goes on from there and leaves the cursor behind (`Free`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobFollow {
    Free,
    Due,
    At(u16),
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

/// A remote identity as assembly compares it: lowercased owner and name, no
/// owner for the legacy key of a payload without `login`
/// (`GithubRepo::identity_key`, the shape of `engine::remote_key`).
pub type RemoteKey = (Option<String>, String);

/// The remote key of the GitHub repo `repo` carries, or None when it carries
/// none. Follows `engine::carries`: a Primary, Elsewhere or NotCloned row is
/// keyed by its owner and name (name alone for a legacy row without owner);
/// a LocalOnly row only when the load made it a carrier by giving it an
/// owner, and then by the name its stored origin names, since its own `name`
/// is its folder's.
pub fn carrier_key(repo: &RepoStatus) -> Option<RemoteKey> {
    use crate::github::GithubRepo;
    if repo.location != RepoLocation::LocalOnly {
        return Some(GithubRepo::identity_key(repo.owner.as_deref(), &repo.name));
    }
    let owner = repo.owner.as_deref()?;
    let origin = crate::model::RemoteRef::parse(repo.remote_url.as_deref()?)?;
    Some(GithubRepo::identity_key(Some(owner), &origin.name))
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
            fleet_drawn: std::cell::Cell::new(false),
            follow_selection: std::cell::Cell::new(false),
            detail_row_tops: std::cell::RefCell::new(Vec::new()),
            detail_hits: std::cell::RefCell::new(Default::default()),
            graph_cursor: 0,
            detail_view_height: std::cell::Cell::new(0),
            inspector_max_scroll: std::cell::Cell::new(0),
            needs_redraw: true,
            spinner_frame: 0,
            blink_on: true,
            loading_field: None,
            view_overridden: false,
            repo_list_pct: DEFAULT_REPO_LIST_PCT,
            inspector_pct: DEFAULT_INSPECTOR_PCT,
            contributions: ContributionsMode::Github,
            inspector_folded: false,
            contributions_folded: false,
            github_calendar: None,
            contrib_author: None,
            contrib_cell: CellSize::default(),
            contrib_cursor: None,
            // Out of range on purpose: the renderer clamps it down to
            // "newest week at the right edge", which is where the grid opens.
            contrib_scroll: std::cell::Cell::new(crate::contributions::WEEKS),
            contrib_visible_weeks: std::cell::Cell::new(crate::contributions::WEEKS),
            persist_pane_sizes: false,
            show_pr_detail: false,
            dragging: None,
            expand_remote_branches: false,
            remotes_drawn_expanded: std::cell::Cell::new(false),
            contrib_drag: None,
            loading: false,
            sweep_started: None,
            sweep_manual: false,
            refresh_awaits_poll: false,
            refresh_fetches: std::collections::HashSet::new(),
            refresh_fetches_started: None,
            github_freshness: GithubFreshness::default(),
            api_budget: ApiBudget::default(),
            github_backoff: GithubBackoff::default(),
            fetch_backoff: FetchBackoff::default(),
            fetch_max_bytes: crate::config::Config::default().fetch_cap_bytes(),
            fetches_in_flight: FetchClaims::default(),
            github_poll_secs: crate::config::Config::default().github_poll_interval as i64,
            github_poll_cost: crate::github::poll_cost(crate::config::Config::default().repo_limit),
            jitter: Box::new(ClockJitter::new()),
            manual_fetches: FetchTally::default(),
            github_poll_inflight: false,
            github_poll_started: None,
            load_error: None,
            push_detected: false,
            dataset: Vec::new(),
            github_meta: crate::github::FetchMeta::default(),
            not_in_last_poll: std::collections::HashSet::new(),
            account_rebuild_due: false,
            window: None,
            dirty_mode: false,
            apply_progress: None,
            ci: std::collections::HashMap::new(),
            ci_budget: ApiBudget::default(),
            ci_cursor: None,
            ci_body: CiBody::default(),
            ci_body_scrolls: [0; 3],
            ci_job_mark: None,
            ci_job_follow: std::cell::Cell::new(JobFollow::Free),
            brackets_cycling_views: false,
            pending_yank: false,
            yank_flash: YankFlash::default(),
            ci_inspected: None,
            ci_log_wrap: std::cell::RefCell::new(None),
            worktree_logs: std::collections::HashMap::new(),
            worktree_logs_generation: 0,
            worktrees_cursor: None,
            notes: std::collections::HashMap::new(),
            notes_generation: 0,
            notes_all_markdown: false,
            notes_cursor: None,
            note_body: None,
            todoz_fallback_said: false,
            reader_scroll: 0,
            reader_max_scroll: std::cell::Cell::new(0),
            reader_view_height: std::cell::Cell::new(0),
            reader_width: std::cell::Cell::new(0),
            main_area_width: std::cell::Cell::new(0),
            reader_raw: false,
            list_stop: None,
            digest_window: crate::digest::DigestWindow::default(),
            digest_selected: 0,
            digest_logs: std::collections::HashMap::new(),
            work_hours: (9, 18),
            list_hits: std::cell::Cell::new(ListHits::default()),
            hover_pending: None,
            changes: std::collections::HashMap::new(),
            changes_generation: 0,
            changes_base: crate::git::ChangesBase::default(),
            changes_cursor: None,
            change_diff: None,
        }
    }

    /// How long the cursor has rested on `target` at `now`, restarting the
    /// clock when it moved to another one.
    pub fn ci_settled_for(
        &mut self,
        target: Option<&RemoteKey>,
        now: std::time::Instant,
    ) -> std::time::Duration {
        match &self.ci_cursor {
            Some((held, since)) if held.as_ref() == target => now.duration_since(*since),
            _ => {
                self.ci_cursor = Some((target.cloned(), now));
                std::time::Duration::ZERO
            }
        }
    }

    /// The cadence inputs for the repo at `key`, judged at `now`.
    pub fn ci_cadence(
        &self,
        key: &RemoteKey,
        settled_for: std::time::Duration,
        now: DateTime<Local>,
    ) -> crate::ci::Cadence<'_> {
        let state = self.ci.get(key);
        let runs = state.map_or(&[][..], |s| &s.runs);
        let last_attempt = state.and_then(|s| s.freshness.last_attempt);
        let cost = state.map_or(crate::ci::CALL_COST, CiState::poll_cost);
        crate::ci::Cadence {
            folded: !self.ci_wanted(),
            has_identity: true,
            settled_for,
            inflight: state.is_some_and(|s| s.inflight),
            runs,
            last_attempt,
            pace: self.ci_budget.pace(cost, last_attempt.unwrap_or(now)),
            not_before: state
                .and_then(|s| s.backoff.not_before(crate::ci::base_interval_secs(runs))),
            now,
        }
    }

    /// When the CI rate limit on `key`'s repo lifts, if one holds it at
    /// `now`: its backoff's limit hold, or the `core` budget's reset once
    /// it is spent. The pane's `rate limited until 12:40`.
    pub fn ci_limit_until(&self, key: &RemoteKey, now: DateTime<Local>) -> Option<DateTime<Local>> {
        let state = self.ci.get(key)?;
        let interval = crate::ci::base_interval_secs(&state.runs);
        let limit = state.backoff.limit_until(interval).filter(|t| *t > now);
        let exhausted = self
            .ci_budget
            .exhausted(state.poll_cost(), now)
            .then(|| self.ci_budget.resume_at())
            .flatten();
        limit.max(exhausted)
    }

    /// The selected repo's CI target and state, as the `ci` view draws them:
    /// `None` when the row has no GitHub identity, and a `None` state while
    /// nothing was ever claimed for it.
    pub fn selected_ci(&self) -> Option<(RemoteKey, Option<&CiState>)> {
        let (key, ..) = self.selected_repo().and_then(crate::ci::target)?;
        let state = self.ci.get(&key);
        Some((key, state))
    }

    /// The CI state to show for `repo` (FZ-83), the one answer the row
    /// lamp, pane 2's header and the `info` line draw: the fleet poll's
    /// `ci_status`, unless every context of the default branch's tip is held
    /// from the `ci` view and was confirmed after the fleet poll's data time
    /// (`GithubFreshness::data_time`, `docs/freshness-contract.md`); then the
    /// verdict of those contexts, with its culprit. The branch is named
    /// whenever a held run ran on the held tip.
    pub fn ci_shown(&self, repo: &RepoStatus) -> CiShown {
        let state = crate::ci::target(repo).and_then(|(key, ..)| self.ci.get(&key));
        let tip = state.and_then(|s| Some((s, s.tip.as_ref()?)));
        let branch = tip.and_then(|(s, t)| {
            s.runs
                .iter()
                .find(|r| r.head_sha.as_deref() == Some(t.sha.as_str()))
                .and_then(|r| r.branch.clone())
        });
        let fresher = tip
            .map(|(_, t)| t)
            .filter(|t| t.complete() && self.newer_than_fleet(t.confirmed));
        match fresher {
            Some(t) => {
                let (status, culprit) = crate::ci::tip_verdict(t.contexts());
                CiShown {
                    status,
                    branch,
                    culprit,
                }
            }
            None => CiShown {
                status: repo.ci_status.clone(),
                branch,
                culprit: None,
            },
        }
    }

    /// Whether data confirmed at `at` is newer than the fleet poll's: later
    /// than its data time, or any time while that is unknown or absent.
    fn newer_than_fleet(&self, at: Option<DateTime<Local>>) -> bool {
        let Some(at) = at else {
            return false;
        };
        match self.github_freshness.data_time {
            DataTime::At(fleet) => at > fleet,
            DataTime::Unknown | DataTime::NoData => true,
        }
    }

    /// Whether `repo`'s row shows the second, dimmer mark (FZ-83): the held
    /// runs, newer than the fleet poll, have one in progress on a commit
    /// other than the default branch's held tip. Never while the tip is not
    /// known, since then no run can be told apart from the default branch's.
    pub fn ci_side_run(&self, repo: &RepoStatus) -> bool {
        let Some(state) = crate::ci::target(repo).and_then(|(key, ..)| self.ci.get(&key)) else {
            return false;
        };
        let Some(tip) = &state.tip else {
            return false;
        };
        let runs_at = match state.freshness.data_time {
            DataTime::At(t) => Some(t),
            DataTime::Unknown | DataTime::NoData => None,
        };
        self.newer_than_fleet(runs_at)
            && state
                .runs
                .iter()
                .any(|r| r.is_live() && r.head_sha.as_deref() != Some(tip.sha.as_str()))
    }

    /// The selected repo's `ci` list as the view draws it (FZ-83): the held
    /// runs and the tip's other contexts (`CiTip::context_runs`), newest
    /// first by the time each row's age counts from, a run's creation or a
    /// context's `at`; a context with no time leads. One cursor stop each.
    pub fn ci_entries(&self) -> Vec<CiEntry<'_>> {
        let Some(state) = self.selected_ci().and_then(|(_, state)| state) else {
            return Vec::new();
        };
        let mut contexts = state
            .tip
            .as_ref()
            .map(CiTip::context_runs)
            .unwrap_or_default();
        contexts.sort_by(|a, b| match (a.at, b.at) {
            (Some(a), Some(b)) => b.cmp(&a),
            (a, b) => a.is_some().cmp(&b.is_some()),
        });
        let mut contexts = contexts.into_iter().peekable();
        let mut entries = Vec::new();
        for run in &state.runs {
            while let Some(context) = contexts.next_if(|c| c.at.is_none_or(|at| at >= run.created))
            {
                entries.push(CiEntry::Context(context));
            }
            entries.push(CiEntry::Run(run));
        }
        entries.extend(contexts.map(CiEntry::Context));
        entries
    }

    /// The entry under the `ci` view's cursor, clamped onto the last as the
    /// view draws it.
    fn highlighted_ci_entry(&self) -> Option<CiEntry<'_>> {
        let mut entries = self.ci_entries();
        let last = entries.len().checked_sub(1)?;
        let at = self.detail_selections[HistoryView::Ci.index()].min(last);
        Some(entries.swap_remove(at))
    }

    /// The other context under the `ci` view's cursor, if it is on one.
    pub fn highlighted_ci_context(&self) -> Option<crate::ci::ContextRun> {
        match self.highlighted_ci_entry()? {
            CiEntry::Context(context) => Some(context),
            CiEntry::Run(_) => None,
        }
    }

    /// The run under the `ci` view's cursor, clamped onto the last row as
    /// the view draws it; `None` while the selected repo holds no runs or
    /// the cursor is on another context's row.
    pub fn highlighted_ci_run(&self) -> Option<&crate::ci::WorkflowRun> {
        match self.highlighted_ci_entry()? {
            CiEntry::Run(run) => Some(run),
            CiEntry::Context(_) => None,
        }
    }

    /// Whether the CI data layer may poll: pane 2 is on screen showing the
    /// `ci` view. The one gate the poll trigger, the loading spinner and
    /// the stuck-poll expiry read. Never true for `info`, whose `ci` section
    /// draws only what is already held.
    pub fn ci_wanted(&self) -> bool {
        self.view == View::Detail && self.history_view == HistoryView::Ci
    }

    /// Whether the `ci` view draws the loading spinner, which repaints on
    /// every tick: the selected repo has a GitHub identity and no answer
    /// has landed for it yet.
    pub fn ci_view_loading(&self) -> bool {
        if !self.ci_wanted() || self.dirty_mode {
            return false;
        }
        match self.selected_ci() {
            Some((_, None)) => true,
            Some((_, Some(state))) => state.freshness.last_attempt.is_none(),
            None => false,
        }
    }

    /// Whether the `ci` view shows a run still moving, whose duration
    /// ticks and whose lamp pulses: about one repaint a second. A pending
    /// context's row (FZ-83) counts too.
    pub fn ci_view_live(&self) -> bool {
        self.ci_wanted()
            && self.ci_entries().iter().any(|entry| match entry {
                CiEntry::Run(run) => run.is_live(),
                CiEntry::Context(context) => context.state == CiStatus::Pending,
            })
    }

    /// Give up on CI polls claimed longer than `limit` before `instant` that
    /// never answered, as `main.rs::expire_stuck_github_poll` does for the
    /// fleet poll: the lock is released and the attempt recorded as a
    /// timed-out failure on that repo, which climbs its ladder. An answer
    /// that still arrives is applied like any.
    pub fn expire_stuck_ci_polls(
        &mut self,
        limit: std::time::Duration,
        instant: std::time::Instant,
        now: DateTime<Local>,
    ) {
        for state in self.ci.values_mut() {
            let stuck = state
                .started
                .is_some_and(|t| instant.saturating_duration_since(t) > limit);
            if !state.inflight || !stuck {
                continue;
            }
            state.inflight = false;
            state.started = None;
            state.freshness.gave_up(limit, now);
            if let Outcome::Failed(failure) = state.freshness.outcome.clone() {
                state
                    .backoff
                    .failed(&failure, now, &self.ci_budget, self.jitter.as_mut());
            }
            self.needs_redraw = true;
        }
    }

    /// Take the repo's in-flight lock and an attempt id, and build the poll.
    pub fn claim_ci_poll(
        &mut self,
        key: RemoteKey,
        owner: String,
        name: String,
    ) -> crate::ci::CiRequest {
        let state = self.ci.entry(key.clone()).or_default();
        state.inflight = true;
        state.started = Some(std::time::Instant::now());
        let attempt = state.freshness.begin_attempt();
        let expanded = state.expanded.map(|run_id| {
            let held = state.jobs.get(&run_id);
            crate::ci::ExpandedRequest {
                run_id,
                jobs_etag: held.and_then(|j| j.etag.clone()),
                held_jobs: held.map(|j| j.jobs.clone()).unwrap_or_default(),
                // A whole log the inspector holds or is reading brings its
                // tail with it, so the poll does not download it again.
                logs_held: state
                    .logs
                    .keys()
                    .chain(state.full_logs.iter().filter_map(|(id, held)| {
                        matches!(
                            held,
                            crate::ci::Detail::Loading | crate::ci::Detail::Ready(_)
                        )
                        .then_some(id)
                    }))
                    .copied()
                    .collect(),
            }
        });
        crate::ci::CiRequest {
            key,
            owner,
            name,
            attempt,
            runs_etag: state.runs_etag.clone(),
            expanded,
            tip: Some(crate::ci::TipRequest {
                sha: state.tip.as_ref().map(|t| t.sha.clone()),
                status_etag: state.tip.as_ref().and_then(|t| t.status_etag.clone()),
                checks_etag: state.tip.as_ref().and_then(|t| t.checks_etag.clone()),
                has_statuses: state.tip.as_ref().is_some_and(|t| !t.statuses.is_empty()),
                history_etag: state.tip.as_ref().and_then(|t| t.history_etag.clone()),
            }),
        }
    }

    /// Land a CI poll's answer (`AppEvent::Ci`) at `now`, and ask for a
    /// redraw. Only CI state changes (`ci`, `ci_budget`): never a row, the
    /// fleet poll's freshness, budget or backoff, or the cache.
    ///
    /// The fleet poll's rules, per repo: the newest attempt releases the lock;
    /// an answer older than one applied is discarded but may still lower the
    /// budget; a failure keeps the data and advances the repo's own retry
    /// clock; a 304 is a success that keeps the data. A list that answered
    /// (fresh or 304) advances the data time even when a later `/jobs` or log
    /// call failed the attempt: the runs on screen are that current, and the
    /// failure still marks the view and climbs the ladder.
    pub fn apply_ci(&mut self, answer: crate::ci::CiAnswer, now: DateTime<Local>) {
        self.needs_redraw = true;
        let Some(state) = self.ci.get_mut(&answer.key) else {
            return;
        };
        if state.freshness.is_latest(answer.attempt) {
            state.inflight = false;
            state.started = None;
        }
        let applied = state.freshness.accept_answer(answer.attempt);
        if let Some(reading) = &answer.budget
            && self
                .ci_budget
                .observe(reading, applied, now, self.jitter.as_mut())
                == Observed::Switched
        {
            for state in self.ci.values_mut() {
                state.backoff.account_switched();
            }
        }
        if !applied {
            return;
        }
        let Some(state) = self.ci.get_mut(&answer.key) else {
            return;
        };
        match answer.failure.clone() {
            Some(failure) => {
                state.freshness.failed(failure.clone(), now);
                if answer.runs.is_some() {
                    state.freshness.data_time = DataTime::At(now);
                }
                state
                    .backoff
                    .failed(&failure, now, &self.ci_budget, self.jitter.as_mut());
            }
            None => {
                state.freshness.succeeded(now);
                state.backoff.succeeded();
            }
        }
        let key = answer.key.clone();
        state.land(answer, now);
        // A shorter list for the repo on screen pulls the `ci` cursor onto
        // its last run rather than leaving it past the end.
        if self
            .selected_ci()
            .is_some_and(|(selected, _)| selected == key)
        {
            let rows = self.ci_entries().len();
            let slot = &mut self.detail_selections[HistoryView::Ci.index()];
            *slot = (*slot).min(rows.saturating_sub(1));
            self.sync_ci_inspector(std::time::Instant::now());
        }
    }

    /// Land an inspector fetch's answer (`AppEvent::CiDetail`) at `now`, and
    /// ask for a redraw. Its budget reading is taken like a poll's.
    pub fn apply_ci_detail(&mut self, answer: crate::ci::DetailAnswer, now: DateTime<Local>) {
        self.needs_redraw = true;
        if let Some(reading) = &answer.budget
            && self
                .ci_budget
                .observe(reading, true, now, self.jitter.as_mut())
                == Observed::Switched
        {
            for state in self.ci.values_mut() {
                state.backoff.account_switched();
            }
        }
        if let Some(state) = self.ci.get_mut(&answer.key) {
            state.land_detail(answer.got, now);
        }
    }

    /// The run the `ci` view's cursor is on for the selected repo, which the
    /// inspector details: read from the view's own selection slot, so it
    /// holds while another view is on screen.
    pub fn ci_highlighted_run(&self) -> Option<&crate::ci::WorkflowRun> {
        self.highlighted_ci_run()
    }

    /// The highlighted run's jobs, once read.
    pub fn ci_highlighted_jobs(&self) -> Option<&[crate::ci::Job]> {
        let run = self.ci_highlighted_run()?;
        let state = self.selected_ci()?.1?;
        state.jobs.get(&run.id).map(|j| &j.jobs[..])
    }

    /// Whether the scrolls and the mark belong to the highlighted run. Until
    /// `sync_ci_inspector` catches up with a change, they read as reset.
    fn ci_run_is_inspected(&self) -> bool {
        let run = self.ci_highlighted_run().map(|r| r.id);
        run.is_some() && run == self.ci_inspected.map(|(id, _)| id)
    }

    /// The index of the job the `jobs` body marks: where `j`/`k` left it,
    /// else the first failed job, else the first.
    pub fn ci_marked_job_index(&self) -> Option<usize> {
        let jobs = self.ci_highlighted_jobs()?;
        let last = jobs.len().checked_sub(1)?;
        let moved = self.ci_job_mark.filter(|_| self.ci_run_is_inspected());
        Some(match moved {
            Some(i) => i.min(last),
            None => jobs
                .iter()
                .position(|j| j.conclusion.as_deref() == Some("failure"))
                .unwrap_or(0),
        })
    }

    /// The marked job, whose log the `log` body shows.
    pub fn ci_marked_job(&self) -> Option<&crate::ci::Job> {
        self.ci_highlighted_jobs()?.get(self.ci_marked_job_index()?)
    }

    /// The shown body's scroll offset: zero for a run just highlighted. The
    /// `jobs` body's is where it last followed its cursor to, until a scroll
    /// moves it on.
    pub fn ci_body_scroll(&self) -> u16 {
        if !self.ci_run_is_inspected() {
            return 0;
        }
        match (self.ci_body, self.ci_job_follow.get()) {
            (CiBody::Jobs, JobFollow::At(followed)) => followed,
            _ => self.ci_body_scrolls[self.ci_body.index()],
        }
    }

    /// Catch up with the highlighted run at `instant`: a new run resets every
    /// body's scroll and the job mark, brings the marked job into view and
    /// restarts the dwell. While the `ci` view shows, the selected repo's
    /// poll reads the highlighted run's jobs when the inspector is open, and
    /// no run's when it is folded.
    pub fn sync_ci_inspector(&mut self, instant: std::time::Instant) {
        let run = self.ci_highlighted_run().map(|r| r.id);
        if run != self.ci_inspected.map(|(id, _)| id) {
            self.ci_inspected = run.map(|id| (id, instant));
            self.ci_body_scrolls = [0; 3];
            self.ci_job_mark = None;
            self.ci_job_follow.set(JobFollow::Due);
        }
        if !self.ci_wanted() {
            return;
        }
        let expanded = run.filter(|_| !self.inspector_folded);
        if let Some((key, _)) = self.selected_ci()
            && let Some(state) = self.ci.get_mut(&key)
        {
            state.expanded = expanded;
        }
    }

    /// `l` on the highlighted run in the `ci` view (FZ-30e, amended
    /// 2026-10-22): the inspector, opened if folded and focused as the
    /// pane's number key does, on the run's `log` body, which shows the
    /// marked job (the failed one, until `j`/`k` move it). Stays in fleetz
    /// rather than suspending into `gh run view --log`.
    pub fn inspect_ci_log(&mut self) {
        self.inspector_folded = false;
        self.focus_panel(FocusedPanel::Inspector);
        self.sync_ci_inspector(std::time::Instant::now());
        self.ci_body = CiBody::Log;
        self.needs_redraw = true;
    }

    /// Whether the inspector shows a run body: pane 2 shows `ci` and the
    /// inspector is open.
    pub fn inspects_ci_run(&self) -> bool {
        self.ci_wanted() && !self.inspector_folded
    }

    /// Whether the `changes` view may read a repo or a diff: pane 2 is on
    /// screen showing it. The one gate both reads pass (FZ-60).
    pub fn changes_wanted(&self) -> bool {
        self.view == View::Detail && self.history_view == HistoryView::Changes
    }

    /// The selected row's read, by its path; `None` for a row with nothing
    /// on disk to read, and a `None` state while nothing was read yet.
    pub fn selected_changes(&self) -> Option<(&str, Option<&ChangesState>)> {
        let path = self.selected_repo()?.path.as_deref()?;
        Some((path, self.changes.get(path)))
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
        let settled_for = match &self.changes_cursor {
            Some((held, since)) if *held == key => instant.duration_since(*since),
            _ => {
                self.changes_cursor = Some((key.clone(), instant));
                std::time::Duration::ZERO
            }
        };
        let key = key?;
        if settled_for < crate::ci::DWELL {
            return None;
        }
        let (generation, base) = (self.changes_generation, self.changes_base);
        let state = self.changes.entry(key.clone()).or_default();
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
        let state = self.changes.entry(read.key).or_default();
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
            generation: self.changes_generation,
        };
        let held = self.change_diff.as_ref().map(|d| &d.read);
        if held == Some(&read) {
            return None;
        }
        if held.is_none_or(|h| {
            h.root != read.root || h.file.path != read.file.path || h.section != read.section
        }) {
            self.inspector_scroll = 0;
        }
        self.change_diff = Some(ChangeDiff {
            read: read.clone(),
            text: None,
        });
        Some(read)
    }

    /// Land a diff (`AppEvent::ChangeDiff`); one no longer wanted is dropped.
    pub fn apply_change_diff(&mut self, read: DiffRead, text: Result<String, String>) {
        if let Some(held) = &mut self.change_diff
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
        self.change_diff.as_ref().filter(|d| {
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
        self.changes_generation += 1;
        self.needs_redraw = true;
    }

    /// `u` in the `changes` view: compare `HEAD` with the default branch
    /// instead of the upstream, or back. Nothing elsewhere.
    pub fn toggle_changes_base(&mut self) {
        if !self.changes_wanted() {
            return;
        }
        use crate::git::ChangesBase;
        self.changes_base = match self.changes_base {
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
        self.worktree_logs.get(&wt.path)?.commits.as_deref()
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
        let settled_for = match &self.worktrees_cursor {
            Some((held, since)) if *held == repo => instant.duration_since(*since),
            _ => {
                self.worktrees_cursor = Some((repo, instant));
                std::time::Duration::ZERO
            }
        };
        if settled_for < crate::ci::DWELL {
            return None;
        }
        let key = self.worktree_highlighted()?.path;
        let generation = self.worktree_logs_generation;
        let state = self.worktree_logs.entry(key.clone()).or_default();
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
        let state = self.worktree_logs.entry(read.key).or_default();
        state.reading = false;
        state.commits = Some(commits);
        state.generation = read.generation;
        self.needs_redraw = true;
    }

    /// Whether the `notes` view may walk a repo or read a file: pane 2 is on
    /// screen showing it. The one gate the walk and the body read pass.
    pub fn notes_wanted(&self) -> bool {
        self.view == View::Detail && self.history_view == HistoryView::Notes
    }

    /// The selected row's walk, by its path; `None` for a row with nothing
    /// on disk to walk, and a `None` state while nothing was walked yet.
    pub fn selected_notes(&self) -> Option<(&str, Option<&NotesState>)> {
        let path = self.selected_repo()?.path.as_deref()?;
        Some((path, self.notes.get(path)))
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
            )) => notes.listed(self.notes_all_markdown),
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
        let settled_for = match &self.notes_cursor {
            Some((held, since)) if *held == key => instant.duration_since(*since),
            _ => {
                self.notes_cursor = Some((key.clone(), instant));
                std::time::Duration::ZERO
            }
        };
        let key = key?;
        if settled_for < crate::ci::DWELL {
            return None;
        }
        let generation = self.notes_generation;
        let state = self.notes.entry(key.clone()).or_default();
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
        let state = self.notes.entry(walk.key).or_default();
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
        let generation = self.notes_generation;
        let held = self.note_body.as_ref();
        if held.is_some_and(|b| b.path == path && b.generation == generation) {
            return None;
        }
        if held.is_none_or(|b| b.path != path) {
            self.reader_scroll = 0;
        }
        let render_width = self.markdown_render_width();
        self.note_body = Some(NoteBody {
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
    /// Rendered whatever `reader_raw` says, so `p` flips at once.
    fn markdown_render_width(&self) -> Option<u16> {
        let width = self.reader_width.get();
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
        let body = self.note_body.as_mut().filter(|b| b.path == path)?;
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
        if let Some(body) = &mut self.note_body
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
        self.reader_raw = !self.reader_raw;
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
        self.note_body
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
        let max = i32::from(self.reader_max_scroll.get());
        self.reader_scroll = (i32::from(self.reader_scroll) + delta).clamp(0, max) as u16;
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
        let page = i32::from(self.reader_view_height.get().max(1));
        self.scroll_reader(pages * page);
    }

    /// Land a body read (`AppEvent::NoteRead`); one for a file or a
    /// generation no longer wanted is dropped.
    pub fn apply_note_read(&mut self, read: NoteRead, text: Result<String, String>) {
        if let Some(body) = &mut self.note_body
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
        self.notes_generation += 1;
        self.needs_redraw = true;
    }

    /// A sweep finished: close its books, stamp the refresh time and have
    /// the notes walked again. The `SweepDone` arm of the event loop calls it.
    pub fn sweep_landed(&mut self, at: chrono::DateTime<chrono::Local>) {
        self.sweep_started = None;
        self.last_refreshed = Some(at);
        self.invalidate_notes();
        self.invalidate_changes();
        self.worktree_logs_generation += 1;
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
        self.notes_all_markdown = !self.notes_all_markdown;
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

    /// `[` or `]`: the next inspector body while the inspector has focus and
    /// holds a run, else pane 2's next view as always. A streak of them
    /// begun on the focused inspector in another view cycles on through
    /// `ci` (`brackets_cycling_views`); any other key ends it.
    pub fn press_bracket(&mut self, back: bool) {
        let on_inspector = self.focused_panel == FocusedPanel::Inspector;
        let holds_run = self.inspects_ci_run() && self.ci_highlighted_run().is_some();
        if !on_inspector || !holds_run || self.brackets_cycling_views {
            self.cycle_history_view(back);
            self.brackets_cycling_views = on_inspector;
            return;
        }
        self.sync_ci_inspector(std::time::Instant::now());
        let n = CiBody::ALL.len();
        let i = self.ci_body.index();
        let next = if back { (i + n - 1) % n } else { (i + 1) % n };
        self.ci_body = CiBody::ALL[next];
        self.needs_redraw = true;
    }

    /// `j`/`k` on the focused inspector: in the `jobs` body they move the
    /// job mark, and the body follows it; anywhere else they scroll.
    pub fn inspector_nav(&mut self, delta: i32) {
        if !self.inspects_ci_run() || self.ci_body != CiBody::Jobs {
            self.scroll_inspector(delta);
            return;
        }
        self.sync_ci_inspector(std::time::Instant::now());
        let Some(current) = self.ci_marked_job_index() else {
            return;
        };
        let last = self.ci_highlighted_jobs().map_or(0, |j| j.len() - 1);
        let next = (current as i64 + i64::from(delta)).clamp(0, last as i64) as usize;
        self.ci_job_mark = Some(next);
        // The follow starts from where the body stands, not where it was
        // last scrolled by hand.
        self.ci_body_scrolls[CiBody::Jobs.index()] = self.ci_body_scroll();
        self.ci_job_follow.set(JobFollow::Due);
        self.needs_redraw = true;
    }

    /// The inspector fetches the shown body needs for the highlighted run,
    /// each claimed (marked loading) so it starts once: the run's jobs when
    /// no poll has read them, the marked job's whole log once the job has
    /// finished, the workflow file at the run's commit (and the jobs, for
    /// the failing step's highlight, when the run failed). Nothing outside
    /// the `ci` view, under `-d`, with the inspector folded, before the run
    /// has been highlighted for `ci::DWELL`, or while the `core` budget's
    /// pace is paused (at or below the reserve, or spent).
    pub fn claim_ci_details(
        &mut self,
        now: DateTime<Local>,
        instant: std::time::Instant,
    ) -> Vec<crate::ci::DetailRequest> {
        use crate::ci::{Detail, DetailRequest, DetailWant};
        if !self.inspects_ci_run() || self.dirty_mode {
            return Vec::new();
        }
        self.sync_ci_inspector(instant);
        let Some((_, since)) = self.ci_inspected else {
            return Vec::new();
        };
        if instant.saturating_duration_since(since) < crate::ci::DWELL {
            return Vec::new();
        }
        // The poll's own pace: at or below the reserve, or with too little
        // left for one call (spent), paused until the resume.
        if let Pace::Paused { until } = self.ci_budget.pace(crate::ci::CALL_COST, now)
            && now < until
        {
            return Vec::new();
        }
        let Some((key, owner, name)) = self.selected_repo().and_then(crate::ci::target) else {
            return Vec::new();
        };
        let Some(run) = self.ci_highlighted_run().cloned() else {
            return Vec::new();
        };
        let clone = self
            .selected_repo()
            .and_then(|r| r.path.as_deref())
            .map(|p| std::path::PathBuf::from(shellexpand::tilde(p).to_string()));
        let marked = self.ci_marked_job().cloned();
        let Some(state) = self.ci.get_mut(&key) else {
            return Vec::new();
        };
        let mut wants = Vec::new();
        let failed = crate::ui::ci::run_status(&run) == crate::model::CiStatus::Failed;
        let needs_jobs = match self.ci_body {
            CiBody::Jobs | CiBody::Log => true,
            CiBody::Script => failed,
        };
        if needs_jobs
            && !state.jobs.contains_key(&run.id)
            && !state.gone.contains(&run.id)
            && Detail::wants_fetch(state.jobs_fetch.get(&run.id), now)
        {
            state.jobs_fetch.insert(run.id, Detail::Loading);
            wants.push(DetailWant::Jobs { run_id: run.id });
        }
        // The jobs body needs the marked job's log only for a failed job's
        // tail; the log body needs any finished job's. A poll in flight may
        // bring a failed job's log with its jobs, so a claim waits for it
        // rather than download the same log twice.
        let wants_log = |job: &crate::ci::Job| match self.ci_body {
            CiBody::Jobs => {
                job.conclusion.as_deref() == Some("failure")
                    && crate::ci::failing_step(job).is_some()
                    && !state.logs.contains_key(&job.id)
            }
            CiBody::Log => true,
            CiBody::Script => false,
        };
        match self.ci_body {
            CiBody::Jobs | CiBody::Log => {
                if let Some(job) = marked
                    && wants_log(&job)
                    && job.status == "completed"
                    && !state.inflight
                    && Detail::wants_fetch(state.full_logs.get(&job.id), now)
                {
                    let step = crate::ci::failing_step(&job)
                        .filter(|_| !state.logs.contains_key(&job.id))
                        .cloned();
                    state.full_logs.insert(job.id, Detail::Loading);
                    wants.push(DetailWant::Log {
                        job_id: job.id,
                        step,
                    });
                }
            }
            CiBody::Script => {
                if let (Some(sha), Some(path)) = (run.head_sha.clone(), run.path.clone()) {
                    let held = (sha.clone(), path.clone());
                    if Detail::wants_fetch(state.scripts.get(&held), now) {
                        state.scripts.insert(held, Detail::Loading);
                        wants.push(DetailWant::Script { sha, path, clone });
                    }
                }
            }
        }
        wants
            .into_iter()
            .map(|want| DetailRequest {
                key: key.clone(),
                owner: owner.clone(),
                name: name.clone(),
                want,
            })
            .collect()
    }

    /// Whether `repo` carries GitHub fields the last poll did not return, so
    /// they are from an earlier refresh. False for a row that carries
    /// nothing.
    pub fn is_not_in_last_poll(&self, repo: &RepoStatus) -> bool {
        carrier_key(repo).is_some_and(|key| self.not_in_last_poll.contains(&key))
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

    /// Advance the GitHub calendar to the local date, called on every tick
    /// with the clock. The poll stamps the calendar's `today`, so without
    /// this the strip drew yesterday as its last live square after midnight
    /// until the next poll (#78). When the grid shifts by whole weeks, the
    /// cursor and the scroll follow so they stay on the same dates. A view
    /// pinned to the newest edge stays pinned, or the new Sunday's column
    /// would open just off the right edge.
    pub fn observe_day(&mut self, today: chrono::NaiveDate) {
        let Some(calendar) = self.github_calendar.as_deref_mut() else {
            return;
        };
        let before = calendar.start();
        if !calendar.advance_to(today) {
            return;
        }
        let weeks = ((calendar.start() - before).num_days() / 7) as usize;
        if weeks > 0 {
            self.contrib_cursor = self.contrib_cursor.map(|c| c.saturating_sub(7 * weeks));
            let newest_edge =
                crate::contributions::WEEKS.saturating_sub(self.contrib_visible_weeks.get().max(1));
            let scroll = self.contrib_scroll.get();
            if scroll < newest_edge {
                self.contrib_scroll.set(scroll.saturating_sub(weeks));
            }
        }
        self.needs_redraw = true;
    }

    /// Which half of a manual `r` is running, or None when none is: first its
    /// local sweep, then the GitHub poll it started and the fetches that poll
    /// set off. The spinner and the lit `[r]` key both read this, so neither
    /// stops before the last row lands. A watch-timer sweep does not count; it
    /// runs silently, unless an `r` pressed during it marked it manual.
    pub fn refresh_phase(&self) -> Option<RefreshPhase> {
        if self.sweep_started.is_some() && self.sweep_manual {
            Some(RefreshPhase::Local)
        } else if self.refresh_awaits_poll || !self.refresh_fetches.is_empty() {
            Some(RefreshPhase::Remote)
        } else {
            None
        }
    }

    /// Whether a manual `r` is still in flight, in either half.
    pub fn is_refreshing(&self) -> bool {
        self.refresh_phase().is_some()
    }

    /// Hand the fetches a GitHub poll set off to the manual refresh waiting
    /// on that poll, if there is one. A poll nobody pressed `r` for (the
    /// watch timer) fetches silently and adds nothing.
    pub fn settle_refresh_poll(&mut self, fetched_paths: impl IntoIterator<Item = String>) {
        if !std::mem::take(&mut self.refresh_awaits_poll) {
            return;
        }
        let fetched_paths: Vec<String> = fetched_paths.into_iter().collect();
        self.manual_fetches.expect(fetched_paths.iter().cloned());
        self.refresh_fetches.extend(fetched_paths);
        if !self.refresh_fetches.is_empty() {
            self.refresh_fetches_started = Some(std::time::Instant::now());
        }
    }

    /// A repo's update landed. Only one whose fetch finished settles a
    /// pending refresh fetch, whether it succeeded or failed: the local sweep
    /// can report the same repo before its fetch has run, and that update does
    /// not carry the remote's news yet.
    pub fn settle_refresh_fetch(&mut self, path: &str, fetch: &crate::git::FetchOutcome) {
        let finished = *fetch != crate::git::FetchOutcome::NotAttempted;
        if finished && self.refresh_fetches.remove(path) && self.refresh_fetches.is_empty() {
            self.refresh_fetches_started = None;
        }
    }

    /// Whether an automatic fetch (watch sweep, moved remote, recovery
    /// refetch) skips `path` at `now`: a recent timeout holds it, measured in
    /// watch intervals (`FetchBackoff`).
    pub fn fetch_held(&self, path: &str, now: DateTime<Local>) -> bool {
        self.fetch_backoff
            .holds(path, now, self.refresh_interval as i64)
    }

    /// Claim `path` for a fetch starting at `now`. False when a fetch of it is
    /// already running: the caller sweeps the repo without fetching, and the
    /// running fetch's outcome is the one that lands (R5).
    pub fn claim_fetch(&mut self, path: &str, now: std::time::Instant) -> bool {
        self.fetches_in_flight.reserve(path, now).is_some()
    }

    /// A sweep's result for `path` landed. Only one that fetched ends the
    /// claim: a sweep that skipped the fetch reports `NotAttempted` while the
    /// claimed fetch is still running.
    #[cfg(test)]
    pub fn fetch_landed(&mut self, path: &str, fetch: &crate::git::FetchOutcome) {
        self.fetch_landed_under(path, fetch, None);
    }

    /// `fetch_landed` for the fetch that held `claim`: it ends that claim
    /// only, so a landing that outlived its claim leaves a newer one alone
    /// (`FetchClaims::release`). `None` ends whichever claim holds the path.
    pub fn fetch_landed_under(
        &mut self,
        path: &str,
        fetch: &crate::git::FetchOutcome,
        claim: Option<u64>,
    ) {
        if *fetch != crate::git::FetchOutcome::NotAttempted {
            self.fetches_in_flight.release(path, claim);
        }
    }

    /// Drop claims with no sign of life for longer than `limit` at `now`, the
    /// safety net for a fetch whose result never landed
    /// (`SWEEP_STUCK_AFTER`). A row whose claim went this way stops showing
    /// the fetch's progress: nothing will land to clear it.
    pub fn expire_fetches_in_flight(
        &mut self,
        limit: std::time::Duration,
        now: std::time::Instant,
    ) {
        self.fetches_in_flight.expire(limit, now);
        for repo in &mut self.repos {
            if repo.fetch.progress.is_some()
                && !repo
                    .path
                    .as_deref()
                    .is_some_and(|p| self.fetches_in_flight.contains_key(p))
            {
                repo.fetch.progress = None;
                self.needs_redraw = true;
            }
        }
    }

    /// A progress update for the fetch running on `path`: drawn on its row
    /// (`ui::fetch_marker`) until the fetch lands (`FetchFacts::record`).
    /// Also a sign of life for a refresh or a fetch report waiting on that
    /// fetch, which then expire from their last update rather than from
    /// their start, as the fetch's claim does (FZ-35b, criterion 7).
    pub fn apply_fetch_progress(&mut self, path: &str, progress: crate::git::FetchProgress) {
        let now = std::time::Instant::now();
        self.manual_fetches.touch(path, now);
        if self.refresh_fetches.contains(path) {
            self.refresh_fetches_started = Some(now);
        }
        if let Some(repo) = self
            .repos
            .iter_mut()
            .find(|r| r.path.as_deref() == Some(path))
        {
            repo.fetch.progress = Some(progress);
            self.needs_redraw = true;
        }
    }

    /// Give up on refresh fetches with no sign of life for longer than
    /// `limit`, and on a fetch report silent for as long.
    pub fn expire_refresh_fetches(&mut self, limit: std::time::Duration) {
        if self
            .refresh_fetches_started
            .is_some_and(|t| t.elapsed() > limit)
        {
            self.refresh_fetches.clear();
            self.refresh_fetches_started = None;
        }
        self.manual_fetches.expire(limit);
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

    /// Stable-sort `repos` by `keys` (one per row, in row order), keeping the
    /// cursor on the repo it stood on (FZ-86). The caller supplies the keys,
    /// so the order has one source (`main.rs::order_rows_by_activity`).
    ///
    /// A search in progress is left alone: its ranked list breaks ties by row
    /// order, so moving rows under it would reshuffle what is being read. The
    /// next update after the search ends applies the order. False, with no
    /// redraw asked, when nothing moved.
    pub fn reorder_repos<K: Ord>(&mut self, keys: Vec<K>) -> bool {
        if self.has_text_query() || keys.len() != self.repos.len() {
            return false;
        }
        let mut order: Vec<usize> = (0..keys.len()).collect();
        order.sort_by(|&a, &b| keys[a].cmp(&keys[b]));
        if order.iter().enumerate().all(|(new, &old)| new == old) {
            return false;
        }
        // None on a section header, and there the cursor needs no help: the
        // sections are by location, which no re-sort changes, so each one
        // starts at the same visible position before and after.
        let held = self.selected_repo_index();

        let mut rows: Vec<Option<RepoStatus>> = std::mem::take(&mut self.repos)
            .into_iter()
            .map(Some)
            .collect();
        self.repos = order.iter().filter_map(|&old| rows[old].take()).collect();

        if let Some(held) = held {
            let moved_to = order.iter().position(|&old| old == held);
            if let Some(pos) = self
                .visible_repos()
                .iter()
                .position(|(i, _)| Some(*i) == moved_to)
            {
                self.selected = pos;
            }
        }
        self.needs_redraw = true;
        true
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
    ///
    /// The `notes` layout docks none either: the file's facts sit in pane 2
    /// beside the header (FZ-68), and `ui::panel_rects` gives pane 1 the rows.
    pub fn shows_inspector(&self) -> bool {
        self.history_view != HistoryView::Graph && !self.notes_layout()
    }

    /// Whether the inspector is drawn with its content, not folded to an
    /// empty frame. Shift-scroll aims only at a pane whose rows it can see.
    fn inspector_is_expanded(&self) -> bool {
        self.shows_inspector() && !self.inspector_folded
    }

    /// The fold flag of a foldable pane; None for the two that never fold,
    /// the repo list and the detail pane, which are the dashboard itself.
    fn fold_flag(&mut self, panel: FocusedPanel) -> Option<&mut bool> {
        match panel {
            FocusedPanel::Inspector => Some(&mut self.inspector_folded),
            FocusedPanel::Contributions => Some(&mut self.contributions_folded),
            FocusedPanel::RepoList | FocusedPanel::Detail | FocusedPanel::Reader => None,
        }
    }

    /// A pane's number key (`1`-`4`). On a pane that is not focused it
    /// focuses it, unfolding it if folded. On the focused pane it folds or
    /// unfolds it in place, keeping focus, so the same key both reaches a
    /// pane and gets it out of the way.
    pub fn press_panel_key(&mut self, panel: FocusedPanel) {
        let focused = self.focused_panel == panel;
        self.focus_panel(panel);
        // Focus refused: the pane is not on screen (the graph view's
        // inspector), so there is nothing to fold either.
        if self.focused_panel != panel {
            return;
        }
        if let Some(folded) = self.fold_flag(panel) {
            *folded = focused && !*folded;
        }
        self.contributions_unfolded(panel, chrono::Local::now().date_naive());
        self.needs_redraw = true;
    }

    /// Fold `panel` to an empty frame, or unfold it: the click on a pane's
    /// `[3]`/`[4]` tag. Unfolding focuses the pane, since opening it is
    /// asking to look at it; folding leaves focus where it is, the same as
    /// the number key does.
    pub fn toggle_fold(&mut self, panel: FocusedPanel) {
        let Some(folded) = self.fold_flag(panel) else {
            return;
        };
        *folded = !*folded;
        if !*folded {
            self.focus_panel(panel);
        }
        self.contributions_unfolded(panel, chrono::Local::now().date_naive());
        self.needs_redraw = true;
    }

    /// The one unfold entry point the `4` key and the `[4]` tag click share:
    /// a strip on screen with no day marked yet (launched with the setting
    /// off, so never opened) stands on `today`, with today's column in view,
    /// exactly as opening it does (#66). A strip that was travelled keeps its
    /// day: folding is getting it out of the way, not resetting it.
    fn contributions_unfolded(&mut self, panel: FocusedPanel, today: chrono::NaiveDate) {
        if panel != FocusedPanel::Contributions
            || self.contributions_folded
            || self.contrib_cursor.is_some()
        {
            return;
        }
        self.contrib_cursor = Some(self.contrib_last_slot(today));
        // Out of range on purpose, as in `open_contributions`: the renderer
        // clamps it to "newest week at the right edge", which holds today.
        self.contrib_scroll.set(crate::contributions::WEEKS);
    }

    pub fn visible_repos(&self) -> Vec<(usize, &RepoStatus)> {
        let active_group = self.active_group.and_then(|i| self.group_names.get(i));
        let parsed = parse_query(&self.search_query);

        let mut visible: Vec<(usize, &RepoStatus, (MatchField, i32))> = self
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
                // Name, owner or author: None means no field matched.
                let hit = text_match(&parsed.text, r)?;
                Some((i, r, hit))
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
            // The field's tier leads the key, so name hits stay on top in
            // this one sort rather than a second pass over the drawn rows.
            visible.sort_by_key(|(_, _, (field, score))| (field.tier(), std::cmp::Reverse(*score)));
        }

        visible.into_iter().map(|(i, r, _)| (i, r)).collect()
    }

    /// True when the query has a free-text portion to rank by, as opposed to
    /// being empty or nothing but `#tag` words. Decides both the sort order
    /// above and whether the repo list draws sections.
    pub fn has_text_query(&self) -> bool {
        !parse_query(&self.search_query).text.is_empty()
    }

    /// The field `repo` matched the free text on, when that is not its
    /// name: what the ranked list marks after the name. None with no text
    /// typed, on a name hit, or for a row the query does not match.
    pub fn search_match_field(&self, repo: &RepoStatus) -> Option<MatchField> {
        let text = parse_query(&self.search_query).text;
        if text.is_empty() {
            return None;
        }
        text_match(&text, repo)
            .map(|(field, _)| field)
            .filter(|field| *field != MatchField::Name)
    }

    /// Move the vertical divider between the repo list and the right column.
    /// Positive widens the repo list.
    ///
    /// The step starts from the width on screen (FZ-69). Where the default
    /// yields to the cap in `split_main`, the percentage is not what is
    /// drawn, and stepping it blind made `<` widen the list.
    pub fn resize_repo_list(&mut self, delta: i16) {
        use crate::ui::detail::repo_list_width;
        let area = self.main_area_width.get();
        let pct = self.repo_list_pct;
        let drawn = repo_list_width(area, pct);
        // The percentage that draws `drawn`, rounded up as a drag converts
        // a column, so the start is the list on screen.
        let uncapped = u32::from(area) * u32::from(pct) / 100;
        let from = if area > 0 && u32::from(drawn) < uncapped {
            (u32::from(drawn) * 100).div_ceil(u32::from(area)) as u16
        } else {
            pct
        };
        let mut next = step_pct(from, delta, REPO_LIST_PCT_MIN, REPO_LIST_PCT_MAX);
        // Landing back on the default would draw the capped width again.
        if delta > 0
            && area > 0
            && next == DEFAULT_REPO_LIST_PCT
            && repo_list_width(area, next) <= drawn
        {
            next = (next + 1).min(REPO_LIST_PCT_MAX);
        }
        // Already narrower than the floor draws: `<` has nowhere to go.
        if delta < 0 && area > 0 && repo_list_width(area, next) > drawn {
            return;
        }
        self.repo_list_pct = next;
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
        self.list_stop = None;
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
        self.list_stop = None;
    }

    /// Clear group filter (show all).
    pub fn clear_group(&mut self) {
        self.active_group = None;
        self.selected = 0;
        self.list_stop = None;
    }

    /// Label for the active group, if any.
    pub fn active_group_label(&self) -> Option<&str> {
        self.active_group
            .and_then(|i| self.group_names.get(i))
            .map(|s| s.as_str())
    }

    /// The currently selected repo, if any.
    /// None while the cursor stands on a section header (`on_header`).
    pub fn selected_repo(&self) -> Option<&RepoStatus> {
        if self.on_header().is_some() {
            return None;
        }
        let visible = self.visible_repos();
        visible.get(self.selected).map(|(_, r)| *r)
    }

    /// Index into self.repos for the currently selected visible repo.
    pub fn selected_repo_index(&self) -> Option<usize> {
        if self.on_header().is_some() {
            return None;
        }
        let visible = self.visible_repos();
        visible.get(self.selected).map(|(i, _)| *i)
    }

    pub fn move_up(&mut self) {
        if self.step_list(-1) {
            return;
        }
        if self.selected > 0 {
            self.selected -= 1;
            self.reset_detail_view();
        }
    }

    pub fn move_down(&mut self) {
        if self.step_list(1) {
            return;
        }
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
            // Local branches then remotes, in the order the view draws them.
            HistoryView::Branches => repo.branches.len() + repo.remote_branches.len(),
            // The runs held for the repo and the tip's other contexts among
            // them (FZ-83), as the view draws them.
            HistoryView::Ci => self.ci_entries().len(),
            // The files the tree lists, in its current mode.
            HistoryView::Notes => self.notes_listed().len(),
            // The main checkout and the linked worktrees.
            HistoryView::Worktrees => self.worktrees_listed().len(),
            // Both sections' files, uncommitted first.
            HistoryView::Changes => self.changes_listed().len(),
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
        // Clamped, not bounds-checked: the index survives a repo change and a
        // view switch, so it can outlive the list it was chosen against. A
        // stale one resolves to the last row rather than to nothing, which is
        // what keeps the readout drawn instead of vanishing.
        let i = self
            .detail_selected()
            .min(self.selected_commit_count().saturating_sub(1));
        self.entry_hash(i)
    }

    /// The hash entry `i` of the active history view names, the one
    /// `inspected_hash` returns when the cursor is on it.
    pub fn entry_hash(&self, i: usize) -> Option<&str> {
        let repo = self.selected_repo()?;
        let graph_nth = |n: usize| repo.graph.get(n).map(|g| g.hash.as_str());
        match self.history_view {
            HistoryView::Info | HistoryView::Commits | HistoryView::Graph => graph_nth(i),
            HistoryView::Reflog => repo.reflog.get(i).map(|e| e.hash.as_str()),
            // A branch resolves to its own tip. Locals come first, so an index
            // past them names a remote. An empty tip is no answer at all: a
            // repo with no commits has branches but no objects to name.
            HistoryView::Branches => {
                let n = repo.branches.len();
                match i < n {
                    true => repo.branches.get(i).map(|b| b.tip.as_str()),
                    false => repo.remote_branches.get(i - n).map(|r| r.tip.as_str()),
                }
                .filter(|h| !h.is_empty())
            }
            // A run is not a commit of the loaded history; the inspector
            // details the run itself (`ci_highlighted_run`).
            HistoryView::Ci => None,
            // A file is not a commit; the inspector reads the file itself.
            HistoryView::Notes => None,
            // A worktree names its HEAD, as a branch names its tip; the main
            // checkout first, as the view draws it. Borrowed from the rows
            // the hash came from, not from a list built here.
            HistoryView::Worktrees => match i {
                0 => repo.main_worktree().and_then(|_| {
                    repo.branches
                        .iter()
                        .find(|b| b.is_head)
                        .map(|b| b.tip.as_str())
                        .filter(|t| !t.is_empty())
                        // Detached: the 7-digit form the row's
                        // `detached @ <sha>` names (`main_worktree`).
                        .or(repo
                            .work_key
                            .as_ref()
                            .map(|k| k.head_sha.get(..7).unwrap_or(&k.head_sha)))
                }),
                _ => repo.worktrees.get(i - 1).map(|w| w.head.as_str()),
            }
            .filter(|h| !h.is_empty()),
            // Nor is a changed file; the inspector reads its diff.
            HistoryView::Changes => None,
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
        if std::mem::take(&mut self.pending_yank) {
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
        self.pending_yank = true;
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
        self.yank_flash.clear();
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
                self.yank_flash
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
        match self.ci_body {
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

    /// A left click the detail pane's hit map resolved: move the cursor to
    /// the clicked row the way `j`/`k` would. The graph's cursor is a row, so
    /// any row of it is a stop; the other views move only onto an entry.
    pub fn click_detail(&mut self, click: &crate::ui::detail::DetailClick) {
        if self.history_view == HistoryView::Graph {
            self.graph_cursor = click.line;
            self.keep_graph_cursor_in_view();
            self.sync_graph_selection();
        } else if let Some(entry) = click.entry {
            self.set_detail_selected(entry);
            self.inspector_scroll = 0;
        }
        self.needs_redraw = true;
    }

    /// Scroll the inspector by `delta` lines, clamped to the content the last
    /// frame actually measured.
    /// On a run (`inspects_ci_run`) it scrolls the shown body's own offset.
    pub fn scroll_inspector(&mut self, delta: i32) {
        let max = self.inspector_max_scroll.get() as i32;
        if self.inspects_ci_run() {
            self.sync_ci_inspector(std::time::Instant::now());
            let from = i32::from(self.ci_body_scroll());
            self.ci_body_scrolls[self.ci_body.index()] = (from + delta).clamp(0, max) as u16;
            if self.ci_body == CiBody::Jobs {
                self.ci_job_follow.set(JobFollow::Free);
            }
            return;
        }
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
            // In the `notes` layout the pane after the tree is the reader,
            // as Tab goes.
            FocusedPanel::Detail if self.notes_layout() => FocusedPanel::Reader,
            FocusedPanel::Detail if self.inspector_is_expanded() => FocusedPanel::Inspector,
            // The graph docks no inspector, so there is no next pane to reach
            // into -- and it is the one view whose rows are finer than its
            // commits, so there is a gesture worth having here instead. The
            // key stays on this pane and walks its rows one at a time, where
            // plain `j`/`k` jumps whole commit blocks. Wrapping to the repo
            // list instead would spend the only line-by-line gesture the graph
            // has on moving to a different repo.
            FocusedPanel::Detail => FocusedPanel::Detail,
            // The reader keeps the keys: in the `notes` layout the shifted
            // keys scroll it from the tree and from the reader alike
            // (Andreas, 2026-10-29), as the graph keeps them on its rows.
            FocusedPanel::Reader => FocusedPanel::Reader,
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
            FocusedPanel::Reader => self.scroll_reader(delta),
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

    /// Cycle keyboard focus: repo list -> detail -> inspector -> strip ->
    /// repo list.
    /// Only meaningful in `Detail` view.
    pub fn toggle_panel_focus(&mut self) {
        if self.view != View::Detail {
            // Compact has one flat grid, but the strip is still a real target.
            self.focused_panel = match self.focused_panel {
                FocusedPanel::Contributions => FocusedPanel::RepoList,
                _ => FocusedPanel::Contributions,
            };
            return;
        }
        // The `notes` layout puts its reader after pane 2, and docks no
        // inspector after it (FZ-68).
        self.focused_panel = match self.focused_panel {
            FocusedPanel::RepoList => FocusedPanel::Detail,
            FocusedPanel::Detail if self.notes_layout() => FocusedPanel::Reader,
            FocusedPanel::Detail | FocusedPanel::Reader if self.shows_inspector() => {
                FocusedPanel::Inspector
            }
            FocusedPanel::Detail | FocusedPanel::Reader | FocusedPanel::Inspector => {
                FocusedPanel::Contributions
            }
            FocusedPanel::Contributions => FocusedPanel::RepoList,
        };
    }

    /// `toggle_panel_focus` in reverse (Shift-Tab): repo list -> strip ->
    /// inspector -> detail -> repo list, skipping a hidden inspector the same
    /// way.
    pub fn toggle_panel_focus_back(&mut self) {
        if self.view != View::Detail {
            // Two targets, so the reverse is the same toggle.
            self.toggle_panel_focus();
            return;
        }
        self.focused_panel = match self.focused_panel {
            FocusedPanel::RepoList => FocusedPanel::Contributions,
            FocusedPanel::Contributions if self.shows_inspector() => FocusedPanel::Inspector,
            FocusedPanel::Contributions | FocusedPanel::Inspector if self.notes_layout() => {
                FocusedPanel::Reader
            }
            FocusedPanel::Contributions | FocusedPanel::Inspector | FocusedPanel::Reader => {
                FocusedPanel::Detail
            }
            FocusedPanel::Detail => FocusedPanel::RepoList,
        };
    }

    /// Jump keyboard focus directly to `panel` (the `1`/`2` bindings).
    /// Only meaningful in `Detail` view.
    pub fn focus_panel(&mut self, panel: FocusedPanel) {
        // The strip is the one panel that exists in both views.
        if panel == FocusedPanel::Contributions {
            self.focused_panel = panel;
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
        // The reader exists only in the `notes` layout.
        if panel == FocusedPanel::Reader && !self.notes_layout() {
            return;
        }
        self.focused_panel = panel;
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
        let next = match self.contributions {
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
    fn open_contributions(&mut self, mode: ContributionsMode, today: chrono::NaiveDate) {
        self.contributions = mode;
        self.contributions_folded = false;
        self.contrib_cursor = Some(self.contrib_last_slot(today));
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
    /// Opening or unfolding the strip already stands on today. A cursor can
    /// still be unset on an `App` whose strip was never shown; there the
    /// first move seeds it on today rather than jumping from nowhere.
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
        self.list_stop = None;
    }

    pub fn search_push_char(&mut self, c: char) {
        self.search_query.push(c);
        self.selected = 0; // reset selection when query changes
        self.list_stop = None;
    }

    pub fn search_pop_char(&mut self) {
        self.search_query.pop();
        self.selected = 0;
        self.list_stop = None;
    }

    pub fn end_search(&mut self) {
        self.search_active = false;
    }

    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.search_active = false;
        self.selected = 0;
        self.list_stop = None;
    }

    pub fn set_status(&mut self, text: String, is_error: bool) {
        self.status_message = Some(StatusMessage {
            text,
            is_error,
            created: std::time::Instant::now(),
        });
    }

    /// A budget reading came with an answer that `applied` or was discarded.
    /// One on another token, from an applied answer, drops the old token's
    /// rate limit (transition 7). Call before the answer's outcome, which
    /// sets the not-before that follows it.
    pub fn observe_budget(
        &mut self,
        reading: Option<&crate::github::BudgetReading>,
        applied: bool,
        arrival: DateTime<Local>,
    ) {
        let Some(reading) = reading else {
            return;
        };
        if self
            .api_budget
            .observe(reading, applied, arrival, self.jitter.as_mut())
            == Observed::Switched
        {
            self.github_backoff.account_switched();
        }
    }

    /// An applied attempt failed at `now`: advance the retry ladder.
    pub fn github_attempt_failed(
        &mut self,
        failure: &crate::github::Failure,
        now: DateTime<Local>,
    ) {
        self.github_backoff
            .failed(failure, now, &self.api_budget, self.jitter.as_mut());
    }

    /// The budget's pace for polls of `poll_cost` points, judged at the last
    /// attempt, the moment polls are spaced from. Before any attempt there is
    /// nothing to space.
    ///
    /// The interval and cost are parameters rather than read off
    /// `github_poll_secs` and `github_poll_cost` so the poll scheduler can
    /// pass the `Config` it holds; the header passes the copies.
    pub fn github_pace(&self, poll_cost: u32) -> Pace {
        match self.github_freshness.last_attempt {
            Some(at) => self.api_budget.pace(poll_cost, at),
            None => Pace::Unknown,
        }
    }

    /// Whether the budget stretches polling past the configured interval of
    /// `poll_secs` or pauses it, which suspends the `Pending` floor and the
    /// push shortcut (A2).
    pub fn budget_slowed(&self, poll_secs: i64, poll_cost: u32) -> bool {
        match self.github_pace(poll_cost) {
            Pace::Unknown => false,
            Pace::Every(secs) => secs > poll_secs,
            Pace::Paused { .. } => true,
        }
    }

    /// Until when GitHub is known to refuse a request at `now`: a rate limit's
    /// not-before that has not passed, or too few points left for one poll
    /// (`docs/api-budget-design.md` 4.3, "Manual refresh"). `r` and `R` send
    /// nothing while this is `Some`.
    pub fn github_limit_until(
        &self,
        poll_secs: i64,
        poll_cost: u32,
        now: DateTime<Local>,
    ) -> Option<DateTime<Local>> {
        let limit = self
            .github_backoff
            .limit_until(poll_secs)
            .filter(|t| *t > now);
        let exhausted = self
            .api_budget
            .exhausted(poll_cost, now)
            .then(|| self.api_budget.resume_at())
            .flatten();
        limit.max(exhausted)
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
        // Every way the highlighted run can change passes here (a run step,
        // a click, a repo change), so the inspector's offsets cannot outlive
        // their run and come back when the cursor returns to it.
        self.sync_ci_inspector(std::time::Instant::now());
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
        // Likewise the reader, once pane 2 leaves `notes`.
        if self.focused_panel == FocusedPanel::Reader && !self.notes_layout() {
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
        let section = self.list_stop?;
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
    fn step_list(&mut self, delta: i32) -> bool {
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
        self.hover_pending = None;
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
                self.list_stop = Some(section);
                self.digest_selected = 0;
                self.inspector_scroll = 0;
                // On a header the `notes` layout gives way to the digest and
                // no reader is drawn; focus cannot stay on it (FZ-62).
                if self.focused_panel == FocusedPanel::Reader {
                    self.focused_panel = FocusedPanel::Detail;
                }
            }
            ListRow::Repo(pos) => {
                self.list_stop = None;
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
        let i = self.list_hits.get().at(col, row)?;
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
        self.hover_pending = match self.list_stop_at(col, row) {
            Some(ListRow::Header(section)) => match self.hover_pending {
                Some((held, since)) if held == section => Some((held, since)),
                _ => Some((section, now)),
            },
            _ => None,
        };
    }

    /// The pointer left the repo list: no hover is pending.
    pub fn clear_hover(&mut self) {
        self.hover_pending = None;
    }

    /// On the tick: land on the header the pointer has rested on for
    /// `HOVER_DWELL`.
    pub fn settle_hover(&mut self, now: std::time::Instant) {
        if let Some((section, since)) = self.hover_pending
            && now.saturating_duration_since(since) >= HOVER_DWELL
        {
            self.land_on(ListRow::Header(section));
            self.hover_pending = None;
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
        let (start, end) = self.work_hours;
        let (cutoff, label) = self.digest_window.bounds(now, start, end);
        let visible = self.visible_repos();
        let rows: Vec<Row> = visible
            .iter()
            .filter(|(_, r)| Section::of(r) == section)
            .map(|(index, repo)| {
                let pushed_at = self.pushed_at(repo);
                let log = needs_log(section, repo, pushed_at, cutoff).and_then(|key| {
                    let held = self.digest_logs.get(repo.path.as_deref()?)?;
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
        Some(build(section, self.digest_window, cutoff, label, &rows))
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
        let (start, end) = self.work_hours;
        let (cutoff, _) = self.digest_window.bounds(now, start, end);
        let wanted: Vec<(String, crate::digest::LogKey)> = self
            .visible_repos()
            .iter()
            .filter(|(_, r)| Section::of(r) == section)
            .filter_map(|(_, repo)| {
                let key = needs_log(section, repo, self.pushed_at(repo), cutoff)?;
                let path = repo.path.clone()?;
                let held = self.digest_logs.get(&path).is_some_and(|h| h.key == key);
                (!held).then_some((path, key))
            })
            .collect();
        wanted
            .into_iter()
            .map(|(path, key)| {
                self.digest_logs.insert(
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
        if let Some(held) = self.digest_logs.get_mut(path)
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
        digest.lines.into_iter().nth(self.digest_selected.min(last))
    }

    /// Move pane 2's digest cursor.
    pub fn digest_nav(&mut self, delta: i32) {
        let Some(digest) = self.digest(Local::now()) else {
            return;
        };
        let last = digest.lines.len().saturating_sub(1) as i32;
        let next = (self.digest_selected.min(last as usize) as i32 + delta).clamp(0, last) as usize;
        if next != self.digest_selected {
            self.digest_selected = next;
            self.inspector_scroll = 0;
        }
        self.needs_redraw = true;
    }

    /// Scroll the inspector's digest line detail. Not `scroll_inspector`,
    /// which in the `ci` view scrolls a run body the digest hides.
    pub fn scroll_digest_inspector(&mut self, delta: i32) {
        let max = self.inspector_max_scroll.get() as i32;
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
        self.digest_window = self.digest_window.next();
        self.digest_selected = 0;
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

#[cfg(test)]
pub(crate) mod tests;

/// FZ-45: the digest's `git log --since` reads are claimed once per touched
/// repo per cutoff and refs, and a read under a key the row has left is
/// dropped.
#[cfg(test)]
mod digest_log_tests;

/// The `changes` view's reads and their gates (FZ-60).
#[cfg(test)]
mod changes_tests;
