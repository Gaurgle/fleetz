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
}

impl HistoryView {
    /// Every view, in the order the tab strip draws them and `V` cycles them.
    pub const ALL: [Self; 8] = [
        Self::Info,
        Self::Commits,
        Self::Reflog,
        Self::Graph,
        Self::Worktrees,
        Self::Branches,
        Self::Ci,
        Self::Notes,
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
pub const DEFAULT_INSPECTOR_PCT: u16 = 30;

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
            CellSize::Medium,
            intro,
            Icons::Nerd,
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

    /// It is marked restart-only, and honestly so: the loading screen only
    /// covers the first load of a launch, so by the time this row can be
    /// reached it has already been and gone.
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

#[cfg(test)]
mod settings_icons_tests {
    use super::*;

    fn overlay(icons: Icons) -> SettingsOverlay {
        let mut so = SettingsOverlay::new(
            60,
            30,
            true,
            None,
            9,
            18,
            false,
            ContributionsMode::Off,
            CellSize::Medium,
            true,
            icons,
        );
        so.cursor = SETTING_FIELDS
            .iter()
            .position(|f| *f == SettingField::Icons)
            .expect("the icons row");
        so
    }

    fn saved_icons(so: &SettingsOverlay) -> crate::config::SettingValue {
        so.config_updates()
            .into_iter()
            .find(|(key, _)| *key == "icons")
            .map(|(_, value)| value)
            .expect("icons is not written to the file")
    }

    /// The row switches between the two sets both ways, says which is
    /// selected, applies without a restart and writes the key the loader
    /// reads. Fails if the row is inert, mislabelled or not saved.
    #[test]
    fn the_icons_row_toggles_and_saves_under_the_key_config_reads() {
        assert_eq!(SettingField::Icons.config_key(), "icons");
        assert_eq!(SettingField::Icons.applies(), Applies::Live);

        let mut so = overlay(Icons::Nerd);
        assert_eq!(so.value_label(SettingField::Icons), "nerd");
        assert_eq!(saved_icons(&so), crate::config::SettingValue::Str("nerd"));

        so.step(1);
        assert_eq!(so.icons, Icons::Plain, "the row did not toggle");
        assert_eq!(so.value_label(SettingField::Icons), "plain");
        assert_eq!(saved_icons(&so), crate::config::SettingValue::Str("plain"));

        so.step(-1);
        assert_eq!(so.icons, Icons::Nerd, "the row did not toggle back");
    }

    /// Typing a digit on the row changes nothing: it is not numeric.
    #[test]
    fn digits_leave_the_icons_row_alone() {
        let mut so = overlay(Icons::Plain);
        so.push_digit(1);
        assert_eq!(so.icons, Icons::Plain);
    }

    /// Both values survive a real save and load, read back the way launch
    /// reads them (`Icons::from_config`). Fails if the written string is one
    /// the loader does not recognise, which would fall back to Nerd.
    #[test]
    fn each_icon_set_survives_a_save_and_load() {
        let dir = std::env::temp_dir().join(format!("fleetz-icons-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("config.toml");

        for icons in [Icons::Plain, Icons::Nerd] {
            let so = overlay(icons);
            crate::config::save_settings(&path, &so.config_updates()).expect("save");
            let loaded = crate::config::Config::load_from(&path).expect("load");
            let read_back = loaded
                .icons
                .as_deref()
                .map(Icons::from_config)
                .unwrap_or_default();
            assert_eq!(read_back, icons, "the set did not survive the round trip");
        }

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

/// Where a load's GitHub dataset came from, which decides what it tells
/// `GithubFreshness`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatasetSource {
    /// No GitHub fetch behind it: a window apply, or `-d`.
    Local,
    /// Read off the disk cache, with no attempt made.
    Cache { written: Option<DateTime<Local>> },
    /// A successful attempt.
    Network,
    /// A failed attempt, with the cache filling in.
    Fallback {
        failure: crate::github::Failure,
        written: Option<DateTime<Local>>,
    },
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

/// One fetch's hold on its path: which claim it is, and when it last showed
/// life (claimed, granted its permit, or reported progress).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FetchClaim {
    token: u64,
    at: std::time::Instant,
}

#[derive(Debug, Default)]
struct ClaimTable {
    next_token: u64,
    claims: std::collections::HashMap<String, FetchClaim>,
}

/// The paths with a fetch running or queued, one fetch per checkout at a
/// time (FZ-34 R5, FZ-35a). Shared between `App` and the fetch tasks, so a
/// fetch can take its claim at the moment it may start.
///
/// The life of a claim:
///
/// - **Reserved when the sweep is scheduled** (`reserve`, through
///   `App::claim_fetch`), so the next selection already sweeps the path
///   without fetching it while the fetch waits for a permit.
/// - **Taken when the fetch permit is granted** (`start`), in the fetch task,
///   before git runs. This restamps the claim, so time spent queued behind
///   `FETCH_CONCURRENCY` other fetches never counts toward its expiry. A
///   reservation that expired while queued is taken again if the path is
///   free; if a newer claim holds it, this fetch does not run at all. The
///   load path (`assemble_view`) takes its claims here too, with no
///   reservation, since it schedules and starts in one place.
/// - **Kept alive by every progress update** (`touch`).
/// - **Ended by its own landing** (`release` with its token): a late landing
///   from a claim that expired cannot end a newer claim on the same path.
/// - **Expired** after `SWEEP_STUCK_AFTER` without a sign of life, the
///   safety net for a result that never lands (`expire`).
///
/// Each claim carries a token from a counter, never reused. Cloning shares
/// the table.
#[derive(Debug, Clone, Default)]
pub struct FetchClaims {
    table: std::sync::Arc<std::sync::Mutex<ClaimTable>>,
}

impl FetchClaims {
    fn table(&self) -> std::sync::MutexGuard<'_, ClaimTable> {
        // A panic while holding the lock leaves the table consistent: every
        // operation is a single map edit.
        self.table
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Reserve `path` at `now`, or `None` when another claim holds it.
    pub fn reserve(&self, path: &str, now: std::time::Instant) -> Option<u64> {
        let mut table = self.table();
        if table.claims.contains_key(path) {
            return None;
        }
        table.next_token += 1;
        let token = table.next_token;
        table
            .claims
            .insert(path.to_string(), FetchClaim { token, at: now });
        Some(token)
    }

    /// The fetch holding `token` was granted its permit at `now`: true when
    /// it may run. Its own claim is restamped; a free path is claimed again
    /// under the same token; a path another claim holds is refused.
    pub fn start(&self, path: &str, token: u64, now: std::time::Instant) -> bool {
        let mut table = self.table();
        match table.claims.get_mut(path) {
            Some(claim) if claim.token == token => claim.at = now,
            Some(_) => return false,
            None => {
                table
                    .claims
                    .insert(path.to_string(), FetchClaim { token, at: now });
            }
        }
        true
    }

    /// A progress update from the fetch holding `token`.
    pub fn touch(&self, path: &str, token: u64, now: std::time::Instant) {
        if let Some(claim) = self.table().claims.get_mut(path)
            && claim.token == token
        {
            claim.at = now;
        }
    }

    /// End the claim on `path`: only the one holding `token`, or whichever
    /// holds it when `token` is `None`.
    pub fn release(&self, path: &str, token: Option<u64>) {
        let mut table = self.table();
        if token.is_none_or(|t| table.claims.get(path).is_some_and(|c| c.token == t)) {
            table.claims.remove(path);
        }
    }

    /// The token of the claim on `path`.
    pub fn token(&self, path: &str) -> Option<u64> {
        self.table().claims.get(path).map(|c| c.token)
    }

    /// Drop claims with no sign of life for longer than `limit` at `now`.
    pub fn expire(&self, limit: std::time::Duration, now: std::time::Instant) {
        self.table()
            .claims
            .retain(|_, claim| now.saturating_duration_since(claim.at) <= limit);
    }

    pub fn contains_key(&self, path: &str) -> bool {
        self.table().claims.contains_key(path)
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.table().claims.is_empty()
    }

    #[cfg(test)]
    pub fn clear(&self) {
        self.table().claims.clear();
    }
}

/// What a batch of fetches someone asked for came to, once the last landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchReport {
    pub total: usize,
    /// Repo name and failure kind of each fetch that failed, in landing order.
    pub failed: Vec<(String, crate::git::FetchFailureKind)>,
}

/// The fetches a keypress is waiting on, so a mixed result is reported as
/// mixed (rule 9 of the fetch contract): `3 of 12 fetches failed`, not
/// silence. Only fetches someone asked for are tallied; a watch sweep's
/// failures show on their rows and nowhere else.
///
/// Pure transitions, like `GithubFreshness`: `expect` when fetches start,
/// `land` as each one finishes, which hands back the report with the last.
#[derive(Debug, Default)]
pub struct FetchTally {
    awaiting: std::collections::HashSet<String>,
    total: usize,
    failed: Vec<(String, crate::git::FetchFailureKind)>,
    /// The batch's last sign of life: when it started, or the latest
    /// progress of a fetch it awaits (`touch`). What `expire` measures from.
    started: Option<std::time::Instant>,
}

impl FetchTally {
    /// Fetches for these display paths have started. Joins a batch still in
    /// flight, so `r` and `F` pressed together report once.
    pub fn expect(&mut self, paths: impl IntoIterator<Item = String>) {
        if self.awaiting.is_empty() {
            *self = Self::default();
        }
        for path in paths {
            if self.awaiting.insert(path) {
                self.total += 1;
            }
        }
        if !self.awaiting.is_empty() && self.started.is_none() {
            self.started = Some(std::time::Instant::now());
        }
    }

    /// A repo's update landed. An update that did not fetch is not the one
    /// awaited. Returns the report when this was the last fetch awaited and
    /// at least one failed; every fetch succeeding says nothing extra.
    pub fn land(
        &mut self,
        path: &str,
        name: &str,
        fetch: &crate::git::FetchOutcome,
    ) -> Option<FetchReport> {
        use crate::git::FetchOutcome;
        if *fetch == FetchOutcome::NotAttempted || !self.awaiting.remove(path) {
            return None;
        }
        if let FetchOutcome::Failed(failure) = fetch {
            self.failed.push((name.to_string(), failure.kind));
        }
        if !self.awaiting.is_empty() {
            return None;
        }
        let done = std::mem::take(self);
        (!done.failed.is_empty()).then_some(FetchReport {
            total: done.total,
            failed: done.failed,
        })
    }

    /// A fetch for `path` made progress at `now`: if this batch awaits it,
    /// the batch is alive. An uncapped `f` on a big repo can run far longer
    /// than `SWEEP_STUCK_AFTER` and still owes its report (FZ-35b,
    /// criterion 7).
    pub fn touch(&mut self, path: &str, now: std::time::Instant) {
        if self.awaiting.contains(path) {
            self.started = Some(now);
        }
    }

    /// Stop waiting on fetches that have shown no sign of life for longer
    /// than `limit`. They report nothing; their rows say what happened if
    /// they ever land.
    pub fn expire(&mut self, limit: std::time::Duration) {
        if self.started.is_some_and(|t| t.elapsed() > limit) {
            *self = Self::default();
        }
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
    /// colored indicator over a dashboard you are reading.
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
}

/// One run's jobs as last read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CiJobs {
    pub etag: Option<String>,
    pub jobs: Vec<crate::ci::Job>,
}

impl CiState {
    /// What one poll of this repo costs on `core`: the list, plus the
    /// expanded run's jobs.
    pub fn poll_cost(&self) -> u32 {
        crate::ci::CALL_COST * if self.expanded.is_some() { 2 } else { 1 }
    }

    /// Land an applied answer's data. The outcome is `App::apply_ci`'s.
    fn land(&mut self, answer: crate::ci::CiAnswer) {
        use crate::ci::{Fetched, JobsAnswer};
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
/// (`GithubRepo::identity_key`, the shape of `main.rs::remote_key`).
pub type RemoteKey = (Option<String>, String);

/// The remote key of the GitHub repo `repo` carries, or None when it carries
/// none. Follows `main.rs::carries`: a Primary, Elsewhere or NotCloned row is
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

    /// The run under the `ci` view's cursor, clamped onto the last run as the
    /// view draws it; `None` while the selected repo holds no runs.
    pub fn highlighted_ci_run(&self) -> Option<&crate::ci::WorkflowRun> {
        let runs = &self.selected_ci()?.1?.runs;
        let at = self.detail_selections[HistoryView::Ci.index()];
        runs.get(at.min(runs.len().saturating_sub(1)))
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
    /// ticks and whose lamp pulses: about one repaint a second.
    pub fn ci_view_live(&self) -> bool {
        self.ci_wanted()
            && self
                .selected_ci()
                .and_then(|(_, state)| state)
                .is_some_and(|s| s.runs.iter().any(crate::ci::WorkflowRun::is_live))
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
        state.land(answer);
        // A shorter list for the repo on screen pulls the `ci` cursor onto
        // its last run rather than leaving it past the end.
        if self
            .selected_ci()
            .is_some_and(|(selected, _)| selected == key)
        {
            let runs = self.ci.get(&key).map_or(0, |s| s.runs.len());
            let slot = &mut self.detail_selections[HistoryView::Ci.index()];
            *slot = (*slot).min(runs.saturating_sub(1));
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
        let runs = &self.selected_ci()?.1?.runs;
        let last = runs.len().checked_sub(1)?;
        runs.get(self.detail_selections[HistoryView::Ci.index()].min(last))
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
    /// runs silently.
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
            // The runs held for the repo, as the view draws them.
            HistoryView::Ci => self
                .selected_ci()
                .and_then(|(_, state)| state)
                .map_or(0, |s| s.runs.len()),
            // The files the tree lists, in its current mode.
            HistoryView::Notes => self.notes_listed().len(),
            // The main checkout and the linked worktrees.
            HistoryView::Worktrees => self.worktrees_listed().len(),
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

    /// `yy` or `yp`: copy what `kind` names under the focused cursor through
    /// `copy` and say so on the status line, `nothing to copy` where there
    /// is nothing (`yank_target`).
    pub fn yank(&mut self, kind: YankKind, copy: impl FnOnce(&str) -> Result<(), String>) {
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
            Ok(()) => self.set_status(said, false),
            Err(reason) => self.set_status(format!("copy failed: {reason}"), true),
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
            .find(|e| crate::carries(repo, e))
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
                daily_key: None,
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
                remote_branches: vec![],
                work_key: None,
                fetch: crate::model::FetchFacts::default(),
                authors: Default::default(),
            })
            .collect();
        App::new(repos, "test".to_string(), 120, 40)
    }

    /// Regression: the `r` spinner keyed on the local sweep alone, so it
    /// stopped while the GitHub poll and the fetches it set off were still
    /// landing rows one at a time.
    #[test]
    fn a_manual_refresh_spins_until_its_poll_and_fetches_land() {
        let mut app = make_app(3);
        app.sweep_started = Some(std::time::Instant::now());
        app.sweep_manual = true;
        app.refresh_awaits_poll = true;
        assert!(app.is_refreshing());

        // The local sweep finishing is not the end of it.
        app.sweep_started = None;
        assert!(app.is_refreshing(), "stopped before the poll landed");

        app.settle_refresh_poll(["~/repos/repo-0".to_string(), "~/repos/repo-2".to_string()]);
        assert!(app.is_refreshing(), "stopped before the fetches landed");

        app.settle_refresh_fetch("~/repos/repo-0", &crate::git::FetchOutcome::Succeeded);
        assert!(app.is_refreshing());
        app.settle_refresh_fetch("~/repos/repo-2", &crate::git::FetchOutcome::Succeeded);
        assert!(!app.is_refreshing());
        assert!(app.refresh_fetches_started.is_none());
    }

    #[test]
    fn a_manual_refresh_is_local_until_its_sweep_ends_then_remote() {
        let mut app = make_app(1);
        assert_eq!(app.refresh_phase(), None);
        app.sweep_started = Some(std::time::Instant::now());
        app.sweep_manual = true;
        app.refresh_awaits_poll = true;
        assert_eq!(app.refresh_phase(), Some(RefreshPhase::Local));
        app.sweep_started = None;
        assert_eq!(app.refresh_phase(), Some(RefreshPhase::Remote));
        app.settle_refresh_poll(std::iter::empty());
        assert_eq!(app.refresh_phase(), None);
    }

    #[test]
    fn a_manual_refresh_ends_with_its_poll_when_nothing_moved() {
        let mut app = make_app(1);
        app.refresh_awaits_poll = true;
        app.settle_refresh_poll(std::iter::empty());
        assert!(!app.is_refreshing());
        assert!(app.refresh_fetches_started.is_none());
    }

    /// The local sweep can report a repo before that repo's fetch has run.
    #[test]
    fn an_unfetched_update_does_not_settle_a_refresh_fetch() {
        let mut app = make_app(1);
        app.refresh_awaits_poll = true;
        app.settle_refresh_poll(["~/repos/repo-0".to_string()]);
        app.settle_refresh_fetch("~/repos/repo-0", &crate::git::FetchOutcome::NotAttempted);
        assert!(app.is_refreshing());
    }

    fn fetch_failed(kind: crate::git::FetchFailureKind) -> crate::git::FetchOutcome {
        crate::git::FetchOutcome::Failed(crate::git::FetchFailure { kind, detail: None })
    }

    /// Rule 4: a fetch that failed is still a fetch that finished, so it
    /// settles the refresh waiting on it.
    #[test]
    fn a_failed_fetch_settles_a_refresh_fetch() {
        let mut app = make_app(1);
        app.refresh_awaits_poll = true;
        app.settle_refresh_poll(["~/repos/repo-0".to_string()]);
        app.settle_refresh_fetch(
            "~/repos/repo-0",
            &fetch_failed(crate::git::FetchFailureKind::Unreachable),
        );
        assert!(!app.is_refreshing());
        assert!(app.refresh_fetches_started.is_none());
    }

    /// The fetches `r`'s poll sets off are the ones its report counts.
    #[test]
    fn a_manual_refresh_tallies_the_fetches_its_poll_set_off() {
        let mut app = make_app(2);
        app.refresh_awaits_poll = true;
        app.settle_refresh_poll(["~/repos/repo-0".to_string(), "~/repos/repo-1".to_string()]);
        let ok = crate::git::FetchOutcome::Succeeded;
        assert_eq!(
            app.manual_fetches.land("~/repos/repo-0", "repo-0", &ok),
            None
        );
        let report = app.manual_fetches.land(
            "~/repos/repo-1",
            "repo-1",
            &fetch_failed(crate::git::FetchFailureKind::Auth),
        );
        assert_eq!(
            report,
            Some(FetchReport {
                total: 2,
                failed: vec![("repo-1".to_string(), crate::git::FetchFailureKind::Auth)],
            })
        );
    }

    /// A watch timer's poll is not a keypress: its fetches report nothing.
    #[test]
    fn a_poll_nobody_pressed_r_for_tallies_nothing() {
        let mut app = make_app(1);
        app.settle_refresh_poll(["~/repos/repo-0".to_string()]);
        let report = app.manual_fetches.land(
            "~/repos/repo-0",
            "repo-0",
            &fetch_failed(crate::git::FetchFailureKind::Auth),
        );
        assert_eq!(report, None);
    }

    /// An update that did not fetch is not the fetch being waited on.
    #[test]
    fn a_tally_is_not_settled_by_an_update_that_did_not_fetch() {
        let mut tally = FetchTally::default();
        tally.expect(["a".to_string()]);
        let not_fetched = crate::git::FetchOutcome::NotAttempted;
        assert_eq!(tally.land("a", "a", &not_fetched), None);
        let report = tally.land("a", "a", &fetch_failed(crate::git::FetchFailureKind::Other));
        assert_eq!(report.map(|r| r.total), Some(1));
    }

    /// `r` and `F` pressed together report once, over both batches, and a
    /// path in both counts once.
    #[test]
    fn a_tally_joins_a_batch_still_in_flight() {
        let mut tally = FetchTally::default();
        tally.expect(["a".to_string(), "b".to_string()]);
        tally.expect(["b".to_string(), "c".to_string()]);
        let failed = fetch_failed(crate::git::FetchFailureKind::Auth);
        assert_eq!(tally.land("a", "a", &failed), None);
        assert_eq!(
            tally.land("b", "b", &crate::git::FetchOutcome::Succeeded),
            None
        );
        let report = tally.land("c", "c", &crate::git::FetchOutcome::Succeeded);
        assert_eq!(report.map(|r| (r.total, r.failed.len())), Some((3, 1)));
    }

    /// A fetch that never lands cannot hold a report open for the session.
    #[test]
    fn a_tally_that_outlives_its_limit_is_dropped() {
        let mut tally = FetchTally::default();
        tally.expect(["a".to_string()]);
        tally.expire(std::time::Duration::from_secs(60));
        assert!(tally.started.is_some(), "expired inside its limit");
        tally.started = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(120));
        tally.expire(std::time::Duration::from_secs(60));
        let late = tally.land("a", "a", &fetch_failed(crate::git::FetchFailureKind::Auth));
        assert_eq!(late, None);
    }

    /// The watch timer's poll fetches silently: no spinner, nothing to wait on.
    #[test]
    fn a_poll_nobody_pressed_r_for_adds_no_refresh_fetches() {
        let mut app = make_app(1);
        app.settle_refresh_poll(["~/repos/repo-0".to_string()]);
        assert!(!app.is_refreshing());
        assert!(app.refresh_fetches.is_empty());
    }

    #[test]
    fn a_watch_timer_sweep_is_not_a_refresh() {
        let mut app = make_app(1);
        app.sweep_started = Some(std::time::Instant::now());
        app.sweep_manual = false;
        assert!(!app.is_refreshing());
    }

    #[test]
    fn a_refresh_fetch_that_never_returns_is_given_up_on() {
        let mut app = make_app(1);
        app.refresh_awaits_poll = true;
        app.settle_refresh_poll(["~/repos/repo-0".to_string()]);
        app.expire_refresh_fetches(std::time::Duration::from_secs(60));
        assert!(
            app.is_refreshing(),
            "expired a fetch still inside its limit"
        );
        app.refresh_fetches_started =
            Some(std::time::Instant::now() - std::time::Duration::from_secs(61));
        app.expire_refresh_fetches(std::time::Duration::from_secs(60));
        assert!(!app.is_refreshing());
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
        assert_eq!(app.history_view, HistoryView::Notes, "wraps backward");
        app.cycle_history_view(true);
        assert_eq!(app.history_view, HistoryView::Ci);
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

    /// A commit by `author`, for the FZ-56 search fixtures.
    fn commit_by(hash: &str, author: &str) -> crate::model::Commit {
        let mut c = commit(hash);
        c.author = author.to_string();
        c
    }

    /// FZ-56's four rows: one name prefix, two owners, one organisation and
    /// one local-only clone with an author nobody else has.
    fn people_app() -> App {
        use crate::model::RepoKind;
        let mut app = make_app(4);
        let r = &mut app.repos[0];
        r.name = "tools".to_string();
        r.owner = Some("Gaurgle".to_string());
        r.repo_kind = RepoKind::Public;
        r.commits = vec![
            commit_by("a1", "Andreas Roos"),
            commit_by("a2", "Andreas Roos"),
        ];
        let r = &mut app.repos[1];
        r.name = "tools-kestrel".to_string();
        r.owner = Some("Gaurgle".to_string());
        r.repo_kind = RepoKind::Private;
        r.location = RepoLocation::NotCloned;
        r.path = None;
        let r = &mut app.repos[2];
        r.name = "tools-web".to_string();
        r.owner = Some("kestrel-labs".to_string());
        r.repo_kind = RepoKind::Org;
        r.commits = vec![commit_by("b1", "Pia Holm")];
        let r = &mut app.repos[3];
        r.name = "tools-scratch".to_string();
        r.location = RepoLocation::LocalOnly;
        r.commits = vec![commit_by("c1", "Mira Lindqvist")];
        app
    }

    fn names_for(app: &mut App, query: &str) -> Vec<String> {
        app.search_query = query.to_string();
        app.visible_repos()
            .iter()
            .map(|(_, r)| r.name.clone())
            .collect()
    }

    #[test]
    fn an_owner_login_finds_the_owners_rows_and_not_the_organisations() {
        let mut app = people_app();
        let mut found = names_for(&mut app, "Gaurgle");
        found.sort();
        assert_eq!(found, vec!["tools", "tools-kestrel"]);
    }

    #[test]
    fn an_organisation_name_finds_its_row() {
        let mut app = people_app();
        assert_eq!(names_for(&mut app, "kestrel-labs"), vec!["tools-web"]);
    }

    #[test]
    fn a_contributor_finds_the_rows_whose_commits_carry_them() {
        let mut app = people_app();
        assert_eq!(names_for(&mut app, "Andreas Roos"), vec!["tools"]);
        assert_eq!(names_for(&mut app, "Roos"), vec!["tools"]);
        // Word by word as well as the phrase: the order typed does not matter.
        assert_eq!(names_for(&mut app, "roos andreas"), vec!["tools"]);
    }

    #[test]
    fn a_local_only_row_matches_on_its_authors() {
        let mut app = people_app();
        assert_eq!(names_for(&mut app, "Lindqvist"), vec!["tools-scratch"]);
    }

    #[test]
    fn a_not_cloned_row_matches_on_its_owner_and_has_no_authors() {
        let mut app = people_app();
        app.repos.retain(|r| r.is_not_cloned());
        assert_eq!(names_for(&mut app, "gaurgle"), vec!["tools-kestrel"]);
        assert!(names_for(&mut app, "Roos").is_empty());
    }

    /// The tiers outrank the score: the organisation's login is a better
    /// fuzzy hit for "kestrel" than `tools-kestrel` is, and still ranks
    /// below it, since a name match is what a search usually means.
    #[test]
    fn a_name_match_ranks_above_an_owner_match() {
        let mut app = people_app();
        assert!(
            crate::fuzzy::score("kestrel", "kestrel-labs")
                > crate::fuzzy::score("kestrel", "tools-kestrel")
        );
        assert_eq!(
            names_for(&mut app, "kestrel"),
            vec!["tools-kestrel", "tools-web"]
        );
    }

    /// All three tiers at once: name, then organisation, then contributor.
    #[test]
    fn an_owner_match_ranks_above_a_contributor_match() {
        let mut app = people_app();
        app.repos[3].commits = vec![commit_by("c2", "Kestrel Hansen")];
        assert_eq!(
            names_for(&mut app, "kestrel"),
            vec!["tools-kestrel", "tools-web", "tools-scratch"]
        );
    }

    /// Owners and authors match on word starts, not as loose subsequences:
    /// a short name query would otherwise pull in every row a prolific
    /// author touched ("dr" is in "Andreas"), burying the names it meant.
    #[test]
    fn an_owner_or_author_matches_on_word_starts_only() {
        let mut app = people_app();
        assert!(names_for(&mut app, "dr").is_empty());
        assert!(names_for(&mut app, "urgle").is_empty());
        assert_eq!(names_for(&mut app, "and ro"), vec!["tools"]);
    }

    /// A text of separators alone splits into no words, and zero words
    /// must not match: "-" or "." against an owner or author matched every
    /// row. A name that holds the separator still matches it as a name.
    #[test]
    fn a_separator_only_query_matches_no_owner_or_author() {
        for text in ["-", ".", "/", "_", "- .", " "] {
            assert_eq!(person_score(text, "Gaurgle"), None, "{text:?}");
            assert_eq!(person_score(text, "Andreas Roos"), None, "{text:?}");
        }
        let mut app = people_app();
        assert!(names_for(&mut app, ".").is_empty());
        let mut found = names_for(&mut app, "-");
        found.sort();
        assert_eq!(found, vec!["tools-kestrel", "tools-scratch", "tools-web"]);
    }

    #[test]
    fn a_pull_request_author_counts_as_a_contributor() {
        let mut app = people_app();
        app.repos[1].prs = vec![crate::model::PullRequest {
            number: 7,
            title: "fix".to_string(),
            author: "octo-reviewer".to_string(),
            created_at: chrono::Utc::now(),
            review_status: crate::model::ReviewStatus::Unknown,
            url: String::new(),
        }];
        assert_eq!(names_for(&mut app, "octo"), vec!["tools-kestrel"]);
    }

    #[test]
    fn the_matched_field_is_reported_only_for_a_non_name_hit() {
        let mut app = people_app();
        let field = |app: &App, i: usize| app.search_match_field(&app.repos[i]);
        assert_eq!(field(&app, 0), None, "nothing typed");
        app.search_query = "kestrel".to_string();
        assert_eq!(field(&app, 1), None, "a name hit needs no mark");
        assert_eq!(field(&app, 2), Some(MatchField::Org));
        app.search_query = "gaurgle".to_string();
        assert_eq!(field(&app, 0), Some(MatchField::Owner));
        app.search_query = "roos".to_string();
        assert_eq!(field(&app, 0), Some(MatchField::Author));
        assert_eq!(field(&app, 3), None, "not a match at all");
    }

    /// The author set is cached on the row, so it has to follow a sweep
    /// that lands a new commit list rather than keep the old authors.
    #[test]
    fn a_sweep_landing_new_commits_refreshes_who_the_row_matches() {
        let mut app = people_app();
        assert_eq!(names_for(&mut app, "Lindqvist"), vec!["tools-scratch"]);
        let mut fresh = app.repos[3].clone();
        fresh.commits = vec![commit_by("c9", "Ola Berg")];
        app.repos[3].apply_local_sweep(fresh);
        assert!(names_for(&mut app, "Lindqvist").is_empty());
        assert_eq!(names_for(&mut app, "Berg"), vec!["tools-scratch"]);
    }

    /// FZ-56's performance criterion: a 200-row fleet, every row with a
    /// 200-commit history, filters in well under the input-to-paint budget
    /// in `docs/performance-baseline.md` (median about 4 to 6 ms for a whole
    /// keypress frame). The bound here is that budget's p90, 9 ms, for the
    /// filter alone in a debug build, a deliberately loose ceiling: the
    /// release measurement is in the FZ-56 report. The first query builds
    /// the author sets; the timed ones reuse them.
    #[test]
    fn a_two_hundred_row_fleet_filters_within_the_input_budget() {
        let mut app = make_app(200);
        for (i, r) in app.repos.iter_mut().enumerate() {
            r.owner = Some(format!("owner-{}", i % 7));
            r.commits = (0..200)
                .map(|n| {
                    commit_by(
                        &format!("{i}-{n}"),
                        ["Andreas Roos", "Pia Holm", "Ola Berg"][n % 3],
                    )
                })
                .collect();
        }
        names_for(&mut app, "warm");
        let first = app.repos[0].commit_authors().as_ptr();
        let started = std::time::Instant::now();
        for query in ["repo-1", "owner-3", "holm", "nothing at all"] {
            names_for(&mut app, query);
        }
        let per_query = started.elapsed() / 4;
        assert_eq!(
            app.repos[0].commit_authors().as_ptr(),
            first,
            "the author set is built once, not per keystroke"
        );
        assert!(
            per_query < std::time::Duration::from_millis(9),
            "filtering took {per_query:?} per query"
        );
    }

    /// Tag filters still narrow an owner search, as they narrow a name one.
    #[test]
    fn tag_filters_combine_with_an_owner_query() {
        let mut app = people_app();
        app.repos[0].dirty = 1;
        assert_eq!(names_for(&mut app, "Gaurgle #dirty"), vec!["tools"]);
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
            CellSize::Medium,
            true,
            Icons::Nerd,
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

    /// The list width `split_main` draws for `app` in a main area `w` wide.
    fn drawn_list_width(app: &App, w: u16) -> u16 {
        let area = ratatui::layout::Rect::new(0, 0, w, 40);
        crate::ui::detail::split_main(area, app.repo_list_pct)
            .0
            .width
    }

    /// FZ-69: at 180 columns the default draws 52 (the cap), and stepping
    /// the percentage blind gave `>` 72 but `<` 54, wider than the 52 on
    /// screen. Every step now starts from the drawn width.
    #[test]
    fn resize_keys_step_from_the_capped_width_on_screen() {
        let w = 180;
        let at_default = || {
            let app = make_app(1);
            app.main_area_width.set(w);
            app
        };

        let mut app = at_default();
        let default = drawn_list_width(&app, w);
        assert!(default < w * DEFAULT_REPO_LIST_PCT / 100, "the cap bites");
        app.resize_repo_list(-1);
        let after_lt = drawn_list_width(&app, w);
        assert!(after_lt < default, "< widened: {default} -> {after_lt}");
        assert_eq!(after_lt, 43, "< steps 5% of 180 from 52");

        let mut app = at_default();
        let mut widths = vec![default];
        for _ in 0..3 {
            app.resize_repo_list(1);
            widths.push(drawn_list_width(&app, w));
        }
        assert_eq!(widths[1], 61, "> steps 5% of 180 from 52");
        assert!(
            widths.windows(2).all(|p| p[0] < p[1]),
            "> did not widen every press: {widths:?}"
        );
    }

    /// At every width a key press moves the list the way it says or, at a
    /// bound, not at all: never back onto the capped default (which would
    /// stall `>`), never wider under `<` where the floor exceeds the cap.
    #[test]
    fn resize_keys_never_move_the_list_the_wrong_way_at_any_width() {
        for w in 60..=400 {
            for delta in [-1i16, 1] {
                let mut app = make_app(1);
                app.main_area_width.set(w);
                for press in 0..10 {
                    let before = drawn_list_width(&app, w);
                    let pct = app.repo_list_pct;
                    app.resize_repo_list(delta);
                    let after = drawn_list_width(&app, w);
                    if delta < 0 {
                        assert!(
                            after <= before,
                            "w {w} press {press}: < {before} -> {after}"
                        );
                    } else if pct < REPO_LIST_PCT_MAX {
                        assert!(after > before, "w {w} press {press}: > {before} -> {after}");
                    }
                }
            }
        }
    }

    /// Where the cap does not bite the step is the plain 5% it always was.
    #[test]
    fn resize_keys_step_five_percent_where_the_cap_does_not_bite() {
        let w = 100;
        let mut app = make_app(1);
        app.main_area_width.set(w);
        assert_eq!(drawn_list_width(&app, w), 35);
        app.resize_repo_list(-1);
        assert_eq!(app.repo_list_pct, DEFAULT_REPO_LIST_PCT - 5);
        assert_eq!(drawn_list_width(&app, w), 30);
        app.reset_pane_sizes();
        app.resize_repo_list(1);
        assert_eq!(app.repo_list_pct, DEFAULT_REPO_LIST_PCT + 5);
        assert_eq!(drawn_list_width(&app, w), 40);
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
    fn c_switches_the_source_while_the_strip_is_up() {
        let mut app = make_app(1);
        assert_eq!(
            app.contributions,
            ContributionsMode::Github,
            "defaults to github"
        );
        app.cycle_contributions_source();
        assert_eq!(app.contributions, ContributionsMode::Local);
        app.cycle_contributions_source();
        assert_eq!(app.contributions, ContributionsMode::Github);
    }

    #[test]
    fn c_does_nothing_while_the_strip_is_folded() {
        let mut app = make_app(1);
        app.contributions_folded = true;
        app.cycle_contributions_source();
        assert_eq!(app.contributions, ContributionsMode::Github);
        assert!(app.contributions_folded, "c unfolded the strip");
    }

    /// The setting's "off" predates folding and meant "not on screen". The
    /// strip is always on screen now, so it folds, and keeps its source.
    #[test]
    fn the_off_setting_folds_the_strip_and_keeps_its_source() {
        let mut app = make_app(1);
        app.apply_contributions_setting(ContributionsMode::Local);
        assert!(!app.contributions_folded);
        app.apply_contributions_setting(ContributionsMode::Off);
        assert!(app.contributions_folded);
        assert_eq!(app.contributions, ContributionsMode::Local);
        app.apply_contributions_setting(ContributionsMode::Github);
        assert!(!app.contributions_folded, "a source setting unfolds it");
        assert_eq!(app.contributions, ContributionsMode::Github);
    }

    #[test]
    fn the_strip_is_always_focusable() {
        let mut app = make_app(1);
        app.contributions_folded = true;
        app.focus_panel(FocusedPanel::Contributions);
        assert_eq!(app.focused_panel, FocusedPanel::Contributions);
    }

    /// Unlike the other three panels, this one exists in Compact view, so the
    /// Detail-only guard on `focus_panel` must not apply to it.
    #[test]
    fn the_pane_is_focusable_in_compact_view() {
        let mut app = make_app(1);
        app.view = View::Compact;
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
    fn tab_walks_all_four_panes() {
        let mut app = make_app(1);
        app.view = View::Detail;
        for next in [
            FocusedPanel::Detail,
            FocusedPanel::Inspector,
            FocusedPanel::Contributions,
            FocusedPanel::RepoList,
        ] {
            app.toggle_panel_focus();
            assert_eq!(app.focused_panel, next);
        }
        app.history_view = HistoryView::Graph;
        app.focused_panel = FocusedPanel::Detail;
        app.toggle_panel_focus();
        assert_eq!(
            app.focused_panel,
            FocusedPanel::Contributions,
            "no inspector"
        );
    }

    #[test]
    fn tab_in_compact_toggles_between_the_list_and_the_pane() {
        let mut app = make_app(1);
        app.view = View::Compact;
        app.toggle_panel_focus();
        assert_eq!(app.focused_panel, FocusedPanel::Contributions);
        app.toggle_panel_focus();
        assert_eq!(app.focused_panel, FocusedPanel::RepoList);
    }

    /// Shift-Tab used to call the forward cycle, so it walked the same way
    /// as Tab and "back one pane" took three presses.
    #[test]
    fn shift_tab_walks_all_four_panes_in_reverse() {
        let mut app = make_app(1);
        app.view = View::Detail;
        for next in [
            FocusedPanel::Contributions,
            FocusedPanel::Inspector,
            FocusedPanel::Detail,
            FocusedPanel::RepoList,
        ] {
            app.toggle_panel_focus_back();
            assert_eq!(app.focused_panel, next);
        }
        for start in [
            FocusedPanel::RepoList,
            FocusedPanel::Detail,
            FocusedPanel::Inspector,
            FocusedPanel::Contributions,
        ] {
            app.focused_panel = start;
            app.toggle_panel_focus();
            app.toggle_panel_focus_back();
            assert_eq!(app.focused_panel, start, "Shift-Tab did not undo Tab");
        }
    }

    #[test]
    fn shift_tab_in_compact_toggles_between_the_list_and_the_pane() {
        let mut app = make_app(1);
        app.view = View::Compact;
        app.toggle_panel_focus_back();
        assert_eq!(app.focused_panel, FocusedPanel::Contributions);
        app.toggle_panel_focus_back();
        assert_eq!(app.focused_panel, FocusedPanel::RepoList);
    }

    /// #78: the tick advances the GitHub calendar to the local date. A
    /// weekday leaves the cursor on its date, and a Sunday shifts the grid a
    /// week, so the cursor moves seven slots back to stay on the same day.
    #[test]
    fn observe_day_keeps_the_cursor_on_its_date() {
        use crate::contributions::{Calendar, Source};
        let d = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();

        let mut app = make_app(1);
        // Scrolled back from the newest edge, so the scroll has to follow.
        app.contrib_visible_weeks.set(20);
        let tuesday = d("2026-08-18");
        app.github_calendar = Some(Box::new(Calendar::new(tuesday, Source::Github)));
        let old_today = app.github_calendar.as_ref().unwrap().today_slot();
        app.contrib_cursor = Some(old_today);
        app.contrib_scroll.set(40);
        app.observe_day(tuesday + chrono::Duration::days(1));
        assert_eq!(app.contrib_cursor, Some(old_today), "weekday: unchanged");
        assert_eq!(app.contrib_scroll.get(), 40, "weekday: same weeks shown");
        assert_eq!(
            app.github_calendar.as_ref().unwrap().today_slot(),
            old_today + 1
        );

        let saturday = d("2026-08-22");
        app.github_calendar = Some(Box::new(Calendar::new(saturday, Source::Github)));
        let old_today = app.github_calendar.as_ref().unwrap().today_slot();
        app.contrib_cursor = Some(old_today);
        app.observe_day(saturday + chrono::Duration::days(1));
        assert_eq!(
            app.contrib_cursor,
            Some(old_today - 7),
            "sunday: a week back"
        );
        assert_eq!(app.contrib_scroll.get(), 39, "sunday: same weeks shown");

        app.contrib_cursor = Some(3);
        app.contrib_scroll.set(0);
        app.observe_day(saturday + chrono::Duration::days(8));
        assert_eq!(app.contrib_cursor, Some(0), "clamped at the first slot");
        assert_eq!(app.contrib_scroll.get(), 0);
    }

    #[test]
    fn observe_day_without_a_github_calendar_changes_nothing() {
        let mut app = make_app(1);
        app.contrib_cursor = Some(12);
        app.contrib_scroll.set(40);
        app.observe_day(today() + chrono::Duration::days(1));
        assert!(app.github_calendar.is_none());
        assert_eq!(app.contrib_cursor, Some(12));
        assert_eq!(app.contrib_scroll.get(), 40);
    }

    /// A strip showing the newest weeks keeps showing them across a Sunday:
    /// shifting its scroll back with the grid would put the new day's column
    /// just off the right edge.
    #[test]
    fn observe_day_keeps_a_strip_pinned_to_the_newest_week() {
        use crate::contributions::{Calendar, Source, WEEKS};
        let saturday = chrono::NaiveDate::from_ymd_opt(2026, 8, 22).unwrap();
        let mut app = make_app(1);
        app.github_calendar = Some(Box::new(Calendar::new(saturday, Source::Github)));
        app.contrib_visible_weeks.set(20);
        app.contrib_scroll.set(WEEKS - 20);
        app.observe_day(saturday + chrono::Duration::days(1));
        assert_eq!(app.contrib_scroll.get(), WEEKS - 20);
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

    /// Opening the strip stands on today (#66). A strip that was never opened
    /// (launched with the setting off) has no cursor, and focusing it does not
    /// invent one: its first move marks today. A source change reopens the
    /// strip, so it returns to today.
    #[test]
    fn an_unopened_strip_marks_today_on_the_first_move_and_a_source_change_returns_to_it() {
        let mut app = make_app(1);
        assert_eq!(app.contrib_cursor, None, "starting marks nothing");
        app.focus_panel(FocusedPanel::Contributions);
        assert_eq!(app.contrib_cursor, None, "focusing marks nothing");

        // The first keypress lands on today rather than jumping from nowhere.
        app.move_contrib_cursor(-1, 0, today());
        assert_eq!(app.contrib_cursor, Some(app.contrib_last_slot(today())));

        // And it goes back to today when the source changes.
        app.cycle_contributions_source();
        let now = chrono::Local::now().date_naive();
        assert_eq!(app.contrib_cursor, Some(app.contrib_last_slot(now)));
    }

    /// Opening the strip stands on today, so the first look answers "what
    /// did I do today" without a throwaway keypress (#66). A cursor left
    /// somewhere else, and a grid scrolled away, both come back to today.
    #[test]
    fn opening_the_strip_stands_on_today() {
        let mut app = make_app(1);
        app.contrib_cursor = Some(0);
        app.contrib_scroll.set(0);
        app.open_contributions(ContributionsMode::Github, today());
        assert_eq!(app.contrib_cursor, Some(app.contrib_last_slot(today())));
        assert!(
            app.contrib_scroll.get() >= crate::contributions::WEEKS,
            "still asks for the newest weeks, which holds today's column"
        );
    }

    /// The first arrow after opening is a real step from today, not the
    /// marking-only keypress it used to be.
    #[test]
    fn the_first_arrow_after_opening_moves_one_step_from_today() {
        let mut app = make_app(1);
        app.focus_panel(FocusedPanel::Contributions);
        let last = app.contrib_last_slot(today());

        app.open_contributions(ContributionsMode::Github, today());
        app.move_contrib_cursor(-1, 0, today());
        assert_eq!(app.contrib_cursor, Some(last - 7), "one week back");

        app.open_contributions(ContributionsMode::Github, today());
        app.move_contrib_cursor(0, -1, today());
        assert_eq!(app.contrib_cursor, Some(last - 1), "one weekday up");
    }

    /// A source change reopens the strip, so it lands on today as well, even
    /// after the user travelled away.
    #[test]
    fn a_source_change_leaves_the_cursor_on_today() {
        let mut app = make_app(1);
        app.contrib_cursor = Some(0);
        app.cycle_contributions_source();
        assert_eq!(app.contributions, ContributionsMode::Local);
        let today = chrono::Local::now().date_naive();
        assert_eq!(app.contrib_cursor, Some(app.contrib_last_slot(today)));

        app.contrib_cursor = Some(0);
        app.cycle_contributions_source();
        assert_eq!(app.contributions, ContributionsMode::Github);
        assert_eq!(app.contrib_cursor, Some(app.contrib_last_slot(today)));
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

    /// Switching the source pins the newest weeks to the right edge: the grid
    /// should show now, not last August. The request is deliberately
    /// past the end, because only the renderer knows how many weeks fit; it
    /// clamps this down on the next frame.
    #[test]
    fn switching_the_source_asks_for_the_newest_weeks() {
        let mut app = make_app(1);
        app.contrib_scroll.set(0);
        app.cycle_contributions_source();
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

    /// The row cycles the three sizes both ways, starts from what the app
    /// holds, and writes the key `Config` reads back. A row whose key did not
    /// match would save happily and change nothing.
    #[test]
    fn the_cell_size_row_cycles_and_writes_its_key() {
        let mut overlay = SettingsOverlay::new(
            60,
            60,
            true,
            None,
            9,
            18,
            false,
            ContributionsMode::Off,
            CellSize::Medium,
            true,
            Icons::Nerd,
        );
        overlay.cursor = SETTING_FIELDS
            .iter()
            .position(|f| *f == SettingField::ContribCellSize)
            .unwrap();
        assert_eq!(SettingField::ContribCellSize.applies(), Applies::Live);
        assert_eq!(overlay.value_label(SettingField::ContribCellSize), "medium");
        overlay.step(1);
        assert_eq!(overlay.value_label(SettingField::ContribCellSize), "large");
        overlay.step(1);
        assert_eq!(overlay.value_label(SettingField::ContribCellSize), "small");
        overlay.step(-1);
        assert_eq!(overlay.value_label(SettingField::ContribCellSize), "large");

        let updates = overlay.config_updates();
        let written = updates
            .iter()
            .find(|(key, _)| *key == "contributions_cell_size")
            .map(|(_, v)| v.clone());
        assert_eq!(written, Some(crate::config::SettingValue::Str("large")));
        let cfg: crate::config::Config =
            toml::from_str(r#"contributions_cell_size = "large""#).unwrap();
        assert_eq!(
            CellSize::from_config(cfg.contributions_cell_size.as_deref().unwrap()),
            CellSize::Large
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
            CellSize::Medium,
            true,
            Icons::Nerd,
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
            CellSize::Medium,
            true,
            Icons::Nerd,
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
        app.open_contributions(ContributionsMode::Github, today());
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

    /// The reverse walk has the same trap from the other side: back from the
    /// strip is the inspector, unless the graph view hides it.
    #[test]
    fn reverse_focus_cycling_skips_a_hidden_inspector() {
        let mut app = make_app(1);
        app.history_view = HistoryView::Graph;
        app.view = View::Detail;
        app.focused_panel = FocusedPanel::Contributions;
        app.toggle_panel_focus_back();
        assert_eq!(
            app.focused_panel,
            FocusedPanel::Detail,
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

    fn fold_app() -> App {
        let mut app = App::new(Vec::new(), String::new(), 120, 44);
        app.view = View::Detail;
        app.contributions = ContributionsMode::Github;
        app
    }

    fn is_folded(app: &App, panel: FocusedPanel) -> bool {
        match panel {
            FocusedPanel::Inspector => app.inspector_folded,
            _ => app.contributions_folded,
        }
    }

    const FOLDABLE: [FocusedPanel; 2] = [FocusedPanel::Inspector, FocusedPanel::Contributions];

    #[test]
    fn a_pane_number_on_an_unfocused_pane_focuses_it_without_folding() {
        for panel in FOLDABLE {
            let mut app = fold_app();
            app.press_panel_key(panel);
            assert_eq!(app.focused_panel, panel);
            assert!(!is_folded(&app, panel), "{panel:?} folded on first press");
        }
    }

    #[test]
    fn the_focused_panes_number_folds_it_and_keeps_focus() {
        for panel in FOLDABLE {
            let mut app = fold_app();
            app.press_panel_key(panel);
            app.press_panel_key(panel);
            assert!(is_folded(&app, panel), "{panel:?}");
            assert_eq!(app.focused_panel, panel, "focus left the folded pane");
            app.press_panel_key(panel);
            assert!(!is_folded(&app, panel), "{panel:?} did not unfold");
            assert_eq!(app.focused_panel, panel);
        }
    }

    #[test]
    fn a_folded_panes_number_from_elsewhere_opens_and_focuses_it() {
        for panel in FOLDABLE {
            let mut app = fold_app();
            app.toggle_fold(panel);
            app.focus_panel(FocusedPanel::RepoList);
            app.press_panel_key(panel);
            assert!(!is_folded(&app, panel), "{panel:?}");
            assert_eq!(app.focused_panel, panel);
        }
    }

    /// The graph view draws no inspector, so `3` there neither focuses nor
    /// folds anything.
    #[test]
    fn three_does_nothing_where_there_is_no_inspector() {
        let mut app = fold_app();
        app.history_view = HistoryView::Graph;
        app.press_panel_key(FocusedPanel::Inspector);
        assert_eq!(app.focused_panel, FocusedPanel::RepoList);
        assert!(!app.inspector_folded);
    }

    #[test]
    fn the_list_and_detail_numbers_only_focus() {
        let mut app = fold_app();
        for panel in [FocusedPanel::RepoList, FocusedPanel::Detail] {
            app.press_panel_key(panel);
            app.press_panel_key(panel);
            assert_eq!(app.focused_panel, panel);
        }
        app.toggle_fold(FocusedPanel::RepoList);
        app.toggle_fold(FocusedPanel::Detail);
        assert!(!app.inspector_folded && !app.contributions_folded);
    }

    #[test]
    fn clicking_the_tag_folds_in_place_and_unfolds_into_focus() {
        for panel in FOLDABLE {
            let mut app = fold_app();
            app.focus_panel(FocusedPanel::Detail);
            app.toggle_fold(panel);
            assert!(is_folded(&app, panel));
            assert_eq!(app.focused_panel, FocusedPanel::Detail, "fold moved focus");
            app.toggle_fold(panel);
            assert!(!is_folded(&app, panel));
            assert_eq!(app.focused_panel, panel, "unfold did not focus");
        }
    }

    /// Shift-scroll reads a pane whose rows it can see; a folded inspector
    /// has none, so the detail pane walks its own rows instead.
    #[test]
    fn shift_scroll_skips_a_folded_inspector() {
        let mut app = fold_app();
        app.inspector_folded = true;
        app.focus_panel(FocusedPanel::Detail);
        assert_eq!(app.shift_scroll_target(), Some(FocusedPanel::Detail));
    }

    /// Saving a source in settings is asking to see it.
    #[test]
    fn saving_a_source_setting_unfolds_the_strip() {
        let mut app = fold_app();
        app.toggle_fold(FocusedPanel::Contributions);
        app.apply_contributions_setting(ContributionsMode::Local);
        assert!(!app.contributions_folded);
    }

    fn at(h: u32, m: u32) -> DateTime<Local> {
        use chrono::TimeZone;
        Local
            .with_ymd_and_hms(2026, 10, 1, h, m, 0)
            .single()
            .expect("unambiguous local time")
    }

    fn offline() -> crate::github::Failure {
        crate::github::Failure::new(crate::github::FailureKind::Offline, "no route")
    }

    /// Rule 1: reading the cache sets the data time from the file and is not
    /// an attempt, so the retry clock stays unset and the poll is due.
    #[test]
    fn a_cache_read_is_not_an_attempt() {
        let mut f = GithubFreshness::default();
        f.loaded(
            &DatasetSource::Cache {
                written: Some(at(6, 0)),
            },
            at(9, 0),
        );
        assert_eq!(f.data_time, DataTime::At(at(6, 0)));
        assert_eq!(f.last_attempt, None);
        assert_eq!(f.outcome, Outcome::NotAttempted);
        assert!(f.retry_due(at(9, 0), 90));
    }

    /// An unreadable write time stays unknown, never the time it was read.
    #[test]
    fn an_unreadable_cache_age_stays_unknown() {
        let mut f = GithubFreshness::default();
        f.loaded(&DatasetSource::Cache { written: None }, at(9, 0));
        assert_eq!(f.data_time, DataTime::Unknown);
        f.failed(offline(), at(9, 1));
        assert_eq!(f.data_time, DataTime::Unknown, "a failure made the age up");
    }

    /// Rule 3: only a success advances the data time. Every kind of failure,
    /// the give-up among them, leaves it where it was.
    #[test]
    fn a_failure_of_any_kind_leaves_the_data_time() {
        use crate::github::{Failure, FailureKind};
        for kind in [
            FailureKind::Offline,
            FailureKind::Http,
            FailureKind::Api,
            FailureKind::Auth,
            FailureKind::Malformed,
        ] {
            let mut f = GithubFreshness::default();
            f.succeeded(at(9, 0));
            f.failed(Failure::new(kind, "x"), at(9, 5));
            assert_eq!(f.data_time, DataTime::At(at(9, 0)), "{kind:?}");
            assert_eq!(f.outcome, Outcome::Failed(Failure::new(kind, "x")));
        }
        let mut f = GithubFreshness::default();
        f.succeeded(at(9, 0));
        f.gave_up(std::time::Duration::from_secs(60), at(9, 5));
        assert_eq!(f.data_time, DataTime::At(at(9, 0)));
    }

    /// Rule 6: every finished attempt advances the retry clock, whatever it
    /// ended in, so a failure waits out an interval like a success does.
    #[test]
    fn every_finished_attempt_advances_the_retry_clock() {
        let mut f = GithubFreshness::default();
        f.failed(offline(), at(9, 0));
        assert_eq!(f.last_attempt, Some(at(9, 0)));
        assert!(!f.retry_due(at(9, 1), 90));
        assert!(f.retry_due(at(9, 2), 90));
        f.gave_up(std::time::Duration::from_secs(60), at(9, 10));
        assert_eq!(f.last_attempt, Some(at(9, 10)));
        assert!(matches!(
            &f.outcome,
            Outcome::Failed(x) if x.kind == crate::github::FailureKind::TimedOut
        ));
    }

    /// Rule 7: a success clears the failure and makes the data current.
    #[test]
    fn a_success_clears_the_failure() {
        let mut f = GithubFreshness::default();
        f.failed(offline(), at(9, 0));
        assert!(f.is_failing());
        f.succeeded(at(9, 2));
        assert!(!f.is_failing());
        assert_eq!(f.outcome, Outcome::Success);
        assert_eq!(f.data_time, DataTime::At(at(9, 2)));
        assert_eq!(f.last_attempt, Some(at(9, 2)));
    }

    /// The `R` fallback row of the transition table: the rows are rebuilt
    /// from the cache, so the data time is the file's, and the attempt is a
    /// failure.
    #[test]
    fn a_reload_fallback_takes_the_files_time_and_records_the_failure() {
        let mut f = GithubFreshness::default();
        f.succeeded(at(9, 0));
        f.loaded(
            &DatasetSource::Fallback {
                failure: offline(),
                written: Some(at(9, 0)),
            },
            at(10, 0),
        );
        assert_eq!(f.data_time, DataTime::At(at(9, 0)));
        assert_eq!(f.last_attempt, Some(at(10, 0)));
        assert_eq!(f.outcome, Outcome::Failed(offline()));
    }

    /// A load with no GitHub fetch behind it (`-d`, a window apply) says
    /// nothing about freshness.
    #[test]
    fn a_local_load_changes_no_freshness_fact() {
        let mut f = GithubFreshness::default();
        f.failed(offline(), at(9, 0));
        let before = f.clone();
        f.loaded(&DatasetSource::Local, at(10, 0));
        assert_eq!(f, before);
    }

    /// A load that went to GitHub supersedes every poll started before it;
    /// a cache read or a local load supersedes none.
    #[test]
    fn only_a_load_from_github_supersedes_earlier_polls() {
        for (source, supersedes) in [
            (DatasetSource::Network, true),
            (
                DatasetSource::Fallback {
                    failure: offline(),
                    written: None,
                },
                true,
            ),
            (DatasetSource::Cache { written: None }, false),
            (DatasetSource::Local, false),
        ] {
            let mut f = GithubFreshness::default();
            let poll = f.begin_attempt();
            f.loaded(&source, at(9, 0));
            assert_eq!(!f.accept_answer(poll), supersedes, "{source:?}");
        }
    }

    /// Rule 10: an answer is applied only when no newer attempt's answer
    /// has been, in either completion order.
    #[test]
    fn an_answer_older_than_the_one_applied_is_discarded() {
        let mut f = GithubFreshness::default();
        let first = f.begin_attempt();
        let second = f.begin_attempt();
        assert!(f.is_latest(second) && !f.is_latest(first));
        assert!(f.accept_answer(second));
        assert!(!f.accept_answer(first), "an older answer was applied");

        let mut g = GithubFreshness::default();
        let first = g.begin_attempt();
        let second = g.begin_attempt();
        assert!(g.accept_answer(first), "nothing newer had landed");
        assert!(g.accept_answer(second));
    }

    use crate::ui::badges::{active_icons, set_active_icons};

    /// One repo of each kind, every one carrying a stash, a linked worktree
    /// and an owner, so each glyph the `icons` key covers is on screen.
    /// `extra` more repos cycle through the kinds, for a grid of several rows.
    fn icon_fleet(extra: usize) -> App {
        use crate::model::RepoKind;
        let kinds = [
            RepoKind::Org,
            RepoKind::Private,
            RepoKind::Public,
            RepoKind::Unknown,
        ];
        let mut app = make_app(kinds.len() + extra);
        for (i, repo) in app.repos.iter_mut().enumerate() {
            repo.repo_kind = kinds[i % kinds.len()];
            repo.owner = Some("acme".to_string());
            repo.stash_count = 2;
            repo.worktrees = vec![crate::model::Worktree {
                path: format!("~/repos/repo-{i}-wt"),
                branch: "wt".to_string(),
                ..Default::default()
            }];
            // Uneven badge groups, so a right edge that lines up is evidence.
            repo.pr_count = i as u32 % 2;
        }
        app
    }

    const SCREEN_W: u16 = 120;
    const SCREEN_H: u16 = 40;

    fn draw_screen(app: &App, width: u16) -> ratatui::buffer::Buffer {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut terminal = Terminal::new(TestBackend::new(width, SCREEN_H)).unwrap();
        terminal.draw(|f| crate::ui::render(f, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn is_private_use(c: char) -> bool {
        matches!(c as u32, 0xE000..=0xF8FF | 0xF0000..)
    }

    /// Every cell holding a private-use codepoint, as `(x, y, symbol)`,
    /// outside the contribution strip: its day squares are FZ-13h, not part
    /// of the `icons` key. The strip's rectangle comes from `panel_rects`, the
    /// same function the renderer lays out with.
    fn private_use_cells(app: &App, buffer: &ratatui::buffer::Buffer) -> Vec<(u16, u16, String)> {
        let area = buffer.area;
        let strip = crate::ui::panel_rects(area, app).contributions;
        let mut found = Vec::new();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if strip.is_some_and(|s| s.contains(ratatui::layout::Position { x, y })) {
                    continue;
                }
                let symbol = buffer[(x, y)].symbol();
                if symbol.chars().any(is_private_use) {
                    found.push((x, y, symbol.to_string()));
                }
            }
        }
        found
    }

    /// Where `text` starts on row `y` inside `area`, matched cell by cell.
    fn find_in_row(
        buffer: &ratatui::buffer::Buffer,
        area: ratatui::layout::Rect,
        y: u16,
        text: &str,
    ) -> Option<u16> {
        let wanted: Vec<String> = text.chars().map(String::from).collect();
        (area.left()..area.right()).find(|&x| {
            wanted.iter().enumerate().all(|(i, c)| {
                let cx = x + i as u16;
                cx < area.right() && buffer[(cx, y)].symbol() == c
            })
        })
    }

    /// `(x, y)` of each repo's name inside `area`, first match from the top.
    fn name_cells(
        app: &App,
        buffer: &ratatui::buffer::Buffer,
        area: ratatui::layout::Rect,
    ) -> Vec<(u16, u16)> {
        app.repos
            .iter()
            .map(|r| {
                (area.top()..area.bottom())
                    .find_map(|y| {
                        find_in_row(buffer, area, y, &format!(" {} ", r.name)).map(|x| (x + 1, y))
                    })
                    .unwrap_or_else(|| panic!("{} not drawn", r.name))
            })
            .collect()
    }

    /// The list, every detail pane and the header, in both watch states,
    /// draw nothing from the private-use area under `icons = "plain"`.
    /// Fails if any of the six glyphs, or a call site drawing one, still
    /// reaches for a Nerd Font codepoint in `plain`.
    #[test]
    fn plain_icons_draw_no_private_use_glyph_in_the_detail_view() {
        let mut app = icon_fleet(0);
        app.view = View::Detail;
        set_active_icons(Icons::Plain);
        for selected in 0..app.repos.len() {
            app.selected = selected;
            for watch in [true, false] {
                app.watch_mode = watch;
                let buffer = draw_screen(&app, SCREEN_W);
                let found = private_use_cells(&app, &buffer);
                assert!(
                    found.is_empty(),
                    "repo {selected}, watch {watch}: private-use glyphs at {found:?}"
                );
            }
        }
    }

    /// The compact grid, the same way.
    #[test]
    fn plain_icons_draw_no_private_use_glyph_in_the_compact_grid() {
        let mut app = icon_fleet(4);
        app.view = View::Compact;
        set_active_icons(Icons::Plain);
        let buffer = draw_screen(&app, SCREEN_W);
        let found = private_use_cells(&app, &buffer);
        assert!(found.is_empty(), "private-use glyphs at {found:?}");
    }

    /// The control for the two scans above: the same screens under the Nerd
    /// set do draw Nerd Font glyphs, so an empty scan in `plain` is the
    /// setting at work and not a scan that cannot see them. Fails if the Nerd
    /// set stops drawing them or the fixture stops carrying them.
    #[test]
    fn the_nerd_set_draws_the_nerd_font_glyphs() {
        let mut app = icon_fleet(0);
        set_active_icons(Icons::Nerd);
        app.watch_mode = true;
        app.view = View::Detail;
        let detail = draw_screen(&app, SCREEN_W);
        let drawn: Vec<String> = private_use_cells(&app, &detail)
            .into_iter()
            .map(|(_, _, s)| s)
            .collect();
        for kind in [
            crate::model::RepoKind::Org,
            crate::model::RepoKind::Private,
            crate::model::RepoKind::Public,
        ] {
            let glyph = crate::ui::badges::kind_glyph(kind).unwrap();
            assert!(drawn.iter().any(|s| s == glyph), "{kind:?} glyph missing");
        }
        assert!(drawn.iter().any(|s| s == crate::ui::badges::STASH_GLYPH));
        let eye = crate::ui::watch_icon(true);
        assert!(eye.chars().all(is_private_use), "{eye:?}");
        assert!(drawn.iter().any(|s| s == eye));

        app.view = View::Compact;
        let compact = draw_screen(&app, SCREEN_W);
        assert!(!private_use_cells(&app, &compact).is_empty());
    }

    /// In `plain` the list rows keep one name column and one right edge for
    /// the status token, a repo of unknown kind (blank glyph) included. Fails
    /// if a plain glyph measures other than one column, which would break
    /// `badges::GLYPH_W` or the badge width the dot fill is computed from.
    #[test]
    fn plain_icons_keep_the_repo_list_aligned() {
        let mut app = icon_fleet(0);
        app.view = View::Detail;
        set_active_icons(Icons::Plain);
        let buffer = draw_screen(&app, SCREEN_W);
        let list = crate::ui::panel_rects(buffer.area, &app).repo_list;
        let names = name_cells(&app, &buffer, list);

        let name_x = names[0].0;
        assert!(
            names.iter().all(|(x, _)| *x == name_x),
            "names at {names:?}"
        );

        // The status token is the last thing before the border on every row.
        let right_edges: Vec<u16> = names
            .iter()
            .map(|&(_, y)| {
                (list.left()..list.right() - 1)
                    .rev()
                    .find(|&x| buffer[(x, y)].symbol() != " ")
                    .unwrap()
            })
            .collect();
        assert!(
            right_edges.iter().all(|x| *x == right_edges[0]),
            "status tokens end at {right_edges:?}"
        );

        // And the name column is where the Nerd set puts it. The control
        // selects its set itself and proves it drew it, so it cannot quietly
        // compare plain with plain.
        let mut nerd = icon_fleet(0);
        nerd.view = View::Detail;
        set_active_icons(Icons::Nerd);
        let nerd_buffer = draw_screen(&nerd, SCREEN_W);
        let nerd_names = name_cells(&nerd, &nerd_buffer, list);
        let org = crate::ui::badges::kind_glyph(crate::model::RepoKind::Org).unwrap();
        let (org_x, org_y) = nerd_names[0];
        assert_eq!(
            nerd_buffer[(org_x - 2, org_y)].symbol(),
            org,
            "the control did not draw the Nerd set"
        );
        assert_eq!(nerd_names[0].0, name_x);
    }

    /// In `plain` the compact grid's rows share their columns: each name in
    /// the second row starts under the one above it. Fails if a plain glyph
    /// or badge measured differently from what `compact.rs` reserves for it.
    #[test]
    fn plain_icons_keep_the_compact_grid_aligned() {
        let mut app = icon_fleet(4);
        app.view = View::Compact;
        set_active_icons(Icons::Plain);
        let buffer = draw_screen(&app, SCREEN_W);
        let names = name_cells(&app, &buffer, buffer.area);

        let mut rows: Vec<u16> = names.iter().map(|(_, y)| *y).collect();
        rows.sort();
        rows.dedup();
        assert!(rows.len() >= 2, "the fixture must wrap: {names:?}");
        let columns = |y: u16| -> Vec<u16> {
            let mut xs: Vec<u16> = names
                .iter()
                .filter(|(_, ny)| *ny == y)
                .map(|(x, _)| *x)
                .collect();
            xs.sort();
            xs
        };
        let first = columns(rows[0]);
        for &y in &rows[1..] {
            let row = columns(y);
            assert_eq!(row[..], first[..row.len()], "row {y} drifted: {names:?}");
        }
    }

    /// The org glyph on the first list row and the header's watch mark, as
    /// drawn under whatever set is active.
    fn row_and_header_marks(app: &App) -> (String, String) {
        let buffer = draw_screen(app, SCREEN_W);
        let list = crate::ui::panel_rects(buffer.area, app).repo_list;
        let (x, y) = name_cells(app, &buffer, list)[0];
        let header: String = (0..buffer.area.width)
            .map(|x| buffer[(x, 0)].symbol())
            .collect();
        let watch = header
            .split_whitespace()
            .take_while(|word| *word != "watch")
            .last()
            .unwrap_or_default()
            .to_string();
        (buffer[(x - 2, y)].symbol().to_string(), watch)
    }

    /// One source: selecting a set changes what the rows and the header draw
    /// together, and a thread that never selected one draws the Nerd set.
    /// Fails if the header and the rows read different settings, or if a
    /// fresh thread starts on anything but Nerd.
    #[test]
    fn selecting_a_set_changes_the_rows_and_the_header_together() {
        use crate::model::RepoKind;
        let mut app = icon_fleet(0);
        app.view = View::Detail;
        app.watch_mode = true;
        let nerd_org = crate::ui::badges::kind_glyph(RepoKind::Org).unwrap();

        set_active_icons(Icons::Plain);
        let (org, watch) = row_and_header_marks(&app);
        assert_eq!(org, "@");
        assert_eq!(watch, "o");

        set_active_icons(Icons::Nerd);
        let (org, watch) = row_and_header_marks(&app);
        assert_eq!(org, nerd_org);
        assert!(watch.chars().all(is_private_use), "{watch:?}");

        set_active_icons(Icons::Plain);
        let fresh = std::thread::spawn(|| {
            let mut app = icon_fleet(0);
            app.view = View::Detail;
            app.watch_mode = true;
            (active_icons(), row_and_header_marks(&app))
        })
        .join()
        .unwrap();
        assert_eq!(fresh.0, Icons::Nerd);
        assert_eq!(fresh.1.0, nerd_org);
        assert!(fresh.1.1.chars().all(is_private_use), "{:?}", fresh.1.1);
    }
    fn timed_out() -> crate::git::FetchOutcome {
        crate::git::FetchOutcome::Failed(crate::git::FetchFailure {
            kind: crate::git::FetchFailureKind::TimedOut,
            detail: None,
        })
    }

    /// FZ-35b (#85), criterion 3: a fetch cut at the cap is not counted by
    /// the timeout backoff, neither holding a path nor resetting a count.
    #[test]
    fn the_fetch_backoff_ignores_a_fetch_cut_at_the_cap() {
        let t0 = Local::now();
        let too_big =
            crate::git::FetchOutcome::Failed(crate::git::FetchFailure::too_big(300 * 1024 * 1024));
        let mut b = FetchBackoff::default();
        b.record("~/r", &too_big, t0);
        assert_eq!(b.timeouts("~/r"), 0);
        assert!(!b.holds("~/r", t0, 60));
        b.record("~/r", &timed_out(), t0);
        b.record("~/r", &too_big, t0 + chrono::Duration::seconds(1));
        assert_eq!(b.timeouts("~/r"), 1, "the count stands");
    }

    /// FZ-35b (#85), criterion 7: an `f` still downloading past
    /// `SWEEP_STUCK_AFTER` from its start keeps its report and its refresh,
    /// timed from its last progress; one silent that long is given up on.
    #[test]
    fn a_fetch_report_lives_while_its_fetch_reports_progress() {
        let stuck = std::time::Duration::from_secs(180);
        let long_ago =
            |secs| std::time::Instant::now().checked_sub(std::time::Duration::from_secs(secs));
        let mut app = make_app(2);
        let path = app.repos[0].path.clone().unwrap();
        let quiet = app.repos[1].path.clone().unwrap();
        app.manual_fetches.expect([path.clone()]);
        app.refresh_awaits_poll = true;
        app.settle_refresh_poll([path.clone()]);
        // Started 200 s ago, under the 180 s limit's nose.
        app.manual_fetches.started = long_ago(200);
        app.refresh_fetches_started = long_ago(200);
        // Another repo's progress is no sign of life for this batch.
        app.apply_fetch_progress(&quiet, receiving(1));
        assert!(
            app.manual_fetches.started.unwrap().elapsed() > stuck,
            "another repo's progress kept this batch alive"
        );
        app.apply_fetch_progress(&path, receiving(300 * 1024 * 1024));
        app.expire_refresh_fetches(stuck);
        assert!(!app.manual_fetches.awaiting.is_empty(), "the tally expired");
        assert!(
            app.is_refreshing(),
            "the refresh gave up on a fetch still moving"
        );
        let report = app.manual_fetches.land(
            &path,
            "repo-0",
            &crate::git::FetchOutcome::Failed(crate::git::FetchFailure {
                kind: crate::git::FetchFailureKind::Unreachable,
                detail: None,
            }),
        );
        assert_eq!(report.map(|r| r.total), Some(1), "the report was lost");

        let mut silent = make_app(1);
        let path = silent.repos[0].path.clone().unwrap();
        silent.manual_fetches.expect([path.clone()]);
        silent.manual_fetches.started = long_ago(200);
        silent.expire_refresh_fetches(stuck);
        assert_eq!(
            silent.manual_fetches.land(&path, "repo-0", &timed_out()),
            None,
            "a silent batch expires as before"
        );
    }

    /// FZ-34 (#83), R3: after the nth consecutive timeout a path is held for
    /// `backoff_secs(n + 1)` from that timeout: two intervals, then four,
    /// doubling up to the cap.
    #[test]
    fn consecutive_fetch_timeouts_hold_two_intervals_then_four_up_to_the_cap() {
        let t0 = Local::now();
        let s = chrono::Duration::seconds;
        let mut b = FetchBackoff::default();
        assert!(!b.holds("~/r", t0, 60), "nothing recorded holds nothing");
        b.record("~/r", &timed_out(), t0);
        assert_eq!(b.timeouts("~/r"), 1);
        assert!(b.holds("~/r", t0 + s(119), 60));
        assert!(!b.holds("~/r", t0 + s(120), 60));
        let t1 = t0 + s(120);
        b.record("~/r", &timed_out(), t1);
        assert_eq!(b.timeouts("~/r"), 2);
        assert!(b.holds("~/r", t1 + s(239), 60));
        assert!(!b.holds("~/r", t1 + s(240), 60));
        for i in 1..10 {
            b.record("~/r", &timed_out(), t1 + s(i));
        }
        let last = t1 + s(9);
        assert!(b.holds("~/r", last + s(BACKOFF_CAP_SECS - 1), 60));
        assert!(!b.holds("~/r", last + s(BACKOFF_CAP_SECS), 60));
        assert!(!b.holds("~/other", t0, 60), "a hold is per path");
    }

    /// A timeout older than the one already counted (a load's fetch landing
    /// after a sweep's) is not counted again.
    #[test]
    fn an_older_timeout_is_not_counted_twice() {
        let t0 = Local::now();
        let mut b = FetchBackoff::default();
        b.record("~/r", &timed_out(), t0);
        b.record("~/r", &timed_out(), t0);
        b.record("~/r", &timed_out(), t0 - chrono::Duration::seconds(5));
        assert_eq!(b.timeouts("~/r"), 1);
    }

    /// Only a success or a manual refresh lifts the hold; another failure
    /// kind neither holds a path nor resets the count.
    #[test]
    fn a_success_or_a_clear_lifts_the_hold_and_other_failures_do_not() {
        use crate::git::{FetchFailure, FetchFailureKind, FetchOutcome};
        let t0 = Local::now();
        let mut b = FetchBackoff::default();
        let unreachable = FetchOutcome::Failed(FetchFailure {
            kind: FetchFailureKind::Unreachable,
            detail: None,
        });
        b.record("~/u", &unreachable, t0);
        assert!(!b.holds("~/u", t0, 60));

        b.record("~/r", &timed_out(), t0);
        b.record("~/r", &unreachable, t0 + chrono::Duration::seconds(1));
        b.record("~/r", &FetchOutcome::NotAttempted, t0);
        assert_eq!(b.timeouts("~/r"), 1);
        assert!(b.holds("~/r", t0 + chrono::Duration::seconds(1), 60));

        b.record("~/r", &FetchOutcome::Succeeded, t0);
        assert!(!b.holds("~/r", t0, 60));
        assert_eq!(b.timeouts("~/r"), 0);

        b.record("~/r", &timed_out(), t0);
        b.clear();
        assert!(!b.holds("~/r", t0, 60));
    }

    /// FZ-34, R5: a running fetch is not started twice. Landing it ends the
    /// claim; a sweep that only rebuilt the repo does not.
    #[test]
    fn a_running_fetch_is_claimed_once_until_it_lands() {
        let mut app = make_app(1);
        let t0 = std::time::Instant::now();
        assert!(app.claim_fetch("~/repos/repo-0", t0));
        assert!(
            !app.claim_fetch("~/repos/repo-0", t0),
            "a second fetch started"
        );
        assert!(app.claim_fetch("~/repos/other", t0), "a claim is per path");
        app.fetch_landed("~/repos/repo-0", &crate::git::FetchOutcome::NotAttempted);
        assert!(
            !app.claim_fetch("~/repos/repo-0", t0),
            "a sweep without a fetch ended the claim"
        );
        app.fetch_landed("~/repos/repo-0", &timed_out());
        assert!(app.claim_fetch("~/repos/repo-0", t0));
        app.fetch_landed("~/repos/repo-0", &crate::git::FetchOutcome::Succeeded);
        assert!(app.claim_fetch("~/repos/repo-0", t0));
    }

    /// FZ-34, R5: a claim whose result never landed expires after the limit.
    #[test]
    fn a_fetch_claim_expires_after_the_stuck_limit() {
        let mut app = make_app(1);
        let t0 = std::time::Instant::now();
        let limit = std::time::Duration::from_secs(180);
        assert!(app.claim_fetch("~/repos/repo-0", t0));
        app.expire_fetches_in_flight(limit, t0 + limit);
        assert!(
            !app.claim_fetch("~/repos/repo-0", t0),
            "expired at the limit"
        );
        app.expire_fetches_in_flight(limit, t0 + limit + std::time::Duration::from_secs(1));
        assert!(app.claim_fetch("~/repos/repo-0", t0));
    }

    /// FZ-35a (#84), criterion 4: a fetch that waited in the queue past the
    /// stuck limit keeps its claim once its permit is granted, because the
    /// claim is timed from the permit, and progress keeps it alive after.
    #[test]
    fn a_claim_is_timed_from_its_permit_and_kept_alive_by_progress() {
        let claims = FetchClaims::default();
        let t0 = std::time::Instant::now();
        let limit = std::time::Duration::from_secs(180);
        let token = claims.reserve("~/r", t0).expect("free");
        // Queued 170 s, then granted its permit.
        assert!(claims.start("~/r", token, t0 + std::time::Duration::from_secs(170)));
        claims.expire(limit, t0 + std::time::Duration::from_secs(200));
        assert!(claims.contains_key("~/r"), "expired while it ran");
        // Running 160 s more, reporting progress.
        claims.touch("~/r", token, t0 + std::time::Duration::from_secs(330));
        claims.expire(limit, t0 + std::time::Duration::from_secs(400));
        assert!(claims.contains_key("~/r"), "expired while it progressed");
        // Silent past the limit: the safety net still frees it.
        claims.expire(limit, t0 + std::time::Duration::from_secs(511));
        assert!(!claims.contains_key("~/r"));
    }

    /// FZ-35a (#84), criterion 4: a queued fetch whose reservation expired
    /// takes the path again at its permit when it is free, and does not run
    /// when a newer claim holds it, so one repo never has two fetches.
    #[test]
    fn a_permit_takes_a_free_path_and_refuses_a_held_one() {
        let claims = FetchClaims::default();
        let t0 = std::time::Instant::now();
        let limit = std::time::Duration::from_secs(180);
        let old = claims.reserve("~/r", t0).unwrap();
        claims.expire(limit, t0 + std::time::Duration::from_secs(181));
        assert!(!claims.contains_key("~/r"));
        assert!(claims.start("~/r", old, t0 + std::time::Duration::from_secs(182)));
        assert_eq!(claims.token("~/r"), Some(old));

        let claims = FetchClaims::default();
        let old = claims.reserve("~/r", t0).unwrap();
        claims.expire(limit, t0 + std::time::Duration::from_secs(181));
        let newer = claims
            .reserve("~/r", t0 + std::time::Duration::from_secs(182))
            .unwrap();
        assert_ne!(old, newer, "a token is never reused");
        assert!(!claims.start("~/r", old, t0 + std::time::Duration::from_secs(183)));
        assert!(claims.start("~/r", newer, t0 + std::time::Duration::from_secs(183)));
        // The refused fetch's progress, had it any, would not touch it.
        claims.touch("~/r", old, t0 + std::time::Duration::from_secs(400));
        claims.expire(limit, t0 + std::time::Duration::from_secs(364));
        assert!(!claims.contains_key("~/r"));
    }

    /// FZ-35a (#84), the #83 reviewer's follow-up: a late landing from a
    /// claim that expired does not end the newer claim on the same path; the
    /// newer fetch's own landing does.
    #[test]
    fn a_late_landing_does_not_end_a_newer_claim() {
        let mut app = make_app(1);
        let t0 = std::time::Instant::now();
        let limit = std::time::Duration::from_secs(180);
        assert!(app.claim_fetch("~/repos/repo-0", t0));
        let old = app.fetches_in_flight.token("~/repos/repo-0").unwrap();
        app.expire_fetches_in_flight(limit, t0 + std::time::Duration::from_secs(181));
        assert!(app.claim_fetch("~/repos/repo-0", t0 + std::time::Duration::from_secs(182)));
        let newer = app.fetches_in_flight.token("~/repos/repo-0").unwrap();
        app.fetch_landed_under("~/repos/repo-0", &timed_out(), Some(old));
        assert!(
            app.fetches_in_flight.contains_key("~/repos/repo-0"),
            "the late landing ended the newer claim"
        );
        app.fetch_landed_under(
            "~/repos/repo-0",
            &crate::git::FetchOutcome::NotAttempted,
            Some(newer),
        );
        assert!(app.fetches_in_flight.contains_key("~/repos/repo-0"));
        app.fetch_landed_under(
            "~/repos/repo-0",
            &crate::git::FetchOutcome::Succeeded,
            Some(newer),
        );
        assert!(!app.fetches_in_flight.contains_key("~/repos/repo-0"));
    }

    fn receiving(bytes: u64) -> crate::git::FetchProgress {
        crate::git::FetchProgress {
            phase: crate::git::FetchPhase::Receiving,
            objects_done: 1,
            objects_total: 2,
            bytes,
        }
    }

    /// FZ-35a (#84), criterion 3: progress lands on the row's fetch facts
    /// and goes when the fetch lands; a sweep that did not fetch leaves it.
    #[test]
    fn progress_is_on_the_row_until_the_fetch_lands() {
        let mut app = make_app(2);
        let path = app.repos[0].path.clone().unwrap();
        app.needs_redraw = false;
        app.apply_fetch_progress(&path, receiving(5));
        assert_eq!(app.repos[0].fetch.progress, Some(receiving(5)));
        assert_eq!(app.repos[1].fetch.progress, None);
        assert!(app.needs_redraw);
        app.apply_fetch_progress("~/no/such/row", receiving(9));
        let now = Local::now();
        app.repos[0]
            .fetch
            .record(&crate::git::FetchOutcome::NotAttempted, now);
        assert_eq!(app.repos[0].fetch.progress, Some(receiving(5)));
        app.repos[0].fetch.record(&timed_out(), now);
        assert_eq!(app.repos[0].fetch.progress, None);
    }

    /// FZ-35a (#84): a row whose fetch claim expired, with nothing left to
    /// land, stops showing progress; a row still claimed keeps it.
    #[test]
    fn an_expired_claim_takes_its_progress_off_the_row() {
        let mut app = make_app(2);
        let t0 = std::time::Instant::now();
        let limit = std::time::Duration::from_secs(180);
        let (a, b) = (
            app.repos[0].path.clone().unwrap(),
            app.repos[1].path.clone().unwrap(),
        );
        assert!(app.claim_fetch(&a, t0));
        assert!(app.claim_fetch(&b, t0 + std::time::Duration::from_secs(100)));
        app.apply_fetch_progress(&a, receiving(5));
        app.apply_fetch_progress(&b, receiving(7));
        app.expire_fetches_in_flight(limit, t0 + std::time::Duration::from_secs(181));
        assert_eq!(app.repos[0].fetch.progress, None);
        assert_eq!(app.repos[1].fetch.progress, Some(receiving(7)));
    }

    /// The app measures the hold in its watch interval.
    #[test]
    fn the_app_holds_a_timed_out_path_in_watch_intervals() {
        let mut app = make_app(1);
        app.refresh_interval = 30;
        let t0 = Local::now();
        app.fetch_backoff.record("~/repos/repo-0", &timed_out(), t0);
        assert!(app.fetch_held("~/repos/repo-0", t0 + chrono::Duration::seconds(59)));
        assert!(!app.fetch_held("~/repos/repo-0", t0 + chrono::Duration::seconds(60)));
    }

    // --- The `worktrees` view (FZ-77) ---

    /// An app on one repo with one linked worktree, pane 2 in the detail
    /// view.
    fn worktrees_app() -> App {
        let mut app = make_app(1);
        app.view = View::Detail;
        app.repos[0].worktrees = vec![crate::model::Worktree {
            path: "~/repos/repo-0-wt".to_string(),
            branch: "side".to_string(),
            head: "b2c3d4e".to_string(),
            dirty: Some(0),
            ..Default::default()
        }];
        app
    }

    /// `[` and `]` reach the view: forward from `graph`, back from
    /// `branches`.
    #[test]
    fn brackets_reach_the_worktrees_view() {
        let mut app = worktrees_app();
        app.history_view = HistoryView::Graph;
        app.press_bracket(false);
        assert_eq!(app.history_view, HistoryView::Worktrees);
        app.history_view = HistoryView::Branches;
        app.press_bracket(true);
        assert_eq!(app.history_view, HistoryView::Worktrees);
        assert!(app.worktrees_wanted());
    }

    /// The log is read only while the view is on screen with the inspector
    /// open, once the cursor has rested on the repo for the dwell, one read
    /// per worktree at a time, held once landed, and read again after a
    /// sweep.
    #[test]
    fn a_worktree_log_is_read_on_demand_and_again_after_a_sweep() {
        let start = std::time::Instant::now();
        let after = |ms: u64| start + std::time::Duration::from_millis(ms);
        let rested = after(2000) + crate::ci::DWELL;
        let mut app = worktrees_app();
        app.inspector_folded = false;
        for view in HistoryView::ALL {
            if view == HistoryView::Worktrees {
                continue;
            }
            app.history_view = view;
            assert_eq!(app.claim_worktree_log(start), None, "{view:?}");
            assert_eq!(app.claim_worktree_log(after(1000)), None, "{view:?}");
        }
        app.history_view = HistoryView::Worktrees;
        app.view = View::Compact;
        assert_eq!(app.claim_worktree_log(after(1500)), None, "compact");
        app.view = View::Detail;

        app.detail_selections[HistoryView::Worktrees.index()] = 1;
        assert_eq!(
            app.claim_worktree_log(after(2000)),
            None,
            "the dwell starts"
        );
        let early = rested - std::time::Duration::from_millis(1);
        assert_eq!(app.claim_worktree_log(early), None, "still dwelling");
        app.inspector_folded = true;
        assert_eq!(app.claim_worktree_log(rested), None, "inspector folded");
        app.inspector_folded = false;
        let read = app
            .claim_worktree_log(rested)
            .expect("reads the highlighted one");
        assert_eq!(read.key, "~/repos/repo-0-wt");
        assert!(read.dir.ends_with("repos/repo-0-wt"), "{:?}", read.dir);
        assert_eq!(app.claim_worktree_log(rested), None, "one at a time");
        app.apply_worktree_log(read, Vec::new());
        assert_eq!(app.worktree_log().map(<[_]>::len), Some(0));
        assert_eq!(app.claim_worktree_log(rested), None, "held, not read again");

        app.sweep_landed(chrono::Local::now());
        assert!(
            app.claim_worktree_log(rested).is_some(),
            "read again after a sweep"
        );
    }

    // --- The `notes` view (FZ-53) ---

    /// An app on one repo at `/notes-fixture`, pane 2 in the detail view.
    fn notes_app() -> App {
        let mut app = make_app(1);
        app.repos[0].path = Some("/notes-fixture".to_string());
        app.view = View::Detail;
        app.inspector_folded = false;
        app
    }

    fn note(rel: &str) -> crate::notes::NoteFile {
        crate::notes::NoteFile {
            rel: std::path::PathBuf::from(rel),
            notez: rel.starts_with("notez/").then(|| "notez".into()),
        }
    }

    /// Land a walk of `/notes-fixture` holding `files`.
    fn walked(app: &mut App, files: &[&str]) {
        let walk = NotesWalk {
            key: "/notes-fixture".to_string(),
            root: "/notes-fixture".into(),
            generation: app.notes_generation,
        };
        let notes = crate::notes::Notes {
            files: files.iter().map(|f| note(f)).collect(),
        };
        app.apply_notes_walk(walk, Ok(notes));
    }

    /// The walk starts only while `notes` is on screen in the detail view,
    /// and only once the cursor has rested on the row for the dwell. Fails
    /// if any other view, or the compact grid, walks the repo.
    #[test]
    fn the_notes_walk_waits_for_its_view_and_the_dwell() {
        let start = std::time::Instant::now();
        let after = |ms: u64| start + std::time::Duration::from_millis(ms);
        let mut app = notes_app();
        for view in HistoryView::ALL {
            if view == HistoryView::Notes {
                continue;
            }
            app.history_view = view;
            assert_eq!(app.claim_notes_walk(start), None, "{view:?}");
            assert_eq!(app.claim_notes_walk(after(1000)), None, "{view:?}");
        }
        app.history_view = HistoryView::Notes;
        app.view = View::Compact;
        assert_eq!(app.claim_notes_walk(after(2000)), None, "compact");

        app.view = View::Detail;
        assert_eq!(app.claim_notes_walk(after(3000)), None, "the dwell starts");
        assert_eq!(app.claim_notes_walk(after(3299)), None, "still dwelling");
        let walk = app.claim_notes_walk(after(3300)).expect("walks");
        assert_eq!(walk.root, std::path::PathBuf::from("/notes-fixture"));
        assert_eq!(app.claim_notes_walk(after(4000)), None, "one at a time");
        assert!(app.notes.values().all(|s| s.walking));
    }

    /// A not-cloned row has nothing to walk.
    #[test]
    fn a_not_cloned_row_is_never_walked() {
        let start = std::time::Instant::now();
        let mut app = notes_app();
        app.repos[0].path = None;
        app.history_view = HistoryView::Notes;
        app.claim_notes_walk(start);
        let later = start + std::time::Duration::from_secs(5);
        assert_eq!(app.claim_notes_walk(later), None);
        assert!(app.notes.is_empty());
    }

    /// The walk is held for the session: leaving the view and coming back
    /// draws it again without a walk, until a sweep lands or a suspend
    /// returns (`invalidate_notes`). The old tree stays drawn meanwhile.
    #[test]
    fn coming_back_does_not_rewalk_until_a_sweep_or_a_suspend() {
        let start = std::time::Instant::now();
        let after = |ms: u64| start + std::time::Duration::from_millis(ms);
        let mut app = notes_app();
        app.history_view = HistoryView::Notes;
        app.claim_notes_walk(start);
        app.claim_notes_walk(after(300)).expect("first walk");
        walked(&mut app, &["notez/a.md"]);

        app.history_view = HistoryView::Info;
        app.claim_notes_walk(after(400));
        app.history_view = HistoryView::Notes;
        app.claim_notes_walk(after(500));
        assert_eq!(app.claim_notes_walk(after(5000)), None, "held, not walked");
        assert_eq!(app.notes_listed().len(), 1);

        app.invalidate_notes();
        assert!(app.claim_notes_walk(after(5100)).is_some(), "walked again");
        assert_eq!(app.notes_listed().len(), 1, "the old tree stays drawn");
    }

    /// A landed sweep has the held walk read again: the `SweepDone` arm
    /// goes through `sweep_landed`, so dropping its invalidation fails here.
    #[test]
    fn a_sweep_landing_rewalks_the_notes() {
        let start = std::time::Instant::now();
        let after = |ms: u64| start + std::time::Duration::from_millis(ms);
        let mut app = notes_app();
        app.history_view = HistoryView::Notes;
        app.claim_notes_walk(start);
        app.claim_notes_walk(after(300)).expect("first walk");
        walked(&mut app, &["notez/a.md"]);
        assert_eq!(app.claim_notes_walk(after(5000)), None, "held, not walked");

        app.sweep_landed(chrono::Local::now());
        assert!(app.claim_notes_walk(after(5100)).is_some(), "walked again");
    }

    /// The body read is the highlighted file's alone, however far the cursor
    /// moved since the last tick, and nothing outside the view. Read again
    /// after a suspend returns, and with the inspector folded: the reader beside
    /// the tree still shows the body (FZ-62).
    #[test]
    fn only_the_highlighted_file_is_read() {
        let mut app = notes_app();
        app.history_view = HistoryView::Notes;
        walked(&mut app, &["notez/a.md", "notez/b.md", "notez/c.md"]);
        app.history_view = HistoryView::Info;
        assert_eq!(app.claim_note_read(), None, "another view reads nothing");

        app.history_view = HistoryView::Notes;
        app.select_commit_down();
        app.select_commit_down();
        let read = app.claim_note_read().expect("reads");
        assert_eq!(
            read.path,
            std::path::PathBuf::from("/notes-fixture/notez/c.md")
        );
        assert_eq!(app.claim_note_read(), None, "read once");

        // An answer for a file no longer highlighted is dropped.
        app.select_commit_up();
        let b = app.claim_note_read().expect("b");
        app.apply_note_read(read, Ok("c".to_string()));
        assert_eq!(app.note_body.as_ref().and_then(|b| b.text.clone()), None);
        app.apply_note_read(b, Ok("b".to_string()));
        assert_eq!(
            app.note_body.as_ref().and_then(|b| b.text.clone()),
            Some(Ok("b".to_string()))
        );

        app.invalidate_notes();
        assert!(app.claim_note_read().is_some(), "re-read after a suspend");

        app.invalidate_notes();
        app.inspector_folded = true;
        assert!(app.claim_note_read().is_some(), "folded card, open reader");
    }

    /// `m` widens the tree to every markdown file and back, keeping the
    /// cursor on its file where the other tree lists it. Does nothing
    /// outside the view.
    #[test]
    fn the_markdown_toggle_widens_the_tree_and_keeps_the_file() {
        let mut app = notes_app();
        app.history_view = HistoryView::Notes;
        walked(&mut app, &["README.md", "docs/guide.md", "notez/a.md"]);
        assert_eq!(app.selected_commit_count(), 1);

        app.toggle_notes_markdown();
        assert!(app.notes_all_markdown);
        assert_eq!(app.selected_commit_count(), 3);
        assert_eq!(
            app.notes_highlighted(),
            Some("/notes-fixture/notez/a.md".into()),
            "the cursor stayed on its file"
        );

        app.history_view = HistoryView::Info;
        app.toggle_notes_markdown();
        assert!(app.notes_all_markdown, "not the view");
    }

    /// In the `notes` layout Tab walks repo list, tree, reader, strip and
    /// Shift-Tab walks it back, with no card to stop on since FZ-68 round 2:
    /// `3` does not reach an inspector there either. The reader takes focus
    /// nowhere else, and leaving the view moves focus off it (FZ-62).
    #[test]
    fn focus_walks_tree_and_reader_in_the_notes_layout() {
        let mut app = notes_app();
        app.history_view = HistoryView::Notes;
        let order = [
            FocusedPanel::RepoList,
            FocusedPanel::Detail,
            FocusedPanel::Reader,
            FocusedPanel::Contributions,
        ];
        app.focused_panel = FocusedPanel::RepoList;
        for want in order.iter().cycle().skip(1).take(order.len()) {
            app.toggle_panel_focus();
            assert_eq!(app.focused_panel, *want, "Tab");
        }
        assert_eq!(app.focused_panel, FocusedPanel::RepoList, "a full turn");
        for want in order.iter().rev() {
            app.toggle_panel_focus_back();
            assert_eq!(app.focused_panel, *want, "Shift-Tab");
        }
        app.focus_panel(FocusedPanel::Inspector);
        assert_eq!(
            app.focused_panel,
            FocusedPanel::RepoList,
            "no card to focus"
        );

        app.focus_panel(FocusedPanel::Reader);
        assert_eq!(app.focused_panel, FocusedPanel::Reader);
        app.cycle_history_view(false);
        assert_eq!(app.focused_panel, FocusedPanel::Detail, "left the view");
        app.focus_panel(FocusedPanel::Reader);
        assert_eq!(app.focused_panel, FocusedPanel::Detail, "no reader here");
        app.toggle_panel_focus();
        assert_eq!(app.focused_panel, FocusedPanel::Inspector, "Tab skips it");
    }

    /// `J`/`K` in the `notes` layout scroll the reader a line from the tree,
    /// with the inspector's fold open (the default) as well as folded
    /// (FZ-62 review F1; no card is docked since FZ-68 round 2, so the fold
    /// must not matter), and from the reader itself, focus unmoved
    /// (finding 6).
    #[test]
    fn shift_scroll_walks_tree_and_reader_in_the_notes_layout() {
        let mut app = notes_app();
        app.history_view = HistoryView::Notes;
        walked(&mut app, &["notez/a.md"]);
        assert!(!app.inspector_folded, "the fold is open");

        app.focused_panel = FocusedPanel::Detail;
        assert_eq!(app.shift_scroll_target(), Some(FocusedPanel::Reader));
        app.reader_max_scroll.set(20);
        app.scroll_next_panel(3);
        assert_eq!(app.reader_scroll, 3, "J from the tree scrolls the reader");
        assert_eq!(app.inspector_scroll, 0, "not the inspector");

        assert_eq!(app.focused_panel, FocusedPanel::Detail, "focus unmoved");

        app.focused_panel = FocusedPanel::Reader;
        assert_eq!(app.shift_scroll_target(), Some(FocusedPanel::Reader));
        app.scroll_next_panel(-1);
        assert_eq!(app.reader_scroll, 2, "J/K on the reader scroll it");
        app.inspector_folded = true;
        assert_eq!(app.shift_scroll_target(), Some(FocusedPanel::Reader));
        app.focused_panel = FocusedPanel::Detail;
        assert_eq!(app.shift_scroll_target(), Some(FocusedPanel::Reader));
    }

    /// FZ-68 round 2: on a section header in `notes` pane 2 is the digest and
    /// the inspector is docked, so it can hold focus; stepping onto the repo
    /// brings the `notes` layout back, which docks none, and focus lands on
    /// pane 2 rather than staying on a pane that is not drawn.
    #[test]
    fn leaving_a_header_in_notes_moves_focus_off_the_inspector() {
        let mut app = notes_app();
        app.history_view = HistoryView::Notes;
        app.repos[0].location = crate::model::RepoLocation::LocalOnly;
        let section = crate::digest::Section::of(&app.repos[0]);
        app.land_on(ListRow::Header(section));
        assert!(!app.notes_layout(), "on the digest");
        app.focus_panel(FocusedPanel::Inspector);
        assert_eq!(app.focused_panel, FocusedPanel::Inspector, "docked here");

        app.land_on(ListRow::Repo(0));
        assert!(app.notes_layout(), "the notes layout is back");
        assert_eq!(app.focused_panel, FocusedPanel::Detail);
    }

    /// `H`/`L` page the reader by the height its last frame drew, from the
    /// tree or the reader, focus unmoved and the tree's cursor where it was;
    /// anywhere else they do nothing (Andreas, 2026-10-29, finding 6).
    #[test]
    fn h_and_l_page_the_reader_from_the_tree() {
        let mut app = notes_app();
        app.history_view = HistoryView::Notes;
        walked(&mut app, &["notez/a.md", "notez/b.md"]);
        app.reader_max_scroll.set(100);
        app.reader_view_height.set(18);

        app.focused_panel = FocusedPanel::Detail;
        assert!(app.reader_pages());
        app.page_reader(1);
        assert_eq!(app.reader_scroll, 18, "L: one page down");
        app.page_reader(1);
        assert_eq!(app.reader_scroll, 36);
        app.page_reader(-1);
        assert_eq!(app.reader_scroll, 18, "H: one page up");
        assert_eq!(app.focused_panel, FocusedPanel::Detail, "focus unmoved");
        assert_eq!(app.detail_selected(), 0, "the tree's cursor stayed");

        app.focused_panel = FocusedPanel::Reader;
        assert!(app.reader_pages(), "from the reader too");
        app.page_reader(-5);
        assert_eq!(app.reader_scroll, 0, "clamped at the top");

        for panel in [FocusedPanel::RepoList, FocusedPanel::Inspector] {
            app.focused_panel = panel;
            assert!(!app.reader_pages(), "{panel:?}");
        }
        app.focused_panel = FocusedPanel::Detail;
        app.history_view = HistoryView::Info;
        assert!(!app.reader_pages(), "no reader outside notes");
    }

    /// The reader keeps its own offset: moving the tree's cursor does not
    /// scroll it, scrolling it does not move the cursor, and a new file
    /// starts it at the top while a re-read of the same file keeps its
    /// place.
    #[test]
    fn the_reader_scrolls_on_its_own_and_resets_on_a_new_file() {
        let mut app = notes_app();
        app.history_view = HistoryView::Notes;
        walked(&mut app, &["notez/a.md", "notez/b.md"]);
        app.claim_note_read().expect("a");
        app.reader_max_scroll.set(30);
        app.scroll_reader(12);
        assert_eq!(app.reader_scroll, 12);
        assert_eq!(app.detail_selected(), 0, "the cursor stayed");
        assert_eq!(app.inspector_scroll, 0, "the card did not scroll");

        app.invalidate_notes();
        app.claim_note_read().expect("re-read of a");
        assert_eq!(app.reader_scroll, 12, "same file keeps its place");

        app.select_commit_down();
        assert_eq!(app.reader_scroll, 12, "the cursor does not scroll it");
        app.claim_note_read().expect("b");
        assert_eq!(app.reader_scroll, 0, "a new file starts at the top");

        app.scroll_reader(-5);
        assert_eq!(app.reader_scroll, 0, "clamped at the top");
        app.scroll_reader(1000);
        assert_eq!(app.reader_scroll, 30, "clamped to what was measured");
    }

    /// FZ-59: a markdown read carries the reader's width so the body is
    /// rendered with it; a text file, or a reader that never drew, asks for
    /// no render.
    #[test]
    fn a_markdown_read_asks_for_a_render_at_the_reader_width() {
        let mut app = notes_app();
        app.history_view = HistoryView::Notes;
        walked(&mut app, &["notez/a.md", "notez/b.txt", "notez/TODO.md"]);
        assert_eq!(app.claim_note_read().unwrap().render_width, None, "undrawn");

        app.reader_width.set(48);
        app.invalidate_notes();
        let mut asked = Vec::new();
        for _ in 0..3 {
            let read = app.claim_note_read().unwrap();
            let name = read
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            asked.push((name, read.render_width));
            app.select_commit_down();
        }
        asked.sort();
        assert_eq!(
            asked,
            [
                ("TODO.md".to_string(), Some(48)),
                ("a.md".to_string(), Some(48)),
                ("b.txt".to_string(), None),
            ],
            "a TODO.md is markdown, a text file is not"
        );
    }

    /// FZ-59: a width the held render was not made for is claimed once, and
    /// again only after the reader moves on to another width; a render for
    /// a file the cursor has left is dropped.
    #[test]
    fn a_new_reader_width_claims_one_render_of_the_held_body() {
        let mut app = notes_app();
        app.history_view = HistoryView::Notes;
        walked(&mut app, &["notez/a.md", "notez/b.md"]);
        let read = app.claim_note_read().expect("a");
        assert_eq!(app.claim_note_render(), None, "body not landed");
        app.apply_note_read(read.clone(), Ok("# A".to_string()));
        assert_eq!(app.claim_note_render(), None, "the reader never drew");

        app.reader_width.set(40);
        let render = app.claim_note_render().expect("a render at 40");
        assert_eq!((render.width, render.text.as_str()), (40, "# A"));
        assert_eq!(app.claim_note_render(), None, "in flight");
        let lines = crate::ui::markdown::render_markdown(&render.text, 40);
        let rendered = RenderedNote { width: 40, lines };
        app.apply_note_rendered(&render.path, render.generation, rendered.clone());
        assert_eq!(app.note_body.as_ref().unwrap().rendered, Some(rendered));
        assert_eq!(app.claim_note_render(), None, "held at this width");

        app.reader_width.set(60);
        assert_eq!(app.claim_note_render().map(|r| r.width), Some(60));

        app.select_commit_down();
        app.claim_note_read().expect("b");
        let stale = RenderedNote {
            width: 60,
            lines: Vec::new(),
        };
        app.apply_note_rendered(&read.path, read.generation, stale);
        assert_eq!(app.note_body.as_ref().unwrap().rendered, None, "dropped");
    }

    /// FZ-59 review: a render of the same file from an older generation,
    /// landing after a sweep or a suspend asked for a re-read, is dropped:
    /// the re-read body holds no render of the old text, and the render it
    /// asked for at that width still lands.
    #[test]
    fn a_render_from_an_older_generation_of_the_same_file_is_dropped() {
        let mut app = notes_app();
        app.history_view = HistoryView::Notes;
        walked(&mut app, &["notez/a.md"]);
        app.reader_width.set(40);
        let old = app.claim_note_read().expect("a");
        assert_eq!(old.render_width, Some(40));

        app.invalidate_notes();
        let new = app.claim_note_read().expect("re-read of a");
        assert_eq!(new.path, old.path, "the same file");
        assert_ne!(new.generation, old.generation);
        let stale = RenderedNote {
            width: 40,
            lines: crate::ui::markdown::render_markdown("# old", 40),
        };
        app.apply_note_rendered(&old.path, old.generation, stale);
        let body = app.note_body.as_ref().unwrap();
        assert_eq!(body.rendered, None, "the old text's render is dropped");
        assert_eq!(body.render_asked, Some(40), "the re-read's is still owed");

        let fresh = RenderedNote {
            width: 40,
            lines: crate::ui::markdown::render_markdown("# new", 40),
        };
        app.apply_note_rendered(&new.path, new.generation, fresh.clone());
        assert_eq!(app.note_body.as_ref().unwrap().rendered, Some(fresh));
    }

    /// FZ-59: `p` flips the reader between rendered and raw for the
    /// session, in the `notes` layout only.
    #[test]
    fn the_raw_toggle_flips_for_the_session_in_the_notes_layout_only() {
        let mut app = notes_app();
        app.history_view = HistoryView::Info;
        app.toggle_reader_raw();
        assert!(!app.reader_raw, "not the notes view");
        app.history_view = HistoryView::Notes;
        walked(&mut app, &["notez/a.md", "notez/b.md"]);
        app.toggle_reader_raw();
        assert!(app.reader_raw);
        app.select_commit_down();
        assert!(app.reader_raw, "one flag, not per file");
        app.toggle_reader_raw();
        assert!(!app.reader_raw);
    }

    /// Facts land beside the body they were read with, and an answer for a
    /// file no longer highlighted is dropped, as a body is.
    #[test]
    fn note_facts_land_only_on_their_file() {
        let mut app = notes_app();
        app.history_view = HistoryView::Notes;
        walked(&mut app, &["notez/a.md", "notez/b.md"]);
        let a = app.claim_note_read().expect("a");
        assert_eq!(a.root, std::path::PathBuf::from("/notes-fixture"));
        let facts = crate::notes::NoteFacts {
            size: Some(3),
            ..Default::default()
        };
        app.select_commit_down();
        app.claim_note_read().expect("b");
        app.apply_note_facts(&a, facts.clone());
        assert_eq!(app.note_body.as_ref().and_then(|b| b.facts.clone()), None);
    }
}

/// FZ-45: the digest's `git log --since` reads are claimed once per touched
/// repo per cutoff and refs, and a read under a key the row has left is
/// dropped.
#[cfg(test)]
mod digest_log_tests {
    use super::*;
    use crate::digest::tests::{commit, fetched, repo, work_key};
    use crate::digest::{DigestWindow, LineBody, Section};

    fn app() -> App {
        let recent = chrono::Utc::now() - chrono::Duration::minutes(1);
        let repos = vec![
            fetched("alpha", recent),
            repo("bravo", RepoLocation::Primary),
            repo("charlie", RepoLocation::LocalOnly),
        ];
        let mut app = App::new(repos, String::new(), 120, 40);
        app.view = View::Detail;
        app.dataset = ["alpha", "bravo"]
            .iter()
            .map(|name| crate::github::GithubRepo {
                name: name.to_string(),
                pushed_at: recent,
                pr_count: 0,
                prs: vec![],
                ci_status: CiStatus::Unknown,
                ci_workflow: None,
                ci_duration: None,
                repo_kind: crate::model::RepoKind::Unknown,
                owner: None,
                disk_usage: None,
            })
            .collect();
        app.land_on(ListRow::Header(Section::GithubActivity));
        app
    }

    #[test]
    fn nothing_is_claimed_off_a_header() {
        let mut app = app();
        app.land_on(ListRow::Repo(0));
        assert!(app.claim_digest_logs(Local::now()).is_empty());
    }

    #[test]
    fn a_touched_fetched_clone_is_read_once_per_cutoff_and_refs() {
        let mut app = app();
        let now = Local::now();
        let first = app.claim_digest_logs(now);
        assert_eq!(
            first.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
            vec!["~/repos/alpha"],
            "only the fetched clone is read; bravo shows its push"
        );
        assert!(
            app.claim_digest_logs(now).is_empty(),
            "running: not asked twice"
        );
        let key = first[0].key.clone();
        app.apply_digest_log("~/repos/alpha", &key, vec![commit("a1", "A", "x")]);
        assert!(
            app.claim_digest_logs(now).is_empty(),
            "held: a sweep that moves no ref reads none"
        );
        let digest = app.digest(now).unwrap();
        assert!(
            matches!(&digest.lines[0].body, LineBody::Counted { commits } if commits.len() == 1)
        );
        assert!(matches!(digest.lines[1].body, LineBody::PushedOnly));

        // A sweep that moved a ref asks again; the old answer is not drawn.
        app.repos[0].work_key = Some(work_key("moved"));
        assert!(matches!(
            app.digest(now).unwrap().lines[0].body,
            LineBody::Reading
        ));
        let second = app.claim_digest_logs(now);
        assert_eq!(second.len(), 1);
        // The first read landing late is dropped.
        app.apply_digest_log("~/repos/alpha", &key, vec![commit("old", "A", "x")]);
        assert!(matches!(
            app.digest(now).unwrap().lines[0].body,
            LineBody::Reading
        ));

        // A new window is a new cutoff.
        app.apply_digest_log("~/repos/alpha", &second[0].key, vec![]);
        app.digest_window = DigestWindow::Week;
        assert_eq!(app.claim_digest_logs(now).len(), 1);
    }

    #[test]
    fn landing_on_a_header_hides_the_repo_and_leaving_restores_its_place() {
        let mut app = app();
        assert_eq!(app.on_header(), Some(Section::GithubActivity));
        assert!(app.selected_repo().is_none());
        assert!(app.selected_repo_index().is_none());
        app.history_view = HistoryView::Reflog;
        app.move_down();
        assert_eq!(app.on_header(), None);
        assert_eq!(app.selected, 0);
        assert_eq!(app.history_view, HistoryView::Reflog);
    }

    /// FZ-62 review F2: with the reader focused in `notes`, the cursor
    /// landing on a header (key, wheel, click or hover dwell, all through
    /// `land_on`) drops the reader from the screen, so focus moves to pane 2,
    /// where `j`/`k` walk the digest. Fails while focus stays on the reader.
    #[test]
    fn landing_on_a_header_takes_focus_off_the_reader() {
        let mut app = app();
        app.move_down();
        assert_eq!(app.on_header(), None);
        app.history_view = HistoryView::Notes;
        app.focus_panel(FocusedPanel::Reader);
        assert_eq!(app.focused_panel, FocusedPanel::Reader);

        app.move_up();
        assert_eq!(app.on_header(), Some(Section::GithubActivity));
        assert!(!app.notes_layout(), "no reader on a header");
        assert_eq!(app.focused_panel, FocusedPanel::Detail);
    }

    /// Review F1: someone else's push, fetched, moves only `origin/*`: HEAD
    /// is where it was and only the ref signature changes. That is a new
    /// read, and the old answer is not drawn.
    #[test]
    fn a_moved_remote_ref_alone_asks_for_a_new_read() {
        let mut app = app();
        let now = Local::now();
        let first = app.claim_digest_logs(now);
        app.apply_digest_log("~/repos/alpha", &first[0].key, vec![commit("a1", "A", "x")]);
        let mut moved = work_key("abc");
        moved.ref_signature += 1;
        app.repos[0].work_key = Some(moved);
        assert!(matches!(
            app.digest(now).unwrap().lines[0].body,
            LineBody::Reading
        ));
        let second = app.claim_digest_logs(now);
        assert_eq!(second.len(), 1, "a new read for the moved remote ref");
        assert_ne!(second[0].key, first[0].key);
    }

    /// Review F2: the round trip through a header keeps the repo pane's
    /// cursor and scroll, not only its view.
    #[test]
    fn a_header_round_trip_keeps_the_detail_cursor_and_scroll() {
        let mut app = app();
        app.land_on(ListRow::Repo(0));
        app.history_view = HistoryView::Reflog;
        app.set_detail_selected(3);
        app.set_detail_scroll(5);
        app.move_up();
        assert_eq!(app.on_header(), Some(Section::GithubActivity));
        app.move_down();
        assert_eq!(app.on_header(), None);
        assert_eq!(app.selected, 0);
        assert_eq!(app.detail_selected(), 3);
        assert_eq!(app.detail_scroll(), 5);
    }

    /// Review F2b: the compact grid draws no headers, so a stop left from
    /// the detail view is the section's first repo there.
    #[test]
    fn a_header_stop_is_its_first_repo_in_the_compact_view() {
        let mut app = app();
        assert_eq!(app.on_header(), Some(Section::GithubActivity));
        app.toggle_view();
        assert_eq!(app.view, View::Compact);
        assert_eq!(app.on_header(), None);
        assert_eq!(app.selected_repo().map(|r| r.name.as_str()), Some("alpha"));
    }

    #[test]
    fn a_search_drops_the_headers_and_the_stop() {
        let mut app = app();
        app.search_query = "alp".to_string();
        assert_eq!(app.list_rows(), vec![ListRow::Results, ListRow::Repo(0)]);
        assert_eq!(app.on_header(), None);
    }

    #[test]
    fn a_lone_section_has_no_header_to_stop_on() {
        let mut app = App::new(
            vec![
                repo("a", RepoLocation::Primary),
                repo("b", RepoLocation::Primary),
            ],
            String::new(),
            120,
            40,
        );
        app.view = View::Detail;
        assert_eq!(app.list_rows(), vec![ListRow::Repo(0), ListRow::Repo(1)]);
        app.move_up();
        assert_eq!(app.on_header(), None);
    }
}
