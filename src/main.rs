mod app;
#[cfg(test)]
mod architecture;
mod cache;
mod ci;
mod cli;
mod clipboard;
mod config;
mod contributions;
mod digest;
mod discovery;
mod engine;
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
use engine::{
    FETCH_CONCURRENCY, FetchAsk, HISTORY_COUNT, LoadError, SweepTarget, ViewNotes,
    activity_position, assemble_view, carries, disk_usage_of, fetch_under_claim_capped, load_repos,
    names_remote, probe_concurrency, resolve_cutoff, size_held_rows, tilde_display,
};
use model::{RepoLocation, RepoStatus};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;
use std::process::Command as StdCommand;

/// How long a footer status message ("Refreshed", "Watch off", …) stays before
/// it auto-dismisses back to the summary.
const STATUS_TTL: std::time::Duration = std::time::Duration::from_millis(1500);

/// Browser-opener binary: macOS ships `open`, everything else gets xdg-open.
#[cfg(target_os = "macos")]
const OPENER: &str = "open";
#[cfg(not(target_os = "macos"))]
const OPENER: &str = "xdg-open";

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

/// Whether an animation is on screen whose frames `should_paint` must not
/// hold: the loading field, or a live copy flash (FZ-81).
fn is_animating(app: &App) -> bool {
    app.loading_field.is_some() || app.yank_flash.is_active()
}

