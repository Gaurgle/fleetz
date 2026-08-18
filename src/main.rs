mod app;
mod cache;
mod cli;
mod config;
mod contributions;
mod discovery;
mod fuzzy;
mod git;
mod github;
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
use std::path::Path;
use std::process::Command as StdCommand;

/// How many commits/reflog entries to show in the detail pane's history
/// sections. Kept modest -- the pane has no scrolling, and these share it
/// with branches, CI, PRs, and stashes.
const RECENT_COMMIT_COUNT: u32 = 6;
const REFLOG_COUNT: u32 = 6;
const GRAPH_COMMIT_COUNT: u32 = 10;

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

/// Everything the event loop reacts to. Terminal input, the periodic tick,
/// and the results of background work all arrive on one channel, so the loop
/// never blocks on any single source.
pub enum AppEvent {
    Input(Event),
    Tick,
    Loaded(Result<Box<LoadResult>, String>),
    RepoUpdated {
        path: String,
        status: Box<RepoStatus>,
    },
    Github(Result<Box<github::GithubFetch>, String>),
    Progress(ApplyProgress),
    SweepDone {
        at: chrono::DateTime<chrono::Local>,
    },
}

/// Why a load ran, which decides what the loop does with the result.
pub enum LoadKind {
    /// First load at startup: no status message, nothing to announce.
    Initial,
    /// `R`: full rediscover, announces "Reloaded".
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
    let mut app = App::new(Vec::new(), String::new(), args.prs, term_width, term_height);
    // Config sets the default; the flags are one-session overrides. `w` still
    // toggles it live, and that choice is not persisted.
    app.watch_mode = resolve_watch(args.watch, args.no_watch, config.watch);
    app.refresh_interval = args.interval.unwrap_or(config.refresh_interval);
    app.window = initial_window;
    app.dirty_mode = mode == Mode::Dirty;
    app.loading = true;
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
            .map(|(repos, header, dataset, calendar)| {
                Box::new(LoadResult {
                    kind,
                    repos,
                    header,
                    dataset: Some(dataset),
                    github_calendar: calendar,
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
            })
        });
        let _ = tx.send(AppEvent::Loaded(result));
    });
}

