use crate::app::{App, ApplyProgress, BranchEntry, BranchOverlay, FocusedPanel, Overlay};
use crate::cli::Args;
use crate::engine::{size_held_rows, tilde_display};
use crate::event::{AppEvent, LoadKind};
use crate::handoff::{
    OPENER, change_to_open, ci_handoff_applies, ci_handoff_key, context_url, note_opener,
    note_to_open, on_path, suspend_and_run, suspend_and_run_in_dir,
};
use crate::load::{spawn_load, spawn_memory_rebuild, spawn_window_apply};
use crate::poll::{github_limit_until, limited_status, start_github_poll};
use crate::sweep::{start_single_sweep, start_sweep};
use crate::{app, clipboard, config, git, ui};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io;
use std::path::Path;
use std::process::Command as StdCommand;

/// Dispatch one key to the overlay, the search box, or the normal bindings.
/// Overlays are modal: they consume the key and nothing else sees it.
pub(crate) async fn handle_key(
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
pub(crate) fn handle_mouse_input(
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
pub(crate) fn click_detail(
    app: &mut App,
    col: u16,
    row: u16,
    copy: impl FnOnce(&str) -> Result<(), String>,
) {
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
pub(crate) fn handle_settings_input(app: &mut App, key: KeyEvent) {
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
pub(crate) fn settings_overlay(app: &App, config: &config::Config) -> app::SettingsOverlay {
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
pub(crate) fn apply_live_settings(app: &mut App, so: &app::SettingsOverlay) {
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
pub(crate) async fn handle_normal_input(
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

/// Rows the detail pane scrolls per wheel notch.
///
/// The wheel used to call `detail_nav_down/up`, moving the selection rather
/// than the view, so a notch jumped a whole commit block in the graph view.
/// The inspector already treats the wheel as a scroll; this makes the detail
/// pane agree, and makes the help overlay's "wheel scrolls" true.
const DETAIL_WHEEL_ROWS: i32 = 3;

/// The status line for `R` ignored because a refresh is already under way
/// (A7, and A6 with a poll in flight). One status for both, so the key does
/// not look dead. Provisional words until Andreas settles them (O1).
const REFRESH_RUNNING: &str = "Already refreshing";

/// What `R` does, decided before anything is spawned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReloadAction {
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

pub(crate) fn reload_action(
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
pub(crate) fn refresh_key(
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

#[cfg(test)]
mod tests;