/// Everything the event loop reacts to. Terminal input, the periodic tick,
/// and the results of background work all arrive on one channel, so the loop
/// never blocks on any single source.
pub enum AppEvent {
    Input(Event),
    Tick,
    /// The copy flash's next frame is due (FZ-81): the loop's own wait ran
    /// out while the flash is live. Comes from `next_event`, not the channel.
    YankFrame,
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
    /// A `worktrees` view log read of one worktree (FZ-77), applied by
    /// `App::apply_worktree_log`.
    WorktreeLogRead {
        read: app::WorktreeLogRead,
        commits: Vec<model::Commit>,
    },
    /// A `notes` view walk of one repo (FZ-53), applied by
    /// `App::apply_notes_walk`.
    NotesWalked {
        walk: app::NotesWalk,
        result: Result<notes::Notes, String>,
    },
    /// A `changes` view read of one repo (FZ-60), applied by
    /// `App::apply_changes_read`.
    ChangesRead {
        read: app::ChangesRead,
        result: Result<git::Changes, String>,
    },
    /// The highlighted changed file's diff, applied by
    /// `App::apply_change_diff`.
    ChangeDiff {
        read: app::DiffRead,
        text: Result<String, String>,
    },
    /// The highlighted note's body and its metadata for the facts, applied by
    /// `App::apply_note_read` and `App::apply_note_facts`, and a markdown
    /// body rendered at the read's `render_width` (FZ-59), applied by
    /// `App::apply_note_rendered` in the same turn so the reader never
    /// draws the source first.
    NoteRead {
        read: app::NoteRead,
        text: Result<String, String>,
        facts: notes::NoteFacts,
        rendered: Option<app::RenderedNote>,
    },
    /// The held markdown body rendered again at a new reader width
    /// (`App::claim_note_render`).
    NoteRendered {
        render: app::NoteRender,
        rendered: app::RenderedNote,
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

impl From<ApplyProgress> for AppEvent {
    fn from(progress: ApplyProgress) -> Self {
        AppEvent::Progress(progress)
    }
}

impl From<(String, git::FetchProgress)> for AppEvent {
    fn from((path, progress): (String, git::FetchProgress)) -> Self {
        AppEvent::FetchProgress { path, progress }
    }
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

/// The loop's next event. While a copy flash is live (FZ-81) the wait is cut
/// to its next frame (`YankFlash::wait`) and a wait that runs out is a
/// `YankFrame`; otherwise the loop waits on the channel alone, as before.
async fn next_event(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    flash: &app::YankFlash,
) -> Option<AppEvent> {
    if !flash.is_active() {
        return rx.recv().await;
    }
    let wait = flash.wait(TICK_INTERVAL, std::time::Instant::now());
    tokio::time::timeout(wait, rx.recv())
        .await
        .unwrap_or(Some(AppEvent::YankFrame))
}

/// End the copy flash on the input that ends it in notez: a key press, a
/// click or a scroll, a paste. Pointer motion, resize, focus and key releases
/// leave it running. Called before the input is handled, so the `y` that
/// finishes a new chord starts a fresh flash.
fn end_yank_flash_on(app: &mut App, input: &Event) {
    let ends = match input {
        Event::Key(key) => key.kind == crossterm::event::KeyEventKind::Press,
        Event::Mouse(mouse) => mouse.kind != MouseEventKind::Moved,
        Event::Paste(_) => true,
        Event::Resize(..) | Event::FocusGained | Event::FocusLost => false,
    };
    if ends && app.yank_flash.is_active() {
        app.yank_flash.clear();
        app.needs_redraw = true;
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

    while let Some(ev) = next_event(&mut rx, &app.yank_flash).await {
        if let AppEvent::Input(input) = &ev {
            end_yank_flash_on(app, input);
        }
        match ev {
            AppEvent::YankFrame => {}
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
                if let Some(read) = app.claim_worktree_log(instant) {
                    spawn_worktree_log(&tx, read);
                }
                if let Some(walk) = app.claim_notes_walk(instant) {
                    spawn_notes_walk(&tx, walk);
                }
                if let Some(read) = app.claim_note_read() {
                    spawn_note_read(&tx, read);
                }
                if let Some(read) = app.claim_changes_read(instant) {
                    spawn_changes_read(&tx, read);
                }
                if let Some(read) = app.claim_change_diff() {
                    spawn_change_diff(&tx, read);
                }
                if let Some(render) = app.claim_note_render() {
                    spawn_note_render(&tx, render);
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
            AppEvent::SweepDone { at } => apply_sweep_done(app, at),
            AppEvent::Ci(answer) => app.apply_ci(*answer, chrono::Local::now()),
            AppEvent::CiDetail(answer) => app.apply_ci_detail(*answer, chrono::Local::now()),
            AppEvent::WorktreeLogRead { read, commits } => app.apply_worktree_log(read, commits),
            AppEvent::NotesWalked { walk, result } => app.apply_notes_walk(walk, result),
            AppEvent::ChangesRead { read, result } => app.apply_changes_read(read, result),
            AppEvent::ChangeDiff { read, text } => app.apply_change_diff(read, text),
            AppEvent::NoteRead {
                read,
                text,
                facts,
                rendered,
            } => {
                app.apply_note_facts(&read, facts);
                if let Some(rendered) = rendered {
                    app.apply_note_rendered(&read.path, read.generation, rendered);
                }
                app.apply_note_read(read, text);
            }
            AppEvent::NoteRendered { render, rendered } => {
                app.apply_note_rendered(&render.path, render.generation, rendered);
            }
            AppEvent::NoteCommit { read, last } => app.apply_note_commit(&read, last),
            AppEvent::DigestLog { path, key, commits } => {
                app.apply_digest_log(&path, &key, commits)
            }
        }

        if app.should_quit {
            return Ok(());
        }
        app.needs_redraw |= app.yank_flash.tick(std::time::Instant::now());
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
            let animating = is_animating(app);
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
    // A press ends a pending yank chord (FZ-74), as a key does: a click can
    // move the cursor, and a `p` after it must not copy from the new row.
    if matches!(m.kind, MouseEventKind::Down(_)) {
        app.pending_yank = false;
    }
    let Ok(size) = terminal.size() else { return };
    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
    handle_mouse_in(app, area, m);
}

/// `handle_mouse_input` against a known screen `area`, so a test can drive
/// the real mouse handling without a terminal.
fn handle_mouse_in(app: &mut App, area: ratatui::layout::Rect, m: MouseEvent) {
    if !matches!(app.overlay, Overlay::None) || app.search_active {
        return;
    }
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
            // The notes tree is a list to pick a file from, as pane 1 is a
            // list to pick a repo from: the wheel walks its cursor and the
            // view follows with the scrolloff margin (FZ-65). Scrolling the
            // view alone left the highlight behind, off the drawn rows of a
            // short tree within one notch.
            FocusedPanel::Detail if app.notes_layout() => app.wheel_notes_tree(true),
            FocusedPanel::Detail => app.scroll_detail(DETAIL_WHEEL_ROWS),
            FocusedPanel::Inspector => app.scroll_inspector(1),
            FocusedPanel::Reader => app.scroll_reader(1),
            FocusedPanel::Contributions => {}
        },
        MouseEventKind::ScrollUp => match panel {
            FocusedPanel::RepoList => app.move_up(),
            FocusedPanel::Detail if app.notes_layout() => app.wheel_notes_tree(false),
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
/// and writes every field back to `config.toml`, Esc discards. The icons row
/// is the one exception to "nothing applies before Enter": stepping it sets
/// the thread-local glyph set at once, so the next frame draws it, and Esc
/// puts the set in force back.
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
            // The icons row previews as it is stepped; discarding puts the
            // set in force back.
            ui::badges::set_active_icons(so.icons_in_force);
            app.overlay = Overlay::None;
        }
        KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => so.move_cursor(1),
        KeyCode::Char('k') | KeyCode::Up => so.move_cursor(-1),
        KeyCode::Char('l') | KeyCode::Right => {
            so.step(1);
            ui::badges::set_active_icons(so.icons);
        }
        KeyCode::Char('h') | KeyCode::Left => {
            so.step(-1);
            ui::badges::set_active_icons(so.icons);
        }
        KeyCode::Char(c) if c.is_ascii_digit() => so.push_digit(c as u8 - b'0'),
        KeyCode::Enter => {
            so.normalize();
            let updates = so.config_updates();
            let path = config::settings_path();
            match config::save_settings(&path, &updates) {
                Ok(()) => {
                    so.status = Some(format!("saved to {}", tilde_display(&path)));
                    so.icons_in_force = so.icons;
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
    // `yy` and `yp` (FZ-74): a `y`, and the key after one, go to the chord
    // first; a key that does not finish it falls through to its own action.
    let plain = match key.code {
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            Some(c)
        }
        _ => None,
    };
    if app.press_yank_key(plain, clipboard::copy) {
        return Ok(());
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
        // From the tree or the reader, `p` flips rendered markdown and its
        // source (FZ-59); `r` still refreshes there, below.
        KeyCode::Char('p') if app.reader_pages() => app.toggle_reader_raw(),
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
        // The `changes` view's second section: upstream or default branch.
        KeyCode::Char('u') => app.toggle_changes_base(),
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
        // On a file in the `notes` view, $EDITOR on that file (FZ-53), or
        // `todoz` on a board (FZ-63).
        KeyCode::Enter if note_to_open(app).is_some() => {
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
            let todoz = std::env::var_os("PATH").is_some_and(|p| on_path("todoz", &p));
            let opener = note_opener(app, todoz, &editor);
            if let Some(o) = &opener {
                let args: Vec<&str> = o.args.iter().map(String::as_str).collect();
                match &o.dir {
                    Some(dir) => {
                        suspend_and_run_in_dir(terminal, &o.program, &args, &dir.to_string_lossy())?
                    }
                    None => suspend_and_run(terminal, &o.program, &args)?,
                }
            }
            app.invalidate_notes();
            app.invalidate_changes();
            if opener.is_some_and(|o| o.fell_back) && !app.todoz_fallback_said {
                app.todoz_fallback_said = true;
                app.set_status(
                    "todoz not on PATH: board opened in $EDITOR".to_string(),
                    false,
                );
            }
        }
        // On a file in the `changes` view, $EDITOR on that file (FZ-60).
        KeyCode::Enter if change_to_open(app).is_some() => {
            let file = change_to_open(app).unwrap_or_default();
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
            suspend_and_run(terminal, &editor, &[&file.to_string_lossy()])?;
            app.invalidate_notes();
            app.invalidate_changes();
        }
        // On an other context in the `ci` view, its page (FZ-83).
        KeyCode::Enter if context_url(app).is_some() => {
            let url = context_url(app).unwrap_or_default();
            let _ = StdCommand::new(OPENER).arg(&url).status();
        }
        // On a header, Enter goes to the digest line's repo row (FZ-45).
        KeyCode::Enter if app.on_header().is_some() => {
            app.jump_to_digest_line(chrono::Local::now())
        }
        KeyCode::Enter => {
            // Open lazygit in the selected repo, or the highlighted worktree
            if let Some(expanded) = app.action_dir() {
                suspend_and_run(terminal, "lazygit", &["-p", &expanded])?;
                app.invalidate_notes();
                app.invalidate_changes();
            }
        }
        KeyCode::Char('e') => {
            // Open $EDITOR in the selected repo, or the highlighted worktree
            if let Some(expanded) = app.action_dir() {
                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
                suspend_and_run(terminal, &editor, &[&expanded])?;
                app.invalidate_notes();
                app.invalidate_changes();
            }
        }
        // The `notes` view's tree: notes directories only, or every
        // markdown file. Nothing elsewhere.
        KeyCode::Char('m') => app.toggle_notes_markdown(),
        KeyCode::Char('o') => {
            // Open a shell in the repo dir, or the highlighted worktree's
            if let Some(expanded) = app.action_dir() {
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
                suspend_and_run_in_dir(terminal, &shell, &[], &expanded)?;
                app.invalidate_notes();
                app.invalidate_changes();
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
        KeyCode::Char('<' | '>') => {
            // The step starts from the list width on screen (FZ-69), which
            // depends on the main area's width.
            if let Ok(size) = terminal.size() {
                let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
                app.main_area_width
                    .set(ui::panel_rects(area, app).body.width);
            }
            let delta = if key.code == KeyCode::Char('<') {
                -1
            } else {
                1
            };
            app.resize_repo_list(delta);
        }
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
/// while pane 2 or the reader has focus. `None` leaves `Enter` to
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

/// The file `Enter` opens in $EDITOR in the `changes` view: the highlighted
/// file, while pane 2 or the inspector has focus. `None` leaves `Enter` to
/// open lazygit on the repo.
fn change_to_open(app: &App) -> Option<std::path::PathBuf> {
    let focused = matches!(
        app.focused_panel,
        FocusedPanel::Detail | FocusedPanel::Inspector
    );
    (app.changes_wanted() && focused)
        .then(|| app.change_to_open())
        .flatten()
}

/// What `Enter` suspends to on the `notes` view's highlighted file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NoteOpener {
    program: String,
    args: Vec<String>,
    /// Where the program runs; `None` keeps fleetz's directory.
    dir: Option<std::path::PathBuf>,
    /// A board todoz could open went to the editor: `todoz` is not on PATH.
    fell_back: bool,
}

/// The scope flags that make `todoz`, started in the repo root, open
/// exactly `file`'s board, or `None` for a board it cannot name. notez
/// resolves a scope from the git toplevel (`notez-core` `resolve::root`):
/// no flag is `Public`, `<repo>/notez/TODO.md`; `-l` is `<repo>/.notez/
/// TODO.md`; `-p` is `~/notez/personal/<project>/TODO.md`. A positional
/// argument is never passed: `todoz <text>` quick-adds a task.
fn todoz_scope(file: &notes::NoteFile) -> Option<&'static [&'static str]> {
    if file.kind() != notes::NoteKind::Todo {
        return None;
    }
    if file.is_personal() {
        return Some(&["-p"]);
    }
    match file.rel.to_str()? {
        "notez/TODO.md" => Some(&[]),
        ".notez/TODO.md" => Some(&["-l"]),
        _ => None,
    }
}

/// The program `Enter` runs on the highlighted note (FZ-63): `todoz` in the
/// repo root with the scope flag of a board it can open (`todoz_scope`),
/// read-only here and edited there; `editor` on the file for every other
/// note, every other board, and a todoz board when `todoz_on_path` is
/// false.
fn note_opener(app: &App, todoz_on_path: bool, editor: &str) -> Option<NoteOpener> {
    let file = note_to_open(app)?;
    let scope = app.notes_highlighted_file().and_then(todoz_scope);
    let root = app
        .selected_notes()
        .map(|(p, _)| std::path::PathBuf::from(shellexpand::tilde(p).as_ref()));
    Some(match (scope, todoz_on_path, root) {
        (Some(flags), true, Some(root)) => NoteOpener {
            program: "todoz".to_string(),
            args: flags.iter().map(|f| f.to_string()).collect(),
            dir: Some(root),
            fell_back: false,
        },
        _ => NoteOpener {
            program: editor.to_string(),
            args: vec![file.to_string_lossy().into_owned()],
            dir: None,
            fell_back: scope.is_some() && !todoz_on_path,
        },
    })
}

/// Whether an executable `name` is in one of `path`'s directories.
fn on_path(name: &str, path: &std::ffi::OsStr) -> bool {
    std::env::split_paths(path).any(|dir| {
        let candidate = dir.join(name);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            candidate
                .metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        }
        #[cfg(not(unix))]
        {
            candidate.is_file()
        }
    })
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

/// The target url of the other context under the `ci` view's cursor
/// (FZ-83), which `Enter` and `O` open; `None` off those rows or for a
/// context without one.
fn context_url(app: &App) -> Option<String> {
    if !ci_handoff_applies(app) {
        return None;
    }
    app.highlighted_ci_context()?.url.clone()
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
    if key == RunKey::Open
        && let Some(url) = context_url(app)
    {
        let _ = StdCommand::new(OPENER).arg(&url).status();
        return Ok(());
    }
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
            // After the indices above are spent: this moves rows.
            order_rows_by_activity(app);
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
    // A watch sweep already running holds the sweep lock, so none was
    // started: its rows are the ones this refresh waits for (FZ-85).
    if app.sweep_started.is_some() {
        app.sweep_manual = true;
    }
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

/// Read a worktree's newest commits for the `worktrees` inspector off the
/// UI thread (FZ-77): one bounded `git log` on `HEAD` in that directory.
fn spawn_worktree_log(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    read: app::WorktreeLogRead,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let commits = git::branch_commits(&read.dir, "HEAD", app::WORKTREE_LOG_COUNT).await;
        let _ = tx.send(AppEvent::WorktreeLogRead { read, commits });
    });
}

/// Walk a repo for the `notes` view off the UI thread and send the tree back.
fn spawn_notes_walk(tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>, walk: app::NotesWalk) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let root = walk.root.clone();
        let home = std::path::PathBuf::from(shellexpand::tilde("~").as_ref());
        let result = tokio::task::spawn_blocking(move || {
            notes::walk(&root).map(|n| n.with_personal(&root, notes::personal_board(&home, &root)))
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r.map_err(|e| e.to_string()));
        let _ = tx.send(AppEvent::NotesWalked { walk, result });
    });
}

/// Read a repo's `changes` off the UI thread and send them back (FZ-60).
fn spawn_changes_read(tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>, read: app::ChangesRead) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let result = git::changes(&read.root, read.base).await;
        let _ = tx.send(AppEvent::ChangesRead { read, result });
    });
}

/// Read the highlighted changed file's diff off the UI thread and send it
/// back (FZ-60).
fn spawn_change_diff(tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>, read: app::DiffRead) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let text = git::changed_file_diff(&read.root, read.base.as_deref(), &read.file).await;
        let _ = tx.send(AppEvent::ChangeDiff { read, text });
    });
}

