use super::*;
use crate::digest::tests::repo;

fn changes_app() -> App {
    let repos = vec![repo("fixture", RepoLocation::Primary)];
    let mut app = App::new(repos, String::new(), 120, 40);
    app.repos[0].path = Some("/changes-fixture".to_string());
    app.view = View::Detail;
    app.inspector_folded = false;
    app
}

fn changed_file(status: &str, path: &str) -> crate::git::ChangedFile {
    crate::git::ChangedFile {
        status: status.to_string(),
        path: path.to_string(),
        from: None,
        churn: Some((1, 0)),
    }
}

/// Land a read of `/changes-fixture`: `uncommitted`, and `unpushed`
/// ahead of `origin/main`.
fn changes_landed(app: &mut App, uncommitted: &[&str], unpushed: &[&str]) {
    let read = ChangesRead {
        key: "/changes-fixture".to_string(),
        root: "/changes-fixture".into(),
        generation: app.changes_generation,
        base: app.changes_base,
    };
    let files = |paths: &[&str], status: &str| -> Vec<crate::git::ChangedFile> {
        paths.iter().map(|p| changed_file(status, p)).collect()
    };
    let changes = crate::git::Changes {
        uncommitted: files(uncommitted, " M"),
        outgoing: crate::git::Outgoing::Unpushed {
            upstream: "origin/main".to_string(),
            ahead: 1,
            files: files(unpushed, "M"),
        },
    };
    app.apply_changes_read(read, Ok(changes));
}

/// FZ-60: the read starts only while `changes` is on screen in the
/// detail view, and only once the cursor has rested on the row for the
/// dwell. Fails if any other view, or the compact grid, reads the repo.
#[test]
fn the_changes_read_waits_for_its_view_and_the_dwell() {
    let start = std::time::Instant::now();
    let after = |ms: u64| start + std::time::Duration::from_millis(ms);
    let mut app = changes_app();
    for view in HistoryView::ALL {
        if view == HistoryView::Changes {
            continue;
        }
        app.history_view = view;
        assert_eq!(app.claim_changes_read(start), None, "{view:?}");
        assert_eq!(app.claim_changes_read(after(1000)), None, "{view:?}");
    }
    app.history_view = HistoryView::Changes;
    app.view = View::Compact;
    assert_eq!(app.claim_changes_read(after(2000)), None, "compact");

    app.view = View::Detail;
    assert_eq!(app.claim_changes_read(after(3000)), None, "dwell starts");
    assert_eq!(app.claim_changes_read(after(3299)), None, "still dwelling");
    let read = app.claim_changes_read(after(3300)).expect("reads");
    assert_eq!(read.root, std::path::PathBuf::from("/changes-fixture"));
    assert_eq!(read.base, crate::git::ChangesBase::Upstream);
    assert_eq!(app.claim_changes_read(after(4000)), None, "one at a time");
}

/// A not-cloned row has nothing to read.
#[test]
fn a_not_cloned_row_reads_no_changes() {
    let start = std::time::Instant::now();
    let mut app = changes_app();
    app.repos[0].path = None;
    app.history_view = HistoryView::Changes;
    app.claim_changes_read(start);
    let later = start + std::time::Duration::from_secs(5);
    assert_eq!(app.claim_changes_read(later), None);
    assert!(app.changes.is_empty());
}

/// Held for the session: leaving the view and coming back reads
/// nothing, until a sweep lands or a suspend returns, or the base
/// toggle asks for the other section.
#[test]
fn coming_back_to_changes_rereads_only_after_a_sweep_a_suspend_or_the_toggle() {
    let start = std::time::Instant::now();
    let after = |ms: u64| start + std::time::Duration::from_millis(ms);
    let mut app = changes_app();
    app.history_view = HistoryView::Changes;
    app.claim_changes_read(start);
    app.claim_changes_read(after(300)).expect("first read");
    changes_landed(&mut app, &["a.txt"], &[]);

    app.history_view = HistoryView::Info;
    app.claim_changes_read(after(400));
    app.history_view = HistoryView::Changes;
    assert_eq!(app.claim_changes_read(after(5000)), None, "held");
    assert_eq!(app.changes_listed().len(), 1);

    app.sweep_landed(chrono::Local::now());
    let read = app.claim_changes_read(after(5100)).expect("after a sweep");
    assert_eq!(app.changes_listed().len(), 1, "the old read stays drawn");
    app.apply_changes_read(read, Err("gone".to_string()));
    assert_eq!(app.claim_changes_read(after(5200)), None, "held again");

    app.invalidate_changes();
    let read = app
        .claim_changes_read(after(5300))
        .expect("after a suspend");
    app.apply_changes_read(read, Err("gone".to_string()));

    app.toggle_changes_base();
    let read = app.claim_changes_read(after(5400)).expect("the toggle");
    assert_eq!(read.base, crate::git::ChangesBase::DefaultBranch);
}

