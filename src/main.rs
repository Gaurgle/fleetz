mod app;
mod cli;
mod config;
mod discovery;
mod git;
mod github;
mod model;
mod pull;
mod time;
mod ui;

use app::{App, Overlay};
use clap::Parser;
use cli::{Args, Mode};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let config = config::Config::load();
    let repo_dir = std::env::var("REPO_CHECK_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| config.resolved_repo_dir());

    let mode = args.mode();
    let (repos, header) = load_repos(&args, &config, &repo_dir, &mode).await?;

    let mut app = App::new(repos, header, args.prs, args.stat);

    // Enter TUI
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_event_loop(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        terminal.draw(|frame| ui::render(frame, app))?;

        if app.should_quit {
            return Ok(());
        }

        if let Event::Key(key) = event::read()? {
            if app.overlay == Overlay::Help {
                match key.code {
                    KeyCode::Char('?') | KeyCode::Esc => app.toggle_help(),
                    _ => {}
                }
                continue;
            }

            if app.search_active {
                handle_search_input(app, key);
                continue;
            }

            handle_normal_input(app, terminal, key).await?;
        }
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
        KeyCode::Char('j') | KeyCode::Down => app.move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.move_up(),
        KeyCode::Char('v') => app.toggle_view(),
        KeyCode::Char('?') => app.toggle_help(),
        KeyCode::Char('/') => app.start_search(),
        KeyCode::Char('p') => {
            if let Some(repo) = app.selected_repo().cloned() {
                let result = pull::pull_single(&repo).await;
                let msg = result.message(&repo.name);
                app.set_status(msg, !result.is_success());
                // Refresh the repo status after pull
                if result.is_success() {
                    if let Some(idx) = app.selected_repo_index() {
                        refresh_single_repo(&mut app.repos[idx]).await;
                    }
                }
            }
        }
        KeyCode::Char('P') => {
            let plan = pull::bulk_pull_candidates(&app.repos);
            if plan.candidate_indices.is_empty() {
                app.set_status("No repos to pull".to_string(), false);
            } else {
                // Pull all candidates
                let mut pulled = 0u32;
                let indices = plan.candidate_indices.clone();
                for &i in &indices {
                    let repo = app.repos[i].clone();
                    let result = pull::pull_single(&repo).await;
                    if result.is_success() {
                        pulled += 1;
                        refresh_single_repo(&mut app.repos[i]).await;
                    }
                }
                app.set_status(
                    format!("Pulled {pulled} of {} repos", indices.len()),
                    false,
                );
            }
        }
        KeyCode::Enter => {
            // Open lazygit in selected repo
            if let Some(repo) = app.selected_repo() {
                if let Some(ref path) = repo.path {
                    suspend_and_run(terminal, "lazygit", &["-p", path])?;
                }
            }
        }
        KeyCode::Char('e') => {
            // Open $EDITOR in selected repo
            if let Some(repo) = app.selected_repo() {
                if let Some(ref path) = repo.path {
                    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
                    suspend_and_run(terminal, &editor, &[path])?;
                }
            }
        }
        KeyCode::Char('o') => {
            // Open shell in repo dir
            if let Some(repo) = app.selected_repo() {
                if let Some(ref path) = repo.path {
                    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
                    suspend_and_run_in_dir(terminal, &shell, &[], path)?;
                }
            }
        }
        KeyCode::Char('r') => {
            app.set_status("Refreshing...".to_string(), false);
            // Full refresh would re-fetch from GitHub + git
            // For now, just refresh git status for all local repos
            for repo in &mut app.repos {
                refresh_single_repo(repo).await;
            }
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

    let mut total_behind = 0u32;
    let mut total_ahead = 0u32;
    for b in &branches {
        total_behind += b.behind;
        total_ahead += b.ahead;
    }

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
            let display = format!("~{}", path.strip_prefix(dirs::home_dir().unwrap_or_default()).unwrap_or(&path).display());

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
                pushed_at: None,
            });
        }
        return Ok((repos, header));
    }

    // Fetch from GitHub
    let gh_repos = github::fetch_repos(config.repo_limit, args.prs).await?;

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
            pushed_at: Some(gh_repo.pushed_at),
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

        let display = format!(
            "~{}",
            path.strip_prefix(&home).unwrap_or(&path).display()
        );

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
            pushed_at: None,
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

    // Determine the range for commits
    let has_changes = total_behind > 0 || total_ahead > 0 || dirty > 0;
    let show_stat = has_changes || args.stat;
    let author = args.author.as_deref();

    let mut commits = Vec::new();
    for b in &branches {
        if b.behind > 0 {
            if let Some(ref up) = b.upstream {
                let range = format!("{}..{}", b.name, up);
                let c = git::commits(path, &range, show_stat, author).await;
                commits.extend(c);
            }
        }
        if b.ahead > 0 {
            if let Some(ref up) = b.upstream {
                let range = format!("{}..{}", up, b.name);
                let c = git::commits(path, &range, show_stat, author).await;
                commits.extend(c);
            }
        }
    }
    // If no divergence, show last commit
    if commits.is_empty() {
        let c = git::commits(path, "-1", show_stat, author).await;
        commits.extend(c);
    }

    let pr_count = gh_repos
        .iter()
        .find(|r| r.name.eq_ignore_ascii_case(&found.name))
        .map_or(0, |r| r.pr_count);
    let pushed_at = gh_repos
        .iter()
        .find(|r| r.name.eq_ignore_ascii_case(&found.name))
        .map(|r| r.pushed_at);

    let home = dirs::home_dir().unwrap_or_default();
    let display = format!(
        "~{}",
        path.strip_prefix(&home).unwrap_or(path).display()
    );

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
        pushed_at,
    }
}
