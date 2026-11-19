use crate::model::{CiStatus, RepoLocation, RepoStatus, Summary};
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
mod query;
mod worktrees;

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

#[cfg(test)]
pub use overlays::SettingField;
pub use overlays::{
    Applies, BranchEntry, BranchOverlay, Overlay, SETTING_FIELDS, SettingsOverlay,
    TimeWindowOverlay, WindowField,
};
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
    /// A `y` was pressed and the next key finishes the yank chord (FZ-74):
    /// `y` copies the thing under the cursor, `p` its path, anything else
    /// copies nothing. Cleared by the next key whatever it is.
    pub pending_yank: bool,
    /// The row a successful `yy` or `yp` copied from flashes once (FZ-81).
    pub yank_flash: YankFlash,
    /// The `worktrees` view's logs (`worktrees.rs`, FZ-120).
    pub(crate) worktrees: worktrees::WorktreesView,
    /// The `notes` view and its reader (`notes.rs`, FZ-119).
    pub(crate) notes: notes::NotesView,
    /// The slot digest and the list's section stops (`digest.rs`, FZ-121).
    pub(crate) digest: digest::DigestView,
    /// The `changes` view (`changes.rs`, FZ-120).
    pub(crate) changes: changes::ChangesView,
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
            pending_yank: false,
            yank_flash: YankFlash::default(),
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
            || self.contrib.cursor.is_some()
        {
            return;
        }
        self.contrib.cursor = Some(self.contrib_last_slot(today));
        // Out of range on purpose, as in `open_contributions`: the renderer
        // clamps it to "newest week at the right edge", which holds today.
        self.drawn.contrib_scroll.set(crate::contributions::WEEKS);
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
        let area = self.drawn.main_area_width.get();
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
        let max = self.drawn.inspector_max_scroll.get() as i32;
        if self.inspects_ci_run() {
            self.sync_ci_inspector(std::time::Instant::now());
            let from = i32::from(self.ci_body_scroll());
            self.ci.body_scrolls[self.ci.body.index()] = (from + delta).clamp(0, max) as u16;
            if self.ci.body == CiBody::Jobs {
                self.drawn.ci_job_follow.set(JobFollow::Free);
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
        let max = self.drawn.detail_max_scroll.get() as i32;
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
        let height = self.drawn.detail_view_height.get();
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
            - if scroll < self.drawn.detail_max_scroll.get() {
                pad
            } else {
                0
            };

        let tops = self.drawn.detail_row_tops.borrow();
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
        self.drawn.detail_max_scroll.get() + self.drawn.detail_view_height.get()
    }

    /// Nudge the scroll offset so the cursor sits at least `SCROLLOFF`
    /// rows inside the pane, and otherwise leave it exactly where it is --
    /// that "otherwise" is what makes the highlight, not the text, move
    /// through the middle of the pane.
    fn keep_graph_cursor_in_view(&mut self) {
        let height = self.drawn.detail_view_height.get();
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
            .min(self.drawn.detail_max_scroll.get());
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
        let height = self.drawn.detail_view_height.get();
        if height == 0 {
            return;
        }
        let pad = scrolloff_for(height);
        let scroll = self.detail_scroll();
        let cursor = self.graph_cursor;
        if cursor < scroll + pad || cursor > scroll + height - 1 - pad {
            let top = cursor
                .saturating_sub(pad)
                .min(self.drawn.detail_max_scroll.get());
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
            .drawn
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
    /// Scroll offset and selection for the *active* history view. Every
    /// caller wants the current one; the arrays exist so switching views does
    /// not lose where you were in the one you left.
    pub fn detail_scroll(&self) -> u16 {
        self.drawn.detail_scrolls[self.history_view.index()].get()
    }

    /// Takes `&self` because the renderer sets it too -- see the field.
    pub fn set_detail_scroll(&self, value: u16) {
        self.drawn.detail_scrolls[self.history_view.index()].set(value);
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
            let top = self.drawn.detail_row_tops.borrow().get(value).copied();
            if let Some(top) = top {
                self.graph_cursor = top;
                self.show_graph_cursor_from_top();
                return;
            }
        }
        // Any cursor move asks the pane to follow. Scrolling does not, which
        // is what keeps the two from fighting each other to a standstill.
        self.drawn.follow_selection.set(true);
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

#[cfg(test)]
pub(crate) mod tests;