/// The diff read is the highlighted file's alone, however far the
/// cursor moved, nothing outside the view or with the inspector folded,
/// and an outgoing file is read against its section's base.
#[test]
fn only_the_highlighted_changed_file_is_diffed() {
    let mut app = changes_app();
    app.history_view = HistoryView::Changes;
    changes_landed(&mut app, &["a.txt", "b.txt"], &["c.txt"]);
    app.history_view = HistoryView::Info;
    assert_eq!(app.claim_change_diff(), None, "another view reads nothing");

    app.history_view = HistoryView::Changes;
    app.select_commit_down();
    let b = app.claim_change_diff().expect("reads");
    assert_eq!(b.file.path, "b.txt");
    assert_eq!(b.base, None, "uncommitted diffs against HEAD");
    assert_eq!(app.claim_change_diff(), None, "read once");

    app.select_commit_down();
    let c = app.claim_change_diff().expect("c");
    assert_eq!(c.file.path, "c.txt");
    assert_eq!(c.section, ChangeSection::Outgoing);
    assert_eq!(c.base.as_deref(), Some("origin/main"));

    app.apply_change_diff(b, Ok("b".to_string()));
    assert_eq!(app.highlighted_change_diff().unwrap().text, None, "stale");
    app.apply_change_diff(c, Ok("c".to_string()));
    assert_eq!(
        app.highlighted_change_diff().unwrap().text,
        Some(Ok("c".to_string()))
    );

    app.invalidate_changes();
    app.inspector_folded = true;
    assert_eq!(app.claim_change_diff(), None, "folded inspector");
    app.inspector_folded = false;
    assert!(app.claim_change_diff().is_some(), "re-read after a suspend");
}

/// The merge with the yank chord (FZ-74): in `changes`, a plain `y`
/// still copies the file's path in the repo at once, `yy` copies it
/// again and `yp` copies its absolute path. Fails while the chord has no
/// `changes` arm and the first `y` copies nothing there.
#[test]
fn the_yank_chord_copies_a_changed_files_paths() {
    let mut app = changes_app();
    app.history_view = HistoryView::Changes;
    app.focused_panel = FocusedPanel::Detail;
    changes_landed(&mut app, &["src/a b.rs"], &[]);
    let mut copied = Vec::new();
    for key in ['y', 'y', 'y', 'p'] {
        assert!(app.press_yank_key(Some(key), |s| {
            copied.push(s.to_string());
            Ok(())
        }));
    }
    assert_eq!(
        copied,
        [
            "src/a b.rs",
            "src/a b.rs",
            "src/a b.rs",
            "/changes-fixture/src/a b.rs"
        ]
    );
}

/// Review 1 on #116: a diff held for one repo is not another's. Two
/// repos each with `a.txt` uncommitted; the first one's diff is held
/// and scrolled, a search moves the cursor to the second: it draws no
/// diff until its own lands, and its claim starts the inspector at the
/// top. Fails while the held diff is matched on section and path alone.
#[test]
fn a_same_named_file_in_another_repo_draws_no_held_diff() {
    let mut app = changes_app();
    app.repos.push(repo("second", RepoLocation::Primary));
    app.repos[1].path = Some("/changes-second".to_string());
    app.history_view = HistoryView::Changes;
    for key in ["/changes-fixture", "/changes-second"] {
        let read = ChangesRead {
            key: key.to_string(),
            root: key.into(),
            generation: app.changes_generation,
            base: app.changes_base,
        };
        let changes = crate::git::Changes {
            uncommitted: vec![changed_file(" M", "a.txt")],
            outgoing: crate::git::Outgoing::NoUpstream { branch: None },
        };
        app.apply_changes_read(read, Ok(changes));
    }
    assert_eq!(
        app.selected_repo().map(|r| r.name.as_str()),
        Some("fixture")
    );
    let first = app.claim_change_diff().expect("the first repo's diff");
    app.apply_change_diff(first, Ok("first".to_string()));
    assert!(app.highlighted_change_diff().is_some(), "held for its repo");
    app.inspector_scroll = 7;

    // A search moves the cursor without the list's scroll reset, so the
    // claim's own reset is what starts the new diff at its top.
    app.start_search();
    "second".chars().for_each(|c| app.search_push_char(c));
    assert_eq!(app.selected_repo().map(|r| r.name.as_str()), Some("second"));
    assert_eq!(
        app.highlighted_change_diff(),
        None,
        "the first repo's diff under the second's heading"
    );
    let second = app.claim_change_diff().expect("the second repo's diff");
    assert_eq!(second.root, std::path::PathBuf::from("/changes-second"));
    assert_eq!(
        app.inspector_scroll, 0,
        "a new repo's diff starts at its top"
    );
}

/// `y` copies the highlighted file's repo-relative path; `u` toggles
/// the base only in the view.
#[test]
fn y_copies_the_changed_path_and_u_toggles_only_in_the_view() {
    let mut app = changes_app();
    app.history_view = HistoryView::Info;
    app.toggle_changes_base();
    assert_eq!(app.changes_base, crate::git::ChangesBase::Upstream);
    app.history_view = HistoryView::Changes;
    changes_landed(&mut app, &["src/a b.rs"], &[]);
    let mut copied = String::new();
    app.yank_change_path(|s| {
        copied = s.to_string();
        Ok(())
    });
    assert_eq!(copied, "src/a b.rs");
    assert_eq!(
        app.change_to_open(),
        Some(std::path::PathBuf::from("/changes-fixture/src/a b.rs"))
    );
    app.toggle_changes_base();
    assert_eq!(app.changes_base, crate::git::ChangesBase::DefaultBranch);
}
