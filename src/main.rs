mod app;
mod cache;
mod ci;
mod cli;
mod clipboard;
mod config;
mod contributions;
mod digest;
mod discovery;
mod fuzzy;
mod git;
mod github;
mod graph;
mod model;
mod notes;
mod report;
mod time;
mod ui;
mod window_state;

use app::{App, ApplyProgress, BranchEntry, BranchOverlay, FocusedPanel, Overlay};
use clap::Parser;
use cli::{Args, Mode};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use model::{RepoLocation, RepoStatus};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

/// How many commits/reflog entries to show in the detail pane's history
/// sections. Kept modest -- the pane has no scrolling, and these share it
/// with branches, CI, PRs, and stashes.
/// Rows fetched for the `commits`, `reflog` and `graph` views, which are
/// meant to be complete rather than a glance.
///
/// Affordable because these probes are already gated on `WorkKey`: a repo
/// whose refs, HEAD, index, shallow boundary and HEAD and stash reflogs have
/// not moved is skipped entirely, so the deeper log is paid once per actual
/// change rather than once per sweep.
const HISTORY_COUNT: u32 = 200;

/// How long a footer status message ("Refreshed", "Watch off", …) stays before
/// it auto-dismisses back to the summary.
const STATUS_TTL: std::time::Duration = std::time::Duration::from_millis(1500);

/// Browser-opener binary: macOS ships `open`, everything else gets xdg-open.
#[cfg(target_os = "macos")]
const OPENER: &str = "open";
#[cfg(not(target_os = "macos"))]
const OPENER: &str = "xdg-open";

/// Parallel git-status probes: enough to hide subprocess latency, capped so a
/// big fleet does not spawn hundreds of processes at once.
fn probe_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 12)
}

/// Network fetches are remote-bound, not CPU-bound: keep it polite.
const FETCH_CONCURRENCY: usize = 4;

/// How often the event loop ticks: fast enough for a smooth spinner and a
/// responsive status-message TTL, slow enough to stay invisible in top.
///
/// The spinner advances one frame per tick, so this *is* the spin rate. Every
/// other cadence derived from it (`BLINK_TICKS`, `SLOW_REPAINT_TICKS`) is
/// expressed in ticks and rescaled alongside, so changing this speeds up the
/// spinner without dragging the CI blink or the repaint budget along with it.
const TICK_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// Ticks between repaints for things that move on their own without a spinner
/// driving every frame: the swept-ago clock, the CI blink, the watch pulse.
///
/// Tied to `BLINK_TICKS` rather than picked independently, and that matters:
/// anything that repaints on an *even multiple* of the blink period draws the
/// blink in the same phase every single time, so it never appears to blink at
/// all. This was previously 10 against a blink of 5, which is exactly that bug.
pub const SLOW_REPAINT_TICKS: usize = app::BLINK_TICKS;

/// How long the input thread waits for a key before looping. Bounds how long
/// a suspend (lazygit, $EDITOR) has to wait for the input gate.
const INPUT_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// How long the input thread sleeps between checks while input is paused.
const INPUT_PAUSE_BACKOFF: std::time::Duration = std::time::Duration::from_millis(20);

/// How many repaints may be held in a row while input is still queued.
///
/// Coalescing a burst is the point, but a source that never stops arriving
/// would then never paint at all, so the hold is bounded: past this many the
/// loop draws whatever it has and starts counting again. Sized so the bound is
/// effectively unreachable by a human gesture -- a trackpad flick is tens of
/// events, not hundreds -- and only trips on something pathological.
const MAX_COALESCED_FRAMES: u32 = 64;

/// Whether a pending repaint should be drawn now or held for the input that is
/// already queued behind it. See the paint at the foot of `run_event_loop`.
///
/// `animating` overrides the hold. Coalescing exists for *input*, where a burst
/// of wheel events all describe the same final state and only the last one
/// matters. An animation frame is the opposite: it is a deadline, and the frame
/// held is a frame not shown. Holding them behind queued background work is
/// what made the intro wave (since removed) stutter on a release build and not
/// on a debug one -- release finishes the repo probes fast enough to flood the
/// channel during an animation, where debug spreads the same work out past the
/// end of it. The loading field is the animation this protects now.
fn should_paint(queue_empty: bool, held: u32, animating: bool) -> bool {
    animating || queue_empty || held >= MAX_COALESCED_FRAMES
}

/// Everything the event loop reacts to. Terminal input, the periodic tick,
/// and the results of background work all arrive on one channel, so the loop
/// never blocks on any single source.
pub enum AppEvent {
    Input(Event),
    Tick,
    Loaded(Result<Box<LoadResult>, LoadError>),
    RepoUpdated {
        path: String,
        status: Box<RepoStatus>,
        /// How the `git fetch` before this rebuild ended, or `NotAttempted`
        /// when the sweep did not fetch. Requested is not successful.
        fetch: git::FetchOutcome,
        /// The token of the fetch claim this sweep held for the path
        /// (`app::FetchClaims`), `None` when it fetched nothing. Its landing
        /// ends that claim and no other.
        claim: Option<u64>,
    },
    /// A fetch on the row at `path` made progress (FZ-35a). Sent from the
    /// fetch task at most twice a second per fetch (`ProgressThrottle`) and
    /// always before that fetch's `RepoUpdated`, which clears it.
    FetchProgress {
        path: String,
        progress: git::FetchProgress,
    },
    /// A background poll's answer. `attempt` is the id it was started with
    /// (`GithubFreshness::begin_attempt`), so an answer that arrives after a
    /// newer one was applied can be told apart and discarded.
    Github {
        attempt: u64,
        result: Result<Box<github::GithubFetch>, github::FetchError>,
    },
    Progress(ApplyProgress),
    /// One frame of the noise field shown while the first load runs. On its
    /// own timer: the loop's 100ms tick is the spinner rate and far too coarse
    /// for an animation. See `ui::intro`.
    LoadingFrame,
    SweepDone {
        at: chrono::DateTime<chrono::Local>,
    },
    /// A CI poll's answer for one repo (FZ-30b), applied by `App::apply_ci`.
    /// It touches only that repo's CI state and the `core` budget.
    Ci(Box<ci::CiAnswer>),
    /// An inspector fetch's answer for the highlighted run (FZ-30f): its
    /// jobs, a job's whole log or the workflow file, applied by
    /// `App::apply_ci_detail`.
    CiDetail(Box<ci::DetailAnswer>),
    /// A `notes` view walk of one repo (FZ-53), applied by
    /// `App::apply_notes_walk`.
    NotesWalked {
        walk: app::NotesWalk,
        result: Result<notes::Notes, String>,
    },
    /// The highlighted note's body and its metadata for the card, applied by
    /// `App::apply_note_read` and `App::apply_note_facts`.
    NoteRead {
        read: app::NoteRead,
        text: Result<String, String>,
        facts: notes::NoteFacts,
    },
    /// git's last commit for that note (FZ-62), sent after the body so the
    /// reader never waits on git; applied by `App::apply_note_commit`.
    NoteCommit {
        read: app::NoteRead,
        last: notes::LastCommit,
    },
    /// A slot digest's `git log --since` read for the row at `path` (FZ-45),
    /// applied by `App::apply_digest_log` under the key it was asked with.
    DigestLog {
        path: String,
        key: digest::LogKey,
        commits: Vec<model::Commit>,
    },
}

/// Why a load ran, which decides what the loop does with the result.
pub enum LoadKind {
    /// First load at startup: no status message, nothing to announce.
    Initial,
    /// `R`: full rediscover, announces "Reloaded", or that it fell back to
    /// the cache.
    Reload,
    /// Time-window overlay: pure re-filter, persists the chosen window.
    WindowApply(time::TimeWindow),
    /// `R` under a known rate limit: rediscovered and re-swept from the
    /// dataset already in memory, with no request (A6). Lands as
    /// `DatasetSource::Local`, so `GithubFreshness` is untouched, and says
    /// when GitHub resumes instead of `Reloaded`.
    FromMemory,
}

pub struct LoadResult {
    pub kind: LoadKind,
    pub repos: Vec<RepoStatus>,
    pub header: String,
    /// None for WindowApply (dataset unchanged by a pure re-filter).
    pub dataset: Option<Vec<github::GithubRepo>>,
    /// The contribution calendar the same fetch returned. None both for a
    /// WindowApply and for a cached payload predating the field, so the
    /// consumer must not treat None as "the year is empty".
    pub github_calendar: Option<Box<contributions::Calendar>>,
    /// Where `dataset` came from. Startup serves any cached payload however
    /// old, so this decides whether the re-poll clock may start here or the
    /// first poll is due immediately, and what the header says about it.
    pub source: app::DatasetSource,
    /// The status line for a clone that claimed no row because its folder
    /// name names repos of several owners (D4 in
    /// `docs/repository-identity-design.md`), naming the first such clone by
    /// path. None when discovery found no ambiguity.
    pub ambiguity: Option<String>,
    /// What the fetch behind `dataset` learned: its coverage and account.
    /// None for a load that fetched nothing (a window apply, `-d`), which
    /// keeps the one `App` holds (C3 in `docs/coverage-cache-design.md`).
    pub meta: Option<github::FetchMeta>,
}

/// What an assembly says besides its rows: the header, and the ambiguity
/// status line that `LoadResult::ambiguity` carries to the app.
struct ViewNotes {
    header: String,
    ambiguity: Option<String>,
}

/// Why a load produced nothing to show.
#[derive(Debug)]
pub struct LoadError {
    pub message: String,
    /// The GitHub attempt behind it, when that is what failed. A failed
    /// attempt still advances the retry clock even though nothing landed.
    pub github_failure: Option<github::Failure>,
    /// The budget reading the failed attempt's answer carried, if any, so a
    /// launch refused by a rate limit learns its reset.
    /// Boxed so the error stays small enough to return by value (clippy's
    /// `result_large_err`), as `FetchError.meta` is.
    pub budget: Option<Box<github::BudgetReading>>,
}

impl From<String> for LoadError {
    fn from(message: String) -> Self {
        Self {
            message,
            github_failure: None,
            budget: None,
        }
    }
}

/// True while a child program (lazygit, $EDITOR, a shell) owns the terminal.
/// The input thread backs off instead of contending for the gate.
static INPUT_PAUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Held by the input thread around each poll+read pair, and by the suspend
/// helpers for a whole child run. Without it the reader thread would race the
/// child for keystrokes and swallow them.
static INPUT_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Stop reading terminal input, run `body` with the terminal to itself, then
/// resume reading.
fn with_input_paused<T>(body: impl FnOnce() -> T) -> T {
    use std::sync::atomic::Ordering;
    INPUT_PAUSED.store(true, Ordering::SeqCst);
    let guard = INPUT_GATE.lock().unwrap_or_else(|e| e.into_inner());
    let out = body();
    drop(guard);
    INPUT_PAUSED.store(false, Ordering::SeqCst);
    out
}

/// Read terminal input on a dedicated OS thread: crossterm's read is blocking
/// and must never park a runtime worker. Polls rather than blocking outright
/// so `with_input_paused` can take the gate promptly.
fn spawn_input_thread(tx: tokio::sync::mpsc::UnboundedSender<AppEvent>) {
    use std::sync::atomic::Ordering;
    std::thread::spawn(move || {
        loop {
            if INPUT_PAUSED.load(Ordering::SeqCst) {
                std::thread::sleep(INPUT_PAUSE_BACKOFF);
                continue;
            }
            let event = {
                let _gate = INPUT_GATE.lock().unwrap_or_else(|e| e.into_inner());
                match event::poll(INPUT_POLL) {
                    Ok(true) => match event::read() {
                        Ok(ev) => Some(ev),
                        Err(_) => break,
                    },
                    Ok(false) => None,
                    Err(_) => break,
                }
            };
            if let Some(ev) = event
                && tx.send(AppEvent::Input(ev)).is_err()
            {
                break; // loop ended, channel closed
            }
        }
    });
}

/// Shimmer the loading field until `running` is cleared, which happens when the
/// first load lands or a key is pressed (`dismiss_loading_field`).
fn spawn_loading_task(
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(ui::intro::FRAME_INTERVAL);
        // A missed frame is gone, not owed. The default `Burst` fires every
        // tick a stalled loop slept through, back to back, which floods the
        // channel with frames that all paint nearly the same thing -- progress
        // comes from the clock, not from a frame counter.
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        while running.load(std::sync::atomic::Ordering::Relaxed) {
            interval.tick().await;
            if tx.send(AppEvent::LoadingFrame).is_err() {
                break;
            }
        }
    });
}

/// One frame of the loading field: ask for a repaint while the field is up.
///
/// It adds to `needs_redraw` and must never overwrite it. A frame can be
/// queued behind `Loaded`, whose paint the loop then holds because the queue
/// is not empty; overwriting the flag with false here dropped that held paint
/// and left the loading field on screen after the load had landed.
fn on_loading_frame(app: &mut App) {
    app.needs_redraw |= app.loading_field.is_some();
}

/// Whether a launch puts the loading field up: `intro` in `config.toml`
/// switches it, and it only ever covers a load that is still running.
fn shows_loading_field(intro_enabled: bool, loading: bool) -> bool {
    intro_enabled && loading
}

/// Take the loading field down and stop its frame timer. Called when the first
/// load lands and on any keypress, so the next paint is the finished screen,
/// with nothing in between.
fn dismiss_loading_field(app: &mut App, running: &std::sync::atomic::AtomicBool) {
    app.loading_field = None;
    running.store(false, std::sync::atomic::Ordering::Relaxed);
}

/// Paint one frame: the loading field while it is up, otherwise the dashboard.
/// The field composes the whole buffer from its current area on every frame,
/// so a resize while it is up never paints a stale layout.
fn draw_screen(frame: &mut ratatui::Frame, app: &App) {
    match app.loading_field.as_ref() {
        Some(field) => field.paint(frame.buffer_mut()),
        None => ui::render(frame, app),
    }
}

/// Drive the clock, the spinner, the status-message TTL, and (later) the
/// watch cadence, independently of whether the user is typing.
fn spawn_tick_task(tx: tokio::sync::mpsc::UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(TICK_INTERVAL);
        loop {
            interval.tick().await;
            if tx.send(AppEvent::Tick).is_err() {
                break;
            }
        }
    });
}

/// Format a path for display, using `~/` shorthand when it's under the home dir.
/// Unlike a bare `~{suffix}` concatenation, this always produces a string
/// `shellexpand::tilde` can correctly re-expand back to the original path.
fn tilde_display(path: &Path) -> String {
    let home = dirs::home_dir().unwrap_or_default();
    match path.strip_prefix(&home) {
        Ok(rel) if !rel.as_os_str().is_empty() => format!("~/{}", rel.display()),
        _ => path.display().to_string(),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    // Print mode returns here, before every terminal call below (the size
    // query, raw mode, the alternate screen, the input thread): it writes
    // text to stdout and exits (D1, pinned by `report::tests`).
    if args.print {
        let mut stdout = io::stdout().lock();
        let status = report::print(&args, &mut stdout).await?;
        if status != 0 {
            std::process::exit(status);
        }
        return Ok(());
    }
    let config = config::Config::load();
    let repo_dir = std::env::var("REPO_CHECK_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| config.resolved_repo_dir());

    let mode = args.mode();
    // "Remember last choice": only a plain launch (no explicit time-mode
    // flags) adopts the saved window; flags are one-off overrides that
    // neither read nor write it.
    let initial_window = if mode == Mode::Auto && args.before.is_none() {
        window_state::WindowState::new().load()
    } else {
        None
    };
    // Empty app first: the terminal comes up before any network or
    // filesystem work, and the initial load fills it in from the background.
    let (term_width, term_height) = crossterm::terminal::size().unwrap_or((80, 24));
    let mut app = App::new(Vec::new(), String::new(), term_width, term_height);
    // Config sets the default; the flags are one-session overrides. `w` still
    // toggles it live, and that choice is not persisted.
    app.watch_mode = resolve_watch(args.watch, args.no_watch, config.watch);
    app.refresh_interval = args.interval.unwrap_or(config.refresh_interval);
    app.window = initial_window;
    app.dirty_mode = mode == Mode::Dirty;
    app.work_hours = (config.work_start, config.work_end);
    app.loading = true;
    // The strip's source, or "off" to start it folded, is persisted, so a
    // dashboard set up once comes back that way. `c` and `4` still change it
    // for the session. Open on github when the key is absent. A dashboard
    // whose headline feature needs a keypress to appear reads as a feature
    // that is broken.
    app.apply_contributions_setting(app::ContributionsMode::from_config(
        config
            .contributions
            .as_deref()
            .unwrap_or(app::DEFAULT_CONTRIBUTIONS),
    ));
    app.contrib_author = config.contributions_author.clone();
    app.github_poll_secs = configured_poll_secs(&config);
    app.github_poll_cost = github::poll_cost(config.repo_limit);
    app.fetch_max_bytes = config.fetch_cap_bytes();
    app.contrib_cell = config
        .contributions_cell_size
        .as_deref()
        .map(app::CellSize::from_config)
        .unwrap_or_default();
    ui::badges::set_active_icons(
        config
            .icons
            .as_deref()
            .map(app::Icons::from_config)
            .unwrap_or_default(),
    );
    app.show_pr_detail = args.prs;
    // Pane sizes start at their defaults every launch. Only an explicit
    // persist_pane_sizes opt-in restores the ones saved on last exit.
    app.persist_pane_sizes = config.persist_pane_sizes;
    if config.persist_pane_sizes {
        app.repo_list_pct = config
            .repo_list_pct
            .clamp(app::REPO_LIST_PCT_MIN, app::REPO_LIST_PCT_MAX);
        app.inspector_pct = config
            .inspector_pct
            .clamp(app::INSPECTOR_PCT_MIN, app::INSPECTOR_PCT_MAX);
    }

    // Set up groups from config
    if !config.groups.is_empty() {
        app.group_names = config.groups.keys().cloned().collect();
        app.group_names.sort();
    }

    // Enter TUI
    // Restore the terminal before a panic prints, or the message lands in raw
    // mode on the alternate screen: every newline is a bare line feed, so it
    // staircases off the right edge and is then wiped when the screen is
    // dropped. The bug is legible, the report of it was not.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = crossterm::execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
        default_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
    spawn_input_thread(tx.clone());
    spawn_tick_task(tx.clone());
    // Temporary cache files a crash left between write and rename. Here and
    // only here: `ApiCache::new` runs on every poll and in tests.
    cache::remove_abandoned_temp_files();
    spawn_load(
        &tx,
        &args,
        &config,
        &repo_dir,
        initial_window,
        LoadKind::Initial,
        false,
        &app.fetches_in_flight,
        HashSet::new(),
    );

    let result = run_event_loop(&mut terminal, &mut app, &args, &config, &repo_dir, tx, rx).await;

    // Restore terminal
    disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    // Pane sizes are saved on the way out, not on every keypress: resizing is
    // a held-key gesture, and writing the config on each step would rewrite
    // the file a dozen times per drag. A failed write is reported but never
    // masks the event loop's own result.
    if app.persist_pane_sizes
        && let Err(e) = config::save_settings(
            &config::settings_path(),
            &[
                (
                    "repo_list_pct",
                    config::SettingValue::Int(app.repo_list_pct as i64),
                ),
                (
                    "inspector_pct",
                    config::SettingValue::Int(app.inspector_pct as i64),
                ),
            ],
        )
    {
        eprintln!("fleetz: could not save pane sizes: {e}");
    }

    result.map_err(|e| e.into())
}

/// Assign each repo to the config-defined groups whose member list names it.
/// Clears first so it's safe to re-run after a reload replaces the list.
fn apply_groups(repos: &mut [RepoStatus], config: &config::Config) {
    for repo in repos.iter_mut() {
        repo.groups.clear();
        for (group_name, members) in &config.groups {
            if members.iter().any(|m| m.eq_ignore_ascii_case(&repo.name)) {
                repo.groups.push(group_name.clone());
            }
        }
    }
}

/// Run the full `load_repos` (GitHub fetch + discovery + git status) on a
/// background task and deliver the result as `AppEvent::Loaded`, so the UI
/// keeps painting and accepting keys throughout. `bypass_cache` is what makes
/// `R` see a just-pushed repo. Not for window applies: those are a pure
/// re-filter and go through `spawn_window_apply`.
#[allow(clippy::too_many_arguments)] // one call spine; a params struct would only rename the problem
fn spawn_load(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    window: Option<time::TimeWindow>,
    kind: LoadKind,
    bypass_cache: bool,
    fetch_claims: &app::FetchClaims,
    size_held: HashSet<String>,
) {
    debug_assert!(
        !matches!(kind, LoadKind::WindowApply(_) | LoadKind::FromMemory),
        "window applies and rebuilds from memory fetch nothing"
    );
    let tx = tx.clone();
    let args = args.clone();
    let config = config.clone();
    let repo_dir = repo_dir.to_path_buf();
    let fetch_claims = fetch_claims.clone();
    tokio::spawn(async move {
        let mode = args.mode();
        let result = load_repos(
            &args,
            &config,
            &repo_dir,
            &mode,
            window,
            bypass_cache,
            &fetch_claims,
            &size_held,
        )
        .await
        .map(|(repos, notes, dataset, calendar, source, meta)| {
            Box::new(LoadResult {
                kind,
                repos,
                header: notes.header,
                dataset: Some(dataset),
                github_calendar: calendar,
                source,
                ambiguity: notes.ambiguity,
                meta,
            })
        });
        let _ = tx.send(AppEvent::Loaded(result));
    });
}

/// Re-filter the already-fetched dataset against a newly chosen time window,
/// on a background task. No git fetch and no GitHub call, so this is cheap;
/// it still reports statting progress so the footer gauge animates.
fn spawn_window_apply(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    app: &App,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    w: time::TimeWindow,
) {
    let tx = tx.clone();
    let dataset = app.dataset.clone();
    // The note is drawn from the coverage of the dataset being re-filtered.
    let coverage = app.github_meta.coverage.clone();
    let args = args.clone();
    let config = config.clone();
    let repo_dir = repo_dir.to_path_buf();
    let fetch_claims = app.fetches_in_flight.clone();
    tokio::spawn(async move {
        let cutoff = w.cutoff(config.work_start, config.work_end);
        let header = w.label(config.work_start, config.work_end);
        let result = assemble_view(
            &dataset,
            coverage.as_ref(),
            Some(cutoff),
            header,
            &args,
            &config,
            &repo_dir,
            false,
            Some(&tx),
            &fetch_claims,
            &HashSet::new(),
        )
        .await
        .map(|(repos, notes)| Box::new(window_apply_result(w, repos, notes)))
        .map_err(LoadError::from);
        let _ = tx.send(AppEvent::Loaded(result));
    });
}

/// Rediscover and re-sweep over the dataset already in memory instead of
/// asking GitHub. On a background task like any load. `R` under a known rate
/// limit (A6) passes `git_fetch` true, for the git fetches `R` has; the
/// account rebuild (design 4.7) passes false.
fn spawn_memory_rebuild(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    app: &App,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    git_fetch: bool,
) {
    let tx = tx.clone();
    let dataset = app.dataset.clone();
    let coverage = app.github_meta.coverage.clone();
    let window = app.window;
    let args = args.clone();
    let config = config.clone();
    let repo_dir = repo_dir.to_path_buf();
    let fetch_claims = app.fetches_in_flight.clone();
    let size_held = size_held_rows(&app.repos);
    tokio::spawn(async move {
        let mode = args.mode();
        let result = match resolve_cutoff(window, &mode, &config) {
            Ok((cutoff, header)) => assemble_view(
                &dataset,
                coverage.as_ref(),
                cutoff,
                header,
                &args,
                &config,
                &repo_dir,
                git_fetch,
                None,
                &fetch_claims,
                &size_held,
            )
            .await
            .map(|(repos, notes)| Box::new(memory_rebuild_result(repos, notes)))
            .map_err(LoadError::from),
            Err(e) => Err(LoadError::from(e)),
        };
        let _ = tx.send(AppEvent::Loaded(result));
    });
}

/// What a rebuild from memory delivers: rows and notes, nothing fetched from
/// GitHub, so the dataset, calendar and coverage in the app stay as they are.
fn memory_rebuild_result(repos: Vec<RepoStatus>, notes: ViewNotes) -> LoadResult {
    LoadResult {
        kind: LoadKind::FromMemory,
        repos,
        header: notes.header,
        dataset: None,
        github_calendar: None,
        // No attempt: data time, last attempt and outcome stay what the last
        // real attempt left (amendment 5).
        source: app::DatasetSource::Local,
        ambiguity: notes.ambiguity,
        meta: None,
    }
}

/// What a window apply delivers: rows and notes, and nothing fetched.
fn window_apply_result(
    w: time::TimeWindow,
    repos: Vec<RepoStatus>,
    notes: ViewNotes,
) -> LoadResult {
    LoadResult {
        kind: LoadKind::WindowApply(w),
        repos,
        header: notes.header,
        dataset: None,
        github_calendar: None,
        // A window apply re-filters what is already in memory and
        // fetches nothing, so it must not touch the poll clock either
        // way. `dataset: None` already guards that.
        source: app::DatasetSource::Local,
        // Discovery runs again, so the ambiguity rides along like any
        // load's.
        ambiguity: notes.ambiguity,
        // Nothing was fetched: the app keeps the dataset's coverage, which
        // the header note above was drawn from.
        meta: None,
    }
}

/// Swap a finished load into the app, preserving UI state (selection is
/// clamped, groups re-applied), and say what happened.
fn apply_load_result(
    app: &mut App,
    config: &config::Config,
    result: Result<Box<LoadResult>, LoadError>,
) {
    apply_load_result_saving_to(app, config, result, &window_state::WindowState::new());
}

/// `apply_load_result` with the store a window apply persists its window to
/// passed in, so a test can land a window apply without writing the user's
/// saved window.
fn apply_load_result_saving_to(
    app: &mut App,
    config: &config::Config,
    result: Result<Box<LoadResult>, LoadError>,
    window_store: &window_state::WindowState,
) {
    let now = chrono::Local::now();
    app.apply_progress = None;
    app.loading = false;
    match result {
        Ok(lr) => {
            app.load_error = None;
            // A fetch the load ran (launch, `R`) counts toward the timeout
            // hold like a sweep's: only such a row has an attempt.
            for repo in &lr.repos {
                if let (Some(path), Some(at)) = (repo.path.as_deref(), repo.fetch.last_attempt) {
                    app.fetch_backoff.record(path, &repo.fetch.outcome, at);
                }
            }
            app.repos = carry_fetch_facts(&app.repos, lr.repos);
            // Every row now comes from one dataset, so none is "not in" it
            // (design 4.7): a launch, `R`, a window apply, a rebuild from
            // memory and an account rebuild alike.
            app.not_in_last_poll.clear();
            // A load that fetched rebuilt the rows from that fetch's account,
            // which a pending account rebuild would only repeat; a later poll
            // on another account asks again.
            if lr.meta.is_some() {
                app.account_rebuild_due = false;
            }
            apply_groups(&mut app.repos, config);
            app.header = lr.header;
            if let Some(ds) = lr.dataset {
                let reading = lr.meta.as_ref().and_then(|m| m.budget.as_ref());
                record_load_attempt(app, &lr.source, reading, now);
                app.dataset = ds;
                // Only an attempt may start the re-poll clock. Startup serves
                // any cached payload however old (`CacheMode::AnyCached`), and
                // starting the clock from that left the first real poll a full
                // `github_poll_interval` away: every launch opened on whatever
                // the last session had left on disk and sat there for thirty
                // seconds, with nothing on screen to say the numbers were not
                // current. A cache read leaves it unset, so the poll is due on
                // the first tick instead.
                app.github_freshness.loaded(&lr.source, now);
            }
            // The rows are new and so may be the dataset: the pre-check
            // reads both (FZ-35b).
            assess_size_holds(&mut app.repos, &app.dataset, app.fetch_max_bytes);
            // Only a load that fetched carries one; a window apply keeps the
            // coverage its dataset came with.
            if let Some(meta) = lr.meta {
                app.github_meta = meta;
            }
            // The calendar rides the same fetch as the dataset. Dropping it
            // here left the strip blank until the first background poll
            // landed, minutes later, which read as the feature being broken.
            // A WindowApply carries None and must not blank a good calendar.
            if lr.github_calendar.is_some() {
                app.github_calendar = lr.github_calendar;
            }
            app.clamp_after_reload();
            app.last_refreshed = Some(chrono::Local::now());
            match lr.kind {
                LoadKind::Initial => {}
                LoadKind::Reload => match &lr.source {
                    // `R` exists to be current, so a reload that could only
                    // rebuild from the cache must not claim it reloaded.
                    app::DatasetSource::Fallback { failure, .. } => app.set_status(
                        format!(
                            "Reloaded from cache: GitHub {}",
                            ui::github_failure_label(failure)
                        ),
                        true,
                    ),
                    _ => app.set_status("Reloaded".to_string(), false),
                },
                // `R` under a known limit: nothing was sent, so the status
                // says when GitHub resumes instead of `Reloaded` (A6).
                LoadKind::FromMemory => {
                    let status = match github_limit_until(app, config, now) {
                        Some(until) => format!("Rebuilt locally: {}", limited_status(until)),
                        None => "Rebuilt locally".to_string(),
                    };
                    app.set_status(status, false);
                }
                LoadKind::WindowApply(w) => {
                    window_store.save(&w);
                    let label = app.header.clone();
                    app.set_status(format!("Window: {label}"), false);
                }
            }
            // After the load's own message, so it wins (N2): the clone stays
            // unclaimed on every load until it gets an origin remote, and a
            // `Reloaded` that hid the reason would leave it unexplained.
            if let Some(line) = lr.ambiguity {
                app.set_status(line, true);
            }
        }
        Err(e) => {
            if let Some(failure) = e.github_failure {
                // An applied attempt: its reading first, then its outcome.
                app.observe_budget(e.budget.as_deref(), true, now);
                app.github_attempt_failed(&failure, now);
                app.github_freshness.failed(failure, now);
            }
            // The status message says it once; `load_error` keeps saying it
            // for as long as there is nothing on screen to say it instead.
            app.set_status(format!("Load failed: {}", e.message), true);
            app.load_error = Some(e.message);
        }
    }
}

/// What a landed load tells the budget and the retry ladder. A load that went
/// to GitHub is an applied attempt (it supersedes every poll out), so its
/// reading applies and its outcome counts; a cache read and a load with no
/// fetch behind it tell them nothing.
fn record_load_attempt(
    app: &mut App,
    source: &app::DatasetSource,
    reading: Option<&github::BudgetReading>,
    now: chrono::DateTime<chrono::Local>,
) {
    match source {
        app::DatasetSource::Local | app::DatasetSource::Cache { .. } => {}
        app::DatasetSource::Network => {
            app.observe_budget(reading, true, now);
            app.github_backoff.succeeded();
        }
        app::DatasetSource::Fallback { failure, .. } => {
            app.observe_budget(reading, true, now);
            app.github_attempt_failed(failure, now);
        }
    }
}

/// GitHub's `diskUsage` for the remote `repo` carries, from `dataset`: the
/// size the fetch cap's pre-check reads (FZ-35b). `None` for a row that
/// carries no entry, or an entry from a payload without the field.
fn disk_usage_of(repo: &RepoStatus, dataset: &[github::GithubRepo]) -> Option<u64> {
    dataset
        .iter()
        .find(|g| carries(repo, g))
        .and_then(|g| g.disk_usage)
}

/// Run the fetch cap's pre-check on every row against `dataset`
/// (`FetchFacts::assess_growth`): whenever the dataset or the rows change,
/// so every automatic selection reads a current hold off the row.
fn assess_size_holds(repos: &mut [RepoStatus], dataset: &[github::GithubRepo], cap: Option<u64>) {
    for repo in repos {
        let current = disk_usage_of(repo, dataset);
        repo.fetch.assess_growth(current, cap);
    }
}

/// Rows a load rebuilt, with what this session learned about each checkout's
/// fetches carried over from the row it replaces (`FetchFacts::carried_into`).
/// Matched by path, the key the fetch facts are kept under.
fn carry_fetch_facts(old: &[RepoStatus], mut loaded: Vec<RepoStatus>) -> Vec<RepoStatus> {
    let by_path: HashMap<&str, &model::FetchFacts> = old
        .iter()
        .filter_map(|r| Some((r.path.as_deref()?, &r.fetch)))
        .collect();
    for repo in &mut loaded {
        if let Some(kept) = repo.path.as_deref().and_then(|p| by_path.get(p)) {
            repo.fetch = kept.carried_into(&repo.fetch);
        }
    }
    loaded
}

/// The event loop: one channel carries input, ticks, and the results of every
/// background task, so nothing the user does ever waits on git or the network.
/// Painting is dirty-flag driven: an event that changes nothing costs nothing.
async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Result<(), String> {
    // Pick the view from the real drawing area rather than the size probed
    // before the alternate screen existed: that probe can report the outer
    // shell's geometry and land on Compact in a terminal with ample room.
    if !app.view_overridden
        && let Ok(size) = terminal.size()
    {
        app.view = config.startup_view(size.width, size.height);
    }
    // The wait is spent on the loading field rather than on an empty frame.
    // Started before the first paint so there is never a blank one.
    let loading_running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if shows_loading_field(config.intro, app.loading) {
        app.loading_field = Some(ui::intro::Loading::new());
        loading_running.store(true, std::sync::atomic::Ordering::Relaxed);
        spawn_loading_task(tx.clone(), loading_running.clone());
    }
    terminal
        .draw(|frame| draw_screen(frame, app))
        .map_err(|e| e.to_string())?;

    // How many held paints have accumulated, so a stream that never drains
    // still gets drawn. See the paint at the foot of the loop.
    let mut coalesced: u32 = 0;

    while let Some(ev) = rx.recv().await {
        match ev {
            AppEvent::Tick => {
                app.on_tick();
                app.observe_day(chrono::Local::now().date_naive());
                // Drop a status message once it's outlived its TTL, so it
                // fades on its own rather than sitting frozen until the next
                // keypress.
                if app.status_expired(STATUS_TTL) {
                    app.clear_status();
                    app.needs_redraw = true;
                }
                // A visible spinner has to paint on every tick, otherwise the
                // frame index advances several steps between repaints and the
                // animation reads as a slow stutter rather than a spin. A
                // silent watch sweep draws no spinner, so it buys no repaints
                // either: it just updates the rows when it lands.
                let spinning = app.loading || app.is_refreshing() || app.ci_view_loading();
                // Everything else that moves on its own (the swept-ago clock,
                // the CI blink, a running CI run's duration) only needs about
                // one repaint a second.
                let slow_animated =
                    app.watch_mode || app.apply_progress.is_some() || app.ci_view_live();
                if spinning
                    || (slow_animated && app.spinner_frame.is_multiple_of(SLOW_REPAINT_TICKS))
                {
                    app.needs_redraw = true;
                }

                // A sweep that never reported back must not block watch forever.
                if let Some(started) = app.sweep_started
                    && started.elapsed() > SWEEP_STUCK_AFTER
                {
                    app.sweep_started = None;
                }
                app.expire_refresh_fetches(SWEEP_STUCK_AFTER);
                app.expire_fetches_in_flight(SWEEP_STUCK_AFTER, std::time::Instant::now());
                if app.watch_mode && !app.loading && app.sweep_started.is_none() {
                    // A missing timestamp means "due now", so watch can never
                    // silently die the way a Some-only guard could.
                    let due = app.last_refreshed.is_none_or(|t| {
                        (chrono::Local::now() - t).num_seconds() >= app.refresh_interval as i64
                    });
                    if due {
                        // Timer-driven, so silent: no header indicator.
                        start_sweep(app, &tx, true, args.author.clone(), false);
                    }
                }
                expire_stuck_github_poll(app, GITHUB_POLL_STUCK_AFTER, chrono::Local::now());
                if github_poll_due(app, config, chrono::Local::now()) {
                    app.push_detected = false;
                    start_github_poll(app, &tx, config);
                }
                if let Some(request) = ci_tick(app, chrono::Local::now(), std::time::Instant::now())
                {
                    spawn_ci_poll(&tx, request);
                }
                let (now, instant) = (chrono::Local::now(), std::time::Instant::now());
                for request in app.claim_ci_details(now, instant) {
                    spawn_ci_detail(&tx, request);
                }
                if let Some(walk) = app.claim_notes_walk(instant) {
                    spawn_notes_walk(&tx, walk);
                }
                if let Some(read) = app.claim_note_read() {
                    spawn_note_read(&tx, read);
                }
                app.settle_hover(instant);
                for request in app.claim_digest_logs(now) {
                    spawn_digest_log(&tx, request);
                }
            }
            AppEvent::Input(Event::Key(key)) => {
                // Any keypress takes the loading field down. Skip means skip:
                // an empty frame is what there was before any of this, and is
                // at least honest about how far along the load is.
                dismiss_loading_field(app, &loading_running);
                // Any keypress clears a transient status message before it's
                // handled, so the message shows until the next interaction and
                // then yields the footer back to the summary. The handler for
                // this same key may set a fresh one, which survives (set after
                // this clear).
                app.clear_status();
                if let Err(e) = handle_key(app, terminal, key, &tx, args, config, repo_dir).await {
                    app.set_status(format!("Error: {e}"), true);
                }
                app.needs_redraw = true;
            }
            AppEvent::Input(Event::Mouse(m)) => {
                handle_mouse_input(app, terminal, m);
                app.needs_redraw = true;
            }
            AppEvent::Input(Event::Resize(w, h)) => {
                // Follow the terminal back to the view that fits, unless the
                // user has taken the wheel with `v` or pinned one in config.
                if !app.view_overridden {
                    app.view = config.startup_view(w, h);
                }
                app.needs_redraw = true;
            }
            AppEvent::Input(_) => {}
            AppEvent::Loaded(result) => {
                apply_load_result(app, config, result);
                // An account rebuild that waited for this load goes now.
                start_account_rebuild(app, &tx, args, config, repo_dir);
                // The field gives way to the finished screen: the paint that
                // follows is the first screenful of real content.
                dismiss_loading_field(app, &loading_running);
                app.needs_redraw = true;
            }
            AppEvent::LoadingFrame => on_loading_frame(app),
            AppEvent::Progress(p) => {
                app.apply_progress = Some(p);
                app.needs_redraw = true;
            }
            AppEvent::RepoUpdated {
                path,
                status,
                fetch,
                claim,
            } => land_repo_update(app, &path, *status, fetch, claim),
            AppEvent::FetchProgress { path, progress } => {
                app.apply_fetch_progress(&path, progress);
            }
            AppEvent::Github { attempt, result } => {
                apply_github_poll(app, attempt, result, &tx, &args.author);
                start_account_rebuild(app, &tx, args, config, repo_dir);
            }
            AppEvent::SweepDone { at } => {
                app.sweep_landed(at);
            }
            AppEvent::Ci(answer) => app.apply_ci(*answer, chrono::Local::now()),
            AppEvent::CiDetail(answer) => app.apply_ci_detail(*answer, chrono::Local::now()),
            AppEvent::NotesWalked { walk, result } => app.apply_notes_walk(walk, result),
            AppEvent::NoteRead { read, text, facts } => {
                app.apply_note_facts(&read, facts);
                app.apply_note_read(read, text);
            }
            AppEvent::NoteCommit { read, last } => app.apply_note_commit(&read, last),
            AppEvent::DigestLog { path, key, commits } => {
                app.apply_digest_log(&path, &key, commits)
            }
        }

        if app.should_quit {
            return Ok(());
        }
        // Paint once per *burst* of events, not once per event.
        //
        // A trackpad flick delivers wheel events far faster than a full
        // repaint takes, and the channel is unbounded, so painting each one
        // let the queue run away: the scroll appeared to freeze, then played
        // out the whole backlog at once after the fingers had stopped. The
        // events themselves are cheap -- it is the repaint that is not -- so
        // holding the paint while more input is already waiting collapses the
        // burst into a single frame and the queue drains as fast as it fills.
        //
        // `needs_redraw` is deliberately left set when the paint is held: the
        // next pass through the loop inherits it, so nothing is lost, only
        // deferred.
        if app.needs_redraw {
            let animating = app.loading_field.is_some();
            if should_paint(rx.is_empty(), coalesced, animating) {
                coalesced = 0;
                app.needs_redraw = false;
                terminal
                    .draw(|frame| draw_screen(frame, app))
                    .map_err(|e| e.to_string())?;
            } else {
                coalesced += 1;
            }
        }
    }
    Ok(())
}

/// Dispatch one key to the overlay, the search box, or the normal bindings.
/// Overlays are modal: they consume the key and nothing else sees it.
async fn handle_key(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    key: KeyEvent,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
) -> Result<(), String> {
    match &app.overlay {
        Overlay::Help => {
            match key.code {
                KeyCode::Char('?') | KeyCode::Esc => app.toggle_help(),
                _ => {}
            }
            return Ok(());
        }
        Overlay::Branches(_) => {
            handle_branch_input(app, key, tx, args).await;
            return Ok(());
        }
        Overlay::TimeWindow(_) => {
            // Enter closes the overlay and hands the chosen window to a
            // background re-filter; the gauge animates from its progress.
            if handle_time_window_input(app, key)
                && let Some(w) = app.window
            {
                app.apply_progress = Some(ApplyProgress::Discovering);
                spawn_window_apply(tx, app, args, config, repo_dir, w);
            }
            return Ok(());
        }
        Overlay::Settings(_) => {
            handle_settings_input(app, key);
            return Ok(());
        }
        Overlay::None => {}
    }

    if app.search_active {
        handle_search_input(app, key);
        return Ok(());
    }

    handle_normal_input(app, terminal, key, tx, args, config, repo_dir).await
}

/// Weeks one wheel notch moves the contribution grid.
const CONTRIB_SCROLL_WEEKS: i32 = 3;

/// Mouse: left-click focuses the panel under the cursor (in pane 2 it also
/// moves the cursor to the clicked row, and copies a clicked hash); the scroll wheel
/// scrolls whichever panel the cursor is over, regardless of which panel has
/// keyboard focus. Ignored while an overlay or the search input is up.
fn handle_mouse_input(
    app: &mut App,
    terminal: &Terminal<CrosstermBackend<io::Stdout>>,
    m: MouseEvent,
) {
    if !matches!(app.overlay, Overlay::None) || app.search_active {
        return;
    }
    let Ok(size) = terminal.size() else { return };
    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
    let rects = ui::panel_rects(area, app);

    // A drag in progress owns every mouse event until the button comes up,
    // wherever the pointer has wandered to. Checked before the panel lookup,
    // which would otherwise drop events once the cursor left the divider.
    if let Some(divider) = app.dragging {
        match m.kind {
            MouseEventKind::Drag(MouseButton::Left) => match divider {
                ui::Divider::Vertical => app.set_repo_list_pct(rects.repo_list_pct_at(m.column)),
                ui::Divider::Horizontal => app.set_inspector_pct(rects.inspector_pct_at(m.row)),
            },
            // Anything that isn't a drag ends it. Releasing outside the
            // terminal can swallow the Up event entirely, which would
            // otherwise wedge every later mouse event in this branch.
            _ => app.dragging = None,
        }
        return;
    }

    // A strip pan owns its events for the same reason, and for the same
    // reason it is checked before the panel lookup: a drag that wanders off
    // the strip should keep panning, not silently stop.
    if let Some(from) = app.contrib_drag {
        match m.kind {
            MouseEventKind::Drag(MouseButton::Left) => {
                app.contrib_drag = Some(app.pan_contributions(from, m.column));
            }
            _ => app.contrib_drag = None,
        }
        return;
    }

    // A click on the inspector's or the strip's `[N]` tag folds or unfolds
    // it. Ahead of the divider, whose grab zone covers the inspector's tag.
    if m.kind == MouseEventKind::Down(MouseButton::Left)
        && let Some(panel) = rects.fold_tag_at(m.column, m.row)
    {
        app.clear_status();
        app.toggle_fold(panel);
        return;
    }

    // Grabbing a boundary starts a resize instead of focusing a panel.
    if m.kind == MouseEventKind::Down(MouseButton::Left)
        && let Some(divider) = rects.divider_at(m.column, m.row)
    {
        app.clear_status();
        app.dragging = Some(divider);
        return;
    }

    let Some(panel) = rects.at(m.column, m.row) else {
        // Off every pane: a pending header hover is cancelled (FZ-45).
        app.clear_hover();
        return;
    };
    // A click or a wheel ends a bracket streak, as a key does.
    if m.kind != MouseEventKind::Moved {
        app.brackets_cycling_views = false;
    }

    match m.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            app.clear_status();
            app.focus_panel(panel);
            // A click inside the strip both focuses it and picks the day under
            // the cursor, so the readout answers what you actually pointed at.
            if let Some(strip) = rects.contributions
                && let Some(slot) =
                    ui::contributions::slot_at(strip, app.contrib_scroll.get(), m.column, m.row)
            {
                app.contrib_cursor = Some(slot);
                app.needs_redraw = true;
            }
            // Press inside the strip also arms a pan, so the same gesture
            // that picks a day can drag the year past it.
            if panel == FocusedPanel::Contributions {
                app.contrib_drag = Some(m.column);
            }
            // A click on a row of pane 2 moves its cursor there, and one on
            // the row's hash copies it as `y` does. Resolved against the map
            // the last frame recorded of what it drew (FZ-37).
            if panel == FocusedPanel::Detail {
                click_detail(app, m.column, m.row, clipboard::copy);
            }
            // A click on a row of pane 1, a section header included (FZ-45),
            // lands the cursor there, resolved against the last frame.
            if panel == FocusedPanel::RepoList {
                app.click_list(m.column, m.row);
            }
        }
        // The pointer resting on a section header lands on it after a dwell,
        // settled on the tick (FZ-45); leaving the list cancels it.
        MouseEventKind::Moved if panel == FocusedPanel::RepoList => {
            app.hover_list(m.column, m.row, std::time::Instant::now())
        }
        MouseEventKind::Moved => app.clear_hover(),
        // Scroll the panel under the cursor without changing focus.
        //
        // The strip is a horizontal timeline, so every wheel axis moves it
        // sideways: a vertical wheel over it would otherwise do nothing a
        // user could see. Terminals disagree about whether shift+wheel
        // arrives as ScrollLeft/Right or as a plain wheel, so both are
        // handled and both mean the same thing here.
        MouseEventKind::ScrollDown | MouseEventKind::ScrollRight
            if panel == FocusedPanel::Contributions =>
        {
            app.scroll_contributions(CONTRIB_SCROLL_WEEKS)
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollLeft
            if panel == FocusedPanel::Contributions =>
        {
            app.scroll_contributions(-CONTRIB_SCROLL_WEEKS)
        }
        MouseEventKind::ScrollDown => match panel {
            FocusedPanel::RepoList => app.move_down(),
            FocusedPanel::Detail => app.scroll_detail(DETAIL_WHEEL_ROWS),
            FocusedPanel::Inspector => app.scroll_inspector(1),
            FocusedPanel::Reader => app.scroll_reader(1),
            FocusedPanel::Contributions => {}
        },
        MouseEventKind::ScrollUp => match panel {
            FocusedPanel::RepoList => app.move_up(),
            FocusedPanel::Detail => app.scroll_detail(-DETAIL_WHEEL_ROWS),
            FocusedPanel::Inspector => app.scroll_inspector(-1),
            FocusedPanel::Reader => app.scroll_reader(-1),
            FocusedPanel::Contributions => {}
        },
        _ => {}
    }
}

/// A left click at (`col`, `row`) inside the detail pane: move its cursor to
/// the row under it, and copy through `copy` when the click is on the hash.
fn click_detail(app: &mut App, col: u16, row: u16, copy: impl FnOnce(&str) -> Result<(), String>) {
    let Some(click) = app.detail_hits.borrow().at(col, row) else {
        return;
    };
    app.click_detail(&click);
    // The hash recorded at paint time, not `inspected_hash` read now: the
    // list can change between the paint and the click.
    if let Some(hash) = &click.hash {
        app.copy_hash(hash, copy);
    }
}

async fn handle_branch_input(
    app: &mut App,
    key: KeyEvent,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    args: &Args,
) {
    // Extract overlay data: need to work around borrow checker
    let is_confirming = matches!(&app.overlay, Overlay::Branches(bo) if bo.confirm_delete);

    if is_confirming {
        match key.code {
            KeyCode::Char('y') => {
                // Delete selected branches
                if let Overlay::Branches(ref bo) = app.overlay {
                    let repo_path = bo.repo_path.clone();
                    let to_delete = bo.selected_branch_names();
                    let expanded = shellexpand::tilde(&repo_path).to_string();
                    let path = Path::new(&expanded);
                    let mut deleted = 0u32;
                    for name in &to_delete {
                        if git::delete_branch(path, name).await.is_ok() {
                            deleted += 1;
                        }
                    }
                    app.set_status(format!("Deleted {deleted} branch(es)"), false);
                    // Refresh the row as the switch does, rather than wait for
                    // the next sweep to notice the deletion.
                    if deleted > 0
                        && let Some(idx) = app.selected_repo_index()
                    {
                        start_single_sweep(app, tx, idx, false, false, args.author.clone());
                    }
                }
                app.overlay = Overlay::None;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                if let Overlay::Branches(ref mut bo) = app.overlay {
                    bo.confirm_delete = false;
                }
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('b') | KeyCode::Char('s') => {
            app.overlay = Overlay::None;
        }
        // Enter switches to the highlighted branch (`git switch`), then
        // closes the overlay and refreshes the repo so ahead/behind and the
        // branch list reflect the new HEAD.
        KeyCode::Enter => {
            let target = if let Overlay::Branches(ref bo) = app.overlay {
                bo.branches
                    .get(bo.selected)
                    .map(|b| (bo.repo_path.clone(), b.name.clone(), b.is_current))
            } else {
                None
            };
            if let Some((repo_path, branch, is_current)) = target {
                if is_current {
                    app.overlay = Overlay::None;
                } else {
                    let expanded = shellexpand::tilde(&repo_path).to_string();
                    match git::switch_branch(Path::new(&expanded), &branch).await {
                        Ok(()) => {
                            app.set_status(format!("Switched to {branch}"), false);
                            // Rebuild in the background: the new HEAD changes
                            // ahead/behind, the branch list, and the history.
                            if let Some(idx) = app.selected_repo_index() {
                                start_single_sweep(app, tx, idx, false, false, args.author.clone());
                            }
                        }
                        Err(e) => app.set_status(format!("Switch failed: {e}"), true),
                    }
                    app.overlay = Overlay::None;
                }
            }
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if let Overlay::Branches(ref mut bo) = app.overlay {
                bo.move_down();
            }
            ensure_preview(app).await;
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Overlay::Branches(ref mut bo) = app.overlay {
                bo.move_up();
            }
            ensure_preview(app).await;
        }
        KeyCode::Char(' ') => {
            if let Overlay::Branches(ref mut bo) = app.overlay {
                bo.toggle_selected();
            }
        }
        KeyCode::Char('d') => {
            if let Overlay::Branches(ref mut bo) = app.overlay
                && bo.selected_count() > 0
            {
                bo.confirm_delete = true;
            }
        }
        _ => {}
    }
}

/// Modal input for the time-window overlay. All keys are consumed here
/// while it's open, so hjkl/arrows can't leak into list navigation.
/// Returns true when Enter confirmed a window, which the caller applies.
fn handle_time_window_input(app: &mut App, key: KeyEvent) -> bool {
    let Overlay::TimeWindow(ref mut two) = app.overlay else {
        return false;
    };
    match key.code {
        KeyCode::Esc | KeyCode::Char('t') | KeyCode::Char('q') => {
            app.overlay = Overlay::None;
        }
        KeyCode::Char('h') | KeyCode::Char('l') | KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
            two.toggle_field()
        }
        KeyCode::Char('k') | KeyCode::Up => two.step_up(),
        KeyCode::Char('j') | KeyCode::Down => two.step_down(),
        KeyCode::Char(c) if c.is_ascii_digit() => two.push_digit(c as u8 - b'0'),
        KeyCode::Enter => {
            let w = two.window();
            app.window = Some(w);
            app.overlay = Overlay::None;
            return true;
        }
        _ => {}
    }
    false
}

/// Key handling for the settings overlay: j/k walk the rows, h/l adjust the
/// focused one, digits type into numeric rows, Enter applies the live fields
/// and writes every field back to `config.toml`, Esc discards.
///
/// Applying and saving are one action deliberately. A separate "apply without
/// saving" would leave the running session disagreeing with the file it was
/// loaded from, with nothing on screen to say so.
fn handle_settings_input(app: &mut App, key: KeyEvent) {
    let Overlay::Settings(ref mut so) = app.overlay else {
        return;
    };
    match key.code {
        KeyCode::Esc | KeyCode::Char(',') | KeyCode::Char('q') => {
            app.overlay = Overlay::None;
        }
        KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => so.move_cursor(1),
        KeyCode::Char('k') | KeyCode::Up => so.move_cursor(-1),
        KeyCode::Char('l') | KeyCode::Right => so.step(1),
        KeyCode::Char('h') | KeyCode::Left => so.step(-1),
        KeyCode::Char(c) if c.is_ascii_digit() => so.push_digit(c as u8 - b'0'),
        KeyCode::Enter => {
            so.normalize();
            let updates = so.config_updates();
            let path = config::settings_path();
            match config::save_settings(&path, &updates) {
                Ok(()) => {
                    so.status = Some(format!("saved to {}", tilde_display(&path)));
                    // Live fields take effect immediately; the overlay stays
                    // open so the confirmation is actually readable.
                    let saved = so.clone();
                    apply_live_settings(app, &saved);
                }
                Err(e) => so.status = Some(e),
            }
        }
        _ => {}
    }
}

/// The settings overlay `,` opens. Seeded from live app state where one
/// exists (interval, view, pane persistence, the icon set) and from the
/// loaded config where it does not, so the values shown are the ones
/// actually in force.
fn settings_overlay(app: &App, config: &config::Config) -> app::SettingsOverlay {
    app::SettingsOverlay::new(
        app.refresh_interval,
        config.github_poll_interval,
        app.watch_mode,
        app.view_overridden.then_some(app.view),
        config.work_start,
        config.work_end,
        app.persist_pane_sizes,
        app.contributions,
        app.contrib_cell,
        config.intro,
        ui::badges::active_icons(),
    )
}

/// Apply the live fields of a saved settings overlay to the running app, so
/// they show on the next frame. Restart-only fields are left to the next
/// launch.
///
/// Runs in the event loop, on the thread that draws, which is what makes
/// `set_active_icons` (thread-local) reach the next frame.
fn apply_live_settings(app: &mut App, so: &app::SettingsOverlay) {
    app.refresh_interval = so.refresh_interval;
    app.persist_pane_sizes = so.persist_panes;
    app.apply_contributions_setting(so.contributions);
    app.contrib_cell = so.contrib_cell;
    ui::badges::set_active_icons(so.icons);
    if let Some(v) = so.view {
        app.view = v;
        // Same flag `v` sets, so a later resize can't undo an explicit
        // choice.
        app.view_overridden = true;
    } else {
        app.view_overridden = false;
    }
    app.needs_redraw = true;
}

/// Build and show the branches overlay for the selected repo: every local
/// branch, with merged ones flagged for cleanup and the current one marked.
/// Enter switches to a branch; space+d deletes merged ones.
async fn open_branch_overlay(app: &mut App) {
    let Some(repo) = app.selected_repo() else {
        return;
    };
    let Some(ref path) = repo.path else { return };
    let repo_name = repo.name.clone();
    let path = path.clone();

    let expanded = shellexpand::tilde(&path).to_string();
    let repo_path = Path::new(&expanded);
    let current = git::current_branch(repo_path).await.unwrap_or_default();
    let base = git::default_branch(repo_path)
        .await
        .unwrap_or_else(|| current.clone());
    let all = git::all_branches(repo_path).await;
    let merged = git::merged_branches(repo_path, &base).await;

    let branches: Vec<BranchEntry> = all
        .into_iter()
        .map(|name| {
            let is_current = name == current;
            let is_protected = git::is_protected_branch(&name);
            let is_merged = merged.contains(&name) && !is_current;
            BranchEntry {
                name,
                is_merged,
                is_current,
                is_protected,
                selected_for_delete: false,
            }
        })
        .collect();

    app.overlay = Overlay::Branches(BranchOverlay {
        repo_name,
        repo_path: path,
        branches,
        selected: 0,
        confirm_delete: false,
        previews: std::collections::HashMap::new(),
    });
    // Prefetch the first branch's preview so the overlay opens with content.
    ensure_preview(app).await;
}

/// How many commits the branch preview shows.
const BRANCH_PREVIEW_COUNT: u32 = 6;

/// Lazily fetch the highlighted branch's recent commits into the overlay's
/// preview cache, so the overlay can show "what happened on that branch"
/// without checking it out. A no-op once a branch is cached.
async fn ensure_preview(app: &mut App) {
    let Overlay::Branches(ref bo) = app.overlay else {
        return;
    };
    let Some(branch) = bo.selected_branch() else {
        return;
    };
    if bo.previews.contains_key(branch) {
        return;
    }
    let branch = branch.to_string();
    let expanded = shellexpand::tilde(&bo.repo_path).to_string();
    let commits = git::branch_commits(Path::new(&expanded), &branch, BRANCH_PREVIEW_COUNT).await;
    if let Overlay::Branches(ref mut bo) = app.overlay {
        bo.previews.insert(branch, commits);
    }
}

fn handle_search_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.clear_search(),
        KeyCode::Enter => app.end_search(),
        KeyCode::Backspace => {
            if app.search_query.is_empty() {
                app.clear_search();
            } else {
                app.search_pop_char();
            }
        }
        KeyCode::Char(c) => app.search_push_char(c),
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)] // the key handler needs the whole context
async fn handle_normal_input(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    key: KeyEvent,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
) -> Result<(), String> {
    // Any key but a bracket ends a streak of them (`App::press_bracket`).
    if !matches!(key.code, KeyCode::Char('[' | ']')) {
        app.brackets_cycling_views = false;
    }
    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        // A leftover search query (set, then Enter'd out of edit mode) is
        // cleared by Esc -- matching the strip's "esc to clear" hint.
        KeyCode::Esc if !app.search_query.is_empty() => app.clear_search(),
        KeyCode::Tab => app.toggle_panel_focus(),
        KeyCode::BackTab => app.toggle_panel_focus_back(),
        // Shift+scroll: scroll the *next* pane, so you can stand in one pane
        // and pull more into view in the one beside it without leaving where
        // you are. Plain ↓/j (below) drive whatever is focused; these reach
        // past it into the pane after it. Shift+j/k
        // arrive as uppercase `J`/`K` (no modifier needed), Shift+arrow as an
        // arrow carrying the SHIFT modifier. `PageUp`/`PageDown` still scroll
        // the focused detail pane itself, which is the gesture this used to
        // be. See `App::shift_scroll_target`.
        // On a header the next pane is the digest's (FZ-45).
        KeyCode::Char('J') if app.on_header().is_some() => app.digest_scroll_next_panel(1),
        KeyCode::Char('K') if app.on_header().is_some() => app.digest_scroll_next_panel(-1),
        KeyCode::Down
            if key.modifiers.contains(KeyModifiers::SHIFT) && app.on_header().is_some() =>
        {
            app.digest_scroll_next_panel(1)
        }
        KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) && app.on_header().is_some() => {
            app.digest_scroll_next_panel(-1)
        }
        KeyCode::Char('J') => app.scroll_next_panel(1),
        KeyCode::Char('K') => app.scroll_next_panel(-1),
        // The `notes` reader pages from the tree or itself (FZ-62); `J`/`K`
        // above scroll it a line through `scroll_next_panel`.
        KeyCode::Char('H') if app.reader_pages() => app.page_reader(-1),
        KeyCode::Char('L') if app.reader_pages() => app.page_reader(1),
        KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => app.scroll_next_panel(1),
        KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => app.scroll_next_panel(-1),
        // Must be matched before the plain `c` arm below, or ctrl-c switches
        // the contributions source instead of quitting.
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        // `[` and `]` step the repo pane's body between its seven views --
        // lazygit's own binding for the same gesture, and unlike a shifted
        // letter it gives backwards for free.
        // On the focused inspector while pane 2 shows `ci`, they step its
        // run bodies instead (`App::press_bracket`).
        KeyCode::Char(']') => app.press_bracket(false),
        KeyCode::Char('[') => app.press_bracket(true),
        // Only while the strip is unfolded; `cycle_contributions_source`
        // ignores it otherwise.
        KeyCode::Char('c') => {
            app.cycle_contributions_source();
            // Switching to the local source is useless without a histogram,
            // and the sweep is the only thing that fills one. Ask for one
            // rather than showing an empty year that looks like a bug.
            if app.contributions == app::ContributionsMode::Local
                && app.repos.iter().all(|r| r.daily.is_none())
            {
                app.set_status("Sweeping for local history…".to_string(), false);
                start_sweep(app, tx, false, args.author.clone(), true);
            }
        }
        KeyCode::Char('4') => app.press_panel_key(FocusedPanel::Contributions),
        // Left/right walk columns (weeks), up/down walk rows (weekdays),
        // which is how the grid reads on screen. They used to be swapped, so
        // the arrow keys moved at right angles to what the eye expected.
        KeyCode::Char('h') | KeyCode::Left if app.focused_panel == FocusedPanel::Contributions => {
            app.move_contrib_cursor(-1, 0, chrono::Local::now().date_naive())
        }
        KeyCode::Char('l') | KeyCode::Right if app.focused_panel == FocusedPanel::Contributions => {
            app.move_contrib_cursor(1, 0, chrono::Local::now().date_naive())
        }
        KeyCode::Home if app.focused_panel == FocusedPanel::Contributions => {
            app.jump_contrib_cursor(false, chrono::Local::now().date_naive())
        }
        KeyCode::End if app.focused_panel == FocusedPanel::Contributions => {
            app.jump_contrib_cursor(true, chrono::Local::now().date_naive())
        }
        // On a section header pane 2 draws the digest and the inspector its
        // line (FZ-45): `j`/`k` there walk the digest's lines and scroll the
        // inspector instead of the hidden repo's views.
        KeyCode::Char('j' | 'k') | KeyCode::Down | KeyCode::Up
            if app.on_header().is_some()
                && matches!(
                    app.focused_panel,
                    FocusedPanel::Detail | FocusedPanel::Inspector
                ) =>
        {
            let delta = if matches!(key.code, KeyCode::Char('j') | KeyCode::Down) {
                1
            } else {
                -1
            };
            if app.focused_panel == FocusedPanel::Detail {
                app.digest_nav(delta);
            } else {
                app.scroll_digest_inspector(delta);
            }
        }
        KeyCode::Char('j') | KeyCode::Down => match app.focused_panel {
            FocusedPanel::RepoList => app.move_down(),
            FocusedPanel::Detail => app.detail_nav_down(),
            FocusedPanel::Inspector => app.inspector_nav(1),
            FocusedPanel::Reader => app.scroll_reader(1),
            FocusedPanel::Contributions => {
                app.move_contrib_cursor(0, 1, chrono::Local::now().date_naive())
            }
        },
        KeyCode::Char('k') | KeyCode::Up => match app.focused_panel {
            FocusedPanel::RepoList => app.move_up(),
            FocusedPanel::Detail => app.detail_nav_up(),
            FocusedPanel::Inspector => app.inspector_nav(-1),
            FocusedPanel::Reader => app.scroll_reader(-1),
            FocusedPanel::Contributions => {
                app.move_contrib_cursor(0, -1, chrono::Local::now().date_naive())
            }
        },
        KeyCode::PageDown => match app.focused_panel {
            FocusedPanel::Inspector => app.scroll_inspector(10),
            FocusedPanel::Reader => app.scroll_reader(10),
            _ => app.scroll_detail(10),
        },
        KeyCode::PageUp => match app.focused_panel {
            FocusedPanel::Inspector => app.scroll_inspector(-10),
            FocusedPanel::Reader => app.scroll_reader(-10),
            _ => app.scroll_detail(-10),
        },
        KeyCode::Char('v') => app.toggle_view(),
        KeyCode::Char('y') => app.yank_with(clipboard::copy),
        KeyCode::Char('?') => app.toggle_help(),
        KeyCode::Char('/') => app.start_search(),
        // Fetch (not pull): update remote-tracking refs so ahead/behind is
        // accurate, without merging anything into the working tree. Actual
        // pull/merge is left to lazygit (Enter) on purpose -- fetching is
        // safe and non-destructive, merging is not.
        KeyCode::Char('f') => fetch_key(app, tx, args.author.clone()),
        KeyCode::Char('F') => {
            app.set_status("\u{27f3} Fetching all…".to_string(), false);
            start_sweep(app, tx, true, args.author.clone(), true);
        }
        // On a file in the `notes` view, $EDITOR on that file (FZ-53).
        KeyCode::Enter if note_to_open(app).is_some() => {
            let file = note_to_open(app).unwrap_or_default();
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
            suspend_and_run(terminal, &editor, &[&file.to_string_lossy()])?;
            app.invalidate_notes();
        }
        // On a header, Enter goes to the digest line's repo row (FZ-45).
        KeyCode::Enter if app.on_header().is_some() => {
            app.jump_to_digest_line(chrono::Local::now())
        }
        KeyCode::Enter => {
            // Open lazygit in selected repo
            if let Some(repo) = app.selected_repo()
                && let Some(ref path) = repo.path
            {
                let expanded = shellexpand::tilde(path).to_string();
                suspend_and_run(terminal, "lazygit", &["-p", &expanded])?;
                app.invalidate_notes();
            }
        }
        KeyCode::Char('e') => {
            // Open $EDITOR in selected repo
            if let Some(repo) = app.selected_repo()
                && let Some(ref path) = repo.path
            {
                let expanded = shellexpand::tilde(path).to_string();
                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
                suspend_and_run(terminal, &editor, &[&expanded])?;
                app.invalidate_notes();
            }
        }
        // The `notes` view's tree: notes directories only, or every
        // markdown file. Nothing elsewhere.
        KeyCode::Char('m') => app.toggle_notes_markdown(),
        KeyCode::Char('o') => {
            // Open shell in repo dir
            if let Some(repo) = app.selected_repo()
                && let Some(ref path) = repo.path
            {
                let expanded = shellexpand::tilde(path).to_string();
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
                suspend_and_run_in_dir(terminal, &shell, &[], &expanded)?;
                app.invalidate_notes();
            }
        }
        KeyCode::Char('g') => {
            app.cycle_group();
        }
        KeyCode::Char('G') => {
            app.clear_group();
        }
        // Panel focus: 1 = repo list, 2 = detail pane, 3 = inspector, and on
        // the focused inspector or strip its number folds it. Quick
        // filters moved to `#tag` words typed into search (see
        // app::parse_query) so plain digits are free for this.
        KeyCode::Char('1') => app.press_panel_key(FocusedPanel::RepoList),
        KeyCode::Char('2') => app.press_panel_key(FocusedPanel::Detail),
        KeyCode::Char('3') => app.press_panel_key(FocusedPanel::Inspector),
        KeyCode::Char('0') => app.clear_filters(),
        // Branches overlay: switch (Enter) or clean up merged branches
        // (space to mark, d to delete). `b` and `s` both open it -- `s` for
        // the "switch" mnemonic the pull-removal left room for.
        // Expand or collapse `info`'s remote branch summary. The full list
        // with its state lives in the `branches` view; this is for a quick
        // look without leaving the glance.
        KeyCode::Char('B') => {
            app.expand_remote_branches = !app.expand_remote_branches;
            app.needs_redraw = true;
        }
        KeyCode::Char('b') | KeyCode::Char('s') => {
            open_branch_overlay(app).await;
        }
        // On a header, `t` cycles the digest's window instead (FZ-45): slot,
        // today, yesterday, week.
        KeyCode::Char('t') if app.on_header().is_some() => app.cycle_digest_window(),
        // Time window overlay: pick how far back "activity" reaches. Not
        // available in -d mode, which never fetched a GitHub dataset.
        KeyCode::Char('t') => {
            if app.dirty_mode {
                app.set_status(
                    "Time window needs GitHub data (not in -d mode)".to_string(),
                    true,
                );
            } else {
                app.open_time_window();
            }
        }
        // Pane resize. Angle brackets move the vertical divider (repo list
        // narrower/wider); +/- move the horizontal one (inspector taller or
        // shorter). `=` resets both, which is also what every launch does
        // unless persist_pane_sizes is on.
        KeyCode::Char('<') => app.resize_repo_list(-1),
        KeyCode::Char('>') => app.resize_repo_list(1),
        KeyCode::Char('+') => app.resize_inspector(1),
        KeyCode::Char('-') => app.resize_inspector(-1),
        KeyCode::Char('=') => app.reset_pane_sizes(),
        KeyCode::Char(',') => {
            app.overlay = Overlay::Settings(settings_overlay(app, config));
        }
        // The `ci` view's handoffs on the highlighted run (FZ-30e). Ahead of
        // `O`, which keeps opening the first PR everywhere else.
        KeyCode::Char('l' | 'a' | 'x' | 'O') if ci_handoff_applies(app) => {
            ci_handoff_key(app, terminal, tx, key.code)?;
        }
        KeyCode::Char('O') => {
            // Open first PR in browser (Shift+O)
            if let Some(repo) = app.selected_repo() {
                if let Some(pr) = repo.prs.first() {
                    let _ = StdCommand::new(OPENER).arg(&pr.url).status();
                } else if repo.pr_count > 0 {
                    app.set_status(
                        "Use -p flag to fetch PR details for browser open".to_string(),
                        false,
                    );
                }
            }
        }
        // Local refresh: rebuild the repos already listed (status,
        // ahead/behind, dirty, commits, reflog, graph, stashes, worktrees).
        // Offline -- no fetch, no GitHub, no discovery -- so it won't surface
        // a new repo, but it reflects local work on the ones already shown.
        // Gets the unshifted key because it's the one you reach for
        // constantly; `R` is the full rediscover.
        // Refresh: re-stat the listed repos *and* re-poll GitHub. The split
        // against `R` is refresh vs refresh-plus-rediscover, not offline vs
        // online: CI status has no local source, so an `r` that skipped
        // GitHub updated two thirds of each row and left the CI lamp beside
        // them frozen. The poll runs on a background task, so the local
        // numbers land exactly as fast as they did before.
        // Under a known rate limit the poll is skipped (`refresh_key`).
        KeyCode::Char('r') => {
            refresh_key(app, tx, args, config, chrono::Local::now());
        }
        // Full reload: re-fetch GitHub (cache-bypassed) and re-run discovery,
        // so a just-pushed or newly-cloned repo appears. Shifted because it
        // costs a network round trip and GitHub rate limit. Runs on a
        // background task, so the list stays scrollable while it works.
        // Ignored while a load runs; under a known rate limit it rebuilds
        // from memory instead (`reload_key`).
        KeyCode::Char('R') => {
            reload_key(app, tx, args, config, repo_dir, chrono::Local::now());
        }
        // Toggle watch mode: sweep the listed repos every `refresh_interval`
        // seconds (fetch included, unlike `r`), and re-poll GitHub for PR and
        // CI data. Reset the clock on enable so the first sweep is a full
        // interval out.
        KeyCode::Char('w') => {
            app.watch_mode = !app.watch_mode;
            if app.watch_mode {
                app.last_refreshed = Some(chrono::Local::now());
                app.set_status(
                    format!("Watch on: auto-refresh every {}s", app.refresh_interval),
                    false,
                );
            } else {
                app.set_status("Watch off".to_string(), false);
            }
        }
        _ => {}
    }
    Ok(())
}

/// The file `Enter` opens in $EDITOR: the `notes` view's highlighted file,
/// while pane 2, the reader or the card has focus. `None` leaves `Enter` to
/// open lazygit on the repo.
fn note_to_open(app: &App) -> Option<std::path::PathBuf> {
    let focused = matches!(
        app.focused_panel,
        FocusedPanel::Detail | FocusedPanel::Inspector | FocusedPanel::Reader
    );
    (app.notes_wanted() && focused)
        .then(|| app.notes_highlighted())
        .flatten()
}

/// Suspend the TUI, run a command, then resume. Input reading is paused for
/// the whole child run so the reader thread cannot swallow its keystrokes.
fn suspend_and_run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    cmd: &str,
    args: &[&str],
) -> Result<(), String> {
    // Leave alternate screen
    disable_raw_mode().map_err(|e| e.to_string())?;
    release_terminal(terminal.backend_mut()).map_err(|e| e.to_string())?;
    terminal.show_cursor().map_err(|e| e.to_string())?;

    // Run the command
    let _ = with_input_paused(|| StdCommand::new(cmd).args(args).status());

    reclaim_terminal(terminal.backend_mut()).map_err(|e| e.to_string())?;
    enable_raw_mode().map_err(|e| e.to_string())?;
    terminal.clear().map_err(|e| e.to_string())?;
    Ok(())
}

/// Suspend the TUI, run a command in a specific directory, then resume.
fn suspend_and_run_in_dir(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    cmd: &str,
    args: &[&str],
    dir: &str,
) -> Result<(), String> {
    disable_raw_mode().map_err(|e| e.to_string())?;
    release_terminal(terminal.backend_mut()).map_err(|e| e.to_string())?;
    terminal.show_cursor().map_err(|e| e.to_string())?;

    let expanded = shellexpand::tilde(dir);
    let _ = with_input_paused(|| {
        StdCommand::new(cmd)
            .args(args)
            .current_dir(expanded.as_ref())
            .status()
    });

    reclaim_terminal(terminal.backend_mut()).map_err(|e| e.to_string())?;
    enable_raw_mode().map_err(|e| e.to_string())?;
    terminal.clear().map_err(|e| e.to_string())?;
    Ok(())
}

/// A `ci` view handoff key, acting on the highlighted run (FZ-30e).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunKey {
    /// `l`: the run's log in the inspector.
    Log,
    /// `a`: rerun it ("again").
    Rerun,
    /// `x`: cancel it.
    Cancel,
    /// `O`: its page in the browser.
    Open,
}

impl RunKey {
    fn from_code(code: KeyCode) -> Option<Self> {
        match code {
            KeyCode::Char('l') => Some(Self::Log),
            KeyCode::Char('a') => Some(Self::Rerun),
            KeyCode::Char('x') => Some(Self::Cancel),
            KeyCode::Char('O') => Some(Self::Open),
            _ => None,
        }
    }
}

/// What a handoff key does with a run it allows.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RunHandoff {
    /// Show the run's `log` body in the inspector, without leaving fleetz.
    Inspect,
    /// Suspend fleetz and run `gh` with these arguments.
    Gh(Vec<String>),
    /// Open this URL with `OPENER`, as `O` opens a PR.
    Open(String),
}

/// Whether the handoff keys act: pane 2 focused and showing `ci`. Anywhere
/// else they keep their other meanings.
fn ci_handoff_applies(app: &App) -> bool {
    app.focused_panel == FocusedPanel::Detail && app.ci_wanted()
}

/// What `key` does to `run` of `owner`/`name`, or the status line note when
/// the run does not allow it. The inspector's `log` body reads a job's log
/// only once it has finished and `gh` will not rerun a run still going, so
/// both wait for a finished run; `gh` cancels only a run in progress or
/// queued. Every `gh` call names the repo with `-R`, so it works from any
/// directory.
fn run_handoff_for(
    run: &ci::WorkflowRun,
    key: RunKey,
    owner: &str,
    name: &str,
) -> Result<RunHandoff, &'static str> {
    let finished = run.status.as_deref() == Some("completed");
    let failed = run.conclusion.as_deref() == Some("failure");
    let (verb, flag) = match key {
        RunKey::Open => return Ok(RunHandoff::Open(run.url.clone())),
        RunKey::Log if !finished => return Err("run has not finished"),
        RunKey::Rerun if !finished => return Err("run has not finished"),
        RunKey::Cancel if !run.is_live() => return Err("run is not in progress"),
        RunKey::Log => return Ok(RunHandoff::Inspect),
        RunKey::Rerun => ("rerun", failed.then_some("--failed")),
        RunKey::Cancel => ("cancel", None),
    };
    let mut args = vec!["run".to_string(), verb.to_string(), run.id.to_string()];
    args.extend(flag.map(str::to_string));
    args.extend(["-R".to_string(), format!("{owner}/{name}")]);
    Ok(RunHandoff::Gh(args))
}

/// The handoff `key` asks for on the highlighted run, with the repo's CI
/// target; `None` on an empty view or a row with no GitHub identity, and
/// `None` with the reason on the status line when the run does not allow it.
fn resolve_run_handoff(
    app: &mut App,
    key: RunKey,
) -> Option<(RunHandoff, (app::RemoteKey, String, String))> {
    let target = app.selected_repo().and_then(ci::target)?;
    let run = app.highlighted_ci_run()?;
    match run_handoff_for(run, key, &target.1, &target.2) {
        Ok(handoff) => Some((handoff, target)),
        Err(note) => {
            app.set_status(note.to_string(), false);
            None
        }
    }
}

/// Run a `ci` view key: `l` shows the run's log in the inspector, the
/// others hand off. Back from `gh`, the run's repo polls at once
/// (`ci_poll_after_handoff`) so the rerun or cancel shows.
fn ci_handoff_key(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    code: KeyCode,
) -> Result<(), String> {
    let Some(key) = RunKey::from_code(code) else {
        return Ok(());
    };
    let Some((handoff, target)) = resolve_run_handoff(app, key) else {
        return Ok(());
    };
    match handoff {
        RunHandoff::Gh(args) => {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            suspend_and_run(terminal, "gh", &args)?;
            if let Some(request) = ci_poll_after_handoff(app, target, chrono::Local::now()) {
                spawn_ci_poll(tx, request);
            }
        }
        RunHandoff::Open(url) => {
            let _ = StdCommand::new(OPENER).arg(&url).status();
        }
        RunHandoff::Inspect => app.inspect_ci_log(),
    }
    Ok(())
}

/// The CI poll to start for `target` back from a `gh` handoff, with its
/// lock taken: due at once, skipping the 10 s or 60 s interval and the
/// dwell, but never under `-d`, with a poll already out, or while a rate
/// limit, the budget or a backoff holds the repo (`ci::poll_due`). It skips
/// the interval and the budget pace's stretch of it, but never a pause or a
/// backoff.
fn ci_poll_after_handoff(
    app: &mut App,
    target: (app::RemoteKey, String, String),
    now: chrono::DateTime<chrono::Local>,
) -> Option<ci::CiRequest> {
    let (key, owner, name) = target;
    if app.dirty_mode {
        return None;
    }
    let due = {
        let mut cadence = app.ci_cadence(&key, ci::DWELL, now);
        cadence.last_attempt = None;
        ci::poll_due(&cadence)
    };
    due.then(|| app.claim_ci_poll(key, owner, name))
}

/// Hand the terminal to a child process: mouse capture off, then the
/// alternate screen left.
///
/// The mirror of `reclaim_terminal`. Mouse reporting belongs to the terminal
/// rather than to a screen (#68), so leaving the alternate screen alone left
/// fleetz's capture on for the child, which then received mouse events it
/// never asked for (#82).
fn release_terminal(out: &mut impl io::Write) -> io::Result<()> {
    crossterm::execute!(out, DisableMouseCapture, LeaveAlternateScreen)
}

/// Take the terminal back from a child process that ran in it.
///
/// Mouse capture is requested again every time, not just at startup: lazygit
/// and nvim switch mouse reporting off on their way out, and the modes belong
/// to the terminal rather than to a screen, so re-entering the alternate
/// screen alone left wheel and drag dead for the rest of the session (#68).
fn reclaim_terminal(out: &mut impl io::Write) -> io::Result<()> {
    crossterm::execute!(out, EnterAlternateScreen, EnableMouseCapture)
}

/// One repo queued for a sweep.
struct SweepTarget {
    /// Display path as stored in RepoStatus.path (tilde form), the update key.
    display_path: String,
    fs_path: std::path::PathBuf,
    /// The row as it was when the sweep started: the WorkKey baseline and
    /// the source of reused probes. Its non-local fields are stale by the
    /// time the result lands, which is why `apply_local_sweep` ignores them.
    old: Box<RepoStatus>,
}

/// A sweep that never reported back must not hold the watch lock forever.
const SWEEP_STUCK_AFTER: std::time::Duration = std::time::Duration::from_secs(180);

/// Same idea for the GitHub poll. The HTTP client has no timeout (`gh` only
/// supplies the token), so a black-holed request would otherwise hold
/// `github_poll_inflight` for the rest of the session and no poll could start.
/// Past this the poll is given up on and recorded as a timed-out failure,
/// which the header shows (`expire_stuck_github_poll`).
const GITHUB_POLL_STUCK_AFTER: std::time::Duration = std::time::Duration::from_secs(60);

/// Same idea for a CI poll (`App::expire_stuck_ci_polls`). Longer than the
/// fleet's, since one poll is up to three calls in a row (list, jobs, log),
/// each bounded at 30 s by the shared client, after a token lookup bounded at
/// 10 s: past this one is wedged, not slow.
const CI_POLL_STUCK_AFTER: std::time::Duration = std::time::Duration::from_secs(120);

/// Floor on the GitHub poll interval, whatever the config says, and the
/// interval while a repo is `Pending`. One poll at `repo_limit` 50 costs 3
/// points, so one instance at the floor spends 720 of the 5000-point hour:
/// comfortable for one instance, not for several on one account, which reach
/// the limit at seven (`docs/api-budget-design.md` section 3). The floor
/// stops a typo; the budget pacing (`app::ApiBudget::pace`) is what slows
/// polling when the account runs low.
const GITHUB_POLL_MIN: u64 = 15;

/// Rows the detail pane scrolls per wheel notch.
///
/// The wheel used to call `detail_nav_down/up`, moving the selection rather
/// than the view, so a notch jumped a whole commit block in the graph view.
/// The inspector already treats the wheel as a scroll; this makes the detail
/// pane agree, and makes the help overlay's "wheel scrolls" true.
const DETAIL_WHEEL_ROWS: i32 = 3;

/// Whether a sweep result means the repo moved in a way CI might care about,
/// and the next GitHub poll should not wait out its interval.
///
/// Two signals, because neither alone is enough:
///
/// - **HEAD moved.** Catches the commit. Note a `git push` does *not* move
///   HEAD, so this misses a push of commits made before fleetz was watching.
/// - **Unpushed count fell to zero.** Catches that push. But it only fires if
///   an earlier sweep actually observed the "committed, not yet pushed" state,
///   which it will not when you commit and push inside one sweep interval --
///   the common case, and the reason the first version of this missed.
///
/// Together they cover commit-then-push at any speed. HEAD is compared only
/// when both sweeps produced a key, so an unknown fingerprint reads as "no
/// information" rather than as a change, which would poll on every sweep.
fn repo_moved(before: &RepoStatus, after: &RepoStatus) -> bool {
    let head_moved = match (&before.work_key, &after.work_key) {
        (Some(a), Some(b)) => a.head_sha != b.head_sha,
        _ => false,
    };
    let pushed = before.ahead > 0 && after.ahead == 0;
    head_moved || pushed
}

/// Land one repo's sweep result (`AppEvent::RepoUpdated`). The full sweep and
/// the single-repo sweep both arrive here, so they share one merge: only the
/// sweep-owned fields are taken (`RepoStatus::apply_local_sweep`), so a poll
/// that landed while the sweep ran keeps its CI and PR data.
///
/// A result whose path no longer has a row is dropped: the repo left the list
/// while its sweep was in flight, and a late result must not bring it back.
///
/// The fetch outcome lands separately from the sweep's fields, on the row's
/// `fetch` facts: only a sweep that fetched changes them, and a failure keeps
/// every local field and the remote refs it had (rules 2 and 4 of the fetch
/// contract). Any finished fetch settles a refresh waiting on it.
#[cfg(test)]
fn apply_repo_updated(app: &mut App, path: &str, status: RepoStatus, fetch: git::FetchOutcome) {
    land_repo_update(app, path, status, fetch, None);
}

/// `apply_repo_updated` for the sweep that held fetch claim `claim`: its
/// landing ends that claim only (`App::fetch_landed_under`), so a sweep
/// whose claim expired cannot end a newer one on the same path.
fn land_repo_update(
    app: &mut App,
    path: &str,
    status: RepoStatus,
    fetch: git::FetchOutcome,
    claim: Option<u64>,
) {
    app.settle_refresh_fetch(path, &fetch);
    app.fetch_landed_under(path, &fetch, claim);
    app.fetch_backoff.record(path, &fetch, chrono::Local::now());
    let mut name = None;
    if let Some(repo) = app
        .repos
        .iter_mut()
        .find(|r| r.path.as_deref() == Some(path))
    {
        if repo_moved(repo, &status) {
            app.push_detected = true;
        }
        repo.apply_local_sweep(status);
        repo.fetch.record(&fetch, chrono::Local::now());
        // The size this success brought the clone to: the pre-check's next
        // baseline (FZ-35b). Recorded clears a growth hold.
        if fetch == git::FetchOutcome::Succeeded {
            repo.fetch.disk_baseline = disk_usage_of(repo, &app.dataset);
        }
        name = Some(repo.name.clone());
    }
    let name = name.unwrap_or_else(|| path.to_string());
    if let Some(report) = app.manual_fetches.land(path, &name, &fetch) {
        app.set_status(ui::fetch_report_label(&report), true);
    }
    app.needs_redraw = true;
}

/// Land a GitHub poll's result (`AppEvent::Github`).
///
/// A failure changes no row and leaves the data time alone: what is in memory
/// is at least as new as the cache file, so the fallback the fetch attaches is
/// not applied here (contract rule 4). Only launch and `R` use it.
fn apply_github_poll(
    app: &mut App,
    attempt: u64,
    result: Result<Box<github::GithubFetch>, github::FetchError>,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    author: &Option<String>,
) {
    apply_github_poll_at(app, attempt, result, tx, author, chrono::Local::now());
}

/// `apply_github_poll` with the arrival time passed in, so a test can plant
/// the clock the budget reading and the not-before are anchored to.
fn apply_github_poll_at(
    app: &mut App,
    attempt: u64,
    result: Result<Box<github::GithubFetch>, github::FetchError>,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    author: &Option<String>,
    now: chrono::DateTime<chrono::Local>,
) {
    // Whether or not the answer is applied, its headers are true about the
    // account at the moment the server wrote them (design 4.1).
    let reading = match &result {
        Ok(fetch) => fetch.meta.budget.clone(),
        Err(e) => e.meta.budget.clone(),
    };
    // The in-flight lock belongs to the newest attempt. A late answer from
    // one given up on must not release it, or a second request stacks; the
    // newest one's answer always does, even when it is discarded below.
    let is_latest = app.github_freshness.is_latest(attempt);
    if is_latest {
        app.github_poll_inflight = false;
        app.github_poll_started = None;
    }
    // An attempt given up on can still answer, and an `R` can land while a
    // poll is out. Once a newer answer has landed, applying this one would
    // move the data backwards (rule 10).
    if !app.github_freshness.accept_answer(attempt) {
        // Its reading may still lower `remaining` on the held token, never
        // switch it; its outcome changes no count and no not-before.
        app.observe_budget(reading.as_ref(), false, now);
        if is_latest {
            // A refresh waiting on this request is over all the same.
            app.settle_refresh_poll(std::iter::empty());
        }
        return;
    }
    app.observe_budget(reading.as_ref(), true, now);
    // Read before the outcome below replaces it: a recovery from `Offline`
    // refetches the rows the outage failed (fetch contract rule 6).
    let previous_outcome = app.github_freshness.outcome.clone();
    match result {
        Ok(fetch) => {
            app.github_freshness.succeeded(now);
            app.github_backoff.succeeded();
            let github::GithubFetch {
                repos: dataset,
                calendar,
                has_pr_counts: _,
                // The poll always bypasses the cache, so this is
                // network data by construction.
                origin: _,
                meta,
            } = *fetch;
            // Before the meta is replaced: the rows were built on the account
            // the meta held until now.
            if account_rebuild_needed(app.github_meta.account.as_deref(), meta.account.as_deref()) {
                app.account_rebuild_due = true;
            }
            // The dataset is replaced below, so its coverage is too: the next
            // window apply draws its note from what this poll covered.
            app.github_meta = meta;
            // The calendar rides the same payload as the repos, so
            // a poll that updated CI also refreshed the year at no
            // extra cost. A payload predating the field leaves the
            // last good calendar in place rather than blanking it.
            // A payload predating the field leaves the last good
            // calendar in place rather than blanking it.
            if calendar.is_some() {
                app.github_calendar = calendar;
            }
            // Overwrite only the GitHub-owned fields: the local
            // git state is the sweep's business, not this poll's.
            // Only on the row the load made the carrier
            // (`carries`), so a same-named row of another owner, or
            // a local-only checkout, never takes these.
            // A carrier row the poll did not return keeps its values and is
            // marked as from an earlier refresh; one it returned loses the
            // mark (design 4.7).
            for repo in &mut app.repos {
                let entry = dataset.iter().find(|g| carries(repo, g));
                if let Some(gh) = entry {
                    repo.apply_github(gh);
                }
                if let Some(key) = app::carrier_key(repo) {
                    if entry.is_some() {
                        app.not_in_last_poll.remove(&key);
                    } else {
                        app.not_in_last_poll.insert(key);
                    }
                }
            }
            // One API call just told us which remotes moved, so
            // fetch only those instead of waiting for the next
            // blanket sweep to fetch all of them.
            let mut moved = remotes_that_moved(&app.dataset, &dataset, &app.repos);
            // The outage's failures ride the same batch, each row once.
            for idx in
                recovery_refetch(&previous_outcome, &app.github_freshness.outcome, &app.repos)
            {
                if !moved.contains(&idx) {
                    moved.push(idx);
                }
            }
            // The new sizes against each row's baseline, before choosing.
            assess_size_holds(&mut app.repos, &dataset, app.fetch_max_bytes);
            // A row held after a timeout waits out its hold (FZ-34), and one
            // the fetch cap holds waits for `f` (FZ-35b).
            moved.retain(|&idx| {
                let repo = &app.repos[idx];
                repo.fetch.size_hold().is_none()
                    && repo
                        .path
                        .as_deref()
                        .is_none_or(|path| !app.fetch_held(path, now))
            });
            for &idx in &moved {
                start_single_sweep(app, tx, idx, true, true, author.clone());
            }
            // A manual `r` keeps spinning until these land.
            let fetched_paths: Vec<String> = moved
                .iter()
                .filter_map(|&idx| app.repos[idx].path.clone())
                .collect();
            if is_latest {
                app.settle_refresh_poll(fetched_paths);
            }
            app.dataset = dataset;
        }
        Err(e) => {
            app.set_status(
                format!("GitHub refresh failed: {}", e.failure.message),
                true,
            );
            app.github_attempt_failed(&e.failure, now);
            app.github_freshness.failed(e.failure, now);
        }
    }
    // A failed poll fetched nothing, so a refresh waiting on it
    // is over. No-op when the Ok arm already settled it.
    if is_latest {
        app.settle_refresh_poll(std::iter::empty());
    }
    app.needs_redraw = true;
}

/// The rows fetched once because the GitHub poll recovered (fetch contract
/// rule 6): when `previous` failed as `Offline` and `current` succeeded,
/// every row with a path whose last fetch failed as `Unreachable` or
/// `TimedOut`. Any other pair selects nothing, so the refetch never repeats
/// on its own, and a row marked for any other reason is left alone.
fn recovery_refetch(
    previous: &app::Outcome,
    current: &app::Outcome,
    repos: &[RepoStatus],
) -> Vec<usize> {
    let recovered = matches!(
        previous,
        app::Outcome::Failed(f) if f.kind == github::FailureKind::Offline
    ) && *current == app::Outcome::Success;
    if !recovered {
        return Vec::new();
    }
    repos
        .iter()
        .enumerate()
        .filter(|(_, r)| r.path.is_some())
        .filter(|(_, r)| {
            r.fetch.failure().is_some_and(|f| {
                matches!(
                    f.kind,
                    git::FetchFailureKind::Unreachable | git::FetchFailureKind::TimedOut
                )
            })
        })
        .map(|(i, _)| i)
        .collect()
}

/// Whether an applied successful poll on account `poll` must rebuild rows
/// built on account `rows` (design 4.7, O5). Only a poll whose account is
/// known decides: it rebuilds when the rows' account differs or is unknown
/// (a launch from the legacy file or under the signed-out rule). A poll with
/// no account never does. A token rotation on one account changes the key
/// and costs one rebuild from that account's own data, which is accepted.
fn account_rebuild_needed(rows: Option<&str>, poll: Option<&str>) -> bool {
    poll.is_some() && rows != poll
}

/// Rebuild the rows from the dataset in memory after an account change,
/// once nothing else is loading: through the rebuild from memory, with no
/// request, no `git fetch`, no `window.toml` write and no "Window:" status.
/// While a load runs the flag waits; a load that fetched clears it.
fn start_account_rebuild(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
) {
    if !app.account_rebuild_due || app.loading {
        return;
    }
    app.account_rebuild_due = false;
    app.loading = true;
    spawn_memory_rebuild(tx, app, args, config, repo_dir, false);
}

/// Give up on a background poll that has not answered within `limit`.
///
/// Recorded as a timed-out failure, which also advances the retry clock: the
/// in-flight flag used to be dropped with nothing else changed, so the next
/// tick started a second request while the first was still open (decision
/// D4). The request itself is not cancelled; that is FZ-11.
fn expire_stuck_github_poll(
    app: &mut App,
    limit: std::time::Duration,
    now: chrono::DateTime<chrono::Local>,
) {
    if let Some(started) = app.github_poll_started
        && started.elapsed() > limit
    {
        app.github_poll_inflight = false;
        app.github_poll_started = None;
        app.refresh_awaits_poll = false;
        app.github_freshness.gave_up(limit, now);
        // A give-up is a `TimedOut` failure and climbs the ladder like one
        // (transition 5).
        if let app::Outcome::Failed(failure) = app.github_freshness.outcome.clone() {
            app.github_attempt_failed(&failure, now);
        }
        app.needs_redraw = true;
    }
}

/// Whether the tick should start a background GitHub poll now.
///
/// With watch on, once per `github_poll_secs` after the last attempt,
/// whatever it ended in, or sooner for a detected push while polls succeed.
/// With watch off, only the one attempt a cache-served launch is owed
/// (decision D2): the data on screen came off the disk and nothing has
/// confirmed it yet.
fn github_poll_due(
    app: &App,
    config: &config::Config,
    now: chrono::DateTime<chrono::Local>,
) -> bool {
    if app.dirty_mode || app.loading || app.github_poll_inflight {
        return false;
    }
    let freshness = &app.github_freshness;
    if !app.watch_mode {
        return freshness.awaits_first_attempt();
    }
    // Beside the interval, not inside it: a rate limit's reset, a
    // `retry-after`, a backoff step or a pause at the reserve.
    let held = github_not_before(app, config).is_some_and(|t| now < t);
    // A detected push jumps the queue: it is the one moment CI is about to
    // change with nothing `Pending` yet to notice. Not while polls fail, where
    // the next attempt waits out at least a full interval (rule 6), nor while
    // the budget slows polling; the next due poll serves it then.
    let slowed = app.budget_slowed(
        configured_poll_secs(config),
        github::poll_cost(config.repo_limit),
    );
    let push = app.push_detected && !freshness.is_failing() && !slowed;
    !held && (push || freshness.retry_due(now, github_poll_secs(app, config)))
}

/// The configured poll interval, floored.
fn configured_poll_secs(config: &config::Config) -> i64 {
    config.github_poll_interval.max(GITHUB_POLL_MIN) as i64
}

/// The earliest the next automatic attempt may go besides the interval: the
/// later of the retry ladder's not-before and a pause at the reserve.
fn github_not_before(
    app: &App,
    config: &config::Config,
) -> Option<chrono::DateTime<chrono::Local>> {
    let ladder = app.github_backoff.not_before(configured_poll_secs(config));
    let pause = match app.github_pace(github::poll_cost(config.repo_limit)) {
        app::Pace::Paused { until } => Some(until),
        _ => None,
    };
    ladder.max(pause)
}

/// `App::github_limit_until` under `config`.
fn github_limit_until(
    app: &App,
    config: &config::Config,
    now: chrono::DateTime<chrono::Local>,
) -> Option<chrono::DateTime<chrono::Local>> {
    app.github_limit_until(
        configured_poll_secs(config),
        github::poll_cost(config.repo_limit),
        now,
    )
}

/// The status line for a refresh that sent nothing because of a known limit.
/// Provisional words; FZ-10c settles them.
fn limited_status(until: chrono::DateTime<chrono::Local>) -> String {
    format!("GitHub rate limited, resumes at {}", until.format("%H:%M"))
}

/// The status line for `R` ignored because a refresh is already under way
/// (A7, and A6 with a poll in flight). One status for both, so the key does
/// not look dead. Provisional words until Andreas settles them (O1).
const REFRESH_RUNNING: &str = "Already refreshing";

/// What `R` does, decided before anything is spawned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReloadAction {
    /// A full load against GitHub, as `R` always did.
    Load,
    /// A load is running, or a poll is in flight under a known limit: no
    /// second load, no request.
    AlreadyRunning,
    /// Under a known limit: rebuild from the dataset in memory, no request.
    FromMemory,
    /// Under a known limit with nothing in memory: only say when GitHub
    /// resumes.
    ResumesOnly(chrono::DateTime<chrono::Local>),
}

fn reload_action(
    app: &App,
    config: &config::Config,
    now: chrono::DateTime<chrono::Local>,
) -> ReloadAction {
    if app.loading {
        return ReloadAction::AlreadyRunning;
    }
    let Some(until) = github_limit_until(app, config, now) else {
        return ReloadAction::Load;
    };
    // A poll started before the limit was known can land newer rows; a
    // rebuild from the older memory copy must not land after it.
    if app.github_poll_inflight {
        ReloadAction::AlreadyRunning
    } else if app.dataset.is_empty() {
        ReloadAction::ResumesOnly(until)
    } else {
        ReloadAction::FromMemory
    }
}

/// `R`: a full reload, or under a known limit a rebuild from memory.
fn reload_key(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    now: chrono::DateTime<chrono::Local>,
) {
    // A manual reload lifts every timeout hold (FZ-34).
    app.fetch_backoff.clear();
    match reload_action(app, config, now) {
        ReloadAction::AlreadyRunning => app.set_status(REFRESH_RUNNING.to_string(), false),
        ReloadAction::ResumesOnly(until) => app.set_status(limited_status(until), false),
        ReloadAction::FromMemory => {
            app.loading = true;
            app.set_status("Reloading without GitHub…".to_string(), false);
            spawn_memory_rebuild(tx, app, args, config, repo_dir, true);
        }
        ReloadAction::Load => {
            app.loading = true;
            app.set_status("Reloading…".to_string(), false);
            spawn_load(
                tx,
                args,
                config,
                repo_dir,
                app.window,
                LoadKind::Reload,
                true,
                &app.fetches_in_flight,
                size_held_rows(&app.repos),
            );
        }
    }
}

/// `r`: the local sweep, and a poll unless GitHub is known to refuse it, in
/// which case the status says when it resumes and nothing is sent.
fn refresh_key(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    args: &Args,
    config: &config::Config,
    now: chrono::DateTime<chrono::Local>,
) {
    // A manual refresh lifts every timeout hold (FZ-34).
    app.fetch_backoff.clear();
    let limited = github_limit_until(app, config, now);
    match limited {
        Some(until) => app.set_status(limited_status(until), false),
        None => app.set_status("Refreshing…".to_string(), false),
    }
    start_sweep(app, tx, false, args.author.clone(), true);
    if limited.is_none() {
        start_github_poll(app, tx, config);
    }
    // Already in flight from the watch timer counts too: its answer
    // is the one this refresh is waiting for.
    app.refresh_awaits_poll = app.github_poll_inflight;
}

/// Indices into `repos` whose remote moved between two GitHub polls.
///
/// This inverts the cost of noticing someone else's push. Discovering it by
/// fetching means a network round trip per remote, every sweep, almost always
/// to learn that nothing changed. One GraphQL call returns `pushed_at` for
/// every repo at once, so the poll can say *which* remotes moved and only
/// those need fetching.
///
/// Note this is the opposite conclusion to per-repo CI queries, for a
/// consistent reason: GraphQL batches cheaply, so ask about everything at
/// once; git fetch is priced per remote, so only fetch where you know there
/// is news.
///
/// A repo missing from the previous dataset yields nothing: on the first poll
/// of a session there is no baseline, and treating "unknown" as "moved" would
/// fetch the entire fleet at once. Repos with no local clone are skipped too,
/// since there is nothing to fetch.
///
/// Only the carrier of a moved remote is fetched (`carries`), the row the poll
/// writes. An extra checkout of the same remote is not; `F` still reaches it.
///
/// The previous entry is the side `names_remote` matches from, so an owned
/// one is only ever the same owner's push, and one without an owner (a
/// launch from a payload without `login`) still matches by name. That keeps
/// the legacy baseline: unmatched, a push since it would read as unknown and
/// never be fetched, since the next poll compares against owned data.
fn remotes_that_moved(
    previous: &[github::GithubRepo],
    current: &[github::GithubRepo],
    repos: &[RepoStatus],
) -> Vec<usize> {
    current
        .iter()
        .filter(|now| {
            previous
                .iter()
                .find(|was| names_remote(was.owner.as_deref(), &was.name, now))
                .is_some_and(|was| now.pushed_at > was.pushed_at)
        })
        .filter_map(|moved| {
            repos
                .iter()
                .position(|r| r.path.is_some() && carries(r, moved))
        })
        // Each row once, in the order its first moved entry came: two entries
        // can resolve to one row (an owner-less legacy row carries every
        // same-named entry), and a second index is a second single sweep.
        .fold(Vec::new(), |mut rows, idx| {
            if !rows.contains(&idx) {
                rows.push(idx);
            }
            rows
        })
}

/// Whether `entry` is the GitHub repo a row or clone known as `owner`/`name`
/// stands for. Names compare case-insensitively, as GitHub does. With an
/// owner, only an entry of that owner (case-insensitive) matches; an entry
/// without one does not. With no owner, the name alone decides, whatever the
/// entry's owner: that is the legacy path of a payload without `login` (m2
/// and D1 in `docs/repository-identity-design.md`). Decided through
/// `model::identity_key`.
fn names_remote(owner: Option<&str>, name: &str, entry: &github::GithubRepo) -> bool {
    let ours = model::identity_key(owner, name);
    let theirs = model::identity_key(entry.owner.as_deref(), &entry.name);
    match owner {
        Some(_) => ours == theirs,
        None => ours.1 == theirs.1,
    }
}

/// Whether `repo` is the row that carries `entry`'s GitHub fields, the mark
/// the load left (D6, section 3.5 of `docs/repository-identity-design.md`).
/// The poll and `remotes_that_moved` follow it; nothing is re-chosen here.
///
/// A Primary, Elsewhere or NotCloned row with an owner matches only an entry
/// of that owner and name, so an entry without an owner never writes it. One
/// with no owner matches any entry of its name (legacy, `names_remote`). A
/// LocalOnly row matches only when the load made it a carrier by giving it
/// an owner, and then only an entry of that owner named like the repo its
/// stored `origin` names, since its `name` is its folder's. A LocalOnly row
/// is never matched by name: that is how a remote-less checkout picked up a
/// stranger's CI on the first poll.
fn carries(repo: &RepoStatus, entry: &github::GithubRepo) -> bool {
    if repo.location != RepoLocation::LocalOnly {
        return names_remote(repo.owner.as_deref(), &repo.name, entry);
    }
    let (Some(owner), Some(url)) = (repo.owner.as_deref(), repo.remote_url.as_deref()) else {
        return false;
    };
    let Some(origin) = model::RemoteRef::parse(url) else {
        return false;
    };
    names_remote(Some(owner), &origin.name, entry)
}

/// Resolve watch mode at startup: an explicit flag wins, otherwise the config
/// decides. Extracted so the precedence is testable -- a default that silently
/// fails to apply is invisible until someone wonders why nothing refreshes.
fn resolve_watch(flag_watch: bool, flag_no_watch: bool, config_watch: bool) -> bool {
    if flag_watch {
        true
    } else if flag_no_watch {
        false
    } else {
        config_watch
    }
}

/// Seconds to wait before the next GitHub poll.
///
/// Drops to the floor while any repo is `Pending`, which is exactly when a
/// human is watching the screen for a run to finish, and relaxes to the
/// configured interval when nothing is in flight. Costs nothing at idle.
///
/// Not while polls fail (decision D3): a `Pending` frozen by a failing poll
/// says nothing about what CI is doing now, and the floor would only retry a
/// dead network four times a minute. Nor while the budget slows polling
/// (A2): then the pace, when longer than the configured interval, is the
/// wait. A backoff step, a limit and a pause are not here but in
/// `github_not_before`, which `github_poll_due` checks beside this.
fn github_poll_secs(app: &App, config: &config::Config) -> i64 {
    let configured = configured_poll_secs(config);
    let cost = github::poll_cost(config.repo_limit);
    let pace = match app.github_pace(cost) {
        app::Pace::Every(secs) => secs,
        app::Pace::Unknown | app::Pace::Paused { .. } => 0,
    };
    // A row the last poll did not return holds a `Pending` frozen at an
    // earlier refresh, which says nothing about CI now (design 4.7).
    let any_pending = app
        .repos
        .iter()
        .any(|r| r.ci_status == model::CiStatus::Pending && !app.is_not_in_last_poll(r));
    let floored =
        any_pending && !app.github_freshness.is_failing() && !app.budget_slowed(configured, cost);
    let interval = if floored {
        GITHUB_POLL_MIN as i64
    } else {
        configured
    };
    interval.max(pace)
}

/// Kick off a GitHub re-poll, taking the in-flight lock. No-op if one is
/// already running, so mashing `r` cannot stack polls. `R` does not come
/// through here: it is a full load, and `reload_action` ignores it while a
/// load runs (A7), which is what keeps it from stacking requests.
fn start_github_poll(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    config: &config::Config,
) {
    if let Some(attempt) = claim_github_poll(app) {
        spawn_github_poll(tx, config, attempt);
    }
}

/// The tick's CI work: give up on a stuck poll, then start the one that is
/// due. Both only while pane 2 shows `ci` (`App::ci_wanted`); elsewhere,
/// `info` included, the CI data layer is left alone, and what it holds
/// stays for the next visit.
fn ci_tick(
    app: &mut App,
    now: chrono::DateTime<chrono::Local>,
    instant: std::time::Instant,
) -> Option<ci::CiRequest> {
    if !app.ci_wanted() {
        return None;
    }
    app.expire_stuck_ci_polls(CI_POLL_STUCK_AFTER, instant, now);
    ci_poll_request(app, now, instant)
}

/// The CI poll the tick should start for the selected repo, with its lock
/// taken, or `None`. Nothing unless pane 2 shows `ci`, nothing under `-d`
/// or on a row without a GitHub identity; otherwise `ci::poll_due` decides,
/// from the cursor's dwell and the repo's cadence.
fn ci_poll_request(
    app: &mut App,
    now: chrono::DateTime<chrono::Local>,
    instant: std::time::Instant,
) -> Option<ci::CiRequest> {
    if !app.ci_wanted() || app.dirty_mode {
        return None;
    }
    let target = app.selected_repo().and_then(ci::target);
    let settled_for = app.ci_settled_for(target.as_ref().map(|(key, ..)| key), instant);
    let (key, owner, name) = target?;
    // The poll reads the highlighted run's jobs (`CiState::expanded`).
    app.sync_ci_inspector(instant);
    ci::poll_due(&app.ci_cadence(&key, settled_for, now))
        .then(|| app.claim_ci_poll(key, owner, name))
}

/// Run a claimed CI poll in the background and send its answer back.
fn spawn_ci_poll(tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>, request: ci::CiRequest) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let answer = ci::poll(request).await;
        let _ = tx.send(AppEvent::Ci(Box::new(answer)));
    });
}

/// Run a claimed inspector fetch in the background and send its answer back.
fn spawn_ci_detail(tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>, request: ci::DetailRequest) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let answer = ci::fetch_detail(request).await;
        let _ = tx.send(AppEvent::CiDetail(Box::new(answer)));
    });
}

/// Walk a repo for the `notes` view off the UI thread and send the tree back.
fn spawn_notes_walk(tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>, walk: app::NotesWalk) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let root = walk.root.clone();
        let result = tokio::task::spawn_blocking(move || notes::walk(&root))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| e.to_string()));
        let _ = tx.send(AppEvent::NotesWalked { walk, result });
    });
}

/// Read the highlighted note off the UI thread and send its body back with
/// its metadata for the card (FZ-62), then its last commit from a bounded
/// `git log`, "untracked" when git prints nothing. Two answers, body first:
/// for a file git never saw it walks the whole history, and the reader
/// shows the body without waiting for that.
fn spawn_note_read(tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>, read: app::NoteRead) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let path = read.path.clone();
        let read_both = tokio::task::spawn_blocking(move || {
            (notes::read_body(&path), notes::file_facts(&path))
        })
        .await;
        let (text, facts) = match read_both {
            Ok((text, facts)) => (text, facts),
            Err(e) => (Err(e.to_string()), notes::NoteFacts::default()),
        };
        let _ = tx.send(AppEvent::NoteRead {
            read: read.clone(),
            text,
            facts,
        });
        let rel = read.path.strip_prefix(&read.root).unwrap_or(&read.path);
        let rel = rel.to_string_lossy();
        // Literal, so a name with `:` or glob characters is a path, not
        // pathspec magic.
        let args = [
            "--literal-pathspecs",
            "log",
            "-1",
            "--format=%cI",
            "--",
            &rel,
        ];
        let last = git::run_git(&read.root, &args)
            .await
            .filter(|out| out.status.success())
            .map_or(notes::LastCommit::Untracked, |out| {
                notes::last_commit_from(&String::from_utf8_lossy(&out.stdout))
            });
        let _ = tx.send(AppEvent::NoteCommit { read, last });
    });
}

/// Run a slot digest's `git log --since` in the background (FZ-45) and send
/// the commits back: never on the UI thread.
fn spawn_digest_log(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    request: digest::LogRequest,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let commits = git::commits_since(&request.dir, request.key.cutoff).await;
        let _ = tx.send(AppEvent::DigestLog {
            path: request.path,
            key: request.key,
            commits,
        });
    });
}

/// Take the in-flight lock and an attempt id for a new poll, or `None` when
/// one is already running or `-d` has no GitHub to poll.
fn claim_github_poll(app: &mut App) -> Option<u64> {
    if app.github_poll_inflight || app.dirty_mode {
        return None;
    }
    app.github_poll_inflight = true;
    app.github_poll_started = Some(std::time::Instant::now());
    Some(app.github_freshness.begin_attempt())
}

/// Snapshot the repos that have a local path, as sweep targets.
fn sweep_targets(app: &App, only: Option<usize>) -> Vec<SweepTarget> {
    let to_target = |r: &RepoStatus| {
        let display = r.path.clone()?;
        let expanded = shellexpand::tilde(&display).to_string();
        Some(SweepTarget {
            display_path: display,
            fs_path: std::path::PathBuf::from(expanded),
            old: Box::new(r.clone()),
        })
    };
    match only {
        Some(i) => app.repos.get(i).and_then(to_target).into_iter().collect(),
        None => app.repos.iter().filter_map(to_target).collect(),
    }
}

/// Kick off a background sweep of every repo with a local path. Each repo is
/// fetched (optionally), rebuilt in full, and streamed back as RepoUpdated;
/// SweepDone closes the books. No-op if a sweep is already running.
/// `manual` marks a sweep the user asked for by keypress. Only those announce
/// themselves in the header; a watch-timer sweep runs silently and simply
/// updates the rows when it lands.
/// What a sweep needs in order to fill the local contribution histogram.
/// `None` means the strip is not showing local data, so the year-long `git
/// log` per repo is skipped entirely: it is the most expensive probe in the
/// sweep and nothing on screen would read it.
#[derive(Clone)]
struct ContribProbe {
    /// Grid start, resolved once so every repo in one sweep shares a calendar.
    start: chrono::NaiveDate,
    /// Author regex from config, or None to resolve each repo's `user.email`.
    author: Option<String>,
}

/// Whether this sweep should pay for the per-repo commit histogram.
fn contrib_probe(app: &App) -> Option<ContribProbe> {
    (app.contributions == app::ContributionsMode::Local).then(|| ContribProbe {
        start: contributions::grid_start(chrono::Local::now().date_naive()),
        author: app.contrib_author.clone(),
    })
}

fn start_sweep(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    do_fetch: bool,
    author: Option<String>,
    manual: bool,
) {
    if app.sweep_started.is_some() {
        return;
    }
    let targets = sweep_targets(app, None);
    if targets.is_empty() {
        return;
    }
    app.sweep_started = Some(std::time::Instant::now());
    app.sweep_manual = manual;
    // `F`: report its fetches when the last one lands. A watch sweep fetches
    // silently, and `r` does not fetch here at all. A row the fetch cap
    // holds is only rebuilt and lands `NotAttempted`, so it is not awaited
    // (FZ-35b); a row whose fetch is already running is, since that fetch
    // lands with a real outcome.
    if do_fetch && manual {
        let held = size_held_paths(app, &targets);
        app.manual_fetches.expect(
            targets
                .iter()
                .map(|t| t.display_path.clone())
                .filter(|path| !held.contains(path)),
        );
    }
    let plan = plan_sweep_fetch(app, &targets, do_fetch, FetchAsk::for_full_sweep(manual));
    let probe = contrib_probe(app);
    spawn_sweep(
        tx.clone(),
        targets,
        do_fetch,
        &plan,
        author,
        true,
        probe,
        &app.fetches_in_flight,
    );
}

/// What a sweep hands `spawn_sweep` about fetching: the targets it rebuilds
/// without a fetch, and the byte cap the rest run under (FZ-35b).
struct SweepFetchPlan {
    no_fetch: HashSet<String>,
    max_bytes: Option<u64>,
}

/// The fetch half of a sweep asked for by `ask`: claims the targets it
/// fetches (`fetch_skips_for`) and picks the cap (`FetchAsk::cap`).
fn plan_sweep_fetch(
    app: &mut App,
    targets: &[SweepTarget],
    do_fetch: bool,
    ask: FetchAsk,
) -> SweepFetchPlan {
    SweepFetchPlan {
        no_fetch: fetch_skips_for(app, targets, do_fetch, ask),
        max_bytes: ask.cap(app.fetch_max_bytes),
    }
}

/// Who asked for a fetch, which decides the holds it honours and whether
/// the fetch cap bounds it (FZ-34, FZ-35b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FetchAsk {
    /// The watch timer, a moved remote, the recovery refetch: skips a
    /// timeout hold and a size hold, and is capped.
    Automatic,
    /// `F`: fetches through a timeout hold, but skips a size hold and is
    /// capped. Nothing but a deliberate per-row key pulls a big repo.
    All,
    /// `f` on one row: the permission. Fetches through both holds, uncapped.
    One,
}

impl FetchAsk {
    /// A full sweep: `F` when `manual`, else the watch timer.
    fn for_full_sweep(manual: bool) -> Self {
        if manual {
            FetchAsk::All
        } else {
            FetchAsk::Automatic
        }
    }

    /// A single-repo sweep: a moved remote or the recovery refetch when
    /// `automatic`, else `f` (a single sweep that does not fetch is no ask).
    fn for_single_sweep(automatic: bool) -> Self {
        if automatic {
            FetchAsk::Automatic
        } else {
            FetchAsk::One
        }
    }

    /// The byte cap this fetch runs under, given the configured one.
    fn cap(self, configured: Option<u64>) -> Option<u64> {
        match self {
            FetchAsk::One => None,
            FetchAsk::Automatic | FetchAsk::All => configured,
        }
    }
}

/// The targets an automatic sweep rebuilds without fetching at `now`: those
/// a recent fetch timeout holds (`App::fetch_held`, FZ-34).
fn held_fetch_paths(
    app: &App,
    targets: &[SweepTarget],
    now: chrono::DateTime<chrono::Local>,
) -> std::collections::HashSet<String> {
    targets
        .iter()
        .filter(|t| app.fetch_held(&t.display_path, now))
        .map(|t| t.display_path.clone())
        .collect()
}

/// The display paths of the rows the fetch cap holds until `f` (FZ-35b).
fn size_held_rows(repos: &[RepoStatus]) -> HashSet<String> {
    repos
        .iter()
        .filter(|r| r.fetch.size_hold().is_some())
        .filter_map(|r| r.path.clone())
        .collect()
}

/// The targets whose row the fetch cap holds until `f` (FZ-35b).
fn size_held_paths(app: &App, targets: &[SweepTarget]) -> HashSet<String> {
    let held = size_held_rows(&app.repos);
    targets
        .iter()
        .filter(|t| held.contains(&t.display_path))
        .map(|t| t.display_path.clone())
        .collect()
}

/// `f`: fetch the selected repo. Manual, so a timeout hold does not stop it
/// (FZ-34), and uncapped, so it is how a repo the fetch cap holds is pulled
/// in full (FZ-35b); a fetch of it already running stops it.
fn fetch_key(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    author: Option<String>,
) {
    if let Some(repo) = app.selected_repo().cloned()
        && repo.path.is_some()
        && let Some(idx) = app.selected_repo_index()
    {
        app.set_status(format!("\u{27f3} Fetching {}…", repo.name), false);
        start_single_sweep(app, tx, idx, true, false, author);
        app.manual_fetches.expect(repo.path.clone());
    }
}

/// `fetch_skips_for` as the FZ-34 tests ask it: `automatic`, or `f`.
#[cfg(test)]
fn fetch_skips(
    app: &mut App,
    targets: &[SweepTarget],
    do_fetch: bool,
    automatic: bool,
) -> std::collections::HashSet<String> {
    let ask = if automatic {
        FetchAsk::Automatic
    } else {
        FetchAsk::One
    };
    fetch_skips_for(app, targets, do_fetch, ask)
}

/// The targets a sweep rebuilds without fetching, claiming the rest for a
/// fetch (FZ-34). Every fetching sweep skips a path whose fetch is still
/// running (R5); an automatic one (the watch timer, a moved remote, the
/// recovery refetch) also skips a path held after a timeout (R3), where `f`
/// and `F` fetch it as asked. Every one but `f` skips a repo the fetch cap
/// holds (FZ-35b). A sweep that does not fetch claims nothing.
fn fetch_skips_for(
    app: &mut App,
    targets: &[SweepTarget],
    do_fetch: bool,
    ask: FetchAsk,
) -> std::collections::HashSet<String> {
    if !do_fetch {
        return std::collections::HashSet::new();
    }
    let mut skips = if ask == FetchAsk::Automatic {
        held_fetch_paths(app, targets, chrono::Local::now())
    } else {
        std::collections::HashSet::new()
    };
    if ask != FetchAsk::One {
        skips.extend(size_held_paths(app, targets));
    }
    let started = std::time::Instant::now();
    for target in targets {
        if !skips.contains(&target.display_path) && !app.claim_fetch(&target.display_path, started)
        {
            skips.insert(target.display_path.clone());
        }
    }
    skips
}

/// Sweep a single repo (the `f` fetch and the refresh after a branch switch
/// or delete).
/// Does not take the sweep lock: it is one repo, and it must work while a
/// full sweep is running.
fn start_single_sweep(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    index: usize,
    do_fetch: bool,
    automatic: bool,
    author: Option<String>,
) {
    let targets = sweep_targets(app, Some(index));
    if targets.is_empty() {
        return;
    }
    let plan = plan_sweep_fetch(
        app,
        &targets,
        do_fetch,
        FetchAsk::for_single_sweep(automatic),
    );
    spawn_sweep(
        tx.clone(),
        targets,
        do_fetch,
        &plan,
        author,
        false,
        contrib_probe(app),
        &app.fetches_in_flight,
    );
}

/// At most one `AppEvent::FetchProgress` per `PROGRESS_EVENT_GAP` per fetch:
/// git prints an update on every percent and every second, more than a row
/// can show.
const PROGRESS_EVENT_GAP: std::time::Duration = std::time::Duration::from_millis(500);

/// Decides which of one fetch's progress updates go to the app.
#[derive(Debug, Default)]
struct ProgressThrottle {
    last_sent: Option<std::time::Instant>,
}

impl ProgressThrottle {
    /// Whether an update at `now` may be sent; it is counted as sent if so.
    fn admit(&mut self, now: std::time::Instant) -> bool {
        let due = self
            .last_sent
            .is_none_or(|last| now.saturating_duration_since(last) >= PROGRESS_EVENT_GAP);
        if due {
            self.last_sent = Some(now);
        }
        due
    }
}

/// Run one fetch under its claim (`app::FetchClaims`): taken at the moment
/// the caller's fetch permit was granted, before git starts, and kept alive
/// by each progress update. `NotAttempted` without running git when a newer
/// claim holds the path. With `tx`, progress goes to the row, throttled.
#[cfg(test)]
async fn fetch_under_claim(
    claims: &app::FetchClaims,
    display_path: &str,
    fs_path: &Path,
    token: u64,
    tx: Option<&tokio::sync::mpsc::UnboundedSender<AppEvent>>,
) -> git::FetchOutcome {
    fetch_under_claim_capped(claims, display_path, fs_path, token, tx, None).await
}

/// `fetch_under_claim`, cut as `TooBig` past `max_bytes` received (FZ-35b).
async fn fetch_under_claim_capped(
    claims: &app::FetchClaims,
    display_path: &str,
    fs_path: &Path,
    token: u64,
    tx: Option<&tokio::sync::mpsc::UnboundedSender<AppEvent>>,
    max_bytes: Option<u64>,
) -> git::FetchOutcome {
    if !claims.start(display_path, token, std::time::Instant::now()) {
        return git::FetchOutcome::NotAttempted;
    }
    let mut throttle = ProgressThrottle::default();
    git::fetch_reporting(fs_path, max_bytes, |progress| {
        let now = std::time::Instant::now();
        claims.touch(display_path, token, now);
        if let Some(tx) = tx
            && throttle.admit(now)
        {
            let _ = tx.send(AppEvent::FetchProgress {
                path: display_path.to_string(),
                progress,
            });
        }
    })
    .await
}

/// The shared sweep body: fetch (capped) then rebuild (capped), streaming
/// each repo back as it lands so the list updates progressively. A target
/// whose display path is in the plan's `no_fetch` is rebuilt without a
/// fetch; every other one fetches under the claim the plan reserved for it,
/// cut as `TooBig` past the plan's `max_bytes` (`plan_sweep_fetch`, FZ-35b).
#[allow(clippy::too_many_arguments)] // one call spine; a params struct would only rename the problem
fn spawn_sweep(
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    targets: Vec<SweepTarget>,
    do_fetch: bool,
    plan: &SweepFetchPlan,
    author: Option<String>,
    report_done: bool,
    probe: Option<ContribProbe>,
    claims: &app::FetchClaims,
) {
    let max_bytes = plan.max_bytes;
    let targets: Vec<(SweepTarget, Option<u64>)> = targets
        .into_iter()
        .map(|t| {
            let fetch = do_fetch && !plan.no_fetch.contains(&t.display_path);
            let claim = if fetch {
                claims.token(&t.display_path)
            } else {
                None
            };
            (t, claim)
        })
        .collect();
    let claims = claims.clone();
    tokio::spawn(async move {
        let probe_sem = std::sync::Arc::new(tokio::sync::Semaphore::new(probe_concurrency()));
        let fetch_sem = std::sync::Arc::new(tokio::sync::Semaphore::new(FETCH_CONCURRENCY));
        let mut set = tokio::task::JoinSet::new();
        for (target, claim) in targets {
            let probe_sem = probe_sem.clone();
            let fetch_sem = fetch_sem.clone();
            let tx = tx.clone();
            let author = author.clone();
            let probe = probe.clone();
            let claims = claims.clone();
            set.spawn(async move {
                let fetch = match claim {
                    Some(token) => {
                        let _permit = fetch_sem.acquire().await;
                        fetch_under_claim_capped(
                            &claims,
                            &target.display_path,
                            &target.fs_path,
                            token,
                            Some(&tx),
                            max_bytes,
                        )
                        .await
                    }
                    None => git::FetchOutcome::NotAttempted,
                };
                let _permit = probe_sem.acquire().await;
                let status = rebuild_repo_status(&target, author.as_deref(), probe.as_ref()).await;
                let _ = tx.send(AppEvent::RepoUpdated {
                    path: target.display_path.clone(),
                    status: Box::new(status),
                    fetch,
                    claim,
                });
            });
        }
        while set.join_next().await.is_some() {}
        if report_done {
            let _ = tx.send(AppEvent::SweepDone {
                at: chrono::Local::now(),
            });
        }
    });
}

/// Whether a sweep may reuse the gated probes: only when it has a key and the
/// key equals the snapshot's. A `None` key (a failed query, an unborn HEAD)
/// never counts as unchanged, not even against another `None`.
fn key_unchanged(new: &Option<git::WorkKey>, old: &Option<git::WorkKey>) -> bool {
    new.is_some() && new == old
}

/// Full per-repo rebuild. Fields the sweep does not probe are copied from the
/// snapshot only to complete the struct; `RepoStatus::apply_local_sweep`
/// discards them when the result lands.
/// Two-tier: the cheap probes always run, the expensive ones only when the
/// repo's WorkKey says something actually moved.
async fn rebuild_repo_status(
    target: &SweepTarget,
    author: Option<&str>,
    probe: Option<&ContribProbe>,
) -> RepoStatus {
    let path = &target.fs_path;
    let old = &target.old;

    // Cheap tier: always current. Untracked files never touch the WorkKey,
    // so dirty_count must not be skipped. Both ref queries feed the key, and
    // a failed one stays a failure there rather than an empty list.
    let local = git::try_branch_statuses(path).await;
    let dirty = git::dirty_count(path).await;
    let other = git::try_ref_listing(path).await;
    let key = git::work_key_from(path, local.as_ref().ok(), other.as_ref().ok()).await;
    let unchanged = key_unchanged(&key, &old.work_key);
    // Not gated: no ref moves when a worktree is added on an existing branch,
    // removed, or switched, so the key cannot see it. One stat, and a process
    // only for a clone that has linked worktrees.
    let worktrees = git::linked_worktrees(path).await;

    let branches = local.map(|l| l.statuses).unwrap_or_default();
    let remote = other.map(|o| o.remotes).unwrap_or_default();
    let branch_names: Vec<String> = branches.iter().map(|b| b.name.clone()).collect();
    let (total_behind, total_ahead) = branches
        .iter()
        .fold((0u32, 0u32), |(b, a), s| (b + s.behind, a + s.ahead));

    let (commits, reflog, graph, graph_expanded, stash_entries) = if unchanged {
        (
            old.commits.clone(),
            old.reflog.clone(),
            old.graph.clone(),
            old.graph_expanded.clone(),
            old.stash_entries.clone(),
        )
    } else {
        (
            git::commits(
                path,
                &format!("-{HISTORY_COUNT}"),
                true,
                author,
                &branch_names,
            )
            .await,
            git::reflog(path, HISTORY_COUNT).await,
            git::graph(path, &branch_names, HISTORY_COUNT).await,
            git::graph_expanded(path, &branch_names, HISTORY_COUNT).await,
            git::stash_list(path).await,
        )
    };

    // The local calendar is a year of log per repo, so it is gated twice:
    // only when the strip is actually showing it, and only when the
    // histogram's own key (grid start, configured author, ref signature,
    // HEAD) differs from the one the kept histogram was computed under. An
    // idle fleet costs nothing. Without the probe both are kept as they are,
    // so a later Local sweep still compares against what `daily` really saw.
    let wanted_daily_key = match (probe, &key) {
        (Some(probe), Some(key)) => Some(model::HistogramKey {
            start: probe.start,
            author: probe.author.clone(),
            ref_signature: key.ref_signature,
            head_sha: key.head_sha.clone(),
        }),
        _ => None,
    };
    let (daily, daily_key) = match probe {
        None => (old.daily.clone(), old.daily_key.clone()),
        Some(_)
            if old.daily.is_some()
                && wanted_daily_key.is_some()
                && wanted_daily_key == old.daily_key =>
        {
            (old.daily.clone(), old.daily_key.clone())
        }
        Some(probe) => {
            let author = match &probe.author {
                Some(a) => Some(a.clone()),
                None => git::user_email(path).await,
            };
            let daily = match author {
                Some(author) => git::daily_commits(path, probe.start, &author)
                    .await
                    .map(Box::new),
                // No identity configured: better to show nothing than to
                // count everyone's commits as if they were yours.
                None => None,
            };
            let daily_key = daily.as_ref().and(wanted_daily_key);
            (daily, daily_key)
        }
    };

    // Computed before the struct literal: the literal moves `branches` in, and
    // this borrows it.
    let remote_only_branches = git::unfetched_branches(&remote, &branches);

    RepoStatus {
        daily,
        daily_key,
        branches,
        dirty,
        behind: total_behind,
        ahead: total_ahead,
        remote_only_branches,
        remote_branches: remote,
        stash_count: stash_entries.len() as u32,
        stash_entries,
        commits,
        reflog,
        graph,
        graph_expanded,
        worktrees,
        work_key: key,
        ..(*old.clone())
    }
}

/// Re-poll GitHub for PR and CI data in the background, so badges stop being
/// frozen at whatever they were when fleetz launched.
fn spawn_github_poll(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    config: &config::Config,
    attempt: u64,
) {
    let tx = tx.clone();
    let repo_limit = config.repo_limit;
    // PR data is always fetched now, so the cache key no longer varies by it.
    let cache = cache::KeyedCache::new(repo_limit, github::HOST);
    tokio::spawn(async move {
        let result = github::fetch_github(repo_limit, &cache, github::CacheMode::Bypass).await;
        let _ = tx.send(AppEvent::Github {
            attempt,
            result: result.map(Box::new),
        });
    });
}

/// Load all repo data: GitHub API + local discovery + git status.
/// Returns the assembled view plus the unfiltered dataset so the TUI can
/// re-filter it live (time-window overlay) without re-fetching.
/// `size_held` are the display paths the fetch cap holds, which its fetches
/// skip (FZ-35b).
#[allow(clippy::too_many_arguments)] // one call spine; a params struct would only rename the problem
async fn load_repos(
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    mode: &Mode,
    window: Option<time::TimeWindow>,
    bypass_cache: bool,
    fetch_claims: &app::FetchClaims,
    size_held: &HashSet<String>,
) -> Result<
    (
        Vec<RepoStatus>,
        ViewNotes,
        Vec<github::GithubRepo>,
        Option<Box<contributions::Calendar>>,
        app::DatasetSource,
        Option<github::FetchMeta>,
    ),
    LoadError,
> {
    // Local scan roots: the primary repo_dir plus any configured extra dirs
    // (e.g. ~/.dotfiles), so repos outside the main tree still surface under
    // "local changes" -- the fleetz equivalent of repoz's EXTRA_DIRS.
    let mut scan_roots = vec![repo_dir.to_path_buf()];
    scan_roots.extend(config.resolved_extra_dirs());

    // Dirty mode: local only, no GitHub
    if *mode == Mode::Dirty {
        let header = "local changes".to_string();
        let local_paths = discovery::find_all_local(&scan_roots);

        // Probe every candidate in parallel behind a semaphore, then put the
        // survivors back in discovery order.
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(probe_concurrency()));
        let mut set = tokio::task::JoinSet::new();
        for (i, path) in local_paths.into_iter().enumerate() {
            let sem = sem.clone();
            let author = args.author.clone();
            set.spawn(async move {
                let _permit = sem.acquire().await;
                (i, dirty_mode_status(&path, author.as_deref()).await)
            });
        }
        let mut results: Vec<(usize, Option<RepoStatus>)> = Vec::new();
        while let Some(joined) = set.join_next().await {
            if let Ok(pair) = joined {
                results.push(pair);
            }
        }
        results.sort_by_key(|(i, _)| *i);
        let repos: Vec<RepoStatus> = results.into_iter().filter_map(|(_, r)| r).collect();
        let notes = ViewNotes {
            header,
            ambiguity: None,
        };
        return Ok((
            repos,
            notes,
            Vec::new(),
            None,
            app::DatasetSource::Local,
            None,
        ));
    }

    // Fetch from GitHub (with caching)
    let api_cache = cache::KeyedCache::new(config.repo_limit, github::HOST);
    let (fetch, source) = match fetch_dataset(config, &api_cache, bypass_cache).await {
        Ok(fetch) => {
            let source = match fetch.origin {
                github::Origin::Network => app::DatasetSource::Network,
                github::Origin::Cache { written } => app::DatasetSource::Cache { written },
            };
            (fetch, source)
        }
        // Launch and `R` have nothing in memory worth keeping over the
        // cache, so they build from it, but as a failure: the header and
        // the status line say the data is not current.
        Err(github::FetchError {
            failure,
            fallback: Some(mut fallback),
            meta,
        }) => {
            // The refused answer's budget reading, not the cache's (which
            // has none): the attempt is applied when the load lands.
            fallback.meta.budget = meta.budget;
            let written = match fallback.origin {
                github::Origin::Cache { written } => written,
                github::Origin::Network => None,
            };
            (*fallback, app::DatasetSource::Fallback { failure, written })
        }
        Err(github::FetchError {
            failure,
            fallback: None,
            meta,
        }) => {
            return Err(LoadError {
                message: failure.message.clone(),
                github_failure: Some(failure),
                budget: meta.budget.map(Box::new),
            });
        }
    };
    let (dataset, calendar, meta) = (fetch.repos, fetch.calendar, fetch.meta);

    let (cutoff, header) = resolve_cutoff(window, mode, config)?;
    let (repos, notes) = assemble_view(
        &dataset,
        meta.coverage.as_ref(),
        cutoff,
        header,
        args,
        config,
        repo_dir,
        true,
        None,
        fetch_claims,
        size_held,
    )
    .await?;

    Ok((repos, notes, dataset, calendar, source, Some(meta)))
}

/// One repo's full local status for `-d` mode. Returns None for a repo with
/// nothing uncommitted and nothing unpushed: dirty mode only lists work in
/// progress.
async fn dirty_mode_status(path: &Path, author: Option<&str>) -> Option<RepoStatus> {
    let dirty = git::dirty_count(path).await;
    let branches = git::branch_statuses(path).await;
    let total_ahead: u32 = branches.iter().map(|b| b.ahead).sum();

    if dirty == 0 && total_ahead == 0 {
        return None;
    }

    let name = discovery::repo_name_for_path(path);
    let display = tilde_display(path);

    // Always fetch the diffstat for recent commits so every commit
    // (not just those in a dirty repo) shows its +/- and file list.
    let branch_names: Vec<String> = branches.iter().map(|b| b.name.clone()).collect();
    let commits = git::commits(
        path,
        &format!("-{HISTORY_COUNT}"),
        true,
        author,
        &branch_names,
    )
    .await;
    let reflog = git::reflog(path, HISTORY_COUNT).await;
    let graph = git::graph(path, &branch_names, HISTORY_COUNT).await;
    let graph_expanded = git::graph_expanded(path, &branch_names, HISTORY_COUNT).await;
    let stash_entries = git::stash_list(path).await;
    let stash_count = stash_entries.len() as u32;
    let worktrees = git::worktree_list(path).await;
    let remote_url = git::remote_url(path).await;
    let remote = git::remote_branches(path).await;
    let remote_only_branches = git::unfetched_branches(&remote, &branches);

    Some(RepoStatus {
        // No local calendar until the strip asks for one.
        daily: None,
        daily_key: None,
        last_local: None,
        name,
        path: Some(display),
        location: RepoLocation::LocalOnly,
        branches,
        commits,
        behind: 0,
        ahead: total_ahead,
        dirty,
        pr_count: 0,
        prs: Vec::new(),
        ci_status: model::CiStatus::Unknown,
        ci_workflow: None,
        ci_duration: None,
        groups: vec![],
        stash_count,
        stash_entries,
        worktrees,
        reflog,
        graph,
        graph_expanded,
        repo_kind: model::RepoKind::Unknown,
        owner: None,
        remote_url,
        remote_only_branches,
        remote_branches: remote,
        work_key: None,
        fetch: model::FetchFacts::from_disk(git::last_successful_fetch_at(path)),
        authors: Default::default(),
    })
}

/// The GitHub GraphQL fetch (with disk cache), unfiltered: the top
/// repo_limit repos by push date.
async fn fetch_dataset(
    config: &config::Config,
    cache: &cache::KeyedCache,
    bypass_cache: bool,
) -> Result<github::GithubFetch, github::FetchError> {
    let mode = match bypass_cache {
        true => github::CacheMode::Bypass,
        false => github::CacheMode::AnyCached,
    };
    github::fetch_github(config.repo_limit, cache, mode).await
}

/// Which of `targets` are worth spending a `git fetch` on before the first
/// paint.
///
/// Startup used to fetch every repo past the cutoff and await all of them
/// before statting anything -- around 0.8s of network per repo at
/// `FETCH_CONCURRENCY`, so a wide time window put six seconds of nothing in
/// front of the fleet while a narrow one cost under a second. The window was
/// never the real variable: it only decides how many repos reach this loop.
///
/// `remotes_that_moved` already answers this for every later poll by diffing
/// two GitHub datasets, but the first poll of a session has no previous
/// dataset to diff against, so it fell back to fetching everything. There is a
/// baseline on disk, though: `FETCH_HEAD`'s mtime is when this clone last
/// fetched. A remote GitHub says was pushed before then is already in hand.
///
/// Unknowns fetch. No `pushed_at` means no GitHub match (a local-only clone,
/// where the remote could be anywhere), and no `FETCH_HEAD` means this clone
/// has never fetched at all; both have to be read as stale. So does an empty
/// `FETCH_HEAD`, which is what a failed fetch leaves behind
/// (`git::last_successful_fetch_at`, rule 7 of the fetch contract).
///
/// Clock skew is survivable in the one direction it can bite: a local clock
/// running ahead can make a needed fetch look unnecessary, and the next poll
/// picks it up, because that comparison is GitHub-to-GitHub and immune.
fn stale_remotes(
    targets: &[(discovery::FoundRepo, RepoLocation)],
    active: &[github::GithubRepo],
) -> Vec<std::path::PathBuf> {
    targets
        .iter()
        .filter(|(found, _)| {
            // The repo this clone claimed, by owner and name: a same-named
            // repo of another owner pushed at another time.
            let Some(pushed_at) = active
                .iter()
                .find(|r| names_remote(found.owner.as_deref(), &found.name, r))
                .map(|r| r.pushed_at)
            else {
                return true;
            };
            match git::last_successful_fetch_at(&found.path) {
                Some(fetched) => pushed_at > chrono::DateTime::<chrono::Utc>::from(fetched),
                None => true,
            }
        })
        .map(|(found, _)| found.path.clone())
        .collect()
}

/// Cutoff + header for the current view. An active TimeWindow (set by the
/// in-TUI selector or loaded from state) takes precedence over the launch
/// mode; Dirty mode never reaches here.
fn resolve_cutoff(
    window: Option<time::TimeWindow>,
    mode: &Mode,
    config: &config::Config,
) -> Result<(Option<chrono::DateTime<chrono::Utc>>, String), String> {
    if let Some(w) = window {
        return Ok((
            Some(w.cutoff(config.work_start, config.work_end)),
            w.label(config.work_start, config.work_end),
        ));
    }
    match mode {
        Mode::Auto => {
            let (c, h) = time::auto_cutoff(config.work_start, config.work_end);
            Ok((Some(c), h))
        }
        Mode::Since(s) => {
            let date = time::parse_relative_date(s)?;
            Ok((Some(time::date_to_utc_midnight(date)), format!("since {s}")))
        }
        Mode::Today => {
            let today = chrono::Local::now().date_naive();
            Ok((Some(time::date_to_utc_midnight(today)), "today".to_string()))
        }
        Mode::All => Ok((None, "all divergence".to_string())),
        Mode::Dirty => Err("dirty mode has no time cutoff".to_string()),
    }
}

/// Everything downstream of the GitHub fetch: time filtering, local
/// discovery, optional per-repo git fetch, statting, and header assembly.
/// `git_fetch: false` makes this purely local (used by the time-window
/// overlay so applying a window is instant).
/// Order discovered repos newest-pushed first, the way repoz lists them.
///
/// `active` arrives in that order already (the GraphQL `orderBy`, re-applied
/// by `merge_sources`), but the ordering never reached the screen:
/// `find_in_primary` reduces the names to a `HashSet` for a membership test,
/// and the walkers then push repos in `read_dir` order. On APFS that is hash
/// order, so the "github activity" section was in no order at all, and it
/// reshuffled whenever a repo was added or removed from the scan root.
///
/// Applied here rather than in `App::visible_repos` because the section sort
/// there is stable and simply carries this through, and *after* the pass-1 /
/// pass-2 dedupe because that is what makes a local clone beat the not-cloned
/// fallback -- reordering before it would break the load-bearing pass order.
///
/// Repos with no GitHub match sort last and keep discovery order, `sort_by_key`
/// being stable. In practice they are the local-only rows, which `section_rank`
/// puts in their own section anyway.
fn sort_by_activity(
    targets: &mut [(discovery::FoundRepo, RepoLocation)],
    active: &[github::GithubRepo],
) {
    // By owner and name, so two same-named repos each sort by their own push.
    targets.sort_by_cached_key(|(f, _)| {
        active
            .iter()
            .position(|r| names_remote(f.owner.as_deref(), &f.name, r))
            .unwrap_or(usize::MAX)
    });
}

/// Pass 3's candidates: the local repos passes 1-2 have not already shown,
/// each with the name it reports as (undotted, `discovery::repo_name_for_path`).
///
/// Deduped by canonical path only, as repoz does (D5 in
/// `docs/repository-identity-design.md`): a candidate is dropped when its
/// canonical path is the canonical path of a claimed row, or of an earlier
/// candidate, so one directory reached through two root spellings (a
/// symlink, `R/sub/..`, `/var` against `/private/var`) is listed once. The
/// raw path is the key when `canonicalize` fails. The key is for comparison
/// only: each candidate keeps the path as discovered, which is what the row
/// displays.
///
/// No name check: a dirty second checkout of a claimed repo, and two
/// same-named local-only repos, each get a row. Matching by path also keeps
/// out a clone passes 1-2 matched through its remote in a folder named
/// otherwise (`DevOps_Portfolio` for `portfolio-site`).
///
/// `checked_names` is ignored. It is the name set pass 3 used to filter by,
/// kept only so the signature existing callers use does not change.
fn unclaimed_local(
    all_local: Vec<PathBuf>,
    claimed_paths: &HashSet<&Path>,
    checked_names: &HashSet<String>,
) -> Vec<(PathBuf, String)> {
    let _ = checked_names;
    let key = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut seen: HashSet<PathBuf> = claimed_paths.iter().map(|path| key(path)).collect();
    all_local
        .into_iter()
        .filter(|path| seen.insert(key(path)))
        .map(|path| {
            let name = discovery::repo_name_for_path(&path);
            (path, name)
        })
        .collect()
}

#[allow(clippy::too_many_arguments)] // one call spine; a params struct would only rename the problem
async fn assemble_view(
    dataset: &[github::GithubRepo],
    coverage: Option<&github::Coverage>,
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
    header: String,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    git_fetch: bool,
    progress: Option<&tokio::sync::mpsc::UnboundedSender<AppEvent>>,
    fetch_claims: &app::FetchClaims,
    size_held: &HashSet<String>,
) -> Result<(Vec<RepoStatus>, ViewNotes), String> {
    // Local scan roots: the primary repo_dir plus any configured extra dirs
    // (e.g. ~/.dotfiles) -- the fleetz equivalent of repoz's EXTRA_DIRS. Used
    // by all three passes, so a repo outside the main tree gets a full
    // GitHub-matched row in pass 1 rather than only surfacing under "local
    // changes" when it happens to be dirty.
    let mut scan_roots = vec![repo_dir.to_path_buf()];
    scan_roots.extend(config.resolved_extra_dirs());
    let home = dirs::home_dir().unwrap_or_default();

    let (repos, mut notes) = assemble_view_held(
        dataset,
        cutoff,
        header,
        args,
        config,
        &home,
        &scan_roots,
        git_fetch,
        progress,
        fetch_claims,
        size_held,
    )
    .await?;
    notes.header = with_coverage_note(notes.header, coverage, cutoff, config.repo_limit);
    Ok((repos, notes))
}

/// The header with the coverage note appended when the view can be missing
/// repos GitHub has (`github::coverage_note`), so an incomplete view never
/// masquerades as complete. No coverage (nothing fetched) adds nothing.
fn with_coverage_note(
    header: String,
    coverage: Option<&github::Coverage>,
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
    repo_limit: u32,
) -> String {
    match coverage.and_then(|coverage| github::coverage_note(coverage, cutoff, repo_limit)) {
        Some(note) => format!("{header} ({note})"),
        None => header,
    }
}

/// The body of `assemble_view`, with the two filesystem anchors it used to
/// read for itself passed in: `home` is where pass 2 sweeps, and `scan_roots`
/// (the repo dir first, then the configured extra dirs) is what passes 1 and 3
/// walk. This is the test seam: a test can run the whole assembly against a
/// temp home and temp roots instead of the real `$HOME`.
///
/// `_config` gives the fetch cap only (FZ-35b): the coverage note moved to
/// `assemble_view` (FZ-09b). The fetches run here under a claim table of
/// their own, with nothing held by size.
#[cfg(test)]
#[allow(clippy::too_many_arguments)] // one call spine; a params struct would only rename the problem
async fn assemble_view_in(
    dataset: &[github::GithubRepo],
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
    header: String,
    args: &Args,
    _config: &config::Config,
    home: &Path,
    scan_roots: &[PathBuf],
    git_fetch: bool,
    progress: Option<&tokio::sync::mpsc::UnboundedSender<AppEvent>>,
) -> Result<(Vec<RepoStatus>, ViewNotes), String> {
    assemble_view_with(
        dataset,
        cutoff,
        header,
        args,
        _config,
        home,
        scan_roots,
        git_fetch,
        progress,
        &app::FetchClaims::default(),
    )
    .await
}

/// The body of `assemble_view` (see `assemble_view_in`, its test seam). Each
/// fetch it runs takes its claim in `fetch_claims` when its permit is
/// granted and gives it back when it ends (FZ-35a); a path a sweep is
/// fetching already is not fetched again, and its row records no fetch.
#[cfg(test)]
#[allow(clippy::too_many_arguments)] // one call spine; a params struct would only rename the problem
async fn assemble_view_with(
    dataset: &[github::GithubRepo],
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
    header: String,
    args: &Args,
    config: &config::Config,
    home: &Path,
    scan_roots: &[PathBuf],
    git_fetch: bool,
    progress: Option<&tokio::sync::mpsc::UnboundedSender<AppEvent>>,
    fetch_claims: &app::FetchClaims,
) -> Result<(Vec<RepoStatus>, ViewNotes), String> {
    assemble_view_held(
        dataset,
        cutoff,
        header,
        args,
        config,
        home,
        scan_roots,
        git_fetch,
        progress,
        fetch_claims,
        &HashSet::new(),
    )
    .await
}

/// `assemble_view_with`, whose fetches are capped at `config.fetch_max_mb`
/// and skip the display paths in `size_held`, which the fetch cap holds
/// (FZ-35b); a successful one records its row's size baseline.
#[allow(clippy::too_many_arguments)] // one call spine; a params struct would only rename the problem
async fn assemble_view_held(
    dataset: &[github::GithubRepo],
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
    header: String,
    args: &Args,
    config: &config::Config,
    home: &Path,
    scan_roots: &[PathBuf],
    git_fetch: bool,
    progress: Option<&tokio::sync::mpsc::UnboundedSender<AppEvent>>,
    fetch_claims: &app::FetchClaims,
    size_held: &HashSet<String>,
) -> Result<(Vec<RepoStatus>, ViewNotes), String> {
    // Apply --before filter
    let before_cutoff = match &args.before {
        Some(b) => {
            let date = time::parse_relative_date(b)?;
            Some(time::date_to_utc_end_of_day(date))
        }
        None => None,
    };

    // Filter active repos
    let mut active: Vec<github::GithubRepo> = dataset.to_vec();
    if let Some(c) = cutoff {
        active = github::filter_since(&active, c);
    }
    if let Some(b) = before_cutoff {
        active = github::filter_before(&active, b);
    }

    // The full dataset rides along: whether a folder name is ambiguous, or an
    // origin names a known repo, must not depend on the window.
    let wanted = discovery::Wanted::with_full_dataset(
        active.iter().map(|r| (r.owner.as_deref(), r.name.as_str())),
        dataset
            .iter()
            .map(|r| (r.owner.as_deref(), r.name.as_str())),
    );

    // Pass 1: find under every scan root (repo_dir + extra_dirs)
    let (primary_found, mut ambiguities) =
        discovery::find_in_primary_with_ambiguities(scan_roots, &wanted);
    let found_set: HashSet<String> = primary_found
        .iter()
        .map(discovery::FoundRepo::key)
        .collect();

    // Pass 2: find elsewhere
    let (elsewhere_found, elsewhere_ambiguities) =
        discovery::find_elsewhere_with_ambiguities(home, scan_roots, &wanted, &found_set);
    ambiguities.extend(elsewhere_ambiguities);
    let ambiguity = ambiguity_line(&ambiguities, dataset);

    // Dedupe up front: a repo found in both passes is one stat target, so the
    // gauge total is the real amount of work rather than a count that has to
    // be padded as duplicates are skipped. Primary wins the location. Keyed
    // by owner and name, so `alice/api` and `company/api` are two targets.
    let mut targets: Vec<(discovery::FoundRepo, RepoLocation)> = Vec::new();
    let mut seen: HashSet<RemoteKey> = HashSet::new();
    for f in &primary_found {
        if seen.insert(remote_key(f.owner.as_deref(), &f.name)) {
            targets.push((f.clone(), RepoLocation::Primary));
        }
    }
    for f in &elsewhere_found {
        if seen.insert(remote_key(f.owner.as_deref(), &f.name)) {
            targets.push((f.clone(), RepoLocation::Elsewhere));
        }
    }
    sort_by_activity(&mut targets, &active);

    if let Some(tx) = progress {
        let _ = tx.send(AppEvent::Progress(app::ApplyProgress::Discovering));
        let _ = tx.send(AppEvent::Progress(app::ApplyProgress::Statting {
            done: 0,
            total: targets.len(),
        }));
    }

    // How each fetch run here ended, by path, for the rows built below.
    let mut load_fetches: HashMap<PathBuf, git::FetchOutcome> = HashMap::new();
    if git_fetch {
        let stale = stale_remotes(&targets, &active)
            .into_iter()
            .filter(|path| !size_held.contains(&tilde_display(path)));
        let cap = config.fetch_cap_bytes();
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(FETCH_CONCURRENCY));
        let mut set = tokio::task::JoinSet::new();
        for path in stale {
            let sem = sem.clone();
            let claims = fetch_claims.clone();
            set.spawn(async move {
                let _permit = sem.acquire().await;
                let key = tilde_display(&path);
                let Some(token) = claims.reserve(&key, std::time::Instant::now()) else {
                    return (path, git::FetchOutcome::NotAttempted);
                };
                let outcome =
                    fetch_under_claim_capped(&claims, &key, &path, token, None, cap).await;
                claims.release(&key, Some(token));
                (path, outcome)
            });
        }
        while let Some(joined) = set.join_next().await {
            if let Ok((path, outcome)) = joined {
                load_fetches.insert(path, outcome);
            }
        }
    }
    let fetched_at = chrono::Local::now();

    // Stat every target in parallel behind a semaphore, keyed by index so the
    // output order matches the discovery order regardless of finish order.
    let active_shared = std::sync::Arc::new(active.clone());
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(probe_concurrency()));
    let mut set = tokio::task::JoinSet::new();
    for (i, (found, location)) in targets.iter().cloned().enumerate() {
        let sem = sem.clone();
        let active = active_shared.clone();
        let args = args.clone();
        set.spawn(async move {
            let _permit = sem.acquire().await;
            (i, build_repo_status(&found, &active, &args, location).await)
        });
    }

    let mut slots: Vec<Option<RepoStatus>> = vec![None; targets.len()];
    let mut stat_done = 0usize;
    while let Some(joined) = set.join_next().await {
        if let Ok((i, repo)) = joined {
            slots[i] = Some(repo);
            stat_done += 1;
            if let Some(tx) = progress {
                let _ = tx.send(AppEvent::Progress(app::ApplyProgress::Statting {
                    done: stat_done,
                    total: targets.len(),
                }));
            }
        }
    }
    // Slots are in target order, so each row's fetch is found by its target.
    for (slot, (found, _)) in slots.iter_mut().zip(&targets) {
        if let (Some(repo), Some(outcome)) = (slot, load_fetches.get(&found.path)) {
            repo.fetch.record(outcome, fetched_at);
            if *outcome == git::FetchOutcome::Succeeded {
                repo.fetch.disk_baseline = disk_usage_of(repo, &active);
            }
        }
    }
    let mut repos: Vec<RepoStatus> = slots.into_iter().flatten().collect();

    if let Some(tx) = progress {
        let _ = tx.send(AppEvent::Progress(app::ApplyProgress::Finishing));
    }

    // Pass 3: local repos (under repo_dir or any configured extra_dir, e.g.
    // ~/.dotfiles) with uncommitted or unpushed changes, not already shown
    // above. Runs *before* the not-cloned loop so a dirty local clone whose
    // origin names an active repo carries it as LocalOnly rather than leaving
    // it mislabeled "not cloned" when it lives at a path passes 1/2 don't
    // reach.
    let all_local = discovery::find_all_local(scan_roots);
    let claimed_paths: HashSet<&Path> = targets.iter().map(|(f, _)| f.path.as_path()).collect();
    let candidates = unclaimed_local(all_local, &claimed_paths, &HashSet::new());
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(probe_concurrency()));
    let mut set = tokio::task::JoinSet::new();
    for (i, (path, name)) in candidates.iter().cloned().enumerate() {
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire().await;
            // No glyph by name: a LocalOnly row gets one only as a carrier,
            // from the repo its origin names, below.
            let status = local_only_status(&path, name, model::RepoKind::Unknown, cutoff).await;
            (i, status)
        });
    }

    // Back into discovery order. A clean repo gets no row, and every dirty
    // one does: `unclaimed_local` already deduped the candidates by path.
    let mut local_results: Vec<(usize, Option<RepoStatus>)> = Vec::new();
    while let Some(joined) = set.join_next().await {
        if let Ok(pair) = joined {
            local_results.push(pair);
        }
    }
    local_results.sort_by_key(|(i, _)| *i);
    let mut local_rows: Vec<(&Path, RepoStatus)> = Vec::new();
    for (i, status) in local_results {
        let Some(repo) = status else { continue };
        local_rows.push((candidates[i].0.as_path(), repo));
    }
    mark_local_carriers(&mut local_rows, dataset, scan_roots, &mut seen);
    repos.extend(local_rows.into_iter().map(|(_, repo)| repo));

    // Not-cloned repos: active on GitHub but carried by no row above. Last,
    // so a local clone always wins over this, but only through the remote it
    // carries: a pass 3 clone without an origin no longer hides the repo.
    for gh_repo in &active {
        if seen.contains(&remote_key(gh_repo.owner.as_deref(), &gh_repo.name)) {
            continue;
        }
        repos.push(RepoStatus {
            // No local calendar until the strip asks for one.
            daily: None,
            daily_key: None,
            last_local: None,
            name: gh_repo.name.clone(),
            path: None,
            location: RepoLocation::NotCloned,
            branches: Vec::new(),
            commits: Vec::new(),
            behind: 0,
            ahead: 0,
            dirty: 0,
            pr_count: gh_repo.pr_count,
            prs: gh_repo.prs.clone(),
            ci_status: gh_repo.ci_status.clone(),
            ci_workflow: gh_repo.ci_workflow.clone(),
            ci_duration: gh_repo.ci_duration.clone(),
            groups: vec![],
            stash_count: 0,
            stash_entries: vec![],
            worktrees: vec![],
            reflog: vec![],
            graph: vec![],
            graph_expanded: Vec::new(),
            repo_kind: gh_repo.repo_kind,
            owner: gh_repo.owner.clone(),
            remote_url: None,
            remote_only_branches: vec![],
            remote_branches: vec![],
            work_key: None,
            // Nothing on disk to fetch into.
            fetch: model::FetchFacts::default(),
            authors: Default::default(),
        });
    }

    // Build header with --before and --author if present
    let mut full_header = header;
    if let Some(ref b) = args.before {
        full_header = format!("{full_header} → before {b}");
    }
    if let Some(ref a) = args.author {
        full_header = format!("{full_header} (author: {a})");
    }
    // The coverage note goes last, in `assemble_view`, which has the payload's
    // coverage; the merged dataset here cannot say what was cut off.

    let notes = ViewNotes {
        header: full_header,
        ambiguity,
    };
    Ok((repos, notes))
}

/// A remote identity as assembly compares it: lowercased owner and name. No
/// owner is the legacy key of a payload without `login`, matched by name.
/// The host is left out: every dataset entry is on `github.com`, and matching
/// is host-agnostic (D2).
type RemoteKey = (Option<String>, String);

fn remote_key(owner: Option<&str>, name: &str) -> RemoteKey {
    model::identity_key(owner, name)
}

/// The D4 status line for the first ambiguous clone by path, or None. The
/// count is the owners the full dataset has for the name, since that is what
/// made the folder name ambiguous.
fn ambiguity_line(
    ambiguities: &[discovery::Ambiguity],
    dataset: &[github::GithubRepo],
) -> Option<String> {
    let first = ambiguities.iter().min()?;
    let owners: HashSet<String> = dataset
        .iter()
        .filter(|r| r.name.eq_ignore_ascii_case(&first.name))
        .filter_map(|r| r.owner.as_deref().map(str::to_lowercase))
        .collect();
    Some(format!(
        "{}: {} matches {} GitHub repos, add an origin remote",
        first.name,
        tilde_display(&first.path),
        owners.len()
    ))
}

/// Make each LocalOnly row the carrier of the repo its stored `origin` names,
/// when that repo is in the full dataset and no row carries it yet (`carried`
/// holds the keys already carried, and gains each one claimed here). The
/// carrier is marked by its `owner` and takes the repo's GitHub fields now,
/// so they show from the first frame (D6, rule 2 of section 3.5 of
/// `docs/repository-identity-design.md`). Every other LocalOnly row keeps no
/// owner and no GitHub fields.
///
/// The URL is the one `git remote get-url` returned, which applies
/// `insteadOf` and, in a linked worktree, names the main clone's origin, so
/// a checkout passes 1 and 2 could not match can still carry here. Several
/// rows naming one repo are tried in discovery's primary-checkout order
/// (`discovery::CheckoutRank`), each as a match through its exact origin.
///
/// The root index is that of the first scan root holding the path, the same
/// index pass 1 gives a clone two overlapping roots both reach, since the
/// lower-ranked offer wins there. Pass 3 walks only the scan roots, so a row
/// always lies under one; `usize::MAX` is a fallback that cannot occur.
fn mark_local_carriers(
    rows: &mut [(&Path, RepoStatus)],
    dataset: &[github::GithubRepo],
    scan_roots: &[PathBuf],
    carried: &mut HashSet<RemoteKey>,
) {
    let mut order: Vec<(discovery::CheckoutRank, usize, &github::GithubRepo)> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, (path, repo))| {
            let origin = model::RemoteRef::parse(repo.remote_url.as_deref()?)?;
            let entry = dataset.iter().find(|r| {
                r.owner.is_some() && names_remote(Some(&origin.owner), &origin.name, r)
            })?;
            let root = scan_roots
                .iter()
                .position(|root| path.starts_with(root))
                .unwrap_or(usize::MAX);
            let rank = discovery::CheckoutRank::exact_origin(&origin, path, root);
            Some((rank, i, entry))
        })
        .collect();
    order.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, i, entry) in order {
        if !carried.insert(remote_key(entry.owner.as_deref(), &entry.name)) {
            continue;
        }
        let repo = &mut rows[i].1;
        repo.owner = entry.owner.clone();
        repo.apply_github(entry);
    }
}

/// Probe a local repo that no GitHub match claimed. Returns None when it has
/// neither uncommitted nor unpushed work, which is the signal that it is not
/// worth a row: a clean local clone is not news.
/// Whether a local-only repo still belongs in "local changes" under `cutoff`.
///
/// The section cannot be gated on `pushed_at` the way "github activity" is:
/// its whole point is work the remote has never seen, and a repo that was
/// never pushed has no such timestamp. So it is gated on the local clock
/// instead, which is what `git::last_local_activity` reports.
///
/// Unpushed commits are exempt. Untracked files left lying around are noise
/// you chose to ignore, but a commit that exists only on this machine is one
/// `git push` from being lost with it, and a narrow window is not a reason to
/// stop saying so.
///
/// No cutoff means no filtering, which is how the "all" window and `--all`
/// keep showing everything the narrower windows evicted.
fn local_repo_is_current(
    ahead: u32,
    last_local: Option<chrono::DateTime<chrono::Utc>>,
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
) -> bool {
    let Some(cutoff) = cutoff else { return true };
    if ahead > 0 {
        return true;
    }
    last_local.is_some_and(|at| at > cutoff)
}

async fn local_only_status(
    path: &Path,
    name: String,
    repo_kind: model::RepoKind,
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
) -> Option<RepoStatus> {
    let dirty = git::dirty_count(path).await;
    let branches = git::branch_statuses(path).await;
    let total_ahead: u32 = branches.iter().map(|b| b.ahead).sum();

    if dirty == 0 && total_ahead == 0 {
        return None;
    }

    // Dirty is not the same as current. A repo `git init`-ed and abandoned
    // reports dirty forever, and before this it held a row forever with it.
    // Kept rather than discarded: `info` draws the same value, so the date on
    // screen is the one the window judged the repo by.
    let last_local = git::last_local_activity(path).await;
    if !local_repo_is_current(total_ahead, last_local, cutoff) {
        return None;
    }

    let display = tilde_display(path);
    let remote_url = git::remote_url(path).await;
    let remote = git::remote_branches(path).await;
    let remote_only_branches = git::unfetched_branches(&remote, &branches);

    Some(RepoStatus {
        // No local calendar until the strip asks for one.
        daily: None,
        daily_key: None,
        last_local,
        name,
        path: Some(display),
        location: RepoLocation::LocalOnly,
        branches,
        commits: Vec::new(),
        behind: 0,
        ahead: total_ahead,
        dirty,
        pr_count: 0,
        prs: Vec::new(),
        ci_status: model::CiStatus::Unknown,
        ci_workflow: None,
        ci_duration: None,
        groups: vec![],
        stash_count: 0,
        stash_entries: vec![],
        worktrees: vec![],
        reflog: vec![],
        graph: vec![],
        graph_expanded: Vec::new(),
        repo_kind,
        // No GitHub match, so the remote URL is the only identity available;
        // `owner_display` parses it at render time.
        owner: None,
        remote_url,
        remote_only_branches,
        remote_branches: remote,
        work_key: None,
        fetch: model::FetchFacts::from_disk(git::last_successful_fetch_at(path)),
        authors: Default::default(),
    })
}

/// Build a full RepoStatus from a discovered local repo.
async fn build_repo_status(
    found: &discovery::FoundRepo,
    gh_repos: &[github::GithubRepo],
    args: &Args,
    location: RepoLocation,
) -> RepoStatus {
    let path = &found.path;
    // Every key input is read before the probes the key gates, so a change
    // landing while they run is in the probes' output or moves the next
    // sweep's key, never recorded in the key alone.
    let local = git::try_branch_statuses(path).await;
    let dirty = git::dirty_count(path).await;
    let other = git::try_ref_listing(path).await;
    let work_key = git::work_key_from(path, local.as_ref().ok(), other.as_ref().ok()).await;
    let branches = local.map(|l| l.statuses).unwrap_or_default();
    let remote = other.map(|o| o.remotes).unwrap_or_default();

    let mut total_behind = 0u32;
    let mut total_ahead = 0u32;
    for b in &branches {
        total_behind += b.behind;
        total_ahead += b.ahead;
    }

    let author = args.author.as_deref();

    // Recent history across the local branches, not just the divergent commits
    // -- a glance at "what's been happening here" shouldn't depend on being
    // ahead/behind, and the graph beside it draws the same refs.
    // Always with the diffstat so every commit shows its +/- and file list.
    let branch_names: Vec<String> = branches.iter().map(|b| b.name.clone()).collect();
    let commits = git::commits(
        path,
        &format!("-{HISTORY_COUNT}"),
        true,
        author,
        &branch_names,
    )
    .await;
    let reflog = git::reflog(path, HISTORY_COUNT).await;

    let graph = git::graph(path, &branch_names, HISTORY_COUNT).await;
    let graph_expanded = git::graph_expanded(path, &branch_names, HISTORY_COUNT).await;

    // The repo discovery matched this clone to, by owner and name: the first
    // same-named entry could be another owner's.
    let gh_match = gh_repos
        .iter()
        .find(|r| names_remote(found.owner.as_deref(), &found.name, r));

    let pr_count = gh_match.map_or(0, |r| r.pr_count);
    let prs = gh_match.map_or_else(Vec::new, |r| r.prs.clone());
    let ci_status = gh_match.map_or(model::CiStatus::Unknown, |r| r.ci_status.clone());
    let ci_workflow = gh_match.and_then(|r| r.ci_workflow.clone());
    let ci_duration = gh_match.and_then(|r| r.ci_duration.clone());
    let repo_kind = gh_match.map_or(model::RepoKind::Unknown, |r| r.repo_kind);
    let owner = gh_match.and_then(|r| r.owner.clone());

    let display = tilde_display(path);

    // Stash + worktree info
    let stash_entries = git::stash_list(path).await;
    let stash_count = stash_entries.len() as u32;
    let worktrees = git::worktree_list(path).await;

    let remote_url = git::remote_url(path).await;
    let remote_only_branches = git::unfetched_branches(&remote, &branches);

    RepoStatus {
        // No local calendar until the strip asks for one.
        daily: None,
        daily_key: None,
        last_local: None,
        name: found.name.clone(),
        path: Some(display),
        location,
        branches,
        commits,
        behind: total_behind,
        ahead: total_ahead,
        dirty,
        pr_count,
        prs,
        ci_status,
        ci_workflow,
        ci_duration,
        groups: vec![],
        stash_count,
        stash_entries,
        worktrees,
        reflog,
        graph,
        graph_expanded,
        repo_kind,
        owner,
        remote_url,
        remote_only_branches,
        remote_branches: remote,
        work_key,
        fetch: model::FetchFacts::from_disk(git::last_successful_fetch_at(path)),
        authors: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression (#68): returning from lazygit or `$EDITOR` left the mouse
    /// dead for the rest of the session. Both switch mouse reporting off as
    /// they exit, and the modes belong to the terminal, not to a screen, so
    /// re-entering the alternate screen does not bring them back. Measured in
    /// tmux: `mouse_any_flag` and `mouse_sgr_flag` went from 1 to 0 across
    /// one Enter-and-quit of lazygit, and the same across `e` and nvim.
    #[test]
    fn reclaiming_the_terminal_asks_for_the_mouse_again() {
        let mut capture = Vec::new();
        crossterm::execute!(capture, EnableMouseCapture).unwrap();
        let mut written = Vec::new();
        reclaim_terminal(&mut written).unwrap();
        assert!(
            written
                .windows(capture.len())
                .any(|window| window == capture.as_slice()),
            "reclaim wrote {:?}, without the mouse capture request {:?}",
            String::from_utf8_lossy(&written),
            String::from_utf8_lossy(&capture),
        );
    }

    /// Handing the terminal to a child must switch mouse reporting off first,
    /// or the child inherits fleetz's capture and its own mouse handling
    /// receives events it did not ask for (#82). The capture-off request has
    /// to precede leaving the screen, mirroring the reclaim order.
    #[test]
    fn releasing_the_terminal_switches_the_mouse_off_before_leaving_the_screen() {
        let mut capture_off = Vec::new();
        crossterm::execute!(capture_off, DisableMouseCapture).unwrap();
        let mut leave = Vec::new();
        crossterm::execute!(leave, LeaveAlternateScreen).unwrap();
        let mut written = Vec::new();
        release_terminal(&mut written).unwrap();
        let position_of = |needle: &[u8]| {
            written
                .windows(needle.len())
                .position(|window| window == needle)
        };
        let off_at = position_of(&capture_off);
        let leave_at = position_of(&leave);
        assert!(
            off_at.is_some() && leave_at.is_some(),
            "release wrote {:?}, missing the capture-off {:?} or the screen leave {:?}",
            String::from_utf8_lossy(&written),
            String::from_utf8_lossy(&capture_off),
            String::from_utf8_lossy(&leave),
        );
        assert!(
            off_at < leave_at,
            "release wrote {:?}: the mouse must go off before the screen is left",
            String::from_utf8_lossy(&written),
        );
    }

    /// An idle loop paints as soon as it has something to paint. This is the
    /// ordinary case and must not be delayed by the coalescing at all.
    #[test]
    fn an_empty_queue_paints_immediately() {
        assert!(should_paint(true, 0, false));
    }

    /// An animation frame is a deadline, not a data update: it is never held,
    /// however much background work is queued behind it. Holding them is what
    /// made the intro stutter under a release build's faster repo probes.
    #[test]
    fn an_animation_frame_is_never_held() {
        assert!(should_paint(false, 0, true));
        assert!(should_paint(false, MAX_COALESCED_FRAMES - 1, true));
        assert!(should_paint(true, 0, true));
    }

    /// The bug: a trackpad flick delivers wheel events faster than a repaint
    /// takes, so painting one frame per event let the queue run away and the
    /// scroll froze, then played out the backlog after the gesture had ended.
    #[test]
    fn a_queued_burst_holds_the_paint() {
        assert!(!should_paint(false, 0, false));
        assert!(!should_paint(false, 1, false));
        assert!(!should_paint(false, MAX_COALESCED_FRAMES - 1, false));
    }

    /// The hold is bounded, so a source that never stops arriving still gets
    /// drawn rather than freezing the pane for as long as it lasts.
    #[test]
    fn a_queue_that_never_drains_still_paints_eventually() {
        assert!(should_paint(false, MAX_COALESCED_FRAMES, false));
        assert!(should_paint(false, MAX_COALESCED_FRAMES + 1, false));
    }

    /// `intro = false` puts no loading field up, and neither does a launch
    /// with no load running. `intro = true` with a load running does.
    #[test]
    fn intro_off_shows_no_loading_field() {
        assert!(!shows_loading_field(false, true));
        assert!(!shows_loading_field(false, false));
        assert!(!shows_loading_field(true, false));
        assert!(shows_loading_field(true, true));
    }

    /// An empty app at launch, with the loading field up as `run_event_loop`
    /// puts it.
    fn app_under_loading_field() -> App {
        let mut app = App::new(Vec::new(), String::new(), 120, 40);
        app.loading = true;
        app.loading_field = Some(ui::intro::Loading::new());
        app
    }

    /// Whether any row of `buf` holds the top row of the `LOADING` banner.
    fn shows_loading_banner(buf: &ratatui::buffer::Buffer) -> bool {
        (0..buf.area.height).any(|y| {
            let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
            row.contains("█    ████ ████ ███  ███ █  █ ████")
        })
    }

    /// The loading field is up until the load lands, and the very next frame
    /// is the finished screen exactly: nothing is drawn in between, so there is
    /// no wave frame and no noise left over. The removed intro wave painted
    /// 1.5 s of frames here.
    #[test]
    fn the_frame_after_the_load_lands_is_the_finished_screen() {
        use ratatui::backend::TestBackend;
        let mut app = app_under_loading_field();
        let running = std::sync::atomic::AtomicBool::new(true);
        let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
        term.draw(|frame| draw_screen(frame, &app)).unwrap();
        assert!(
            shows_loading_banner(term.backend().buffer()),
            "the loading field was not painted while the load ran"
        );

        // What the `Loaded` arm does: the load is applied, then the field goes.
        app.loading = false;
        dismiss_loading_field(&mut app, &running);
        assert!(app.loading_field.is_none(), "the field is still up");
        assert!(
            !running.load(std::sync::atomic::Ordering::Relaxed),
            "the field's frame timer is still running"
        );
        term.draw(|frame| draw_screen(frame, &app)).unwrap();

        let mut finished = Terminal::new(TestBackend::new(120, 40)).unwrap();
        finished.draw(|frame| ui::render(frame, &app)).unwrap();
        assert_eq!(
            term.backend().buffer(),
            finished.backend().buffer(),
            "the frame after the load is not the finished screen"
        );
    }

    /// The bug (#64): a `LoadingFrame` queued behind `Loaded` arrives after the
    /// load's paint was held for it, and overwrote `needs_redraw` with false.
    /// The held paint was lost and the loading field stayed on screen until
    /// something else asked for a redraw.
    #[test]
    fn a_late_loading_frame_keeps_the_held_paint_of_the_finished_screen() {
        let mut app = app_under_loading_field();
        let running = std::sync::atomic::AtomicBool::new(true);

        // What the `Loaded` arm does: the load is applied, the field goes, and
        // the finished screen is asked for.
        app.loading = false;
        dismiss_loading_field(&mut app, &running);
        app.needs_redraw = true;
        // The late frame is already queued and nothing animates, so the loop
        // holds that paint for it and leaves the flag set.
        assert!(!should_paint(false, 0, app.loading_field.is_some()));

        on_loading_frame(&mut app);
        assert!(
            app.needs_redraw,
            "the late frame cancelled the held paint of the finished screen"
        );
        // The queue is drained now, so the finished screen paints at once.
        assert!(should_paint(true, 1, app.loading_field.is_some()));
    }

    /// While the field is up, each of its frames asks for a repaint.
    #[test]
    fn a_loading_frame_while_the_field_is_up_asks_for_a_redraw() {
        let mut app = app_under_loading_field();
        app.needs_redraw = false;
        on_loading_frame(&mut app);
        assert!(app.needs_redraw);
    }

    /// A frame that arrives after the field is gone, with nothing pending,
    /// does not invent a repaint.
    #[test]
    fn a_late_loading_frame_with_no_pending_paint_asks_for_none() {
        let mut app = app_under_loading_field();
        let running = std::sync::atomic::AtomicBool::new(true);
        dismiss_loading_field(&mut app, &running);
        app.needs_redraw = false;
        on_loading_frame(&mut app);
        assert!(!app.needs_redraw);
    }

    /// A resize while the field is up paints the field at the new size, banner
    /// re-centred and no cell left over from the old layout.
    #[test]
    fn a_resize_while_loading_paints_the_field_at_the_new_size() {
        use ratatui::backend::TestBackend;
        let app = app_under_loading_field();
        let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
        term.draw(|frame| draw_screen(frame, &app)).unwrap();

        term.backend_mut().resize(80, 24);
        term.draw(|frame| draw_screen(frame, &app)).unwrap();
        let buf = term.backend().buffer();
        assert_eq!(buf.area, ratatui::layout::Rect::new(0, 0, 80, 24));
        assert!(
            shows_loading_banner(buf),
            "the banner is gone after a resize"
        );

        let mut fresh = ratatui::buffer::Buffer::empty(buf.area);
        app.loading_field
            .as_ref()
            .expect("the field")
            .paint(&mut fresh);
        let differing_symbols = (0..24u16)
            .flat_map(|y| (0..80u16).map(move |x| (x, y)))
            .filter(|(x, y)| {
                // The noise churns with the clock, so compare where the banner
                // sits, which is still and derived from the area alone.
                buf[(*x, *y)].symbol().starts_with('█') != fresh[(*x, *y)].symbol().starts_with('█')
            })
            .count();
        assert_eq!(
            differing_symbols, 0,
            "the banner is not where a fresh field at the new size puts it"
        );
    }

    fn test_repo() -> RepoStatus {
        RepoStatus {
            daily: None,
            daily_key: None,
            last_local: None,
            name: "r".to_string(),
            path: None,
            location: model::RepoLocation::Primary,
            branches: vec![],
            commits: vec![],
            behind: 0,
            ahead: 0,
            dirty: 0,
            pr_count: 0,
            prs: vec![],
            ci_status: model::CiStatus::Unknown,
            ci_workflow: None,
            ci_duration: None,
            groups: vec![],
            stash_count: 0,
            stash_entries: vec![],
            worktrees: vec![],
            reflog: vec![],
            graph: vec![],
            graph_expanded: Vec::new(),
            repo_kind: model::RepoKind::Unknown,
            owner: None,
            remote_url: None,
            remote_only_branches: vec![],
            remote_branches: vec![],
            work_key: None,
            fetch: model::FetchFacts::default(),
            authors: Default::default(),
        }
    }

    fn key(head: &str) -> git::WorkKey {
        git::WorkKey {
            head_sha: head.to_string(),
            index_mtime: None,
            index_size: 0,
            stash_size: 0,
            head_log_size: 0,
            head_ref: None,
            ref_signature: 0,
        }
    }

    /// A missing key is never "unchanged": two sweeps whose ref queries both
    /// failed must both re-probe, not reuse each other's empty answer.
    #[test]
    fn two_missing_work_keys_never_count_as_unchanged() {
        assert!(!key_unchanged(&None, &None));
        assert!(!key_unchanged(&None, &Some(key("aaa"))));
        assert!(!key_unchanged(&Some(key("aaa")), &None));
        assert!(!key_unchanged(&Some(key("aaa")), &Some(key("bbb"))));
        assert!(key_unchanged(&Some(key("aaa")), &Some(key("aaa"))));
    }

    // --- FZ-02: a local sweep result must not undo a newer GitHub poll ---
    //
    // The race, driven with controlled events: a sweep snapshots the row
    // (`sweep_targets`) while CI is pending, a poll lands and says passed,
    // then the sweep's result lands carrying the snapshot's GitHub fields.

    const SWEPT_PATH: &str = "~/repos/r";

    /// An app with one cloned repo whose CI is pending, as at launch.
    fn app_with_pending_repo() -> App {
        let mut repo = test_repo();
        repo.path = Some(SWEPT_PATH.to_string());
        repo.ci_status = model::CiStatus::Pending;
        repo.ci_workflow = Some("ci".to_string());
        repo.ci_duration = None;
        repo.pr_count = 0;
        repo.repo_kind = model::RepoKind::Public;
        repo.owner = Some("Gaurgle".to_string());
        repo.remote_url = Some("git@github.com:Gaurgle/r.git".to_string());
        repo.groups = vec!["work".to_string()];
        repo.work_key = Some(key("aaa"));
        App::new(vec![repo], "h".to_string(), 120, 44)
    }

    /// What `rebuild_repo_status` returns: the snapshot taken when the sweep
    /// started, with every local probe re-run and moved on.
    fn rebuilt_from(snapshot: &RepoStatus) -> RepoStatus {
        let mut status = snapshot.clone();
        status.dirty = 3;
        status.ahead = 2;
        status.behind = 1;
        status.branches = vec![model::BranchStatus {
            name: "feature".to_string(),
            upstream: None,
            behind: 1,
            ahead: 2,
            is_head: true,
            tip: "bbb".to_string(),
        }];
        status.commits = vec![commit_template("bbb")];
        status.stash_entries = vec!["stash@{0}: WIP".to_string()];
        status.stash_count = 1;
        status.worktrees = vec![model::Worktree {
            path: "~/repos/r-wt".to_string(),
            branch: "wt".to_string(),
            ..Default::default()
        }];
        status.remote_only_branches = vec!["origin/other".to_string()];
        status.work_key = Some(key("bbb"));
        status
    }

    fn commit_template(hash: &str) -> model::Commit {
        model::Commit {
            hash: hash.to_string(),
            message: "work".to_string(),
            body: String::new(),
            author: "a".to_string(),
            date: None,
            insertions: 0,
            deletions: 0,
            files: vec![],
        }
    }

    /// A successful poll reporting `r` as passed with one open PR.
    fn poll_says_passed() -> Result<Box<github::GithubFetch>, String> {
        let pr = model::PullRequest {
            number: 7,
            title: "Fix it".to_string(),
            author: "someone".to_string(),
            created_at: chrono::Utc::now(),
            review_status: model::ReviewStatus::Approved,
            url: "https://github.com/Gaurgle/r/pull/7".to_string(),
        };
        Ok(Box::new(github::GithubFetch {
            repos: vec![github::GithubRepo {
                name: "r".to_string(),
                pushed_at: chrono::Utc::now(),
                pr_count: 1,
                prs: vec![pr],
                ci_status: model::CiStatus::Passed,
                ci_workflow: Some("release".to_string()),
                ci_duration: Some("1m 4s".to_string()),
                repo_kind: model::RepoKind::Private,
                owner: Some("Gaurgle".to_string()),
                disk_usage: None,
            }],
            calendar: None,
            has_pr_counts: true,
            origin: github::Origin::Network,
            meta: Default::default(),
        }))
    }

    fn poll(app: &mut App, result: Result<Box<github::GithubFetch>, String>) {
        let result = result.map_err(|message| github::FetchError {
            failure: github::Failure::new(github::FailureKind::Http, message),
            fallback: None,
            meta: Default::default(),
        });
        poll_with(app, result);
    }

    /// Start an attempt and land `result` as its answer.
    fn poll_with(app: &mut App, result: Result<Box<github::GithubFetch>, github::FetchError>) {
        let attempt = claim_github_poll(app).expect("no poll in flight");
        answer(app, attempt, result);
    }

    /// Land `result` as the answer to `attempt`.
    fn answer(
        app: &mut App,
        attempt: u64,
        result: Result<Box<github::GithubFetch>, github::FetchError>,
    ) {
        // No previous dataset, so no remote "moved" and nothing is spawned:
        // the receiver is never read and no runtime is needed.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        apply_github_poll(app, attempt, result, &tx, &None);
    }

    fn assert_poll_fields(repo: &RepoStatus) {
        assert_eq!(repo.ci_status, model::CiStatus::Passed);
        assert_eq!(repo.ci_workflow.as_deref(), Some("release"));
        assert_eq!(repo.ci_duration.as_deref(), Some("1m 4s"));
        assert_eq!(repo.pr_count, 1);
        assert_eq!(repo.prs.len(), 1);
        assert_eq!(repo.prs[0].number, 7);
        assert_eq!(repo.repo_kind, model::RepoKind::Private);
    }

    fn assert_sweep_fields(repo: &RepoStatus) {
        assert_eq!(repo.dirty, 3);
        assert_eq!(repo.ahead, 2);
        assert_eq!(repo.behind, 1);
        assert_eq!(repo.branches.len(), 1);
        assert_eq!(repo.branches[0].name, "feature");
        assert_eq!(repo.commits.len(), 1);
        assert_eq!(repo.commits[0].hash, "bbb");
        assert_eq!(repo.stash_count, 1);
        assert_eq!(repo.stash_entries.len(), 1);
        assert_eq!(repo.worktrees.len(), 1);
        assert_eq!(repo.remote_only_branches, vec!["origin/other".to_string()]);
        assert_eq!(
            repo.work_key.as_ref().map(|k| k.head_sha.as_str()),
            Some("bbb")
        );
    }

    /// The regression: local start, GitHub update, local finish. The sweep's
    /// result restored the snapshot's `Pending` over the poll's `Passed`.
    #[test]
    fn a_sweep_landing_after_a_poll_keeps_the_polls_github_fields() {
        let mut app = app_with_pending_repo();
        let snapshot = sweep_targets(&app, None).remove(0).old;
        poll(&mut app, poll_says_passed());
        apply_repo_updated(
            &mut app,
            SWEPT_PATH,
            rebuilt_from(&snapshot),
            git::FetchOutcome::NotAttempted,
        );
        let repo = &app.repos[0];
        assert_poll_fields(repo);
        assert_sweep_fields(repo);
    }

    /// The other completion order: the sweep lands first, then the poll.
    /// Both halves must end up on the row either way.
    #[test]
    fn a_poll_landing_after_a_sweep_keeps_the_sweeps_local_fields() {
        let mut app = app_with_pending_repo();
        let snapshot = sweep_targets(&app, None).remove(0).old;
        apply_repo_updated(
            &mut app,
            SWEPT_PATH,
            rebuilt_from(&snapshot),
            git::FetchOutcome::NotAttempted,
        );
        poll(&mut app, poll_says_passed());
        let repo = &app.repos[0];
        assert_poll_fields(repo);
        assert_sweep_fields(repo);
    }

    /// A failed poll changes no GitHub field, so the row keeps the last good
    /// values, and the sweep landing after it still updates the local half.
    #[test]
    fn a_failed_poll_mid_sweep_leaves_github_fields_and_lands_the_sweep() {
        let mut app = app_with_pending_repo();
        poll(&mut app, poll_says_passed());
        let snapshot = sweep_targets(&app, None).remove(0).old;
        poll(&mut app, Err("HTTP 502".to_string()));
        apply_repo_updated(
            &mut app,
            SWEPT_PATH,
            rebuilt_from(&snapshot),
            git::FetchOutcome::Succeeded,
        );
        assert!(app.github_freshness.is_failing());
        let repo = &app.repos[0];
        assert_poll_fields(repo);
        assert_sweep_fields(repo);
    }

    fn local(h: u32, m: u32) -> chrono::DateTime<chrono::Local> {
        use chrono::TimeZone;
        chrono::Local
            .with_ymd_and_hms(2026, 10, 1, h, m, 0)
            .single()
            .expect("unambiguous local time")
    }

    /// The payload a failed request finds on disk: the same repo, but CI as
    /// it was when the cache was written.
    fn offline_with_cache(written: Option<chrono::DateTime<chrono::Local>>) -> github::FetchError {
        let mut cached = poll_says_passed().ok().unwrap();
        cached.repos[0].ci_status = model::CiStatus::Failed;
        cached.origin = github::Origin::Cache { written };
        github::FetchError {
            failure: github::Failure::new(github::FailureKind::Offline, "no route"),
            fallback: Some(cached),
            meta: Default::default(),
        }
    }

    /// The case that was invisible. `fetch_repos` answered an offline poll
    /// with the cache as `Ok`, and this cleared the failure, restarted the
    /// clock and wrote the cache's older CI over the rows. Now it is a
    /// failure: no row changes, the data time stays, the marker shows.
    #[test]
    fn a_failed_background_poll_with_a_cache_present_changes_no_row() {
        let mut app = app_with_pending_repo();
        poll(&mut app, poll_says_passed());
        let data_time = app.github_freshness.data_time;
        poll_with(&mut app, Err(offline_with_cache(Some(local(9, 0)))));

        assert_poll_fields(&app.repos[0]);
        assert!(app.github_freshness.is_failing(), "offline looked healthy");
        assert_eq!(app.github_freshness.data_time, data_time);
        assert!(
            ui::github_freshness_segment(&app.github_freshness, chrono::Local::now())
                .is_some_and(|(long, _, _)| long.contains("⚠ github offline")),
            "no marker for an offline poll"
        );
    }

    /// Rule 7: the first success after a failure clears it on the same frame
    /// and lands its data.
    #[test]
    fn a_successful_poll_after_a_failure_clears_it() {
        let mut app = app_with_pending_repo();
        poll_with(&mut app, Err(offline_with_cache(None)));
        assert!(app.github_freshness.is_failing());
        poll(&mut app, poll_says_passed());
        assert_eq!(app.github_freshness.outcome, app::Outcome::Success);
        assert!(matches!(
            app.github_freshness.data_time,
            app::DataTime::At(_)
        ));
        assert!(
            ui::github_freshness_segment(&app.github_freshness, chrono::Local::now()).is_none()
        );
        assert_poll_fields(&app.repos[0]);
    }

    fn watching(app: &mut App) -> config::Config {
        app.watch_mode = true;
        config::Config {
            github_poll_interval: 90,
            ..config::Config::default()
        }
    }

    /// Rule 6: a failure advances the retry clock, so the next automatic
    /// attempt waits one whole interval. Not at once, and not in a loop.
    #[test]
    fn no_attempt_sooner_than_one_interval_after_a_failure() {
        let mut app = app_with_pending_repo();
        app.repos[0].ci_status = model::CiStatus::Passed;
        let config = watching(&mut app);
        poll(&mut app, Err("HTTP 502".to_string()));
        let failed_at = app.github_freshness.last_attempt.expect("clock advanced");

        let early = failed_at + chrono::Duration::seconds(89);
        assert!(!github_poll_due(&app, &config, early), "retried early");
        let on_time = failed_at + chrono::Duration::seconds(90);
        assert!(github_poll_due(&app, &config, on_time));

        // A detected push does not jump the queue while polls fail either.
        app.push_detected = true;
        assert!(!github_poll_due(&app, &config, early), "push retried early");
    }

    /// Decision D4: a poll that never answers is recorded as a timed-out
    /// failure and advances the clock, so the next tick does not open a
    /// second request beside the first.
    #[test]
    fn a_poll_given_up_on_is_recorded_as_a_timed_out_failure() {
        let mut app = app_with_pending_repo();
        let config = watching(&mut app);
        claim_github_poll(&mut app).unwrap();
        app.github_poll_started = std::time::Instant::now()
            .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
        let now = chrono::Local::now();
        expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, now);

        assert!(!app.github_poll_inflight);
        assert!(matches!(
            &app.github_freshness.outcome,
            app::Outcome::Failed(f) if f.kind == github::FailureKind::TimedOut
        ));
        assert_eq!(app.github_freshness.last_attempt, Some(now));
        assert!(!github_poll_due(&app, &config, now), "restarted at once");
    }

    /// Rule 10: once a newer attempt has been applied, a late answer from
    /// the one given up on is discarded, so the rows and the data time
    /// never go backwards.
    #[test]
    fn an_answer_from_an_older_attempt_is_discarded_once_a_newer_one_applied() {
        let mut app = app_with_pending_repo();
        let first = claim_github_poll(&mut app).unwrap();
        app.github_poll_started = std::time::Instant::now()
            .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
        expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, chrono::Local::now());
        let second = claim_github_poll(&mut app).unwrap();
        answer(&mut app, second, Ok(poll_says_passed().ok().unwrap()));
        let applied = app.github_freshness.clone();

        let mut late = poll_says_passed().ok().unwrap();
        late.repos[0].ci_status = model::CiStatus::Failed;
        late.repos[0].pushed_at = app.dataset[0].pushed_at;
        answer(&mut app, first, Ok(late));

        assert_poll_fields(&app.repos[0]);
        assert_eq!(app.github_freshness, applied);
    }

    /// The other order: the given-up attempt answers while the newer one is
    /// still out. Its data is the newest there is, so it lands, but the lock
    /// belongs to the newer request and stays held.
    #[test]
    fn a_late_answer_does_not_release_the_newer_attempts_lock() {
        let mut app = app_with_pending_repo();
        let first = claim_github_poll(&mut app).unwrap();
        app.github_poll_started = std::time::Instant::now()
            .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
        expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, chrono::Local::now());
        let _second = claim_github_poll(&mut app).unwrap();
        answer(&mut app, first, Ok(poll_says_passed().ok().unwrap()));

        assert_poll_fields(&app.repos[0]);
        assert!(app.github_poll_inflight, "the newer request lost its lock");
        assert!(app.github_poll_started.is_some());
    }

    /// Decision D3: a `Pending` frozen by a failing poll does not hold the
    /// interval at the floor.
    #[test]
    fn a_pending_repo_does_not_shorten_the_interval_while_polls_fail() {
        let config = config::Config {
            github_poll_interval: 90,
            ..config::Config::default()
        };
        let mut app = app_with(&[model::CiStatus::Pending]);
        assert_eq!(github_poll_secs(&app, &config), GITHUB_POLL_MIN as i64);
        app.github_freshness.failed(
            github::Failure::new(github::FailureKind::Offline, "no route"),
            chrono::Local::now(),
        );
        assert_eq!(github_poll_secs(&app, &config), 90);
    }

    fn launch(app: &mut App, kind: LoadKind, source: app::DatasetSource) {
        apply_load_result(
            app,
            &config::Config::default(),
            Ok(Box::new(LoadResult {
                kind,
                repos: Vec::new(),
                header: "h".to_string(),
                dataset: Some(Vec::new()),
                github_calendar: None,
                source,
                ambiguity: None,
                meta: None,
            })),
        );
    }

    /// Decision D2: a cache-served launch is followed by one attempt even with
    /// watch off, and only one. Watch decides whether polling continues.
    #[test]
    fn a_cache_served_launch_with_watch_off_makes_exactly_one_attempt() {
        for outcome in [Ok(()), Err(())] {
            let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
            let config = config::Config::default();
            assert!(!app.watch_mode);
            assert!(
                !github_poll_due(&app, &config, chrono::Local::now()),
                "before load"
            );
            launch(
                &mut app,
                LoadKind::Initial,
                app::DatasetSource::Cache {
                    written: Some(local(8, 0)),
                },
            );
            assert!(github_poll_due(&app, &config, chrono::Local::now()));

            let attempt = claim_github_poll(&mut app).unwrap();
            assert!(!github_poll_due(&app, &config, chrono::Local::now()));
            let result = match outcome {
                Ok(()) => poll_says_passed(),
                Err(()) => Err("HTTP 502".to_string()),
            };
            let result = result.map_err(|m| github::FetchError {
                failure: github::Failure::new(github::FailureKind::Http, m),
                fallback: None,
                meta: Default::default(),
            });
            answer(&mut app, attempt, result);

            let tomorrow = chrono::Local::now() + chrono::Duration::days(1);
            assert!(
                !github_poll_due(&app, &config, tomorrow),
                "watch off kept polling after {outcome:?}"
            );
        }
    }

    /// A network-served launch with watch off owes nothing more.
    #[test]
    fn a_network_served_launch_with_watch_off_does_not_poll() {
        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        launch(&mut app, LoadKind::Initial, app::DatasetSource::Network);
        let tomorrow = chrono::Local::now() + chrono::Duration::days(1);
        assert!(!github_poll_due(&app, &config::Config::default(), tomorrow));
    }

    /// Rule 1 with a planted age: the cache's write time is the data time,
    /// and no attempt is recorded, so the poll stays due.
    #[test]
    fn a_launch_from_a_cache_of_planted_age_carries_that_age() {
        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        let written = chrono::Local::now() - chrono::Duration::hours(3);
        launch(
            &mut app,
            LoadKind::Initial,
            app::DatasetSource::Cache {
                written: Some(written),
            },
        );
        assert_eq!(app.github_freshness.data_time, app::DataTime::At(written));
        assert_eq!(app.github_freshness.outcome, app::Outcome::NotAttempted);
        assert!(app.github_freshness.last_attempt.is_none());
    }

    /// A cache whose age cannot be read stays unknown: never the launch time.
    #[test]
    fn a_launch_from_a_cache_of_unreadable_age_leaves_the_age_unknown() {
        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        launch(
            &mut app,
            LoadKind::Initial,
            app::DatasetSource::Cache { written: None },
        );
        assert_eq!(app.github_freshness.data_time, app::DataTime::Unknown);
    }

    /// No usable cache and the request fails: the error panel, as before, and
    /// the failed attempt is recorded so the retry clock advances.
    #[test]
    fn a_launch_with_no_usable_cache_records_the_failed_attempt() {
        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        let failure = github::Failure::new(github::FailureKind::Auth, "not authenticated");
        apply_load_result(
            &mut app,
            &config::Config::default(),
            Err(LoadError {
                message: failure.message.clone(),
                github_failure: Some(failure.clone()),
                budget: None,
            }),
        );
        assert_eq!(app.load_error.as_deref(), Some("not authenticated"));
        assert_eq!(app.github_freshness.outcome, app::Outcome::Failed(failure));
        assert!(app.github_freshness.last_attempt.is_some());
        assert_eq!(app.github_freshness.data_time, app::DataTime::NoData);
    }

    /// An `R` that went to GitHub, landing over the row `app_with_pending_repo`
    /// draws, the way a reload rebuilds it: CI as the reload saw it.
    fn reload_lands(app: &mut App, source: app::DatasetSource) {
        let mut repo = app.repos[0].clone();
        repo.ci_status = model::CiStatus::Passed;
        apply_load_result(
            app,
            &config::Config::default(),
            Ok(Box::new(LoadResult {
                kind: LoadKind::Reload,
                repos: vec![repo],
                header: "h".to_string(),
                dataset: Some(Vec::new()),
                github_calendar: None,
                source,
                ambiguity: None,
                meta: None,
            })),
        );
    }

    /// A poll answer older than the `R` that landed: CI as it was before.
    fn stale_answer() -> Result<Box<github::GithubFetch>, github::FetchError> {
        let mut old = poll_says_passed().ok().unwrap();
        old.repos[0].ci_status = model::CiStatus::Failed;
        Ok(old)
    }

    /// Rule 10 with `R` as the newer attempt. A watch poll hangs and is
    /// given up on, `R` succeeds, then the hung request answers. Loads never
    /// touched the attempt ids, so the old payload was written over the
    /// reload's rows and stamped fresh.
    #[test]
    fn a_late_poll_answer_after_a_successful_reload_is_discarded() {
        let mut app = app_with_pending_repo();
        let hung = claim_github_poll(&mut app).unwrap();
        app.github_poll_started = std::time::Instant::now()
            .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
        expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, chrono::Local::now());
        reload_lands(&mut app, app::DatasetSource::Network);
        let after_reload = app.github_freshness.clone();

        answer(&mut app, hung, stale_answer());

        assert_eq!(
            app.repos[0].ci_status,
            model::CiStatus::Passed,
            "rows rewound"
        );
        assert_eq!(app.github_freshness, after_reload, "freshness rewound");
        assert!(!app.github_poll_inflight);
        assert!(app.github_poll_started.is_none());
        let config = watching(&mut app);
        let later = chrono::Local::now() + chrono::Duration::seconds(90);
        assert!(
            github_poll_due(&app, &config, later),
            "polling never resumed"
        );
    }

    /// The mirror case: the poll is still in flight, not given up on, when
    /// `R` lands, with an `r` waiting on it. `R` wins: it is applied first and
    /// rebuilt every row, and an answer that cannot be ordered against it
    /// must not overwrite it. The answer still releases the lock and settles
    /// the waiting `r`, or polling and the spinner would stall for good.
    #[test]
    fn a_poll_in_flight_when_a_reload_lands_is_superseded_but_releases_its_lock() {
        let mut app = app_with_pending_repo();
        let in_flight = claim_github_poll(&mut app).unwrap();
        app.refresh_awaits_poll = true;
        reload_lands(&mut app, app::DatasetSource::Network);
        let after_reload = app.github_freshness.clone();
        assert!(
            app.github_poll_inflight,
            "the lock is the request's to release"
        );

        answer(&mut app, in_flight, stale_answer());

        assert_eq!(
            app.repos[0].ci_status,
            model::CiStatus::Passed,
            "rows rewound"
        );
        assert_eq!(app.github_freshness, after_reload);
        assert!(!app.github_poll_inflight, "lock left set");
        assert!(app.github_poll_started.is_none());
        assert!(!app.refresh_awaits_poll, "the waiting r never settled");
        assert!(claim_github_poll(&mut app).is_some(), "no poll can start");
    }

    /// A reload that fell back to the cache supersedes the same way: its rows
    /// were rebuilt from the file, and a poll started before it is older.
    #[test]
    fn a_reload_fallback_also_supersedes_an_older_poll() {
        let mut app = app_with_pending_repo();
        let in_flight = claim_github_poll(&mut app).unwrap();
        reload_lands(
            &mut app,
            app::DatasetSource::Fallback {
                failure: github::Failure::new(github::FailureKind::Offline, "no route"),
                written: None,
            },
        );
        answer(&mut app, in_flight, stale_answer());
        assert_eq!(app.repos[0].ci_status, model::CiStatus::Passed);
        assert!(app.github_freshness.is_failing());
        assert!(!app.github_poll_inflight);
    }

    /// A cache-served launch made no attempt and is older than any poll, so
    /// a poll in flight past it still lands.
    #[test]
    fn a_cache_served_load_does_not_supersede_a_poll() {
        let mut app = app_with_pending_repo();
        let in_flight = claim_github_poll(&mut app).unwrap();
        reload_lands(&mut app, app::DatasetSource::Cache { written: None });
        answer(
            &mut app,
            in_flight,
            poll_says_passed().map_err(|_| unreachable!()),
        );
        assert_poll_fields(&app.repos[0]);
        assert_eq!(app.github_freshness.outcome, app::Outcome::Success);
    }

    /// `R` exists to be current. One that could only rebuild from the cache
    /// says so instead of `Reloaded`, and the header goes stale with the
    /// cache file's age.
    #[test]
    fn a_reload_that_fell_back_to_the_cache_does_not_claim_a_reload() {
        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        let written = local(7, 30);
        launch(
            &mut app,
            LoadKind::Reload,
            app::DatasetSource::Fallback {
                failure: github::Failure::new(github::FailureKind::Offline, "no route"),
                written: Some(written),
            },
        );
        let status = app.status_message.as_ref().map(|m| m.text.clone()).unwrap();
        assert_ne!(status, "Reloaded");
        assert!(status.contains("cache"), "{status:?}");
        assert!(app.github_freshness.is_failing());
        assert!(app.github_freshness.last_attempt.is_some());
        assert_eq!(app.github_freshness.data_time, app::DataTime::At(written));

        launch(&mut app, LoadKind::Reload, app::DatasetSource::Network);
        let status = app.status_message.as_ref().map(|m| m.text.clone()).unwrap();
        assert_eq!(status, "Reloaded");
    }

    /// Rows the sweep does not probe keep their current values: identity,
    /// groups and owner come from the load, not from the snapshot. The
    /// selection is the app's, and a sweep result must not move it.
    #[test]
    fn a_sweep_result_keeps_identity_groups_and_selection() {
        let mut app = app_with_pending_repo();
        let snapshot = sweep_targets(&app, None).remove(0).old;
        let selected = app.selected;
        let mut status = rebuilt_from(&snapshot);
        // Whatever a stale snapshot carried for fields the sweep does not
        // own, the row as it is now wins.
        status.groups = vec![];
        status.owner = None;
        status.remote_url = None;
        apply_repo_updated(
            &mut app,
            SWEPT_PATH,
            status,
            git::FetchOutcome::NotAttempted,
        );
        let repo = &app.repos[0];
        assert_eq!(repo.name, "r");
        assert_eq!(repo.path.as_deref(), Some(SWEPT_PATH));
        assert_eq!(repo.groups, vec!["work".to_string()]);
        assert_eq!(repo.owner.as_deref(), Some("Gaurgle"));
        assert_eq!(
            repo.remote_url.as_deref(),
            Some("git@github.com:Gaurgle/r.git")
        );
        assert_eq!(app.selected, selected);
        assert_eq!(app.repos.len(), 1);
    }

    /// The single-repo sweep (`f`, a moved remote) reports `fetched: true`
    /// and the full watch sweep often does not; both land through the same
    /// merge, so the poll's fields survive either.
    #[test]
    fn single_and_full_sweep_results_merge_alike() {
        for fetched in [false, true] {
            let mut app = app_with_pending_repo();
            let single = sweep_targets(&app, Some(0)).remove(0).old;
            let full = sweep_targets(&app, None).remove(0).old;
            poll(&mut app, poll_says_passed());
            let snapshot = if fetched { single } else { full };
            let fetch = if fetched {
                git::FetchOutcome::Succeeded
            } else {
                git::FetchOutcome::NotAttempted
            };
            apply_repo_updated(&mut app, SWEPT_PATH, rebuilt_from(&snapshot), fetch);
            assert_poll_fields(&app.repos[0]);
            assert_sweep_fields(&app.repos[0]);
        }
    }

    /// A late result for a path that no longer has a row is dropped: it must
    /// not create a row, nor land on an unrelated one.
    #[test]
    fn a_late_result_for_a_removed_repo_adds_no_row() {
        let mut app = app_with_pending_repo();
        let snapshot = sweep_targets(&app, None).remove(0).old;
        let mut other = test_repo();
        other.name = "other".to_string();
        other.path = Some("~/repos/other".to_string());
        app.repos = vec![other];
        apply_repo_updated(
            &mut app,
            SWEPT_PATH,
            rebuilt_from(&snapshot),
            git::FetchOutcome::Succeeded,
        );
        assert_eq!(app.repos.len(), 1);
        let repo = &app.repos[0];
        assert_eq!(repo.name, "other");
        assert_eq!(repo.dirty, 0);
        assert!(repo.commits.is_empty());
    }

    /// S15 of the repository identity design (section 3.6): a sweep result
    /// lands by path, on whatever row a reload has put there since, even when
    /// that row stands for another remote. Every field a sweep writes is a
    /// probe of the directory, so it is right for the row at that path; the
    /// row's identity, GitHub fields and groups stay the reload's.
    #[test]
    fn a_late_result_lands_by_path_on_the_row_a_reload_put_there() {
        let mut app = app_with_pending_repo();
        let snapshot = sweep_targets(&app, None).remove(0).old;
        let mut now_there = test_repo();
        now_there.name = "y".to_string();
        now_there.path = Some(SWEPT_PATH.to_string());
        now_there.owner = Some("Gaurgle".to_string());
        now_there.remote_url = Some("git@github.com:Gaurgle/y.git".to_string());
        now_there.ci_status = model::CiStatus::Passed;
        now_there.groups = vec!["side".to_string()];
        app.repos = vec![now_there];

        apply_repo_updated(
            &mut app,
            SWEPT_PATH,
            rebuilt_from(&snapshot),
            git::FetchOutcome::NotAttempted,
        );

        assert_eq!(app.repos.len(), 1);
        let repo = &app.repos[0];
        assert_eq!(repo.name, "y");
        assert_eq!(repo.owner.as_deref(), Some("Gaurgle"));
        assert_eq!(
            repo.remote_url.as_deref(),
            Some("git@github.com:Gaurgle/y.git")
        );
        assert_eq!(repo.ci_status, model::CiStatus::Passed);
        assert_eq!(repo.groups, vec!["side".to_string()]);
        assert_sweep_fields(repo);
    }

    // --- FZ-04: fetch outcomes land as outcomes, not as "requested" ---

    fn fetch_failed(kind: git::FetchFailureKind) -> git::FetchOutcome {
        git::FetchOutcome::Failed(git::FetchFailure {
            kind,
            detail: Some("fatal: '/gone' does not appear to be a git repository".to_string()),
        })
    }

    /// One fetch that worked: the row says so and has a success time.
    #[test]
    fn a_successful_fetch_lands_as_a_success() {
        let mut app = app_with_pending_repo();
        let snapshot = sweep_targets(&app, Some(0)).remove(0).old;
        apply_repo_updated(
            &mut app,
            SWEPT_PATH,
            rebuilt_from(&snapshot),
            git::FetchOutcome::Succeeded,
        );
        let facts = &app.repos[0].fetch;
        assert_eq!(facts.outcome, git::FetchOutcome::Succeeded);
        assert!(facts.last_success.is_some());
    }

    /// Rule 1 and rule 4: a fetch from a remote that is gone lands as a
    /// failure, not as a fetch. The rebuilt local state still lands, the
    /// last success stays where it was, and nothing claims the refs moved.
    #[test]
    fn a_failed_fetch_lands_as_a_failure_and_keeps_the_last_success() {
        let mut app = app_with_pending_repo();
        let earlier = chrono::Local::now() - chrono::Duration::hours(1);
        app.repos[0].fetch = model::FetchFacts::from_disk(Some(earlier.into()));
        let snapshot = sweep_targets(&app, Some(0)).remove(0).old;
        apply_repo_updated(
            &mut app,
            SWEPT_PATH,
            rebuilt_from(&snapshot),
            fetch_failed(git::FetchFailureKind::NoRemote),
        );
        let repo = &app.repos[0];
        assert_sweep_fields(repo);
        assert_eq!(
            repo.fetch.failure().map(|f| f.kind),
            Some(git::FetchFailureKind::NoRemote)
        );
        assert_eq!(repo.fetch.last_success, Some(earlier));
        assert!(repo.fetch.last_attempt.is_some());
    }

    /// A timeout is a failure of its own kind, settled like any other.
    #[test]
    fn a_timed_out_fetch_lands_as_timed_out_and_settles() {
        let mut app = app_with_pending_repo();
        app.refresh_awaits_poll = true;
        app.settle_refresh_poll([SWEPT_PATH.to_string()]);
        let snapshot = sweep_targets(&app, Some(0)).remove(0).old;
        let timed_out = git::FetchOutcome::Failed(git::FetchFailure {
            kind: git::FetchFailureKind::TimedOut,
            detail: None,
        });
        apply_repo_updated(&mut app, SWEPT_PATH, rebuilt_from(&snapshot), timed_out);
        assert_eq!(
            app.repos[0].fetch.failure().map(|f| f.kind),
            Some(git::FetchFailureKind::TimedOut)
        );
        assert!(
            !app.is_refreshing(),
            "a timed-out fetch left the spinner running"
        );
    }

    /// Rule 2: a watch sweep that did not fetch lands its local fields and
    /// leaves the failure, its time and the last success exactly as they were.
    #[test]
    fn a_sweep_that_did_not_fetch_leaves_the_fetch_facts_alone() {
        let mut app = app_with_pending_repo();
        let snapshot = sweep_targets(&app, Some(0)).remove(0).old;
        apply_repo_updated(
            &mut app,
            SWEPT_PATH,
            rebuilt_from(&snapshot),
            fetch_failed(git::FetchFailureKind::Auth),
        );
        let before = app.repos[0].fetch.clone();
        // The snapshot predates the failure; its copy of the facts must not
        // land either.
        apply_repo_updated(
            &mut app,
            SWEPT_PATH,
            rebuilt_from(&snapshot),
            git::FetchOutcome::NotAttempted,
        );
        assert_eq!(app.repos[0].fetch, before);
    }

    /// Recovery: the next fetch that works clears the failure.
    #[test]
    fn a_fetch_that_works_after_a_failure_recovers() {
        let mut app = app_with_pending_repo();
        let snapshot = sweep_targets(&app, Some(0)).remove(0).old;
        apply_repo_updated(
            &mut app,
            SWEPT_PATH,
            rebuilt_from(&snapshot),
            fetch_failed(git::FetchFailureKind::Unreachable),
        );
        apply_repo_updated(
            &mut app,
            SWEPT_PATH,
            rebuilt_from(&snapshot),
            git::FetchOutcome::Succeeded,
        );
        let facts = &app.repos[0].fetch;
        assert_eq!(facts.failure(), None);
        assert!(facts.last_success.is_some());
    }

    /// An app with `n` cloned repos at `~/repos/repo-<i>`.
    fn fleet_app(n: usize) -> App {
        let repos = (0..n)
            .map(|i| {
                let mut r = test_repo();
                r.name = format!("repo-{i}");
                r.path = Some(format!("~/repos/repo-{i}"));
                r
            })
            .collect();
        App::new(repos, "h".to_string(), 120, 44)
    }

    fn land(app: &mut App, i: usize, fetch: git::FetchOutcome) {
        let path = format!("~/repos/repo-{i}");
        let status = app.repos[i].clone();
        apply_repo_updated(app, &path, status, fetch);
    }

    /// Rule 9: a batch someone asked for, some of whose fetches failed, says
    /// how many, and only once the last has landed.
    #[test]
    fn a_mixed_sweep_reports_how_many_fetches_failed() {
        let mut app = fleet_app(3);
        app.manual_fetches
            .expect((0..3).map(|i| format!("~/repos/repo-{i}")));
        land(&mut app, 0, git::FetchOutcome::Succeeded);
        land(&mut app, 1, fetch_failed(git::FetchFailureKind::Auth));
        assert!(
            app.status_message.is_none(),
            "reported before the last landed"
        );
        land(
            &mut app,
            2,
            fetch_failed(git::FetchFailureKind::Unreachable),
        );
        let status = app.status_message.as_ref().expect("a report");
        assert_eq!(status.text, "2 of 3 fetches failed");
        assert!(status.is_error);
        assert_eq!(app.repos[0].fetch.failure(), None);
        assert!(app.repos[1].fetch.failure().is_some());
        assert!(app.repos[2].fetch.failure().is_some());
    }

    /// Nothing extra when every fetch worked.
    #[test]
    fn a_sweep_whose_fetches_all_worked_reports_nothing() {
        let mut app = fleet_app(2);
        app.manual_fetches
            .expect((0..2).map(|i| format!("~/repos/repo-{i}")));
        land(&mut app, 0, git::FetchOutcome::Succeeded);
        land(&mut app, 1, git::FetchOutcome::Succeeded);
        assert!(app.status_message.is_none());
    }

    /// `f` on one repo whose fetch fails names the repo and why.
    #[test]
    fn a_single_fetch_that_fails_names_the_repo() {
        let mut app = fleet_app(1);
        app.manual_fetches.expect(["~/repos/repo-0".to_string()]);
        land(&mut app, 0, fetch_failed(git::FetchFailureKind::NoRemote));
        let status = app.status_message.as_ref().expect("a report");
        assert_eq!(status.text, "fetch failed: repo-0, remote not found");
    }

    /// Rule 6: a failure starts nothing. No sweep, no poll, no push signal;
    /// the repo is fetched again only when it would have been anyway.
    #[test]
    fn a_failed_fetch_schedules_nothing_new() {
        let mut app = fleet_app(1);
        let refreshed = app.last_refreshed;
        land(
            &mut app,
            0,
            fetch_failed(git::FetchFailureKind::Unreachable),
        );
        assert!(app.sweep_started.is_none());
        assert!(!app.github_poll_inflight);
        assert!(!app.push_detected);
        assert_eq!(app.last_refreshed, refreshed);
    }

    fn offline_outcome() -> app::Outcome {
        app::Outcome::Failed(github::Failure::new(
            github::FailureKind::Offline,
            "no route",
        ))
    }

    /// Rule 6, amended: only the step from an `Offline` poll to a success
    /// selects, and only the rows the outage failed (`Unreachable`,
    /// `TimedOut`), never one marked for another reason or a clean one.
    #[test]
    fn a_poll_recovering_from_offline_refetches_only_the_network_failures() {
        let mut app = fleet_app(7);
        land(
            &mut app,
            0,
            fetch_failed(git::FetchFailureKind::Unreachable),
        );
        land(&mut app, 1, fetch_failed(git::FetchFailureKind::TimedOut));
        land(&mut app, 2, fetch_failed(git::FetchFailureKind::Auth));
        land(&mut app, 3, fetch_failed(git::FetchFailureKind::NoRemote));
        land(&mut app, 4, fetch_failed(git::FetchFailureKind::Other));
        land(&mut app, 5, git::FetchOutcome::Succeeded);
        // Row 6 was never fetched.
        let success = app::Outcome::Success;

        assert_eq!(
            recovery_refetch(&offline_outcome(), &success, &app.repos),
            vec![0, 1]
        );
        assert!(recovery_refetch(&success, &success, &app.repos).is_empty());
        assert!(recovery_refetch(&app::Outcome::NotAttempted, &success, &app.repos).is_empty());
        let still_failing = app::Outcome::Failed(github::Failure::new(
            github::FailureKind::Server,
            "HTTP 502",
        ));
        assert!(recovery_refetch(&offline_outcome(), &still_failing, &app.repos).is_empty());
        assert!(recovery_refetch(&offline_outcome(), &offline_outcome(), &app.repos).is_empty());
        let other_failure = app::Outcome::Failed(github::Failure::new(
            github::FailureKind::TimedOut,
            "no answer",
        ));
        assert!(recovery_refetch(&other_failure, &success, &app.repos).is_empty());
    }

    /// Rule 6, amended, through the poll: a row the outage failed is in the
    /// fetch batch of the first successful poll after an `Offline` one, the
    /// way a moved remote is, and not in the next poll's when it failed
    /// again: the recovery refetch happens once.
    #[tokio::test]
    async fn a_row_the_outage_failed_is_fetched_once_when_the_poll_recovers() {
        // A path that does not exist, so the spawned fetch touches nothing.
        let path = std::env::temp_dir()
            .join("fleetz-76-no-such-dir")
            .join("repo")
            .display()
            .to_string();
        let mut repo = test_repo();
        repo.path = Some(path.clone());
        let mut app = App::new(vec![repo], "h".to_string(), 120, 44);
        let status = app.repos[0].clone();
        let unreachable = || fetch_failed(git::FetchFailureKind::Unreachable);
        apply_repo_updated(&mut app, &path, status.clone(), unreachable());
        let ok = || {
            Ok(Box::new(github::GithubFetch {
                repos: Vec::new(),
                calendar: None,
                has_pr_counts: true,
                origin: github::Origin::Network,
                meta: Default::default(),
            }))
        };
        let offline = || {
            Err(github::FetchError {
                failure: github::Failure::new(github::FailureKind::Offline, "no route"),
                fallback: None,
                meta: Default::default(),
            })
        };

        app.refresh_awaits_poll = true;
        poll_with(&mut app, offline());
        assert!(app.refresh_fetches.is_empty(), "an offline poll fetched");

        app.refresh_awaits_poll = true;
        poll_with(&mut app, ok());
        assert_eq!(
            app.refresh_fetches.iter().collect::<Vec<_>>(),
            vec![&path],
            "the recovering poll did not refetch the failed row"
        );

        // Its refetch fails again; the next poll leaves it for the next
        // trigger.
        apply_repo_updated(&mut app, &path, status, unreachable());
        assert!(app.refresh_fetches.is_empty());
        app.refresh_awaits_poll = true;
        poll_with(&mut app, ok());
        assert!(
            app.refresh_fetches.is_empty(),
            "the recovery refetch repeated"
        );
        assert_eq!(
            app.repos[0].fetch.failure().map(|f| f.kind),
            Some(git::FetchFailureKind::Unreachable)
        );
    }

    /// A reload replaces every row; the session's fetch facts go across by
    /// path, so a window apply cannot wipe a failure off a row.
    #[test]
    fn a_reload_carries_the_fetch_facts_by_path() {
        let mut app = fleet_app(2);
        land(&mut app, 1, fetch_failed(git::FetchFailureKind::Auth));
        let reloaded = fleet_app(2).repos;
        let carried = carry_fetch_facts(&app.repos, reloaded);
        assert_eq!(carried[0].fetch, model::FetchFacts::default());
        assert_eq!(carried[1].fetch, app.repos[1].fetch);
    }

    /// FZ-34 (#83), criterion 3: the watch sweep rebuilds a row held after
    /// a timeout without fetching it, and fetches it again once the hold
    /// (two intervals after the first timeout) has passed or a fetch
    /// succeeded. Other failure kinds hold nothing.
    #[test]
    fn the_watch_sweep_skips_the_fetch_of_a_row_that_timed_out() {
        let mut app = fleet_app(3);
        app.refresh_interval = 60;
        land(&mut app, 0, fetch_failed(git::FetchFailureKind::TimedOut));
        land(
            &mut app,
            1,
            fetch_failed(git::FetchFailureKind::Unreachable),
        );
        let now = chrono::Local::now();
        let targets = sweep_targets(&app, None);
        assert_eq!(
            held_fetch_paths(&app, &targets, now),
            ["~/repos/repo-0".to_string()].into(),
        );
        assert!(
            held_fetch_paths(&app, &targets, now + chrono::Duration::seconds(121)).is_empty(),
            "still held after two intervals"
        );
        assert_eq!(
            app.repos[0].fetch.failure().map(|f| f.kind),
            Some(git::FetchFailureKind::TimedOut),
            "the row stopped showing its timeout"
        );
        // The automatic selection skips the held row and claims no fetch for
        // it; a manual one fetches it.
        let automatic = fetch_skips(&mut app, &targets, true, true);
        assert!(automatic.contains("~/repos/repo-0"), "{automatic:?}");
        assert!(!app.fetches_in_flight.contains_key("~/repos/repo-0"));
        assert!(app.fetches_in_flight.contains_key("~/repos/repo-1"));
        app.fetches_in_flight.clear();
        let manual = fetch_skips(&mut app, &targets, true, false);
        assert!(!manual.contains("~/repos/repo-0"), "{manual:?}");
        assert!(app.fetches_in_flight.contains_key("~/repos/repo-0"));
        land(&mut app, 0, git::FetchOutcome::Succeeded);
        assert!(held_fetch_paths(&app, &targets, now).is_empty());
    }

    /// FZ-34 (#83), R5: a fetching sweep claims each path it fetches, and
    /// the next selection, manual or automatic, sweeps a path whose fetch is
    /// still running without fetching it again. The landed outcome ends the
    /// claim; a sweep that did not fetch claims nothing.
    #[test]
    fn a_running_fetch_is_skipped_by_the_next_selection_until_it_lands() {
        let mut app = fleet_app(2);
        let targets = sweep_targets(&app, None);
        assert!(fetch_skips(&mut app, &targets, false, true).is_empty());
        assert!(
            app.fetches_in_flight.is_empty(),
            "a sweep without fetch claimed"
        );
        assert!(fetch_skips(&mut app, &targets, true, true).is_empty());
        let both: std::collections::HashSet<String> =
            ["~/repos/repo-0".to_string(), "~/repos/repo-1".to_string()].into();
        assert_eq!(fetch_skips(&mut app, &targets, true, false), both, "manual");
        assert_eq!(
            fetch_skips(&mut app, &targets, true, true),
            both,
            "automatic"
        );
        // A sweep that only rebuilt repo-0 lands first; its fetch still runs.
        land(&mut app, 0, git::FetchOutcome::NotAttempted);
        assert_eq!(fetch_skips(&mut app, &targets, true, false), both);
        land(&mut app, 0, git::FetchOutcome::Succeeded);
        assert_eq!(
            fetch_skips(&mut app, &targets, true, false),
            ["~/repos/repo-1".to_string()].into()
        );
    }

    /// An app whose rows sit at paths that do not exist, so a sweep spawned
    /// from a test touches nothing on disk.
    fn missing_paths_app(n: usize, tag: &str) -> App {
        let repos = (0..n)
            .map(|i| {
                let mut r = test_repo();
                r.name = format!("repo-{i}");
                r.path = Some(
                    std::env::temp_dir()
                        .join(format!("fleetz-83-{tag}-no-such-dir"))
                        .join(format!("repo-{i}"))
                        .display()
                        .to_string(),
                );
                r
            })
            .collect();
        App::new(repos, "h".to_string(), 120, 44)
    }

    fn timed_out_at(app: &mut App, i: usize) -> String {
        let path = app.repos[i].path.clone().unwrap();
        let status = app.repos[i].clone();
        apply_repo_updated(
            app,
            &path,
            status,
            fetch_failed(git::FetchFailureKind::TimedOut),
        );
        path
    }

    /// FZ-34 (#83), criterion 3 through `start_sweep`: the watch timer's
    /// sweep fetches every row but the one held after a timeout.
    #[tokio::test]
    async fn the_watch_timer_sweep_claims_no_fetch_for_a_held_row() {
        let mut app = missing_paths_app(3, "watch");
        let held = timed_out_at(&mut app, 1);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        start_sweep(&mut app, &tx, true, None, false);
        assert!(
            !app.fetches_in_flight.contains_key(&held),
            "held row fetched"
        );
        for i in [0, 2] {
            let path = app.repos[i].path.clone().unwrap();
            assert!(
                app.fetches_in_flight.contains_key(&path),
                "row {i} not fetched"
            );
        }
    }

    /// FZ-34 (#83): `f` is manual, so it fetches a row held after a timeout.
    #[tokio::test]
    async fn f_fetches_a_row_held_after_a_timeout() {
        let mut app = missing_paths_app(1, "fkey");
        let held = timed_out_at(&mut app, 0);
        assert!(app.fetch_held(&held, chrono::Local::now()));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        fetch_key(&mut app, &tx, None);
        assert!(
            app.fetches_in_flight.contains_key(&held),
            "f skipped the held row"
        );
    }

    /// FZ-35a (#84), criterion 3: at most one progress event per half
    /// second per fetch, the first at once.
    #[test]
    fn progress_events_are_throttled_to_two_a_second() {
        let mut throttle = ProgressThrottle::default();
        let t0 = std::time::Instant::now();
        let ms = std::time::Duration::from_millis;
        assert!(throttle.admit(t0));
        assert!(!throttle.admit(t0 + ms(1)));
        assert!(!throttle.admit(t0 + ms(499)));
        assert!(throttle.admit(t0 + ms(500)));
        assert!(!throttle.admit(t0 + ms(900)));
        assert!(throttle.admit(t0 + ms(1000)));
        // Two fetches throttle apart.
        let mut other = ProgressThrottle::default();
        assert!(other.admit(t0 + ms(1001)));
    }

    /// FZ-35a (#84), criterion 4: a sweep's fetch does not run when, by the
    /// time its permit is granted, a newer claim holds the path, and runs
    /// (taking the claim back) when the path is free. The path does not
    /// exist, so a fetch that ran fails rather than doing nothing.
    #[tokio::test]
    async fn a_fetch_runs_only_under_its_own_claim_at_the_permit() {
        let claims = app::FetchClaims::default();
        let missing = std::env::temp_dir().join("fleetz-84-no-such-dir");
        let t0 = std::time::Instant::now();
        let old = claims.reserve("~/r", t0).unwrap();
        claims.expire(
            std::time::Duration::ZERO,
            t0 + std::time::Duration::from_secs(1),
        );
        let newer = claims.reserve("~/r", t0).unwrap();
        assert_eq!(
            fetch_under_claim(&claims, "~/r", &missing, old, None).await,
            git::FetchOutcome::NotAttempted
        );
        assert_eq!(claims.token("~/r"), Some(newer));
        claims.release("~/r", Some(newer));
        assert!(matches!(
            fetch_under_claim(&claims, "~/r", &missing, old, None).await,
            git::FetchOutcome::Failed(_)
        ));
        assert_eq!(
            claims.token("~/r"),
            Some(old),
            "the fetch ran without its claim"
        );
    }

    /// FZ-35a (#84), criterion 4: a sweep's landing ends only the claim it
    /// held, and clears the row's progress.
    #[test]
    fn a_landing_from_an_expired_sweep_leaves_the_newer_claim() {
        let mut app = fleet_app(1);
        let path = app.repos[0].path.clone().unwrap();
        let status = app.repos[0].clone();
        let t0 = std::time::Instant::now();
        assert!(app.claim_fetch(&path, t0));
        let old = app.fetches_in_flight.token(&path);
        app.expire_fetches_in_flight(SWEEP_STUCK_AFTER, t0 + SWEEP_STUCK_AFTER * 2);
        assert!(app.claim_fetch(&path, t0 + SWEEP_STUCK_AFTER * 2));
        let newer = app.fetches_in_flight.token(&path);
        app.apply_fetch_progress(
            &path,
            git::FetchProgress {
                phase: git::FetchPhase::Receiving,
                objects_done: 1,
                objects_total: 2,
                bytes: 1,
            },
        );
        land_repo_update(
            &mut app,
            &path,
            status.clone(),
            git::FetchOutcome::Succeeded,
            old,
        );
        assert_eq!(app.fetches_in_flight.token(&path), newer);
        assert_eq!(app.repos[0].fetch.progress, None, "landing kept progress");
        land_repo_update(&mut app, &path, status, git::FetchOutcome::Succeeded, newer);
        assert!(!app.fetches_in_flight.contains_key(&path));
    }

    /// FZ-35a (#84), criteria 3 and 4 through a real sweep: the fetch's
    /// progress reaches the app as `FetchProgress` events, throttled, all
    /// before the `RepoUpdated` that lands it with the claim's token. The
    /// clone's upload-pack hook prints the progress, standing in for git's.
    #[tokio::test]
    async fn a_sweep_streams_its_fetch_progress_before_it_lands() {
        let scratch = wt_scratch("sweep_progress");
        let upstream = scratch.join("up");
        id_repo(&upstream, None, false);
        let clone = scratch.join("down");
        wt_sh(
            &scratch,
            &[
                "git",
                "clone",
                "-q",
                upstream.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );
        wt_sh(
            &upstream,
            &["git", "commit", "-q", "--allow-empty", "-m", "two"],
        );
        let hook = "i=1; while [ $i -le 5 ]; do \
             printf 'Receiving objects: %d%% (%d/5), %d.00 MiB | 1.00 MiB/s\\r' \
             $((i * 20)) $i $i >&2; sleep 0.3; i=$((i + 1)); done; git upload-pack";
        wt_sh(&clone, &["git", "config", "remote.origin.uploadpack", hook]);
        let mut row = test_repo();
        row.path = Some(clone.display().to_string());
        let mut app = App::new(vec![row], "h".to_string(), 120, 44);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let start = std::time::Instant::now();
        start_single_sweep(&mut app, &tx, 0, true, false, None);
        let path = clone.display().to_string();
        let token = app.fetches_in_flight.token(&path).expect("claimed");
        let mut progress = Vec::new();
        let (fetch, claim) = loop {
            match tokio::time::timeout(std::time::Duration::from_secs(20), rx.recv()).await {
                Ok(Some(AppEvent::FetchProgress {
                    path: p,
                    progress: update,
                })) => {
                    assert_eq!(p, path);
                    progress.push(update);
                }
                Ok(Some(AppEvent::RepoUpdated { fetch, claim, .. })) => break (fetch, claim),
                Ok(Some(_)) => {}
                other => panic!("the sweep never landed: {}", other.is_ok()),
            }
        };
        let took = start.elapsed();
        assert_eq!(fetch, git::FetchOutcome::Succeeded);
        assert_eq!(claim, Some(token));
        assert!(
            progress.iter().any(|p| p.bytes >= 1024 * 1024),
            "{progress:?}"
        );
        // Five hook updates and git's own lines, at most two a second.
        let allowed = 1 + (took.as_millis() / PROGRESS_EVENT_GAP.as_millis()) as usize;
        assert!(
            !progress.is_empty() && progress.len() <= allowed,
            "{} events in {took:?}",
            progress.len()
        );
        std::fs::remove_dir_all(&scratch).ok();
    }

    /// FZ-35a (#84), criterion 4: the load path claims its fetches. A clone
    /// whose path a sweep is fetching is not fetched again by the load (its
    /// row records no fetch, the sweep's claim stays); a free one is fetched
    /// under a claim given back when it ends. The clone has no remote, so
    /// git's fetch has nothing to do and nothing leaves the disk.
    #[tokio::test]
    async fn the_load_fetches_under_a_claim_and_skips_a_claimed_path() {
        let home = wt_scratch("load_claim");
        let root = home.join("R");
        let repo = root.join("svc");
        id_repo(&repo, None, false);
        let dataset = [id_entry(
            Some("x"),
            "svc",
            0,
            model::CiStatus::Unknown,
            0,
            model::RepoKind::Unknown,
        )];
        async fn assemble(
            dataset: &[github::GithubRepo],
            home: &Path,
            root: &Path,
            claims: &app::FetchClaims,
        ) -> Vec<RepoStatus> {
            let args = Args::parse_from(["fleetz"]);
            let config = config::Config::default();
            let (repos, _) = assemble_view_with(
                dataset,
                None,
                String::new(),
                &args,
                &config,
                home,
                &[root.to_path_buf()],
                true,
                None,
                claims,
            )
            .await
            .expect("assembly");
            repos
        }
        let key = tilde_display(&repo);

        let claims = app::FetchClaims::default();
        let sweeping = claims.reserve(&key, std::time::Instant::now()).unwrap();
        let repos = assemble(&dataset, &home, &root, &claims).await;
        let row = row_at(&repos, &repo);
        assert_eq!(row.fetch.last_attempt, None, "fetched a claimed path");
        assert_eq!(claims.token(&key), Some(sweeping));

        let claims = app::FetchClaims::default();
        let repos = assemble(&dataset, &home, &root, &claims).await;
        let row = row_at(&repos, &repo);
        assert!(
            row.fetch.last_attempt.is_some(),
            "the free path was not fetched"
        );
        assert_eq!(row.fetch.outcome, git::FetchOutcome::Succeeded);
        assert!(claims.is_empty(), "the load kept its claim");
        std::fs::remove_dir_all(&home).ok();
    }

    /// FZ-35b (#85), criterion 2 at launch and `R`: the load does not fetch a
    /// path the fetch cap holds, and a fetch it runs records the row's size
    /// baseline. The clone has no remote, so nothing leaves the disk.
    #[tokio::test]
    async fn the_load_skips_a_size_held_path_and_records_the_baseline() {
        let home = wt_scratch("load_size_hold");
        let root = home.join("R");
        let repo = root.join("svc");
        id_repo(&repo, None, false);
        let dataset = [github::GithubRepo {
            disk_usage: Some(777),
            ..id_entry(
                Some("x"),
                "svc",
                0,
                model::CiStatus::Unknown,
                0,
                model::RepoKind::Unknown,
            )
        }];
        let assemble = |held: HashSet<String>| {
            let (dataset, home, root) = (dataset.clone(), home.clone(), root.clone());
            async move {
                let args = Args::parse_from(["fleetz"]);
                let config = config::Config::default();
                assemble_view_held(
                    &dataset,
                    None,
                    String::new(),
                    &args,
                    &config,
                    &home,
                    &[root],
                    true,
                    None,
                    &app::FetchClaims::default(),
                    &held,
                )
                .await
                .expect("assembly")
                .0
            }
        };
        let key = tilde_display(&repo);
        let repos = assemble([key].into()).await;
        let row = row_at(&repos, &repo);
        assert_eq!(row.fetch.last_attempt, None, "fetched a held path");

        let repos = assemble(HashSet::new()).await;
        let row = row_at(&repos, &repo);
        assert_eq!(row.fetch.outcome, git::FetchOutcome::Succeeded);
        assert_eq!(row.fetch.disk_baseline, Some(777));
        std::fs::remove_dir_all(&home).ok();
    }

    const MIB: u64 = 1024 * 1024;

    /// A row grown past the cap (the pre-check) and a row whose fetch was
    /// cut at the cap, as the poll and a landing leave them.
    fn hold_sizes(app: &mut App, grown: usize, cut: usize) {
        app.repos[grown].fetch.disk_baseline = Some(1_000);
        app.repos[grown]
            .fetch
            .assess_growth(Some(1_000 + 300 * 1024), Some(200 * MIB));
        let path = app.repos[cut].path.clone().unwrap();
        let status = app.repos[cut].clone();
        apply_repo_updated(
            app,
            &path,
            status,
            git::FetchOutcome::Failed(git::FetchFailure::too_big(201 * MIB)),
        );
    }

    /// FZ-35b (#85), criterion 4: `f` fetches a held row, uncapped; `F` and
    /// the automatic selections skip it and stay capped.
    #[test]
    fn f_fetches_a_held_row_uncapped_and_shift_f_and_automatic_do_not() {
        let mut app = fleet_app(3);
        hold_sizes(&mut app, 0, 1);
        let targets = sweep_targets(&app, None);
        let held: HashSet<String> =
            ["~/repos/repo-0".to_string(), "~/repos/repo-1".to_string()].into();
        for ask in [FetchAsk::Automatic, FetchAsk::All] {
            app.fetches_in_flight.clear();
            assert_eq!(
                fetch_skips_for(&mut app, &targets, true, ask),
                held,
                "{ask:?}"
            );
            assert!(app.fetches_in_flight.contains_key("~/repos/repo-2"));
            assert_eq!(ask.cap(Some(200 * MIB)), Some(200 * MIB), "{ask:?}");
        }
        app.fetches_in_flight.clear();
        assert!(fetch_skips_for(&mut app, &targets, true, FetchAsk::One).is_empty());
        assert_eq!(FetchAsk::One.cap(Some(200 * MIB)), None);
        // `fetch_max_mb = 0`: nothing is capped.
        assert_eq!(FetchAsk::All.cap(None), None);
    }

    /// FZ-35b (#85), review F1: the cap each sweep entry point hands
    /// `spawn_sweep` (it takes the plan, so the plan's cap is the one the
    /// fetches run under): `f` uncapped, `F`, the watch timer and the poll's
    /// single sweeps at the configured cap, and nothing capped at `0`.
    #[test]
    fn each_sweep_entry_point_plans_its_fetch_cap() {
        let mut app = fleet_app(1);
        app.fetch_max_bytes = Some(7 * MIB);
        let targets = sweep_targets(&app, None);
        let mut cap_for = |ask: FetchAsk| {
            app.fetches_in_flight.clear();
            plan_sweep_fetch(&mut app, &targets, true, ask).max_bytes
        };
        assert_eq!(cap_for(FetchAsk::for_single_sweep(false)), None, "f");
        assert_eq!(
            cap_for(FetchAsk::for_single_sweep(true)),
            Some(7 * MIB),
            "moved remote"
        );
        assert_eq!(cap_for(FetchAsk::for_full_sweep(true)), Some(7 * MIB), "F");
        assert_eq!(
            cap_for(FetchAsk::for_full_sweep(false)),
            Some(7 * MIB),
            "watch"
        );
        app.fetch_max_bytes = None;
        app.fetches_in_flight.clear();
        let plan = plan_sweep_fetch(&mut app, &targets, true, FetchAsk::for_full_sweep(true));
        assert_eq!(plan.max_bytes, None, "fetch_max_mb = 0");
    }

    /// The same through the keys' own entry points: `F` claims no fetch for
    /// a held row, `f` on it does.
    #[tokio::test]
    async fn shift_f_skips_a_held_row_and_f_on_it_fetches() {
        let mut app = missing_paths_app(3, "sizehold");
        hold_sizes(&mut app, 0, 1);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        start_sweep(&mut app, &tx, true, None, true);
        for i in [0, 1] {
            let path = app.repos[i].path.clone().unwrap();
            assert!(
                !app.fetches_in_flight.contains_key(&path),
                "F fetched held row {i}"
            );
        }
        let free = app.repos[2].path.clone().unwrap();
        assert!(app.fetches_in_flight.contains_key(&free));
        for i in [0, 1] {
            start_single_sweep(&mut app, &tx, i, true, false, None);
            let path = app.repos[i].path.clone().unwrap();
            assert!(
                app.fetches_in_flight.contains_key(&path),
                "f skipped held row {i}"
            );
        }
    }

    /// FZ-35b (#85), review B1: `F` reports over the fetches it ran. A row
    /// the fetch cap holds is only rebuilt and lands `NotAttempted`, so it
    /// must not be awaited, or the batch never completes and a failure among
    /// the others never reaches the status line (rule 9).
    #[tokio::test]
    async fn shift_f_reports_its_failures_with_a_size_held_row() {
        let mut app = missing_paths_app(3, "ftally");
        app.repos[0].fetch.disk_baseline = Some(1_000);
        app.repos[0]
            .fetch
            .assess_growth(Some(1_000 + 300 * 1024), Some(200 * MIB));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        start_sweep(&mut app, &tx, true, None, true);
        for (i, fetch) in [
            git::FetchOutcome::NotAttempted,
            fetch_failed(git::FetchFailureKind::Unreachable),
            git::FetchOutcome::Succeeded,
        ]
        .into_iter()
        .enumerate()
        {
            let path = app.repos[i].path.clone().unwrap();
            let status = app.repos[i].clone();
            apply_repo_updated(&mut app, &path, status, fetch);
        }
        let status = app.status_message.as_ref().map(|m| m.text.clone());
        assert_eq!(status.as_deref(), Some("1 of 2 fetches failed: repo-1"));
    }

    /// FZ-35b (#85), criterion 2 for moved remotes: a poll that sees a remote
    /// grow past the cap since the last fetch holds the row and does not
    /// fetch it; a moved remote that did not grow is fetched as before.
    #[tokio::test]
    async fn a_moved_remote_grown_past_the_cap_is_held_not_fetched() {
        let mut app = missing_paths_app(2, "moved");
        let before = "2026-10-20T10:00:00Z";
        let after = "2026-10-20T11:00:00Z";
        let sized = |name: &str, pushed: &str, kb: u64| github::GithubRepo {
            disk_usage: Some(kb),
            ..gh(name, pushed)
        };
        app.dataset = vec![
            sized("repo-0", before, 1_000),
            sized("repo-1", before, 1_000),
        ];
        for repo in &mut app.repos {
            repo.fetch.disk_baseline = Some(1_000);
        }
        app.refresh_awaits_poll = true;
        poll_with(
            &mut app,
            Ok(Box::new(github::GithubFetch {
                repos: vec![
                    sized("repo-0", after, 1_000 + 300 * 1024),
                    sized("repo-1", after, 1_000 + 10 * 1024),
                ],
                calendar: None,
                has_pr_counts: true,
                origin: github::Origin::Network,
                meta: Default::default(),
            })),
        );
        let (grown, quiet) = (
            app.repos[0].path.clone().unwrap(),
            app.repos[1].path.clone().unwrap(),
        );
        assert_eq!(
            app.repos[0].fetch.size_hold(),
            Some(model::SizeHold::Grown(300 * MIB))
        );
        assert!(
            !app.refresh_fetches.contains(&grown),
            "fetched the grown remote"
        );
        assert!(!app.fetches_in_flight.contains_key(&grown));
        assert_eq!(app.repos[1].fetch.size_hold(), None);
        assert!(
            app.refresh_fetches.contains(&quiet),
            "the small push was not fetched"
        );
    }

    /// FZ-35b (#85), criterion 4: a successful fetch records GitHub's current
    /// size as the baseline and lifts the hold.
    #[test]
    fn a_successful_fetch_records_the_baseline_and_lifts_the_hold() {
        let mut app = fleet_app(1);
        app.dataset = vec![github::GithubRepo {
            disk_usage: Some(308_200),
            ..gh("repo-0", "2026-10-20T11:00:00Z")
        }];
        hold_sizes(&mut app, 0, 0);
        assert!(app.repos[0].fetch.size_hold().is_some());
        land(&mut app, 0, git::FetchOutcome::Succeeded);
        assert_eq!(app.repos[0].fetch.size_hold(), None);
        assert_eq!(app.repos[0].fetch.disk_baseline, Some(308_200));
        assess_size_holds(&mut app.repos, &app.dataset, app.fetch_max_bytes);
        assert_eq!(
            app.repos[0].fetch.size_hold(),
            None,
            "held again at its own size"
        );
    }

    /// FZ-34 (#83): a timeout from a fetch a load ran (launch, `R`) holds the
    /// row like a sweep's.
    #[test]
    fn a_load_that_timed_out_holds_the_row() {
        let mut app = fleet_app(1);
        let path = app.repos[0].path.clone().unwrap();
        let now = chrono::Local::now();
        let mut repo = app.repos[0].clone();
        repo.fetch
            .record(&fetch_failed(git::FetchFailureKind::TimedOut), now);
        apply_load_result(
            &mut app,
            &config::Config::default(),
            Ok(Box::new(LoadResult {
                kind: LoadKind::Reload,
                repos: vec![repo],
                header: "h".to_string(),
                dataset: Some(Vec::new()),
                github_calendar: None,
                source: app::DatasetSource::Local,
                ambiguity: None,
                meta: None,
            })),
        );
        assert!(
            app.fetch_held(&path, now),
            "the load's timeout held nothing"
        );
    }

    /// FZ-34 (#83): the poll's batch (moved remotes and the recovery
    /// refetch) leaves out a row held after a timeout.
    #[tokio::test]
    async fn a_recovering_poll_skips_a_row_held_after_a_timeout() {
        let path = std::env::temp_dir()
            .join("fleetz-83-no-such-dir")
            .join("repo")
            .display()
            .to_string();
        let mut repo = test_repo();
        repo.path = Some(path.clone());
        let mut app = App::new(vec![repo], "h".to_string(), 120, 44);
        let status = app.repos[0].clone();
        apply_repo_updated(
            &mut app,
            &path,
            status,
            fetch_failed(git::FetchFailureKind::TimedOut),
        );
        poll_with(
            &mut app,
            Err(github::FetchError {
                failure: github::Failure::new(github::FailureKind::Offline, "no route"),
                fallback: None,
                meta: Default::default(),
            }),
        );
        app.refresh_awaits_poll = true;
        poll_with(
            &mut app,
            Ok(Box::new(github::GithubFetch {
                repos: Vec::new(),
                calendar: None,
                has_pr_counts: true,
                origin: github::Origin::Network,
                meta: Default::default(),
            })),
        );
        assert!(
            app.refresh_fetches.is_empty(),
            "the recovery refetch fetched a held row"
        );
    }

    /// FZ-34 (#83): a manual `r` or `R` lifts every timeout hold.
    #[test]
    fn r_and_shift_r_lift_the_timeout_holds() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
        let budget = reading("A", 2, at(17, 23, 6), at(17, 0, 0));
        land_at(&mut app, ok_with(Some(budget)), at(17, 0, 0));
        let timed_out = fetch_failed(git::FetchFailureKind::TimedOut);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let args = Args::parse_from(["fleetz"]);

        app.fetch_backoff.record("~/a", &timed_out, at(17, 9, 0));
        assert!(app.fetch_held("~/a", at(17, 10, 0)));
        // Under a known limit `r` sends nothing, so no runtime is needed.
        refresh_key(&mut app, &tx, &args, &config, at(17, 10, 0));
        assert!(!app.fetch_held("~/a", at(17, 10, 0)), "r kept the hold");

        app.fetch_backoff.record("~/a", &timed_out, at(17, 9, 30));
        assert!(app.fetch_held("~/a", at(17, 10, 0)));
        // A load already running: `R` starts nothing new.
        app.loading = true;
        reload_key(
            &mut app,
            &tx,
            &args,
            &config,
            Path::new("/nonexistent"),
            at(17, 10, 0),
        );
        assert!(!app.fetch_held("~/a", at(17, 10, 0)), "R kept the hold");
    }

    #[test]
    fn a_commit_and_push_inside_one_sweep_is_still_detected() {
        // The regression: `git push` does not move HEAD, so watching only for
        // the unpushed count hitting zero misses the common case where you
        // commit and push between two sweeps. HEAD moving is what catches it.
        let mut before = test_repo();
        before.work_key = Some(key("aaa"));
        before.ahead = 0;

        let mut after = test_repo();
        after.work_key = Some(key("bbb"));
        after.ahead = 0;

        assert!(repo_moved(&before, &after));
    }

    #[test]
    fn pushing_previously_committed_work_is_detected() {
        // HEAD is unchanged here, so only the unpushed count can catch it.
        let mut before = test_repo();
        before.work_key = Some(key("aaa"));
        before.ahead = 3;

        let mut after = test_repo();
        after.work_key = Some(key("aaa"));
        after.ahead = 0;

        assert!(repo_moved(&before, &after));
    }

    #[test]
    fn an_unchanged_repo_does_not_trigger_a_poll() {
        let mut before = test_repo();
        before.work_key = Some(key("aaa"));
        let mut after = test_repo();
        after.work_key = Some(key("aaa"));
        assert!(!repo_moved(&before, &after));
    }

    #[test]
    fn an_unknown_fingerprint_reads_as_no_information() {
        // Treating a missing key as "changed" would poll on every single
        // sweep for any repo whose fingerprint could not be taken.
        let before = test_repo();
        let mut after = test_repo();
        after.work_key = Some(key("bbb"));
        assert!(!repo_moved(&before, &after));
        assert!(!repo_moved(&after, &test_repo()));
    }

    /// A clone whose remote GitHub last touched before our last `git fetch`
    /// is already in hand, and startup should not spend a network round trip
    /// re-confirming it. This is the whole startup cost: fetching every repo
    /// past the cutoff was around 0.8s of network each, so a wide time window
    /// put seconds of nothing in front of the fleet.
    fn fetch_fixture(tag: &str, names: &[&str], write_fetch_head: bool) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fleetz_stale_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for name in names {
            let git = dir.join(name).join(".git");
            std::fs::create_dir_all(&git).unwrap();
            if write_fetch_head {
                std::fs::write(git.join("FETCH_HEAD"), b"x").unwrap();
            }
        }
        dir
    }

    /// The regression: a clone matched through its remote lives in a folder
    /// named something else, so deduping pass 3 by name alone listed it a
    /// second time, as local-only, once it was dirty.
    #[test]
    fn a_clone_passes_1_and_2_claimed_is_not_listed_again_as_local_only() {
        let claimed = PathBuf::from("/tmp/x/DevOps_Portfolio");
        let candidates = unclaimed_local(
            vec![claimed.clone(), PathBuf::from("/tmp/x/scratch")],
            &HashSet::from([claimed.as_path()]),
            &HashSet::from(["portfolio-site".to_string()]),
        );
        let names: Vec<&str> = candidates.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(names, ["scratch"]);
    }

    fn found_targets(dir: &Path, names: &[&str]) -> Vec<(discovery::FoundRepo, RepoLocation)> {
        names
            .iter()
            .map(|n| {
                (
                    discovery::FoundRepo {
                        name: (*n).to_string(),
                        path: dir.join(n),
                        owner: None,
                    },
                    RepoLocation::Primary,
                )
            })
            .collect()
    }

    #[test]
    fn a_remote_pushed_before_our_last_fetch_is_not_fetched_again() {
        // FETCH_HEAD is written now, so a push dated 2001 is comfortably
        // older than it whatever the machine's clock says.
        let dir = fetch_fixture("skip", &["alpha"], true);
        let targets = found_targets(&dir, &["alpha"]);
        let active = vec![gh("alpha", "2001-01-01T00:00:00Z")];
        assert!(
            stale_remotes(&targets, &active).is_empty(),
            "fetched a remote we already had"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_remote_pushed_since_our_last_fetch_is_fetched() {
        let dir = fetch_fixture("stale", &["alpha"], true);
        let targets = found_targets(&dir, &["alpha"]);
        let active = vec![gh("alpha", "2099-01-01T00:00:00Z")];
        assert_eq!(stale_remotes(&targets, &active).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Both unknowns have to read as stale. A clone that has never fetched
    /// writes no `FETCH_HEAD`, and a repo with no GitHub match has no
    /// `pushed_at` to compare against -- skipping either would silently show
    /// the fleet as more up to date than it is.
    #[test]
    fn an_unknown_baseline_is_treated_as_stale() {
        let never = fetch_fixture("never", &["alpha"], false);
        let active = vec![gh("alpha", "2001-01-01T00:00:00Z")];
        assert_eq!(
            stale_remotes(&found_targets(&never, &["alpha"]), &active).len(),
            1,
            "a clone that never fetched was skipped"
        );
        let _ = std::fs::remove_dir_all(&never);

        let unmatched = fetch_fixture("unmatched", &["local-only"], true);
        assert_eq!(
            stale_remotes(&found_targets(&unmatched, &["local-only"]), &active).len(),
            1,
            "a repo with no GitHub match was skipped"
        );
        let _ = std::fs::remove_dir_all(&unmatched);
    }

    /// Rule 7: a failed `git fetch` leaves `FETCH_HEAD` empty with a fresh
    /// mtime (pinned by `git::a_failed_fetch_empties_fetch_head`). Reading
    /// that mtime as "fetched just now" skipped exactly the repo whose remote
    /// refs were known to be behind.
    #[test]
    fn an_empty_fetch_head_is_treated_as_never_fetched() {
        let dir = fetch_fixture("emptyhead", &["alpha"], false);
        std::fs::write(dir.join("alpha").join(".git").join("FETCH_HEAD"), b"").unwrap();
        let active = vec![gh("alpha", "2001-01-01T00:00:00Z")];
        assert_eq!(
            stale_remotes(&found_targets(&dir, &["alpha"]), &active).len(),
            1,
            "a clone whose last fetch failed was skipped as already in hand"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The case that motivated this: a fleet opened again an hour later
    /// fetches nothing at all, so startup no longer scales with the time
    /// window -- which only ever decided how many repos reached the loop.
    #[test]
    fn a_warm_fleet_costs_no_network_at_startup() {
        let names = ["alpha", "beta", "gamma", "delta"];
        let dir = fetch_fixture("warm", &names, true);
        let active: Vec<_> = names
            .iter()
            .map(|n| gh(n, "2001-01-01T00:00:00Z"))
            .collect();
        assert!(stale_remotes(&found_targets(&dir, &names), &active).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn gh(name: &str, pushed: &str) -> github::GithubRepo {
        github::GithubRepo {
            name: name.to_string(),
            pushed_at: chrono::DateTime::parse_from_rfc3339(pushed)
                .unwrap()
                .with_timezone(&chrono::Utc),
            pr_count: 0,
            prs: vec![],
            ci_status: model::CiStatus::Unknown,
            ci_workflow: None,
            ci_duration: None,
            repo_kind: model::RepoKind::Unknown,
            owner: None,
            disk_usage: None,
        }
    }

    fn cloned(name: &str) -> RepoStatus {
        RepoStatus {
            name: name.to_string(),
            path: Some(format!("~/repos/{name}")),
            ..test_repo()
        }
    }

    #[test]
    fn a_remote_that_moved_is_the_only_one_fetched() {
        // The whole point: one API call says which remotes have news, so the
        // fetch goes only there instead of to all of them.
        let previous = vec![
            gh("alpha", "2026-08-06T10:00:00Z"),
            gh("beta", "2026-08-06T10:00:00Z"),
        ];
        let current = vec![
            gh("alpha", "2026-08-06T11:00:00Z"),
            gh("beta", "2026-08-06T10:00:00Z"),
        ];
        let repos = vec![cloned("alpha"), cloned("beta")];

        assert_eq!(remotes_that_moved(&previous, &current, &repos), vec![0]);
    }

    #[test]
    fn an_unchanged_fleet_fetches_nothing() {
        let dataset = vec![gh("alpha", "2026-08-06T10:00:00Z")];
        let repos = vec![cloned("alpha")];
        assert!(remotes_that_moved(&dataset, &dataset, &repos).is_empty());
    }

    #[test]
    fn the_first_poll_of_a_session_fetches_nothing() {
        // No baseline to compare against. Treating unknown as moved would
        // fetch every remote at once, which is exactly what this avoids.
        let current = vec![gh("alpha", "2026-08-06T11:00:00Z")];
        let repos = vec![cloned("alpha")];
        assert!(remotes_that_moved(&[], &current, &repos).is_empty());
    }

    #[test]
    fn a_repo_with_no_local_clone_is_skipped() {
        // Nothing to fetch into.
        let previous = vec![gh("alpha", "2026-08-06T10:00:00Z")];
        let current = vec![gh("alpha", "2026-08-06T11:00:00Z")];
        let repos = vec![test_repo()]; // name "r", and no path anyway
        assert!(remotes_that_moved(&previous, &current, &repos).is_empty());
    }

    #[test]
    fn a_remote_going_backwards_is_not_treated_as_news() {
        // Clock skew or a rewritten history should not trigger a fetch storm.
        let previous = vec![gh("alpha", "2026-08-06T11:00:00Z")];
        let current = vec![gh("alpha", "2026-08-06T10:00:00Z")];
        let repos = vec![cloned("alpha")];
        assert!(remotes_that_moved(&previous, &current, &repos).is_empty());
    }

    #[test]
    fn watch_flags_beat_the_config_both_ways() {
        // A default that silently fails to apply is invisible until someone
        // wonders why nothing ever refreshes.
        assert!(resolve_watch(false, false, true), "config on, no flags");
        assert!(!resolve_watch(false, false, false), "config off, no flags");
        assert!(resolve_watch(true, false, false), "--watch overrides");
        assert!(!resolve_watch(false, true, true), "--no-watch overrides");
    }

    fn app_with(ci: &[model::CiStatus]) -> App {
        let mut app = App::new(Vec::new(), String::new(), 100, 40);
        app.repos = ci
            .iter()
            .map(|status| RepoStatus {
                ci_status: status.clone(),
                ..test_repo()
            })
            .collect();
        app
    }

    #[test]
    fn poll_relaxes_when_nothing_is_pending() {
        let config = config::Config {
            github_poll_interval: 90,
            ..config::Config::default()
        };
        let app = app_with(&[model::CiStatus::Passed, model::CiStatus::Unknown]);
        assert_eq!(github_poll_secs(&app, &config), 90);
    }

    #[test]
    fn poll_tightens_while_a_run_is_in_flight() {
        // Exactly when someone is watching the screen for it to finish.
        let config = config::Config {
            github_poll_interval: 90,
            ..config::Config::default()
        };
        let app = app_with(&[model::CiStatus::Passed, model::CiStatus::Pending]);
        assert_eq!(github_poll_secs(&app, &config), GITHUB_POLL_MIN as i64);
    }

    #[test]
    fn a_configured_interval_below_the_floor_is_raised() {
        // The floor exists so a typo cannot turn the dashboard into a
        // rate-limit incident.
        let config = config::Config {
            github_poll_interval: 1,
            ..config::Config::default()
        };
        let app = app_with(&[model::CiStatus::Passed]);
        assert_eq!(github_poll_secs(&app, &config), GITHUB_POLL_MIN as i64);
    }

    #[test]
    fn resolve_cutoff_window_overrides_mode() {
        let config = config::Config::default();
        let w = time::TimeWindow {
            n: 2,
            unit: time::WindowUnit::Weeks,
        };
        let (cutoff, header) = resolve_cutoff(Some(w), &Mode::All, &config).unwrap();
        assert!(cutoff.is_some());
        assert_eq!(header, "since 2 weeks");
    }

    #[test]
    fn resolve_cutoff_all_mode_has_no_cutoff() {
        let config = config::Config::default();
        let (cutoff, header) = resolve_cutoff(None, &Mode::All, &config).unwrap();
        assert!(cutoff.is_none());
        assert_eq!(header, "all divergence");
    }

    #[test]
    fn resolve_cutoff_dirty_mode_is_error() {
        let config = config::Config::default();
        assert!(resolve_cutoff(None, &Mode::Dirty, &config).is_err());
    }

    fn ago(days: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now() - chrono::Duration::days(days)
    }

    /// The case that prompted this: `~/Repos/Rust`, `bonsai` and `AoC-2025`
    /// are dirty with untracked files from months back and sat in "local
    /// changes" permanently, because the section consulted no clock at all.
    #[test]
    fn a_local_repo_stale_past_the_window_drops_out() {
        assert!(!local_repo_is_current(0, Some(ago(118)), Some(ago(7))));
    }

    /// The failure `docs/ideas.md` warned about when it argued the section
    /// should ignore the window: narrowing must never hide a repo you are
    /// working in right now.
    #[test]
    fn a_repo_dirtied_today_survives_the_narrowest_window() {
        assert!(local_repo_is_current(0, Some(ago(0)), Some(ago(1))));
    }

    /// An unpushed commit exists on this machine and nowhere else, so it
    /// outranks the window however old it is.
    #[test]
    fn unpushed_commits_survive_any_window() {
        assert!(local_repo_is_current(2, Some(ago(158)), Some(ago(7))));
    }

    /// Widening to "all" clears the cutoff, which is the escape hatch for
    /// everything the window evicted.
    #[test]
    fn no_cutoff_keeps_every_local_repo() {
        assert!(local_repo_is_current(0, Some(ago(158)), None));
    }

    /// A repo git could date at all is the normal case; one it could not is
    /// a repo nothing has ever happened in, and it has no claim on a row.
    #[test]
    fn a_local_repo_with_no_timestamp_drops_under_a_window() {
        assert!(!local_repo_is_current(0, None, Some(ago(7))));
    }

    /// The `~/Repos/Rust` shape end to end: `git init`, untracked files, no
    /// commit ever, last touched months ago. The predicate is only half the
    /// fix -- this pins that pass 3 actually consults it, which is the wiring
    /// a unit test of the predicate alone would happily let rot.
    #[tokio::test]
    async fn pass_three_drops_a_repo_stale_past_the_window() {
        let dir = std::env::temp_dir().join(format!("fleetz_p3_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for args in [
            vec!["git", "init", "-q", "-b", "main"],
            vec!["git", "config", "user.email", "t@example.com"],
            vec!["git", "config", "user.name", "t"],
        ] {
            let ok = std::process::Command::new(args[0])
                .args(&args[1..])
                .current_dir(&dir)
                .status()
                .expect("spawn")
                .success();
            assert!(ok, "setup failed: {args:?}");
        }
        std::fs::write(dir.join("junk.txt"), "scratch\n").unwrap();
        // Backdate it the way an abandoned project is backdated: by not
        // having been touched.
        let ok = std::process::Command::new("touch")
            .args(["-t", "202604300000", "junk.txt"])
            .current_dir(&dir)
            .status()
            .expect("spawn")
            .success();
        assert!(ok, "touch failed");

        let stale = local_only_status(
            &dir,
            "junkrepo".to_string(),
            model::RepoKind::Unknown,
            Some(chrono::Utc::now() - chrono::Duration::days(7)),
        )
        .await;
        assert!(stale.is_none(), "months-old untracked work is not current");

        let unfiltered =
            local_only_status(&dir, "junkrepo".to_string(), model::RepoKind::Unknown, None).await;
        assert!(
            unfiltered.is_some(),
            "the same repo must still be there with no window"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// S8a of the repository identity design, through the assembly seam: a
    /// repo with no remote and a modified tracked file, under the scan root,
    /// with nothing in the dataset naming it, is one LocalOnly row named for
    /// its folder. Runs against a temp home and root, so the real `$HOME` is
    /// never walked; no dataset and `git_fetch: false`, so nothing reaches
    /// the network or the cache.
    #[tokio::test]
    async fn a_remoteless_dirty_repo_under_the_root_is_one_local_only_row() {
        let home = wt_scratch("seam_s8a");
        let root = home.join("R");
        let repo = root.join("scratch");
        std::fs::create_dir_all(&repo).unwrap();
        wt_sh(&repo, &["git", "init", "-q", "-b", "main"]);
        wt_sh(&repo, &["git", "config", "user.email", "t@example.com"]);
        wt_sh(&repo, &["git", "config", "user.name", "t"]);
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        wt_sh(&repo, &["git", "add", "."]);
        wt_sh(&repo, &["git", "commit", "-q", "-m", "init"]);
        std::fs::write(repo.join("a.txt"), "two\n").unwrap();

        let args = Args::parse_from(["fleetz"]);
        let config = config::Config::default();
        let (repos, _) = assemble_view_in(
            &[],
            None,
            String::new(),
            &args,
            &config,
            &home,
            std::slice::from_ref(&root),
            false,
            None,
        )
        .await
        .expect("assembly");

        assert_eq!(repos.len(), 1, "exactly one row: {repos:?}");
        assert_eq!(repos[0].name, "scratch");
        assert_eq!(repos[0].location, RepoLocation::LocalOnly);
        assert_eq!(repos[0].dirty, 1);

        std::fs::remove_dir_all(&home).ok();
    }

    // --- FZ-05d: GitHub fields live on one carrier row per remote ---
    //
    // Scenarios of section 4 of `docs/repository-identity-design.md`. The
    // load half runs through `assemble_view_in` with a temp home and root, so
    // the real `$HOME` is never walked; `git_fetch: false` and a dataset
    // handed in directly keep the network, the cache and `gh` out of it.

    /// A repo at `dir` with one commit, `origin` set to `url` when given, and
    /// a modified tracked file when `dirty`.
    fn id_repo(dir: &Path, url: Option<&str>, dirty: bool) {
        std::fs::create_dir_all(dir).unwrap();
        wt_sh(dir, &["git", "init", "-q", "-b", "main"]);
        wt_sh(dir, &["git", "config", "user.email", "t@example.com"]);
        wt_sh(dir, &["git", "config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        wt_sh(dir, &["git", "add", "."]);
        wt_sh(dir, &["git", "commit", "-q", "-m", "init"]);
        if let Some(url) = url {
            wt_sh(dir, &["git", "remote", "add", "origin", url]);
        }
        if dirty {
            std::fs::write(dir.join("a.txt"), "two\n").unwrap();
        }
    }

    /// A dataset entry pushed `days_ago`, with `prs` open pull requests.
    fn id_entry(
        owner: Option<&str>,
        name: &str,
        days_ago: i64,
        ci: model::CiStatus,
        prs: u32,
        kind: model::RepoKind,
    ) -> github::GithubRepo {
        let pr = |number: u32| model::PullRequest {
            number,
            title: "Change".to_string(),
            author: "someone".to_string(),
            created_at: chrono::Utc::now(),
            review_status: model::ReviewStatus::Approved,
            url: format!("https://github.com/x/{name}/pull/{number}"),
        };
        github::GithubRepo {
            name: name.to_string(),
            pushed_at: chrono::Utc::now() - chrono::Duration::days(days_ago),
            pr_count: prs,
            prs: (1..=prs).map(pr).collect(),
            ci_status: ci,
            ci_workflow: None,
            ci_duration: None,
            repo_kind: kind,
            owner: owner.map(str::to_string),
            disk_usage: None,
        }
    }

    /// The 1-day window the scenarios name, as a cutoff.
    fn one_day() -> Option<chrono::DateTime<chrono::Utc>> {
        Some(chrono::Utc::now() - chrono::Duration::days(1))
    }

    async fn id_assemble(
        home: &Path,
        roots: &[PathBuf],
        dataset: &[github::GithubRepo],
        cutoff: Option<chrono::DateTime<chrono::Utc>>,
    ) -> (Vec<RepoStatus>, ViewNotes) {
        let args = Args::parse_from(["fleetz"]);
        let config = config::Config::default();
        assemble_view_in(
            dataset,
            cutoff,
            String::new(),
            &args,
            &config,
            home,
            roots,
            false,
            None,
        )
        .await
        .expect("assembly")
    }

    fn row_at<'a>(repos: &'a [RepoStatus], path: &Path) -> &'a RepoStatus {
        let want = tilde_display(path);
        repos
            .iter()
            .find(|r| r.path.as_deref() == Some(want.as_str()))
            .unwrap_or_else(|| panic!("no row at {want}: {repos:#?}"))
    }

    fn not_cloned<'a>(repos: &'a [RepoStatus], owner: &str, name: &str) -> Option<&'a RepoStatus> {
        repos.iter().find(|r| {
            r.location == RepoLocation::NotCloned
                && r.name == name
                && r.owner.as_deref() == Some(owner)
        })
    }

    /// A row with no GitHub fields: what every non-carrier LocalOnly row is.
    fn assert_carries_nothing(repo: &RepoStatus) {
        assert_eq!(repo.owner, None, "{} was given an owner", repo.name);
        assert_eq!(repo.ci_status, model::CiStatus::Unknown, "{}", repo.name);
        assert_eq!(repo.pr_count, 0, "{}", repo.name);
        assert!(repo.prs.is_empty(), "{}", repo.name);
        assert_eq!(repo.repo_kind, model::RepoKind::Unknown, "{}", repo.name);
    }

    /// Land a successful poll carrying `repos` on `app`.
    fn poll_entries(app: &mut App, repos: Vec<github::GithubRepo>) {
        poll_with(
            app,
            Ok(Box::new(github::GithubFetch {
                repos,
                calendar: None,
                has_pr_counts: true,
                origin: github::Origin::Network,
                meta: Default::default(),
            })),
        );
    }

    /// S1, load and poll. Two clones of two same-named repos of different
    /// owners are two Primary rows, each with its own owner and CI, and a
    /// poll keeps each on its own repo. The name dedupe kept one row, and
    /// both rows would have taken the first `api` in the dataset. A push to
    /// one of them fetches that clone only.
    #[tokio::test]
    async fn two_same_named_repos_of_two_owners_each_keep_their_own_ci() {
        let home = wt_scratch("id_s1");
        let root = home.join("R");
        id_repo(
            &root.join("api"),
            Some("git@github.com:company/api.git"),
            false,
        );
        id_repo(
            &root.join("alice-api"),
            Some("https://github.com/alice/api"),
            false,
        );
        let alice = id_entry(
            Some("alice"),
            "api",
            0,
            model::CiStatus::Passed,
            0,
            model::RepoKind::Public,
        );
        let company = id_entry(
            Some("company"),
            "api",
            0,
            model::CiStatus::Failed,
            0,
            model::RepoKind::Org,
        );
        let dataset = vec![alice.clone(), company.clone()];

        let (repos, _) = id_assemble(&home, std::slice::from_ref(&root), &dataset, None).await;

        assert_eq!(repos.len(), 2, "{repos:#?}");
        let alices = row_at(&repos, &root.join("alice-api"));
        assert_eq!(alices.location, RepoLocation::Primary);
        assert_eq!(alices.owner.as_deref(), Some("alice"));
        assert_eq!(alices.ci_status, model::CiStatus::Passed);
        let companys = row_at(&repos, &root.join("api"));
        assert_eq!(companys.location, RepoLocation::Primary);
        assert_eq!(companys.owner.as_deref(), Some("company"));
        assert_eq!(companys.ci_status, model::CiStatus::Failed);

        let mut app = App::new(repos, "h".to_string(), 120, 44);
        let mut alice_now = alice.clone();
        alice_now.ci_status = model::CiStatus::Failed;
        let mut company_now = company.clone();
        company_now.ci_status = model::CiStatus::Passed;
        company_now.pushed_at = company.pushed_at + chrono::Duration::hours(1);
        poll_entries(&mut app, vec![alice_now.clone(), company_now.clone()]);

        let alices = row_at(&app.repos, &root.join("alice-api"));
        assert_eq!(alices.ci_status, model::CiStatus::Failed);
        let companys = row_at(&app.repos, &root.join("api"));
        assert_eq!(companys.ci_status, model::CiStatus::Passed);

        let company_index = app
            .repos
            .iter()
            .position(|r| r.owner.as_deref() == Some("company"))
            .unwrap();
        assert_eq!(
            remotes_that_moved(&dataset, &[alice_now, company_now], &app.repos),
            vec![company_index],
            "the push to company/api fetched another clone"
        );

        std::fs::remove_dir_all(&home).ok();
    }

    /// S16, load and poll. Two same-named repos of different owners, neither
    /// cloned, are two NotCloned rows, and a poll gives each its own CI. By
    /// name, both took the first entry's.
    #[tokio::test]
    async fn two_not_cloned_repos_of_one_name_each_get_their_own_ci() {
        let home = wt_scratch("id_s16");
        let root = home.join("R");
        std::fs::create_dir_all(&root).unwrap();
        let alice = id_entry(
            Some("alice"),
            "api",
            0,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Public,
        );
        let company = id_entry(
            Some("company"),
            "api",
            0,
            model::CiStatus::Failed,
            0,
            model::RepoKind::Org,
        );

        let (repos, _) = id_assemble(
            &home,
            std::slice::from_ref(&root),
            &[alice.clone(), company.clone()],
            None,
        )
        .await;

        assert_eq!(repos.len(), 2, "{repos:#?}");
        let row = not_cloned(&repos, "alice", "api").expect("alice/api not cloned");
        assert_eq!(row.ci_status, model::CiStatus::Passed);
        assert_eq!(row.pr_count, 1);
        let row = not_cloned(&repos, "company", "api").expect("company/api not cloned");
        assert_eq!(row.ci_status, model::CiStatus::Failed);
        assert_eq!(row.pr_count, 0);

        let mut app = App::new(repos, "h".to_string(), 120, 44);
        let mut alice_now = alice;
        alice_now.ci_status = model::CiStatus::Failed;
        alice_now.pr_count = 0;
        alice_now.prs.clear();
        let mut company_now = company;
        company_now.ci_status = model::CiStatus::Passed;
        poll_entries(&mut app, vec![alice_now, company_now]);

        let row = not_cloned(&app.repos, "alice", "api").unwrap();
        assert_eq!(row.ci_status, model::CiStatus::Failed);
        assert_eq!(row.pr_count, 0);
        let row = not_cloned(&app.repos, "company", "api").unwrap();
        assert_eq!(row.ci_status, model::CiStatus::Passed);

        std::fs::remove_dir_all(&home).ok();
    }

    /// The FZ-05c seam carried forward: pass 2 sweeps the `home` it is given.
    /// A clone outside the scan roots but under that home is an Elsewhere
    /// row, the carrier of its repo at load and in the poll.
    #[tokio::test]
    async fn pass_two_finds_a_carrier_under_the_given_home() {
        let home = wt_scratch("id_elsewhere");
        let root = home.join("R");
        std::fs::create_dir_all(&root).unwrap();
        let clone = home.join("src").join("tool");
        id_repo(&clone, Some("git@github.com:Gaurgle/tool.git"), false);
        let entry = id_entry(
            Some("Gaurgle"),
            "tool",
            0,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Private,
        );

        let (repos, _) = id_assemble(
            &home,
            std::slice::from_ref(&root),
            std::slice::from_ref(&entry),
            None,
        )
        .await;

        assert_eq!(repos.len(), 1, "{repos:#?}");
        let row = row_at(&repos, &clone);
        assert_eq!(row.location, RepoLocation::Elsewhere);
        assert_eq!(row.owner.as_deref(), Some("Gaurgle"));
        assert_eq!(row.ci_status, model::CiStatus::Passed);
        assert_eq!(row.pr_count, 1);

        let mut app = App::new(repos, "h".to_string(), 120, 44);
        let mut now = entry;
        now.ci_status = model::CiStatus::Failed;
        poll_entries(&mut app, vec![now]);
        assert_eq!(app.repos[0].ci_status, model::CiStatus::Failed);

        std::fs::remove_dir_all(&home).ok();
    }

    /// S3: a remote-less clone whose folder name names two owners' repos
    /// claims neither. Both stay not cloned, the clone is a LocalOnly row
    /// with no GitHub fields and no glyph, and the load says why. Before,
    /// its pass 3 row took a glyph by name and hid both NotCloned rows.
    #[tokio::test]
    async fn an_ambiguous_remoteless_clone_carries_nothing_and_says_why() {
        let home = wt_scratch("id_s3");
        let root = home.join("R");
        let clone = root.join("api");
        id_repo(&clone, None, true);
        let dataset = vec![
            id_entry(
                Some("alice"),
                "api",
                0,
                model::CiStatus::Passed,
                1,
                model::RepoKind::Public,
            ),
            id_entry(
                Some("company"),
                "api",
                0,
                model::CiStatus::Failed,
                0,
                model::RepoKind::Org,
            ),
        ];

        let (repos, notes) = id_assemble(&home, std::slice::from_ref(&root), &dataset, None).await;

        assert_eq!(repos.len(), 3, "{repos:#?}");
        assert!(not_cloned(&repos, "alice", "api").is_some());
        assert!(not_cloned(&repos, "company", "api").is_some());
        let local = row_at(&repos, &clone);
        assert_eq!(local.location, RepoLocation::LocalOnly);
        assert_carries_nothing(local);
        assert_eq!(
            notes.ambiguity.as_deref(),
            Some(
                format!(
                    "api: {} matches 2 GitHub repos, add an origin remote",
                    tilde_display(&clone)
                )
                .as_str()
            )
        );

        std::fs::remove_dir_all(&home).ok();
    }

    /// A load landing with `ambiguity` as its kind's result would.
    fn land_load(
        app: &mut App,
        kind: LoadKind,
        ambiguity: Option<&str>,
        store: &window_state::WindowState,
    ) {
        apply_load_result_saving_to(
            app,
            &config::Config::default(),
            Ok(Box::new(LoadResult {
                kind,
                repos: Vec::new(),
                header: "last 2 days".to_string(),
                dataset: Some(Vec::new()),
                github_calendar: None,
                source: app::DatasetSource::Network,
                ambiguity: ambiguity.map(str::to_string),
                meta: None,
            })),
            store,
        );
    }

    /// S3's status line (N2). Set after the `Reloaded` and `Window: ...`
    /// messages, so every kind of load leaves it on screen; with no
    /// ambiguity each kind says what it said before.
    #[test]
    fn the_ambiguity_line_wins_over_every_loads_own_message() {
        let scratch = wt_scratch("id_status");
        let store = window_state::WindowState::at(scratch.join("window.toml"));
        let line = "api: ~/repos/api matches 2 GitHub repos, add an origin remote";
        let window = || time::TimeWindow::clamped(2, time::WindowUnit::Days);
        let status = |app: &App| app.status_message.as_ref().map(|m| m.text.clone());

        let kinds = [
            (LoadKind::Initial, None),
            (LoadKind::Reload, Some("Reloaded".to_string())),
            (
                LoadKind::WindowApply(window()),
                Some("Window: last 2 days".to_string()),
            ),
        ];
        for (kind, today) in kinds {
            let label = format!("{today:?}");
            let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
            let again = match &kind {
                LoadKind::Initial => LoadKind::Initial,
                LoadKind::Reload => LoadKind::Reload,
                LoadKind::WindowApply(_) => LoadKind::WindowApply(window()),
                LoadKind::FromMemory => LoadKind::FromMemory,
            };
            land_load(&mut app, kind, Some(line), &store);
            assert_eq!(status(&app).as_deref(), Some(line), "after {label}");

            let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
            land_load(&mut app, again, None, &store);
            assert_eq!(status(&app), today, "{label} without an ambiguity");
        }

        std::fs::remove_dir_all(&scratch).ok();
    }

    /// S5 load and poll twin (section 5, FZ-05d). Two clones of one remote:
    /// the Primary is the carrier, and the dirty copy is a LocalOnly row
    /// with local fields only, at load and after a poll. A push to the remote
    /// fetches the carrier only.
    #[tokio::test]
    async fn an_extra_checkout_carries_nothing_at_load_or_in_the_poll() {
        let home = wt_scratch("id_s5");
        let root = home.join("R");
        let url = "git@github.com:Gaurgle/fleetz.git";
        id_repo(&root.join("fleetz"), Some(url), false);
        id_repo(&root.join("fleetz-copy"), Some(url), true);
        let entry = id_entry(
            Some("Gaurgle"),
            "fleetz",
            0,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Private,
        );

        let (repos, _) = id_assemble(
            &home,
            std::slice::from_ref(&root),
            std::slice::from_ref(&entry),
            None,
        )
        .await;

        assert_eq!(repos.len(), 2, "{repos:#?}");
        let primary = row_at(&repos, &root.join("fleetz"));
        assert_eq!(primary.location, RepoLocation::Primary);
        assert_eq!(primary.owner.as_deref(), Some("Gaurgle"));
        assert_eq!(primary.ci_status, model::CiStatus::Passed);
        assert_eq!(primary.pr_count, 1);
        let copy = row_at(&repos, &root.join("fleetz-copy"));
        assert_eq!(copy.location, RepoLocation::LocalOnly);
        assert_carries_nothing(copy);

        let mut app = App::new(repos, "h".to_string(), 120, 44);
        let mut now = id_entry(
            Some("Gaurgle"),
            "fleetz",
            0,
            model::CiStatus::Failed,
            2,
            model::RepoKind::Private,
        );
        poll_entries(&mut app, vec![now.clone()]);
        let primary = row_at(&app.repos, &root.join("fleetz"));
        assert_eq!(primary.ci_status, model::CiStatus::Failed);
        assert_eq!(primary.pr_count, 2);
        assert_carries_nothing(row_at(&app.repos, &root.join("fleetz-copy")));

        let primary_index = app
            .repos
            .iter()
            .position(|r| r.location == RepoLocation::Primary)
            .unwrap();
        let before = app.dataset.clone();
        now.pushed_at += chrono::Duration::hours(1);
        assert_eq!(
            remotes_that_moved(&before, &[now], &app.repos),
            vec![primary_index]
        );

        std::fs::remove_dir_all(&home).ok();
    }

    /// S4's poll twin. The dirty copy at `old/fleetz` has the remote's own
    /// name, so the name match wrote the poll onto it as well. Its load half
    /// waits for FZ-05f: pass 3 hides it by name until then.
    #[test]
    fn an_extra_checkout_named_like_its_remote_takes_nothing_from_the_poll() {
        let mut primary = cloned("fleetz");
        primary.owner = Some("Gaurgle".to_string());
        primary.remote_url = Some("git@github.com:Gaurgle/fleetz.git".to_string());
        let mut copy = cloned("fleetz");
        copy.path = Some("~/repos/old/fleetz".to_string());
        copy.location = RepoLocation::LocalOnly;
        copy.remote_url = primary.remote_url.clone();
        let mut app = App::new(vec![primary, copy], "h".to_string(), 120, 44);

        let mut now = id_entry(
            Some("Gaurgle"),
            "fleetz",
            0,
            model::CiStatus::Failed,
            2,
            model::RepoKind::Private,
        );
        poll_entries(&mut app, vec![now.clone()]);

        assert_eq!(app.repos[0].ci_status, model::CiStatus::Failed);
        assert_eq!(app.repos[0].pr_count, 2);
        assert_carries_nothing(&app.repos[1]);
        let before = app.dataset.clone();
        now.pushed_at += chrono::Duration::hours(1);
        assert_eq!(remotes_that_moved(&before, &[now], &app.repos), vec![0]);
    }

    /// S14, load: a remote-less clone named like a repo outside the window
    /// takes nothing from it, not even its glyph.
    #[tokio::test]
    async fn a_remoteless_clone_takes_no_glyph_from_a_same_named_repo() {
        let home = wt_scratch("id_s14");
        let root = home.join("R");
        let clone = root.join("api");
        id_repo(&clone, None, true);
        let entry = id_entry(
            Some("company"),
            "api",
            60,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Org,
        );

        let (repos, _) = id_assemble(
            &home,
            std::slice::from_ref(&root),
            std::slice::from_ref(&entry),
            one_day(),
        )
        .await;

        assert_eq!(repos.len(), 1, "{repos:#?}");
        let local = row_at(&repos, &clone);
        assert_eq!(local.location, RepoLocation::LocalOnly);
        assert_carries_nothing(local);

        std::fs::remove_dir_all(&home).ok();
    }

    /// S14, poll (section 5, FZ-05d): a LocalOnly row with no owner and no
    /// origin is never written by name, whether the entry has an owner or
    /// is a legacy one without. This is F4: a stranger's CI badge on a local
    /// repo on the first tick.
    #[test]
    fn a_remoteless_local_row_takes_nothing_from_the_poll() {
        for owner in [Some("company"), None] {
            let mut local = cloned("api");
            local.location = RepoLocation::LocalOnly;
            let mut app = App::new(vec![local], "h".to_string(), 120, 44);
            let entry = id_entry(
                owner,
                "api",
                0,
                model::CiStatus::Passed,
                1,
                model::RepoKind::Org,
            );
            poll_entries(&mut app, vec![entry]);
            let row = &app.repos[0];
            assert_eq!(row.ci_status, model::CiStatus::Unknown, "owner {owner:?}");
            assert_eq!(row.pr_count, 0, "owner {owner:?}");
            assert_eq!(row.repo_kind, model::RepoKind::Unknown, "owner {owner:?}");
        }
    }

    /// S6: a dirty linked worktree beside its main clone. The main clone is
    /// the Primary carrier; the worktree's stored origin names the same
    /// remote, so it is a LocalOnly row with local fields only.
    #[tokio::test]
    async fn a_worktree_beside_its_main_clone_carries_nothing() {
        let home = wt_scratch("id_s6");
        let root = home.join("R");
        let main = root.join("site");
        id_repo(
            &main,
            Some("git@github.com:Gaurgle/portfolio-site.git"),
            false,
        );
        let worktree = root.join("site-feature");
        wt_sh(
            &main,
            &[
                "git",
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                &path_arg(&worktree),
            ],
        );
        std::fs::write(worktree.join("a.txt"), "two\n").unwrap();
        let entry = id_entry(
            Some("Gaurgle"),
            "portfolio-site",
            0,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Public,
        );

        let (repos, _) = id_assemble(
            &home,
            std::slice::from_ref(&root),
            std::slice::from_ref(&entry),
            None,
        )
        .await;

        assert_eq!(repos.len(), 2, "{repos:#?}");
        let primary = row_at(&repos, &main);
        assert_eq!(primary.location, RepoLocation::Primary);
        assert_eq!(primary.owner.as_deref(), Some("Gaurgle"));
        assert_eq!(primary.ci_status, model::CiStatus::Passed);
        let local = row_at(&repos, &worktree);
        assert_eq!(local.location, RepoLocation::LocalOnly);
        assert!(
            local.remote_url.is_some(),
            "the worktree's origin was not read"
        );
        assert_carries_nothing(local);

        std::fs::remove_dir_all(&home).ok();
    }

    /// S6b: only the worktree is under a scan root, and its main clone is
    /// outside every root and outside the home. It carries the remote
    /// through its stored origin, with GitHub fields from the first frame,
    /// and the repo is not also listed as not cloned.
    #[tokio::test]
    async fn a_lone_worktree_carries_its_remote_through_its_stored_origin() {
        let scratch = wt_scratch("id_s6b");
        let home = scratch.join("home");
        let root = home.join("R");
        std::fs::create_dir_all(&root).unwrap();
        let main = scratch.join("outside").join("site");
        id_repo(
            &main,
            Some("git@github.com:Gaurgle/portfolio-site.git"),
            false,
        );
        let worktree = root.join("site-feature");
        wt_sh(
            &main,
            &[
                "git",
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                &path_arg(&worktree),
            ],
        );
        std::fs::write(worktree.join("a.txt"), "two\n").unwrap();
        let entry = id_entry(
            Some("Gaurgle"),
            "portfolio-site",
            0,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Public,
        );

        let (repos, _) = id_assemble(
            &home,
            std::slice::from_ref(&root),
            std::slice::from_ref(&entry),
            None,
        )
        .await;

        assert_eq!(repos.len(), 1, "one row, no NotCloned: {repos:#?}");
        let row = row_at(&repos, &worktree);
        assert_eq!(row.location, RepoLocation::LocalOnly);
        assert_eq!(row.name, "site-feature");
        assert_eq!(row.owner.as_deref(), Some("Gaurgle"));
        assert_eq!(row.ci_status, model::CiStatus::Passed);
        assert_eq!(row.pr_count, 1);
        assert_eq!(row.repo_kind, model::RepoKind::Public);

        std::fs::remove_dir_all(&scratch).ok();
    }

    /// S9d: a clone whose origin names a known repo outside the window, in a
    /// folder named like another repo in it. The folder claims nothing; the
    /// clone carries the repo its origin names, at load and in the poll, and
    /// the repo its folder names stays not cloned. Before, the LocalOnly row
    /// took the folder repo's glyph and hid its NotCloned row.
    #[tokio::test]
    async fn a_clone_carries_the_known_repo_its_origin_names() {
        let home = wt_scratch("id_s9d");
        let root = home.join("R");
        let clone = root.join("tool");
        id_repo(&clone, Some("git@github.com:Gaurgle/other.git"), true);
        let tool = id_entry(
            Some("Gaurgle"),
            "tool",
            0,
            model::CiStatus::Failed,
            0,
            model::RepoKind::Public,
        );
        let other = id_entry(
            Some("Gaurgle"),
            "other",
            60,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Private,
        );

        let (repos, _) = id_assemble(
            &home,
            std::slice::from_ref(&root),
            &[tool.clone(), other.clone()],
            one_day(),
        )
        .await;

        assert_eq!(repos.len(), 2, "{repos:#?}");
        let folder_repo = not_cloned(&repos, "Gaurgle", "tool").expect("Gaurgle/tool");
        assert_eq!(folder_repo.ci_status, model::CiStatus::Failed);
        let local = row_at(&repos, &clone);
        assert_eq!(local.location, RepoLocation::LocalOnly);
        assert_eq!(local.name, "tool");
        assert_eq!(local.owner.as_deref(), Some("Gaurgle"));
        assert_eq!(local.ci_status, model::CiStatus::Passed);
        assert_eq!(local.pr_count, 1);
        assert_eq!(local.repo_kind, model::RepoKind::Private);

        // The poll keys the carrier by the repo its origin names, not by its
        // folder's name, which is the other repo's.
        let mut app = App::new(repos, "h".to_string(), 120, 44);
        let mut tool_now = tool;
        tool_now.ci_status = model::CiStatus::Pending;
        let mut other_now = other;
        other_now.ci_status = model::CiStatus::Failed;
        poll_entries(&mut app, vec![tool_now, other_now]);
        let local = row_at(&app.repos, &clone);
        assert_eq!(local.ci_status, model::CiStatus::Failed);
        let folder_repo = not_cloned(&app.repos, "Gaurgle", "tool").unwrap();
        assert_eq!(folder_repo.ci_status, model::CiStatus::Pending);

        std::fs::remove_dir_all(&home).ok();
    }

    /// S11a: a clone of a project on another host, in a folder named like a
    /// GitHub repo. The GitHub repo is not cloned and carries its CI; the
    /// clone is a LocalOnly row with no GitHub fields and no glyph. Before,
    /// its pass 3 row took the glyph by name and hid the NotCloned row.
    #[tokio::test]
    async fn a_foreign_host_clone_leaves_the_github_repo_not_cloned() {
        let home = wt_scratch("id_s11a");
        let root = home.join("R");
        let clone = root.join("api");
        id_repo(&clone, Some("git@gitlab.com:someone/api.git"), true);
        let entry = id_entry(
            Some("company"),
            "api",
            0,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Org,
        );

        let (repos, _) = id_assemble(
            &home,
            std::slice::from_ref(&root),
            std::slice::from_ref(&entry),
            None,
        )
        .await;

        assert_eq!(repos.len(), 2, "{repos:#?}");
        let carrier = not_cloned(&repos, "company", "api").expect("company/api");
        assert_eq!(carrier.ci_status, model::CiStatus::Passed);
        assert_eq!(carrier.pr_count, 1);
        let local = row_at(&repos, &clone);
        assert_eq!(local.location, RepoLocation::LocalOnly);
        assert_carries_nothing(local);

        std::fs::remove_dir_all(&home).ok();
    }

    /// S13b: `~/.dotfiles` as a scan root, with an origin, dirty, its repo
    /// pushed outside the window. No other row has the repo, so the clone
    /// carries it: glyph, CI and PRs at load, not first after a poll, and
    /// the poll keeps writing them.
    #[tokio::test]
    async fn an_out_of_window_dotfiles_clone_carries_its_repo_from_the_first_frame() {
        let home = wt_scratch("id_s13b");
        let root = home.join("R");
        std::fs::create_dir_all(&root).unwrap();
        let dotfiles = home.join(".dotfiles");
        id_repo(&dotfiles, Some("git@github.com:Gaurgle/dotfiles.git"), true);
        let entry = id_entry(
            Some("Gaurgle"),
            "dotfiles",
            60,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Public,
        );

        let (repos, _) = id_assemble(
            &home,
            &[root.clone(), dotfiles.clone()],
            std::slice::from_ref(&entry),
            one_day(),
        )
        .await;

        assert_eq!(repos.len(), 1, "{repos:#?}");
        let row = row_at(&repos, &dotfiles);
        assert_eq!(row.location, RepoLocation::LocalOnly);
        assert_eq!(row.name, "dotfiles");
        assert_eq!(row.owner.as_deref(), Some("Gaurgle"));
        assert_eq!(row.ci_status, model::CiStatus::Passed);
        assert_eq!(row.pr_count, 1);
        assert_eq!(row.repo_kind, model::RepoKind::Public);

        let mut app = App::new(repos, "h".to_string(), 120, 44);
        let mut now = entry;
        now.ci_status = model::CiStatus::Failed;
        poll_entries(&mut app, vec![now]);
        assert_eq!(app.repos[0].ci_status, model::CiStatus::Failed);

        std::fs::remove_dir_all(&home).ok();
    }

    /// S17: a dirty clone with an origin at depth 4 under the root, which
    /// pass 1 (depth 3) misses and pass 3 (depth 4) reaches. It is the one
    /// row for its repo and carries it from the first frame; no NotCloned
    /// row beside it.
    #[tokio::test]
    async fn a_depth_four_clone_carries_its_repo_at_load() {
        let home = wt_scratch("id_s17");
        let root = home.join("R");
        let deep = root.join("a").join("b").join("c").join("fleetz");
        id_repo(&deep, Some("git@github.com:Gaurgle/fleetz.git"), true);
        let entry = id_entry(
            Some("Gaurgle"),
            "fleetz",
            0,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Public,
        );

        let (repos, _) = id_assemble(
            &home,
            std::slice::from_ref(&root),
            std::slice::from_ref(&entry),
            None,
        )
        .await;

        assert_eq!(repos.len(), 1, "one row, no NotCloned: {repos:#?}");
        let row = row_at(&repos, &deep);
        assert_eq!(row.location, RepoLocation::LocalOnly);
        assert_eq!(row.owner.as_deref(), Some("Gaurgle"));
        assert_eq!(row.ci_status, model::CiStatus::Passed);
        assert_eq!(row.pr_count, 1);

        std::fs::remove_dir_all(&home).ok();
    }

    /// The first-poll baseline reads each clone's own repo: two same-named
    /// repos of two owners are pushed at different times. By name, both
    /// clones were judged by one of the two.
    #[test]
    fn the_startup_fetch_judges_each_clone_by_its_own_repo() {
        let dir = fetch_fixture("owners", &["alice-api", "company-api"], true);
        let target = |folder: &str, owner: &str| {
            (
                discovery::FoundRepo {
                    name: "api".to_string(),
                    path: dir.join(folder),
                    owner: Some(owner.to_string()),
                },
                RepoLocation::Primary,
            )
        };
        let targets = vec![
            target("alice-api", "alice"),
            target("company-api", "company"),
        ];
        let mut alice = gh("api", "2099-01-01T00:00:00Z");
        alice.owner = Some("alice".to_string());
        let mut company = gh("api", "2001-01-01T00:00:00Z");
        company.owner = Some("company".to_string());

        assert_eq!(
            stale_remotes(&targets, &[alice.clone(), company.clone()]),
            vec![dir.join("alice-api")]
        );
        assert_eq!(
            stale_remotes(&targets, &[company, alice]),
            vec![dir.join("alice-api")]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Push order is per repo: two same-named repos of two owners each sort
    /// by their own push. By name, both took one rank.
    #[test]
    fn same_named_repos_of_two_owners_sort_by_their_own_push() {
        let dir = Path::new("/tmp/x");
        let found = |name: &str, owner: Option<&str>| {
            (
                discovery::FoundRepo {
                    name: name.to_string(),
                    path: dir.join(format!("{}-{name}", owner.unwrap_or("none"))),
                    owner: owner.map(str::to_string),
                },
                RepoLocation::Primary,
            )
        };
        let mut targets = vec![
            found("api", Some("company")),
            found("mid", None),
            found("api", Some("alice")),
        ];
        let mut alice = gh("api", "2026-08-24T10:00:00Z");
        alice.owner = Some("alice".to_string());
        let mid = gh("mid", "2026-08-20T10:00:00Z");
        let mut company = gh("api", "2026-08-01T10:00:00Z");
        company.owner = Some("company".to_string());

        sort_by_activity(&mut targets, &[alice, mid, company]);
        let order: Vec<Option<&str>> = targets.iter().map(|(f, _)| f.owner.as_deref()).collect();
        assert_eq!(order, [Some("alice"), None, Some("company")]);
    }

    /// A push is judged against the same repo's previous push, not a
    /// same-named repo's. `company/api` moves from 09:00 to 10:00 while
    /// `alice/api` sits at 12:00: by name, 10:00 was checked against 12:00
    /// and company's clone was never fetched.
    #[test]
    fn a_push_is_judged_against_the_same_owners_previous_push() {
        let entry = |owner: &str, pushed: &str| {
            let mut repo = gh("api", pushed);
            repo.owner = Some(owner.to_string());
            repo
        };
        let mut alices = cloned("api");
        alices.owner = Some("alice".to_string());
        alices.path = Some("~/repos/alice-api".to_string());
        let mut companys = cloned("api");
        companys.owner = Some("company".to_string());
        let repos = vec![alices, companys];

        let previous = vec![
            entry("alice", "2026-08-06T12:00:00Z"),
            entry("company", "2026-08-06T09:00:00Z"),
        ];
        let current = vec![
            entry("alice", "2026-08-06T12:00:00Z"),
            entry("company", "2026-08-06T10:00:00Z"),
        ];
        assert_eq!(remotes_that_moved(&previous, &current, &repos), vec![1]);
    }

    /// A launch served from a payload without owners, then a first network
    /// poll that has them. The legacy previous entry is still the baseline:
    /// unmatched, the push would read as unknown, never be fetched, and the
    /// next poll would compare against the owned dataset and miss it too.
    #[test]
    fn a_push_since_a_legacy_payload_is_still_fetched() {
        let previous = vec![gh("api", "2026-08-06T09:00:00Z")];
        let mut current = gh("api", "2026-08-06T10:00:00Z");
        current.owner = Some("company".to_string());
        let repos = vec![cloned("api")];
        assert_eq!(remotes_that_moved(&previous, &[current], &repos), vec![0]);
    }

    /// A launch from a payload without owners, then a poll holding both
    /// `alice/api` and `company/api` (FZ-05e keeps both), each newer than the
    /// legacy `api`. Both match that baseline and both resolve to the one
    /// owner-less row, which came back twice and got two single sweeps.
    #[test]
    fn a_row_two_moved_entries_resolve_to_is_fetched_once() {
        let previous = vec![gh("api", "2026-08-06T09:00:00Z")];
        let owned = |owner: &str| {
            let mut repo = gh("api", "2026-08-06T10:00:00Z");
            repo.owner = Some(owner.to_string());
            repo
        };
        let current = vec![owned("alice"), owned("company")];
        let repos = vec![cloned("api")];
        assert_eq!(remotes_that_moved(&previous, &current, &repos), vec![0]);
    }

    /// The carrier among LocalOnly rows equal on every earlier step is
    /// decided by the path, whichever order the walk listed them in.
    #[test]
    fn the_local_carrier_does_not_depend_on_listing_order() {
        let root = PathBuf::from("/tmp/fleetz_no_such_root/R");
        let a = root.join("fleetz-a");
        let b = root.join("fleetz-b");
        let row = |name: &str| {
            let mut repo = cloned(name);
            repo.location = RepoLocation::LocalOnly;
            repo.remote_url = Some("git@github.com:Gaurgle/fleetz.git".to_string());
            repo
        };
        let entry = id_entry(
            Some("Gaurgle"),
            "fleetz",
            0,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Public,
        );
        let listings = [
            [(a.as_path(), "fleetz-a"), (b.as_path(), "fleetz-b")],
            [(b.as_path(), "fleetz-b"), (a.as_path(), "fleetz-a")],
        ];
        for listing in listings {
            let mut rows: Vec<(&Path, RepoStatus)> = listing
                .iter()
                .map(|(path, name)| (*path, row(name)))
                .collect();
            let mut carried = HashSet::new();
            mark_local_carriers(
                &mut rows,
                std::slice::from_ref(&entry),
                std::slice::from_ref(&root),
                &mut carried,
            );
            let carriers: Vec<&str> = rows
                .iter()
                .filter(|(_, r)| r.owner.is_some())
                .map(|(_, r)| r.name.as_str())
                .collect();
            assert_eq!(carriers, ["fleetz-a"], "listed as {listing:?}");
        }
    }

    /// Several LocalOnly clones naming one repo: the carrier is the first in
    /// discovery's primary-checkout order, not the first the filesystem
    /// lists. Pair one ties on every step but the path, so the path decides
    /// (`fleetz-a`). Pair two is decided by depth against the path order:
    /// `a/notes-deep` sorts first by path, but `z-notes` has fewer
    /// components and wins. The repos are pushed outside the window, so
    /// passes 1 and 2 never look for them and only pass 3 sees the clones.
    #[tokio::test]
    async fn the_carrier_among_local_clones_follows_the_primary_checkout_order() {
        let home = wt_scratch("id_order");
        let root = home.join("R");
        let fleetz = "git@github.com:Gaurgle/fleetz.git";
        let notes = "git@github.com:Gaurgle/notes.git";
        id_repo(&root.join("fleetz-b"), Some(fleetz), true);
        id_repo(&root.join("fleetz-a"), Some(fleetz), true);
        id_repo(&root.join("z-notes"), Some(notes), true);
        id_repo(&root.join("a").join("notes-deep"), Some(notes), true);
        let dataset = vec![
            id_entry(
                Some("Gaurgle"),
                "fleetz",
                60,
                model::CiStatus::Passed,
                1,
                model::RepoKind::Public,
            ),
            id_entry(
                Some("Gaurgle"),
                "notes",
                60,
                model::CiStatus::Failed,
                2,
                model::RepoKind::Private,
            ),
        ];

        let (repos, _) = id_assemble(&home, std::slice::from_ref(&root), &dataset, one_day()).await;

        assert_eq!(repos.len(), 4, "{repos:#?}");
        let carrier = row_at(&repos, &root.join("fleetz-a"));
        assert_eq!(carrier.owner.as_deref(), Some("Gaurgle"));
        assert_eq!(carrier.ci_status, model::CiStatus::Passed);
        assert_eq!(carrier.pr_count, 1);
        assert_carries_nothing(row_at(&repos, &root.join("fleetz-b")));

        let carrier = row_at(&repos, &root.join("z-notes"));
        assert_eq!(carrier.owner.as_deref(), Some("Gaurgle"));
        assert_eq!(carrier.ci_status, model::CiStatus::Failed);
        assert_eq!(carrier.pr_count, 2);
        assert_eq!(carrier.repo_kind, model::RepoKind::Private);
        assert_carries_nothing(row_at(&repos, &root.join("a").join("notes-deep")));

        std::fs::remove_dir_all(&home).ok();
    }

    /// A row with an owner is written only by an entry of that owner: a
    /// same-named entry without one is not its repo (3.5, D1).
    #[test]
    fn an_owned_row_takes_nothing_from_an_ownerless_entry() {
        let mut row = cloned("api");
        row.owner = Some("company".to_string());
        let mut app = App::new(vec![row], "h".to_string(), 120, 44);
        let entry = id_entry(
            None,
            "api",
            0,
            model::CiStatus::Failed,
            1,
            model::RepoKind::Org,
        );
        poll_entries(&mut app, vec![entry]);
        let row = &app.repos[0];
        assert_eq!(row.ci_status, model::CiStatus::Unknown);
        assert_eq!(row.pr_count, 0);
        assert_eq!(row.repo_kind, model::RepoKind::Unknown);
    }

    /// The mirror, which must keep working: a row without an owner, the
    /// legacy case (m2), is written by a same-named entry, owned or not.
    #[test]
    fn an_ownerless_row_is_still_written_by_name() {
        let mut app = App::new(vec![cloned("api")], "h".to_string(), 120, 44);
        let entry = id_entry(
            Some("company"),
            "api",
            0,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Org,
        );
        poll_entries(&mut app, vec![entry]);
        let row = &app.repos[0];
        assert_eq!(row.ci_status, model::CiStatus::Passed);
        assert_eq!(row.pr_count, 1);
        assert_eq!(row.repo_kind, model::RepoKind::Org);
    }

    /// A legacy payload has no owners, so no repo in it has a remote identity
    /// for a LocalOnly row's origin to name: the row carries nothing.
    #[tokio::test]
    async fn a_local_clone_carries_nothing_from_a_legacy_payload() {
        let home = wt_scratch("id_legacy");
        let root = home.join("R");
        let clone = root.join("fleetz-copy");
        id_repo(&clone, Some("git@github.com:Gaurgle/fleetz.git"), true);
        let entry = id_entry(
            None,
            "fleetz",
            60,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Public,
        );

        let (repos, _) = id_assemble(
            &home,
            std::slice::from_ref(&root),
            std::slice::from_ref(&entry),
            one_day(),
        )
        .await;

        assert_eq!(repos.len(), 1, "{repos:#?}");
        let row = row_at(&repos, &clone);
        assert_eq!(row.location, RepoLocation::LocalOnly);
        assert_carries_nothing(row);

        std::fs::remove_dir_all(&home).ok();
    }

    // --- FZ-05f: pass 3 dedupes by canonical path only ---

    /// A dirty remote-less repo at `dir`.
    fn dirty_remoteless(dir: &Path) {
        id_repo(dir, None, true);
    }

    /// S4, load and poll. Two clones of one remote with the same folder name:
    /// the Primary at `R/fleetz` carries the repo, and the dirty `R/old/fleetz`
    /// is a LocalOnly row with local fields only. A third checkout at
    /// `R/older/fleetz` with nothing uncommitted or unpushed (cloned, so its
    /// `origin/main` is its `main`) is not news and gets no row. Before, pass
    /// 3 hid the dirty copy because its folder name was already claimed.
    #[tokio::test]
    async fn a_dirty_extra_checkout_named_like_its_remote_is_a_local_row() {
        let home = wt_scratch("id_s4");
        let root = home.join("R");
        let url = "git@github.com:Gaurgle/fleetz.git";
        id_repo(&root.join("fleetz"), Some(url), false);
        id_repo(&root.join("old").join("fleetz"), Some(url), true);
        let older = root.join("older");
        std::fs::create_dir_all(&older).unwrap();
        wt_sh(
            &older,
            &[
                "git",
                "clone",
                "-q",
                &path_arg(&root.join("fleetz")),
                "fleetz",
            ],
        );
        wt_sh(
            &older.join("fleetz"),
            &["git", "remote", "set-url", "origin", url],
        );
        let entry = id_entry(
            Some("Gaurgle"),
            "fleetz",
            0,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Private,
        );

        let (repos, _) = id_assemble(
            &home,
            std::slice::from_ref(&root),
            std::slice::from_ref(&entry),
            None,
        )
        .await;

        assert_eq!(repos.len(), 2, "{repos:#?}");
        let primary = row_at(&repos, &root.join("fleetz"));
        assert_eq!(primary.location, RepoLocation::Primary);
        assert_eq!(primary.owner.as_deref(), Some("Gaurgle"));
        assert_eq!(primary.ci_status, model::CiStatus::Passed);
        assert_eq!(primary.pr_count, 1);
        let copy = row_at(&repos, &root.join("old").join("fleetz"));
        assert_eq!(copy.location, RepoLocation::LocalOnly);
        assert_eq!(copy.name, "fleetz");
        assert_eq!(copy.dirty, 1);
        assert_carries_nothing(copy);

        let mut app = App::new(repos, "h".to_string(), 120, 44);
        let now = id_entry(
            Some("Gaurgle"),
            "fleetz",
            0,
            model::CiStatus::Failed,
            2,
            model::RepoKind::Private,
        );
        poll_entries(&mut app, vec![now]);
        let primary = row_at(&app.repos, &root.join("fleetz"));
        assert_eq!(primary.ci_status, model::CiStatus::Failed);
        assert_eq!(primary.pr_count, 2);
        assert_carries_nothing(row_at(&app.repos, &root.join("old").join("fleetz")));

        std::fs::remove_dir_all(&home).ok();
    }

    /// S8b: two dirty remote-less repos both named `notes` are two LocalOnly
    /// rows, one per path. Before, the second was hidden by name.
    #[tokio::test]
    async fn two_same_named_remoteless_repos_are_two_local_rows() {
        let home = wt_scratch("id_s8b");
        let root = home.join("R");
        let a = root.join("a").join("notes");
        let b = root.join("b").join("notes");
        dirty_remoteless(&a);
        dirty_remoteless(&b);

        let (repos, _) = id_assemble(&home, std::slice::from_ref(&root), &[], None).await;

        assert_eq!(repos.len(), 2, "{repos:#?}");
        for path in [&a, &b] {
            let row = row_at(&repos, path);
            assert_eq!(row.location, RepoLocation::LocalOnly);
            assert_eq!(row.name, "notes");
            assert_carries_nothing(row);
        }

        std::fs::remove_dir_all(&home).ok();
    }

    /// Two owners of one name in the dataset, a Primary clone of one of them,
    /// and a dirty remote-less `api` elsewhere under the root. The clone
    /// carries `company/api`, `alice/api` stays not cloned, and the remote-less
    /// folder, ambiguous by name (rule 2 of 3.3), claims neither and is a
    /// LocalOnly row with no GitHub fields, named in the status line. Before,
    /// the Primary's name hid it.
    #[tokio::test]
    async fn a_remoteless_checkout_named_like_a_claimed_repo_is_a_local_row() {
        let home = wt_scratch("id_owners_p3");
        let root = home.join("R");
        let clone = root.join("api");
        id_repo(&clone, Some("git@github.com:company/api.git"), false);
        let scratch = root.join("x").join("api");
        dirty_remoteless(&scratch);
        let dataset = vec![
            id_entry(
                Some("alice"),
                "api",
                0,
                model::CiStatus::Passed,
                1,
                model::RepoKind::Public,
            ),
            id_entry(
                Some("company"),
                "api",
                0,
                model::CiStatus::Failed,
                0,
                model::RepoKind::Org,
            ),
        ];

        let (repos, notes) = id_assemble(&home, std::slice::from_ref(&root), &dataset, None).await;

        assert_eq!(repos.len(), 3, "{repos:#?}");
        let primary = row_at(&repos, &clone);
        assert_eq!(primary.location, RepoLocation::Primary);
        assert_eq!(primary.owner.as_deref(), Some("company"));
        assert_eq!(primary.ci_status, model::CiStatus::Failed);
        assert!(not_cloned(&repos, "alice", "api").is_some());
        let local = row_at(&repos, &scratch);
        assert_eq!(local.location, RepoLocation::LocalOnly);
        assert_eq!(local.name, "api");
        assert_carries_nothing(local);
        assert_eq!(
            notes.ambiguity.as_deref(),
            Some(
                format!(
                    "api: {} matches 2 GitHub repos, add an origin remote",
                    tilde_display(&scratch)
                )
                .as_str()
            )
        );

        std::fs::remove_dir_all(&home).ok();
    }

    /// One directory reached through two spellings of one root is one row,
    /// whether pass 1 claimed it (`fleetz`, dirty, with its remote) or only
    /// pass 3 lists it (`scratch`). Spellings: a symlink to the root, the
    /// root with `sub/..` appended, and the root canonicalized (`/private/var`
    /// against `/var` for a macOS temp dir). Comparing raw paths would list
    /// each repo once per spelling.
    #[cfg(unix)]
    #[tokio::test]
    async fn one_directory_through_two_root_spellings_is_one_row() {
        let home = wt_scratch("id_spellings");
        let root = home.join("R");
        id_repo(
            &root.join("fleetz"),
            Some("git@github.com:Gaurgle/fleetz.git"),
            true,
        );
        dirty_remoteless(&root.join("scratch"));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let link = home.join("L");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        let entry = id_entry(
            Some("Gaurgle"),
            "fleetz",
            0,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Public,
        );

        let spellings = [
            link.clone(),
            root.join("sub").join(".."),
            root.canonicalize().unwrap(),
        ];
        for other in spellings {
            let roots = [root.clone(), other.clone()];
            let (repos, _) = id_assemble(&home, &roots, std::slice::from_ref(&entry), None).await;
            assert_eq!(repos.len(), 2, "roots {roots:?}: {repos:#?}");
            assert_eq!(
                row_at(&repos, &root.join("fleetz")).location,
                RepoLocation::Primary,
                "roots {roots:?}"
            );
            assert_eq!(
                row_at(&repos, &root.join("scratch")).location,
                RepoLocation::LocalOnly,
                "roots {roots:?}"
            );
        }

        std::fs::remove_dir_all(&home).ok();
    }

    /// The dedupe key is canonical, the display path is not: a repo found
    /// under a symlinked root shows the path as the root was spelled.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_row_under_a_symlinked_root_displays_the_root_as_spelled() {
        let home = wt_scratch("id_symlink_display");
        let real = home.join("real");
        dirty_remoteless(&real.join("scratch"));
        let link = home.join("R");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let (repos, _) = id_assemble(&home, std::slice::from_ref(&link), &[], None).await;

        assert_eq!(repos.len(), 1, "{repos:#?}");
        assert_eq!(
            repos[0].path.as_deref(),
            Some(tilde_display(&link.join("scratch")).as_str())
        );
        assert_eq!(repos[0].name, "scratch");

        std::fs::remove_dir_all(&home).ok();
    }

    /// `c` switches the strip's source and ctrl-c quits. Match arms are
    /// ordered, so an unguarded `c` arm placed above the modifier check
    /// silently turns the quit key into a pane key. That regression happened
    /// once already.
    #[test]
    fn ctrl_c_is_matched_before_the_plain_c_arm() {
        let source = include_str!("main.rs");
        let ctrl_c = source
            .find("KeyCode::Char('c') if key.modifiers")
            .expect("ctrl-c arm present");
        let plain_c = source
            .find("KeyCode::Char('c') => {")
            .expect("plain c arm present");
        assert!(
            ctrl_c < plain_c,
            "the ctrl-c arm must be matched before the plain c arm"
        );
    }

    /// The local histogram is the most expensive probe in the sweep: a year
    /// of log per repo. It must not run for a strip showing GitHub's own
    /// numbers.
    #[test]
    fn the_daily_probe_is_skipped_unless_the_local_source_is_showing() {
        let mut app = App::new(vec![test_repo()], "h".to_string(), 120, 44);
        app.contributions = app::ContributionsMode::Github;
        assert!(
            contrib_probe(&app).is_none(),
            "github source needs no local log"
        );
        app.contributions = app::ContributionsMode::Local;
        let probe = contrib_probe(&app).expect("local source probes");
        assert_eq!(
            probe.start,
            contributions::grid_start(chrono::Local::now().date_naive()),
            "every repo in one sweep shares a grid"
        );
    }

    #[test]
    fn the_probe_carries_the_configured_author_override() {
        let mut app = App::new(vec![test_repo()], "h".to_string(), 120, 44);
        app.contributions = app::ContributionsMode::Local;
        app.contrib_author = Some(r"me@work.com\|me@home.com".to_string());
        let probe = contrib_probe(&app).unwrap();
        assert_eq!(probe.author.as_deref(), Some(r"me@work.com\|me@home.com"));
    }

    /// The startup fetch carries the calendar; dropping it here left the
    /// strip blank until the first background poll landed minutes later.
    #[test]
    fn a_load_result_stores_its_calendar_on_the_app() {
        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        assert!(app.github_calendar.is_none());
        let today = chrono::Local::now().date_naive();
        let mut cal = contributions::Calendar::new(today, contributions::Source::Github);
        cal.add(today, 5);
        apply_load_result(
            &mut app,
            &config::Config::default(),
            Ok(Box::new(LoadResult {
                kind: LoadKind::Initial,
                repos: Vec::new(),
                header: "h".to_string(),
                dataset: Some(Vec::new()),
                github_calendar: Some(Box::new(cal)),
                source: app::DatasetSource::Network,
                ambiguity: None,
                meta: None,
            })),
        );
        assert_eq!(
            app.github_calendar.as_ref().map(|c| c.total()),
            Some(5),
            "the startup fetch must fill the strip"
        );
    }

    /// A cache-served launch must leave the re-poll clock unset, so the first
    /// tick polls immediately.
    ///
    /// Startup passes `CacheMode::AnyCached` and takes any payload however
    /// old, so "the load returned a dataset" never meant "GitHub was just
    /// asked". Starting the clock from it put the first real poll a whole
    /// `github_poll_interval` away, and the dashboard spent it showing what
    /// the previous session had left on disk. Seen for real: a launch at
    /// 15:38:11 painted a stale contribution count and did not correct it
    /// until 15:38:50.
    #[test]
    fn a_cache_served_launch_leaves_the_poll_due_immediately() {
        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        apply_load_result(
            &mut app,
            &config::Config::default(),
            Ok(Box::new(LoadResult {
                kind: LoadKind::Initial,
                repos: Vec::new(),
                header: "h".to_string(),
                dataset: Some(Vec::new()),
                github_calendar: None,
                source: app::DatasetSource::Cache { written: None },
                ambiguity: None,
                meta: None,
            })),
        );
        assert!(
            app.github_freshness.last_attempt.is_none(),
            "a cached payload started the re-poll clock, delaying the first \
             real poll by a whole interval"
        );
    }

    /// The other half: a load that did reach GitHub has just polled, so the
    /// clock starts here rather than firing again a tick later.
    #[test]
    fn a_network_served_launch_starts_the_poll_clock() {
        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        apply_load_result(
            &mut app,
            &config::Config::default(),
            Ok(Box::new(LoadResult {
                kind: LoadKind::Initial,
                repos: Vec::new(),
                header: "h".to_string(),
                dataset: Some(Vec::new()),
                github_calendar: None,
                source: app::DatasetSource::Network,
                ambiguity: None,
                meta: None,
            })),
        );
        assert!(
            app.github_freshness.last_attempt.is_some(),
            "fresh data should not re-poll at once"
        );
    }

    /// The owner's viewer page (design 1.2): 50 of 95, cut off 237 days back.
    fn owners_coverage() -> github::Coverage {
        github::Coverage {
            viewer: github::PageCoverage {
                returned: 50,
                total: Some(95),
                has_next_page: Some(true),
                boundary: Some(chrono::Utc::now() - chrono::Duration::days(237)),
            },
            org_list: None,
            org_pages: Vec::new(),
        }
    }

    /// CV19: a window apply fetches nothing, so it carries no `FetchMeta`;
    /// the app keeps the load's, and the apply draws its note from it.
    #[test]
    fn cv19_a_window_apply_keeps_the_loads_coverage_and_draws_its_note() {
        let scratch = wt_scratch("cv19");
        let store = window_state::WindowState::at(scratch.join("window.toml"));
        let config = config::Config::default();
        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        let meta = github::FetchMeta {
            coverage: Some(owners_coverage()),
            ..Default::default()
        };
        apply_load_result_saving_to(
            &mut app,
            &config,
            Ok(Box::new(LoadResult {
                kind: LoadKind::Initial,
                repos: Vec::new(),
                header: "h".to_string(),
                dataset: Some(Vec::new()),
                github_calendar: None,
                source: app::DatasetSource::Network,
                ambiguity: None,
                meta: Some(meta.clone()),
            })),
            &store,
        );
        assert_eq!(app.github_meta, meta);

        // As `spawn_window_apply` builds it, from the app's coverage.
        let w = time::TimeWindow::clamped(1, time::WindowUnit::Years);
        let header = with_coverage_note(
            w.label(config.work_start, config.work_end),
            app.github_meta.coverage.as_ref(),
            Some(w.cutoff(config.work_start, config.work_end)),
            config.repo_limit,
        );
        let notes = ViewNotes {
            header,
            ambiguity: None,
        };
        let result = window_apply_result(w, Vec::new(), notes);
        assert!(result.meta.is_none(), "a window apply fetched nothing");
        apply_load_result_saving_to(&mut app, &config, Ok(Box::new(result)), &store);

        assert_eq!(app.github_meta, meta, "the load's coverage was dropped");
        assert!(
            app.header.ends_with("(fetched 50 of 95 repos)"),
            "no note at 365 days: {}",
            app.header
        );
        std::fs::remove_dir_all(&scratch).ok();
    }

    /// A successful poll replaces the dataset and its coverage with it; a
    /// failed one changes neither (contract rule 4).
    #[test]
    fn a_poll_replaces_the_coverage_and_a_failed_one_keeps_it() {
        let mut app = app_with_pending_repo();
        let coverage = owners_coverage();
        let mut fetch = poll_says_passed().unwrap();
        fetch.meta.coverage = Some(coverage.clone());
        poll(&mut app, Ok(fetch));
        assert_eq!(app.github_meta.coverage.as_ref(), Some(&coverage));

        poll(&mut app, Err("boom".to_string()));
        assert!(app.github_freshness.is_failing(), "the failure landed");
        assert_eq!(app.github_meta.coverage.as_ref(), Some(&coverage));
    }

    /// A time-window apply re-filters what is already loaded and carries no
    /// calendar. It must not blank the one already on screen.
    #[test]
    fn a_window_apply_does_not_blank_the_calendar() {
        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        let today = chrono::Local::now().date_naive();
        let mut cal = contributions::Calendar::new(today, contributions::Source::Github);
        cal.add(today, 9);
        app.github_calendar = Some(Box::new(cal));
        apply_load_result(
            &mut app,
            &config::Config::default(),
            Ok(Box::new(LoadResult {
                kind: LoadKind::Initial,
                repos: Vec::new(),
                header: "h".to_string(),
                dataset: None,
                github_calendar: None,
                source: app::DatasetSource::Network,
                ambiguity: None,
                meta: None,
            })),
        );
        assert_eq!(app.github_calendar.as_ref().map(|c| c.total()), Some(9));
    }

    /// repoz lists newest-pushed first (`sort_by(.pushedAt) | reverse`, then
    /// iterated in that order). fleetz drew the same section in `read_dir`
    /// order, because `find_in_primary` reduces the push-ordered names to a
    /// HashSet and the walker pushes whatever the filesystem hands back. On
    /// APFS that is hash order: arbitrary, and it reshuffles when any repo is
    /// added or removed.
    #[test]
    fn discovered_repos_are_ordered_by_push_recency() {
        let dir = Path::new("/tmp/x");
        // Deliberately not the order the walker found them in.
        let mut targets = found_targets(dir, &["oldest", "newest", "middle"]);
        let active = vec![
            gh("newest", "2026-08-24T10:00:00Z"),
            gh("middle", "2026-08-20T10:00:00Z"),
            gh("oldest", "2026-08-01T10:00:00Z"),
        ];
        sort_by_activity(&mut targets, &active);
        let names: Vec<&str> = targets.iter().map(|(f, _)| f.name.as_str()).collect();
        assert_eq!(names, ["newest", "middle", "oldest"]);
    }

    /// A repo the GitHub fetch never matched has no push date to sort by. It
    /// must fall to the end rather than to the front, and keep the order
    /// discovery found it in.
    #[test]
    fn repos_without_a_github_match_sort_last_in_discovery_order() {
        let dir = Path::new("/tmp/x");
        let mut targets = found_targets(dir, &["local-a", "known", "local-b"]);
        let active = vec![gh("known", "2026-08-24T10:00:00Z")];
        sort_by_activity(&mut targets, &active);
        let names: Vec<&str> = targets.iter().map(|(f, _)| f.name.as_str()).collect();
        assert_eq!(names, ["known", "local-a", "local-b"]);
    }

    /// The sort runs after the pass-1/pass-2 dedupe, so it must not disturb
    /// which location won. Discovery pass order is load-bearing: a local clone
    /// has to beat the not-cloned fallback.
    #[test]
    fn sorting_by_activity_does_not_change_where_a_repo_was_found() {
        let dir = Path::new("/tmp/x");
        let mut targets = vec![
            (
                discovery::FoundRepo {
                    name: "a".to_string(),
                    path: dir.join("a"),
                    owner: None,
                },
                RepoLocation::Elsewhere,
            ),
            (
                discovery::FoundRepo {
                    name: "b".to_string(),
                    path: dir.join("b"),
                    owner: None,
                },
                RepoLocation::Primary,
            ),
        ];
        let active = vec![
            gh("b", "2026-08-24T10:00:00Z"),
            gh("a", "2026-08-01T10:00:00Z"),
        ];
        sort_by_activity(&mut targets, &active);
        assert_eq!(targets[0].0.name, "b");
        assert_eq!(targets[0].1, RepoLocation::Primary);
        assert_eq!(
            targets[1].1,
            RepoLocation::Elsewhere,
            "location was rewritten"
        );
    }

    /// Case is not a sort key: GitHub reports `Glanze`, the clone on disk may
    /// be `glanze`, and both sides of the lookup lowercase for exactly that
    /// reason elsewhere in discovery.
    #[test]
    fn activity_order_survives_a_case_mismatch() {
        let dir = Path::new("/tmp/x");
        let mut targets = found_targets(dir, &["old", "glanze"]);
        let active = vec![
            gh("Glanze", "2026-08-24T10:00:00Z"),
            gh("old", "2026-08-01T10:00:00Z"),
        ];
        sort_by_activity(&mut targets, &active);
        let names: Vec<&str> = targets.iter().map(|(f, _)| f.name.as_str()).collect();
        assert_eq!(
            names,
            ["glanze", "old"],
            "case mismatch dropped it to the end"
        );
    }

    // --- FZ-06c: the worktree list is re-read on every sweep ---
    //
    // Each scenario sweeps a real repo with `rebuild_repo_status`, feeding
    // every sweep the previous one's result as its snapshot, as the app does.
    // Unless a test says otherwise, the row asserted on is the MAIN
    // checkout's: its `worktrees` lists the linked worktrees of its clone.

    fn wt_sh(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new(args[0])
            .args(&args[1..])
            .current_dir(dir)
            .status()
            .expect("spawn");
        assert!(status.success(), "command failed: {args:?}");
    }

    /// A scratch directory unique to this test and this test process.
    fn wt_scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fleetz_wt_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A repo with one commit on `main` and an idle branch `spare`, at
    /// `<scratch>/main`.
    fn wt_repo(scratch: &Path) -> PathBuf {
        let dir = scratch.join("main");
        std::fs::create_dir_all(&dir).unwrap();
        wt_sh(&dir, &["git", "init", "-q", "-b", "main"]);
        wt_sh(&dir, &["git", "config", "user.email", "t@example.com"]);
        wt_sh(&dir, &["git", "config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        wt_sh(&dir, &["git", "add", "."]);
        wt_sh(&dir, &["git", "commit", "-q", "-m", "init"]);
        wt_sh(&dir, &["git", "branch", "spare"]);
        dir
    }

    fn path_arg(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    /// One sweep of `path` against `old`, without the histogram probe.
    async fn sweep_once(path: &Path, old: &RepoStatus) -> RepoStatus {
        let target = SweepTarget {
            display_path: path_arg(path),
            fs_path: path.to_path_buf(),
            old: Box::new(old.clone()),
        };
        rebuild_repo_status(&target, None, None).await
    }

    /// The first sweep of a row, from a snapshot with no key.
    async fn first_sweep(path: &Path) -> RepoStatus {
        let mut row = test_repo();
        row.path = Some(path_arg(path));
        sweep_once(path, &row).await
    }

    /// The branches the row's linked worktrees have checked out, sorted.
    fn worktree_branches(row: &RepoStatus) -> Vec<String> {
        let mut names: Vec<String> = row.worktrees.iter().map(|w| w.branch.clone()).collect();
        names.sort();
        names
    }

    fn common_worktrees_dir(path: &Path) -> PathBuf {
        git::repo_dirs(path)
            .expect("a repo")
            .common_dir
            .join("worktrees")
    }

    /// E4a: `worktree add -b` must show up on the next sweep.
    #[tokio::test]
    async fn a_worktree_added_on_a_new_branch_is_listed_on_the_next_sweep() {
        let scratch = wt_scratch("e4a");
        let repo = wt_repo(&scratch);
        let before = first_sweep(&repo).await;
        assert!(before.worktrees.is_empty());

        let wt = path_arg(&scratch.join("lw"));
        wt_sh(&repo, &["git", "worktree", "add", "-q", "-b", "wt1", &wt]);
        let after = sweep_once(&repo, &before).await;

        assert_eq!(worktree_branches(&after), ["wt1"]);
    }

    /// E4b: `worktree add` of an existing branch creates no ref, so nothing
    /// in the key moves; the list must move anyway.
    #[tokio::test]
    async fn a_worktree_added_on_an_existing_branch_is_listed_on_the_next_sweep() {
        let scratch = wt_scratch("e4b");
        let repo = wt_repo(&scratch);
        let before = first_sweep(&repo).await;
        assert!(before.worktrees.is_empty());

        let wt = path_arg(&scratch.join("lw"));
        wt_sh(&repo, &["git", "worktree", "add", "-q", &wt, "spare"]);
        let after = sweep_once(&repo, &before).await;

        assert_eq!(worktree_branches(&after), ["spare"]);
    }

    /// E4c: `worktree remove` of one of two linked worktrees.
    #[tokio::test]
    async fn a_removed_worktree_drops_off_on_the_next_sweep() {
        let scratch = wt_scratch("e4c");
        let repo = wt_repo(&scratch);
        let (one, two) = (scratch.join("lw1"), scratch.join("lw2"));
        wt_sh(&repo, &["git", "branch", "other"]);
        wt_sh(
            &repo,
            &["git", "worktree", "add", "-q", &path_arg(&one), "spare"],
        );
        wt_sh(
            &repo,
            &["git", "worktree", "add", "-q", &path_arg(&two), "other"],
        );
        let before = first_sweep(&repo).await;
        assert_eq!(worktree_branches(&before), ["other", "spare"]);

        wt_sh(&repo, &["git", "worktree", "remove", &path_arg(&one)]);
        let after = sweep_once(&repo, &before).await;

        assert_eq!(worktree_branches(&after), ["other"]);
    }

    /// E4d: a linked worktree switches to a new branch.
    #[tokio::test]
    async fn a_linked_worktree_switching_to_a_new_branch_is_seen_on_the_next_sweep() {
        let scratch = wt_scratch("e4d");
        let repo = wt_repo(&scratch);
        let wt = scratch.join("lw");
        wt_sh(
            &repo,
            &["git", "worktree", "add", "-q", &path_arg(&wt), "spare"],
        );
        let before = first_sweep(&repo).await;
        assert_eq!(worktree_branches(&before), ["spare"]);

        wt_sh(&wt, &["git", "switch", "-q", "-c", "fresh"]);
        let after = sweep_once(&repo, &before).await;

        assert_eq!(worktree_branches(&after), ["fresh"]);
    }

    /// E4d's quieter form: switching a linked worktree to a branch that
    /// already exists moves no ref and touches nothing of the main
    /// checkout's, so the main row's key cannot see it.
    #[tokio::test]
    async fn a_linked_worktree_switching_to_an_existing_branch_is_seen_on_the_next_sweep() {
        let scratch = wt_scratch("e4d_existing");
        let repo = wt_repo(&scratch);
        wt_sh(&repo, &["git", "branch", "other"]);
        let wt = scratch.join("lw");
        wt_sh(
            &repo,
            &["git", "worktree", "add", "-q", &path_arg(&wt), "spare"],
        );
        let before = first_sweep(&repo).await;
        assert_eq!(worktree_branches(&before), ["spare"]);

        wt_sh(&wt, &["git", "switch", "-q", "other"]);
        let after = sweep_once(&repo, &before).await;

        assert_eq!(worktree_branches(&after), ["other"]);
    }

    /// Git deletes `<common>/worktrees` with the last linked worktree, so the
    /// absent directory is the only trace of the removal. It must read as an
    /// empty list, never as "reuse the last one".
    #[tokio::test]
    async fn removing_the_only_worktree_empties_the_list() {
        let scratch = wt_scratch("only");
        let repo = wt_repo(&scratch);
        let first = first_sweep(&repo).await;
        let wt = path_arg(&scratch.join("lw"));
        wt_sh(&repo, &["git", "worktree", "add", "-q", "-b", "solo", &wt]);
        let with_one = sweep_once(&repo, &first).await;
        assert_eq!(worktree_branches(&with_one), ["solo"]);

        wt_sh(&repo, &["git", "worktree", "remove", &wt]);
        let after = sweep_once(&repo, &with_one).await;

        assert!(
            !common_worktrees_dir(&repo).exists(),
            "git left <common>/worktrees behind, so this did not test the absent path"
        );
        assert!(
            after.worktrees.is_empty(),
            "the removed worktree is still listed: {:?}",
            after.worktrees
        );
    }

    /// A sweep of the linked worktree's own row: it lists the main checkout
    /// and every other linked worktree, and sees a sibling added on an
    /// existing branch.
    #[tokio::test]
    async fn a_linked_worktrees_own_row_sees_a_new_sibling() {
        let scratch = wt_scratch("own_row");
        let repo = wt_repo(&scratch);
        wt_sh(&repo, &["git", "branch", "other"]);
        let wt = scratch.join("lw");
        wt_sh(
            &repo,
            &["git", "worktree", "add", "-q", &path_arg(&wt), "spare"],
        );
        let before = first_sweep(&wt).await;
        assert_eq!(worktree_branches(&before), ["main"]);

        let sibling = path_arg(&scratch.join("lw2"));
        wt_sh(&repo, &["git", "worktree", "add", "-q", &sibling, "other"]);
        let after = sweep_once(&wt, &before).await;

        assert_eq!(worktree_branches(&after), ["main", "other"]);
    }

    // --- FZ-06d: the local histogram has its own reuse key ---
    //
    // Every commit is dated explicitly and every probe passes its grid start
    // in, so no assertion depends on today's date.

    const HIST_EMAIL: &str = "hist@example.com";

    fn hist_date(text: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    /// A histogram probe for `start`, counting `author` (None resolves each
    /// repo's `user.email`, as an unset `contributions_author` does).
    fn hist_probe(start: &str, author: Option<&str>) -> ContribProbe {
        ContribProbe {
            start: hist_date(start),
            author: author.map(str::to_string),
        }
    }

    /// Commit an empty change at noon local time on `day`, as whatever
    /// identity the repo is configured with.
    fn hist_commit(dir: &Path, day: &str, message: &str) {
        let when = format!("{day}T12:00:00");
        let status = std::process::Command::new("git")
            .args(["commit", "-q", "--allow-empty", "-m", message])
            .env("GIT_AUTHOR_DATE", &when)
            .env("GIT_COMMITTER_DATE", &when)
            .current_dir(dir)
            .status()
            .expect("spawn");
        assert!(status.success(), "commit failed in {}", dir.display());
    }

    /// An empty repo at `<scratch>/<name>` with `email` as its local identity.
    fn hist_repo(scratch: &Path, name: &str, email: &str) -> PathBuf {
        let dir = scratch.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        wt_sh(&dir, &["git", "init", "-q", "-b", "main"]);
        wt_sh(&dir, &["git", "config", "user.email", email]);
        wt_sh(&dir, &["git", "config", "user.name", "h"]);
        dir
    }

    /// One sweep of `path` against `old`, with or without the probe.
    async fn hist_sweep(path: &Path, old: &RepoStatus, probe: Option<&ContribProbe>) -> RepoStatus {
        let target = SweepTarget {
            display_path: path_arg(path),
            fs_path: path.to_path_buf(),
            old: Box::new(old.clone()),
        };
        rebuild_repo_status(&target, None, probe).await
    }

    /// A row for `path` that no sweep has seen yet.
    fn hist_row(path: &Path) -> RepoStatus {
        let mut row = test_repo();
        row.path = Some(path_arg(path));
        row
    }

    /// The non-empty slots of a row's histogram, as (day, count), dated by
    /// the grid `start` the caller expects the slots to be offsets from.
    fn hist_days(row: &RepoStatus, start: &str) -> Vec<(chrono::NaiveDate, u16)> {
        let daily = row.daily.as_ref().expect("the sweep left no histogram");
        let start = hist_date(start);
        daily
            .0
            .iter()
            .enumerate()
            .filter(|(_, n)| **n > 0)
            .map(|(slot, n)| (start + chrono::Duration::days(slot as i64), *n))
            .collect()
    }

    fn hist_total(row: &RepoStatus) -> u32 {
        let daily = row.daily.as_ref().expect("the sweep left no histogram");
        daily.0.iter().map(|n| u32::from(*n)).sum()
    }

    /// E7: the grid start rolls over a week while the repo stays put. The
    /// slots are offsets from the start they were built with, so reusing them
    /// under the new start draws every commit a week late.
    #[tokio::test]
    async fn a_new_grid_start_reprobes_an_unchanged_repo() {
        let scratch = wt_scratch("hist_e7");
        let repo = hist_repo(&scratch, "r", HIST_EMAIL);
        hist_commit(&repo, "2024-01-10", "one");

        let first = hist_sweep(
            &repo,
            &hist_row(&repo),
            Some(&hist_probe("2023-10-01", Some(HIST_EMAIL))),
        )
        .await;
        assert_eq!(
            hist_days(&first, "2023-10-01"),
            [(hist_date("2024-01-10"), 1)]
        );

        let rolled = hist_sweep(
            &repo,
            &first,
            Some(&hist_probe("2023-10-08", Some(HIST_EMAIL))),
        )
        .await;
        let days = hist_days(&rolled, "2023-10-08");
        let _ = std::fs::remove_dir_all(&scratch);

        assert_eq!(
            days,
            [(hist_date("2024-01-10"), 1)],
            "the histogram built for the old grid start was reused"
        );
    }

    /// E14: a sweep without the probe (the strip showing GitHub data) must
    /// not record the repo as seen by the histogram, or the next Local sweep
    /// reuses a histogram that predates the commit.
    #[tokio::test]
    async fn a_sweep_without_the_probe_does_not_bless_a_stale_histogram() {
        let scratch = wt_scratch("hist_e14");
        let repo = hist_repo(&scratch, "r", HIST_EMAIL);
        hist_commit(&repo, "2024-01-10", "one");
        let probe = hist_probe("2023-10-01", Some(HIST_EMAIL));

        let local = hist_sweep(&repo, &hist_row(&repo), Some(&probe)).await;
        assert_eq!(hist_total(&local), 1);

        hist_commit(&repo, "2024-01-11", "two");
        let github = hist_sweep(&repo, &local, None).await;
        assert_eq!(
            hist_total(&github),
            1,
            "a sweep without the probe keeps the histogram it had"
        );

        let back = hist_sweep(&repo, &github, Some(&probe)).await;
        let total = hist_total(&back);
        let _ = std::fs::remove_dir_all(&scratch);

        assert_eq!(
            total, 2,
            "the Local sweep reused a histogram from before the commit"
        );
    }

    /// `--all` walks HEAD, so a commit on a detached HEAD counts. The
    /// histogram key must carry HEAD's object id, since the ref signature
    /// only records that HEAD is detached.
    #[tokio::test]
    async fn a_commit_on_a_detached_head_reaches_the_histogram() {
        let scratch = wt_scratch("hist_detached");
        let repo = hist_repo(&scratch, "r", HIST_EMAIL);
        hist_commit(&repo, "2024-01-10", "one");
        let probe = hist_probe("2023-10-01", Some(HIST_EMAIL));

        let before = hist_sweep(&repo, &hist_row(&repo), Some(&probe)).await;
        assert_eq!(hist_total(&before), 1);

        wt_sh(&repo, &["git", "checkout", "-q", "--detach"]);
        hist_commit(&repo, "2024-01-11", "detached");
        let after = hist_sweep(&repo, &before, Some(&probe)).await;
        let total = hist_total(&after);
        let _ = std::fs::remove_dir_all(&scratch);

        assert_eq!(total, 2, "the detached commit was not counted");
    }

    /// The configured author is part of the key: changing
    /// `contributions_author` on an unchanged repo must count the new one.
    #[tokio::test]
    async fn a_changed_configured_author_reprobes_an_unchanged_repo() {
        let scratch = wt_scratch("hist_author");
        let repo = hist_repo(&scratch, "r", "a@example.com");
        hist_commit(&repo, "2024-01-10", "as a");
        wt_sh(&repo, &["git", "config", "user.email", "b@example.com"]);
        hist_commit(&repo, "2024-01-12", "as b");

        let as_a = hist_sweep(
            &repo,
            &hist_row(&repo),
            Some(&hist_probe("2023-10-01", Some("a@example.com"))),
        )
        .await;
        assert_eq!(
            hist_days(&as_a, "2023-10-01"),
            [(hist_date("2024-01-10"), 1)]
        );

        let as_b = hist_sweep(
            &repo,
            &as_a,
            Some(&hist_probe("2023-10-01", Some("b@example.com"))),
        )
        .await;
        let days = hist_days(&as_b, "2023-10-01");
        let _ = std::fs::remove_dir_all(&scratch);

        assert_eq!(
            days,
            [(hist_date("2024-01-12"), 1)],
            "the histogram counted for the previous author was reused"
        );
    }

    /// OD4: with no configured author the histogram counts the repo's
    /// `user.email` as resolved at the re-probe, not one remembered from an
    /// earlier probe.
    #[tokio::test]
    async fn an_unset_author_counts_the_email_resolved_at_the_reprobe() {
        let scratch = wt_scratch("hist_od4");
        let repo = hist_repo(&scratch, "r", "a@example.com");
        hist_commit(&repo, "2024-01-10", "as a");
        let probe = hist_probe("2023-10-01", None);

        let as_a = hist_sweep(&repo, &hist_row(&repo), Some(&probe)).await;
        assert_eq!(
            hist_days(&as_a, "2023-10-01"),
            [(hist_date("2024-01-10"), 1)]
        );

        wt_sh(&repo, &["git", "config", "user.email", "b@example.com"]);
        hist_commit(&repo, "2024-01-12", "as b");
        let mut snapshot = as_a.clone();
        snapshot.work_key = None;
        let as_b = hist_sweep(&repo, &snapshot, Some(&probe)).await;
        let days = hist_days(&as_b, "2023-10-01");
        let _ = std::fs::remove_dir_all(&scratch);

        assert_eq!(
            days,
            [(hist_date("2024-01-12"), 1)],
            "the re-probe did not count the newly resolved email alone"
        );
    }

    /// E2c, a check rather than this ticket's evidence: a commit of yours
    /// fetched from another clone moves a remote-tracking ref, which `--all`
    /// walks, so the next Local sweep must count it.
    #[tokio::test]
    async fn a_fetched_commit_of_yours_reaches_the_histogram() {
        let scratch = wt_scratch("hist_e2c");
        let origin = hist_repo(&scratch, "origin", HIST_EMAIL);
        hist_commit(&origin, "2024-01-10", "one");
        let clone = |name: &str| {
            let dir = scratch.join(name);
            wt_sh(
                &scratch,
                &["git", "clone", "-q", &path_arg(&origin), &path_arg(&dir)],
            );
            wt_sh(&dir, &["git", "config", "user.email", HIST_EMAIL]);
            wt_sh(&dir, &["git", "config", "user.name", "h"]);
            dir
        };
        let mine = clone("mine");
        let other = clone("other");
        let probe = hist_probe("2023-10-01", Some(HIST_EMAIL));

        let before = hist_sweep(&mine, &hist_row(&mine), Some(&probe)).await;
        assert_eq!(hist_total(&before), 1);

        hist_commit(&other, "2024-01-11", "elsewhere");
        wt_sh(
            &other,
            &["git", "push", "-q", "origin", "HEAD:refs/heads/side"],
        );
        wt_sh(&mine, &["git", "fetch", "-q", "origin"]);
        let after = hist_sweep(&mine, &before, Some(&probe)).await;
        let total = hist_total(&after);
        let _ = std::fs::remove_dir_all(&scratch);

        assert_eq!(total, 2, "the fetched commit was not counted");
    }

    // FZ-13u: the icons row of the settings overlay. The set is a
    // thread-local, and libtest gives each test its own thread; the guard
    // puts back whatever the thread held anyway, panics included.

    struct RestoreIcons(app::Icons);

    impl RestoreIcons {
        fn capture() -> Self {
            Self(ui::badges::active_icons())
        }
    }

    impl Drop for RestoreIcons {
        fn drop(&mut self) {
            ui::badges::set_active_icons(self.0);
        }
    }

    const ICONS_PROBE: &str = "glyph-probe";

    /// One public repo in the detail view with watch on, so a list row and
    /// the header both draw a mark from the icon set.
    fn app_for_icons() -> App {
        let mut repo = test_repo();
        repo.name = ICONS_PROBE.to_string();
        repo.repo_kind = model::RepoKind::Public;
        repo.owner = Some("Gaurgle".to_string());
        let mut app = App::new(vec![repo], "h".to_string(), 120, 40);
        app.view = app::View::Detail;
        app.watch_mode = true;
        app
    }

    /// The kind glyph left of the repo's name in the list, and the header's
    /// watch mark, as the next frame draws them.
    fn drawn_icon_marks(app: &App) -> (String, String) {
        use ratatui::backend::TestBackend;
        let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
        term.draw(|f| ui::render(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        let list = ui::panel_rects(buf.area, app).repo_list;
        let name: Vec<String> = ICONS_PROBE.chars().map(String::from).collect();
        let (x, y) = (list.top()..list.bottom())
            .find_map(|y| {
                (list.left()..list.right())
                    .find(|&x| {
                        name.iter().enumerate().all(|(i, c)| {
                            let cx = x + i as u16;
                            cx < list.right() && buf[(cx, y)].symbol() == c
                        })
                    })
                    .map(|x| (x, y))
            })
            .expect("the repo row is drawn");
        let header: String = (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect();
        let watch = header
            .split_whitespace()
            .take_while(|word| *word != "watch")
            .last()
            .unwrap_or_default()
            .to_string();
        (buf[(x - 2, y)].symbol().to_string(), watch)
    }

    fn press(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    /// `4` is the strip's key (back from `5`, FZ-30d). From another pane it
    /// focuses a folded strip and unfolds it; on the focused strip it folds
    /// it and keeps focus. Fails if the handler binds the strip to any other
    /// key.
    #[tokio::test]
    async fn the_4_key_reaches_and_folds_the_strip() {
        let args = Args::parse_from(["fleetz"]);
        let config = config::Config::default();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let backend = CrosstermBackend::new(io::stdout());
        let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 120, 44));
        let mut terminal =
            Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap();
        let mut app = App::new(Vec::new(), String::new(), 120, 44);
        app.view = app::View::Detail;
        app.contributions = app::ContributionsMode::Github;
        app.focused_panel = FocusedPanel::Detail;
        app.contributions_folded = true;

        let repo_dir = Path::new("/nonexistent");
        handle_normal_input(
            &mut app,
            &mut terminal,
            press('4'),
            &tx,
            &args,
            &config,
            repo_dir,
        )
        .await
        .unwrap();
        assert_eq!(
            app.focused_panel,
            FocusedPanel::Contributions,
            "not focused"
        );
        assert!(!app.contributions_folded, "a folded strip stayed folded");

        handle_normal_input(
            &mut app,
            &mut terminal,
            press('4'),
            &tx,
            &args,
            &config,
            repo_dir,
        )
        .await
        .unwrap();
        assert!(app.contributions_folded, "the focused strip did not fold");
        assert_eq!(
            app.focused_panel,
            FocusedPanel::Contributions,
            "folding moved focus"
        );
    }

    /// Open the overlay as `,` does and walk to the icons row with `j`.
    fn open_settings_on_the_icons_row(app: &mut App) {
        app.overlay = Overlay::Settings(settings_overlay(app, &config::Config::default()));
        for _ in 0..app::SETTING_FIELDS.len() {
            if let Overlay::Settings(ref so) = app.overlay
                && so.field() == app::SettingField::Icons
            {
                return;
            }
            handle_settings_input(app, press('j'));
        }
        panic!("no icons row in the settings overlay");
    }

    /// What Enter applies once the file is written.
    fn apply_open_settings(app: &mut App) {
        let Overlay::Settings(ref so) = app.overlay else {
            panic!("the settings overlay is not open");
        };
        let saved = so.clone();
        apply_live_settings(app, &saved);
    }

    /// Toggling the row and applying it changes the list row's kind glyph
    /// and the header's watch mark on the next frame, and toggling again
    /// brings the Nerd set back, with no restart. Fails if applying the
    /// overlay leaves the active set alone.
    #[test]
    fn toggling_the_icons_row_redraws_the_rows_and_the_header() {
        let _restore = RestoreIcons::capture();
        ui::badges::set_active_icons(app::Icons::Nerd);
        let mut app = app_for_icons();
        let nerd = drawn_icon_marks(&app);
        assert_eq!(
            nerd.0,
            ui::badges::kind_glyph(model::RepoKind::Public).unwrap(),
            "the control did not draw the Nerd set"
        );

        open_settings_on_the_icons_row(&mut app);
        handle_settings_input(&mut app, press('l'));
        app.needs_redraw = false;
        apply_open_settings(&mut app);
        assert!(app.needs_redraw, "applying did not ask for a frame");
        assert_eq!(ui::badges::active_icons(), app::Icons::Plain);
        assert_eq!(
            drawn_icon_marks(&app),
            ("+".to_string(), "o".to_string()),
            "the next frame still draws the old set"
        );

        handle_settings_input(&mut app, press('l'));
        apply_open_settings(&mut app);
        assert_eq!(ui::badges::active_icons(), app::Icons::Nerd);
        assert_eq!(drawn_icon_marks(&app), nerd);
    }

    /// Esc after toggling the row discards it, as it discards every row:
    /// the overlay does not preview, so the set in force is untouched.
    #[test]
    fn cancelling_the_overlay_leaves_the_icon_set_as_it_was() {
        let _restore = RestoreIcons::capture();
        for before in [app::Icons::Nerd, app::Icons::Plain] {
            ui::badges::set_active_icons(before);
            let mut app = app_for_icons();
            let drawn = drawn_icon_marks(&app);

            open_settings_on_the_icons_row(&mut app);
            handle_settings_input(&mut app, press('l'));
            handle_settings_input(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

            assert!(matches!(app.overlay, Overlay::None), "Esc did not close");
            assert_eq!(ui::badges::active_icons(), before);
            assert_eq!(drawn_icon_marks(&app), drawn);
        }
    }

    /// The overlay opens on the set the app is drawing with, not on a
    /// default, so saving another row writes back the set in force. Fails
    /// if a session switched to `plain` would save `nerd` the next time any
    /// setting is saved.
    #[test]
    fn the_overlay_opens_on_the_icon_set_in_force() {
        let _restore = RestoreIcons::capture();
        let app = app_for_icons();
        for in_force in [app::Icons::Plain, app::Icons::Nerd] {
            ui::badges::set_active_icons(in_force);
            let so = settings_overlay(&app, &config::Config::default());
            assert_eq!(so.icons, in_force);
            let written = so
                .config_updates()
                .into_iter()
                .find(|(key, _)| *key == "icons")
                .map(|(_, value)| value);
            let expected = match in_force {
                app::Icons::Nerd => "nerd",
                app::Icons::Plain => "plain",
            };
            assert_eq!(written, Some(config::SettingValue::Str(expected)));
        }
    }

    // FZ-10b: the scenarios of `docs/api-budget-design.md` section 5, each
    // named after its number. Clocks, jitter and responses are planted; the
    // interval is the default 30 s and `repo_limit` 50, a poll costing 3.

    /// A jitter source that always draws the same values.
    struct Planted {
        factor: f64,
        resume: i64,
    }

    impl app::Jitter for Planted {
        fn backoff_factor(&mut self) -> f64 {
            self.factor
        }
        fn resume_secs(&mut self) -> i64 {
            self.resume
        }
    }

    fn at(h: u32, m: u32, s: u32) -> chrono::DateTime<chrono::Local> {
        use chrono::TimeZone;
        chrono::Local
            .with_ymd_and_hms(2026, 10, 1, h, m, s)
            .single()
            .expect("unambiguous local time")
    }

    fn secs(n: i64) -> chrono::Duration {
        chrono::Duration::seconds(n)
    }

    /// A reading of `remaining` of 5000 on `token`, resetting at `reset`, as
    /// stated by a server whose clock read `date`.
    fn reading(
        token: &str,
        remaining: u32,
        reset: chrono::DateTime<chrono::Local>,
        date: chrono::DateTime<chrono::Local>,
    ) -> github::BudgetReading {
        github::BudgetReading {
            limit: 5000,
            remaining,
            used: 5000 - remaining,
            reset: reset.timestamp(),
            date: Some(date.with_timezone(&chrono::Utc)),
            token_id: token.to_string(),
        }
    }

    /// A successful poll that changes no row, carrying `budget`.
    fn ok_with(
        budget: Option<github::BudgetReading>,
    ) -> Result<Box<github::GithubFetch>, github::FetchError> {
        Ok(Box::new(github::GithubFetch {
            repos: Vec::new(),
            calendar: None,
            has_pr_counts: true,
            origin: github::Origin::Network,
            meta: github::FetchMeta {
                budget,
                ..Default::default()
            },
        }))
    }

    /// A successful poll that returns `budget_app`'s row `r`, still
    /// `Pending`, so the row stays covered by the last poll (FZ-09d).
    fn ok_with_pending_r(
        budget: Option<github::BudgetReading>,
    ) -> Result<Box<github::GithubFetch>, github::FetchError> {
        let mut fetch = ok_with(budget)?;
        fetch.repos.push(github::GithubRepo {
            name: "r".to_string(),
            pushed_at: chrono::DateTime::UNIX_EPOCH,
            pr_count: 0,
            prs: vec![],
            ci_status: model::CiStatus::Pending,
            ci_workflow: None,
            ci_duration: None,
            repo_kind: model::RepoKind::Unknown,
            owner: None,
            disk_usage: None,
        });
        Ok(fetch)
    }

    fn err_with(
        failure: github::Failure,
        budget: Option<github::BudgetReading>,
    ) -> Result<Box<github::GithubFetch>, github::FetchError> {
        Err(github::FetchError {
            failure,
            fallback: None,
            meta: Box::new(github::FetchMeta {
                budget,
                ..Default::default()
            }),
        })
    }

    fn failure(kind: github::FailureKind) -> github::Failure {
        github::Failure::new(kind, format!("{kind:?}"))
    }

    /// A 403 or 429 secondary limit, with or without `retry-after`.
    fn secondary(retry_after: Option<u64>) -> github::Failure {
        github::Failure {
            rate_limited: true,
            limit: Some(github::RateLimit::Secondary {
                retry_after: retry_after.map(std::time::Duration::from_secs),
            }),
            ..github::Failure::new(github::FailureKind::Http, "HTTP 429")
        }
    }

    /// Start a poll and land `result` as its answer at `now`.
    fn land_at(
        app: &mut App,
        result: Result<Box<github::GithubFetch>, github::FetchError>,
        now: chrono::DateTime<chrono::Local>,
    ) {
        let attempt = claim_github_poll(app).expect("no poll in flight");
        answer_at(app, attempt, result, now);
    }

    fn answer_at(
        app: &mut App,
        attempt: u64,
        result: Result<Box<github::GithubFetch>, github::FetchError>,
        now: chrono::DateTime<chrono::Local>,
    ) {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        apply_github_poll_at(app, attempt, result, &tx, &None, now);
    }

    /// A watching app with one row of `ci` (no path, so nothing is swept)
    /// and planted jitter.
    fn budget_app(ci: model::CiStatus, factor: f64, resume: i64) -> (App, config::Config) {
        let mut app = App::new(
            vec![RepoStatus {
                ci_status: ci,
                ..test_repo()
            }],
            "h".to_string(),
            120,
            44,
        );
        app.watch_mode = true;
        app.jitter = Box::new(Planted { factor, resume });
        (app, config::Config::default())
    }

    /// Seconds after `from` at which the next automatic attempt is first due.
    fn next_due(app: &App, config: &config::Config, from: chrono::DateTime<chrono::Local>) -> i64 {
        (0..=4000)
            .find(|s| github_poll_due(app, config, from + secs(*s)))
            .expect("never due again")
    }

    fn status_text(app: &App) -> String {
        app.status_message
            .as_ref()
            .map(|m| m.text.clone())
            .unwrap_or_default()
    }

    /// The rate limit of S6 and S15: a `RATE_LIMITED` body at 17:00:43 whose
    /// headers say 0 left until 17:23:06.
    fn land_primary_limit(app: &mut App) {
        let budget = reading("A", 0, at(17, 23, 6), at(17, 0, 43));
        land_at(
            app,
            err_with(
                github::Failure::rate_limit("API rate limit exceeded"),
                Some(budget),
            ),
            at(17, 0, 43),
        );
    }

    #[test]
    fn s2b_only_502s_since_launch_back_off_from_the_interval_without_pacing() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        let mut now = at(17, 0, 0);
        for expected in [30, 60, 120] {
            land_at(
                &mut app,
                err_with(failure(github::FailureKind::Server), None),
                now,
            );
            assert!(
                app.api_budget.known(now).is_none(),
                "a 502 made the budget known"
            );
            assert_eq!(github_poll_secs(&app, &config), 30, "paced on a guess");
            assert_eq!(next_due(&app, &config, now), expected);
            now += secs(expected);
        }
    }

    #[test]
    fn s2c_a_known_budget_past_its_reset_is_unknown_and_the_floor_returns() {
        let (mut app, config) = budget_app(model::CiStatus::Pending, 1.0, 0);
        // 600 left with 23 minutes to go stretches the pace past 30 s.
        let budget = reading("A", 600, at(17, 23, 6), at(17, 0, 0));
        land_at(&mut app, ok_with_pending_r(Some(budget)), at(17, 0, 0));
        assert!(app.api_budget.known(at(17, 0, 0)).is_some());
        assert!(github_poll_secs(&app, &config) > 30);

        assert!(app.api_budget.known(at(17, 23, 6)).is_none());
        land_at(&mut app, ok_with_pending_r(None), at(17, 30, 0));
        assert_eq!(github_poll_secs(&app, &config), GITHUB_POLL_MIN as i64);
        assert_eq!(next_due(&app, &config, at(17, 30, 0)), 15);
    }

    #[test]
    fn s3_a_healthy_budget_keeps_the_pending_floor() {
        let (mut app, config) = budget_app(model::CiStatus::Pending, 1.0, 0);
        let budget = reading("A", 3564, at(17, 23, 6), at(17, 0, 43));
        land_at(&mut app, ok_with_pending_r(Some(budget)), at(17, 0, 43));
        assert_eq!(github_poll_secs(&app, &config), 15);
        assert_eq!(next_due(&app, &config, at(17, 0, 43)), 15);
    }

    #[test]
    fn s4_600_left_with_40_minutes_paces_at_72_seconds() {
        let (mut app, config) = budget_app(model::CiStatus::Pending, 1.0, 0);
        let budget = reading("A", 600, at(17, 40, 0), at(17, 0, 0));
        assert_eq!(
            app.api_budget.pace(3, at(17, 0, 0)),
            app::Pace::Unknown,
            "known before any reading"
        );
        land_at(&mut app, ok_with(Some(budget)), at(17, 0, 0));
        assert_eq!(app.api_budget.pace(3, at(17, 0, 0)), app::Pace::Every(72));
        // The Pending row would ask for 15 s: the floor is suspended.
        assert_eq!(github_poll_secs(&app, &config), 72);
        assert_eq!(next_due(&app, &config, at(17, 0, 0)), 72);
        // And so is the push shortcut.
        app.push_detected = true;
        assert!(!github_poll_due(&app, &config, at(17, 0, 1)));
        assert_eq!(next_due(&app, &config, at(17, 0, 0)), 72);
    }

    #[test]
    fn s5_below_the_reserve_pauses_until_the_reset_plus_jitter_and_r_still_sends() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
        let budget = reading("A", 480, at(17, 23, 6), at(17, 0, 0));
        land_at(&mut app, ok_with(Some(budget)), at(17, 0, 0));
        assert!(!github_poll_due(&app, &config, at(17, 23, 15)));
        assert!(github_poll_due(&app, &config, at(17, 23, 16)));
        assert_eq!(github_limit_until(&app, &config, at(17, 10, 0)), None);
    }

    #[test]
    fn s5a_a_push_while_paused_waits_for_the_first_poll_after_the_resume() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
        let budget = reading("A", 480, at(17, 23, 6), at(17, 0, 0));
        land_at(&mut app, ok_with(Some(budget)), at(17, 0, 0));
        app.push_detected = true;
        assert!(!github_poll_due(&app, &config, at(17, 10, 0)));
        assert_eq!(next_due(&app, &config, at(17, 10, 0)), 13 * 60 + 16);
        assert!(
            app.push_detected,
            "the push was dropped before a poll served it"
        );
    }

    #[test]
    fn s5b_fewer_points_than_one_poll_is_a_known_limit_and_r_sends_nothing() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
        let budget = reading("A", 2, at(17, 23, 6), at(17, 0, 0));
        land_at(&mut app, ok_with(Some(budget)), at(17, 0, 0));
        assert_eq!(
            github_limit_until(&app, &config, at(17, 10, 0)),
            Some(at(17, 23, 16))
        );
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let args = Args::parse_from(["fleetz"]);
        // Outside a runtime: a spawned request would panic here.
        refresh_key(&mut app, &tx, &args, &config, at(17, 10, 0));
        assert!(!app.github_poll_inflight, "a request was sent");
        assert_eq!(status_text(&app), "GitHub rate limited, resumes at 17:23");
    }

    #[test]
    fn s6_a_rate_limited_body_holds_until_the_reset_plus_jitter() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
        land_primary_limit(&mut app);
        assert!(matches!(
            &app.github_freshness.outcome,
            app::Outcome::Failed(f) if f.kind == github::FailureKind::Api && f.rate_limited
        ));
        assert_eq!(
            app.github_backoff.not_before(30),
            Some(at(17, 23, 16)),
            "not-before"
        );
        assert!(!github_poll_due(&app, &config, at(17, 23, 15)));
        assert!(github_poll_due(&app, &config, at(17, 23, 16)));
    }

    #[test]
    fn s6a_a_local_clock_ten_minutes_ahead_anchors_the_reset_on_arrival() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
        // The server's clock says 17:00:00 and the reset 17:23:06; the local
        // clock reads 17:10:00 when the answer arrives.
        let budget = reading("A", 0, at(17, 23, 6), at(17, 0, 0));
        land_at(
            &mut app,
            err_with(
                github::Failure::rate_limit("API rate limit exceeded"),
                Some(budget),
            ),
            at(17, 10, 0),
        );
        assert_eq!(app.api_budget.reset_at(), Some(at(17, 33, 6)));
        assert_eq!(app.github_backoff.not_before(30), Some(at(17, 33, 16)));
        assert!(!github_poll_due(&app, &config, at(17, 33, 15)));
        assert!(github_poll_due(&app, &config, at(17, 33, 16)));
    }

    #[test]
    fn s8_a_403_with_retry_after_120_holds_120_seconds() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        land_at(&mut app, err_with(secondary(Some(120)), None), at(17, 0, 0));
        assert_eq!(next_due(&app, &config, at(17, 0, 0)), 120);
    }

    #[test]
    fn s9_a_429_without_retry_after_holds_the_later_of_60_s_and_the_backoff() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        let mut now = at(17, 0, 0);
        // Steps 1 and 2 of the ladder (30, 60) are under the minute; step 3
        // (120) is over it.
        for expected in [60, 60, 120] {
            land_at(&mut app, err_with(secondary(None), None), now);
            assert_eq!(next_due(&app, &config, now), expected);
            now += secs(expected);
        }
    }

    #[test]
    fn s10_six_consecutive_502s_climb_to_the_cap() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        let mut now = at(17, 0, 0);
        for expected in [30, 60, 120, 240, 480, 600] {
            land_at(
                &mut app,
                err_with(failure(github::FailureKind::Server), None),
                now,
            );
            assert_eq!(next_due(&app, &config, now), expected);
            now += secs(expected);
        }
        assert_eq!(app.github_backoff.failures(), 6);
        // The jitter applies on top of the cap: 480 to 720 there.
        assert_eq!(app::backoff_secs(6, 30, 0.8), 480);
        assert_eq!(app::backoff_secs(6, 30, 1.2), 720);
    }

    #[test]
    fn s10a_step_one_has_no_jitter_and_step_two_does() {
        for factor in [0.8, 1.2] {
            let (mut app, config) = budget_app(model::CiStatus::Passed, factor, 0);
            land_at(
                &mut app,
                err_with(failure(github::FailureKind::Server), None),
                at(17, 0, 0),
            );
            assert_eq!(next_due(&app, &config, at(17, 0, 0)), 30, "draw {factor}");
        }
        let (mut app, config) = budget_app(model::CiStatus::Passed, 0.8, 0);
        land_at(
            &mut app,
            err_with(failure(github::FailureKind::Server), None),
            at(17, 0, 0),
        );
        land_at(
            &mut app,
            err_with(failure(github::FailureKind::Server), None),
            at(17, 0, 30),
        );
        assert_eq!(next_due(&app, &config, at(17, 0, 30)), 48);
    }

    #[test]
    fn s11_a_success_at_step_three_resets_the_count() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        let mut now = at(17, 0, 0);
        for wait in [30, 60, 120] {
            land_at(
                &mut app,
                err_with(failure(github::FailureKind::Server), None),
                now,
            );
            now += secs(wait);
        }
        assert_eq!(app.github_backoff.failures(), 3);
        land_at(&mut app, ok_with(None), now);
        assert_eq!(app.github_backoff.failures(), 0);
        assert_eq!(app.github_backoff.not_before(30), None);
        assert_eq!(next_due(&app, &config, now), 30);
    }

    /// With O2 accepted `r` sends nothing under the limit, so the success
    /// here is an answer applied while the secondary limit's not-before was
    /// still pending, which transition 3 clears all the same.
    #[test]
    fn s11a_a_success_inside_a_secondary_limit_clears_it() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        land_at(
            &mut app,
            err_with(secondary(Some(120)), None),
            at(17, 10, 0),
        );
        assert_eq!(app.github_backoff.not_before(30), Some(at(17, 12, 0)));
        land_at(&mut app, ok_with(None), at(17, 10, 0));
        assert_eq!(app.github_backoff.not_before(30), None);
        assert_eq!(app.github_backoff.failures(), 0);
        assert_eq!(next_due(&app, &config, at(17, 10, 0)), 30);
    }

    #[test]
    fn s12_three_offline_failures_each_wait_one_interval() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        let mut now = at(17, 0, 0);
        for _ in 0..3 {
            land_at(
                &mut app,
                err_with(failure(github::FailureKind::Offline), None),
                now,
            );
            assert_eq!(next_due(&app, &config, now), 30);
            now += secs(30);
        }
        assert_eq!(app.github_backoff.failures(), 0);
    }

    #[test]
    fn s12a_offline_keeps_the_count_between_502s() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        let mut now = at(17, 0, 0);
        for (kind, expected) in [
            (github::FailureKind::Server, 30),
            (github::FailureKind::Server, 60),
            (github::FailureKind::Offline, 30),
            (github::FailureKind::Server, 120),
        ] {
            land_at(&mut app, err_with(failure(kind), None), now);
            assert_eq!(next_due(&app, &config, now), expected, "after {kind:?}");
            now += secs(expected);
        }
    }

    #[test]
    fn s12b_an_offline_or_auth_r_during_backoff_leaves_it_in_place() {
        for interrupting in [github::FailureKind::Offline, github::FailureKind::Auth] {
            let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
            let mut now = at(17, 0, 0);
            for wait in [30, 60] {
                land_at(
                    &mut app,
                    err_with(failure(github::FailureKind::Server), None),
                    now,
                );
                now += secs(wait);
            }
            // Step 3: not-before at +120 s.
            land_at(
                &mut app,
                err_with(failure(github::FailureKind::Server), None),
                now,
            );
            let step3 = now;
            land_at(
                &mut app,
                err_with(failure(interrupting), None),
                step3 + secs(20),
            );
            assert_eq!(app.github_backoff.failures(), 3, "{interrupting:?}");
            assert_eq!(
                next_due(&app, &config, step3),
                120,
                "{interrupting:?} moved the not-before"
            );
        }
    }

    #[test]
    fn s13_no_token_retries_after_one_interval_and_a_token_then_polls() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        land_at(
            &mut app,
            err_with(failure(github::FailureKind::Auth), None),
            at(17, 0, 0),
        );
        assert_eq!(next_due(&app, &config, at(17, 0, 0)), 30);
        land_at(&mut app, ok_with(None), at(17, 0, 30));
        assert_eq!(app.github_freshness.outcome, app::Outcome::Success);
        assert_eq!(next_due(&app, &config, at(17, 0, 30)), 30);
    }

    #[test]
    fn s14_three_401s_back_off_and_a_success_returns_to_normal() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        let mut now = at(17, 0, 0);
        for expected in [30, 60, 120] {
            land_at(
                &mut app,
                err_with(failure(github::FailureKind::Refused), None),
                now,
            );
            assert_eq!(next_due(&app, &config, now), expected);
            now += secs(expected);
        }
        land_at(&mut app, ok_with(None), now);
        assert_eq!(next_due(&app, &config, now), 30);
    }

    #[test]
    fn s15_r_under_a_rate_limit_sweeps_and_sends_nothing() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
        land_primary_limit(&mut app);
        let before = app.github_freshness.clone();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let args = Args::parse_from(["fleetz"]);
        // Outside a runtime: a spawned request would panic here.
        refresh_key(&mut app, &tx, &args, &config, at(17, 10, 0));
        assert!(!app.github_poll_inflight, "a request was sent");
        assert!(status_text(&app).contains("17:23"), "{}", status_text(&app));
        assert_eq!(app.github_freshness, before);
    }

    /// A primary limit relative to the real clock, for the paths that read
    /// it (`apply_load_result`): 13 minutes left, jitter 10 s.
    fn limited_now(app: &mut App) -> chrono::DateTime<chrono::Local> {
        let now = chrono::Local::now();
        let budget = reading("A", 0, now + secs(13 * 60), now);
        land_at(
            app,
            err_with(
                github::Failure::rate_limit("API rate limit exceeded"),
                Some(budget),
            ),
            now,
        );
        now + secs(13 * 60 + 10)
    }

    #[test]
    fn s16_r_under_a_rate_limit_rebuilds_from_memory_and_names_the_resume() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
        app.dataset = poll_says_passed().ok().unwrap().repos;
        let resume = limited_now(&mut app);
        assert_eq!(
            reload_action(&app, &config, chrono::Local::now()),
            ReloadAction::FromMemory
        );
        let before = app.github_freshness.clone();
        let notes = ViewNotes {
            header: "h".to_string(),
            ambiguity: None,
        };
        apply_load_result(
            &mut app,
            &config,
            Ok(Box::new(memory_rebuild_result(vec![test_repo()], notes))),
        );
        assert_eq!(app.github_freshness, before, "freshness moved");
        let status = status_text(&app);
        assert_ne!(status, "Reloaded");
        assert!(
            status.contains(&resume.format("%H:%M").to_string()),
            "{status:?}"
        );
        assert!(!app.github_poll_inflight);
        assert!(!app.loading);
    }

    #[test]
    fn s16a_r_under_a_rate_limit_with_nothing_in_memory_only_names_the_resume() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
        land_primary_limit(&mut app);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let args = Args::parse_from(["fleetz"]);
        // Outside a runtime: a spawned load would panic here.
        reload_key(
            &mut app,
            &tx,
            &args,
            &config,
            Path::new("/nonexistent"),
            at(17, 10, 0),
        );
        assert_eq!(status_text(&app), "GitHub rate limited, resumes at 17:23");
        assert!(!app.loading);
        assert!(!app.github_poll_inflight);
    }

    #[test]
    fn s16b_r_under_a_rate_limit_with_a_poll_in_flight_waits_for_it() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
        app.dataset = poll_says_passed().ok().unwrap().repos;
        land_primary_limit(&mut app);
        let in_flight = claim_github_poll(&mut app).unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let args = Args::parse_from(["fleetz"]);
        reload_key(
            &mut app,
            &tx,
            &args,
            &config,
            Path::new("/nonexistent"),
            at(17, 10, 0),
        );
        assert_eq!(status_text(&app), REFRESH_RUNNING);
        assert!(!app.loading, "a rebuild started");

        let budget = reading("A", 0, at(17, 23, 6), at(17, 10, 5));
        answer_at(
            &mut app,
            in_flight,
            err_with(
                github::Failure::rate_limit("API rate limit exceeded"),
                Some(budget),
            ),
            at(17, 10, 5),
        );
        assert_eq!(
            reload_action(&app, &config, at(17, 10, 6)),
            ReloadAction::FromMemory
        );
    }

    #[test]
    fn s17_r_during_backoff_sends_and_its_outcome_moves_the_ladder() {
        for succeeds in [false, true] {
            let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
            land_at(
                &mut app,
                err_with(failure(github::FailureKind::Server), None),
                at(17, 0, 0),
            );
            land_at(
                &mut app,
                err_with(failure(github::FailureKind::Server), None),
                at(17, 0, 30),
            );
            // Backing off, not limited: `r` goes to GitHub.
            assert_eq!(github_limit_until(&app, &config, at(17, 0, 40)), None);
            let result = match succeeds {
                true => ok_with(None),
                false => err_with(failure(github::FailureKind::Server), None),
            };
            land_at(&mut app, result, at(17, 0, 40));
            let expected = if succeeds { 0 } else { 3 };
            assert_eq!(app.github_backoff.failures(), expected);
        }
    }

    #[test]
    fn s18_r_while_a_load_runs_is_ignored() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        app.loading = true;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let args = Args::parse_from(["fleetz"]);
        // Outside a runtime: a second load would panic here.
        reload_key(
            &mut app,
            &tx,
            &args,
            &config,
            Path::new("/nonexistent"),
            at(17, 0, 0),
        );
        assert_eq!(status_text(&app), REFRESH_RUNNING);
        assert!(!app.github_poll_inflight);
    }

    #[test]
    fn s19b_one_failure_after_a_suspend_waits_exactly_one_interval() {
        for kind in [github::FailureKind::Offline, github::FailureKind::TimedOut] {
            // A 0.8 draw would show if step 1 were jittered.
            let (mut app, config) = budget_app(model::CiStatus::Passed, 0.8, 0);
            land_at(&mut app, ok_with(None), at(17, 0, 0));
            land_at(&mut app, err_with(failure(kind), None), at(17, 0, 30));
            assert_eq!(next_due(&app, &config, at(17, 0, 30)), 30, "{kind:?}");
            assert_eq!(github_limit_until(&app, &config, at(17, 0, 31)), None);
        }
    }

    /// Any sequence of outcomes and readings: the next automatic attempt is
    /// never sooner than the rule 6 interval in force after the last one,
    /// except for the push shortcut while healthy and unstretched.
    #[test]
    fn s20_no_automatic_attempt_sooner_than_the_interval_in_force() {
        let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
        let mut draw = |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        for _run in 0..20 {
            let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
            app.jitter = Box::new(Planted {
                factor: 0.8 + 0.4 * (draw(100) as f64 / 99.0),
                resume: draw(31) as i64,
            });
            let mut now = at(12, 0, 0);
            for _step in 0..40 {
                let budget = match draw(3) {
                    0 => None,
                    _ => Some(reading(
                        "A",
                        draw(5001) as u32,
                        now + secs(draw(3600) as i64 + 1),
                        now,
                    )),
                };
                let result = match draw(9) {
                    0 | 1 => ok_with(budget),
                    2 => err_with(failure(github::FailureKind::Server), budget),
                    3 => err_with(failure(github::FailureKind::Offline), None),
                    4 => err_with(failure(github::FailureKind::Auth), None),
                    5 => err_with(github::Failure::rate_limit("limit"), budget),
                    6 => err_with(secondary(Some(draw(200))), budget),
                    7 => err_with(secondary(None), budget),
                    _ => err_with(failure(github::FailureKind::Refused), budget),
                };
                land_at(&mut app, result, now);
                app.repos[0].ci_status = match draw(2) {
                    0 => model::CiStatus::Pending,
                    _ => model::CiStatus::Passed,
                };
                app.push_detected = draw(4) == 0;

                let failing = app.github_freshness.is_failing();
                let slowed = match app.api_budget.pace(3, now) {
                    app::Pace::Unknown => false,
                    app::Pace::Every(s) => s > 30,
                    app::Pace::Paused { .. } => true,
                };
                let pending = app.repos[0].ci_status == model::CiStatus::Pending;
                let in_force = if !failing && !slowed && pending {
                    15
                } else {
                    30
                };
                let due = next_due(&app, &config, now);
                if due < in_force {
                    assert!(
                        app.push_detected && !failing && !slowed,
                        "due at +{due}, under {in_force}, without the push exception"
                    );
                }
                assert!(due > 0 || (app.push_detected && !failing && !slowed));
                app.push_detected = false;
                now += secs(next_due(&app, &config, now));
            }
        }
    }

    #[test]
    fn s21_a_late_reading_never_raises_remaining_in_the_same_window() {
        let (mut app, _config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        let first = claim_github_poll(&mut app).unwrap();
        app.github_poll_started = std::time::Instant::now()
            .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
        expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, at(17, 1, 0));
        let second = claim_github_poll(&mut app).unwrap();
        let newer = reading("A", 2900, at(17, 23, 6), at(17, 1, 10));
        answer_at(&mut app, second, ok_with(Some(newer)), at(17, 1, 10));
        let applied = app.github_freshness.clone();

        let late = reading("A", 3000, at(17, 23, 6), at(17, 0, 30));
        answer_at(&mut app, first, ok_with(Some(late)), at(17, 1, 20));
        assert_eq!(app.api_budget.reading().map(|r| r.remaining), Some(2900));
        assert_eq!(app.github_freshness, applied, "the late outcome applied");
    }

    #[test]
    fn s21a_a_late_reading_of_a_newer_window_replaces() {
        let (mut app, _config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        let first = claim_github_poll(&mut app).unwrap();
        app.github_poll_started = std::time::Instant::now()
            .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
        expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, at(17, 1, 0));
        let second = claim_github_poll(&mut app).unwrap();
        answer_at(
            &mut app,
            second,
            ok_with(Some(reading("A", 100, at(17, 23, 6), at(17, 1, 10)))),
            at(17, 1, 10),
        );
        let late = reading("A", 4990, at(18, 23, 6), at(17, 23, 30));
        answer_at(&mut app, first, ok_with(Some(late.clone())), at(17, 23, 30));
        assert_eq!(app.api_budget.reading(), Some(&late));
    }

    #[test]
    fn s22_an_applied_reading_on_another_token_replaces_and_drops_the_limit() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
        land_primary_limit(&mut app);
        assert_eq!(app.github_backoff.failures(), 1);
        assert!(github_limit_until(&app, &config, at(17, 1, 0)).is_some());
        // A body cut off mid-read is `Offline`, yet its headers came on the
        // new token: transparent to the ladder, but the switch drops A's
        // limit.
        let other = reading("B", 4000, at(17, 23, 6), at(17, 1, 0));
        land_at(
            &mut app,
            err_with(failure(github::FailureKind::Offline), Some(other.clone())),
            at(17, 1, 0),
        );
        assert_eq!(app.api_budget.reading(), Some(&other));
        assert_eq!(app.github_backoff.not_before(30), None, "A's limit kept");
        assert_eq!(app.github_backoff.failures(), 1, "the count was reset");
        assert_eq!(github_limit_until(&app, &config, at(17, 1, 1)), None);
    }

    #[test]
    fn s22a_a_discarded_answer_on_the_old_token_undoes_nothing() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
        let now = chrono::Local::now();
        let a_poll = claim_github_poll(&mut app).unwrap();
        let b = reading("B", 0, now + secs(600), now);
        apply_load_result(
            &mut app,
            &config,
            Ok(Box::new(LoadResult {
                kind: LoadKind::Reload,
                repos: Vec::new(),
                header: "h".to_string(),
                dataset: Some(Vec::new()),
                github_calendar: None,
                source: app::DatasetSource::Fallback {
                    failure: github::Failure::rate_limit("API rate limit exceeded"),
                    written: None,
                },
                ambiguity: None,
                meta: Some(github::FetchMeta {
                    budget: Some(b.clone()),
                    ..Default::default()
                }),
            })),
        );
        let held = app.api_budget.clone();
        let not_before = app.github_backoff.clone();
        assert_eq!(held.reading(), Some(&b));
        assert!(not_before.limit_until(30).is_some());

        let a = reading("A", 0, now + secs(1200), now);
        answer_at(
            &mut app,
            a_poll,
            err_with(
                github::Failure::rate_limit("API rate limit exceeded"),
                Some(a),
            ),
            now + secs(1),
        );
        assert_eq!(app.api_budget, held, "the old token came back");
        assert_eq!(app.github_backoff, not_before);
    }

    #[test]
    fn s23_a_cache_served_launch_with_watch_off_polls_once_whatever_the_budget() {
        for limited in [false, true] {
            let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
            app.watch_mode = false;
            launch(
                &mut app,
                LoadKind::Initial,
                app::DatasetSource::Cache {
                    written: Some(at(8, 0, 0)),
                },
            );
            assert!(github_poll_due(&app, &config, at(17, 0, 0)), "not at once");
            let result = match limited {
                false => ok_with(Some(reading("A", 600, at(17, 40, 0), at(17, 0, 0)))),
                true => err_with(
                    github::Failure::rate_limit("API rate limit exceeded"),
                    Some(reading("A", 0, at(17, 23, 6), at(17, 0, 0))),
                ),
            };
            land_at(&mut app, result, at(17, 0, 0));
            for later in [at(17, 23, 30), at(23, 0, 0)] {
                assert!(
                    !github_poll_due(&app, &config, later),
                    "watch off polled again (limited: {limited})"
                );
            }
        }
    }

    #[test]
    fn s24_two_instances_paused_together_resume_apart() {
        for jitter in [3, 21] {
            let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, jitter);
            let budget = reading("A", 400, at(17, 23, 6), at(17, 0, 0));
            land_at(&mut app, ok_with(Some(budget)), at(17, 0, 0));
            assert_eq!(
                next_due(&app, &config, at(17, 0, 0)),
                23 * 60 + 6 + jitter,
                "jitter {jitter}"
            );
        }
    }

    /// Design 4.1 rule 3: within one window `remaining` only falls, so the
    /// lower reading is the newer one, from an applied answer or a discarded
    /// one on the held token.
    #[test]
    fn a_lower_remaining_in_the_same_window_replaces_the_held_reading() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        let reset = at(17, 40, 0);
        land_at(
            &mut app,
            ok_with(Some(reading("A", 4000, reset, at(17, 0, 0)))),
            at(17, 0, 0),
        );
        assert_eq!(github_poll_secs(&app, &config), 30);
        land_at(
            &mut app,
            ok_with(Some(reading("A", 600, reset, at(17, 0, 0)))),
            at(17, 0, 0),
        );
        assert_eq!(app.api_budget.reading().map(|r| r.remaining), Some(600));
        assert_eq!(app.api_budget.pace(3, at(17, 0, 0)), app::Pace::Every(72));
        assert_eq!(github_poll_secs(&app, &config), 72);

        // A given-up poll answers after a newer one: discarded, but its lower
        // reading on the held token still applies.
        let first = claim_github_poll(&mut app).unwrap();
        app.github_poll_started = std::time::Instant::now()
            .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
        expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, at(17, 1, 0));
        let second = claim_github_poll(&mut app).unwrap();
        answer_at(&mut app, second, ok_with(None), at(17, 1, 10));
        let applied = app.github_freshness.clone();
        answer_at(
            &mut app,
            first,
            ok_with(Some(reading("A", 550, reset, at(17, 0, 50)))),
            at(17, 1, 20),
        );
        assert_eq!(app.api_budget.reading().map(|r| r.remaining), Some(550));
        assert_eq!(
            app.github_freshness, applied,
            "the discarded outcome applied"
        );
    }

    /// The pace never reaches past the resume: with one point above the
    /// reserve, an hour to the reset and a cost of 3, the uncapped spacing
    /// would be 10800 s.
    #[test]
    fn the_pace_is_capped_at_the_resume() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        land_at(
            &mut app,
            ok_with(Some(reading("A", 501, at(18, 0, 0), at(17, 0, 0)))),
            at(17, 0, 0),
        );
        let to_resume = (app.api_budget.resume_at().unwrap() - at(17, 0, 0)).num_seconds();
        assert_eq!(to_resume, 3600);
        match app.api_budget.pace(3, at(17, 0, 0)) {
            app::Pace::Every(secs) => assert!(secs <= to_resume, "paced {secs} s"),
            other => panic!("expected a pace, got {other:?}"),
        }
        assert!(github_poll_due(&app, &config, at(18, 0, 0)));
        assert_eq!(next_due(&app, &config, at(17, 0, 0)), 3600);
    }

    /// Transition 5: a give-up is a `TimedOut` failure and climbs the ladder.
    #[test]
    fn a_give_up_climbs_the_backoff_ladder() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        land_at(
            &mut app,
            err_with(failure(github::FailureKind::Server), None),
            at(17, 0, 0),
        );
        claim_github_poll(&mut app).unwrap();
        app.github_poll_started = std::time::Instant::now()
            .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
        expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, at(17, 0, 30));
        assert_eq!(app.github_backoff.failures(), 2);
        assert_eq!(next_due(&app, &config, at(17, 0, 30)), 60, "not step 2");
    }

    /// Transitions 3 and 6: an `R` that went to GitHub and succeeded clears
    /// the ladder like a successful poll.
    #[test]
    fn a_successful_reload_clears_the_backoff_ladder() {
        let (mut app, _config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        let mut now = at(17, 0, 0);
        for wait in [30, 60, 120, 240, 480] {
            land_at(
                &mut app,
                err_with(failure(github::FailureKind::Server), None),
                now,
            );
            now += secs(wait);
        }
        assert_eq!(app.github_backoff.failures(), 5);
        launch(&mut app, LoadKind::Reload, app::DatasetSource::Network);
        assert_eq!(app.github_backoff.failures(), 0);
        assert_eq!(app.github_backoff.not_before(30), None);
    }

    /// A launch refused by a rate limit with no cache to fall back on still
    /// learns the reset from the refusal's headers.
    #[test]
    fn a_failed_load_carries_its_budget_reading() {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
        let now = chrono::Local::now();
        let budget = reading("A", 0, now + secs(600), now);
        let limit = github::Failure::rate_limit("API rate limit exceeded");
        apply_load_result(
            &mut app,
            &config,
            Err(LoadError {
                message: limit.message.clone(),
                github_failure: Some(limit),
                budget: Some(Box::new(budget.clone())),
            }),
        );
        assert_eq!(app.api_budget.reading(), Some(&budget));
        let until = github_limit_until(&app, &config, now).expect("no known limit");
        assert!(until >= now + secs(610), "held only until {until}");
        assert!(
            until <= chrono::Local::now() + secs(610),
            "held until {until}"
        );
    }

    // --- FZ-09d: rows a poll did not cover, and an account change ---
    //
    // `docs/coverage-cache-design.md` 4.7 and scenarios CV9 to CV20.

    /// A Primary carrier row of `owner`/`name` with CI `ci` and two PRs.
    fn carrier_row(owner: &str, name: &str, ci: model::CiStatus) -> RepoStatus {
        RepoStatus {
            name: name.to_string(),
            owner: Some(owner.to_string()),
            ci_status: ci,
            ci_workflow: Some("ci".to_string()),
            pr_count: 2,
            repo_kind: model::RepoKind::Public,
            ..test_repo()
        }
    }

    /// The dataset entry for `owner`/`name`, with CI `ci` and no PRs.
    fn entry(owner: &str, name: &str, ci: model::CiStatus) -> github::GithubRepo {
        github::GithubRepo {
            name: name.to_string(),
            pushed_at: chrono::DateTime::UNIX_EPOCH,
            pr_count: 0,
            prs: vec![],
            ci_status: ci,
            ci_workflow: Some("ci".to_string()),
            ci_duration: None,
            repo_kind: model::RepoKind::Public,
            owner: Some(owner.to_string()),
            disk_usage: None,
        }
    }

    /// A successful poll on `account` returning `repos` and, when given, a
    /// GitHub calendar whose year totals `calendar_total`.
    fn fetch_on(
        account: Option<&str>,
        repos: Vec<github::GithubRepo>,
        calendar_total: Option<u32>,
    ) -> Result<Box<github::GithubFetch>, github::FetchError> {
        Ok(Box::new(github::GithubFetch {
            repos,
            calendar: calendar_total.map(calendar_of),
            has_pr_counts: true,
            origin: github::Origin::Network,
            meta: github::FetchMeta {
                account: account.map(str::to_string),
                ..Default::default()
            },
        }))
    }

    fn calendar_of(total: u32) -> Box<contributions::Calendar> {
        let today = chrono::Local::now().date_naive();
        let mut calendar = contributions::Calendar::new(today, contributions::Source::Github);
        calendar.set(today, total);
        Box::new(calendar)
    }

    fn remote(owner: &str, name: &str) -> app::RemoteKey {
        (Some(owner.to_string()), name.to_string())
    }

    /// Rows for `alice/a` and `alice/b`, both passing, as CV9 gives them.
    fn alice_app() -> App {
        App::new(
            vec![
                carrier_row("alice", "a", model::CiStatus::Passed),
                carrier_row("alice", "b", model::CiStatus::Passed),
            ],
            "h".to_string(),
            120,
            44,
        )
    }

    fn row<'a>(app: &'a App, name: &str) -> &'a RepoStatus {
        app.repos.iter().find(|r| r.name == name).expect("row")
    }

    /// Land a fetched load of `kind` on `account` with `rows` and `dataset`,
    /// saving any window to `store`.
    fn land_fetched(
        app: &mut App,
        kind: LoadKind,
        account: Option<&str>,
        rows: Vec<RepoStatus>,
        dataset: Vec<github::GithubRepo>,
        calendar_total: Option<u32>,
        store: &window_state::WindowState,
    ) {
        apply_load_result_saving_to(
            app,
            &config::Config::default(),
            Ok(Box::new(LoadResult {
                kind,
                repos: rows,
                header: "h".to_string(),
                dataset: Some(dataset),
                github_calendar: calendar_total.map(calendar_of),
                source: app::DatasetSource::Network,
                ambiguity: None,
                meta: Some(github::FetchMeta {
                    account: account.map(str::to_string),
                    ..Default::default()
                }),
            })),
            store,
        );
    }

    /// The rows assembly builds from `dataset` with nothing on disk: an
    /// empty home and an empty root, no `git fetch`, as the account rebuild
    /// asks `assemble_view` for them.
    async fn rows_from(scratch: &Path, dataset: &[github::GithubRepo]) -> Vec<RepoStatus> {
        let home = scratch.join("home");
        let root = scratch.join("repos");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        id_assemble(&home, &[root], dataset, None).await.0
    }

    /// Land what the account rebuild delivers: a rebuild from memory over
    /// `app.dataset`, saving any window to `store`.
    async fn land_account_rebuild(
        app: &mut App,
        scratch: &Path,
        store: &window_state::WindowState,
    ) {
        let rows = rows_from(scratch, &app.dataset).await;
        let notes = ViewNotes {
            header: "h".to_string(),
            ambiguity: None,
        };
        let result = memory_rebuild_result(rows, notes);
        assert!(matches!(result.kind, LoadKind::FromMemory));
        assert!(result.meta.is_none() && result.dataset.is_none());
        apply_load_result_saving_to(app, &config::Config::default(), Ok(Box::new(result)), store);
    }

    #[test]
    fn cv9_a_carrier_row_the_poll_did_not_return_is_marked_and_keeps_its_values() {
        let mut app = alice_app();
        poll_with(
            &mut app,
            fetch_on(
                None,
                vec![entry("alice", "a", model::CiStatus::Failed)],
                None,
            ),
        );
        assert!(app.not_in_last_poll.contains(&remote("alice", "b")));
        assert!(!app.not_in_last_poll.contains(&remote("alice", "a")));
        let b = row(&app, "b");
        assert_eq!(b.ci_status, model::CiStatus::Passed, "b's CI was cleared");
        assert_eq!(b.pr_count, 2, "b's PRs were cleared");
        assert!(app.is_not_in_last_poll(b));
        assert_eq!(row(&app, "a").ci_status, model::CiStatus::Failed);

        poll_with(
            &mut app,
            fetch_on(
                None,
                vec![
                    entry("alice", "a", model::CiStatus::Failed),
                    entry("alice", "b", model::CiStatus::Passed),
                ],
                None,
            ),
        );
        assert!(
            app.not_in_last_poll.is_empty(),
            "{:?}",
            app.not_in_last_poll
        );
    }

    #[test]
    fn cv10_a_failed_poll_leaves_the_not_covered_set_unchanged() {
        let mut app = alice_app();
        poll_with(
            &mut app,
            fetch_on(
                None,
                vec![entry("alice", "a", model::CiStatus::Passed)],
                None,
            ),
        );
        let before = app.not_in_last_poll.clone();
        assert!(!before.is_empty());
        poll(&mut app, Err("HTTP 502".to_string()));
        assert_eq!(app.not_in_last_poll, before);
    }

    #[test]
    fn cv11_a_pending_row_the_poll_did_not_cover_does_not_hold_the_floor() {
        let config = config::Config {
            github_poll_interval: 90,
            ..config::Config::default()
        };
        let mut app = App::new(
            vec![
                carrier_row("alice", "a", model::CiStatus::Passed),
                carrier_row("alice", "b", model::CiStatus::Pending),
            ],
            "h".to_string(),
            120,
            44,
        );
        app.watch_mode = true;
        assert_eq!(github_poll_secs(&app, &config), GITHUB_POLL_MIN as i64);
        poll_with(
            &mut app,
            fetch_on(
                None,
                vec![entry("alice", "a", model::CiStatus::Passed)],
                None,
            ),
        );
        assert_eq!(row(&app, "b").ci_status, model::CiStatus::Pending);
        assert_eq!(github_poll_secs(&app, &config), 90);
    }

    #[tokio::test]
    async fn cv12_a_poll_on_another_account_rebuilds_the_rows_from_its_dataset() {
        let scratch = wt_scratch("cv12");
        let store = window_state::WindowState::at(scratch.join("window.toml"));
        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        let a_dataset = vec![
            entry("alice", "a", model::CiStatus::Passed),
            entry("alice", "b", model::CiStatus::Failed),
        ];
        let a_rows = rows_from(&scratch, &a_dataset).await;
        land_fetched(
            &mut app,
            LoadKind::Initial,
            Some("A"),
            a_rows,
            a_dataset,
            Some(3),
            &store,
        );
        assert_eq!(app.repos.len(), 2);
        let attempts_before = app.github_freshness.clone();

        poll_with(
            &mut app,
            fetch_on(
                Some("B"),
                vec![entry("bob", "x", model::CiStatus::Pending)],
                Some(11),
            ),
        );
        assert!(app.account_rebuild_due, "no rebuild asked for");
        // The poll itself marks A's rows, until the rebuild lands.
        assert!(app.not_in_last_poll.contains(&remote("alice", "a")));
        let after_poll = app.github_freshness.clone();
        assert_ne!(after_poll, attempts_before);

        land_account_rebuild(&mut app, &scratch, &store).await;
        let names: Vec<_> = app
            .repos
            .iter()
            .map(|r| (r.owner.clone(), r.name.clone(), r.ci_status.clone()))
            .collect();
        assert_eq!(
            names,
            vec![(
                Some("bob".to_string()),
                "x".to_string(),
                model::CiStatus::Pending
            )],
            "a row kept A-only data"
        );
        assert!(app.not_in_last_poll.is_empty());
        assert_eq!(
            app.github_calendar.as_ref().map(|c| c.total()),
            Some(11),
            "B's calendar is not shown"
        );
        // No request: freshness is what the poll left. No window written,
        // no "Window:" status.
        assert_eq!(app.github_freshness, after_poll);
        assert!(!scratch.join("window.toml").exists(), "window.toml written");
        let status = status_text(&app);
        assert!(!status.starts_with("Window:"), "{status}");
        std::fs::remove_dir_all(&scratch).ok();
    }

    #[test]
    fn cv13_a_late_answer_on_the_old_account_triggers_no_rebuild() {
        let scratch = wt_scratch("cv13");
        let store = window_state::WindowState::at(scratch.join("window.toml"));
        let mut app = alice_app();
        land_fetched(
            &mut app,
            LoadKind::Initial,
            Some("A"),
            alice_app().repos,
            vec![entry("alice", "a", model::CiStatus::Passed)],
            None,
            &store,
        );
        let first = claim_github_poll(&mut app).unwrap();
        app.github_poll_started = std::time::Instant::now()
            .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
        expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, chrono::Local::now());
        let second = claim_github_poll(&mut app).unwrap();
        answer(
            &mut app,
            second,
            fetch_on(
                Some("B"),
                vec![entry("bob", "x", model::CiStatus::Passed)],
                None,
            ),
        );
        assert!(app.account_rebuild_due);
        // As `start_account_rebuild` takes the flag when it spawns.
        app.account_rebuild_due = false;

        answer(
            &mut app,
            first,
            fetch_on(
                Some("A"),
                vec![entry("alice", "a", model::CiStatus::Failed)],
                None,
            ),
        );
        assert!(!app.account_rebuild_due, "the discarded answer rebuilt");
        assert_eq!(app.github_meta.account.as_deref(), Some("B"));
        std::fs::remove_dir_all(&scratch).ok();
    }

    #[tokio::test]
    async fn cv15_every_rebuild_from_one_dataset_clears_the_not_covered_set() {
        let scratch = wt_scratch("cv15");
        let store = window_state::WindowState::at(scratch.join("window.toml"));
        let alice = || {
            vec![
                entry("alice", "a", model::CiStatus::Passed),
                entry("alice", "b", model::CiStatus::Passed),
            ]
        };
        let marked = || {
            let mut app = alice_app();
            app.dataset = alice();
            poll_with(
                &mut app,
                fetch_on(
                    None,
                    vec![entry("alice", "a", model::CiStatus::Passed)],
                    None,
                ),
            );
            assert!(app.not_in_last_poll.contains(&remote("alice", "b")));
            app
        };
        let notes = || ViewNotes {
            header: "h".to_string(),
            ambiguity: None,
        };
        let config = config::Config::default();

        let mut app = marked();
        land_fetched(
            &mut app,
            LoadKind::Initial,
            None,
            alice_app().repos,
            alice(),
            None,
            &store,
        );
        assert!(app.not_in_last_poll.is_empty(), "after a launch");

        let mut app = marked();
        land_fetched(
            &mut app,
            LoadKind::Reload,
            None,
            alice_app().repos,
            alice(),
            None,
            &store,
        );
        assert!(app.not_in_last_poll.is_empty(), "after R");

        let mut app = marked();
        let w = time::TimeWindow::clamped(2, time::WindowUnit::Days);
        let result = window_apply_result(w, alice_app().repos, notes());
        apply_load_result_saving_to(&mut app, &config, Ok(Box::new(result)), &store);
        assert!(app.not_in_last_poll.is_empty(), "after a window apply");

        let mut app = marked();
        let result = memory_rebuild_result(alice_app().repos, notes());
        apply_load_result_saving_to(&mut app, &config, Ok(Box::new(result)), &store);
        assert!(
            app.not_in_last_poll.is_empty(),
            "after a rebuild from memory"
        );

        let mut app = marked();
        poll_with(
            &mut app,
            fetch_on(
                Some("B"),
                vec![entry("bob", "x", model::CiStatus::Passed)],
                None,
            ),
        );
        assert!(app.account_rebuild_due);
        assert!(!app.not_in_last_poll.is_empty());
        land_account_rebuild(&mut app, &scratch, &store).await;
        assert!(app.not_in_last_poll.is_empty(), "after an account rebuild");
        std::fs::remove_dir_all(&scratch).ok();
    }

    #[tokio::test]
    async fn cv16_a_launch_on_an_unknown_account_rebuilds_once_on_the_first_keyed_poll() {
        let scratch = wt_scratch("cv16");
        let store = window_state::WindowState::at(scratch.join("window.toml"));
        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        // Served from the legacy file, or under the signed-out rule: the
        // load's account is unknown.
        apply_load_result_saving_to(
            &mut app,
            &config::Config::default(),
            Ok(Box::new(LoadResult {
                kind: LoadKind::Initial,
                repos: alice_app().repos,
                header: "h".to_string(),
                dataset: Some(vec![entry("alice", "a", model::CiStatus::Passed)]),
                github_calendar: None,
                source: app::DatasetSource::Cache { written: None },
                ambiguity: None,
                meta: Some(github::FetchMeta::default()),
            })),
            &store,
        );
        assert_eq!(app.github_meta.account, None);
        let a_poll = || {
            fetch_on(
                Some("A"),
                vec![entry("alice", "a", model::CiStatus::Failed)],
                None,
            )
        };

        poll_with(&mut app, a_poll());
        assert!(
            app.account_rebuild_due,
            "the first keyed poll did not rebuild"
        );
        app.account_rebuild_due = false;
        land_account_rebuild(&mut app, &scratch, &store).await;
        let rows: Vec<_> = app.repos.iter().map(|r| r.name.clone()).collect();
        assert_eq!(rows, vec!["a".to_string()], "not rebuilt from A's dataset");

        poll_with(&mut app, a_poll());
        assert!(!app.account_rebuild_due, "a second rebuild");
        std::fs::remove_dir_all(&scratch).ok();
    }

    #[tokio::test]
    async fn cv17_a_rotated_token_on_one_account_rebuilds_once() {
        let scratch = wt_scratch("cv17");
        let store = window_state::WindowState::at(scratch.join("window.toml"));
        let dataset = vec![
            entry("alice", "a", model::CiStatus::Passed),
            entry("alice", "b", model::CiStatus::Passed),
        ];
        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        let rows = rows_from(&scratch, &dataset).await;
        land_fetched(
            &mut app,
            LoadKind::Initial,
            Some("F1"),
            rows,
            dataset,
            None,
            &store,
        );

        let polled = vec![
            entry("alice", "a", model::CiStatus::Failed),
            entry("alice", "c", model::CiStatus::Passed),
        ];
        poll_with(&mut app, fetch_on(Some("F2"), polled.clone(), None));
        assert!(app.account_rebuild_due, "no rebuild on a new fingerprint");
        app.account_rebuild_due = false;
        land_account_rebuild(&mut app, &scratch, &store).await;

        let shape = |rows: &[RepoStatus]| -> Vec<_> {
            rows.iter()
                .map(|r| {
                    (
                        r.owner.clone(),
                        r.name.clone(),
                        r.ci_status.clone(),
                        r.location.clone(),
                    )
                })
                .collect()
        };
        let alone = rows_from(&scratch, &polled).await;
        assert_eq!(shape(&app.repos), shape(&alone));

        poll_with(&mut app, fetch_on(Some("F2"), polled, None));
        assert!(!app.account_rebuild_due, "a second rebuild on the same key");
        std::fs::remove_dir_all(&scratch).ok();
    }

    #[test]
    fn cv18_a_poll_with_no_account_key_never_rebuilds() {
        let scratch = wt_scratch("cv18");
        let store = window_state::WindowState::at(scratch.join("window.toml"));
        // Never had a load, and had one whose account is unknown.
        let mut never_loaded = alice_app();
        let mut unknown = alice_app();
        land_fetched(
            &mut unknown,
            LoadKind::Initial,
            None,
            alice_app().repos,
            vec![],
            None,
            &store,
        );
        for app in [&mut never_loaded, &mut unknown] {
            let before = app.repos.clone();
            poll_with(
                app,
                fetch_on(
                    None,
                    vec![entry("alice", "a", model::CiStatus::Failed)],
                    None,
                ),
            );
            assert!(!app.account_rebuild_due, "a keyless poll rebuilt");
            assert_eq!(row(app, "a").ci_status, model::CiStatus::Failed);
            assert_eq!(row(app, "a").pr_count, 0);
            let b = row(app, "b");
            let b_before = before.iter().find(|r| r.name == "b").unwrap();
            assert_eq!(b.ci_status, b_before.ci_status);
            assert_eq!(b.pr_count, b_before.pr_count);
            assert_eq!(b.repo_kind, b_before.repo_kind);
            assert_eq!(app.repos.len(), 2);
        }
        std::fs::remove_dir_all(&scratch).ok();
    }

    #[test]
    fn cv20_a_window_apply_keeps_the_account_so_a_poll_on_it_does_not_rebuild() {
        let scratch = wt_scratch("cv20");
        let store = window_state::WindowState::at(scratch.join("window.toml"));
        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        land_fetched(
            &mut app,
            LoadKind::Initial,
            Some("A"),
            alice_app().repos,
            vec![entry("alice", "a", model::CiStatus::Passed)],
            None,
            &store,
        );
        let w = time::TimeWindow::clamped(2, time::WindowUnit::Days);
        let notes = ViewNotes {
            header: "h".to_string(),
            ambiguity: None,
        };
        apply_load_result_saving_to(
            &mut app,
            &config::Config::default(),
            Ok(Box::new(window_apply_result(w, alice_app().repos, notes))),
            &store,
        );
        assert_eq!(app.github_meta.account.as_deref(), Some("A"));
        poll_with(
            &mut app,
            fetch_on(
                Some("A"),
                vec![entry("alice", "a", model::CiStatus::Passed)],
                None,
            ),
        );
        assert!(
            !app.account_rebuild_due,
            "a poll on the same account rebuilt"
        );
        std::fs::remove_dir_all(&scratch).ok();
    }

    /// The rebuild waits while a load runs: outside a runtime a spawn would
    /// panic, so returning at all says nothing was spawned.
    #[test]
    fn an_account_rebuild_waits_for_a_running_load() {
        let mut app = alice_app();
        app.loading = true;
        app.account_rebuild_due = true;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let args = Args::parse_from(["fleetz"]);
        start_account_rebuild(
            &mut app,
            &tx,
            &args,
            &config::Config::default(),
            Path::new("/nonexistent"),
        );
        assert!(app.account_rebuild_due, "the waiting rebuild was dropped");
    }

    /// A load that fetched rebuilt the rows on its own account, so a pending
    /// account rebuild is dropped rather than repeated.
    #[test]
    fn a_fetched_load_settles_a_pending_account_rebuild() {
        let scratch = wt_scratch("fz09d_settle");
        let store = window_state::WindowState::at(scratch.join("window.toml"));
        let mut app = alice_app();
        app.account_rebuild_due = true;
        land_fetched(
            &mut app,
            LoadKind::Reload,
            Some("B"),
            vec![],
            vec![],
            None,
            &store,
        );
        assert!(!app.account_rebuild_due);
        std::fs::remove_dir_all(&scratch).ok();
    }

    #[test]
    fn the_rebuild_rule_needs_a_known_poll_account() {
        assert!(!account_rebuild_needed(None, None));
        assert!(!account_rebuild_needed(Some("A"), None));
        assert!(!account_rebuild_needed(Some("A"), Some("A")));
        assert!(account_rebuild_needed(Some("A"), Some("B")));
        assert!(account_rebuild_needed(None, Some("A")));
    }

    #[test]
    fn names_remote_matches_case_insensitively_by_owner_or_by_name_alone() {
        let entry = github::GithubRepo {
            owner: Some("gaurgle".to_string()),
            ..gh("fleetz", "2026-01-01T00:00:00Z")
        };
        assert!(names_remote(Some("GAURGLE"), "Fleetz", &entry));
        assert!(!names_remote(Some("someone"), "fleetz", &entry));
        assert!(!names_remote(Some("gaurgle"), "other", &entry));
        // No owner: the legacy path, the name alone decides.
        assert!(names_remote(None, "FLEETZ", &entry));
        assert!(!names_remote(None, "other", &entry));
        // An owned row never matches an entry without an owner.
        let legacy = gh("fleetz", "2026-01-01T00:00:00Z");
        assert!(!names_remote(Some("gaurgle"), "fleetz", &legacy));
        assert!(names_remote(None, "Fleetz", &legacy));
    }

    // FZ-37: `y` and a click in pane 2 copy a commit hash.

    const YANK_HASHES: [&str; 3] = ["aaa1111", "bbb2222", "ccc3333"];

    /// A repo whose every view lists the three `YANK_HASHES` commits, plus
    /// `extra` more, newest first.
    fn yank_repo(extra: usize) -> RepoStatus {
        let mut repo = test_repo();
        repo.path = Some("/tmp/yank".to_string());
        let hashes: Vec<String> = YANK_HASHES
            .iter()
            .map(|h| h.to_string())
            .chain((0..extra).map(|i| format!("{:07x}", 0xd000000 + i)))
            .collect();
        repo.graph = hashes
            .iter()
            .map(|h| model::GraphLine {
                lanes: vec![graph::commit_cell()],
                hash: h.clone(),
                is_merge: false,
                refs: vec![],
                author: "Gaurgle".to_string(),
                message: format!("subject of {h}"),
            })
            .collect();
        repo.reflog = hashes
            .iter()
            .map(|h| model::ReflogEntry {
                hash: h.clone(),
                action: "commit:".to_string(),
                description: format!("subject of {h}"),
            })
            .collect();
        repo.graph_expanded = hashes
            .iter()
            .flat_map(|h| {
                [
                    format!("* commit {h}"),
                    "| Author: Gaurgle <g@example.com>".to_string(),
                    "|".to_string(),
                    format!("|     subject of {h}"),
                ]
            })
            .collect();
        repo
    }

    fn yank_app(view: app::HistoryView, extra: usize) -> App {
        let mut app = App::new(vec![yank_repo(extra)], "h".to_string(), 120, 40);
        app.view = app::View::Detail;
        app.history_view = view;
        app
    }

    /// Draw a frame, as the event loop does before any click can arrive.
    fn yank_frame(app: &App) -> ratatui::buffer::Buffer {
        use ratatui::backend::TestBackend;
        let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
        term.draw(|f| ui::render(f, app)).unwrap();
        term.backend().buffer().clone()
    }

    /// The first cell of `needle` inside the detail pane, read off the
    /// screen rather than out of the hit map under test.
    fn on_screen(buf: &ratatui::buffer::Buffer, app: &App, needle: &str) -> (u16, u16) {
        let pane = ui::panel_rects(buf.area, app).detail.expect("detail pane");
        (pane.top()..pane.bottom())
            .find_map(|y| {
                let row: String = (pane.left()..pane.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect();
                row.find(needle)
                    .map(|byte| (pane.left() + row[..byte].chars().count() as u16, y))
            })
            .unwrap_or_else(|| panic!("`{needle}` is not drawn in pane 2"))
    }

    fn yank_status(app: &App) -> Option<(String, bool)> {
        app.status_message
            .as_ref()
            .map(|m| (m.text.clone(), m.is_error))
    }

    #[test]
    fn yank_copies_the_inspected_hash() {
        let mut app = yank_app(app::HistoryView::Commits, 0);
        app.set_detail_selected(1);
        let mut copied = Vec::new();
        app.yank_with(|h| {
            copied.push(h.to_string());
            Ok(())
        });
        assert_eq!(copied, ["bbb2222"]);
        assert_eq!(
            yank_status(&app),
            Some(("copied bbb2222".to_string(), false))
        );
    }

    #[test]
    fn yank_follows_the_active_view() {
        let mut app = yank_app(app::HistoryView::Reflog, 0);
        app.set_detail_selected(2);
        let mut copied = String::new();
        app.yank_with(|h| {
            copied = h.to_string();
            Ok(())
        });
        assert_eq!(copied, "ccc3333");
    }

    #[test]
    fn yank_with_nothing_inspected_copies_nothing() {
        let mut app = App::new(vec![test_repo()], "h".to_string(), 120, 40);
        let mut called = false;
        app.yank_with(|_| {
            called = true;
            Ok(())
        });
        assert!(!called, "the clipboard was asked to copy nothing");
        assert_eq!(
            yank_status(&app),
            Some(("nothing to copy".to_string(), false))
        );
    }

    #[test]
    fn a_failed_copy_names_the_reason() {
        let mut app = yank_app(app::HistoryView::Commits, 0);
        app.yank_with(|_| Err("pbcopy timed out".to_string()));
        assert_eq!(
            yank_status(&app),
            Some(("copy failed: pbcopy timed out".to_string(), true))
        );
    }

    /// Click on `needle` in pane 2, offset `dx` columns from its first cell,
    /// and return what was copied.
    fn click_at(app: &mut App, needle: &str, dx: u16) -> Vec<String> {
        let buf = yank_frame(app);
        let (x, y) = on_screen(&buf, app, needle);
        let (x, y) = (x + dx, y);
        assert_eq!(
            ui::panel_rects(buf.area, app).at(x, y),
            Some(FocusedPanel::Detail)
        );
        let mut copied = Vec::new();
        click_detail(app, x, y, |h| {
            copied.push(h.to_string());
            Ok(())
        });
        copied
    }

    #[test]
    fn a_click_on_a_hash_moves_the_cursor_and_copies_it() {
        for view in [
            app::HistoryView::Info,
            app::HistoryView::Commits,
            app::HistoryView::Reflog,
            app::HistoryView::Graph,
        ] {
            for dx in [0, 6] {
                let mut app = yank_app(view, 0);
                let copied = click_at(&mut app, "bbb2222", dx);
                assert_eq!(copied, ["bbb2222"], "{view:?} at +{dx}");
                assert_eq!(app.inspected_hash(), Some("bbb2222"), "{view:?}");
                assert_eq!(
                    yank_status(&app),
                    Some(("copied bbb2222".to_string(), false)),
                    "{view:?}"
                );
            }
        }
    }

    #[test]
    fn a_click_on_the_row_off_the_hash_only_moves_the_cursor() {
        for view in [
            app::HistoryView::Info,
            app::HistoryView::Commits,
            app::HistoryView::Reflog,
        ] {
            let mut app = yank_app(view, 0);
            // The column just past the hash, and the subject further along.
            for (needle, dx) in [("bbb2222", 7), ("subject of ccc3333", 3)] {
                let copied = click_at(&mut app, needle, dx);
                assert!(copied.is_empty(), "{view:?} {needle}: copied {copied:?}");
                assert_eq!(app.status_message.as_ref().map(|m| m.text.as_str()), None);
            }
            assert_eq!(app.inspected_hash(), Some("ccc3333"), "{view:?}");
        }
        // The graph's cursor is a row: a click on a block's message line
        // moves the cursor into that block and copies nothing.
        let mut app = yank_app(app::HistoryView::Graph, 0);
        let copied = click_at(&mut app, "subject of ccc3333", 0);
        assert!(copied.is_empty());
        assert_eq!(app.inspected_hash(), Some("ccc3333"));
    }

    #[test]
    fn a_click_outside_any_row_does_nothing() {
        let mut app = yank_app(app::HistoryView::Commits, 0);
        app.set_detail_selected(1);
        let buf = yank_frame(&app);
        let pane = ui::panel_rects(buf.area, &app).detail.unwrap();
        let (_, last) = on_screen(&buf, &app, "ccc3333");
        let (x, header) = on_screen(&buf, &app, "/tmp/yank");
        let mut copied = Vec::new();
        // Below the last row, in the pinned header, and on the border.
        for (cx, cy) in [(x, last + 2), (x, header), (pane.left(), last)] {
            click_detail(&mut app, cx, cy, |h| {
                copied.push(h.to_string());
                Ok(())
            });
        }
        assert!(copied.is_empty(), "copied {copied:?}");
        assert_eq!(app.inspected_hash(), Some("bbb2222"), "the cursor moved");
        assert!(app.status_message.is_none());
    }

    /// The map is read in body rows, so a scrolled list resolves to the row
    /// drawn under the pointer, not to the row that many lines from the top.
    #[test]
    fn a_click_on_a_scrolled_list_lands_on_the_drawn_row() {
        let mut app = yank_app(app::HistoryView::Commits, 60);
        app.set_detail_selected(50);
        yank_frame(&app);
        assert!(app.detail_scroll() > 0, "the list did not scroll");
        let target = format!("{:07x}", 0xd000000 + 49);
        let copied = click_at(&mut app, &target, 0);
        assert_eq!(copied, std::slice::from_ref(&target));
        assert_eq!(app.inspected_hash(), Some(target.as_str()));
    }

    /// FZ-37 review: a click copies the hash that was on screen, even when
    /// the list changed between the paint and the click (a `RepoUpdated`
    /// landing in between).
    #[test]
    fn a_click_copies_the_painted_hash_after_the_list_changes() {
        let mut app = yank_app(app::HistoryView::Commits, 0);
        let buf = yank_frame(&app);
        let (x, y) = on_screen(&buf, &app, "bbb2222");
        // A new commit lands on top: index 1 now names `aaa1111`.
        let mut newer = app.repos[0].graph[0].clone();
        newer.hash = "fff0000".to_string();
        app.repos[0].graph.insert(0, newer);
        let mut copied = Vec::new();
        click_detail(&mut app, x, y, |h| {
            copied.push(h.to_string());
            Ok(())
        });
        assert_eq!(copied, ["bbb2222"]);
        assert_eq!(
            yank_status(&app),
            Some(("copied bbb2222".to_string(), false))
        );
    }

    // FZ-30b: the `ci` view's data layer, its trigger and its answers.

    fn ci_app() -> App {
        let mut app = App::new(Vec::new(), String::new(), 120, 40);
        app.repos = vec![RepoStatus {
            name: "fleetz".to_string(),
            owner: Some("Gaurgle".to_string()),
            ..test_repo()
        }];
        app.jitter = Box::new(Planted {
            factor: 1.0,
            resume: 7,
        });
        app
    }

    /// `Enter` opens the `notes` view's highlighted file while pane 2 or the
    /// inspector has focus, and lazygit everywhere else, a row with no file
    /// included. Fails while `Enter` only ever opens lazygit.
    #[test]
    fn enter_opens_the_highlighted_note_only_in_the_notes_view() {
        let mut app = App::new(Vec::new(), String::new(), 120, 40);
        app.repos = vec![RepoStatus {
            path: Some("/repo".to_string()),
            ..test_repo()
        }];
        app.view = app::View::Detail;
        app.history_view = app::HistoryView::Notes;
        app.focused_panel = FocusedPanel::Detail;
        assert_eq!(note_to_open(&app), None, "nothing walked yet");

        let walk = app::NotesWalk {
            key: "/repo".to_string(),
            root: "/repo".into(),
            generation: 0,
        };
        let files = vec![notes::NoteFile {
            rel: "notez/a.md".into(),
            notez: Some("notez".into()),
        }];
        app.apply_notes_walk(walk, Ok(notes::Notes { files }));
        let file = Some(std::path::PathBuf::from("/repo/notez/a.md"));
        assert_eq!(note_to_open(&app), file);
        app.focused_panel = FocusedPanel::Inspector;
        assert_eq!(note_to_open(&app), file);

        app.focused_panel = FocusedPanel::RepoList;
        assert_eq!(note_to_open(&app), None, "the repo list opens lazygit");
        app.focused_panel = FocusedPanel::Detail;
        app.history_view = app::HistoryView::Info;
        assert_eq!(note_to_open(&app), None, "another view opens lazygit");
    }

    /// `Enter` on the focused reader opens the file it shows (FZ-62).
    #[test]
    fn enter_on_the_reader_opens_its_note() {
        let mut app = App::new(Vec::new(), String::new(), 120, 40);
        app.repos = vec![RepoStatus {
            path: Some("/repo".to_string()),
            ..test_repo()
        }];
        app.view = app::View::Detail;
        app.history_view = app::HistoryView::Notes;
        let walk = app::NotesWalk {
            key: "/repo".to_string(),
            root: "/repo".into(),
            generation: 0,
        };
        let files = vec![notes::NoteFile {
            rel: "notez/a.md".into(),
            notez: Some("notez".into()),
        }];
        app.apply_notes_walk(walk, Ok(notes::Notes { files }));
        app.focus_panel(FocusedPanel::Reader);
        assert_eq!(app.focused_panel, FocusedPanel::Reader);
        assert_eq!(
            note_to_open(&app),
            Some(std::path::PathBuf::from("/repo/notez/a.md"))
        );
    }

    /// The next answer `spawn_note_read` sent, split into its two kinds.
    async fn next_note_answer(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ) -> Result<
        (app::NoteRead, Result<String, String>, notes::NoteFacts),
        (app::NoteRead, notes::LastCommit),
    > {
        match rx.recv().await {
            Some(AppEvent::NoteRead { read, text, facts }) => Ok((read, text, facts)),
            Some(AppEvent::NoteCommit { read, last }) => Err((read, last)),
            _ => panic!("not a note answer"),
        }
    }

    /// The card's facts are read off the UI thread (FZ-62): `spawn_note_read`
    /// returns before anything is sent. The body and its metadata come
    /// first and git's last commit after, so the reader never waits on git
    /// (review F4): a date for a committed file, "untracked" for one git
    /// never saw, and a name with glob characters read literally (review
    /// F5: `notez/*.md` would otherwise match the committed `plan.md`). A
    /// commit answer for a file no longer highlighted is dropped.
    #[tokio::test]
    async fn spawn_note_read_reads_the_body_and_facts_off_the_ui_thread() {
        let scratch = std::env::temp_dir().join("fleetz_test_note_read_seam");
        let _ = std::fs::remove_dir_all(&scratch);
        let repo = hist_repo(&scratch, "repo", "n@example.com");
        std::fs::create_dir_all(repo.join("notez")).unwrap();
        std::fs::create_dir_all(repo.join(".notez")).unwrap();
        std::fs::write(repo.join("notez/plan.md"), "# Plan\n").unwrap();
        std::fs::write(repo.join("notez/*.md"), "star\n").unwrap();
        std::fs::write(repo.join(".notez/scratch.md"), "scratch\n").unwrap();
        wt_sh(&repo, &["git", "add", "notez/plan.md"]);
        hist_commit(&repo, "2026-10-01", "plan");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let answer = |rel: &str| app::NoteRead {
            path: repo.join(rel),
            root: repo.clone(),
            generation: 0,
        };
        spawn_note_read(&tx, answer("notez/plan.md"));
        // A current-thread test runtime has not run the task yet: had the
        // read run inline, its answer would already be here.
        assert!(rx.try_recv().is_err(), "read on the calling thread");
        let Ok((read, text, facts)) = next_note_answer(&mut rx).await else {
            panic!("the body comes first");
        };
        assert_eq!(read.path, repo.join("notez/plan.md"));
        assert_eq!(text, Ok("# Plan\n".to_string()));
        assert_eq!(facts.size, Some(7));
        assert!(facts.modified.is_some());
        let Err((read, last)) = next_note_answer(&mut rx).await else {
            panic!("then the commit");
        };
        assert_eq!(read.path, repo.join("notez/plan.md"));
        let notes::LastCommit::At(date) = last else {
            panic!("committed: {last:?}");
        };
        assert!(date.starts_with("2026-10-01T12:00:00"), "{date}");

        for (rel, size) in [(".notez/scratch.md", 8), ("notez/*.md", 5)] {
            spawn_note_read(&tx, answer(rel));
            let Ok((_, _, facts)) = next_note_answer(&mut rx).await else {
                panic!("{rel}: the body comes first");
            };
            assert_eq!(facts.size, Some(size), "{rel}");
            let Err((_, last)) = next_note_answer(&mut rx).await else {
                panic!("{rel}: then the commit");
            };
            assert_eq!(last, notes::LastCommit::Untracked, "{rel}");
        }

        // Landed in the app: the commit of a file the cursor has left is
        // dropped, and the highlighted file's lands.
        let key = repo.to_string_lossy().into_owned();
        let mut app = App::new(Vec::new(), String::new(), 120, 40);
        app.repos = vec![RepoStatus {
            path: Some(key.clone()),
            ..test_repo()
        }];
        app.view = app::View::Detail;
        app.history_view = app::HistoryView::Notes;
        let walk = app::NotesWalk {
            key: key.clone(),
            root: repo.clone(),
            generation: 0,
        };
        let files = ["notez/plan.md", "notez/zz.md"]
            .map(|rel| notes::NoteFile {
                rel: rel.into(),
                notez: Some("notez".into()),
            })
            .to_vec();
        app.apply_notes_walk(walk, Ok(notes::Notes { files }));
        let plan = app.claim_note_read().expect("plan");
        spawn_note_read(&tx, plan.clone());
        app.select_commit_down();
        let other = app.claim_note_read().expect("the other file");
        let Ok((read, text, facts)) = next_note_answer(&mut rx).await else {
            panic!("body");
        };
        app.apply_note_facts(&read, facts);
        app.apply_note_read(read, text);
        let Err((read, last)) = next_note_answer(&mut rx).await else {
            panic!("commit");
        };
        app.apply_note_commit(&read, last);
        let held = app.note_body.as_ref().expect("held");
        assert_eq!(held.path, other.path);
        assert_eq!(
            (&held.text, &held.facts, &held.last_commit),
            (&None, &None, &None),
            "nothing of the file the cursor left"
        );
        app.apply_note_commit(&other, notes::LastCommit::Untracked);
        assert_eq!(
            app.note_body.as_ref().and_then(|b| b.last_commit.clone()),
            Some(notes::LastCommit::Untracked)
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    fn ci_key() -> app::RemoteKey {
        (Some("gaurgle".to_string()), "fleetz".to_string())
    }

    fn ci_runs() -> Vec<ci::WorkflowRun> {
        ci::parse_runs(include_str!("ci/fixtures/runs.json")).unwrap()
    }

    fn ci_answer(request: &ci::CiRequest) -> ci::CiAnswer {
        ci::CiAnswer {
            key: request.key.clone(),
            attempt: request.attempt,
            runs: None,
            jobs: None,
            log: None,
            full_log: None,
            failure: None,
            budget: None,
        }
    }

    fn fresh_runs(etag: &str) -> Option<ci::Fetched<Vec<ci::WorkflowRun>>> {
        Some(ci::Fetched::Fresh {
            data: ci_runs(),
            etag: Some(etag.to_string()),
        })
    }

    /// Claim and land one successful list poll at `now`.
    fn ci_landed(app: &mut App, now: chrono::DateTime<chrono::Local>) {
        let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
        let answer = ci::CiAnswer {
            runs: fresh_runs("W/\"6b13\""),
            ..ci_answer(&request)
        };
        app.apply_ci(answer, now);
    }

    #[test]
    fn a_ci_target_needs_an_owner() {
        let legacy = RepoStatus {
            name: "fleetz".to_string(),
            ..test_repo()
        };
        assert_eq!(ci::target(&legacy), None, "known by name only: no URL");
        let (key, owner, name) = ci::target(&ci_app().repos[0]).unwrap();
        assert_eq!(key, ci_key());
        assert_eq!((owner.as_str(), name.as_str()), ("gaurgle", "fleetz"));
    }

    #[test]
    fn the_ci_poll_waits_for_the_ci_view() {
        let mut app = ci_app();
        assert!(!app.ci_wanted(), "pane 2 opens on info");
        let start = std::time::Instant::now();
        for ms in [0, 300, 60_000] {
            let instant = start + std::time::Duration::from_millis(ms);
            assert_eq!(ci_poll_request(&mut app, at(12, 0, 0), instant), None);
        }
        assert!(app.ci.is_empty());
    }

    #[test]
    fn the_ci_view_polls_the_selected_repo_once_the_cursor_dwells() {
        let mut app = ci_app();
        app.history_view = app::HistoryView::Ci;
        app.view = app::View::Detail;
        let start = std::time::Instant::now();
        let after = |ms| start + std::time::Duration::from_millis(ms);
        assert_eq!(ci_poll_request(&mut app, at(12, 0, 0), after(0)), None);
        assert_eq!(ci_poll_request(&mut app, at(12, 0, 0), after(299)), None);
        let request = ci_poll_request(&mut app, at(12, 0, 0), after(300)).expect("settled");
        assert_eq!(request.key, ci_key());
        assert_eq!(request.runs_etag, None);
        assert_eq!(
            ci_poll_request(&mut app, at(12, 0, 1), after(1300)),
            None,
            "one poll out at a time"
        );
    }

    #[test]
    fn a_row_without_a_github_identity_sends_no_ci_poll() {
        let mut app = ci_app();
        app.history_view = app::HistoryView::Ci;
        app.view = app::View::Detail;
        app.repos[0].owner = None;
        let start = std::time::Instant::now();
        ci_poll_request(&mut app, at(12, 0, 0), start);
        let later = start + std::time::Duration::from_secs(5);
        assert_eq!(ci_poll_request(&mut app, at(12, 0, 5), later), None);
    }

    #[test]
    fn the_ci_poll_follows_the_cadence_of_the_shown_runs() {
        let mut app = ci_app();
        app.history_view = app::HistoryView::Ci;
        app.view = app::View::Detail;
        ci_landed(&mut app, at(12, 0, 0));
        let start = std::time::Instant::now();
        let settled = start + std::time::Duration::from_secs(1);
        ci_poll_request(&mut app, at(12, 0, 0), start);
        // The fixture's newest run is in progress: 10 s.
        assert_eq!(ci_poll_request(&mut app, at(12, 0, 9), settled), None);
        let request = ci_poll_request(&mut app, at(12, 0, 10), settled).expect("10 s on");
        assert_eq!(request.runs_etag.as_deref(), Some("W/\"6b13\""));
        // Every shown run finished: 60 s.
        let mut finished = ci_runs();
        finished.retain(|r| !r.is_live());
        let answer = ci::CiAnswer {
            runs: Some(ci::Fetched::Fresh {
                data: finished,
                etag: None,
            }),
            ..ci_answer(&request)
        };
        app.apply_ci(answer, at(12, 0, 10));
        assert_eq!(ci_poll_request(&mut app, at(12, 1, 9), settled), None);
        assert!(ci_poll_request(&mut app, at(12, 1, 10), settled).is_some());
    }

    #[test]
    fn a_304_keeps_the_runs_and_is_a_successful_attempt() {
        let mut app = ci_app();
        ci_landed(&mut app, at(12, 0, 0));
        let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
        assert_eq!(
            request.runs_etag.as_deref(),
            Some("W/\"6b13\""),
            "sent back"
        );
        let answer = ci::CiAnswer {
            runs: Some(ci::Fetched::NotModified),
            ..ci_answer(&request)
        };
        app.apply_ci(answer, at(12, 0, 10));
        let state = &app.ci[&ci_key()];
        assert_eq!(state.runs, ci_runs(), "the data is kept");
        assert_eq!(state.runs_etag.as_deref(), Some("W/\"6b13\""));
        assert_eq!(state.freshness.last_attempt, Some(at(12, 0, 10)));
        assert_eq!(state.freshness.data_time, app::DataTime::At(at(12, 0, 10)));
        assert_eq!(state.freshness.outcome, app::Outcome::Success);
        assert!(!state.inflight);
    }

    /// What a CI answer must never move: the rows, and the fleet poll's
    /// freshness, budget, backoff, lock, dataset and coverage.
    fn fleet_snapshot(app: &App) -> String {
        format!(
            "{:?}|{:?}|{:?}|{:?}|{}|{}|{:?}|{:?}",
            app.repos,
            app.github_freshness,
            app.api_budget,
            app.github_backoff,
            app.github_poll_inflight,
            app.dataset.len(),
            app.github_meta,
            app.not_in_last_poll,
        )
    }

    fn ci_failure_lands(failure: github::Failure, budget: Option<github::BudgetReading>) -> App {
        let mut app = ci_app();
        ci_landed(&mut app, at(12, 0, 0));
        let before = fleet_snapshot(&app);
        let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
        let answer = ci::CiAnswer {
            failure: Some(failure.clone()),
            budget,
            ..ci_answer(&request)
        };
        app.apply_ci(answer, at(12, 0, 10));
        assert_eq!(fleet_snapshot(&app), before, "{failure:?} moved the fleet");
        let state = &app.ci[&ci_key()];
        assert_eq!(state.runs, ci_runs(), "a failure keeps the data");
        assert_eq!(state.freshness.data_time, app::DataTime::At(at(12, 0, 0)));
        assert_eq!(state.freshness.last_attempt, Some(at(12, 0, 10)));
        assert_eq!(state.freshness.outcome, app::Outcome::Failed(failure));
        assert!(!state.inflight);
        app
    }

    #[test]
    fn an_offline_ci_poll_keeps_the_runs_and_leaves_the_fleet_alone() {
        let failure = github::Failure::new(github::FailureKind::Offline, "no route");
        let app = ci_failure_lands(failure, None);
        assert_eq!(
            app.ci[&ci_key()].backoff.not_before(60),
            None,
            "offline counts nothing"
        );
    }

    #[test]
    fn a_rate_limited_ci_poll_holds_until_the_reset_and_leaves_the_fleet_alone() {
        let reset = at(12, 40, 0);
        let budget = reading("tok", 0, reset, at(12, 0, 10));
        let failure = github::Failure::rate_limit("API rate limit exceeded");
        let mut app = ci_failure_lands(failure, Some(budget));
        let resume = reset + secs(7);
        assert_eq!(app.ci[&ci_key()].backoff.limit_until(60), Some(resume));
        assert_eq!(app.ci_budget.known(at(12, 0, 10)).unwrap().remaining, 0);
        app.history_view = app::HistoryView::Ci;
        app.view = app::View::Detail;
        let start = std::time::Instant::now();
        let settled = start + std::time::Duration::from_secs(1);
        ci_poll_request(&mut app, at(12, 0, 10), start);
        assert_eq!(ci_poll_request(&mut app, at(12, 39, 0), settled), None);
        assert!(ci_poll_request(&mut app, resume, settled).is_some());
    }

    #[test]
    fn a_404_on_a_gone_run_marks_it_until_the_next_list_drops_it() {
        let mut app = ci_app();
        ci_landed(&mut app, at(12, 0, 0));
        let gone = ci_runs()[2].id;
        app.ci.get_mut(&ci_key()).unwrap().expanded = Some(gone);
        let before = fleet_snapshot(&app);
        let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
        assert_eq!(request.expanded.as_ref().map(|e| e.run_id), Some(gone));
        let answer = ci::CiAnswer {
            runs: Some(ci::Fetched::NotModified),
            jobs: Some((gone, ci::JobsAnswer::Gone)),
            ..ci_answer(&request)
        };
        app.apply_ci(answer, at(12, 0, 10));
        assert_eq!(fleet_snapshot(&app), before);
        let state = &app.ci[&ci_key()];
        assert!(state.gone.contains(&gone), "the pane can say `run gone`");
        assert_eq!(state.runs.len(), 3, "until the next list");
        assert_eq!(state.freshness.outcome, app::Outcome::Success);

        let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
        let answer = ci::CiAnswer {
            runs: fresh_runs("W/\"next\""),
            ..ci_answer(&request)
        };
        app.apply_ci(answer, at(12, 0, 20));
        let state = &app.ci[&ci_key()];
        assert!(state.runs.iter().all(|r| r.id != gone), "dropped");
        assert!(state.gone.is_empty());
        assert_eq!(state.expanded, None, "nothing left to expand");
    }

    #[test]
    fn a_late_ci_answer_does_not_move_the_data_backwards() {
        let mut app = ci_app();
        let first = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
        let second = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
        let newer = ci::CiAnswer {
            runs: fresh_runs("W/\"new\""),
            ..ci_answer(&second)
        };
        app.apply_ci(newer, at(12, 0, 10));
        let older = ci::CiAnswer {
            runs: Some(ci::Fetched::Fresh {
                data: Vec::new(),
                etag: Some("W/\"old\"".to_string()),
            }),
            ..ci_answer(&first)
        };
        app.apply_ci(older, at(12, 0, 20));
        let state = &app.ci[&ci_key()];
        assert_eq!(state.runs_etag.as_deref(), Some("W/\"new\""));
        assert_eq!(state.runs.len(), 3);
        assert_eq!(state.freshness.last_attempt, Some(at(12, 0, 10)));
    }

    #[test]
    fn a_failed_log_is_read_once_per_job() {
        let mut app = ci_app();
        ci_landed(&mut app, at(12, 0, 0));
        let run = ci_runs()[2].id;
        app.ci.get_mut(&ci_key()).unwrap().expanded = Some(run);
        let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
        let jobs = ci::parse_jobs(include_str!("ci/fixtures/jobs-failed.json")).unwrap();
        let answer = ci::CiAnswer {
            runs: Some(ci::Fetched::NotModified),
            jobs: Some((
                run,
                ci::JobsAnswer::Fetched(ci::Fetched::Fresh {
                    data: jobs.clone(),
                    etag: Some("W/\"jobs\"".to_string()),
                }),
            )),
            log: Some((jobs[0].id, ci::LogTail::Lines(vec!["boom".to_string()]))),
            ..ci_answer(&request)
        };
        app.apply_ci(answer, at(12, 0, 10));
        let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
        let expanded = request.expanded.expect("the run is still expanded");
        assert_eq!(expanded.jobs_etag.as_deref(), Some("W/\"jobs\""));
        assert_eq!(expanded.held_jobs, jobs);
        assert_eq!(expanded.logs_held, vec![jobs[0].id]);
    }

    // FZ-30c: the follow-ups to #90, kept by the `ci` view.

    /// One repo's failures climb that repo's ladder only: a second repo the
    /// cursor moves to polls as soon as it dwells. Fails while the backoff
    /// is one for the whole session.
    #[test]
    fn one_repos_ci_failure_does_not_hold_another_repos_first_poll() {
        let mut app = ci_app();
        app.repos.push(RepoStatus {
            name: "repoz".to_string(),
            owner: Some("Gaurgle".to_string()),
            ..test_repo()
        });
        app.history_view = app::HistoryView::Ci;
        app.view = app::View::Detail;
        let server = github::Failure::new(github::FailureKind::Server, "502");
        for n in 0..3 {
            let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
            let answer = ci::CiAnswer {
                failure: Some(server.clone()),
                ..ci_answer(&request)
            };
            app.apply_ci(answer, at(12, 0, n));
        }
        assert!(app.ci[&ci_key()].backoff.not_before(60).is_some());
        let other = app
            .visible_repos()
            .iter()
            .position(|(_, r)| r.name == "repoz")
            .unwrap();
        app.selected = other;
        let start = std::time::Instant::now();
        ci_poll_request(&mut app, at(12, 0, 3), start);
        let settled = start + std::time::Duration::from_secs(1);
        let request = ci_poll_request(&mut app, at(12, 0, 3), settled).expect("not held");
        assert_eq!(request.name, "repoz");
    }

    /// A CI poll that never answers is given up on, as the fleet poll is:
    /// the lock is released and the attempt is a timed-out failure on that
    /// repo. Fails while `inflight` can stay set for the session.
    #[test]
    fn a_stuck_ci_poll_is_given_up_on() {
        let mut app = ci_app();
        ci_landed(&mut app, at(12, 0, 0));
        let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
        let claimed = std::time::Instant::now();
        app.expire_stuck_ci_polls(CI_POLL_STUCK_AFTER, claimed, at(12, 0, 30));
        assert!(app.ci[&ci_key()].inflight, "not yet");
        let later = claimed + CI_POLL_STUCK_AFTER + std::time::Duration::from_secs(1);
        app.expire_stuck_ci_polls(CI_POLL_STUCK_AFTER, later, at(12, 2, 1));
        let state = &app.ci[&ci_key()];
        assert!(!state.inflight);
        assert_eq!(state.started, None);
        assert!(matches!(
            &state.freshness.outcome,
            app::Outcome::Failed(f) if f.kind == github::FailureKind::TimedOut
        ));
        assert_eq!(state.freshness.last_attempt, Some(at(12, 2, 1)));
        assert_eq!(state.runs, ci_runs(), "the data is kept");
        assert!(
            state.backoff.not_before(60).is_some(),
            "it climbs the ladder"
        );
        // An answer that still arrives is applied like any.
        let answer = ci::CiAnswer {
            runs: Some(ci::Fetched::NotModified),
            ..ci_answer(&request)
        };
        app.apply_ci(answer, at(12, 2, 5));
        assert_eq!(app.ci[&ci_key()].freshness.outcome, app::Outcome::Success);
    }

    /// The list answered and a later call failed: the runs on screen are
    /// that current, so the data time advances while the outcome is the
    /// failure. Fails while a failed attempt never moves the data time.
    #[test]
    fn a_fresh_list_advances_the_data_time_when_a_later_call_fails() {
        let mut app = ci_app();
        ci_landed(&mut app, at(12, 0, 0));
        app.ci.get_mut(&ci_key()).unwrap().expanded = Some(ci_runs()[2].id);
        let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
        let offline = github::Failure::new(github::FailureKind::Offline, "jobs: no route");
        let answer = ci::CiAnswer {
            runs: fresh_runs("W/\"next\""),
            failure: Some(offline.clone()),
            ..ci_answer(&request)
        };
        app.apply_ci(answer, at(12, 0, 10));
        let state = &app.ci[&ci_key()];
        assert_eq!(state.freshness.data_time, app::DataTime::At(at(12, 0, 10)));
        assert_eq!(state.freshness.outcome, app::Outcome::Failed(offline));
        assert_eq!(state.runs_etag.as_deref(), Some("W/\"next\""));
    }

    /// An answer landing is something to draw. Fails while `AppEvent::Ci`
    /// waits for the next tick's repaint.
    #[test]
    fn a_ci_answer_asks_for_a_redraw() {
        let mut app = ci_app();
        let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
        app.needs_redraw = false;
        app.apply_ci(
            ci::CiAnswer {
                runs: fresh_runs("W/\"6b13\""),
                ..ci_answer(&request)
            },
            at(12, 0, 0),
        );
        assert!(app.needs_redraw);
    }

    // FZ-30d: the `ci` view in pane 2 is the only thing that polls.

    /// `]` from `branches` reaches `ci`, and there `j`/`k` walk its runs in
    /// the view's own selection slot, never the repo list. Fails while `ci`
    /// is not a pane 2 view.
    #[tokio::test]
    async fn j_and_k_walk_the_runs_in_the_ci_view() {
        let args = Args::parse_from(["fleetz"]);
        let config = config::Config::default();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let backend = CrosstermBackend::new(io::stdout());
        let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 120, 44));
        let mut terminal =
            Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap();
        let mut app = ci_app();
        app.view = app::View::Detail;
        app.history_view = app::HistoryView::Branches;
        ci_landed(&mut app, at(12, 0, 0));
        app.focused_panel = FocusedPanel::Detail;
        let repo_dir = Path::new("/nonexistent");
        handle_normal_input(
            &mut app,
            &mut terminal,
            press(']'),
            &tx,
            &args,
            &config,
            repo_dir,
        )
        .await
        .unwrap();
        assert_eq!(app.history_view, app::HistoryView::Ci);
        assert!(app.ci_wanted());
        let runs = app.ci[&ci_key()].runs.len();
        assert!(runs >= 3, "fixture has {runs} runs");
        for (key, cursor) in [('j', 1), ('j', 2), ('k', 1)] {
            handle_normal_input(
                &mut app,
                &mut terminal,
                press(key),
                &tx,
                &args,
                &config,
                repo_dir,
            )
            .await
            .unwrap();
            assert_eq!(app.detail_selected(), cursor, "after {key}");
        }
        assert_eq!(app.selected, 0, "the repo list did not move");
    }

    /// FZ-30f: on the focused inspector in `ci`, `]` steps the body and `j`
    /// moves the job mark; pane 2's view and the run cursor stay put.
    #[tokio::test]
    async fn the_inspector_keys_step_the_bodies_and_the_job_mark() {
        let args = Args::parse_from(["fleetz"]);
        let config = config::Config::default();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let backend = CrosstermBackend::new(io::stdout());
        let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 120, 44));
        let mut terminal =
            Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap();
        let mut app = ci_app();
        app.view = app::View::Detail;
        app.history_view = app::HistoryView::Ci;
        ci_landed(&mut app, at(12, 0, 0));
        let run = ci_runs()[2].id;
        let mut jobs = ci::parse_jobs(include_str!("ci/fixtures/jobs-failed.json")).unwrap();
        let mut second = jobs[0].clone();
        second.id = 7;
        jobs.push(second);
        app.ci
            .get_mut(&ci_key())
            .unwrap()
            .jobs
            .insert(run, app::CiJobs { etag: None, jobs });
        app.set_detail_selected(2);
        app.focused_panel = FocusedPanel::Inspector;
        let repo_dir = Path::new("/nonexistent");
        for key in ['j', ']'] {
            handle_normal_input(
                &mut app,
                &mut terminal,
                press(key),
                &tx,
                &args,
                &config,
                repo_dir,
            )
            .await
            .unwrap();
        }
        assert_eq!(
            app.ci_marked_job().map(|j| j.id),
            Some(7),
            "j moved the mark"
        );
        assert_eq!(app.ci_body, app::CiBody::Log, "] stepped the body");
        assert_eq!(app.history_view, app::HistoryView::Ci, "not the view");
        assert_eq!(app.detail_selected(), 2, "not the run");
    }

    /// FZ-30f fix cycle 3: `]` from the focused inspector in `branches`
    /// enters `ci` with its runs held and the next `]` goes on to `info`;
    /// back in `ci`, any other key ends the streak and `]` steps the bodies.
    /// Fails while the second `]` sticks on the bodies.
    #[tokio::test]
    async fn a_bracket_streak_cycles_views_through_ci_until_another_key() {
        let args = Args::parse_from(["fleetz"]);
        let config = config::Config::default();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let backend = CrosstermBackend::new(io::stdout());
        let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 120, 44));
        let mut terminal =
            Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap();
        let mut app = ci_app();
        app.view = app::View::Detail;
        app.history_view = app::HistoryView::Ci;
        ci_landed(&mut app, at(12, 0, 0));
        app.history_view = app::HistoryView::Branches;
        app.focused_panel = FocusedPanel::Inspector;
        let repo_dir = Path::new("/nonexistent");
        let mut views = Vec::new();
        for key in [']', ']', '[', 'j', ']'] {
            handle_normal_input(
                &mut app,
                &mut terminal,
                press(key),
                &tx,
                &args,
                &config,
                repo_dir,
            )
            .await
            .unwrap();
            views.push((app.history_view, app.ci_body));
        }
        use app::{CiBody, HistoryView};
        assert_eq!(
            views,
            vec![
                (HistoryView::Ci, CiBody::Jobs),
                (HistoryView::Notes, CiBody::Jobs),
                (HistoryView::Ci, CiBody::Jobs),
                (HistoryView::Ci, CiBody::Jobs),
                (HistoryView::Ci, CiBody::Log),
            ]
        );
    }

    /// The tick polls only while pane 2 shows `ci`: never on `info` (whose
    /// `ci` section draws what is held), never in Compact, and switching the
    /// view away stops it. The held runs survive the switch. Fails while the
    /// trigger ignores the view.
    #[test]
    fn the_ci_tick_polls_only_while_pane_2_shows_ci() {
        let mut app = ci_app();
        app.view = app::View::Detail;
        let start = std::time::Instant::now();
        let after = |s| start + std::time::Duration::from_secs(s);
        for view in app::HistoryView::ALL {
            if view == app::HistoryView::Ci {
                continue;
            }
            app.history_view = view;
            ci_tick(&mut app, at(12, 0, 0), start);
            assert_eq!(ci_tick(&mut app, at(12, 0, 1), after(1)), None, "{view:?}");
        }
        assert!(app.ci.is_empty(), "nothing was claimed outside `ci`");

        app.history_view = app::HistoryView::Ci;
        app.view = app::View::Compact;
        assert_eq!(ci_tick(&mut app, at(12, 0, 2), after(2)), None, "compact");
        app.view = app::View::Detail;
        // The cursor's dwell starts on the first tick that wants CI.
        assert_eq!(ci_tick(&mut app, at(12, 0, 3), after(3)), None, "dwell");
        let request = ci_tick(&mut app, at(12, 0, 4), after(4)).expect("ci polls");
        let answer = ci::CiAnswer {
            runs: fresh_runs("W/\"6b13\""),
            ..ci_answer(&request)
        };
        app.apply_ci(answer, at(12, 0, 5));

        app.history_view = app::HistoryView::Info;
        assert_eq!(
            ci_tick(&mut app, at(12, 5, 0), after(300)),
            None,
            "info stops it"
        );
        assert_eq!(app.ci[&ci_key()].runs, ci_runs(), "held runs survive");
        app.history_view = app::HistoryView::Ci;
        assert!(
            ci_tick(&mut app, at(12, 5, 0), after(300)).is_some(),
            "back in `ci`, the overdue poll goes at once"
        );
    }

    /// A stuck CI poll is given up on by the tick only while `ci` is on
    /// screen, like the trigger; back in the view the expiry runs first, so
    /// the lock never holds the view's next poll.
    #[test]
    fn the_ci_tick_expires_a_stuck_poll_only_in_the_ci_view() {
        let mut app = ci_app();
        app.view = app::View::Detail;
        app.history_view = app::HistoryView::Info;
        app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
        let late = std::time::Instant::now() + CI_POLL_STUCK_AFTER * 2;
        ci_tick(&mut app, at(12, 5, 0), late);
        assert!(app.ci[&ci_key()].inflight, "info leaves the CI layer alone");
        app.history_view = app::HistoryView::Ci;
        ci_tick(&mut app, at(12, 5, 0), late);
        assert!(
            app.ci[&ci_key()].freshness.last_attempt.is_some(),
            "expired as a failed attempt"
        );
        assert!(matches!(
            &app.ci[&ci_key()].freshness.outcome,
            app::Outcome::Failed(f) if f.kind == github::FailureKind::TimedOut
        ));
    }

    // FZ-30e: the `ci` view's handoffs to `gh` and the browser.

    /// `ci_app` with its runs landed, pane 2 focused on `ci`, the cursor on
    /// run `index` of the fixture (in progress, success, failure).
    fn handoff_app(index: usize) -> App {
        let mut app = ci_app();
        ci_landed(&mut app, at(12, 0, 0));
        app.view = app::View::Detail;
        app.history_view = app::HistoryView::Ci;
        app.focused_panel = FocusedPanel::Detail;
        app.detail_selections[app::HistoryView::Ci.index()] = index;
        app
    }

    fn argv(args: &[&str]) -> RunHandoff {
        RunHandoff::Gh(args.iter().map(|a| a.to_string()).collect())
    }

    #[test]
    fn the_highlighted_run_is_the_one_the_ci_cursor_is_on() {
        let app = handoff_app(1);
        assert_eq!(app.highlighted_ci_run(), Some(&ci_runs()[1]));
        let past = handoff_app(9);
        assert_eq!(
            past.highlighted_ci_run(),
            Some(&ci_runs()[2]),
            "clamped onto the last run, as the view draws the cursor"
        );
        assert_eq!(ci_app().highlighted_ci_run(), None, "nothing held");
    }

    /// `l` stays in fleetz: a finished run, failed or not, is shown in the
    /// inspector, and no `gh run view --log` is built. Fails while `l`
    /// hands the terminal to `gh`.
    #[test]
    fn l_shows_a_finished_run_in_the_inspector_with_no_gh_command() {
        for (index, conclusion) in [(1, "success"), (2, "failure")] {
            let mut app = handoff_app(index);
            let (handoff, target) = resolve_run_handoff(&mut app, RunKey::Log).unwrap();
            assert_eq!(handoff, RunHandoff::Inspect, "{conclusion}");
            assert_eq!(target, (ci_key(), "gaurgle".into(), "fleetz".into()));
            assert_eq!(yank_status(&app), None, "{conclusion}");
        }
    }

    /// The inspector opens if folded, takes focus and shows the `log`
    /// body of the run under the cursor, whose failed job is the marked one.
    #[test]
    fn l_focuses_the_inspector_on_the_runs_log_body() {
        let mut app = handoff_app(2);
        app.inspector_folded = true;
        app.inspect_ci_log();
        assert_eq!(app.focused_panel, FocusedPanel::Inspector);
        assert!(!app.inspector_folded, "a folded inspector opens");
        assert_eq!(app.ci_body, app::CiBody::Log);
        assert!(app.inspects_ci_run());
        assert_eq!(
            app.ci_inspected.map(|(id, _)| id),
            Some(ci_runs()[2].id),
            "the highlighted run"
        );
        assert_eq!(
            app.selected_ci()
                .and_then(|(_, s)| s)
                .and_then(|s| s.expanded),
            Some(ci_runs()[2].id),
            "the poll reads the run's jobs now the inspector is open"
        );
        assert!(app.needs_redraw);
    }

    #[test]
    fn a_reruns_a_finished_run_and_only_the_failed_jobs_of_a_failed_one() {
        let runs = ci_runs();
        assert_eq!(
            run_handoff_for(&runs[1], RunKey::Rerun, "gaurgle", "fleetz"),
            Ok(argv(&[
                "run",
                "rerun",
                "39590683935",
                "-R",
                "gaurgle/fleetz"
            ]))
        );
        assert_eq!(
            run_handoff_for(&runs[2], RunKey::Rerun, "gaurgle", "fleetz"),
            Ok(argv(&[
                "run",
                "rerun",
                "37958308363",
                "--failed",
                "-R",
                "gaurgle/fleetz"
            ]))
        );
    }

    /// Only `failure` means failed: a run that timed out or was cancelled
    /// gets a whole rerun.
    #[test]
    fn a_timed_out_or_cancelled_run_gets_a_whole_rerun() {
        for conclusion in ["timed_out", "cancelled"] {
            let run = ci::WorkflowRun {
                conclusion: Some(conclusion.to_string()),
                ..ci_runs()[1].clone()
            };
            assert_eq!(
                run_handoff_for(&run, RunKey::Rerun, "gaurgle", "fleetz"),
                Ok(argv(&[
                    "run",
                    "rerun",
                    "39590683935",
                    "-R",
                    "gaurgle/fleetz"
                ])),
                "{conclusion}"
            );
        }
    }

    #[test]
    fn x_cancels_only_a_run_in_progress_or_queued() {
        let runs = ci_runs();
        assert_eq!(
            run_handoff_for(&runs[0], RunKey::Cancel, "gaurgle", "fleetz"),
            Ok(argv(&[
                "run",
                "cancel",
                "39590684061",
                "-R",
                "gaurgle/fleetz"
            ]))
        );
        let queued = ci::WorkflowRun {
            status: Some("queued".to_string()),
            ..runs[0].clone()
        };
        assert_eq!(
            run_handoff_for(&queued, RunKey::Cancel, "gaurgle", "fleetz"),
            Ok(argv(&[
                "run",
                "cancel",
                "39590684061",
                "-R",
                "gaurgle/fleetz"
            ]))
        );
        for finished in [&runs[1], &runs[2]] {
            assert_eq!(
                run_handoff_for(finished, RunKey::Cancel, "gaurgle", "fleetz"),
                Err("run is not in progress")
            );
        }
    }

    /// `gh` has no log for a run still going and will not rerun one, so
    /// fleetz says so rather than handing over a command that only fails.
    #[test]
    fn a_run_still_going_has_no_log_or_rerun_yet() {
        let live = &ci_runs()[0];
        for key in [RunKey::Log, RunKey::Rerun] {
            assert_eq!(
                run_handoff_for(live, key, "gaurgle", "fleetz"),
                Err("run has not finished"),
                "{key:?}"
            );
        }
    }

    #[test]
    fn capital_o_opens_the_run_page_whatever_its_state() {
        for run in ci_runs() {
            assert_eq!(
                run_handoff_for(&run, RunKey::Open, "gaurgle", "fleetz"),
                Ok(RunHandoff::Open(run.url.clone()))
            );
        }
    }

    #[test]
    fn the_handoff_keys_are_l_a_x_and_capital_o() {
        let keys: Vec<Option<RunKey>> = ['l', 'a', 'x', 'O', 'o', 'L', 'X']
            .into_iter()
            .map(|c| RunKey::from_code(KeyCode::Char(c)))
            .collect();
        assert_eq!(
            keys,
            [
                Some(RunKey::Log),
                Some(RunKey::Rerun),
                Some(RunKey::Cancel),
                Some(RunKey::Open),
                None,
                None,
                None
            ]
        );
    }

    /// Only pane 2, focused, showing `ci`: anywhere else the keys keep
    /// their old meaning (`O` the first PR, `l` the strip's day cursor).
    #[test]
    fn the_handoffs_apply_only_with_pane_2_focused_on_ci() {
        assert!(ci_handoff_applies(&handoff_app(0)));
        for view in app::HistoryView::ALL {
            if view == app::HistoryView::Ci {
                continue;
            }
            let mut app = handoff_app(0);
            app.history_view = view;
            assert!(!ci_handoff_applies(&app), "{view:?}");
        }
        for panel in [
            FocusedPanel::RepoList,
            FocusedPanel::Inspector,
            FocusedPanel::Contributions,
        ] {
            let mut app = handoff_app(0);
            app.focused_panel = panel;
            assert!(!ci_handoff_applies(&app), "{panel:?}");
        }
        let mut compact = handoff_app(0);
        compact.view = app::View::Compact;
        assert!(!ci_handoff_applies(&compact), "compact draws no pane 2");
    }

    #[test]
    fn a_handoff_targets_the_rows_repo_and_the_highlighted_run() {
        let mut app = handoff_app(2);
        let (handoff, (key, owner, name)) = resolve_run_handoff(&mut app, RunKey::Rerun).unwrap();
        assert_eq!(
            handoff,
            argv(&[
                "run",
                "rerun",
                "37958308363",
                "--failed",
                "-R",
                "gaurgle/fleetz"
            ])
        );
        assert_eq!(
            (key, owner.as_str(), name.as_str()),
            (ci_key(), "gaurgle", "fleetz")
        );
        assert_eq!(yank_status(&app), None, "nothing to note");
    }

    #[test]
    fn an_empty_ci_view_does_nothing() {
        let mut app = ci_app();
        app.view = app::View::Detail;
        app.history_view = app::HistoryView::Ci;
        app.focused_panel = FocusedPanel::Detail;
        for key in [RunKey::Log, RunKey::Rerun, RunKey::Cancel, RunKey::Open] {
            assert_eq!(resolve_run_handoff(&mut app, key), None, "{key:?}");
        }
        assert_eq!(yank_status(&app), None);
        let mut no_runs = handoff_app(0);
        no_runs.ci.get_mut(&ci_key()).unwrap().runs.clear();
        assert_eq!(resolve_run_handoff(&mut no_runs, RunKey::Open), None);
    }

    #[test]
    fn a_row_without_a_github_identity_hands_nothing_off() {
        let mut app = handoff_app(0);
        app.repos[0].owner = None;
        assert_eq!(resolve_run_handoff(&mut app, RunKey::Open), None);
        assert_eq!(yank_status(&app), None);
    }

    #[test]
    fn a_refused_handoff_says_why_on_the_status_line() {
        let mut app = handoff_app(1);
        assert_eq!(resolve_run_handoff(&mut app, RunKey::Cancel), None);
        assert_eq!(
            yank_status(&app),
            Some(("run is not in progress".to_string(), false))
        );
        let mut live = handoff_app(0);
        assert_eq!(resolve_run_handoff(&mut live, RunKey::Log), None);
        assert_eq!(
            yank_status(&live),
            Some(("run has not finished".to_string(), false))
        );
    }

    /// Back from `gh`, the repo polls at once rather than at its next 10 or
    /// 60 s slot, so a rerun or a cancel shows up without waiting.
    #[test]
    fn back_from_gh_the_runs_repo_polls_at_once() {
        let mut app = handoff_app(1);
        let start = std::time::Instant::now();
        let settled = start + std::time::Duration::from_secs(1);
        ci_poll_request(&mut app, at(12, 0, 0), start);
        assert_eq!(
            ci_poll_request(&mut app, at(12, 0, 5), settled),
            None,
            "not due on its own until 12:00:10"
        );
        let target = ci::target(&app.repos[0]).unwrap();
        let request =
            ci_poll_after_handoff(&mut app, target.clone(), at(12, 0, 5)).expect("polled at once");
        assert_eq!(request.key, ci_key());
        assert!(app.ci[&ci_key()].inflight);
        assert_eq!(
            ci_poll_after_handoff(&mut app, target, at(12, 0, 6)),
            None,
            "one poll out at a time"
        );
    }

    /// The immediate poll skips the interval, nothing else: a rate limit or
    /// `-d` still sends nothing.
    #[test]
    fn back_from_gh_a_rate_limited_or_dirty_mode_repo_does_not_poll() {
        let failure = github::Failure::rate_limit("API rate limit exceeded");
        let budget = reading("tok", 0, at(12, 40, 0), at(12, 0, 10));
        let mut limited = ci_failure_lands(failure, Some(budget));
        limited.view = app::View::Detail;
        limited.history_view = app::HistoryView::Ci;
        let target = ci::target(&limited.repos[0]).unwrap();
        assert_eq!(
            ci_poll_after_handoff(&mut limited, target.clone(), at(12, 1, 0)),
            None
        );
        let mut dirty = handoff_app(1);
        dirty.dirty_mode = true;
        assert_eq!(
            ci_poll_after_handoff(&mut dirty, target, at(12, 1, 0)),
            None
        );
    }
}

/// FZ-45: section headers as cursor stops and the slot digest, driven
/// through the key handler and checked against what the renderer draws.
#[cfg(test)]
mod digest_handler_tests {
    use super::*;
    use crate::digest::tests::repo;

    const W: u16 = 120;
    const H: u16 = 44;

    /// Two github activity rows, one local changes row, two also active.
    fn sectioned_app() -> App {
        let repos = vec![
            repo("alpha", RepoLocation::Primary),
            repo("bravo", RepoLocation::Primary),
            repo("charlie", RepoLocation::LocalOnly),
            repo("delta", RepoLocation::Elsewhere),
            repo("echo", RepoLocation::NotCloned),
        ];
        let mut app = App::new(repos, String::new(), W, H);
        app.view = app::View::Detail;
        app.focused_panel = FocusedPanel::RepoList;
        let pushed = chrono::Utc::now() - chrono::Duration::minutes(1);
        app.dataset = ["alpha", "bravo", "delta", "echo"]
            .iter()
            .map(|name| github::GithubRepo {
                name: name.to_string(),
                pushed_at: pushed,
                pr_count: 0,
                prs: vec![],
                ci_status: model::CiStatus::Unknown,
                ci_workflow: None,
                ci_duration: None,
                repo_kind: model::RepoKind::Unknown,
                owner: None,
                disk_usage: None,
            })
            .collect();
        app
    }

    fn screen(app: &App) -> Vec<String> {
        use ratatui::backend::TestBackend;
        let mut terminal = Terminal::new(TestBackend::new(W, H)).unwrap();
        terminal.draw(|f| ui::render(f, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..H)
            .map(|y| (0..W).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect()
    }

    /// The pane 1 row the renderer marks as the cursor's, trimmed.
    fn marked_row(app: &App) -> (usize, String) {
        let rows = screen(app);
        let list = app.list_hits.get().area;
        let found: Vec<(usize, String)> = rows
            .iter()
            .enumerate()
            .filter_map(|(y, row)| {
                let cells: String = row
                    .chars()
                    .skip(list.x as usize)
                    .take(list.width as usize)
                    .collect();
                cells
                    .trim_start()
                    .starts_with('▸')
                    .then(|| (y, cells.trim().to_string()))
            })
            .collect();
        assert_eq!(found.len(), 1, "one marked row: {found:?}");
        found.into_iter().next().unwrap()
    }

    async fn press_key(app: &mut App, code: KeyCode) {
        let args = Args::parse_from(["fleetz"]);
        let config = config::Config::default();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let backend = CrosstermBackend::new(io::stdout());
        let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, W, H));
        let mut terminal =
            Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap();
        handle_normal_input(
            app,
            &mut terminal,
            KeyEvent::new(code, KeyModifiers::NONE),
            &tx,
            &args,
            &config,
            Path::new("/nonexistent"),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn j_and_k_stop_on_every_header_in_the_order_the_list_draws() {
        let mut app = sectioned_app();
        press_key(&mut app, KeyCode::Char('k')).await;
        assert_eq!(
            app.on_header(),
            Some(digest::Section::GithubActivity),
            "k from the first repo lands on its header"
        );
        let mut walked = vec![marked_row(&app)];
        for _ in 0..8 {
            press_key(&mut app, KeyCode::Char('j')).await;
            walked.push(marked_row(&app));
        }
        let ys: Vec<usize> = walked.iter().map(|(y, _)| *y).collect();
        let texts: Vec<&str> = walked.iter().map(|(_, t)| t.as_str()).collect();
        assert!(
            ys.windows(2).take(7).all(|w| w[0] < w[1]),
            "the cursor walks down the drawn rows: {walked:?}"
        );
        assert_eq!(ys[7], ys[8], "j stops at the last row");
        let expected = [
            "github activity",
            "alpha",
            "bravo",
            "local changes",
            "charlie",
            "also active",
            "delta",
            "echo",
        ];
        for (text, want) in texts.iter().zip(expected) {
            assert!(text.contains(want), "{text:?} is not {want:?}: {texts:?}");
        }
        // And back up through the same stops.
        for want in expected.iter().rev().skip(1) {
            press_key(&mut app, KeyCode::Char('k')).await;
            assert!(marked_row(&app).1.contains(want), "k back to {want}");
        }
    }

    #[tokio::test]
    async fn the_compact_grid_has_no_header_stops() {
        let mut app = sectioned_app();
        app.view = app::View::Compact;
        app.selected = 2;
        press_key(&mut app, KeyCode::Char('k')).await;
        assert_eq!(app.selected, 1, "k skips the local changes header");
        assert_eq!(app.on_header(), None);
    }

    #[tokio::test]
    async fn on_a_header_pane_2_draws_the_digest_and_leaving_restores_its_view() {
        let mut app = sectioned_app();
        app.history_view = app::HistoryView::Commits;
        let before = screen(&app).join("\n");
        assert!(before.contains("COMMITS"), "{before}");

        press_key(&mut app, KeyCode::Char('k')).await;
        let on_header = screen(&app).join("\n");
        assert!(on_header.contains("2 repos · 0 commits"), "{on_header}");
        assert!(on_header.contains("pushed 1m ago"), "{on_header}");
        assert!(!on_header.contains("COMMITS"), "{on_header}");

        press_key(&mut app, KeyCode::Char('j')).await;
        assert_eq!(app.history_view, app::HistoryView::Commits);
        assert_eq!(app.selected, 0);
        let after = screen(&app).join("\n");
        assert!(after.contains("COMMITS"), "{after}");
        assert!(!after.contains("2 repos · 0 commits"), "{after}");
    }

    #[tokio::test]
    async fn t_cycles_the_digest_window_and_leaves_the_overlay_shut() {
        let mut app = sectioned_app();
        press_key(&mut app, KeyCode::Char('k')).await;
        let mut seen = vec![app.digest_window];
        for _ in 0..4 {
            press_key(&mut app, KeyCode::Char('t')).await;
            assert!(matches!(app.overlay, Overlay::None), "t opened the overlay");
            seen.push(app.digest_window);
        }
        use digest::DigestWindow::*;
        assert_eq!(seen, vec![Slot, Today, Yesterday, Week, Slot]);
        let drawn = screen(&app).join("\n");
        assert!(drawn.contains("slot · today · yesterday · week"), "{drawn}");

        // Off the header `t` is the time window overlay again.
        press_key(&mut app, KeyCode::Char('j')).await;
        press_key(&mut app, KeyCode::Char('t')).await;
        assert!(matches!(app.overlay, Overlay::TimeWindow(_)));
    }

    #[tokio::test]
    async fn enter_on_a_digest_line_lands_on_its_repo_row() {
        let mut app = sectioned_app();
        app.selected = 3;
        press_key(&mut app, KeyCode::Char('k')).await;
        assert_eq!(app.on_header(), Some(digest::Section::AlsoActive));
        app.focused_panel = FocusedPanel::Detail;
        press_key(&mut app, KeyCode::Char('j')).await;
        assert_eq!(app.digest_selected, 1, "j walks the digest in pane 2");
        assert_eq!(app.on_header(), Some(digest::Section::AlsoActive));
        press_key(&mut app, KeyCode::Enter).await;
        assert_eq!(app.on_header(), None);
        assert_eq!(app.selected_repo().map(|r| r.name.as_str()), Some("echo"));
        assert_eq!(app.focused_panel, FocusedPanel::RepoList);
    }

    #[test]
    fn a_click_or_a_hover_on_a_header_lands_on_it() {
        let mut app = sectioned_app();
        let rows = screen(&app);
        let list = app.list_hits.get().area;
        let y_of = |label: &str| {
            rows.iter()
                .position(|r| r.contains(label))
                .unwrap_or_else(|| panic!("{label} not drawn")) as u16
        };
        let x = list.x + 2;
        app.click_list(x, y_of("local changes"));
        assert_eq!(app.on_header(), Some(digest::Section::LocalChanges));
        app.click_list(x, y_of("delta"));
        assert_eq!(app.on_header(), None);
        assert_eq!(app.selected_repo().map(|r| r.name.as_str()), Some("delta"));
        // A rested hover lands on a header and on nothing else.
        screen(&app);
        let t0 = std::time::Instant::now();
        app.hover_list(x, y_of("bravo"), t0);
        app.settle_hover(t0 + app::HOVER_DWELL);
        assert_eq!(app.selected_repo().map(|r| r.name.as_str()), Some("delta"));
        app.hover_list(x, y_of("also active"), t0);
        app.settle_hover(t0 + app::HOVER_DWELL);
        assert_eq!(app.on_header(), Some(digest::Section::AlsoActive));
        // The blank row between sections is not a stop.
        let gap = y_of("also active") - 1;
        app.click_list(x, gap);
        assert_eq!(app.on_header(), Some(digest::Section::AlsoActive));
    }

    /// Review F6: a pointer passing over a header lands nowhere; one resting
    /// on it for `HOVER_DWELL` lands there.
    #[test]
    fn a_hover_lands_on_a_header_only_after_the_pointer_rests() {
        let mut app = sectioned_app();
        let rows = screen(&app);
        let x = app.list_hits.get().area.x + 2;
        let y_of = |label: &str| rows.iter().position(|r| r.contains(label)).unwrap() as u16;
        let ms = std::time::Duration::from_millis;
        let t0 = std::time::Instant::now();
        app.hover_list(x, y_of("also active"), t0);
        app.settle_hover(t0 + ms(100));
        assert_eq!(app.on_header(), None, "not rested yet");
        app.hover_list(x, y_of("delta"), t0 + ms(150));
        app.settle_hover(t0 + ms(1000));
        assert_eq!(app.on_header(), None, "passed over, not rested on");
        assert_eq!(app.selected, 0);

        let t1 = t0 + ms(2000);
        app.hover_list(x, y_of("also active"), t1);
        app.hover_list(x, y_of("also active"), t1 + ms(200));
        app.settle_hover(t1 + app::HOVER_DWELL - ms(1));
        assert_eq!(
            app.on_header(),
            None,
            "the dwell runs from the first arrival"
        );
        app.settle_hover(t1 + app::HOVER_DWELL);
        assert_eq!(app.on_header(), Some(digest::Section::AlsoActive));

        // Leaving the list cancels a pending hover.
        app.hover_list(x, y_of("local changes"), t1);
        app.clear_hover();
        app.settle_hover(t1 + ms(5000));
        assert_eq!(app.on_header(), Some(digest::Section::AlsoActive));
    }

    /// Review B1: the compact grid never writes `list_hits`, so a click
    /// there after a detail frame must not resolve against that frame.
    #[test]
    fn a_click_in_compact_after_a_detail_frame_leaves_the_selection() {
        let mut app = sectioned_app();
        let rows = screen(&app);
        let x = app.list_hits.get().area.x + 2;
        let y = rows.iter().position(|r| r.contains("echo")).unwrap() as u16;
        app.toggle_view();
        assert_eq!(app.view, app::View::Compact);
        app.click_list(x, y);
        let t0 = std::time::Instant::now();
        app.hover_list(x, y - 3, t0);
        app.settle_hover(t0 + app::HOVER_DWELL);
        assert_eq!(app.selected, 0);
        assert_eq!(app.selected_repo().map(|r| r.name.as_str()), Some("alpha"));
    }

    /// Review F5: on a header `J`/`K` drive the digest's pane 2 cursor and
    /// the digest inspector, never the hidden repo's detail or `ci` bodies.
    #[tokio::test]
    async fn shift_j_on_a_header_drives_the_digest_panes() {
        let mut app = sectioned_app();
        app.history_view = app::HistoryView::Ci;
        press_key(&mut app, KeyCode::Char('k')).await;
        assert!(app.on_header().is_some());
        app.detail_max_scroll.set(10);
        app.inspector_max_scroll.set(10);
        press_key(&mut app, KeyCode::Char('J')).await;
        assert_eq!(app.digest_selected, 1, "J from the list walks the digest");
        assert_eq!(app.detail_scroll(), 0, "the hidden view did not scroll");
        assert_eq!(app.detail_selected(), 0);

        app.focused_panel = FocusedPanel::Detail;
        press_key(&mut app, KeyCode::Char('J')).await;
        assert_eq!(
            app.inspector_scroll, 1,
            "J from pane 2 scrolls the digest inspector"
        );
        assert_eq!(app.ci_body_scrolls, [0; 3], "not the hidden `ci` bodies");
        press_key(&mut app, KeyCode::Char('K')).await;
        assert_eq!(app.inspector_scroll, 0);
        assert_eq!(app.history_view, app::HistoryView::Ci);
    }

    /// Review F7: `g` puts `selected` back on 0, which is the github
    /// activity section's first repo in the group too; the header the cursor
    /// stood on must not come back with it.
    #[tokio::test]
    async fn a_group_change_does_not_bring_the_header_back() {
        let mut app = sectioned_app();
        app.group_names = vec!["work".to_string()];
        app.repos[0].groups = vec!["work".to_string()];
        app.repos[2].groups = vec!["work".to_string()];
        press_key(&mut app, KeyCode::Char('k')).await;
        assert_eq!(app.on_header(), Some(digest::Section::GithubActivity));
        press_key(&mut app, KeyCode::Char('g')).await;
        assert_eq!(app.active_group, Some(0));
        assert!(
            app.list_rows()
                .contains(&app::ListRow::Header(digest::Section::GithubActivity))
        );
        assert_eq!(app.selected, 0);
        assert_eq!(app.on_header(), None);
        assert_eq!(app.selected_repo().map(|r| r.name.as_str()), Some("alpha"));
    }
}
