use crate::app::{App, ApplyProgress};
use crate::cli::Args;
use crate::engine::LoadError;
use crate::input::{handle_key, handle_mouse_input};
use crate::load::apply_load_result;
use crate::model::RepoStatus;
use crate::poll::{
    GITHUB_POLL_STUCK_AFTER, apply_github_poll, ci_tick, expire_stuck_github_poll, github_poll_due,
    spawn_change_diff, spawn_changes_read, spawn_ci_detail, spawn_ci_poll, spawn_digest_log,
    spawn_note_read, spawn_note_render, spawn_notes_walk, spawn_worktree_log,
    start_account_rebuild, start_github_poll,
};
use crate::sweep::{SWEEP_STUCK_AFTER, apply_sweep_done, land_repo_update, start_sweep};
use crate::{app, ci, config, contributions, digest, git, github, model, notes, time, ui};
use crossterm::event::{self, Event, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io;
use std::path::Path;

/// How long a footer status message ("Refreshed", "Watch off", …) stays before
/// it auto-dismisses back to the summary.
const STATUS_TTL: std::time::Duration = std::time::Duration::from_millis(1500);

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
pub(crate) fn with_input_paused<T>(body: impl FnOnce() -> T) -> T {
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
pub(crate) fn spawn_input_thread(tx: tokio::sync::mpsc::UnboundedSender<AppEvent>) {
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
pub(crate) fn spawn_tick_task(tx: tokio::sync::mpsc::UnboundedSender<AppEvent>) {
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

/// The event loop: one channel carries input, ticks, and the results of every
/// background task, so nothing the user does ever waits on git or the network.
/// Painting is dirty-flag driven: an event that changes nothing costs nothing.
pub(crate) async fn run_event_loop(
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

#[cfg(test)]
mod tests;