/// Read the highlighted note off the UI thread and send its body back with
/// its metadata for the facts (FZ-62), then its last commit from a bounded
/// `git log`, "untracked" when git prints nothing. Two answers, body first:
/// for a file git never saw it walks the whole history, and the reader
/// shows the body without waiting for that. A markdown body is rendered in
/// the same blocking task at the read's `render_width` (FZ-59): a capped
/// file costs more than a keypress frame.
fn spawn_note_read(tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>, read: app::NoteRead) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let path = read.path.clone();
        let render_width = read.render_width;
        let read_both = tokio::task::spawn_blocking(move || {
            let text = notes::read_body(&path);
            let rendered = match (&text, render_width) {
                (Ok(body), Some(width)) => Some(render_note(body, width)),
                _ => None,
            };
            (text, notes::file_facts(&path), rendered)
        })
        .await;
        let (text, facts, rendered) = match read_both {
            Ok(answer) => answer,
            Err(e) => (Err(e.to_string()), notes::NoteFacts::default(), None),
        };
        let _ = tx.send(AppEvent::NoteRead {
            read: read.clone(),
            text,
            facts,
            rendered,
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

/// `body` as the reader draws it rendered at `width` (FZ-59).
/// A body `notes::read_body` cut at the cap ends in its marker line, which
/// is not the note's markdown: it is rendered without it and drawn after
/// it, plain, as the raw reader draws it, so an open fence or table cannot
/// swallow it.
fn render_note(body: &str, width: u16) -> app::RenderedNote {
    let marker = format!("… cut at {} KiB", notes::BODY_CAP / 1024);
    let suffix = format!("\n{marker}");
    let (markdown, cut) = match body.strip_suffix(&suffix) {
        Some(markdown) => (markdown, true),
        None => (body, false),
    };
    let mut lines = ui::markdown::render_markdown(markdown, width);
    if cut {
        let plain = ratatui::style::Style::default().fg(ui::colors::TEXT);
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            marker, plain,
        )));
    }
    app::RenderedNote { width, lines }
}

