mod app;
mod cache;
mod cli;
mod config;
mod contributions;
mod discovery;
mod fuzzy;
mod git;
mod github;
mod graph;
mod model;
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
/// what made the intro stutter on a release build and not on a debug one --
/// release finishes the repo probes fast enough to flood the channel during the
/// wave, where debug spreads the same work out past the end of it.
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
    },
    /// A background poll's answer. `attempt` is the id it was started with
    /// (`GithubFreshness::begin_attempt`), so an answer that arrives after a
    /// newer one was applied can be told apart and discarded.
    Github {
        attempt: u64,
        result: Result<Box<github::GithubFetch>, github::FetchError>,
    },
    Progress(ApplyProgress),
    /// One frame of the intro wave. On its own timer: the loop's 100ms tick is
    /// the spinner rate and far too coarse for an animation. See `ui::intro`.
    IntroFrame,
    /// One frame of the noise field shown while the first load runs.
    LoadingFrame,
    SweepDone {
        at: chrono::DateTime<chrono::Local>,
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
}

/// Why a load produced nothing to show.
#[derive(Debug)]
pub struct LoadError {
    pub message: String,
    /// The GitHub attempt behind it, when that is what failed. A failed
    /// attempt still advances the retry clock even though nothing landed.
    pub github_failure: Option<github::Failure>,
}

