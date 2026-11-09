use super::*;
use crate::model::RepoStatus;
use crate::tests::{
    argv, at, ci_app, ci_failure_lands, ci_key, ci_runs, handoff_app, reading, test_repo,
    yank_status,
};
use crate::{git, github, model};

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

/// `Enter` and `O` on an other context in the `ci` view (FZ-83) open its
/// target url; on a run row, or without pane 2's focus, there is none,
/// and both keep their other meanings.
#[test]
fn enter_and_o_open_an_other_contexts_target_url() {
    let mut app = ci_app();
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Ci;
    app.focused_panel = FocusedPanel::Detail;
    let vercel = ci::Context {
        name: "Vercel".to_string(),
        state: model::CiStatus::Failed,
        description: Some("Aliasing has failed".to_string()),
        url: Some("https://vercel.com/x".to_string()),
        actions: false,
        source: "vercel".to_string(),
        started: None,
        completed: None,
    };
    app.ci.insert(
        ci_key(),
        app::CiState {
            runs: ci_runs(),
            tip: Some(app::CiTip {
                sha: "cc524b4".to_string(),
                statuses: vec![vercel],
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    app.detail_selections[app::HistoryView::Ci.index()] = 0;
    assert_eq!(context_url(&app).as_deref(), Some("https://vercel.com/x"));
    app.detail_selections[app::HistoryView::Ci.index()] = 1;
    assert_eq!(context_url(&app), None, "a run row");
    app.detail_selections[app::HistoryView::Ci.index()] = 0;
    app.focused_panel = FocusedPanel::RepoList;
    assert_eq!(context_url(&app), None, "pane 2 not focused");
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

/// FZ-60: `Enter` opens the highlighted changed file in `$EDITOR` while
/// pane 2 or the inspector has focus in `changes`, and lazygit
/// everywhere else, a repo with nothing read included.
#[test]
fn enter_opens_the_highlighted_changed_file_only_in_the_changes_view() {
    let mut app = App::new(Vec::new(), String::new(), 120, 40);
    app.repos = vec![RepoStatus {
        path: Some("/repo".to_string()),
        ..test_repo()
    }];
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Changes;
    app.focused_panel = FocusedPanel::Detail;
    assert_eq!(change_to_open(&app), None, "nothing read yet");

    let read = app::ChangesRead {
        key: "/repo".to_string(),
        root: "/repo".into(),
        generation: 0,
        base: git::ChangesBase::Upstream,
    };
    let changes = git::Changes {
        uncommitted: vec![git::ChangedFile {
            status: " M".to_string(),
            path: "src/a b.rs".to_string(),
            from: None,
            churn: Some((1, 0)),
        }],
        outgoing: git::Outgoing::NoUpstream { branch: None },
    };
    app.apply_changes_read(read, Ok(changes));
    let file = Some(std::path::PathBuf::from("/repo/src/a b.rs"));
    assert_eq!(change_to_open(&app), file);
    app.focused_panel = FocusedPanel::Inspector;
    assert_eq!(change_to_open(&app), file);

    app.focused_panel = FocusedPanel::RepoList;
    assert_eq!(change_to_open(&app), None, "the repo list opens lazygit");
    app.focused_panel = FocusedPanel::Detail;
    app.history_view = app::HistoryView::Notes;
    assert_eq!(change_to_open(&app), None, "another view opens lazygit");
}

/// `Enter`, `e` and `o` act in the highlighted worktree's directory
/// while pane 2 has focus on the `worktrees` view (FZ-77), and in the
/// repo's everywhere else. Fails while the arms take the repo path only.
#[test]
fn enter_opens_lazygit_in_the_highlighted_worktree() {
    let mut app = App::new(Vec::new(), String::new(), 120, 40);
    app.repos = vec![RepoStatus {
        path: Some("/repo".to_string()),
        worktrees: vec![model::Worktree {
            path: "/repo-wt".to_string(),
            branch: "side".to_string(),
            ..Default::default()
        }],
        ..test_repo()
    }];
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Worktrees;
    app.focused_panel = FocusedPanel::Detail;
    assert_eq!(note_to_open(&app), None, "Enter reaches the lazygit arm");
    assert_eq!(app.action_dir().as_deref(), Some("/repo"), "main first");
    app.detail_selections[app::HistoryView::Worktrees.index()] = 1;
    assert_eq!(app.action_dir().as_deref(), Some("/repo-wt"));

    app.focused_panel = FocusedPanel::RepoList;
    assert_eq!(app.action_dir().as_deref(), Some("/repo"), "pane 1");
    app.focused_panel = FocusedPanel::Detail;
    app.history_view = app::HistoryView::Info;
    assert_eq!(app.action_dir().as_deref(), Some("/repo"), "another view");
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

/// FZ-63, review finding 1: `todoz <text>` quick-adds, so `Enter` on a
/// board runs `todoz` with no positional argument, in the repo root,
/// with the scope flag notez resolves to that board: none for
/// `notez/TODO.md`, `-l` for `.notez/TODO.md`, `-p` for the personal
/// board. Any other board, any other note, and a todoz board without
/// `todoz` on PATH (saying it fell back) go to `$EDITOR` on the file.
#[test]
fn enter_on_a_board_opens_todoz_and_falls_back_to_the_editor() {
    let mut app = App::new(Vec::new(), String::new(), 120, 40);
    app.repos = vec![RepoStatus {
        path: Some("/repo".to_string()),
        ..test_repo()
    }];
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Notes;
    app.focused_panel = FocusedPanel::Detail;
    let walk = app::NotesWalk {
        key: "/repo".to_string(),
        root: "/repo".into(),
        generation: 0,
    };
    let personal = "/home/u/notez/personal/repo/TODO.md";
    let file = |rel: &str, notez: Option<&str>| notes::NoteFile {
        rel: rel.into(),
        notez: notez.map(Into::into),
    };
    let files = vec![
        file("notez/TODO.md", Some("notez")),
        file(".notez/TODO.md", Some(".notez")),
        file("TODO.md", None),
        file("crates/x/notez/TODO.md", Some("crates/x/notez")),
        file("notez/a.md", Some("notez")),
        file(personal, None),
    ];
    app.apply_notes_walk(walk, Ok(notes::Notes { files }));
    let todoz = |args: &[&str]| NoteOpener {
        program: "todoz".to_string(),
        args: args.iter().map(|a| a.to_string()).collect(),
        dir: Some("/repo".into()),
        fell_back: false,
    };
    let editor = |path: &str, fell_back| NoteOpener {
        program: "nvim".to_string(),
        args: vec![path.to_string()],
        dir: None,
        fell_back,
    };
    // In drawing order: the boards by path, the personal one, then
    // the `notez/` section.
    let cases = [
        (".notez/TODO.md", Some(todoz(&["-l"]))),
        ("TODO.md", None),
        ("crates/x/notez/TODO.md", None),
        ("notez/TODO.md", Some(todoz(&[]))),
        (personal, Some(todoz(&["-p"]))),
        ("notez/a.md", None),
    ];
    for (i, (rel, with_todoz)) in cases.into_iter().enumerate() {
        app.set_detail_selected(i);
        let path = std::path::Path::new("/repo").join(rel);
        let path = path.to_str().unwrap();
        let board = with_todoz.is_some();
        assert_eq!(
            note_opener(&app, true, "nvim"),
            Some(with_todoz.unwrap_or_else(|| editor(path, false))),
            "{rel} with todoz on PATH"
        );
        assert_eq!(
            note_opener(&app, false, "nvim"),
            Some(editor(path, board)),
            "{rel} without todoz"
        );
    }
    app.focused_panel = FocusedPanel::RepoList;
    assert_eq!(note_opener(&app, true, "nvim"), None);
}

#[test]
fn on_path_finds_an_executable_only() {
    let dir = std::env::temp_dir().join("fleetz_test_on_path");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("todoz"), "#!/bin/sh\n").unwrap();
    std::fs::write(dir.join("plain"), "").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let exec = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(dir.join("todoz"), exec).unwrap();
        let plain = std::fs::Permissions::from_mode(0o644);
        std::fs::set_permissions(dir.join("plain"), plain).unwrap();
        assert!(!on_path("plain", dir.as_os_str()));
    }
    let path = std::env::join_paths(["/nonexistent".into(), dir.clone()]).unwrap();
    assert!(on_path("todoz", &path));
    assert!(!on_path("absent", &path));
    let _ = std::fs::remove_dir_all(&dir);
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