/// Render the held markdown body again at a new reader width, off the UI
/// thread, and send it back (`App::claim_note_render`).
fn spawn_note_render(tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>, render: app::NoteRender) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let rendered = tokio::task::spawn_blocking(move || {
            let rendered = render_note(&render.text, render.width);
            (render, rendered)
        })
        .await;
        if let Ok((render, rendered)) = rendered {
            let _ = tx.send(AppEvent::NoteRendered { render, rendered });
        }
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

/// Re-order the rows by activity after a poll or a sweep lands (FZ-86), so
/// a repo pushed since launch rises within its section as a fresh launch
/// would put it. Before this the order was set once, by `sort_by_activity`
/// at load, and a refresh or a watch poll only updated rows in place.
///
/// The key restates how `assemble_view` lays the rows out, so a re-sort
/// moves nothing a launch on the same dataset would not: passes 1 and 2 by
/// `activity_position`, then pass 3's rows in discovery order (a stable
/// sort on an equal key keeps it), then the not-cloned rows by activity.
/// `App::visible_repos` groups that into sections, as at launch.
fn order_rows_by_activity(app: &mut App) {
    let keys: Vec<(u8, usize)> = app
        .repos
        .iter()
        .map(|repo| match repo.location {
            RepoLocation::Primary | RepoLocation::Elsewhere => (
                0,
                activity_position(repo.owner.as_deref(), &repo.name, &app.dataset),
            ),
            RepoLocation::LocalOnly => (1, 0),
            RepoLocation::NotCloned => (
                2,
                activity_position(repo.owner.as_deref(), &repo.name, &app.dataset),
            ),
        })
        .collect();
    app.reorder_repos(keys);
}

/// `AppEvent::SweepDone`: close the sweep's books, then apply the order. A
/// sweep changes no row's activity, but an order a search held back
/// (`App::reorder_repos`) lands here at the latest.
fn apply_sweep_done(app: &mut App, at: chrono::DateTime<chrono::Local>) {
    app.sweep_landed(at);
    order_rows_by_activity(app);
}

#[cfg(test)]
mod tests;

/// FZ-45: section headers as cursor stops and the slot digest, driven
/// through the key handler and checked against what the renderer draws.
#[cfg(test)]
mod digest_handler_tests;