impl From<String> for LoadError {
    fn from(message: String) -> Self {
        Self {
            message,
            github_failure: None,
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
/// first load lands and the wave takes over.
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

/// Drive the intro wave at its own frame rate for as long as it lasts, then
/// stop. A one-shot task rather than a faster global tick, because the global
/// tick *is* the spinner rate and everything else is expressed in it.
fn spawn_intro_task(tx: tokio::sync::mpsc::UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(ui::intro::FRAME_INTERVAL);
        // See `spawn_loading_task`: catch-up ticks paint the same frame twice.
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let deadline = std::time::Instant::now() + ui::intro::DURATION;
        loop {
            interval.tick().await;
            if tx.send(AppEvent::IntroFrame).is_err() || std::time::Instant::now() >= deadline {
                break;
            }
        }
    });
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
    app.contrib_cell = config
        .contributions_cell_size
        .as_deref()
        .map(app::CellSize::from_config)
        .unwrap_or_default();
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
    spawn_load(
        &tx,
        &args,
        &config,
        &repo_dir,
        initial_window,
        LoadKind::Initial,
        false,
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
fn spawn_load(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    window: Option<time::TimeWindow>,
    kind: LoadKind,
    bypass_cache: bool,
) {
    debug_assert!(
        !matches!(kind, LoadKind::WindowApply(_)),
        "window applies belong in spawn_window_apply"
    );
    let tx = tx.clone();
    let args = args.clone();
    let config = config.clone();
    let repo_dir = repo_dir.to_path_buf();
    tokio::spawn(async move {
        let mode = args.mode();
        let result = load_repos(&args, &config, &repo_dir, &mode, window, bypass_cache)
            .await
            .map(|(repos, header, dataset, calendar, source)| {
                Box::new(LoadResult {
                    kind,
                    repos,
                    header,
                    dataset: Some(dataset),
                    github_calendar: calendar,
                    source,
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
    let args = args.clone();
    let config = config.clone();
    let repo_dir = repo_dir.to_path_buf();
    tokio::spawn(async move {
        let cutoff = w.cutoff(config.work_start, config.work_end);
        let header = w.label(config.work_start, config.work_end);
        let result = assemble_view(
            &dataset,
            Some(cutoff),
            header,
            &args,
            &config,
            &repo_dir,
            false,
            Some(&tx),
        )
        .await
        .map(|(repos, header)| {
            Box::new(LoadResult {
                kind: LoadKind::WindowApply(w),
                repos,
                header,
                dataset: None,
                github_calendar: None,
                // A window apply re-filters what is already in memory and
                // fetches nothing, so it must not touch the poll clock either
                // way. `dataset: None` already guards that.
                source: app::DatasetSource::Local,
            })
        })
        .map_err(LoadError::from);
        let _ = tx.send(AppEvent::Loaded(result));
    });
}

/// Swap a finished load into the app, preserving UI state (selection is
/// clamped, groups re-applied), and say what happened.
fn apply_load_result(
    app: &mut App,
    config: &config::Config,
    result: Result<Box<LoadResult>, LoadError>,
) {
    let now = chrono::Local::now();
    app.apply_progress = None;
    app.loading = false;
    match result {
        Ok(lr) => {
            app.load_error = None;
            app.repos = carry_fetch_facts(&app.repos, lr.repos);
            apply_groups(&mut app.repos, config);
            app.header = lr.header;
            if let Some(ds) = lr.dataset {
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
                LoadKind::WindowApply(w) => {
                    window_state::WindowState::new().save(&w);
                    let label = app.header.clone();
                    app.set_status(format!("Window: {label}"), false);
                }
            }
        }
        Err(e) => {
            if let Some(failure) = e.github_failure {
                app.github_freshness.failed(failure, now);
            }
            // The status message says it once; `load_error` keeps saying it
            // for as long as there is nothing on screen to say it instead.
            app.set_status(format!("Load failed: {}", e.message), true);
            app.load_error = Some(e.message);
        }
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
    // The wait is spent on the noise the wave will later wash away, rather
    // than on an empty frame. Started before the first paint so there is never
    // a blank one.
    let loading_running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if config.intro && app.loading {
        app.loading_field = Some(ui::intro::Loading::new());
        loading_running.store(true, std::sync::atomic::Ordering::Relaxed);
        spawn_loading_task(tx.clone(), loading_running.clone());
    }
    terminal
        .draw(|frame| match app.loading_field.as_ref() {
            Some(field) => field.paint(frame.buffer_mut()),
            None => ui::render(frame, app),
        })
        .map_err(|e| e.to_string())?;

    // How many held paints have accumulated, so a stream that never drains
    // still gets drawn. See the paint at the foot of the loop.
    let mut coalesced: u32 = 0;

    while let Some(ev) = rx.recv().await {
        match ev {
            AppEvent::Tick => {
                app.on_tick();
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
                let spinning = app.loading || app.is_refreshing();
                // Everything else that moves on its own (the swept-ago clock,
                // the CI blink) only needs about one repaint a second.
                let slow_animated = app.watch_mode || app.apply_progress.is_some();
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
            }
            AppEvent::Input(Event::Key(key)) => {
                // Any keypress ends the wave on the spot. Seven tenths of a
                // second is short, but it is still time someone did not ask
                // for, and a dashboard that ignores the first keystroke of
                // every launch would be worse than one that never animated.
                app.intro = None;
                // ...and the field with it. Skip means skip: an empty frame is
                // what there was before any of this, and is at least honest
                // about how far along the load is.
                app.loading_field = None;
                app.intro_pending = false;
                loading_running.store(false, std::sync::atomic::Ordering::Relaxed);
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
                // Arm the wave for the paint that follows: that paint is the
                // first screenful of real content, which is what it resolves
                // into. Only ever armed here, so it runs once a launch.
                if config.intro && !app.intro_armed {
                    app.intro_armed = true;
                    app.intro_pending = true;
                }
                // The field hands over to the wave. Cleared here rather than
                // when the wave starts, so the two never both want the screen.
                app.loading_field = None;
                loading_running.store(false, std::sync::atomic::Ordering::Relaxed);
                app.needs_redraw = true;
            }
            AppEvent::IntroFrame => {
                if app.intro.as_ref().is_some_and(|i| i.finished()) {
                    app.intro = None;
                }
                app.needs_redraw = true;
            }
            AppEvent::LoadingFrame => {
                app.needs_redraw = app.loading_field.is_some();
            }
            AppEvent::Progress(p) => {
                app.apply_progress = Some(p);
                app.needs_redraw = true;
            }
            AppEvent::RepoUpdated {
                path,
                status,
                fetch,
            } => apply_repo_updated(app, &path, *status, fetch),
            AppEvent::Github { attempt, result } => {
                apply_github_poll(app, attempt, result, &tx, &args.author)
            }
            AppEvent::SweepDone { at } => {
                app.sweep_started = None;
                app.last_refreshed = Some(at);
                app.needs_redraw = true;
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
            let animating = app.intro.is_some() || app.loading_field.is_some();
            if should_paint(rx.is_empty(), coalesced, animating) {
                coalesced = 0;
                app.needs_redraw = false;
                // While the wave is running it composes every frame from the
                // screen it captured, so the real UI is not re-rendered forty
                // times in seven tenths of a second. A resize invalidates that
                // capture, and `paint` says so rather than drawing a stale
                // layout.
                let mut wave_ended = false;
                // The screen the wave will resolve into, captured *inside* the
                // closure. It has to be: `draw` swaps buffers on the way out,
                // so by the time it returns the screen just painted is the
                // previous buffer and `current_buffer_mut` is a freshly reset
                // blank. Capturing after the call captured that blank, and the
                // wave dutifully resolved into nothing -- noise, an empty pane,
                // then the content snapping in when the wave ended, which is
                // not a reveal at all.
                let mut captured = None;
                terminal
                    .draw(
                        |frame| match (app.loading_field.as_ref(), app.intro.as_ref()) {
                            (Some(field), _) => field.paint(frame.buffer_mut()),
                            (None, Some(intro)) => wave_ended = !intro.paint(frame.buffer_mut()),
                            (None, None) => {
                                ui::render(frame, app);
                                if app.intro_pending {
                                    let intro = ui::intro::Intro::new(frame.buffer_mut().clone());
                                    // Painted over immediately, in this same frame.
                                    // Presenting the dashboard and only then
                                    // starting the wave would show a single frame
                                    // of finished content before it dissolved --
                                    // a flash on the way in to a reveal.
                                    intro.paint(frame.buffer_mut());
                                    captured = Some(intro);
                                }
                            }
                        },
                    )
                    .map_err(|e| e.to_string())?;
                if wave_ended {
                    app.intro = None;
                    app.needs_redraw = true;
                }
                // Started from the first painted screenful of real content, so
                // a slow load never animates an empty dashboard.
                if let Some(intro) = captured {
                    app.intro_pending = false;
                    app.intro = Some(intro);
                    spawn_intro_task(tx.clone());
                }
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

/// Mouse: left-click focuses the panel under the cursor; the scroll wheel
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
        return;
    };

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
        }
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
            FocusedPanel::Contributions => {}
        },
        MouseEventKind::ScrollUp => match panel {
            FocusedPanel::RepoList => app.move_up(),
            FocusedPanel::Detail => app.scroll_detail(-DETAIL_WHEEL_ROWS),
            FocusedPanel::Inspector => app.scroll_inspector(-1),
            FocusedPanel::Contributions => {}
        },
        _ => {}
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
                        start_single_sweep(app, tx, idx, false, args.author.clone());
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
                                start_single_sweep(app, tx, idx, false, args.author.clone());
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
                    let (interval, view, persist, contributions, contrib_cell) = (
                        so.refresh_interval,
                        so.view,
                        so.persist_panes,
                        so.contributions,
                        so.contrib_cell,
                    );
                    so.status = Some(format!("saved to {}", tilde_display(&path)));
                    // Live fields take effect immediately; the overlay stays
                    // open so the confirmation is actually readable.
                    app.refresh_interval = interval;
                    app.persist_pane_sizes = persist;
                    app.apply_contributions_setting(contributions);
                    app.contrib_cell = contrib_cell;
                    if let Some(v) = view {
                        app.view = v;
                        // Same flag `v` sets, so a later resize can't undo an
                        // explicit choice.
                        app.view_overridden = true;
                    } else {
                        app.view_overridden = false;
                    }
                    app.needs_redraw = true;
                }
                Err(e) => so.status = Some(e),
            }
        }
        _ => {}
    }
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
    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        // A leftover search query (set, then Enter'd out of edit mode) is
        // cleared by Esc -- matching the strip's "esc to clear" hint.
        KeyCode::Esc if !app.search_query.is_empty() => app.clear_search(),
        KeyCode::Tab | KeyCode::BackTab => app.toggle_panel_focus(),
        // Shift+scroll: scroll the *next* pane, so you can stand in one pane
        // and pull more into view in the one beside it without leaving where
        // you are. Plain ↓/j (below) drive whatever is focused; these reach
        // past it into the pane after it. Shift+j/k
        // arrive as uppercase `J`/`K` (no modifier needed), Shift+arrow as an
        // arrow carrying the SHIFT modifier. `PageUp`/`PageDown` still scroll
        // the focused detail pane itself, which is the gesture this used to
        // be. See `App::shift_scroll_target`.
        KeyCode::Char('J') => app.scroll_next_panel(1),
        KeyCode::Char('K') => app.scroll_next_panel(-1),
        KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => app.scroll_next_panel(1),
        KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => app.scroll_next_panel(-1),
        // Must be matched before the plain `c` arm below, or ctrl-c switches
        // the contributions source instead of quitting.
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        // `[` and `]` step the repo pane's body between its four views --
        // lazygit's own binding for the same gesture, and unlike a shifted
        // letter it gives backwards for free.
        KeyCode::Char(']') => app.cycle_history_view(false),
        KeyCode::Char('[') => app.cycle_history_view(true),
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
        KeyCode::Char('j') | KeyCode::Down => match app.focused_panel {
            FocusedPanel::RepoList => app.move_down(),
            FocusedPanel::Detail => app.detail_nav_down(),
            FocusedPanel::Inspector => app.scroll_inspector(1),
            FocusedPanel::Contributions => {
                app.move_contrib_cursor(0, 1, chrono::Local::now().date_naive())
            }
        },
        KeyCode::Char('k') | KeyCode::Up => match app.focused_panel {
            FocusedPanel::RepoList => app.move_up(),
            FocusedPanel::Detail => app.detail_nav_up(),
            FocusedPanel::Inspector => app.scroll_inspector(-1),
            FocusedPanel::Contributions => {
                app.move_contrib_cursor(0, -1, chrono::Local::now().date_naive())
            }
        },
        KeyCode::PageDown => match app.focused_panel {
            FocusedPanel::Inspector => app.scroll_inspector(10),
            _ => app.scroll_detail(10),
        },
        KeyCode::PageUp => match app.focused_panel {
            FocusedPanel::Inspector => app.scroll_inspector(-10),
            _ => app.scroll_detail(-10),
        },
        KeyCode::Char('v') => app.toggle_view(),
        KeyCode::Char('?') => app.toggle_help(),
        KeyCode::Char('/') => app.start_search(),
        // Fetch (not pull): update remote-tracking refs so ahead/behind is
        // accurate, without merging anything into the working tree. Actual
        // pull/merge is left to lazygit (Enter) on purpose -- fetching is
        // safe and non-destructive, merging is not.
        KeyCode::Char('f') => {
            if let Some(repo) = app.selected_repo().cloned()
                && repo.path.is_some()
                && let Some(idx) = app.selected_repo_index()
            {
                app.set_status(format!("\u{27f3} Fetching {}…", repo.name), false);
                start_single_sweep(app, tx, idx, true, args.author.clone());
                app.manual_fetches.expect(repo.path.clone());
            }
        }
        KeyCode::Char('F') => {
            app.set_status("\u{27f3} Fetching all…".to_string(), false);
            start_sweep(app, tx, true, args.author.clone(), true);
        }
        KeyCode::Enter => {
            // Open lazygit in selected repo
            if let Some(repo) = app.selected_repo()
                && let Some(ref path) = repo.path
            {
                let expanded = shellexpand::tilde(path).to_string();
                suspend_and_run(terminal, "lazygit", &["-p", &expanded])?;
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
            }
        }
        KeyCode::Char('o') => {
            // Open shell in repo dir
            if let Some(repo) = app.selected_repo()
                && let Some(ref path) = repo.path
            {
                let expanded = shellexpand::tilde(path).to_string();
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
                suspend_and_run_in_dir(terminal, &shell, &[], &expanded)?;
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
        // Settings overlay. Seeded from live app state where one exists
        // (interval, view, pane persistence) and from the loaded config where
        // it does not, so the values shown are the ones actually in force.
        KeyCode::Char(',') => {
            app.overlay = Overlay::Settings(app::SettingsOverlay::new(
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
            ));
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
        KeyCode::Char('r') => {
            app.set_status("Refreshing…".to_string(), false);
            start_sweep(app, tx, false, args.author.clone(), true);
            start_github_poll(app, tx, config);
            // Already in flight from the watch timer counts too: its answer
            // is the one this refresh is waiting for.
            app.refresh_awaits_poll = app.github_poll_inflight;
        }
        // Full reload: re-fetch GitHub (cache-bypassed) and re-run discovery,
        // so a just-pushed or newly-cloned repo appears. Shifted because it
        // costs a network round trip and GitHub rate limit. Runs on a
        // background task, so the list stays scrollable while it works.
        KeyCode::Char('R') => {
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
            );
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

/// Suspend the TUI, run a command, then resume. Input reading is paused for
/// the whole child run so the reader thread cannot swallow its keystrokes.
fn suspend_and_run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    cmd: &str,
    args: &[&str],
) -> Result<(), String> {
    // Leave alternate screen
    disable_raw_mode().map_err(|e| e.to_string())?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen).map_err(|e| e.to_string())?;
    terminal.show_cursor().map_err(|e| e.to_string())?;

    // Run the command
    let _ = with_input_paused(|| StdCommand::new(cmd).args(args).status());

    // Re-enter alternate screen
    crossterm::execute!(terminal.backend_mut(), EnterAlternateScreen).map_err(|e| e.to_string())?;
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
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen).map_err(|e| e.to_string())?;
    terminal.show_cursor().map_err(|e| e.to_string())?;

    let expanded = shellexpand::tilde(dir);
    let _ = with_input_paused(|| {
        StdCommand::new(cmd)
            .args(args)
            .current_dir(expanded.as_ref())
            .status()
    });

    crossterm::execute!(terminal.backend_mut(), EnterAlternateScreen).map_err(|e| e.to_string())?;
    enable_raw_mode().map_err(|e| e.to_string())?;
    terminal.clear().map_err(|e| e.to_string())?;
    Ok(())
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

/// Floor on the GitHub poll interval, whatever the config says. Measured
/// against a 5000 point/hour GraphQL budget, one poll of ~50 repos costs a
/// few points, so this is comfortable; it exists to stop a typo turning the
/// dashboard into a rate-limit incident.
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
fn apply_repo_updated(app: &mut App, path: &str, status: RepoStatus, fetch: git::FetchOutcome) {
    app.settle_refresh_fetch(path, &fetch);
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
        if is_latest {
            // A refresh waiting on this request is over all the same.
            app.settle_refresh_poll(std::iter::empty());
        }
        return;
    }
    let now = chrono::Local::now();
    match result {
        Ok(fetch) => {
            app.github_freshness.succeeded(now);
            let github::GithubFetch {
                repos: dataset,
                calendar,
                has_pr_counts: _,
                // The poll always bypasses the cache, so this is
                // network data by construction.
                origin: _,
            } = *fetch;
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
            for repo in &mut app.repos {
                if let Some(gh) = dataset
                    .iter()
                    .find(|g| g.name.eq_ignore_ascii_case(&repo.name))
                {
                    repo.apply_github(gh);
                }
            }
            // One API call just told us which remotes moved, so
            // fetch only those instead of waiting for the next
            // blanket sweep to fetch all of them.
            let moved = remotes_that_moved(&app.dataset, &dataset, &app.repos);
            for &idx in &moved {
                start_single_sweep(app, tx, idx, true, author.clone());
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
    // A detected push jumps the queue: it is the one moment CI is about to
    // change with nothing `Pending` yet to notice. Not while polls fail, where
    // the next attempt waits out a full interval (rule 6).
    let push = app.push_detected && !freshness.is_failing();
    push || freshness.retry_due(now, github_poll_secs(app, config))
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
                .find(|was| was.name.eq_ignore_ascii_case(&now.name))
                .is_some_and(|was| now.pushed_at > was.pushed_at)
        })
        .filter_map(|moved| {
            repos
                .iter()
                .position(|r| r.path.is_some() && r.name.eq_ignore_ascii_case(&moved.name))
        })
        .collect()
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
/// dead network four times a minute.
fn github_poll_secs(app: &App, config: &config::Config) -> i64 {
    let configured = config.github_poll_interval.max(GITHUB_POLL_MIN);
    if app.github_freshness.is_failing() {
        return configured as i64;
    }
    let any_pending = app
        .repos
        .iter()
        .any(|r| r.ci_status == model::CiStatus::Pending);
    if any_pending {
        GITHUB_POLL_MIN as i64
    } else {
        configured as i64
    }
}

/// Kick off a GitHub re-poll, taking the in-flight lock. No-op if one is
/// already running, so mashing `r` cannot stack requests.
fn start_github_poll(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    config: &config::Config,
) {
    if let Some(attempt) = claim_github_poll(app) {
        spawn_github_poll(tx, config, attempt);
    }
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
    // silently, and `r` does not fetch here at all.
    if do_fetch && manual {
        app.manual_fetches
            .expect(targets.iter().map(|t| t.display_path.clone()));
    }
    let probe = contrib_probe(app);
    spawn_sweep(tx.clone(), targets, do_fetch, author, true, probe);
}

/// Sweep a single repo (the `f` fetch and the refresh after a branch switch
/// or delete).
/// Does not take the sweep lock: it is one repo, and it must work while a
/// full sweep is running.
fn start_single_sweep(
    app: &App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    index: usize,
    do_fetch: bool,
    author: Option<String>,
) {
    let targets = sweep_targets(app, Some(index));
    if targets.is_empty() {
        return;
    }
    spawn_sweep(
        tx.clone(),
        targets,
        do_fetch,
        author,
        false,
        contrib_probe(app),
    );
}

/// The shared sweep body: fetch (capped) then rebuild (capped), streaming
/// each repo back as it lands so the list updates progressively.
fn spawn_sweep(
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    targets: Vec<SweepTarget>,
    do_fetch: bool,
    author: Option<String>,
    report_done: bool,
    probe: Option<ContribProbe>,
) {
    tokio::spawn(async move {
        let probe_sem = std::sync::Arc::new(tokio::sync::Semaphore::new(probe_concurrency()));
        let fetch_sem = std::sync::Arc::new(tokio::sync::Semaphore::new(FETCH_CONCURRENCY));
        let mut set = tokio::task::JoinSet::new();
        for target in targets {
            let probe_sem = probe_sem.clone();
            let fetch_sem = fetch_sem.clone();
            let tx = tx.clone();
            let author = author.clone();
            let probe = probe.clone();
            set.spawn(async move {
                let fetch = if do_fetch {
                    let _permit = fetch_sem.acquire().await;
                    git::fetch(&target.fs_path).await
                } else {
                    git::FetchOutcome::NotAttempted
                };
                let _permit = probe_sem.acquire().await;
                let status = rebuild_repo_status(&target, author.as_deref(), probe.as_ref()).await;
                let _ = tx.send(AppEvent::RepoUpdated {
                    path: target.display_path.clone(),
                    status: Box::new(status),
                    fetch,
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
    let cache = cache::ApiCache::new(config.cache_ttl, repo_limit);
    tokio::spawn(async move {
        let result = github::fetch_repos(repo_limit, &cache, github::CacheMode::Bypass).await;
        let _ = tx.send(AppEvent::Github {
            attempt,
            result: result.map(Box::new),
        });
    });
}

/// Load all repo data: GitHub API + local discovery + git status.
/// Returns the assembled view plus the unfiltered dataset so the TUI can
/// re-filter it live (time-window overlay) without re-fetching.
async fn load_repos(
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    mode: &Mode,
    window: Option<time::TimeWindow>,
    bypass_cache: bool,
) -> Result<
    (
        Vec<RepoStatus>,
        String,
        Vec<github::GithubRepo>,
        Option<Box<contributions::Calendar>>,
        app::DatasetSource,
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
        return Ok((repos, header, Vec::new(), None, app::DatasetSource::Local));
    }

    // Fetch from GitHub (with caching)
    let api_cache = cache::ApiCache::new(config.cache_ttl, config.repo_limit);
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
            fallback: Some(fallback),
        }) => {
            let written = match fallback.origin {
                github::Origin::Cache { written } => written,
                github::Origin::Network => None,
            };
            (*fallback, app::DatasetSource::Fallback { failure, written })
        }
        Err(github::FetchError {
            failure,
            fallback: None,
        }) => {
            return Err(LoadError {
                message: failure.message.clone(),
                github_failure: Some(failure),
            });
        }
    };
    let (dataset, calendar) = (fetch.repos, fetch.calendar);

    let (cutoff, header) = resolve_cutoff(window, mode, config)?;
    let (repos, full_header) =
        assemble_view(&dataset, cutoff, header, args, config, repo_dir, true, None).await?;

    Ok((repos, full_header, dataset, calendar, source))
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
    })
}

/// The GitHub GraphQL fetch (with disk cache), unfiltered: the top
/// repo_limit repos by push date.
async fn fetch_dataset(
    config: &config::Config,
    cache: &cache::ApiCache,
    bypass_cache: bool,
) -> Result<github::GithubFetch, github::FetchError> {
    let mode = match bypass_cache {
        true => github::CacheMode::Bypass,
        false => github::CacheMode::AnyCached,
    };
    github::fetch_repos(config.repo_limit, cache, mode).await
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
    let pushed: HashMap<String, chrono::DateTime<chrono::Utc>> = active
        .iter()
        .map(|r| (r.name.to_lowercase(), r.pushed_at))
        .collect();

    targets
        .iter()
        .filter(|(found, _)| {
            let Some(pushed_at) = pushed.get(&found.name.to_lowercase()) else {
                return true;
            };
            match git::last_successful_fetch_at(&found.path) {
                Some(fetched) => *pushed_at > chrono::DateTime::<chrono::Utc>::from(fetched),
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
    let rank: HashMap<String, usize> = active
        .iter()
        .enumerate()
        .map(|(i, r)| (r.name.to_lowercase(), i))
        .collect();
    targets.sort_by_key(|(f, _)| {
        rank.get(&f.name.to_lowercase())
            .copied()
            .unwrap_or(usize::MAX)
    });
}

/// Pass 3's candidates: the local repos passes 1-2 have not already shown,
/// each with the name it reports as.
///
/// Skipped by path as well as by name. A clone matched through its remote can
/// sit in a folder whose name is not the GitHub name (`DevOps_Portfolio` for
/// `portfolio-site`), and the name check alone let it back in as a second,
/// local-only row whenever it was dirty.
fn unclaimed_local(
    all_local: Vec<PathBuf>,
    claimed_paths: &HashSet<&Path>,
    checked_names: &HashSet<String>,
) -> Vec<(PathBuf, String)> {
    all_local
        .into_iter()
        .filter(|path| !claimed_paths.contains(path.as_path()))
        .filter_map(|path| {
            // Undotted, so a hidden clone dedupes against the GitHub name passes
            // 1-2 registered it under rather than showing a second `.foo` row.
            let name = discovery::repo_name_for_path(&path);
            (!checked_names.contains(&name.to_lowercase())).then_some((path, name))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)] // one call spine; a params struct would only rename the problem
async fn assemble_view(
    dataset: &[github::GithubRepo],
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
    header: String,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    git_fetch: bool,
    progress: Option<&tokio::sync::mpsc::UnboundedSender<AppEvent>>,
) -> Result<(Vec<RepoStatus>, String), String> {
    // name -> org/private/public, from the *full* fetch (before the activity
    // cutoff filters it down), so a local-only repo can still show its glyph
    // even though it wasn't pushed recently enough to be "active". Mirrors
    // repoz's `meta_lookup`.
    let kind_by_name: HashMap<String, model::RepoKind> = dataset
        .iter()
        .map(|r| (r.name.to_lowercase(), r.repo_kind))
        .collect();

    // Local scan roots: the primary repo_dir plus any configured extra dirs
    // (e.g. ~/.dotfiles) -- the fleetz equivalent of repoz's EXTRA_DIRS. Used
    // by all three passes, so a repo outside the main tree gets a full
    // GitHub-matched row in pass 1 rather than only surfacing under "local
    // changes" when it happens to be dirty.
    let mut scan_roots = vec![repo_dir.to_path_buf()];
    scan_roots.extend(config.resolved_extra_dirs());

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

    let wanted =
        discovery::Wanted::new(active.iter().map(|r| (r.owner.as_deref(), r.name.as_str())));

    // Pass 1: find under every scan root (repo_dir + extra_dirs)
    let primary_found = discovery::find_in_primary(&scan_roots, &wanted);
    let found_set: HashSet<String> = primary_found
        .iter()
        .map(|f| f.name.to_lowercase())
        .collect();

    // Pass 2: find elsewhere
    let home = dirs::home_dir().unwrap_or_default();
    let elsewhere_found = discovery::find_elsewhere(&home, &scan_roots, &wanted, &found_set);

    // Dedupe up front: a repo found in both passes is one stat target, so the
    // gauge total is the real amount of work rather than a count that has to
    // be padded as duplicates are skipped. Primary wins the location.
    let mut targets: Vec<(discovery::FoundRepo, RepoLocation)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for f in &primary_found {
        if seen.insert(f.name.to_lowercase()) {
            targets.push((f.clone(), RepoLocation::Primary));
        }
    }
    for f in &elsewhere_found {
        if seen.insert(f.name.to_lowercase()) {
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
        let stale = stale_remotes(&targets, &active);
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(FETCH_CONCURRENCY));
        let mut set = tokio::task::JoinSet::new();
        for path in stale {
            let sem = sem.clone();
            set.spawn(async move {
                let _permit = sem.acquire().await;
                let outcome = git::fetch(&path).await;
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
        }
    }
    let mut repos: Vec<RepoStatus> = slots.into_iter().flatten().collect();
    let mut checked_names: HashSet<String> = seen;

    if let Some(tx) = progress {
        let _ = tx.send(AppEvent::Progress(app::ApplyProgress::Finishing));
    }

    // Pass 3: local repos (under repo_dir or any configured extra_dir, e.g.
    // ~/.dotfiles) with uncommitted or unpushed changes, not already shown
    // above. Runs *before* the not-cloned loop so a dirty local clone claims
    // the repo as LocalOnly rather than being mislabeled "not cloned" when
    // it lives at a hidden path passes 1/2 don't reach.
    let all_local = discovery::find_all_local(&scan_roots);
    let claimed_paths: HashSet<&Path> = targets.iter().map(|(f, _)| f.path.as_path()).collect();
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(probe_concurrency()));
    let mut set = tokio::task::JoinSet::new();
    for (i, (path, name)) in unclaimed_local(all_local, &claimed_paths, &checked_names)
        .into_iter()
        .enumerate()
    {
        // Glyph metadata by name from the full GitHub fetch, so a local-only
        // repo (e.g. dotfiles) still shows its org/private/public glyph.
        let repo_kind = kind_by_name
            .get(&name.to_lowercase())
            .copied()
            .unwrap_or(model::RepoKind::Unknown);
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire().await;
            (i, local_only_status(&path, name, repo_kind, cutoff).await)
        });
    }

    // Back into discovery order, then claim names: a clean repo never claims
    // one, so an active GitHub repo hidden from passes 1-2 still falls
    // through to the not-cloned list below, exactly as it did serially.
    let mut local_results: Vec<(usize, Option<RepoStatus>)> = Vec::new();
    while let Some(joined) = set.join_next().await {
        if let Ok(pair) = joined {
            local_results.push(pair);
        }
    }
    local_results.sort_by_key(|(i, _)| *i);
    for (_, status) in local_results {
        let Some(repo) = status else { continue };
        if !checked_names.insert(repo.name.to_lowercase()) {
            continue;
        }
        repos.push(repo);
    }

    // Not-cloned repos: active on GitHub but found nowhere locally (passes
    // 1-3 all missed them). Last, so a local clone always wins over this.
    for gh_repo in &active {
        if checked_names.contains(&gh_repo.name.to_lowercase()) {
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
    // The dataset only reaches back repo_limit repos; flag a window that
    // outruns it so an incomplete view never masquerades as complete.
    if let Some(c) = cutoff
        && github::window_exceeds_dataset(dataset, c, config.repo_limit)
    {
        full_header = format!(
            "{full_header} (window exceeds fetched repos, repo_limit={})",
            config.repo_limit
        );
    }

    Ok((repos, full_header))
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

    let gh_match = gh_repos
        .iter()
        .find(|r| r.name.eq_ignore_ascii_case(&found.name));

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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            }],
            calendar: None,
            has_pr_counts: true,
            origin: github::Origin::Network,
        }))
    }

    fn poll(app: &mut App, result: Result<Box<github::GithubFetch>, String>) {
        let result = result.map_err(|message| github::FetchError {
            failure: github::Failure::new(github::FailureKind::Http, message),
            fallback: None,
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
            })),
        );
        assert!(
            app.github_freshness.last_attempt.is_some(),
            "fresh data should not re-poll at once"
        );
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
                },
                RepoLocation::Elsewhere,
            ),
            (
                discovery::FoundRepo {
                    name: "b".to_string(),
                    path: dir.join("b"),
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
}
