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
mod event;
mod fuzzy;
mod git;
mod github;
mod graph;
mod handoff;
mod input;
mod load;
mod model;
mod notes;
mod poll;
mod report;
mod sweep;
mod time;
mod ui;
mod window_state;

use app::App;
#[cfg(test)]
use app::{FocusedPanel, Overlay};
use clap::Parser;
use cli::{Args, Mode};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
#[cfg(test)]
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
pub use event::AppEvent;
#[cfg(test)]
use event::SLOW_REPAINT_TICKS;
use event::{LoadKind, run_event_loop, spawn_input_thread, spawn_tick_task};
#[cfg(test)]
use input::handle_normal_input;
use load::spawn_load;
#[cfg(test)]
use model::RepoLocation;
#[cfg(test)]
use poll::GITHUB_POLL_STUCK_AFTER;
use poll::configured_poll_secs;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::collections::HashSet;
use std::io;
#[cfg(test)]
use std::path::Path;

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

#[cfg(test)]
mod tests;

/// FZ-45: section headers as cursor stops and the slot digest, driven
/// through the key handler and checked against what the renderer draws.
#[cfg(test)]
mod digest_handler_tests;
