mod app;
mod cache;
mod cli;
mod config;
mod discovery;
mod git;
mod github;
mod model;
mod time;
mod ui;

use app::{App, BranchEntry, BranchOverlay, FocusedPanel, Overlay};
use clap::Parser;
use cli::{Args, Mode};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use model::{RepoLocation, RepoStatus};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::process::Command as StdCommand;

/// How many commits/reflog entries to show in the detail pane's history
/// sections. Kept modest -- the pane has no scrolling, and these share it
/// with branches, CI, PRs, and stashes.
const RECENT_COMMIT_COUNT: u32 = 6;
const REFLOG_COUNT: u32 = 6;
const GRAPH_COMMIT_COUNT: u32 = 10;

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
    let (repos, header) = load_repos(&args, &config, &repo_dir, &mode).await?;

    let (term_width, term_height) = crossterm::terminal::size().unwrap_or((80, 24));
    let mut app = App::new(repos, header, args.prs, args.stat, term_width, term_height);
    app.watch_mode = args.watch;
    app.refresh_interval = config.refresh_interval;

    // Set up groups from config
    if !config.groups.is_empty() {
        app.group_names = config.groups.keys().cloned().collect();
        app.group_names.sort();
        for repo in &mut app.repos {
            for (group_name, members) in &config.groups {
                if members.iter().any(|m| m.eq_ignore_ascii_case(&repo.name)) {
                    repo.groups.push(group_name.clone());
                }
            }
        }
    }

    // Enter TUI
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_event_loop(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    let poll_timeout = if app.watch_mode {
        std::time::Duration::from_secs(1)
    } else {
        std::time::Duration::from_secs(3600) // effectively blocking
    };

    loop {
        terminal.draw(|frame| ui::render(frame, app))?;

        if app.should_quit {
            return Ok(());
        }

        // Check if watch mode refresh is due
        if app.watch_mode {
            if let Some(last) = app.last_refreshed {
                let elapsed = (chrono::Local::now() - last).num_seconds();
                if elapsed >= app.refresh_interval as i64 {
                    for repo in &mut app.repos {
                        refresh_single_repo(repo).await;
                    }
                    app.last_refreshed = Some(chrono::Local::now());
                }
            }
        }

        if event::poll(poll_timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    // Any keypress clears a transient status message before it's
                    // handled, so the message shows until the next interaction and
                    // then yields the footer back to the summary. The handler for
                    // this same key may set a fresh one, which survives (set after
                    // this clear).
                    app.clear_status();

                    match &app.overlay {
                        Overlay::Help => {
                            match key.code {
                                KeyCode::Char('?') | KeyCode::Esc => app.toggle_help(),
                                _ => {}
                            }
                            continue;
                        }
                        Overlay::Branches(_) => {
                            handle_branch_input(app, key).await;
                            continue;
                        }
                        Overlay::None => {}
                    }

                    if app.search_active {
                        handle_search_input(app, key);
                        continue;
                    }

                    handle_normal_input(app, terminal, key).await?;
                }
                Event::Mouse(m) => handle_mouse_input(app, terminal, m),
                _ => {}
            }
        }
    }
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
    let Some(panel) = rects.at(m.column, m.row) else { return };

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
        },
        MouseEventKind::ScrollUp => match panel {
            FocusedPanel::RepoList => app.move_up(),
            FocusedPanel::Detail => app.detail_nav_up(),
            FocusedPanel::Inspector => app.scroll_inspector(-1),
        },
        _ => {}
    }
}

async fn handle_branch_input(app: &mut App, key: KeyEvent) {
    // Extract overlay data — need to work around borrow checker
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
                bo.branches.get(bo.selected).map(|b| (bo.repo_path.clone(), b.name.clone(), b.is_current))
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
                            if let Some(idx) = app.selected_repo_index() {
                                refresh_single_repo(&mut app.repos[idx]).await;
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
            if let Overlay::Branches(ref mut bo) = app.overlay {
                if bo.selected_count() > 0 {
                    bo.confirm_delete = true;
                }
            }
        }
        _ => {}
    }
}

