use super::*;
use crate::digest::tests::{commit, fetched, repo, work_key};
use crate::digest::{DigestWindow, LineBody, Section};

fn app() -> App {
    let recent = chrono::Utc::now() - chrono::Duration::minutes(1);
    let repos = vec![
        fetched("alpha", recent),
        repo("bravo", RepoLocation::Primary),
        repo("charlie", RepoLocation::LocalOnly),
    ];
    let mut app = App::new(repos, String::new(), 120, 40);
    app.view = View::Detail;
    app.dataset = ["alpha", "bravo"]
        .iter()
        .map(|name| crate::github::GithubRepo {
            name: name.to_string(),
            pushed_at: recent,
            pr_count: 0,
            prs: vec![],
            ci_status: CiStatus::Unknown,
            ci_workflow: None,
            ci_duration: None,
            repo_kind: crate::model::RepoKind::Unknown,
            owner: None,
            disk_usage: None,
        })
        .collect();
    app.land_on(ListRow::Header(Section::GithubActivity));
    app
}

#[test]
fn nothing_is_claimed_off_a_header() {
    let mut app = app();
    app.land_on(ListRow::Repo(0));
    assert!(app.claim_digest_logs(Local::now()).is_empty());
}

#[test]
fn a_touched_fetched_clone_is_read_once_per_cutoff_and_refs() {
    let mut app = app();
    let now = Local::now();
    let first = app.claim_digest_logs(now);
    assert_eq!(
        first.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        vec!["~/repos/alpha"],
        "only the fetched clone is read; bravo shows its push"
    );
    assert!(
        app.claim_digest_logs(now).is_empty(),
        "running: not asked twice"
    );
    let key = first[0].key.clone();
    app.apply_digest_log("~/repos/alpha", &key, vec![commit("a1", "A", "x")]);
    assert!(
        app.claim_digest_logs(now).is_empty(),
        "held: a sweep that moves no ref reads none"
    );
    let digest = app.digest(now).unwrap();
    assert!(matches!(&digest.lines[0].body, LineBody::Counted { commits } if commits.len() == 1));
    assert!(matches!(digest.lines[1].body, LineBody::PushedOnly));

    // A sweep that moved a ref asks again; the old answer is not drawn.
    app.repos[0].work_key = Some(work_key("moved"));
    assert!(matches!(
        app.digest(now).unwrap().lines[0].body,
        LineBody::Reading
    ));
    let second = app.claim_digest_logs(now);
    assert_eq!(second.len(), 1);
    // The first read landing late is dropped.
    app.apply_digest_log("~/repos/alpha", &key, vec![commit("old", "A", "x")]);
    assert!(matches!(
        app.digest(now).unwrap().lines[0].body,
        LineBody::Reading
    ));

    // A new window is a new cutoff.
    app.apply_digest_log("~/repos/alpha", &second[0].key, vec![]);
    app.digest_window = DigestWindow::Week;
    assert_eq!(app.claim_digest_logs(now).len(), 1);
}

#[test]
fn landing_on_a_header_hides_the_repo_and_leaving_restores_its_place() {
    let mut app = app();
    assert_eq!(app.on_header(), Some(Section::GithubActivity));
    assert!(app.selected_repo().is_none());
    assert!(app.selected_repo_index().is_none());
    app.history_view = HistoryView::Reflog;
    app.move_down();
    assert_eq!(app.on_header(), None);
    assert_eq!(app.selected, 0);
    assert_eq!(app.history_view, HistoryView::Reflog);
}

/// FZ-62 review F2: with the reader focused in `notes`, the cursor
/// landing on a header (key, wheel, click or hover dwell, all through
/// `land_on`) drops the reader from the screen, so focus moves to pane 2,
/// where `j`/`k` walk the digest. Fails while focus stays on the reader.
#[test]
fn landing_on_a_header_takes_focus_off_the_reader() {
    let mut app = app();
    app.move_down();
    assert_eq!(app.on_header(), None);
    app.history_view = HistoryView::Notes;
    app.focus_panel(FocusedPanel::Reader);
    assert_eq!(app.focused_panel, FocusedPanel::Reader);

    app.move_up();
    assert_eq!(app.on_header(), Some(Section::GithubActivity));
    assert!(!app.notes_layout(), "no reader on a header");
    assert_eq!(app.focused_panel, FocusedPanel::Detail);
}

/// Review F1: someone else's push, fetched, moves only `origin/*`: HEAD
/// is where it was and only the ref signature changes. That is a new
/// read, and the old answer is not drawn.
#[test]
fn a_moved_remote_ref_alone_asks_for_a_new_read() {
    let mut app = app();
    let now = Local::now();
    let first = app.claim_digest_logs(now);
    app.apply_digest_log("~/repos/alpha", &first[0].key, vec![commit("a1", "A", "x")]);
    let mut moved = work_key("abc");
    moved.ref_signature += 1;
    app.repos[0].work_key = Some(moved);
    assert!(matches!(
        app.digest(now).unwrap().lines[0].body,
        LineBody::Reading
    ));
    let second = app.claim_digest_logs(now);
    assert_eq!(second.len(), 1, "a new read for the moved remote ref");
    assert_ne!(second[0].key, first[0].key);
}

/// Review F2: the round trip through a header keeps the repo pane's
/// cursor and scroll, not only its view.
#[test]
fn a_header_round_trip_keeps_the_detail_cursor_and_scroll() {
    let mut app = app();
    app.land_on(ListRow::Repo(0));
    app.history_view = HistoryView::Reflog;
    app.set_detail_selected(3);
    app.set_detail_scroll(5);
    app.move_up();
    assert_eq!(app.on_header(), Some(Section::GithubActivity));
    app.move_down();
    assert_eq!(app.on_header(), None);
    assert_eq!(app.selected, 0);
    assert_eq!(app.detail_selected(), 3);
    assert_eq!(app.detail_scroll(), 5);
}

/// Review F2b: the compact grid draws no headers, so a stop left from
/// the detail view is the section's first repo there.
#[test]
fn a_header_stop_is_its_first_repo_in_the_compact_view() {
    let mut app = app();
    assert_eq!(app.on_header(), Some(Section::GithubActivity));
    app.toggle_view();
    assert_eq!(app.view, View::Compact);
    assert_eq!(app.on_header(), None);
    assert_eq!(app.selected_repo().map(|r| r.name.as_str()), Some("alpha"));
}

#[test]
fn a_search_drops_the_headers_and_the_stop() {
    let mut app = app();
    app.search_query = "alp".to_string();
    assert_eq!(app.list_rows(), vec![ListRow::Results, ListRow::Repo(0)]);
    assert_eq!(app.on_header(), None);
}

#[test]
fn a_lone_section_has_no_header_to_stop_on() {
    let mut app = App::new(
        vec![
            repo("a", RepoLocation::Primary),
            repo("b", RepoLocation::Primary),
        ],
        String::new(),
        120,
        40,
    );
    app.view = View::Detail;
    assert_eq!(app.list_rows(), vec![ListRow::Repo(0), ListRow::Repo(1)]);
    app.move_up();
    assert_eq!(app.on_header(), None);
}
