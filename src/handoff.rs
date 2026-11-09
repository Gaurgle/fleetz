use crate::app::{App, FocusedPanel};
use crate::event::{AppEvent, with_input_paused};
use crate::poll::spawn_ci_poll;
use crate::{app, ci, notes};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture, KeyCode};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io;
use std::process::Command as StdCommand;

/// Browser-opener binary: macOS ships `open`, everything else gets xdg-open.
#[cfg(target_os = "macos")]
pub(crate) const OPENER: &str = "open";
#[cfg(not(target_os = "macos"))]
pub(crate) const OPENER: &str = "xdg-open";

/// The file `Enter` opens in $EDITOR: the `notes` view's highlighted file,
/// while pane 2 or the reader has focus. `None` leaves `Enter` to
/// open lazygit on the repo.
pub(crate) fn note_to_open(app: &App) -> Option<std::path::PathBuf> {
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
pub(crate) fn change_to_open(app: &App) -> Option<std::path::PathBuf> {
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
pub(crate) struct NoteOpener {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    /// Where the program runs; `None` keeps fleetz's directory.
    pub(crate) dir: Option<std::path::PathBuf>,
    /// A board todoz could open went to the editor: `todoz` is not on PATH.
    pub(crate) fell_back: bool,
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
pub(crate) fn note_opener(app: &App, todoz_on_path: bool, editor: &str) -> Option<NoteOpener> {
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
pub(crate) fn on_path(name: &str, path: &std::ffi::OsStr) -> bool {
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
pub(crate) fn suspend_and_run(
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
pub(crate) fn suspend_and_run_in_dir(
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
pub(crate) enum RunHandoff {
    /// Show the run's `log` body in the inspector, without leaving fleetz.
    Inspect,
    /// Suspend fleetz and run `gh` with these arguments.
    Gh(Vec<String>),
    /// Open this URL with `OPENER`, as `O` opens a PR.
    Open(String),
}

/// Whether the handoff keys act: pane 2 focused and showing `ci`. Anywhere
/// else they keep their other meanings.
pub(crate) fn ci_handoff_applies(app: &App) -> bool {
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
pub(crate) fn context_url(app: &App) -> Option<String> {
    if !ci_handoff_applies(app) {
        return None;
    }
    app.highlighted_ci_context()?.url.clone()
}

/// Run a `ci` view key: `l` shows the run's log in the inspector, the
/// others hand off. Back from `gh`, the run's repo polls at once
/// (`ci_poll_after_handoff`) so the rerun or cancel shows.
pub(crate) fn ci_handoff_key(
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
pub(crate) fn ci_poll_after_handoff(
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

#[cfg(test)]
mod tests;
