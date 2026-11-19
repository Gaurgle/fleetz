#[cfg(test)]
use crate::model::RepoLocation;
use crate::model::{CiStatus, RepoStatus, Summary};
use chrono::{DateTime, Local};

pub use crate::config::{DEFAULT_INSPECTOR_PCT, DEFAULT_REPO_LIST_PCT, Icons, View, initial_view};
#[cfg(test)]
pub use crate::engine::Jitter;
pub use crate::engine::{
    ApplyProgress, DataTime, DatasetSource, FetchClaims, FetchReport, FetchState, GithubFreshness,
    Outcome, Pace, RefreshPhase,
};
use crate::engine::{ClockJitter, GithubSync, PollContext};
pub(crate) use crate::engine::{RemoteKey, carrier_key};

mod changes;
mod ci;
mod contrib;
mod digest;
mod feedback;
mod notes;
mod overlays;
mod panes;
mod query;
mod selection;
mod worktrees;
mod yank;

#[cfg(test)]
pub use changes::ChangeSection;
pub use changes::{ChangesRead, DiffRead};
pub use ci::{CiBody, CiEntry, CiLogWrap, CiShown, CiState, JobFollow};
#[cfg(test)]
pub use ci::{CiJobs, CiTip};
pub use contrib::{CellSize, ContributionsMode, DEFAULT_CONTRIBUTIONS};
#[cfg(test)]
pub use digest::HOVER_DWELL;
pub use digest::{ListHits, ListRow};
pub use notes::{NoteBody, NoteRead, NoteRender, NotesWalk, RenderedNote};
pub use worktrees::{WORKTREE_LOG_COUNT, WorktreeLogRead};
pub use yank::YankFlash;
#[cfg(test)]
pub use yank::YankKind;