/// Swap a finished load into the app, preserving UI state (selection is
/// clamped, groups re-applied), and say what happened.
fn apply_load_result(
    app: &mut App,
    config: &config::Config,
    result: Result<Box<LoadResult>, String>,
) {
    app.apply_progress = None;
    app.loading = false;
    match result {
        Ok(lr) => {
            app.repos = lr.repos;
            apply_groups(&mut app.repos, config);
            app.header = lr.header;
            if let Some(ds) = lr.dataset {
                app.dataset = ds;
                // This dataset just came from GitHub, so the watch re-poll
                // clock starts here rather than firing again immediately.
                app.last_github_poll = Some(chrono::Local::now());
            }
            app.clamp_after_reload();
            app.last_refreshed = Some(chrono::Local::now());
            match lr.kind {
                LoadKind::Initial => {}
                LoadKind::Reload => app.set_status("Reloaded".to_string(), false),
                LoadKind::WindowApply(w) => {
                    window_state::WindowState::new().save(&w);
                    let label = app.header.clone();
                    app.set_status(format!("Window: {label}"), false);
                }
            }
        }
        Err(e) => app.set_status(format!("Load failed: {e}"), true),
    }
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
    terminal
        .draw(|frame| ui::render(frame, app))
        .map_err(|e| e.to_string())?;

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
                let spinning = app.loading || (app.sweep_started.is_some() && app.sweep_manual);
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
                // A poll that never reported back must not wedge CI updates
                // for the rest of the session. `gh` has no timeout of its own,
                // so a dead network can leave the in-flight flag set forever.
                if let Some(started) = app.github_poll_started
                    && started.elapsed() > GITHUB_POLL_STUCK_AFTER
                {
                    app.github_poll_inflight = false;
                    app.github_poll_started = None;
                }
                if app.watch_mode && !app.dirty_mode && !app.loading && !app.github_poll_inflight {
                    let poll_every = github_poll_secs(app, config);
                    let due = app
                        .last_github_poll
                        .is_none_or(|t| (chrono::Local::now() - t).num_seconds() >= poll_every);
                    // A detected push jumps the queue: it is the one moment CI
                    // is about to change with nothing `Pending` yet to notice.
                    if due || app.push_detected {
                        app.push_detected = false;
                        start_github_poll(app, &tx, args, config);
                    }
                }
            }
            AppEvent::Input(Event::Key(key)) => {
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
                app.needs_redraw = true;
            }
            AppEvent::Progress(p) => {
                app.apply_progress = Some(p);
                app.needs_redraw = true;
            }
            AppEvent::RepoUpdated { path, status } => {
                if let Some(repo) = app
                    .repos
                    .iter_mut()
                    .find(|r| r.path.as_deref() == Some(path.as_str()))
                {
                    if repo_moved(repo, &status) {
                        app.push_detected = true;
                    }
                    *repo = *status;
                }
                app.needs_redraw = true;
            }
            AppEvent::Github(result) => {
                app.github_poll_inflight = false;
                app.github_poll_started = None;
                app.last_github_poll = Some(chrono::Local::now());
                match result {
                    Ok(fetch) => {
                        let github::GithubFetch {
                            repos: dataset,
                            calendar,
                        } = *fetch;
                        // The calendar rides the same payload as the repos, so
                        // a poll that updated CI also refreshed the year at no
                        // extra cost. A payload predating the field leaves the
                        // last good calendar in place rather than blanking it.
                        // App state for it lands with the pane itself; until
                        // then it is parsed and dropped rather than ignored.
                        let _ = calendar;
                        // Overwrite only the GitHub-owned fields: the local
                        // git state is the sweep's business, not this poll's.
                        for repo in &mut app.repos {
                            if let Some(gh) = dataset
                                .iter()
                                .find(|g| g.name.eq_ignore_ascii_case(&repo.name))
                            {
                                repo.pr_count = gh.pr_count;
                                repo.prs = gh.prs.clone();
                                repo.ci_status = gh.ci_status.clone();
                                repo.ci_workflow = gh.ci_workflow.clone();
                                repo.ci_duration = gh.ci_duration.clone();
                                repo.repo_kind = gh.repo_kind;
                            }
                        }
                        // One API call just told us which remotes moved, so
                        // fetch only those instead of waiting for the next
                        // blanket sweep to fetch all of them.
                        for idx in remotes_that_moved(&app.dataset, &dataset, &app.repos) {
                            start_single_sweep(app, &tx, idx, true, args.author.clone());
                        }
                        app.dataset = dataset;
                    }
                    Err(e) => app.set_status(format!("GitHub refresh failed: {e}"), true),
                }
                app.needs_redraw = true;
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
        if app.needs_redraw {
            app.needs_redraw = false;
            terminal
                .draw(|frame| ui::render(frame, app))
                .map_err(|e| e.to_string())?;
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
        }
        // Scroll the panel under the cursor without changing focus.
        MouseEventKind::ScrollDown => match panel {
            FocusedPanel::RepoList => app.move_down(),
            FocusedPanel::Detail => app.detail_nav_down(),
            FocusedPanel::Inspector => app.scroll_inspector(1),
            // A wheel over the strip walks the day cursor a week at a time,
            // which is the only axis a year-wide grid can scroll along.
            FocusedPanel::Contributions => {
                app.move_contrib_cursor(7, chrono::Local::now().date_naive())
            }
        },
        MouseEventKind::ScrollUp => match panel {
            FocusedPanel::RepoList => app.move_up(),
            FocusedPanel::Detail => app.detail_nav_up(),
            FocusedPanel::Inspector => app.scroll_inspector(-1),
            FocusedPanel::Contributions => {
                app.move_contrib_cursor(-7, chrono::Local::now().date_naive())
            }
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
                    let (interval, view, persist) =
                        (so.refresh_interval, so.view, so.persist_panes);
                    so.status = Some(format!("saved to {}", tilde_display(&path)));
                    // Live fields take effect immediately; the overlay stays
                    // open so the confirmation is actually readable.
                    app.refresh_interval = interval;
                    app.persist_pane_sizes = persist;
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
        // Shift+scroll: scroll the focused pane's *content* without moving the
        // selection cursor -- so you can hold one commit in the inspector while
        // scrolling the detail pane down into the history/reflog below. Plain
        // ↓/j (below) still drive the cursor; these are pure scroll. Shift+j/k
        // arrive as uppercase `J`/`K` (no modifier needed), Shift+arrow as an
        // arrow carrying the SHIFT modifier.
        KeyCode::Char('J') => app.scroll_focused(1),
        KeyCode::Char('K') => app.scroll_focused(-1),
        KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => app.scroll_focused(1),
        KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => app.scroll_focused(-1),
        KeyCode::Char('j') | KeyCode::Down => match app.focused_panel {
            FocusedPanel::RepoList => app.move_down(),
            FocusedPanel::Detail => app.detail_nav_down(),
            FocusedPanel::Inspector => app.scroll_inspector(1),
            FocusedPanel::Contributions => {
                app.move_contrib_cursor(7, chrono::Local::now().date_naive())
            }
        },
        KeyCode::Char('k') | KeyCode::Up => match app.focused_panel {
            FocusedPanel::RepoList => app.move_up(),
            FocusedPanel::Detail => app.detail_nav_up(),
            FocusedPanel::Inspector => app.scroll_inspector(-1),
            FocusedPanel::Contributions => {
                app.move_contrib_cursor(-7, chrono::Local::now().date_naive())
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
        // Panel focus: 1 = repo list, 2 = detail pane, 3 = inspector. Quick
        // filters moved to `#tag` words typed into search (see
        // app::parse_query) so plain digits are free for this.
        KeyCode::Char('1') => app.focus_panel(FocusedPanel::RepoList),
        KeyCode::Char('2') => app.focus_panel(FocusedPanel::Detail),
        KeyCode::Char('3') => app.focus_panel(FocusedPanel::Inspector),
        KeyCode::Char('0') => app.clear_filters(),
        // Branches overlay: switch (Enter) or clean up merged branches
        // (space to mark, d to delete). `b` and `s` both open it -- `s` for
        // the "switch" mnemonic the pull-removal left room for.
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
            start_github_poll(app, tx, args, config);
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
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
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
    /// Existing status, carried so GitHub-only fields survive the rebuild.
    old: Box<RepoStatus>,
}

/// A sweep that never reported back must not hold the watch lock forever.
const SWEEP_STUCK_AFTER: std::time::Duration = std::time::Duration::from_secs(180);

/// Same idea for the GitHub poll. `gh` carries no timeout of its own, so a
/// black-holed request would otherwise leave `github_poll_inflight` set for
/// the rest of the session and stop CI updating, silently.
const GITHUB_POLL_STUCK_AFTER: std::time::Duration = std::time::Duration::from_secs(60);

/// Floor on the GitHub poll interval, whatever the config says. Measured
/// against a 5000 point/hour GraphQL budget, one poll of ~50 repos costs a
/// few points, so this is comfortable; it exists to stop a typo turning the
/// dashboard into a rate-limit incident.
const GITHUB_POLL_MIN: u64 = 15;

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
fn github_poll_secs(app: &App, config: &config::Config) -> i64 {
    let configured = config.github_poll_interval.max(GITHUB_POLL_MIN);
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
    args: &Args,
    config: &config::Config,
) {
    if app.github_poll_inflight || app.dirty_mode {
        return;
    }
    app.github_poll_inflight = true;
    app.github_poll_started = Some(std::time::Instant::now());
    spawn_github_poll(tx, args, config);
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
    spawn_sweep(tx.clone(), targets, do_fetch, author, true);
}

/// Sweep a single repo (the `f` fetch and the post-branch-switch refresh).
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
    spawn_sweep(tx.clone(), targets, do_fetch, author, false);
}

/// The shared sweep body: fetch (capped) then rebuild (capped), streaming
/// each repo back as it lands so the list updates progressively.
fn spawn_sweep(
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    targets: Vec<SweepTarget>,
    do_fetch: bool,
    author: Option<String>,
    report_done: bool,
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
            set.spawn(async move {
                if do_fetch {
                    let _permit = fetch_sem.acquire().await;
                    git::fetch(&target.fs_path).await;
                }
                let _permit = probe_sem.acquire().await;
                let status = rebuild_repo_status(&target, author.as_deref()).await;
                let _ = tx.send(AppEvent::RepoUpdated {
                    path: target.display_path.clone(),
                    status: Box::new(status),
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

/// Full per-repo rebuild, preserving GitHub-only fields from the old status.
/// Two-tier: the cheap probes always run, the expensive ones only when the
/// repo's WorkKey says something actually moved.
async fn rebuild_repo_status(target: &SweepTarget, author: Option<&str>) -> RepoStatus {
    let path = &target.fs_path;
    let old = &target.old;

    // Cheap tier: always current. Untracked files never touch the WorkKey,
    // so dirty_count must not be skipped.
    let branches = git::branch_statuses(path).await;
    let dirty = git::dirty_count(path).await;
    let remote = git::remote_branches(path).await;
    let branch_names: Vec<String> = branches.iter().map(|b| b.name.clone()).collect();
    let (total_behind, total_ahead) = branches
        .iter()
        .fold((0u32, 0u32), |(b, a), s| (b + s.behind, a + s.ahead));

    let key = git::work_key(path).await;
    let unchanged = key.is_some() && key == old.work_key;

    let (commits, reflog, graph, stash_entries, worktrees) = if unchanged {
        (
            old.commits.clone(),
            old.reflog.clone(),
            old.graph.clone(),
            old.stash_entries.clone(),
            old.worktrees.clone(),
        )
    } else {
        (
            git::commits(path, &format!("-{RECENT_COMMIT_COUNT}"), true, author).await,
            git::reflog(path, REFLOG_COUNT).await,
            git::graph(path, &branch_names, GRAPH_COMMIT_COUNT).await,
            git::stash_list(path).await,
            git::worktree_list(path).await,
        )
    };

    RepoStatus {
        branches,
        dirty,
        behind: total_behind,
        ahead: total_ahead,
        remote_only_branches: git::unfetched_branches(&remote, &branch_names),
        stash_count: stash_entries.len() as u32,
        stash_entries,
        commits,
        reflog,
        graph,
        worktrees,
        work_key: key,
        ..(*old.clone())
    }
}

/// Re-poll GitHub for PR and CI data in the background, so badges stop being
/// frozen at whatever they were when fleetz launched.
fn spawn_github_poll(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    args: &Args,
    config: &config::Config,
) {
    let tx = tx.clone();
    let repo_limit = config.repo_limit;
    let include_prs = args.prs;
    let cache = cache::ApiCache::new(config.cache_ttl, repo_limit, include_prs);
    tokio::spawn(async move {
        let result = github::fetch_repos(repo_limit, include_prs, &cache, true).await;
        let _ = tx.send(AppEvent::Github(result.map(Box::new)));
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
    ),
    String,
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
        return Ok((repos, header, Vec::new(), None));
    }

    // Fetch from GitHub (with caching)
    let api_cache = cache::ApiCache::new(config.cache_ttl, config.repo_limit, args.prs);
    let fetch = fetch_dataset(config, args, &api_cache, bypass_cache).await?;
    let (dataset, calendar) = (fetch.repos, fetch.calendar);

    let (cutoff, header) = resolve_cutoff(window, mode, config)?;
    let (repos, full_header) =
        assemble_view(&dataset, cutoff, header, args, config, repo_dir, true, None).await?;

    Ok((repos, full_header, dataset, calendar))
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
    let commits = git::commits(path, &format!("-{RECENT_COMMIT_COUNT}"), true, author).await;
    let reflog = git::reflog(path, REFLOG_COUNT).await;
    let branch_names: Vec<String> = branches.iter().map(|b| b.name.clone()).collect();
    let graph = git::graph(path, &branch_names, GRAPH_COMMIT_COUNT).await;
    let stash_entries = git::stash_list(path).await;
    let stash_count = stash_entries.len() as u32;
    let worktrees = git::worktree_list(path).await;
    let remote_url = git::remote_url(path).await;
    let remote = git::remote_branches(path).await;
    let remote_only_branches = git::unfetched_branches(&remote, &branch_names);

    Some(RepoStatus {
        // No local calendar until the strip asks for one.
        daily: None,
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
        repo_kind: model::RepoKind::Unknown,
        owner: None,
        remote_url,
        remote_only_branches,
        work_key: None,
    })
}

/// The GitHub GraphQL fetch (with disk cache), unfiltered: the top
/// repo_limit repos by push date.
async fn fetch_dataset(
    config: &config::Config,
    args: &Args,
    cache: &cache::ApiCache,
    bypass_cache: bool,
) -> Result<github::GithubFetch, String> {
    github::fetch_repos(config.repo_limit, args.prs, cache, bypass_cache).await
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

    let active_names: Vec<String> = active.iter().map(|r| r.name.clone()).collect();

    // Pass 1: find under every scan root (repo_dir + extra_dirs)
    let primary_found = discovery::find_in_primary(&scan_roots, &active_names);
    let found_set: HashSet<String> = primary_found
        .iter()
        .map(|f| f.name.to_lowercase())
        .collect();

    // Pass 2: find elsewhere
    let home = dirs::home_dir().unwrap_or_default();
    let elsewhere_found = discovery::find_elsewhere(&home, &scan_roots, &active_names, &found_set);

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

    if let Some(tx) = progress {
        let _ = tx.send(AppEvent::Progress(app::ApplyProgress::Discovering));
        let _ = tx.send(AppEvent::Progress(app::ApplyProgress::Statting {
            done: 0,
            total: targets.len(),
        }));
    }

    if git_fetch {
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(FETCH_CONCURRENCY));
        let mut set = tokio::task::JoinSet::new();
        for (found, _) in &targets {
            let path = found.path.clone();
            let sem = sem.clone();
            set.spawn(async move {
                let _permit = sem.acquire().await;
                git::fetch(&path).await
            });
        }
        while set.join_next().await.is_some() {}
    }

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
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(probe_concurrency()));
    let mut set = tokio::task::JoinSet::new();
    for (i, path) in all_local.into_iter().enumerate() {
        // Undotted, so a hidden clone dedupes against the GitHub name passes
        // 1-2 registered it under rather than showing a second `.foo` row.
        let name = discovery::repo_name_for_path(&path);
        if checked_names.contains(&name.to_lowercase()) {
            continue;
        }
        // Glyph metadata by name from the full GitHub fetch, so a local-only
        // repo (e.g. dotfiles) still shows its org/private/public glyph.
        let repo_kind = kind_by_name
            .get(&name.to_lowercase())
            .copied()
            .unwrap_or(model::RepoKind::Unknown);
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire().await;
            (i, local_only_status(&path, name, repo_kind).await)
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
            repo_kind: gh_repo.repo_kind,
            owner: gh_repo.owner.clone(),
            remote_url: None,
            remote_only_branches: vec![],
            work_key: None,
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
async fn local_only_status(
    path: &Path,
    name: String,
    repo_kind: model::RepoKind,
) -> Option<RepoStatus> {
    let dirty = git::dirty_count(path).await;
    let branches = git::branch_statuses(path).await;
    let total_ahead: u32 = branches.iter().map(|b| b.ahead).sum();

    if dirty == 0 && total_ahead == 0 {
        return None;
    }

    let display = tilde_display(path);
    let remote_url = git::remote_url(path).await;
    let remote = git::remote_branches(path).await;
    let branch_names: Vec<String> = branches.iter().map(|b| b.name.clone()).collect();
    let remote_only_branches = git::unfetched_branches(&remote, &branch_names);

    Some(RepoStatus {
        // No local calendar until the strip asks for one.
        daily: None,
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
        repo_kind,
        // No GitHub match, so the remote URL is the only identity available;
        // `owner_display` parses it at render time.
        owner: None,
        remote_url,
        remote_only_branches,
        work_key: None,
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
    let branches = git::branch_statuses(path).await;
    let dirty = git::dirty_count(path).await;

    let mut total_behind = 0u32;
    let mut total_ahead = 0u32;
    for b in &branches {
        total_behind += b.behind;
        total_ahead += b.ahead;
    }

    let author = args.author.as_deref();

    // Recent history on HEAD, not just the divergent commits -- a glance at
    // "what's been happening here" shouldn't depend on being ahead/behind.
    // Always with the diffstat so every commit shows its +/- and file list.
    let commits = git::commits(path, &format!("-{RECENT_COMMIT_COUNT}"), true, author).await;
    let reflog = git::reflog(path, REFLOG_COUNT).await;

    let branch_names: Vec<String> = branches.iter().map(|b| b.name.clone()).collect();
    let graph = git::graph(path, &branch_names, GRAPH_COMMIT_COUNT).await;

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
    let remote = git::remote_branches(path).await;
    let remote_only_branches = git::unfetched_branches(&remote, &branch_names);

    RepoStatus {
        // No local calendar until the strip asks for one.
        daily: None,
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
        repo_kind,
        owner,
        remote_url,
        remote_only_branches,
        // Taken last, after every probe above, so the first sweep that finds
        // it unchanged can safely reuse all of them.
        work_key: git::work_key(path).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_repo() -> RepoStatus {
        RepoStatus {
            daily: None,
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
            repo_kind: model::RepoKind::Unknown,
            owner: None,
            remote_url: None,
            remote_only_branches: vec![],
            work_key: None,
        }
    }

    fn key(head: &str) -> git::WorkKey {
        git::WorkKey {
            head_sha: head.to_string(),
            index_mtime: None,
            index_size: 0,
            stash_size: 0,
        }
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
        let mut app = App::new(Vec::new(), String::new(), false, 100, 40);
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
}