/// Build and show the branches overlay for the selected repo: every local
/// branch, with merged ones flagged for cleanup and the current one marked.
/// Enter switches to a branch; space+d deletes merged ones.
async fn open_branch_overlay(app: &mut App) {
    let Some(repo) = app.selected_repo() else { return };
    let Some(ref path) = repo.path else { return };
    let repo_name = repo.name.clone();
    let path = path.clone();

    let expanded = shellexpand::tilde(&path).to_string();
    let repo_path = Path::new(&expanded);
    let current = git::current_branch(repo_path).await.unwrap_or_default();
    let all = git::all_branches(repo_path).await;
    let merged = git::merged_branches(repo_path, &current).await;

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
    let Overlay::Branches(ref bo) = app.overlay else { return };
    let Some(branch) = bo.selected_branch() else { return };
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

async fn handle_normal_input(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    key: KeyEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        // A leftover search query (set, then Enter'd out of edit mode) is
        // cleared by Esc -- matching the strip's "esc to clear" hint.
        KeyCode::Esc if !app.search_query.is_empty() => app.clear_search(),
        KeyCode::Tab | KeyCode::BackTab => app.toggle_panel_focus(),
        KeyCode::Char('j') | KeyCode::Down => match app.focused_panel {
            FocusedPanel::RepoList => app.move_down(),
            FocusedPanel::Detail => app.detail_nav_down(),
            FocusedPanel::Inspector => app.scroll_inspector(1),
        },
        KeyCode::Char('k') | KeyCode::Up => match app.focused_panel {
            FocusedPanel::RepoList => app.move_up(),
            FocusedPanel::Detail => app.detail_nav_up(),
            FocusedPanel::Inspector => app.scroll_inspector(-1),
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
            if let Some(repo) = app.selected_repo().cloned() {
                if let Some(ref path) = repo.path {
                    let expanded = shellexpand::tilde(path).to_string();
                    // Draw the "fetching" state before the blocking fetch, so
                    // the indicator actually appears rather than being set and
                    // immediately overwritten by the result on the next frame.
                    app.set_status(format!("\u{27f3} Fetching {}…", repo.name), false);
                    terminal.draw(|frame| ui::render(frame, app))?;
                    let ok = git::fetch(Path::new(&expanded)).await;
                    if ok {
                        if let Some(idx) = app.selected_repo_index() {
                            refresh_single_repo(&mut app.repos[idx]).await;
                        }
                        app.set_status(format!("Fetched {}", repo.name), false);
                    } else {
                        app.set_status(format!("Fetch failed for {}", repo.name), true);
                    }
                }
            }
        }
        KeyCode::Char('F') => {
            let paths: Vec<(usize, String)> = app
                .repos
                .iter()
                .enumerate()
                .filter_map(|(i, r)| r.path.clone().map(|p| (i, p)))
                .collect();
            let total = paths.len();
            let mut fetched = 0u32;
            for (n, (i, path)) in paths.iter().enumerate() {
                // Live progress indicator, redrawn before each blocking fetch.
                app.set_status(format!("\u{27f3} Fetching {}/{total}…", n + 1), false);
                terminal.draw(|frame| ui::render(frame, app))?;
                let expanded = shellexpand::tilde(path).to_string();
                if git::fetch(Path::new(&expanded)).await {
                    fetched += 1;
                    refresh_single_repo(&mut app.repos[*i]).await;
                }
            }
            app.set_status(format!("Fetched {fetched} of {total} repos"), false);
        }
        KeyCode::Enter => {
            // Open lazygit in selected repo
            if let Some(repo) = app.selected_repo() {
                if let Some(ref path) = repo.path {
                    let expanded = shellexpand::tilde(path).to_string();
                    suspend_and_run(terminal, "lazygit", &["-p", &expanded])?;
                }
            }
        }
        KeyCode::Char('e') => {
            // Open $EDITOR in selected repo
            if let Some(repo) = app.selected_repo() {
                if let Some(ref path) = repo.path {
                    let expanded = shellexpand::tilde(path).to_string();
                    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
                    suspend_and_run(terminal, &editor, &[&expanded])?;
                }
            }
        }
        KeyCode::Char('o') => {
            // Open shell in repo dir
            if let Some(repo) = app.selected_repo() {
                if let Some(ref path) = repo.path {
                    let expanded = shellexpand::tilde(path).to_string();
                    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
                    suspend_and_run_in_dir(terminal, &shell, &[], &expanded)?;
                }
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
        KeyCode::Char('O') => {
            // Open first PR in browser (Shift+O)
            if let Some(repo) = app.selected_repo() {
                if let Some(pr) = repo.prs.first() {
                    let _ = StdCommand::new("open").arg(&pr.url).status();
                } else if repo.pr_count > 0 {
                    app.set_status("Use -p flag to fetch PR details for browser open".to_string(), false);
                }
            }
        }
        KeyCode::Char('r') => {
            app.set_status("Refreshing...".to_string(), false);
            for repo in &mut app.repos {
                refresh_single_repo(repo).await;
            }
            app.last_refreshed = Some(chrono::Local::now());
            app.set_status("Refreshed".to_string(), false);
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        _ => {}
    }
    Ok(())
}

/// Suspend the TUI, run a command, then resume.
fn suspend_and_run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    cmd: &str,
    args: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    // Leave alternate screen
    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Run the command
    let _ = StdCommand::new(cmd).args(args).status();

    // Re-enter alternate screen
    crossterm::execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}

/// Suspend the TUI, run a command in a specific directory, then resume.
fn suspend_and_run_in_dir(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    cmd: &str,
    args: &[&str],
    dir: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    let expanded = shellexpand::tilde(dir);
    let _ = StdCommand::new(cmd)
        .args(args)
        .current_dir(expanded.as_ref())
        .status();

    crossterm::execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}

/// Refresh git status for a single repo in-place.
async fn refresh_single_repo(repo: &mut RepoStatus) {
    let path = match &repo.path {
        Some(p) => p.clone(),
        None => return,
    };
    let expanded = shellexpand::tilde(&path).to_string();
    let path = Path::new(&expanded);

    let branches = git::branch_statuses(path).await;
    let dirty = git::dirty_count(path).await;
    let remote = git::remote_branches(path).await;
    let local_names: Vec<String> = branches.iter().map(|b| b.name.clone()).collect();

    let mut total_behind = 0u32;
    let mut total_ahead = 0u32;
    for b in &branches {
        total_behind += b.behind;
        total_ahead += b.ahead;
    }

    repo.remote_only_branches = git::unfetched_branches(&remote, &local_names);
    repo.branches = branches;
    repo.dirty = dirty;
    repo.behind = total_behind;
    repo.ahead = total_ahead;
}

/// Load all repo data: GitHub API + local discovery + git status.
async fn load_repos(
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    mode: &Mode,
) -> Result<(Vec<RepoStatus>, String), Box<dyn std::error::Error>> {
    // Dirty mode: local only, no GitHub
    if *mode == Mode::Dirty {
        let header = "local changes".to_string();
        let local_paths = discovery::find_all_local(repo_dir);
        let mut repos = Vec::new();

        for path in local_paths {
            let dirty = git::dirty_count(&path).await;
            let branches = git::branch_statuses(&path).await;
            let mut total_ahead = 0u32;
            for b in &branches {
                total_ahead += b.ahead;
            }

            if dirty == 0 && total_ahead == 0 {
                continue;
            }

            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            let display = tilde_display(&path);

            let show_stat = args.stat;
            let author = args.author.as_deref();
            let commits = git::commits(&path, &format!("-{RECENT_COMMIT_COUNT}"), show_stat, author).await;
            let reflog = git::reflog(&path, REFLOG_COUNT).await;
            let branch_names: Vec<String> = branches.iter().map(|b| b.name.clone()).collect();
            let graph = git::graph(&path, &branch_names, GRAPH_COMMIT_COUNT).await;
            let stash_entries = git::stash_list(&path).await;
            let stash_count = stash_entries.len() as u32;
            let worktrees = git::worktree_list(&path).await;
            let remote_url = git::remote_url(&path).await;
            let remote = git::remote_branches(&path).await;
            let remote_only_branches = git::unfetched_branches(&remote, &branch_names);

            repos.push(RepoStatus {
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
                pushed_at: None,
                groups: vec![],
                stash_count,
                stash_entries,
                worktrees,
                reflog,
                graph,
                repo_kind: model::RepoKind::Unknown,
                remote_url,
                remote_only_branches,
            });
        }
        return Ok((repos, header));
    }

    // Fetch from GitHub (with caching)
    let api_cache = cache::ApiCache::new(config.cache_ttl);
    let gh_repos = github::fetch_repos(config.repo_limit, args.prs, &api_cache, false).await?;

    // Determine cutoff and header
    let (cutoff, header) = match mode {
        Mode::Auto => {
            let (c, h) = time::auto_cutoff(config.work_start, config.work_end);
            (Some(c), h)
        }
        Mode::Since(s) => {
            let date = time::parse_relative_date(s)?;
            let c = time::date_to_utc_midnight(date);
            (Some(c), format!("since {s}"))
        }
        Mode::Today => {
            let today = chrono::Local::now().date_naive();
            let c = time::date_to_utc_midnight(today);
            (Some(c), "today".to_string())
        }
        Mode::All => (None, "all divergence".to_string()),
        Mode::Dirty => unreachable!(),
    };

    // Apply --before filter
    let before_cutoff = match &args.before {
        Some(b) => {
            let date = time::parse_relative_date(b)?;
            Some(time::date_to_utc_end_of_day(date))
        }
        None => None,
    };

    // Filter active repos
    let mut active = gh_repos;
    if let Some(c) = cutoff {
        active = github::filter_since(&active, c);
    }
    if let Some(b) = before_cutoff {
        active = github::filter_before(&active, b);
    }

    let active_names: Vec<String> = active.iter().map(|r| r.name.clone()).collect();

    // Pass 1: find in primary repo_dir
    let primary_found = discovery::find_in_primary(repo_dir, &active_names);
    let found_set: HashSet<String> = primary_found
        .iter()
        .map(|f| f.name.to_lowercase())
        .collect();

    // Pass 2: find elsewhere
    let home = dirs::home_dir().unwrap_or_default();
    let elsewhere_found =
        discovery::find_elsewhere(&home, repo_dir, &active_names, &found_set);

    // Parallel fetch for all found repos
    let all_found: Vec<_> = primary_found
        .iter()
        .chain(elsewhere_found.iter())
        .collect();

    let fetch_handles: Vec<_> = all_found
        .iter()
        .map(|f| {
            let path = f.path.clone();
            tokio::spawn(async move { git::fetch(&path).await })
        })
        .collect();

    for handle in fetch_handles {
        let _ = handle.await;
    }

    // Build repo statuses
    let mut repos = Vec::new();
    let mut checked_names: HashSet<String> = HashSet::new();

    // Process primary repos
    for found in &primary_found {
        let repo = build_repo_status(found, &active, args, RepoLocation::Primary).await;
        checked_names.insert(found.name.to_lowercase());
        repos.push(repo);
    }

    // Process elsewhere repos
    for found in &elsewhere_found {
        if checked_names.contains(&found.name.to_lowercase()) {
            continue;
        }
        let repo = build_repo_status(found, &active, args, RepoLocation::Elsewhere).await;
        checked_names.insert(found.name.to_lowercase());
        repos.push(repo);
    }

    // Not-cloned repos
    for gh_repo in &active {
        if checked_names.contains(&gh_repo.name.to_lowercase()) {
            continue;
        }
        repos.push(RepoStatus {
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
            pushed_at: Some(gh_repo.pushed_at),
            groups: vec![],
            stash_count: 0,
            stash_entries: vec![],
            worktrees: vec![],
            reflog: vec![],
            graph: vec![],
            repo_kind: gh_repo.repo_kind,
            remote_url: None,
            remote_only_branches: vec![],
        });
    }

    // Pass 3: local dirty repos not already included
    let all_local = discovery::find_all_local(repo_dir);
    for path in all_local {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if checked_names.contains(&name.to_lowercase()) {
            continue;
        }

        let dirty = git::dirty_count(&path).await;
        let branches = git::branch_statuses(&path).await;
        let mut total_ahead = 0u32;
        for b in &branches {
            total_ahead += b.ahead;
        }

        if dirty == 0 && total_ahead == 0 {
            continue;
        }

        let display = tilde_display(&path);
        let remote_url = git::remote_url(&path).await;
        let remote = git::remote_branches(&path).await;
        let branch_names: Vec<String> = branches.iter().map(|b| b.name.clone()).collect();
        let remote_only_branches = git::unfetched_branches(&remote, &branch_names);

        repos.push(RepoStatus {
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
            pushed_at: None,
            groups: vec![],
            stash_count: 0,
            stash_entries: vec![],
            worktrees: vec![],
            reflog: vec![],
            graph: vec![],
            repo_kind: model::RepoKind::Unknown,
            remote_url,
            remote_only_branches,
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

    Ok((repos, full_header))
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

    let has_changes = total_behind > 0 || total_ahead > 0 || dirty > 0;
    let show_stat = has_changes || args.stat;
    let author = args.author.as_deref();

    // Recent history on HEAD, not just the divergent commits -- a glance at
    // "what's been happening here" shouldn't depend on being ahead/behind.
    let commits = git::commits(path, &format!("-{RECENT_COMMIT_COUNT}"), show_stat, author).await;
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
    let pushed_at = gh_match.map(|r| r.pushed_at);
    let repo_kind = gh_match.map_or(model::RepoKind::Unknown, |r| r.repo_kind);

    let display = tilde_display(path);

    // Stash + worktree info
    let stash_entries = git::stash_list(path).await;
    let stash_count = stash_entries.len() as u32;
    let worktrees = git::worktree_list(path).await;

    let remote_url = git::remote_url(path).await;
    let remote = git::remote_branches(path).await;
    let remote_only_branches = git::unfetched_branches(&remote, &branch_names);

    RepoStatus {
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
        pushed_at,
        groups: vec![],
        stash_count,
        stash_entries,
        worktrees,
        reflog,
        graph,
        repo_kind,
        remote_url,
        remote_only_branches,
    }
}