#[cfg(test)]
pub use overlays::SettingField;
pub use overlays::{
    Applies, BranchEntry, BranchOverlay, Overlay, SETTING_FIELDS, SettingsOverlay,
    TimeWindowOverlay, WindowField,
};
pub use panes::{INSPECTOR_PCT_MAX, INSPECTOR_PCT_MIN, REPO_LIST_PCT_MAX, REPO_LIST_PCT_MIN};
pub use query::{FILTER_TAGS, FilterKind, MatchField, parse_query, text_match};

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
    /// What the last frame measured and drew, written by the renderer
    /// through `&App` (`app::feedback::RenderFeedback`).
    pub(crate) drawn: feedback::RenderFeedback,
    /// Index of the row highlighted in each history view, whose commit the
    /// inspector shows. Reset to 0 when the selected repo changes.
    ///
    /// Per view for the same reason as the scroll offset, and because the
    /// lists have different lengths: an index valid in the reflog is
    /// meaningless against the commit list.
    pub detail_selections: [usize; HistoryView::COUNT],
    /// Scroll offset for the inspector pane (long commit bodies).
    pub inspector_scroll: u16,
    /// Body row the graph view highlights. The graph's cursor is a *row*, not
    /// a commit: its blocks run twenty rows and more, so a commit-granular
    /// cursor could only jump a whole block at a time and left the highlight
    /// nowhere while you read through one.
    pub graph_cursor: u16,
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
    /// The contribution strip: source, GitHub calendar, author filter, day
    /// icon, day cursor and pan (`contrib::ContribStrip`, FZ-122).
    pub(crate) contrib: contrib::ContribStrip,
    /// Whether the inspector and the strip are folded to an empty frame. A
    /// folded pane stays on screen and can hold focus, so its `[3]`/`[4]` tag
    /// is still there to click and its number still reaches it. Neither is
    /// ever removed outright. Only these two fold: the repo list and the
    /// detail pane are the dashboard itself.
    pub inspector_folded: bool,
    pub contributions_folded: bool,
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
    /// True while the initial load or an R reload is in flight.
    pub loading: bool,
    /// The local fetch: the sweep lock, the manual refresh's two halves, the
    /// timeout holds, the cap, the fetch claims and the keypress tally
    /// (`engine::FetchState`). The cap is set from `Config` at startup
    /// (`FetchState::set_max_bytes`); the header reads its queries.
    pub(crate) fetch: FetchState,
    /// The GitHub sync: freshness, budget, retry ladder, the poll's in-flight
    /// lock and the push shortcut, with the interval and cost they are judged
    /// against (`engine::GithubSync`). Set from `Config` at startup
    /// (`GithubSync::from_config`); the header reads it through its queries.
    pub(crate) github: GithubSync,
    /// Why the last full load failed, while it has left nothing to draw.
    ///
    /// A failed initial load used to set only a status message, which the
    /// next keypress clears. Since the repo list is derived from the GitHub
    /// dataset, the result was a completely blank dashboard whose one line of
    /// explanation vanished on the first keystroke -- indistinguishable from
    /// a fleet that genuinely has no repos in the window. Cleared by the next
    /// load that succeeds.
    pub load_error: Option<String>,
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
    /// The `ci` view's state: its data per repo, the `core` budget, the
    /// cursor's dwell and the inspector's run bodies (`app::ci::CiView`).
    pub(crate) ci: ci::CiView,
    /// `[`/`]` pressed on the focused inspector cycled pane 2's views, and
    /// no other key came since: the streak keeps cycling views on arriving
    /// in `ci` rather than stepping the run bodies there.
    pub brackets_cycling_views: bool,
    /// The yank chord and its copy flash (`yank.rs`, FZ-123).
    pub(crate) yank: yank::YankState,
    /// The `worktrees` view's logs (`worktrees.rs`, FZ-120).
    pub(crate) worktrees: worktrees::WorktreesView,
    /// The `notes` view and its reader (`notes.rs`, FZ-119).
    pub(crate) notes: notes::NotesView,
    /// The slot digest and the list's section stops (`digest.rs`, FZ-121).
    pub(crate) digest: digest::DigestView,
    /// The `changes` view (`changes.rs`, FZ-120).
    pub(crate) changes: changes::ChangesView,
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
            drawn: feedback::RenderFeedback::new(),
            detail_selections: [0; HistoryView::COUNT],
            inspector_scroll: 0,
            graph_cursor: 0,
            needs_redraw: true,
            spinner_frame: 0,
            blink_on: true,
            loading_field: None,
            view_overridden: false,
            repo_list_pct: DEFAULT_REPO_LIST_PCT,
            inspector_pct: DEFAULT_INSPECTOR_PCT,
            contrib: contrib::ContribStrip::new(),
            inspector_folded: false,
            contributions_folded: false,
            persist_pane_sizes: false,
            show_pr_detail: false,
            dragging: None,
            expand_remote_branches: false,
            loading: false,
            fetch: FetchState::new(crate::config::Config::default().fetch_cap_bytes()),
            github: GithubSync::new(
                crate::config::Config::default().github_poll_interval as i64,
                crate::github::poll_cost(crate::config::Config::default().repo_limit),
                Box::new(ClockJitter::new()),
            ),
            load_error: None,
            dataset: Vec::new(),
            github_meta: crate::github::FetchMeta::default(),
            not_in_last_poll: std::collections::HashSet::new(),
            account_rebuild_due: false,
            window: None,
            dirty_mode: false,
            apply_progress: None,
            ci: ci::CiView::new(),
            brackets_cycling_views: false,
            yank: yank::YankState::new(),
            worktrees: worktrees::WorktreesView::new(),
            notes: notes::NotesView::new(),
            digest: digest::DigestView::new(),
            changes: changes::ChangesView::new(),
        }
    }

    /// A sweep finished: close its books, stamp the refresh time and have
    /// the notes walked again. The `SweepDone` arm of the event loop calls it.
    pub fn sweep_landed(&mut self, at: chrono::DateTime<chrono::Local>) {
        self.fetch.sweep_landed();
        self.last_refreshed = Some(at);
        self.invalidate_notes();
        self.invalidate_changes();
        self.worktrees.generation += 1;
        self.needs_redraw = true;
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

    /// Drop claims with no sign of life for longer than `limit` at `now`, the
    /// safety net for a fetch whose result never landed
    /// (`SWEEP_STUCK_AFTER`). A row whose claim went this way stops showing
    /// the fetch's progress: nothing will land to clear it.
    pub fn expire_fetches_in_flight(
        &mut self,
        limit: std::time::Duration,
        now: std::time::Instant,
    ) {
        self.fetch.expire_claims(limit, now);
        for repo in &mut self.repos {
            if repo.fetch.progress.is_some()
                && !repo
                    .path
                    .as_deref()
                    .is_some_and(|p| self.fetch.claims().contains_key(p))
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
        self.fetch.touch(path, now);
        if let Some(repo) = self
            .repos
            .iter_mut()
            .find(|r| r.path.as_deref() == Some(path))
        {
            repo.fetch.progress = Some(progress);
            self.needs_redraw = true;
        }
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

    /// Strip any `#tag` filter words from the search query, keeping free
    /// text intact.
    pub fn clear_filters(&mut self) {
        self.search_query = parse_query(&self.search_query).text;
        self.selected = 0;
        self.digest.list_stop = None;
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
        self.digest.list_stop = None;
    }

    /// Clear group filter (show all).
    pub fn clear_group(&mut self) {
        self.active_group = None;
        self.selected = 0;
        self.digest.list_stop = None;
    }

    /// Label for the active group, if any.
    pub fn active_group_label(&self) -> Option<&str> {
        self.active_group
            .and_then(|i| self.group_names.get(i))
            .map(|s| s.as_str())
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
        self.digest.list_stop = None;
    }

    pub fn search_push_char(&mut self, c: char) {
        self.search_query.push(c);
        self.selected = 0; // reset selection when query changes
        self.digest.list_stop = None;
    }

    pub fn search_pop_char(&mut self) {
        self.search_query.pop();
        self.selected = 0;
        self.digest.list_stop = None;
    }

    pub fn end_search(&mut self) {
        self.search_active = false;
    }

    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.search_active = false;
        self.selected = 0;
        self.digest.list_stop = None;
    }

    pub fn set_status(&mut self, text: String, is_error: bool) {
        self.status_message = Some(StatusMessage {
            text,
            is_error,
            created: std::time::Instant::now(),
        });
    }

    /// What the GitHub poll decisions read off the front end (D2).
    pub(crate) fn poll_context(&self) -> PollContext {
        // A row the last poll did not return holds a `Pending` frozen at an
        // earlier refresh, which says nothing about CI now (design 4.7).
        let any_pending = self
            .repos
            .iter()
            .any(|r| r.ci_status == CiStatus::Pending && !self.is_not_in_last_poll(r));
        PollContext {
            watch: self.watch_mode,
            dirty: self.dirty_mode,
            loading: self.loading,
            any_pending,
        }
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
    pub fn summary(&self) -> Summary {
        Summary::from_refs(self.visible_repos().into_iter().map(|(_, r)| r))
    }
}

#[cfg(test)]
pub(crate) mod tests;
