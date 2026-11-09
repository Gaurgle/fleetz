use super::*;

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

/// An idle loop paints as soon as it has something to paint. This is the
/// ordinary case and must not be delayed by the coalescing at all.
#[test]
fn an_empty_queue_paints_immediately() {
    assert!(should_paint(true, 0, false));
}

/// An animation frame is a deadline, not a data update: it is never held,
/// however much background work is queued behind it. Holding them is what
/// made the intro stutter under a release build's faster repo probes.
#[test]
fn an_animation_frame_is_never_held() {
    assert!(should_paint(false, 0, true));
    assert!(should_paint(false, MAX_COALESCED_FRAMES - 1, true));
    assert!(should_paint(true, 0, true));
}

/// The bug: a trackpad flick delivers wheel events faster than a repaint
/// takes, so painting one frame per event let the queue run away and the
/// scroll froze, then played out the backlog after the gesture had ended.
#[test]
fn a_queued_burst_holds_the_paint() {
    assert!(!should_paint(false, 0, false));
    assert!(!should_paint(false, 1, false));
    assert!(!should_paint(false, MAX_COALESCED_FRAMES - 1, false));
}

/// The hold is bounded, so a source that never stops arriving still gets
/// drawn rather than freezing the pane for as long as it lasts.
#[test]
fn a_queue_that_never_drains_still_paints_eventually() {
    assert!(should_paint(false, MAX_COALESCED_FRAMES, false));
    assert!(should_paint(false, MAX_COALESCED_FRAMES + 1, false));
}

/// `intro = false` puts no loading field up, and neither does a launch
/// with no load running. `intro = true` with a load running does.
#[test]
fn intro_off_shows_no_loading_field() {
    assert!(!shows_loading_field(false, true));
    assert!(!shows_loading_field(false, false));
    assert!(!shows_loading_field(true, false));
    assert!(shows_loading_field(true, true));
}

/// An empty app at launch, with the loading field up as `run_event_loop`
/// puts it.
fn app_under_loading_field() -> App {
    let mut app = App::new(Vec::new(), String::new(), 120, 40);
    app.loading = true;
    app.loading_field = Some(ui::intro::Loading::new());
    app
}

/// Whether any row of `buf` holds the top row of the `LOADING` banner.
fn shows_loading_banner(buf: &ratatui::buffer::Buffer) -> bool {
    (0..buf.area.height).any(|y| {
        let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
        row.contains("█    ████ ████ ███  ███ █  █ ████")
    })
}

/// The loading field is up until the load lands, and the very next frame
/// is the finished screen exactly: nothing is drawn in between, so there is
/// no wave frame and no noise left over. The removed intro wave painted
/// 1.5 s of frames here.
#[test]
fn the_frame_after_the_load_lands_is_the_finished_screen() {
    use ratatui::backend::TestBackend;
    let mut app = app_under_loading_field();
    let running = std::sync::atomic::AtomicBool::new(true);
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|frame| draw_screen(frame, &app)).unwrap();
    assert!(
        shows_loading_banner(term.backend().buffer()),
        "the loading field was not painted while the load ran"
    );

    // What the `Loaded` arm does: the load is applied, then the field goes.
    app.loading = false;
    dismiss_loading_field(&mut app, &running);
    assert!(app.loading_field.is_none(), "the field is still up");
    assert!(
        !running.load(std::sync::atomic::Ordering::Relaxed),
        "the field's frame timer is still running"
    );
    term.draw(|frame| draw_screen(frame, &app)).unwrap();

    let mut finished = Terminal::new(TestBackend::new(120, 40)).unwrap();
    finished.draw(|frame| ui::render(frame, &app)).unwrap();
    assert_eq!(
        term.backend().buffer(),
        finished.backend().buffer(),
        "the frame after the load is not the finished screen"
    );
}

/// The bug (#64): a `LoadingFrame` queued behind `Loaded` arrives after the
/// load's paint was held for it, and overwrote `needs_redraw` with false.
/// The held paint was lost and the loading field stayed on screen until
/// something else asked for a redraw.
#[test]
fn a_late_loading_frame_keeps_the_held_paint_of_the_finished_screen() {
    let mut app = app_under_loading_field();
    let running = std::sync::atomic::AtomicBool::new(true);

    // What the `Loaded` arm does: the load is applied, the field goes, and
    // the finished screen is asked for.
    app.loading = false;
    dismiss_loading_field(&mut app, &running);
    app.needs_redraw = true;
    // The late frame is already queued and nothing animates, so the loop
    // holds that paint for it and leaves the flag set.
    assert!(!should_paint(false, 0, app.loading_field.is_some()));

    on_loading_frame(&mut app);
    assert!(
        app.needs_redraw,
        "the late frame cancelled the held paint of the finished screen"
    );
    // The queue is drained now, so the finished screen paints at once.
    assert!(should_paint(true, 1, app.loading_field.is_some()));
}

/// While the field is up, each of its frames asks for a repaint.
#[test]
fn a_loading_frame_while_the_field_is_up_asks_for_a_redraw() {
    let mut app = app_under_loading_field();
    app.needs_redraw = false;
    on_loading_frame(&mut app);
    assert!(app.needs_redraw);
}

/// A frame that arrives after the field is gone, with nothing pending,
/// does not invent a repaint.
#[test]
fn a_late_loading_frame_with_no_pending_paint_asks_for_none() {
    let mut app = app_under_loading_field();
    let running = std::sync::atomic::AtomicBool::new(true);
    dismiss_loading_field(&mut app, &running);
    app.needs_redraw = false;
    on_loading_frame(&mut app);
    assert!(!app.needs_redraw);
}

/// A resize while the field is up paints the field at the new size, banner
/// re-centred and no cell left over from the old layout.
#[test]
fn a_resize_while_loading_paints_the_field_at_the_new_size() {
    use ratatui::backend::TestBackend;
    let app = app_under_loading_field();
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|frame| draw_screen(frame, &app)).unwrap();

    term.backend_mut().resize(80, 24);
    term.draw(|frame| draw_screen(frame, &app)).unwrap();
    let buf = term.backend().buffer();
    assert_eq!(buf.area, ratatui::layout::Rect::new(0, 0, 80, 24));
    assert!(
        shows_loading_banner(buf),
        "the banner is gone after a resize"
    );

    let mut fresh = ratatui::buffer::Buffer::empty(buf.area);
    app.loading_field
        .as_ref()
        .expect("the field")
        .paint(&mut fresh);
    let differing_symbols = (0..24u16)
        .flat_map(|y| (0..80u16).map(move |x| (x, y)))
        .filter(|(x, y)| {
            // The noise churns with the clock, so compare where the banner
            // sits, which is still and derived from the area alone.
            buf[(*x, *y)].symbol().starts_with('█') != fresh[(*x, *y)].symbol().starts_with('█')
        })
        .count();
    assert_eq!(
        differing_symbols, 0,
        "the banner is not where a fresh field at the new size puts it"
    );
}

fn test_repo() -> RepoStatus {
    RepoStatus {
        daily: None,
        daily_key: None,
        last_local: None,
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
        graph_expanded: Vec::new(),
        repo_kind: model::RepoKind::Unknown,
        owner: None,
        remote_url: None,
        remote_only_branches: vec![],
        remote_branches: vec![],
        work_key: None,
        fetch: model::FetchFacts::default(),
        authors: Default::default(),
    }
}

fn key(head: &str) -> git::WorkKey {
    git::WorkKey {
        head_sha: head.to_string(),
        index_mtime: None,
        index_size: 0,
        stash_size: 0,
        head_log_size: 0,
        head_ref: None,
        ref_signature: 0,
    }
}

/// A missing key is never "unchanged": two sweeps whose ref queries both
/// failed must both re-probe, not reuse each other's empty answer.
#[test]
fn two_missing_work_keys_never_count_as_unchanged() {
    assert!(!key_unchanged(&None, &None));
    assert!(!key_unchanged(&None, &Some(key("aaa"))));
    assert!(!key_unchanged(&Some(key("aaa")), &None));
    assert!(!key_unchanged(&Some(key("aaa")), &Some(key("bbb"))));
    assert!(key_unchanged(&Some(key("aaa")), &Some(key("aaa"))));
}

// --- FZ-02: a local sweep result must not undo a newer GitHub poll ---
//
// The race, driven with controlled events: a sweep snapshots the row
// (`sweep_targets`) while CI is pending, a poll lands and says passed,
// then the sweep's result lands carrying the snapshot's GitHub fields.

const SWEPT_PATH: &str = "~/repos/r";

/// An app with one cloned repo whose CI is pending, as at launch.
fn app_with_pending_repo() -> App {
    let mut repo = test_repo();
    repo.path = Some(SWEPT_PATH.to_string());
    repo.ci_status = model::CiStatus::Pending;
    repo.ci_workflow = Some("ci".to_string());
    repo.ci_duration = None;
    repo.pr_count = 0;
    repo.repo_kind = model::RepoKind::Public;
    repo.owner = Some("Gaurgle".to_string());
    repo.remote_url = Some("git@github.com:Gaurgle/r.git".to_string());
    repo.groups = vec!["work".to_string()];
    repo.work_key = Some(key("aaa"));
    App::new(vec![repo], "h".to_string(), 120, 44)
}

/// What `rebuild_repo_status` returns: the snapshot taken when the sweep
/// started, with every local probe re-run and moved on.
fn rebuilt_from(snapshot: &RepoStatus) -> RepoStatus {
    let mut status = snapshot.clone();
    status.dirty = 3;
    status.ahead = 2;
    status.behind = 1;
    status.branches = vec![model::BranchStatus {
        name: "feature".to_string(),
        upstream: None,
        behind: 1,
        ahead: 2,
        is_head: true,
        tip: "bbb".to_string(),
    }];
    status.commits = vec![commit_template("bbb")];
    status.stash_entries = vec!["stash@{0}: WIP".to_string()];
    status.stash_count = 1;
    status.worktrees = vec![model::Worktree {
        path: "~/repos/r-wt".to_string(),
        branch: "wt".to_string(),
        ..Default::default()
    }];
    status.remote_only_branches = vec!["origin/other".to_string()];
    status.work_key = Some(key("bbb"));
    status
}

fn commit_template(hash: &str) -> model::Commit {
    model::Commit {
        hash: hash.to_string(),
        message: "work".to_string(),
        body: String::new(),
        author: "a".to_string(),
        date: None,
        insertions: 0,
        deletions: 0,
        files: vec![],
    }
}

/// A successful poll reporting `r` as passed with one open PR.
fn poll_says_passed() -> Result<Box<github::GithubFetch>, String> {
    let pr = model::PullRequest {
        number: 7,
        title: "Fix it".to_string(),
        author: "someone".to_string(),
        created_at: chrono::Utc::now(),
        review_status: model::ReviewStatus::Approved,
        url: "https://github.com/Gaurgle/r/pull/7".to_string(),
    };
    Ok(Box::new(github::GithubFetch {
        repos: vec![github::GithubRepo {
            name: "r".to_string(),
            pushed_at: chrono::Utc::now(),
            pr_count: 1,
            prs: vec![pr],
            ci_status: model::CiStatus::Passed,
            ci_workflow: Some("release".to_string()),
            ci_duration: Some("1m 4s".to_string()),
            repo_kind: model::RepoKind::Private,
            owner: Some("Gaurgle".to_string()),
            disk_usage: None,
        }],
        calendar: None,
        has_pr_counts: true,
        origin: github::Origin::Network,
        meta: Default::default(),
    }))
}

fn poll(app: &mut App, result: Result<Box<github::GithubFetch>, String>) {
    let result = result.map_err(|message| github::FetchError {
        failure: github::Failure::new(github::FailureKind::Http, message),
        fallback: None,
        meta: Default::default(),
    });
    poll_with(app, result);
}

/// Start an attempt and land `result` as its answer.
fn poll_with(app: &mut App, result: Result<Box<github::GithubFetch>, github::FetchError>) {
    let attempt = claim_github_poll(app).expect("no poll in flight");
    answer(app, attempt, result);
}

/// Land `result` as the answer to `attempt`.
fn answer(
    app: &mut App,
    attempt: u64,
    result: Result<Box<github::GithubFetch>, github::FetchError>,
) {
    // No previous dataset, so no remote "moved" and nothing is spawned:
    // the receiver is never read and no runtime is needed.
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    apply_github_poll(app, attempt, result, &tx, &None);
}

fn assert_poll_fields(repo: &RepoStatus) {
    assert_eq!(repo.ci_status, model::CiStatus::Passed);
    assert_eq!(repo.ci_workflow.as_deref(), Some("release"));
    assert_eq!(repo.ci_duration.as_deref(), Some("1m 4s"));
    assert_eq!(repo.pr_count, 1);
    assert_eq!(repo.prs.len(), 1);
    assert_eq!(repo.prs[0].number, 7);
    assert_eq!(repo.repo_kind, model::RepoKind::Private);
}

fn assert_sweep_fields(repo: &RepoStatus) {
    assert_eq!(repo.dirty, 3);
    assert_eq!(repo.ahead, 2);
    assert_eq!(repo.behind, 1);
    assert_eq!(repo.branches.len(), 1);
    assert_eq!(repo.branches[0].name, "feature");
    assert_eq!(repo.commits.len(), 1);
    assert_eq!(repo.commits[0].hash, "bbb");
    assert_eq!(repo.stash_count, 1);
    assert_eq!(repo.stash_entries.len(), 1);
    assert_eq!(repo.worktrees.len(), 1);
    assert_eq!(repo.remote_only_branches, vec!["origin/other".to_string()]);
    assert_eq!(
        repo.work_key.as_ref().map(|k| k.head_sha.as_str()),
        Some("bbb")
    );
}

/// The regression: local start, GitHub update, local finish. The sweep's
/// result restored the snapshot's `Pending` over the poll's `Passed`.
#[test]
fn a_sweep_landing_after_a_poll_keeps_the_polls_github_fields() {
    let mut app = app_with_pending_repo();
    let snapshot = sweep_targets(&app, None).remove(0).old;
    poll(&mut app, poll_says_passed());
    apply_repo_updated(
        &mut app,
        SWEPT_PATH,
        rebuilt_from(&snapshot),
        git::FetchOutcome::NotAttempted,
    );
    let repo = &app.repos[0];
    assert_poll_fields(repo);
    assert_sweep_fields(repo);
}

/// The other completion order: the sweep lands first, then the poll.
/// Both halves must end up on the row either way.
#[test]
fn a_poll_landing_after_a_sweep_keeps_the_sweeps_local_fields() {
    let mut app = app_with_pending_repo();
    let snapshot = sweep_targets(&app, None).remove(0).old;
    apply_repo_updated(
        &mut app,
        SWEPT_PATH,
        rebuilt_from(&snapshot),
        git::FetchOutcome::NotAttempted,
    );
    poll(&mut app, poll_says_passed());
    let repo = &app.repos[0];
    assert_poll_fields(repo);
    assert_sweep_fields(repo);
}

/// A failed poll changes no GitHub field, so the row keeps the last good
/// values, and the sweep landing after it still updates the local half.
#[test]
fn a_failed_poll_mid_sweep_leaves_github_fields_and_lands_the_sweep() {
    let mut app = app_with_pending_repo();
    poll(&mut app, poll_says_passed());
    let snapshot = sweep_targets(&app, None).remove(0).old;
    poll(&mut app, Err("HTTP 502".to_string()));
    apply_repo_updated(
        &mut app,
        SWEPT_PATH,
        rebuilt_from(&snapshot),
        git::FetchOutcome::Succeeded,
    );
    assert!(app.github_freshness.is_failing());
    let repo = &app.repos[0];
    assert_poll_fields(repo);
    assert_sweep_fields(repo);
}

fn local(h: u32, m: u32) -> chrono::DateTime<chrono::Local> {
    use chrono::TimeZone;
    chrono::Local
        .with_ymd_and_hms(2026, 10, 1, h, m, 0)
        .single()
        .expect("unambiguous local time")
}

/// The payload a failed request finds on disk: the same repo, but CI as
/// it was when the cache was written.
fn offline_with_cache(written: Option<chrono::DateTime<chrono::Local>>) -> github::FetchError {
    let mut cached = poll_says_passed().ok().unwrap();
    cached.repos[0].ci_status = model::CiStatus::Failed;
    cached.origin = github::Origin::Cache { written };
    github::FetchError {
        failure: github::Failure::new(github::FailureKind::Offline, "no route"),
        fallback: Some(cached),
        meta: Default::default(),
    }
}

/// The case that was invisible. `fetch_repos` answered an offline poll
/// with the cache as `Ok`, and this cleared the failure, restarted the
/// clock and wrote the cache's older CI over the rows. Now it is a
/// failure: no row changes, the data time stays, the marker shows.
#[test]
fn a_failed_background_poll_with_a_cache_present_changes_no_row() {
    let mut app = app_with_pending_repo();
    poll(&mut app, poll_says_passed());
    let data_time = app.github_freshness.data_time;
    poll_with(&mut app, Err(offline_with_cache(Some(local(9, 0)))));

    assert_poll_fields(&app.repos[0]);
    assert!(app.github_freshness.is_failing(), "offline looked healthy");
    assert_eq!(app.github_freshness.data_time, data_time);
    assert!(
        ui::github_freshness_segment(&app.github_freshness, chrono::Local::now())
            .is_some_and(|(long, _, _)| long.contains("⚠ github offline")),
        "no marker for an offline poll"
    );
}

/// Rule 7: the first success after a failure clears it on the same frame
/// and lands its data.
#[test]
fn a_successful_poll_after_a_failure_clears_it() {
    let mut app = app_with_pending_repo();
    poll_with(&mut app, Err(offline_with_cache(None)));
    assert!(app.github_freshness.is_failing());
    poll(&mut app, poll_says_passed());
    assert_eq!(app.github_freshness.outcome, app::Outcome::Success);
    assert!(matches!(
        app.github_freshness.data_time,
        app::DataTime::At(_)
    ));
    assert!(ui::github_freshness_segment(&app.github_freshness, chrono::Local::now()).is_none());
    assert_poll_fields(&app.repos[0]);
}

fn watching(app: &mut App) -> config::Config {
    app.watch_mode = true;
    config::Config {
        github_poll_interval: 90,
        ..config::Config::default()
    }
}

/// Rule 6: a failure advances the retry clock, so the next automatic
/// attempt waits one whole interval. Not at once, and not in a loop.
#[test]
fn no_attempt_sooner_than_one_interval_after_a_failure() {
    let mut app = app_with_pending_repo();
    app.repos[0].ci_status = model::CiStatus::Passed;
    let config = watching(&mut app);
    poll(&mut app, Err("HTTP 502".to_string()));
    let failed_at = app.github_freshness.last_attempt.expect("clock advanced");

    let early = failed_at + chrono::Duration::seconds(89);
    assert!(!github_poll_due(&app, &config, early), "retried early");
    let on_time = failed_at + chrono::Duration::seconds(90);
    assert!(github_poll_due(&app, &config, on_time));

    // A detected push does not jump the queue while polls fail either.
    app.push_detected = true;
    assert!(!github_poll_due(&app, &config, early), "push retried early");
}

/// Decision D4: a poll that never answers is recorded as a timed-out
/// failure and advances the clock, so the next tick does not open a
/// second request beside the first.
#[test]
fn a_poll_given_up_on_is_recorded_as_a_timed_out_failure() {
    let mut app = app_with_pending_repo();
    let config = watching(&mut app);
    claim_github_poll(&mut app).unwrap();
    app.github_poll_started = std::time::Instant::now()
        .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
    let now = chrono::Local::now();
    expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, now);

    assert!(!app.github_poll_inflight);
    assert!(matches!(
        &app.github_freshness.outcome,
        app::Outcome::Failed(f) if f.kind == github::FailureKind::TimedOut
    ));
    assert_eq!(app.github_freshness.last_attempt, Some(now));
    assert!(!github_poll_due(&app, &config, now), "restarted at once");
}

/// Rule 10: once a newer attempt has been applied, a late answer from
/// the one given up on is discarded, so the rows and the data time
/// never go backwards.
#[test]
fn an_answer_from_an_older_attempt_is_discarded_once_a_newer_one_applied() {
    let mut app = app_with_pending_repo();
    let first = claim_github_poll(&mut app).unwrap();
    app.github_poll_started = std::time::Instant::now()
        .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
    expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, chrono::Local::now());
    let second = claim_github_poll(&mut app).unwrap();
    answer(&mut app, second, Ok(poll_says_passed().ok().unwrap()));
    let applied = app.github_freshness.clone();

    let mut late = poll_says_passed().ok().unwrap();
    late.repos[0].ci_status = model::CiStatus::Failed;
    late.repos[0].pushed_at = app.dataset[0].pushed_at;
    answer(&mut app, first, Ok(late));

    assert_poll_fields(&app.repos[0]);
    assert_eq!(app.github_freshness, applied);
}

/// The other order: the given-up attempt answers while the newer one is
/// still out. Its data is the newest there is, so it lands, but the lock
/// belongs to the newer request and stays held.
#[test]
fn a_late_answer_does_not_release_the_newer_attempts_lock() {
    let mut app = app_with_pending_repo();
    let first = claim_github_poll(&mut app).unwrap();
    app.github_poll_started = std::time::Instant::now()
        .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
    expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, chrono::Local::now());
    let _second = claim_github_poll(&mut app).unwrap();
    answer(&mut app, first, Ok(poll_says_passed().ok().unwrap()));

    assert_poll_fields(&app.repos[0]);
    assert!(app.github_poll_inflight, "the newer request lost its lock");
    assert!(app.github_poll_started.is_some());
}

/// Decision D3: a `Pending` frozen by a failing poll does not hold the
/// interval at the floor.
#[test]
fn a_pending_repo_does_not_shorten_the_interval_while_polls_fail() {
    let config = config::Config {
        github_poll_interval: 90,
        ..config::Config::default()
    };
    let mut app = app_with(&[model::CiStatus::Pending]);
    assert_eq!(github_poll_secs(&app, &config), GITHUB_POLL_MIN as i64);
    app.github_freshness.failed(
        github::Failure::new(github::FailureKind::Offline, "no route"),
        chrono::Local::now(),
    );
    assert_eq!(github_poll_secs(&app, &config), 90);
}

fn launch(app: &mut App, kind: LoadKind, source: app::DatasetSource) {
    apply_load_result(
        app,
        &config::Config::default(),
        Ok(Box::new(LoadResult {
            kind,
            repos: Vec::new(),
            header: "h".to_string(),
            dataset: Some(Vec::new()),
            github_calendar: None,
            source,
            ambiguity: None,
            meta: None,
        })),
    );
}

/// Decision D2: a cache-served launch is followed by one attempt even with
/// watch off, and only one. Watch decides whether polling continues.
#[test]
fn a_cache_served_launch_with_watch_off_makes_exactly_one_attempt() {
    for outcome in [Ok(()), Err(())] {
        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        let config = config::Config::default();
        assert!(!app.watch_mode);
        assert!(
            !github_poll_due(&app, &config, chrono::Local::now()),
            "before load"
        );
        launch(
            &mut app,
            LoadKind::Initial,
            app::DatasetSource::Cache {
                written: Some(local(8, 0)),
            },
        );
        assert!(github_poll_due(&app, &config, chrono::Local::now()));

        let attempt = claim_github_poll(&mut app).unwrap();
        assert!(!github_poll_due(&app, &config, chrono::Local::now()));
        let result = match outcome {
            Ok(()) => poll_says_passed(),
            Err(()) => Err("HTTP 502".to_string()),
        };
        let result = result.map_err(|m| github::FetchError {
            failure: github::Failure::new(github::FailureKind::Http, m),
            fallback: None,
            meta: Default::default(),
        });
        answer(&mut app, attempt, result);

        let tomorrow = chrono::Local::now() + chrono::Duration::days(1);
        assert!(
            !github_poll_due(&app, &config, tomorrow),
            "watch off kept polling after {outcome:?}"
        );
    }
}

/// A network-served launch with watch off owes nothing more.
#[test]
fn a_network_served_launch_with_watch_off_does_not_poll() {
    let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
    launch(&mut app, LoadKind::Initial, app::DatasetSource::Network);
    let tomorrow = chrono::Local::now() + chrono::Duration::days(1);
    assert!(!github_poll_due(&app, &config::Config::default(), tomorrow));
}

/// Rule 1 with a planted age: the cache's write time is the data time,
/// and no attempt is recorded, so the poll stays due.
#[test]
fn a_launch_from_a_cache_of_planted_age_carries_that_age() {
    let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
    let written = chrono::Local::now() - chrono::Duration::hours(3);
    launch(
        &mut app,
        LoadKind::Initial,
        app::DatasetSource::Cache {
            written: Some(written),
        },
    );
    assert_eq!(app.github_freshness.data_time, app::DataTime::At(written));
    assert_eq!(app.github_freshness.outcome, app::Outcome::NotAttempted);
    assert!(app.github_freshness.last_attempt.is_none());
}

/// A cache whose age cannot be read stays unknown: never the launch time.
#[test]
fn a_launch_from_a_cache_of_unreadable_age_leaves_the_age_unknown() {
    let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
    launch(
        &mut app,
        LoadKind::Initial,
        app::DatasetSource::Cache { written: None },
    );
    assert_eq!(app.github_freshness.data_time, app::DataTime::Unknown);
}

/// No usable cache and the request fails: the error panel, as before, and
/// the failed attempt is recorded so the retry clock advances.
#[test]
fn a_launch_with_no_usable_cache_records_the_failed_attempt() {
    let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
    let failure = github::Failure::new(github::FailureKind::Auth, "not authenticated");
    apply_load_result(
        &mut app,
        &config::Config::default(),
        Err(LoadError {
            message: failure.message.clone(),
            github_failure: Some(failure.clone()),
            budget: None,
        }),
    );
    assert_eq!(app.load_error.as_deref(), Some("not authenticated"));
    assert_eq!(app.github_freshness.outcome, app::Outcome::Failed(failure));
    assert!(app.github_freshness.last_attempt.is_some());
    assert_eq!(app.github_freshness.data_time, app::DataTime::NoData);
}

/// An `R` that went to GitHub, landing over the row `app_with_pending_repo`
/// draws, the way a reload rebuilds it: CI as the reload saw it.
fn reload_lands(app: &mut App, source: app::DatasetSource) {
    let mut repo = app.repos[0].clone();
    repo.ci_status = model::CiStatus::Passed;
    apply_load_result(
        app,
        &config::Config::default(),
        Ok(Box::new(LoadResult {
            kind: LoadKind::Reload,
            repos: vec![repo],
            header: "h".to_string(),
            dataset: Some(Vec::new()),
            github_calendar: None,
            source,
            ambiguity: None,
            meta: None,
        })),
    );
}

/// A poll answer older than the `R` that landed: CI as it was before.
fn stale_answer() -> Result<Box<github::GithubFetch>, github::FetchError> {
    let mut old = poll_says_passed().ok().unwrap();
    old.repos[0].ci_status = model::CiStatus::Failed;
    Ok(old)
}

/// Rule 10 with `R` as the newer attempt. A watch poll hangs and is
/// given up on, `R` succeeds, then the hung request answers. Loads never
/// touched the attempt ids, so the old payload was written over the
/// reload's rows and stamped fresh.
#[test]
fn a_late_poll_answer_after_a_successful_reload_is_discarded() {
    let mut app = app_with_pending_repo();
    let hung = claim_github_poll(&mut app).unwrap();
    app.github_poll_started = std::time::Instant::now()
        .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
    expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, chrono::Local::now());
    reload_lands(&mut app, app::DatasetSource::Network);
    let after_reload = app.github_freshness.clone();

    answer(&mut app, hung, stale_answer());

    assert_eq!(
        app.repos[0].ci_status,
        model::CiStatus::Passed,
        "rows rewound"
    );
    assert_eq!(app.github_freshness, after_reload, "freshness rewound");
    assert!(!app.github_poll_inflight);
    assert!(app.github_poll_started.is_none());
    let config = watching(&mut app);
    let later = chrono::Local::now() + chrono::Duration::seconds(90);
    assert!(
        github_poll_due(&app, &config, later),
        "polling never resumed"
    );
}

/// The mirror case: the poll is still in flight, not given up on, when
/// `R` lands, with an `r` waiting on it. `R` wins: it is applied first and
/// rebuilt every row, and an answer that cannot be ordered against it
/// must not overwrite it. The answer still releases the lock and settles
/// the waiting `r`, or polling and the spinner would stall for good.
#[test]
fn a_poll_in_flight_when_a_reload_lands_is_superseded_but_releases_its_lock() {
    let mut app = app_with_pending_repo();
    let in_flight = claim_github_poll(&mut app).unwrap();
    app.refresh_awaits_poll = true;
    reload_lands(&mut app, app::DatasetSource::Network);
    let after_reload = app.github_freshness.clone();
    assert!(
        app.github_poll_inflight,
        "the lock is the request's to release"
    );

    answer(&mut app, in_flight, stale_answer());

    assert_eq!(
        app.repos[0].ci_status,
        model::CiStatus::Passed,
        "rows rewound"
    );
    assert_eq!(app.github_freshness, after_reload);
    assert!(!app.github_poll_inflight, "lock left set");
    assert!(app.github_poll_started.is_none());
    assert!(!app.refresh_awaits_poll, "the waiting r never settled");
    assert!(claim_github_poll(&mut app).is_some(), "no poll can start");
}

/// A reload that fell back to the cache supersedes the same way: its rows
/// were rebuilt from the file, and a poll started before it is older.
#[test]
fn a_reload_fallback_also_supersedes_an_older_poll() {
    let mut app = app_with_pending_repo();
    let in_flight = claim_github_poll(&mut app).unwrap();
    reload_lands(
        &mut app,
        app::DatasetSource::Fallback {
            failure: github::Failure::new(github::FailureKind::Offline, "no route"),
            written: None,
        },
    );
    answer(&mut app, in_flight, stale_answer());
    assert_eq!(app.repos[0].ci_status, model::CiStatus::Passed);
    assert!(app.github_freshness.is_failing());
    assert!(!app.github_poll_inflight);
}

/// A cache-served launch made no attempt and is older than any poll, so
/// a poll in flight past it still lands.
#[test]
fn a_cache_served_load_does_not_supersede_a_poll() {
    let mut app = app_with_pending_repo();
    let in_flight = claim_github_poll(&mut app).unwrap();
    reload_lands(&mut app, app::DatasetSource::Cache { written: None });
    answer(
        &mut app,
        in_flight,
        poll_says_passed().map_err(|_| unreachable!()),
    );
    assert_poll_fields(&app.repos[0]);
    assert_eq!(app.github_freshness.outcome, app::Outcome::Success);
}

/// `R` exists to be current. One that could only rebuild from the cache
/// says so instead of `Reloaded`, and the header goes stale with the
/// cache file's age.
#[test]
fn a_reload_that_fell_back_to_the_cache_does_not_claim_a_reload() {
    let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
    let written = local(7, 30);
    launch(
        &mut app,
        LoadKind::Reload,
        app::DatasetSource::Fallback {
            failure: github::Failure::new(github::FailureKind::Offline, "no route"),
            written: Some(written),
        },
    );
    let status = app.status_message.as_ref().map(|m| m.text.clone()).unwrap();
    assert_ne!(status, "Reloaded");
    assert!(status.contains("cache"), "{status:?}");
    assert!(app.github_freshness.is_failing());
    assert!(app.github_freshness.last_attempt.is_some());
    assert_eq!(app.github_freshness.data_time, app::DataTime::At(written));

    launch(&mut app, LoadKind::Reload, app::DatasetSource::Network);
    let status = app.status_message.as_ref().map(|m| m.text.clone()).unwrap();
    assert_eq!(status, "Reloaded");
}

/// Rows the sweep does not probe keep their current values: identity,
/// groups and owner come from the load, not from the snapshot. The
/// selection is the app's, and a sweep result must not move it.
#[test]
fn a_sweep_result_keeps_identity_groups_and_selection() {
    let mut app = app_with_pending_repo();
    let snapshot = sweep_targets(&app, None).remove(0).old;
    let selected = app.selected;
    let mut status = rebuilt_from(&snapshot);
    // Whatever a stale snapshot carried for fields the sweep does not
    // own, the row as it is now wins.
    status.groups = vec![];
    status.owner = None;
    status.remote_url = None;
    apply_repo_updated(
        &mut app,
        SWEPT_PATH,
        status,
        git::FetchOutcome::NotAttempted,
    );
    let repo = &app.repos[0];
    assert_eq!(repo.name, "r");
    assert_eq!(repo.path.as_deref(), Some(SWEPT_PATH));
    assert_eq!(repo.groups, vec!["work".to_string()]);
    assert_eq!(repo.owner.as_deref(), Some("Gaurgle"));
    assert_eq!(
        repo.remote_url.as_deref(),
        Some("git@github.com:Gaurgle/r.git")
    );
    assert_eq!(app.selected, selected);
    assert_eq!(app.repos.len(), 1);
}

/// The single-repo sweep (`f`, a moved remote) reports `fetched: true`
/// and the full watch sweep often does not; both land through the same
/// merge, so the poll's fields survive either.
#[test]
fn single_and_full_sweep_results_merge_alike() {
    for fetched in [false, true] {
        let mut app = app_with_pending_repo();
        let single = sweep_targets(&app, Some(0)).remove(0).old;
        let full = sweep_targets(&app, None).remove(0).old;
        poll(&mut app, poll_says_passed());
        let snapshot = if fetched { single } else { full };
        let fetch = if fetched {
            git::FetchOutcome::Succeeded
        } else {
            git::FetchOutcome::NotAttempted
        };
        apply_repo_updated(&mut app, SWEPT_PATH, rebuilt_from(&snapshot), fetch);
        assert_poll_fields(&app.repos[0]);
        assert_sweep_fields(&app.repos[0]);
    }
}

/// A late result for a path that no longer has a row is dropped: it must
/// not create a row, nor land on an unrelated one.
#[test]
fn a_late_result_for_a_removed_repo_adds_no_row() {
    let mut app = app_with_pending_repo();
    let snapshot = sweep_targets(&app, None).remove(0).old;
    let mut other = test_repo();
    other.name = "other".to_string();
    other.path = Some("~/repos/other".to_string());
    app.repos = vec![other];
    apply_repo_updated(
        &mut app,
        SWEPT_PATH,
        rebuilt_from(&snapshot),
        git::FetchOutcome::Succeeded,
    );
    assert_eq!(app.repos.len(), 1);
    let repo = &app.repos[0];
    assert_eq!(repo.name, "other");
    assert_eq!(repo.dirty, 0);
    assert!(repo.commits.is_empty());
}

/// S15 of the repository identity design (section 3.6): a sweep result
/// lands by path, on whatever row a reload has put there since, even when
/// that row stands for another remote. Every field a sweep writes is a
/// probe of the directory, so it is right for the row at that path; the
/// row's identity, GitHub fields and groups stay the reload's.
#[test]
fn a_late_result_lands_by_path_on_the_row_a_reload_put_there() {
    let mut app = app_with_pending_repo();
    let snapshot = sweep_targets(&app, None).remove(0).old;
    let mut now_there = test_repo();
    now_there.name = "y".to_string();
    now_there.path = Some(SWEPT_PATH.to_string());
    now_there.owner = Some("Gaurgle".to_string());
    now_there.remote_url = Some("git@github.com:Gaurgle/y.git".to_string());
    now_there.ci_status = model::CiStatus::Passed;
    now_there.groups = vec!["side".to_string()];
    app.repos = vec![now_there];

    apply_repo_updated(
        &mut app,
        SWEPT_PATH,
        rebuilt_from(&snapshot),
        git::FetchOutcome::NotAttempted,
    );

    assert_eq!(app.repos.len(), 1);
    let repo = &app.repos[0];
    assert_eq!(repo.name, "y");
    assert_eq!(repo.owner.as_deref(), Some("Gaurgle"));
    assert_eq!(
        repo.remote_url.as_deref(),
        Some("git@github.com:Gaurgle/y.git")
    );
    assert_eq!(repo.ci_status, model::CiStatus::Passed);
    assert_eq!(repo.groups, vec!["side".to_string()]);
    assert_sweep_fields(repo);
}

// --- FZ-04: fetch outcomes land as outcomes, not as "requested" ---

fn fetch_failed(kind: git::FetchFailureKind) -> git::FetchOutcome {
    git::FetchOutcome::Failed(git::FetchFailure {
        kind,
        detail: Some("fatal: '/gone' does not appear to be a git repository".to_string()),
    })
}

/// One fetch that worked: the row says so and has a success time.
#[test]
fn a_successful_fetch_lands_as_a_success() {
    let mut app = app_with_pending_repo();
    let snapshot = sweep_targets(&app, Some(0)).remove(0).old;
    apply_repo_updated(
        &mut app,
        SWEPT_PATH,
        rebuilt_from(&snapshot),
        git::FetchOutcome::Succeeded,
    );
    let facts = &app.repos[0].fetch;
    assert_eq!(facts.outcome, git::FetchOutcome::Succeeded);
    assert!(facts.last_success.is_some());
}

/// Rule 1 and rule 4: a fetch from a remote that is gone lands as a
/// failure, not as a fetch. The rebuilt local state still lands, the
/// last success stays where it was, and nothing claims the refs moved.
#[test]
fn a_failed_fetch_lands_as_a_failure_and_keeps_the_last_success() {
    let mut app = app_with_pending_repo();
    let earlier = chrono::Local::now() - chrono::Duration::hours(1);
    app.repos[0].fetch = model::FetchFacts::from_disk(Some(earlier.into()));
    let snapshot = sweep_targets(&app, Some(0)).remove(0).old;
    apply_repo_updated(
        &mut app,
        SWEPT_PATH,
        rebuilt_from(&snapshot),
        fetch_failed(git::FetchFailureKind::NoRemote),
    );
    let repo = &app.repos[0];
    assert_sweep_fields(repo);
    assert_eq!(
        repo.fetch.failure().map(|f| f.kind),
        Some(git::FetchFailureKind::NoRemote)
    );
    assert_eq!(repo.fetch.last_success, Some(earlier));
    assert!(repo.fetch.last_attempt.is_some());
}

/// A timeout is a failure of its own kind, settled like any other.
#[test]
fn a_timed_out_fetch_lands_as_timed_out_and_settles() {
    let mut app = app_with_pending_repo();
    app.refresh_awaits_poll = true;
    app.settle_refresh_poll([SWEPT_PATH.to_string()]);
    let snapshot = sweep_targets(&app, Some(0)).remove(0).old;
    let timed_out = git::FetchOutcome::Failed(git::FetchFailure {
        kind: git::FetchFailureKind::TimedOut,
        detail: None,
    });
    apply_repo_updated(&mut app, SWEPT_PATH, rebuilt_from(&snapshot), timed_out);
    assert_eq!(
        app.repos[0].fetch.failure().map(|f| f.kind),
        Some(git::FetchFailureKind::TimedOut)
    );
    assert!(
        !app.is_refreshing(),
        "a timed-out fetch left the spinner running"
    );
}

/// `r` pressed while the watch timer's sweep and poll are both in
/// flight, which with a short `refresh_interval` is most of the time.
/// Neither is started again, so nothing is spawned and no runtime is
/// needed. Returns the poll's attempt, for its answer.
fn r_during_a_watch_sweep(app: &mut App) -> u64 {
    app.watch_mode = true;
    app.sweep_started = Some(std::time::Instant::now());
    app.sweep_manual = false;
    let attempt = claim_github_poll(app).expect("no poll in flight");
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let args = Args::parse_from(["fleetz"]);
    refresh_key(
        app,
        &tx,
        &args,
        &config::Config::default(),
        chrono::Local::now(),
    );
    assert!(app.is_refreshing(), "r did not start the spinner");
    attempt
}

/// FZ-85 (#142): the events of `r` pressed during a watch sweep, in the
/// order they arrive. The sweep lock made `r`'s own sweep a no-op and
/// left the running sweep unmarked, so the spinner keyed on the poll
/// alone: `AppEvent::Github` landed in a second and stopped it, and the
/// watch sweep's fetched `RepoUpdated` rows landed after it.
#[test]
fn r_during_a_watch_sweep_spins_until_that_sweeps_rows_land() {
    let mut app = fleet_app(2);
    let attempt = r_during_a_watch_sweep(&mut app);

    answer(&mut app, attempt, ok_with(None));
    assert!(
        app.is_refreshing(),
        "stopped at the poll, before the sweep's rows landed"
    );
    land(&mut app, 0, git::FetchOutcome::Succeeded);
    assert!(app.is_refreshing(), "stopped before the last row landed");
    land(&mut app, 1, git::FetchOutcome::Succeeded);
    // `spawn_sweep` sends `SweepDone` once every row has been sent.
    app.sweep_landed(chrono::Local::now());
    assert!(!app.is_refreshing(), "spun on after the last row landed");
}

/// FZ-85 (#142): a fetch that fails still ends its part of the refresh,
/// both the poll's fetch of a moved remote and the sweep's own.
#[tokio::test]
async fn r_during_a_watch_sweep_ends_when_a_fetch_fails() {
    let mut app = missing_paths_app(2, "fz85");
    app.dataset = vec![
        gh("repo-0", "2026-11-05T10:00:00Z"),
        gh("repo-1", "2026-11-05T10:00:00Z"),
    ];
    let attempt = r_during_a_watch_sweep(&mut app);
    let (moved, swept) = (
        app.repos[0].path.clone().unwrap(),
        app.repos[1].path.clone().unwrap(),
    );
    let mut fetch = ok_with(None).unwrap();
    fetch.repos = vec![
        gh("repo-0", "2026-11-05T11:00:00Z"),
        gh("repo-1", "2026-11-05T10:00:00Z"),
    ];
    answer(&mut app, attempt, Ok(fetch));
    assert!(
        app.refresh_fetches.contains(&moved),
        "the push was not fetched"
    );

    let failed = fetch_failed(git::FetchFailureKind::Unreachable);
    let status = app.repos[1].clone();
    apply_repo_updated(&mut app, &swept, status, failed.clone());
    app.sweep_landed(chrono::Local::now());
    assert!(
        app.is_refreshing(),
        "stopped before the moved remote's fetch landed"
    );
    let status = app.repos[0].clone();
    apply_repo_updated(&mut app, &moved, status, failed);
    assert!(
        !app.is_refreshing(),
        "a failed fetch left the spinner running"
    );
}

/// Rule 2: a watch sweep that did not fetch lands its local fields and
/// leaves the failure, its time and the last success exactly as they were.
#[test]
fn a_sweep_that_did_not_fetch_leaves_the_fetch_facts_alone() {
    let mut app = app_with_pending_repo();
    let snapshot = sweep_targets(&app, Some(0)).remove(0).old;
    apply_repo_updated(
        &mut app,
        SWEPT_PATH,
        rebuilt_from(&snapshot),
        fetch_failed(git::FetchFailureKind::Auth),
    );
    let before = app.repos[0].fetch.clone();
    // The snapshot predates the failure; its copy of the facts must not
    // land either.
    apply_repo_updated(
        &mut app,
        SWEPT_PATH,
        rebuilt_from(&snapshot),
        git::FetchOutcome::NotAttempted,
    );
    assert_eq!(app.repos[0].fetch, before);
}

/// Recovery: the next fetch that works clears the failure.
#[test]
fn a_fetch_that_works_after_a_failure_recovers() {
    let mut app = app_with_pending_repo();
    let snapshot = sweep_targets(&app, Some(0)).remove(0).old;
    apply_repo_updated(
        &mut app,
        SWEPT_PATH,
        rebuilt_from(&snapshot),
        fetch_failed(git::FetchFailureKind::Unreachable),
    );
    apply_repo_updated(
        &mut app,
        SWEPT_PATH,
        rebuilt_from(&snapshot),
        git::FetchOutcome::Succeeded,
    );
    let facts = &app.repos[0].fetch;
    assert_eq!(facts.failure(), None);
    assert!(facts.last_success.is_some());
}

/// An app with `n` cloned repos at `~/repos/repo-<i>`.
fn fleet_app(n: usize) -> App {
    let repos = (0..n)
        .map(|i| {
            let mut r = test_repo();
            r.name = format!("repo-{i}");
            r.path = Some(format!("~/repos/repo-{i}"));
            r
        })
        .collect();
    App::new(repos, "h".to_string(), 120, 44)
}

fn land(app: &mut App, i: usize, fetch: git::FetchOutcome) {
    let path = format!("~/repos/repo-{i}");
    let status = app.repos[i].clone();
    apply_repo_updated(app, &path, status, fetch);
}

/// Rule 9: a batch someone asked for, some of whose fetches failed, says
/// how many, and only once the last has landed.
#[test]
fn a_mixed_sweep_reports_how_many_fetches_failed() {
    let mut app = fleet_app(3);
    app.manual_fetches
        .expect((0..3).map(|i| format!("~/repos/repo-{i}")));
    land(&mut app, 0, git::FetchOutcome::Succeeded);
    land(&mut app, 1, fetch_failed(git::FetchFailureKind::Auth));
    assert!(
        app.status_message.is_none(),
        "reported before the last landed"
    );
    land(
        &mut app,
        2,
        fetch_failed(git::FetchFailureKind::Unreachable),
    );
    let status = app.status_message.as_ref().expect("a report");
    assert_eq!(status.text, "2 of 3 fetches failed");
    assert!(status.is_error);
    assert_eq!(app.repos[0].fetch.failure(), None);
    assert!(app.repos[1].fetch.failure().is_some());
    assert!(app.repos[2].fetch.failure().is_some());
}

/// Nothing extra when every fetch worked.
#[test]
fn a_sweep_whose_fetches_all_worked_reports_nothing() {
    let mut app = fleet_app(2);
    app.manual_fetches
        .expect((0..2).map(|i| format!("~/repos/repo-{i}")));
    land(&mut app, 0, git::FetchOutcome::Succeeded);
    land(&mut app, 1, git::FetchOutcome::Succeeded);
    assert!(app.status_message.is_none());
}

/// `f` on one repo whose fetch fails names the repo and why.
#[test]
fn a_single_fetch_that_fails_names_the_repo() {
    let mut app = fleet_app(1);
    app.manual_fetches.expect(["~/repos/repo-0".to_string()]);
    land(&mut app, 0, fetch_failed(git::FetchFailureKind::NoRemote));
    let status = app.status_message.as_ref().expect("a report");
    assert_eq!(status.text, "fetch failed: repo-0, remote not found");
}

/// Rule 6: a failure starts nothing. No sweep, no poll, no push signal;
/// the repo is fetched again only when it would have been anyway.
#[test]
fn a_failed_fetch_schedules_nothing_new() {
    let mut app = fleet_app(1);
    let refreshed = app.last_refreshed;
    land(
        &mut app,
        0,
        fetch_failed(git::FetchFailureKind::Unreachable),
    );
    assert!(app.sweep_started.is_none());
    assert!(!app.github_poll_inflight);
    assert!(!app.push_detected);
    assert_eq!(app.last_refreshed, refreshed);
}

fn offline_outcome() -> app::Outcome {
    app::Outcome::Failed(github::Failure::new(
        github::FailureKind::Offline,
        "no route",
    ))
}

/// Rule 6, amended: only the step from an `Offline` poll to a success
/// selects, and only the rows the outage failed (`Unreachable`,
/// `TimedOut`), never one marked for another reason or a clean one.
#[test]
fn a_poll_recovering_from_offline_refetches_only_the_network_failures() {
    let mut app = fleet_app(7);
    land(
        &mut app,
        0,
        fetch_failed(git::FetchFailureKind::Unreachable),
    );
    land(&mut app, 1, fetch_failed(git::FetchFailureKind::TimedOut));
    land(&mut app, 2, fetch_failed(git::FetchFailureKind::Auth));
    land(&mut app, 3, fetch_failed(git::FetchFailureKind::NoRemote));
    land(&mut app, 4, fetch_failed(git::FetchFailureKind::Other));
    land(&mut app, 5, git::FetchOutcome::Succeeded);
    // Row 6 was never fetched.
    let success = app::Outcome::Success;

    assert_eq!(
        recovery_refetch(&offline_outcome(), &success, &app.repos),
        vec![0, 1]
    );
    assert!(recovery_refetch(&success, &success, &app.repos).is_empty());
    assert!(recovery_refetch(&app::Outcome::NotAttempted, &success, &app.repos).is_empty());
    let still_failing = app::Outcome::Failed(github::Failure::new(
        github::FailureKind::Server,
        "HTTP 502",
    ));
    assert!(recovery_refetch(&offline_outcome(), &still_failing, &app.repos).is_empty());
    assert!(recovery_refetch(&offline_outcome(), &offline_outcome(), &app.repos).is_empty());
    let other_failure = app::Outcome::Failed(github::Failure::new(
        github::FailureKind::TimedOut,
        "no answer",
    ));
    assert!(recovery_refetch(&other_failure, &success, &app.repos).is_empty());
}

/// Rule 6, amended, through the poll: a row the outage failed is in the
/// fetch batch of the first successful poll after an `Offline` one, the
/// way a moved remote is, and not in the next poll's when it failed
/// again: the recovery refetch happens once.
#[tokio::test]
async fn a_row_the_outage_failed_is_fetched_once_when_the_poll_recovers() {
    // A path that does not exist, so the spawned fetch touches nothing.
    let path = std::env::temp_dir()
        .join("fleetz-76-no-such-dir")
        .join("repo")
        .display()
        .to_string();
    let mut repo = test_repo();
    repo.path = Some(path.clone());
    let mut app = App::new(vec![repo], "h".to_string(), 120, 44);
    let status = app.repos[0].clone();
    let unreachable = || fetch_failed(git::FetchFailureKind::Unreachable);
    apply_repo_updated(&mut app, &path, status.clone(), unreachable());
    let ok = || {
        Ok(Box::new(github::GithubFetch {
            repos: Vec::new(),
            calendar: None,
            has_pr_counts: true,
            origin: github::Origin::Network,
            meta: Default::default(),
        }))
    };
    let offline = || {
        Err(github::FetchError {
            failure: github::Failure::new(github::FailureKind::Offline, "no route"),
            fallback: None,
            meta: Default::default(),
        })
    };

    app.refresh_awaits_poll = true;
    poll_with(&mut app, offline());
    assert!(app.refresh_fetches.is_empty(), "an offline poll fetched");

    app.refresh_awaits_poll = true;
    poll_with(&mut app, ok());
    assert_eq!(
        app.refresh_fetches.iter().collect::<Vec<_>>(),
        vec![&path],
        "the recovering poll did not refetch the failed row"
    );

    // Its refetch fails again; the next poll leaves it for the next
    // trigger.
    apply_repo_updated(&mut app, &path, status, unreachable());
    assert!(app.refresh_fetches.is_empty());
    app.refresh_awaits_poll = true;
    poll_with(&mut app, ok());
    assert!(
        app.refresh_fetches.is_empty(),
        "the recovery refetch repeated"
    );
    assert_eq!(
        app.repos[0].fetch.failure().map(|f| f.kind),
        Some(git::FetchFailureKind::Unreachable)
    );
}

/// A reload replaces every row; the session's fetch facts go across by
/// path, so a window apply cannot wipe a failure off a row.
#[test]
fn a_reload_carries_the_fetch_facts_by_path() {
    let mut app = fleet_app(2);
    land(&mut app, 1, fetch_failed(git::FetchFailureKind::Auth));
    let reloaded = fleet_app(2).repos;
    let carried = carry_fetch_facts(&app.repos, reloaded);
    assert_eq!(carried[0].fetch, model::FetchFacts::default());
    assert_eq!(carried[1].fetch, app.repos[1].fetch);
}

/// FZ-34 (#83), criterion 3: the watch sweep rebuilds a row held after
/// a timeout without fetching it, and fetches it again once the hold
/// (two intervals after the first timeout) has passed or a fetch
/// succeeded. Other failure kinds hold nothing.
#[test]
fn the_watch_sweep_skips_the_fetch_of_a_row_that_timed_out() {
    let mut app = fleet_app(3);
    app.refresh_interval = 60;
    land(&mut app, 0, fetch_failed(git::FetchFailureKind::TimedOut));
    land(
        &mut app,
        1,
        fetch_failed(git::FetchFailureKind::Unreachable),
    );
    let now = chrono::Local::now();
    let targets = sweep_targets(&app, None);
    assert_eq!(
        held_fetch_paths(&app, &targets, now),
        ["~/repos/repo-0".to_string()].into(),
    );
    assert!(
        held_fetch_paths(&app, &targets, now + chrono::Duration::seconds(121)).is_empty(),
        "still held after two intervals"
    );
    assert_eq!(
        app.repos[0].fetch.failure().map(|f| f.kind),
        Some(git::FetchFailureKind::TimedOut),
        "the row stopped showing its timeout"
    );
    // The automatic selection skips the held row and claims no fetch for
    // it; a manual one fetches it.
    let automatic = fetch_skips(&mut app, &targets, true, true);
    assert!(automatic.contains("~/repos/repo-0"), "{automatic:?}");
    assert!(!app.fetches_in_flight.contains_key("~/repos/repo-0"));
    assert!(app.fetches_in_flight.contains_key("~/repos/repo-1"));
    app.fetches_in_flight.clear();
    let manual = fetch_skips(&mut app, &targets, true, false);
    assert!(!manual.contains("~/repos/repo-0"), "{manual:?}");
    assert!(app.fetches_in_flight.contains_key("~/repos/repo-0"));
    land(&mut app, 0, git::FetchOutcome::Succeeded);
    assert!(held_fetch_paths(&app, &targets, now).is_empty());
}

/// FZ-34 (#83), R5: a fetching sweep claims each path it fetches, and
/// the next selection, manual or automatic, sweeps a path whose fetch is
/// still running without fetching it again. The landed outcome ends the
/// claim; a sweep that did not fetch claims nothing.
#[test]
fn a_running_fetch_is_skipped_by_the_next_selection_until_it_lands() {
    let mut app = fleet_app(2);
    let targets = sweep_targets(&app, None);
    assert!(fetch_skips(&mut app, &targets, false, true).is_empty());
    assert!(
        app.fetches_in_flight.is_empty(),
        "a sweep without fetch claimed"
    );
    assert!(fetch_skips(&mut app, &targets, true, true).is_empty());
    let both: std::collections::HashSet<String> =
        ["~/repos/repo-0".to_string(), "~/repos/repo-1".to_string()].into();
    assert_eq!(fetch_skips(&mut app, &targets, true, false), both, "manual");
    assert_eq!(
        fetch_skips(&mut app, &targets, true, true),
        both,
        "automatic"
    );
    // A sweep that only rebuilt repo-0 lands first; its fetch still runs.
    land(&mut app, 0, git::FetchOutcome::NotAttempted);
    assert_eq!(fetch_skips(&mut app, &targets, true, false), both);
    land(&mut app, 0, git::FetchOutcome::Succeeded);
    assert_eq!(
        fetch_skips(&mut app, &targets, true, false),
        ["~/repos/repo-1".to_string()].into()
    );
}

/// An app whose rows sit at paths that do not exist, so a sweep spawned
/// from a test touches nothing on disk.
fn missing_paths_app(n: usize, tag: &str) -> App {
    let repos = (0..n)
        .map(|i| {
            let mut r = test_repo();
            r.name = format!("repo-{i}");
            r.path = Some(
                std::env::temp_dir()
                    .join(format!("fleetz-83-{tag}-no-such-dir"))
                    .join(format!("repo-{i}"))
                    .display()
                    .to_string(),
            );
            r
        })
        .collect();
    App::new(repos, "h".to_string(), 120, 44)
}

fn timed_out_at(app: &mut App, i: usize) -> String {
    let path = app.repos[i].path.clone().unwrap();
    let status = app.repos[i].clone();
    apply_repo_updated(
        app,
        &path,
        status,
        fetch_failed(git::FetchFailureKind::TimedOut),
    );
    path
}

/// FZ-34 (#83), criterion 3 through `start_sweep`: the watch timer's
/// sweep fetches every row but the one held after a timeout.
#[tokio::test]
async fn the_watch_timer_sweep_claims_no_fetch_for_a_held_row() {
    let mut app = missing_paths_app(3, "watch");
    let held = timed_out_at(&mut app, 1);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    start_sweep(&mut app, &tx, true, None, false);
    assert!(
        !app.fetches_in_flight.contains_key(&held),
        "held row fetched"
    );
    for i in [0, 2] {
        let path = app.repos[i].path.clone().unwrap();
        assert!(
            app.fetches_in_flight.contains_key(&path),
            "row {i} not fetched"
        );
    }
}

/// FZ-34 (#83): `f` is manual, so it fetches a row held after a timeout.
#[tokio::test]
async fn f_fetches_a_row_held_after_a_timeout() {
    let mut app = missing_paths_app(1, "fkey");
    let held = timed_out_at(&mut app, 0);
    assert!(app.fetch_held(&held, chrono::Local::now()));
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    fetch_key(&mut app, &tx, None);
    assert!(
        app.fetches_in_flight.contains_key(&held),
        "f skipped the held row"
    );
}

/// FZ-35a (#84), criterion 3: at most one progress event per half
/// second per fetch, the first at once.
#[test]
fn progress_events_are_throttled_to_two_a_second() {
    let mut throttle = ProgressThrottle::default();
    let t0 = std::time::Instant::now();
    let ms = std::time::Duration::from_millis;
    assert!(throttle.admit(t0));
    assert!(!throttle.admit(t0 + ms(1)));
    assert!(!throttle.admit(t0 + ms(499)));
    assert!(throttle.admit(t0 + ms(500)));
    assert!(!throttle.admit(t0 + ms(900)));
    assert!(throttle.admit(t0 + ms(1000)));
    // Two fetches throttle apart.
    let mut other = ProgressThrottle::default();
    assert!(other.admit(t0 + ms(1001)));
}

/// FZ-35a (#84), criterion 4: a sweep's fetch does not run when, by the
/// time its permit is granted, a newer claim holds the path, and runs
/// (taking the claim back) when the path is free. The path does not
/// exist, so a fetch that ran fails rather than doing nothing.
#[tokio::test]
async fn a_fetch_runs_only_under_its_own_claim_at_the_permit() {
    let claims = app::FetchClaims::default();
    let missing = std::env::temp_dir().join("fleetz-84-no-such-dir");
    let t0 = std::time::Instant::now();
    let old = claims.reserve("~/r", t0).unwrap();
    claims.expire(
        std::time::Duration::ZERO,
        t0 + std::time::Duration::from_secs(1),
    );
    let newer = claims.reserve("~/r", t0).unwrap();
    assert_eq!(
        fetch_under_claim(&claims, "~/r", &missing, old, None).await,
        git::FetchOutcome::NotAttempted
    );
    assert_eq!(claims.token("~/r"), Some(newer));
    claims.release("~/r", Some(newer));
    assert!(matches!(
        fetch_under_claim(&claims, "~/r", &missing, old, None).await,
        git::FetchOutcome::Failed(_)
    ));
    assert_eq!(
        claims.token("~/r"),
        Some(old),
        "the fetch ran without its claim"
    );
}

/// FZ-35a (#84), criterion 4: a sweep's landing ends only the claim it
/// held, and clears the row's progress.
#[test]
fn a_landing_from_an_expired_sweep_leaves_the_newer_claim() {
    let mut app = fleet_app(1);
    let path = app.repos[0].path.clone().unwrap();
    let status = app.repos[0].clone();
    let t0 = std::time::Instant::now();
    assert!(app.claim_fetch(&path, t0));
    let old = app.fetches_in_flight.token(&path);
    app.expire_fetches_in_flight(SWEEP_STUCK_AFTER, t0 + SWEEP_STUCK_AFTER * 2);
    assert!(app.claim_fetch(&path, t0 + SWEEP_STUCK_AFTER * 2));
    let newer = app.fetches_in_flight.token(&path);
    app.apply_fetch_progress(
        &path,
        git::FetchProgress {
            phase: git::FetchPhase::Receiving,
            objects_done: 1,
            objects_total: 2,
            bytes: 1,
        },
    );
    land_repo_update(
        &mut app,
        &path,
        status.clone(),
        git::FetchOutcome::Succeeded,
        old,
    );
    assert_eq!(app.fetches_in_flight.token(&path), newer);
    assert_eq!(app.repos[0].fetch.progress, None, "landing kept progress");
    land_repo_update(&mut app, &path, status, git::FetchOutcome::Succeeded, newer);
    assert!(!app.fetches_in_flight.contains_key(&path));
}

/// FZ-35a (#84), criteria 3 and 4 through a real sweep: the fetch's
/// progress reaches the app as `FetchProgress` events, throttled, all
/// before the `RepoUpdated` that lands it with the claim's token. The
/// clone's upload-pack hook prints the progress, standing in for git's.
#[tokio::test]
async fn a_sweep_streams_its_fetch_progress_before_it_lands() {
    let scratch = wt_scratch("sweep_progress");
    let upstream = scratch.join("up");
    id_repo(&upstream, None, false);
    let clone = scratch.join("down");
    wt_sh(
        &scratch,
        &[
            "git",
            "clone",
            "-q",
            upstream.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    wt_sh(
        &upstream,
        &["git", "commit", "-q", "--allow-empty", "-m", "two"],
    );
    let hook = "i=1; while [ $i -le 5 ]; do \
         printf 'Receiving objects: %d%% (%d/5), %d.00 MiB | 1.00 MiB/s\\r' \
         $((i * 20)) $i $i >&2; sleep 0.3; i=$((i + 1)); done; git upload-pack";
    wt_sh(&clone, &["git", "config", "remote.origin.uploadpack", hook]);
    let mut row = test_repo();
    row.path = Some(clone.display().to_string());
    let mut app = App::new(vec![row], "h".to_string(), 120, 44);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let start = std::time::Instant::now();
    start_single_sweep(&mut app, &tx, 0, true, false, None);
    let path = clone.display().to_string();
    let token = app.fetches_in_flight.token(&path).expect("claimed");
    let mut progress = Vec::new();
    let (fetch, claim) = loop {
        match tokio::time::timeout(std::time::Duration::from_secs(20), rx.recv()).await {
            Ok(Some(AppEvent::FetchProgress {
                path: p,
                progress: update,
            })) => {
                assert_eq!(p, path);
                progress.push(update);
            }
            Ok(Some(AppEvent::RepoUpdated { fetch, claim, .. })) => break (fetch, claim),
            Ok(Some(_)) => {}
            other => panic!("the sweep never landed: {}", other.is_ok()),
        }
    };
    let took = start.elapsed();
    assert_eq!(fetch, git::FetchOutcome::Succeeded);
    assert_eq!(claim, Some(token));
    assert!(
        progress.iter().any(|p| p.bytes >= 1024 * 1024),
        "{progress:?}"
    );
    // Five hook updates and git's own lines, at most two a second.
    let allowed = 1 + (took.as_millis() / PROGRESS_EVENT_GAP.as_millis()) as usize;
    assert!(
        !progress.is_empty() && progress.len() <= allowed,
        "{} events in {took:?}",
        progress.len()
    );
    std::fs::remove_dir_all(&scratch).ok();
}

/// FZ-35a (#84), criterion 4: the load path claims its fetches. A clone
/// whose path a sweep is fetching is not fetched again by the load (its
/// row records no fetch, the sweep's claim stays); a free one is fetched
/// under a claim given back when it ends. The clone has no remote, so
/// git's fetch has nothing to do and nothing leaves the disk.
#[tokio::test]
async fn the_load_fetches_under_a_claim_and_skips_a_claimed_path() {
    let home = wt_scratch("load_claim");
    let root = home.join("R");
    let repo = root.join("svc");
    id_repo(&repo, None, false);
    let dataset = [id_entry(
        Some("x"),
        "svc",
        0,
        model::CiStatus::Unknown,
        0,
        model::RepoKind::Unknown,
    )];
    async fn assemble(
        dataset: &[github::GithubRepo],
        home: &Path,
        root: &Path,
        claims: &app::FetchClaims,
    ) -> Vec<RepoStatus> {
        let args = Args::parse_from(["fleetz"]);
        let config = config::Config::default();
        let (repos, _) = assemble_view_with(
            dataset,
            None,
            String::new(),
            &args,
            &config,
            home,
            &[root.to_path_buf()],
            true,
            None,
            claims,
        )
        .await
        .expect("assembly");
        repos
    }
    let key = tilde_display(&repo);

    let claims = app::FetchClaims::default();
    let sweeping = claims.reserve(&key, std::time::Instant::now()).unwrap();
    let repos = assemble(&dataset, &home, &root, &claims).await;
    let row = row_at(&repos, &repo);
    assert_eq!(row.fetch.last_attempt, None, "fetched a claimed path");
    assert_eq!(claims.token(&key), Some(sweeping));

    let claims = app::FetchClaims::default();
    let repos = assemble(&dataset, &home, &root, &claims).await;
    let row = row_at(&repos, &repo);
    assert!(
        row.fetch.last_attempt.is_some(),
        "the free path was not fetched"
    );
    assert_eq!(row.fetch.outcome, git::FetchOutcome::Succeeded);
    assert!(claims.is_empty(), "the load kept its claim");
    std::fs::remove_dir_all(&home).ok();
}

/// FZ-35b (#85), criterion 2 at launch and `R`: the load does not fetch a
/// path the fetch cap holds, and a fetch it runs records the row's size
/// baseline. The clone has no remote, so nothing leaves the disk.
#[tokio::test]
async fn the_load_skips_a_size_held_path_and_records_the_baseline() {
    let home = wt_scratch("load_size_hold");
    let root = home.join("R");
    let repo = root.join("svc");
    id_repo(&repo, None, false);
    let dataset = [github::GithubRepo {
        disk_usage: Some(777),
        ..id_entry(
            Some("x"),
            "svc",
            0,
            model::CiStatus::Unknown,
            0,
            model::RepoKind::Unknown,
        )
    }];
    let assemble = |held: HashSet<String>| {
        let (dataset, home, root) = (dataset.clone(), home.clone(), root.clone());
        async move {
            let args = Args::parse_from(["fleetz"]);
            let config = config::Config::default();
            assemble_view_held(
                &dataset,
                None,
                String::new(),
                &args,
                &config,
                &home,
                &[root],
                true,
                None,
                &app::FetchClaims::default(),
                &held,
            )
            .await
            .expect("assembly")
            .0
        }
    };
    let key = tilde_display(&repo);
    let repos = assemble([key].into()).await;
    let row = row_at(&repos, &repo);
    assert_eq!(row.fetch.last_attempt, None, "fetched a held path");

    let repos = assemble(HashSet::new()).await;
    let row = row_at(&repos, &repo);
    assert_eq!(row.fetch.outcome, git::FetchOutcome::Succeeded);
    assert_eq!(row.fetch.disk_baseline, Some(777));
    std::fs::remove_dir_all(&home).ok();
}

const MIB: u64 = 1024 * 1024;

/// A row grown past the cap (the pre-check) and a row whose fetch was
/// cut at the cap, as the poll and a landing leave them.
fn hold_sizes(app: &mut App, grown: usize, cut: usize) {
    app.repos[grown].fetch.disk_baseline = Some(1_000);
    app.repos[grown]
        .fetch
        .assess_growth(Some(1_000 + 300 * 1024), Some(200 * MIB));
    let path = app.repos[cut].path.clone().unwrap();
    let status = app.repos[cut].clone();
    apply_repo_updated(
        app,
        &path,
        status,
        git::FetchOutcome::Failed(git::FetchFailure::too_big(201 * MIB)),
    );
}

/// FZ-35b (#85), criterion 4: `f` fetches a held row, uncapped; `F` and
/// the automatic selections skip it and stay capped.
#[test]
fn f_fetches_a_held_row_uncapped_and_shift_f_and_automatic_do_not() {
    let mut app = fleet_app(3);
    hold_sizes(&mut app, 0, 1);
    let targets = sweep_targets(&app, None);
    let held: HashSet<String> = ["~/repos/repo-0".to_string(), "~/repos/repo-1".to_string()].into();
    for ask in [FetchAsk::Automatic, FetchAsk::All] {
        app.fetches_in_flight.clear();
        assert_eq!(
            fetch_skips_for(&mut app, &targets, true, ask),
            held,
            "{ask:?}"
        );
        assert!(app.fetches_in_flight.contains_key("~/repos/repo-2"));
        assert_eq!(ask.cap(Some(200 * MIB)), Some(200 * MIB), "{ask:?}");
    }
    app.fetches_in_flight.clear();
    assert!(fetch_skips_for(&mut app, &targets, true, FetchAsk::One).is_empty());
    assert_eq!(FetchAsk::One.cap(Some(200 * MIB)), None);
    // `fetch_max_mb = 0`: nothing is capped.
    assert_eq!(FetchAsk::All.cap(None), None);
}

/// FZ-35b (#85), review F1: the cap each sweep entry point hands
/// `spawn_sweep` (it takes the plan, so the plan's cap is the one the
/// fetches run under): `f` uncapped, `F`, the watch timer and the poll's
/// single sweeps at the configured cap, and nothing capped at `0`.
#[test]
fn each_sweep_entry_point_plans_its_fetch_cap() {
    let mut app = fleet_app(1);
    app.fetch_max_bytes = Some(7 * MIB);
    let targets = sweep_targets(&app, None);
    let mut cap_for = |ask: FetchAsk| {
        app.fetches_in_flight.clear();
        plan_sweep_fetch(&mut app, &targets, true, ask).max_bytes
    };
    assert_eq!(cap_for(FetchAsk::for_single_sweep(false)), None, "f");
    assert_eq!(
        cap_for(FetchAsk::for_single_sweep(true)),
        Some(7 * MIB),
        "moved remote"
    );
    assert_eq!(cap_for(FetchAsk::for_full_sweep(true)), Some(7 * MIB), "F");
    assert_eq!(
        cap_for(FetchAsk::for_full_sweep(false)),
        Some(7 * MIB),
        "watch"
    );
    app.fetch_max_bytes = None;
    app.fetches_in_flight.clear();
    let plan = plan_sweep_fetch(&mut app, &targets, true, FetchAsk::for_full_sweep(true));
    assert_eq!(plan.max_bytes, None, "fetch_max_mb = 0");
}

/// The same through the keys' own entry points: `F` claims no fetch for
/// a held row, `f` on it does.
#[tokio::test]
async fn shift_f_skips_a_held_row_and_f_on_it_fetches() {
    let mut app = missing_paths_app(3, "sizehold");
    hold_sizes(&mut app, 0, 1);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    start_sweep(&mut app, &tx, true, None, true);
    for i in [0, 1] {
        let path = app.repos[i].path.clone().unwrap();
        assert!(
            !app.fetches_in_flight.contains_key(&path),
            "F fetched held row {i}"
        );
    }
    let free = app.repos[2].path.clone().unwrap();
    assert!(app.fetches_in_flight.contains_key(&free));
    for i in [0, 1] {
        start_single_sweep(&mut app, &tx, i, true, false, None);
        let path = app.repos[i].path.clone().unwrap();
        assert!(
            app.fetches_in_flight.contains_key(&path),
            "f skipped held row {i}"
        );
    }
}

/// FZ-35b (#85), review B1: `F` reports over the fetches it ran. A row
/// the fetch cap holds is only rebuilt and lands `NotAttempted`, so it
/// must not be awaited, or the batch never completes and a failure among
/// the others never reaches the status line (rule 9).
#[tokio::test]
async fn shift_f_reports_its_failures_with_a_size_held_row() {
    let mut app = missing_paths_app(3, "ftally");
    app.repos[0].fetch.disk_baseline = Some(1_000);
    app.repos[0]
        .fetch
        .assess_growth(Some(1_000 + 300 * 1024), Some(200 * MIB));
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    start_sweep(&mut app, &tx, true, None, true);
    for (i, fetch) in [
        git::FetchOutcome::NotAttempted,
        fetch_failed(git::FetchFailureKind::Unreachable),
        git::FetchOutcome::Succeeded,
    ]
    .into_iter()
    .enumerate()
    {
        let path = app.repos[i].path.clone().unwrap();
        let status = app.repos[i].clone();
        apply_repo_updated(&mut app, &path, status, fetch);
    }
    let status = app.status_message.as_ref().map(|m| m.text.clone());
    assert_eq!(status.as_deref(), Some("1 of 2 fetches failed: repo-1"));
}

/// FZ-35b (#85), criterion 2 for moved remotes: a poll that sees a remote
/// grow past the cap since the last fetch holds the row and does not
/// fetch it; a moved remote that did not grow is fetched as before.
#[tokio::test]
async fn a_moved_remote_grown_past_the_cap_is_held_not_fetched() {
    let mut app = missing_paths_app(2, "moved");
    let before = "2026-10-20T10:00:00Z";
    let after = "2026-10-20T11:00:00Z";
    let sized = |name: &str, pushed: &str, kb: u64| github::GithubRepo {
        disk_usage: Some(kb),
        ..gh(name, pushed)
    };
    app.dataset = vec![
        sized("repo-0", before, 1_000),
        sized("repo-1", before, 1_000),
    ];
    for repo in &mut app.repos {
        repo.fetch.disk_baseline = Some(1_000);
    }
    app.refresh_awaits_poll = true;
    poll_with(
        &mut app,
        Ok(Box::new(github::GithubFetch {
            repos: vec![
                sized("repo-0", after, 1_000 + 300 * 1024),
                sized("repo-1", after, 1_000 + 10 * 1024),
            ],
            calendar: None,
            has_pr_counts: true,
            origin: github::Origin::Network,
            meta: Default::default(),
        })),
    );
    let (grown, quiet) = (
        app.repos[0].path.clone().unwrap(),
        app.repos[1].path.clone().unwrap(),
    );
    assert_eq!(
        app.repos[0].fetch.size_hold(),
        Some(model::SizeHold::Grown(300 * MIB))
    );
    assert!(
        !app.refresh_fetches.contains(&grown),
        "fetched the grown remote"
    );
    assert!(!app.fetches_in_flight.contains_key(&grown));
    assert_eq!(app.repos[1].fetch.size_hold(), None);
    assert!(
        app.refresh_fetches.contains(&quiet),
        "the small push was not fetched"
    );
}

/// FZ-35b (#85), criterion 4: a successful fetch records GitHub's current
/// size as the baseline and lifts the hold.
#[test]
fn a_successful_fetch_records_the_baseline_and_lifts_the_hold() {
    let mut app = fleet_app(1);
    app.dataset = vec![github::GithubRepo {
        disk_usage: Some(308_200),
        ..gh("repo-0", "2026-10-20T11:00:00Z")
    }];
    hold_sizes(&mut app, 0, 0);
    assert!(app.repos[0].fetch.size_hold().is_some());
    land(&mut app, 0, git::FetchOutcome::Succeeded);
    assert_eq!(app.repos[0].fetch.size_hold(), None);
    assert_eq!(app.repos[0].fetch.disk_baseline, Some(308_200));
    assess_size_holds(&mut app.repos, &app.dataset, app.fetch_max_bytes);
    assert_eq!(
        app.repos[0].fetch.size_hold(),
        None,
        "held again at its own size"
    );
}

/// FZ-34 (#83): a timeout from a fetch a load ran (launch, `R`) holds the
/// row like a sweep's.
#[test]
fn a_load_that_timed_out_holds_the_row() {
    let mut app = fleet_app(1);
    let path = app.repos[0].path.clone().unwrap();
    let now = chrono::Local::now();
    let mut repo = app.repos[0].clone();
    repo.fetch
        .record(&fetch_failed(git::FetchFailureKind::TimedOut), now);
    apply_load_result(
        &mut app,
        &config::Config::default(),
        Ok(Box::new(LoadResult {
            kind: LoadKind::Reload,
            repos: vec![repo],
            header: "h".to_string(),
            dataset: Some(Vec::new()),
            github_calendar: None,
            source: app::DatasetSource::Local,
            ambiguity: None,
            meta: None,
        })),
    );
    assert!(
        app.fetch_held(&path, now),
        "the load's timeout held nothing"
    );
}

/// FZ-34 (#83): the poll's batch (moved remotes and the recovery
/// refetch) leaves out a row held after a timeout.
#[tokio::test]
async fn a_recovering_poll_skips_a_row_held_after_a_timeout() {
    let path = std::env::temp_dir()
        .join("fleetz-83-no-such-dir")
        .join("repo")
        .display()
        .to_string();
    let mut repo = test_repo();
    repo.path = Some(path.clone());
    let mut app = App::new(vec![repo], "h".to_string(), 120, 44);
    let status = app.repos[0].clone();
    apply_repo_updated(
        &mut app,
        &path,
        status,
        fetch_failed(git::FetchFailureKind::TimedOut),
    );
    poll_with(
        &mut app,
        Err(github::FetchError {
            failure: github::Failure::new(github::FailureKind::Offline, "no route"),
            fallback: None,
            meta: Default::default(),
        }),
    );
    app.refresh_awaits_poll = true;
    poll_with(
        &mut app,
        Ok(Box::new(github::GithubFetch {
            repos: Vec::new(),
            calendar: None,
            has_pr_counts: true,
            origin: github::Origin::Network,
            meta: Default::default(),
        })),
    );
    assert!(
        app.refresh_fetches.is_empty(),
        "the recovery refetch fetched a held row"
    );
}

/// FZ-34 (#83): a manual `r` or `R` lifts every timeout hold.
#[test]
fn r_and_shift_r_lift_the_timeout_holds() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    let budget = reading("A", 2, at(17, 23, 6), at(17, 0, 0));
    land_at(&mut app, ok_with(Some(budget)), at(17, 0, 0));
    let timed_out = fetch_failed(git::FetchFailureKind::TimedOut);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let args = Args::parse_from(["fleetz"]);

    app.fetch_backoff.record("~/a", &timed_out, at(17, 9, 0));
    assert!(app.fetch_held("~/a", at(17, 10, 0)));
    // Under a known limit `r` sends nothing, so no runtime is needed.
    refresh_key(&mut app, &tx, &args, &config, at(17, 10, 0));
    assert!(!app.fetch_held("~/a", at(17, 10, 0)), "r kept the hold");

    app.fetch_backoff.record("~/a", &timed_out, at(17, 9, 30));
    assert!(app.fetch_held("~/a", at(17, 10, 0)));
    // A load already running: `R` starts nothing new.
    app.loading = true;
    reload_key(
        &mut app,
        &tx,
        &args,
        &config,
        Path::new("/nonexistent"),
        at(17, 10, 0),
    );
    assert!(!app.fetch_held("~/a", at(17, 10, 0)), "R kept the hold");
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

/// A clone whose remote GitHub last touched before our last `git fetch`
/// is already in hand, and startup should not spend a network round trip
/// re-confirming it. This is the whole startup cost: fetching every repo
/// past the cutoff was around 0.8s of network each, so a wide time window
/// put seconds of nothing in front of the fleet.
fn fetch_fixture(tag: &str, names: &[&str], write_fetch_head: bool) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fleetz_stale_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for name in names {
        let git = dir.join(name).join(".git");
        std::fs::create_dir_all(&git).unwrap();
        if write_fetch_head {
            std::fs::write(git.join("FETCH_HEAD"), b"x").unwrap();
        }
    }
    dir
}

/// The regression: a clone matched through its remote lives in a folder
/// named something else, so deduping pass 3 by name alone listed it a
/// second time, as local-only, once it was dirty.
#[test]
fn a_clone_passes_1_and_2_claimed_is_not_listed_again_as_local_only() {
    let claimed = PathBuf::from("/tmp/x/DevOps_Portfolio");
    let candidates = unclaimed_local(
        vec![claimed.clone(), PathBuf::from("/tmp/x/scratch")],
        &HashSet::from([claimed.as_path()]),
        &HashSet::from(["portfolio-site".to_string()]),
    );
    let names: Vec<&str> = candidates.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(names, ["scratch"]);
}

fn found_targets(dir: &Path, names: &[&str]) -> Vec<(discovery::FoundRepo, RepoLocation)> {
    names
        .iter()
        .map(|n| {
            (
                discovery::FoundRepo {
                    name: (*n).to_string(),
                    path: dir.join(n),
                    owner: None,
                },
                RepoLocation::Primary,
            )
        })
        .collect()
}

#[test]
fn a_remote_pushed_before_our_last_fetch_is_not_fetched_again() {
    // FETCH_HEAD is written now, so a push dated 2001 is comfortably
    // older than it whatever the machine's clock says.
    let dir = fetch_fixture("skip", &["alpha"], true);
    let targets = found_targets(&dir, &["alpha"]);
    let active = vec![gh("alpha", "2001-01-01T00:00:00Z")];
    assert!(
        stale_remotes(&targets, &active).is_empty(),
        "fetched a remote we already had"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_remote_pushed_since_our_last_fetch_is_fetched() {
    let dir = fetch_fixture("stale", &["alpha"], true);
    let targets = found_targets(&dir, &["alpha"]);
    let active = vec![gh("alpha", "2099-01-01T00:00:00Z")];
    assert_eq!(stale_remotes(&targets, &active).len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Both unknowns have to read as stale. A clone that has never fetched
/// writes no `FETCH_HEAD`, and a repo with no GitHub match has no
/// `pushed_at` to compare against -- skipping either would silently show
/// the fleet as more up to date than it is.
#[test]
fn an_unknown_baseline_is_treated_as_stale() {
    let never = fetch_fixture("never", &["alpha"], false);
    let active = vec![gh("alpha", "2001-01-01T00:00:00Z")];
    assert_eq!(
        stale_remotes(&found_targets(&never, &["alpha"]), &active).len(),
        1,
        "a clone that never fetched was skipped"
    );
    let _ = std::fs::remove_dir_all(&never);

    let unmatched = fetch_fixture("unmatched", &["local-only"], true);
    assert_eq!(
        stale_remotes(&found_targets(&unmatched, &["local-only"]), &active).len(),
        1,
        "a repo with no GitHub match was skipped"
    );
    let _ = std::fs::remove_dir_all(&unmatched);
}

/// Rule 7: a failed `git fetch` leaves `FETCH_HEAD` empty with a fresh
/// mtime (pinned by `git::a_failed_fetch_empties_fetch_head`). Reading
/// that mtime as "fetched just now" skipped exactly the repo whose remote
/// refs were known to be behind.
#[test]
fn an_empty_fetch_head_is_treated_as_never_fetched() {
    let dir = fetch_fixture("emptyhead", &["alpha"], false);
    std::fs::write(dir.join("alpha").join(".git").join("FETCH_HEAD"), b"").unwrap();
    let active = vec![gh("alpha", "2001-01-01T00:00:00Z")];
    assert_eq!(
        stale_remotes(&found_targets(&dir, &["alpha"]), &active).len(),
        1,
        "a clone whose last fetch failed was skipped as already in hand"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The case that motivated this: a fleet opened again an hour later
/// fetches nothing at all, so startup no longer scales with the time
/// window -- which only ever decided how many repos reached the loop.
#[test]
fn a_warm_fleet_costs_no_network_at_startup() {
    let names = ["alpha", "beta", "gamma", "delta"];
    let dir = fetch_fixture("warm", &names, true);
    let active: Vec<_> = names
        .iter()
        .map(|n| gh(n, "2001-01-01T00:00:00Z"))
        .collect();
    assert!(stale_remotes(&found_targets(&dir, &names), &active).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
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
        disk_usage: None,
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
    let mut app = App::new(Vec::new(), String::new(), 100, 40);
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

fn ago(days: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now() - chrono::Duration::days(days)
}

/// The case that prompted this: `~/Repos/Rust`, `bonsai` and `AoC-2025`
/// are dirty with untracked files from months back and sat in "local
/// changes" permanently, because the section consulted no clock at all.
#[test]
fn a_local_repo_stale_past_the_window_drops_out() {
    assert!(!local_repo_is_current(0, Some(ago(118)), Some(ago(7))));
}

/// The failure `docs/ideas.md` warned about when it argued the section
/// should ignore the window: narrowing must never hide a repo you are
/// working in right now.
#[test]
fn a_repo_dirtied_today_survives_the_narrowest_window() {
    assert!(local_repo_is_current(0, Some(ago(0)), Some(ago(1))));
}

/// An unpushed commit exists on this machine and nowhere else, so it
/// outranks the window however old it is.
#[test]
fn unpushed_commits_survive_any_window() {
    assert!(local_repo_is_current(2, Some(ago(158)), Some(ago(7))));
}

/// Widening to "all" clears the cutoff, which is the escape hatch for
/// everything the window evicted.
#[test]
fn no_cutoff_keeps_every_local_repo() {
    assert!(local_repo_is_current(0, Some(ago(158)), None));
}

/// A repo git could date at all is the normal case; one it could not is
/// a repo nothing has ever happened in, and it has no claim on a row.
#[test]
fn a_local_repo_with_no_timestamp_drops_under_a_window() {
    assert!(!local_repo_is_current(0, None, Some(ago(7))));
}

/// The `~/Repos/Rust` shape end to end: `git init`, untracked files, no
/// commit ever, last touched months ago. The predicate is only half the
/// fix -- this pins that pass 3 actually consults it, which is the wiring
/// a unit test of the predicate alone would happily let rot.
#[tokio::test]
async fn pass_three_drops_a_repo_stale_past_the_window() {
    let dir = std::env::temp_dir().join(format!("fleetz_p3_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for args in [
        vec!["git", "init", "-q", "-b", "main"],
        vec!["git", "config", "user.email", "t@example.com"],
        vec!["git", "config", "user.name", "t"],
    ] {
        let ok = std::process::Command::new(args[0])
            .args(&args[1..])
            .current_dir(&dir)
            .status()
            .expect("spawn")
            .success();
        assert!(ok, "setup failed: {args:?}");
    }
    std::fs::write(dir.join("junk.txt"), "scratch\n").unwrap();
    // Backdate it the way an abandoned project is backdated: by not
    // having been touched.
    let ok = std::process::Command::new("touch")
        .args(["-t", "202604300000", "junk.txt"])
        .current_dir(&dir)
        .status()
        .expect("spawn")
        .success();
    assert!(ok, "touch failed");

    let stale = local_only_status(
        &dir,
        "junkrepo".to_string(),
        model::RepoKind::Unknown,
        Some(chrono::Utc::now() - chrono::Duration::days(7)),
    )
    .await;
    assert!(stale.is_none(), "months-old untracked work is not current");

    let unfiltered =
        local_only_status(&dir, "junkrepo".to_string(), model::RepoKind::Unknown, None).await;
    assert!(
        unfiltered.is_some(),
        "the same repo must still be there with no window"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// S8a of the repository identity design, through the assembly seam: a
/// repo with no remote and a modified tracked file, under the scan root,
/// with nothing in the dataset naming it, is one LocalOnly row named for
/// its folder. Runs against a temp home and root, so the real `$HOME` is
/// never walked; no dataset and `git_fetch: false`, so nothing reaches
/// the network or the cache.
#[tokio::test]
async fn a_remoteless_dirty_repo_under_the_root_is_one_local_only_row() {
    let home = wt_scratch("seam_s8a");
    let root = home.join("R");
    let repo = root.join("scratch");
    std::fs::create_dir_all(&repo).unwrap();
    wt_sh(&repo, &["git", "init", "-q", "-b", "main"]);
    wt_sh(&repo, &["git", "config", "user.email", "t@example.com"]);
    wt_sh(&repo, &["git", "config", "user.name", "t"]);
    std::fs::write(repo.join("a.txt"), "one\n").unwrap();
    wt_sh(&repo, &["git", "add", "."]);
    wt_sh(&repo, &["git", "commit", "-q", "-m", "init"]);
    std::fs::write(repo.join("a.txt"), "two\n").unwrap();

    let args = Args::parse_from(["fleetz"]);
    let config = config::Config::default();
    let (repos, _) = assemble_view_in(
        &[],
        None,
        String::new(),
        &args,
        &config,
        &home,
        std::slice::from_ref(&root),
        false,
        None,
    )
    .await
    .expect("assembly");

    assert_eq!(repos.len(), 1, "exactly one row: {repos:?}");
    assert_eq!(repos[0].name, "scratch");
    assert_eq!(repos[0].location, RepoLocation::LocalOnly);
    assert_eq!(repos[0].dirty, 1);

    std::fs::remove_dir_all(&home).ok();
}

// --- FZ-05d: GitHub fields live on one carrier row per remote ---
//
// Scenarios of section 4 of `docs/repository-identity-design.md`. The
// load half runs through `assemble_view_in` with a temp home and root, so
// the real `$HOME` is never walked; `git_fetch: false` and a dataset
// handed in directly keep the network, the cache and `gh` out of it.

/// A repo at `dir` with one commit, `origin` set to `url` when given, and
/// a modified tracked file when `dirty`.
fn id_repo(dir: &Path, url: Option<&str>, dirty: bool) {
    std::fs::create_dir_all(dir).unwrap();
    wt_sh(dir, &["git", "init", "-q", "-b", "main"]);
    wt_sh(dir, &["git", "config", "user.email", "t@example.com"]);
    wt_sh(dir, &["git", "config", "user.name", "t"]);
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    wt_sh(dir, &["git", "add", "."]);
    wt_sh(dir, &["git", "commit", "-q", "-m", "init"]);
    if let Some(url) = url {
        wt_sh(dir, &["git", "remote", "add", "origin", url]);
    }
    if dirty {
        std::fs::write(dir.join("a.txt"), "two\n").unwrap();
    }
}

/// A dataset entry pushed `days_ago`, with `prs` open pull requests.
fn id_entry(
    owner: Option<&str>,
    name: &str,
    days_ago: i64,
    ci: model::CiStatus,
    prs: u32,
    kind: model::RepoKind,
) -> github::GithubRepo {
    let pr = |number: u32| model::PullRequest {
        number,
        title: "Change".to_string(),
        author: "someone".to_string(),
        created_at: chrono::Utc::now(),
        review_status: model::ReviewStatus::Approved,
        url: format!("https://github.com/x/{name}/pull/{number}"),
    };
    github::GithubRepo {
        name: name.to_string(),
        pushed_at: chrono::Utc::now() - chrono::Duration::days(days_ago),
        pr_count: prs,
        prs: (1..=prs).map(pr).collect(),
        ci_status: ci,
        ci_workflow: None,
        ci_duration: None,
        repo_kind: kind,
        owner: owner.map(str::to_string),
        disk_usage: None,
    }
}

/// The 1-day window the scenarios name, as a cutoff.
fn one_day() -> Option<chrono::DateTime<chrono::Utc>> {
    Some(chrono::Utc::now() - chrono::Duration::days(1))
}

async fn id_assemble(
    home: &Path,
    roots: &[PathBuf],
    dataset: &[github::GithubRepo],
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
) -> (Vec<RepoStatus>, ViewNotes) {
    let args = Args::parse_from(["fleetz"]);
    let config = config::Config::default();
    assemble_view_in(
        dataset,
        cutoff,
        String::new(),
        &args,
        &config,
        home,
        roots,
        false,
        None,
    )
    .await
    .expect("assembly")
}

fn row_at<'a>(repos: &'a [RepoStatus], path: &Path) -> &'a RepoStatus {
    let want = tilde_display(path);
    repos
        .iter()
        .find(|r| r.path.as_deref() == Some(want.as_str()))
        .unwrap_or_else(|| panic!("no row at {want}: {repos:#?}"))
}

fn not_cloned<'a>(repos: &'a [RepoStatus], owner: &str, name: &str) -> Option<&'a RepoStatus> {
    repos.iter().find(|r| {
        r.location == RepoLocation::NotCloned && r.name == name && r.owner.as_deref() == Some(owner)
    })
}

/// A row with no GitHub fields: what every non-carrier LocalOnly row is.
fn assert_carries_nothing(repo: &RepoStatus) {
    assert_eq!(repo.owner, None, "{} was given an owner", repo.name);
    assert_eq!(repo.ci_status, model::CiStatus::Unknown, "{}", repo.name);
    assert_eq!(repo.pr_count, 0, "{}", repo.name);
    assert!(repo.prs.is_empty(), "{}", repo.name);
    assert_eq!(repo.repo_kind, model::RepoKind::Unknown, "{}", repo.name);
}

/// Land a successful poll carrying `repos` on `app`.
fn poll_entries(app: &mut App, repos: Vec<github::GithubRepo>) {
    poll_with(
        app,
        Ok(Box::new(github::GithubFetch {
            repos,
            calendar: None,
            has_pr_counts: true,
            origin: github::Origin::Network,
            meta: Default::default(),
        })),
    );
}

/// S1, load and poll. Two clones of two same-named repos of different
/// owners are two Primary rows, each with its own owner and CI, and a
/// poll keeps each on its own repo. The name dedupe kept one row, and
/// both rows would have taken the first `api` in the dataset. A push to
/// one of them fetches that clone only.
#[tokio::test]
async fn two_same_named_repos_of_two_owners_each_keep_their_own_ci() {
    let home = wt_scratch("id_s1");
    let root = home.join("R");
    id_repo(
        &root.join("api"),
        Some("git@github.com:company/api.git"),
        false,
    );
    id_repo(
        &root.join("alice-api"),
        Some("https://github.com/alice/api"),
        false,
    );
    let alice = id_entry(
        Some("alice"),
        "api",
        0,
        model::CiStatus::Passed,
        0,
        model::RepoKind::Public,
    );
    let company = id_entry(
        Some("company"),
        "api",
        0,
        model::CiStatus::Failed,
        0,
        model::RepoKind::Org,
    );
    let dataset = vec![alice.clone(), company.clone()];

    let (repos, _) = id_assemble(&home, std::slice::from_ref(&root), &dataset, None).await;

    assert_eq!(repos.len(), 2, "{repos:#?}");
    let alices = row_at(&repos, &root.join("alice-api"));
    assert_eq!(alices.location, RepoLocation::Primary);
    assert_eq!(alices.owner.as_deref(), Some("alice"));
    assert_eq!(alices.ci_status, model::CiStatus::Passed);
    let companys = row_at(&repos, &root.join("api"));
    assert_eq!(companys.location, RepoLocation::Primary);
    assert_eq!(companys.owner.as_deref(), Some("company"));
    assert_eq!(companys.ci_status, model::CiStatus::Failed);

    let mut app = App::new(repos, "h".to_string(), 120, 44);
    let mut alice_now = alice.clone();
    alice_now.ci_status = model::CiStatus::Failed;
    let mut company_now = company.clone();
    company_now.ci_status = model::CiStatus::Passed;
    company_now.pushed_at = company.pushed_at + chrono::Duration::hours(1);
    poll_entries(&mut app, vec![alice_now.clone(), company_now.clone()]);

    let alices = row_at(&app.repos, &root.join("alice-api"));
    assert_eq!(alices.ci_status, model::CiStatus::Failed);
    let companys = row_at(&app.repos, &root.join("api"));
    assert_eq!(companys.ci_status, model::CiStatus::Passed);

    let company_index = app
        .repos
        .iter()
        .position(|r| r.owner.as_deref() == Some("company"))
        .unwrap();
    assert_eq!(
        remotes_that_moved(&dataset, &[alice_now, company_now], &app.repos),
        vec![company_index],
        "the push to company/api fetched another clone"
    );

    std::fs::remove_dir_all(&home).ok();
}

/// S16, load and poll. Two same-named repos of different owners, neither
/// cloned, are two NotCloned rows, and a poll gives each its own CI. By
/// name, both took the first entry's.
#[tokio::test]
async fn two_not_cloned_repos_of_one_name_each_get_their_own_ci() {
    let home = wt_scratch("id_s16");
    let root = home.join("R");
    std::fs::create_dir_all(&root).unwrap();
    let alice = id_entry(
        Some("alice"),
        "api",
        0,
        model::CiStatus::Passed,
        1,
        model::RepoKind::Public,
    );
    let company = id_entry(
        Some("company"),
        "api",
        0,
        model::CiStatus::Failed,
        0,
        model::RepoKind::Org,
    );

    let (repos, _) = id_assemble(
        &home,
        std::slice::from_ref(&root),
        &[alice.clone(), company.clone()],
        None,
    )
    .await;

    assert_eq!(repos.len(), 2, "{repos:#?}");
    let row = not_cloned(&repos, "alice", "api").expect("alice/api not cloned");
    assert_eq!(row.ci_status, model::CiStatus::Passed);
    assert_eq!(row.pr_count, 1);
    let row = not_cloned(&repos, "company", "api").expect("company/api not cloned");
    assert_eq!(row.ci_status, model::CiStatus::Failed);
    assert_eq!(row.pr_count, 0);

    let mut app = App::new(repos, "h".to_string(), 120, 44);
    let mut alice_now = alice;
    alice_now.ci_status = model::CiStatus::Failed;
    alice_now.pr_count = 0;
    alice_now.prs.clear();
    let mut company_now = company;
    company_now.ci_status = model::CiStatus::Passed;
    poll_entries(&mut app, vec![alice_now, company_now]);

    let row = not_cloned(&app.repos, "alice", "api").unwrap();
    assert_eq!(row.ci_status, model::CiStatus::Failed);
    assert_eq!(row.pr_count, 0);
    let row = not_cloned(&app.repos, "company", "api").unwrap();
    assert_eq!(row.ci_status, model::CiStatus::Passed);

    std::fs::remove_dir_all(&home).ok();
}

/// The FZ-05c seam carried forward: pass 2 sweeps the `home` it is given.
/// A clone outside the scan roots but under that home is an Elsewhere
/// row, the carrier of its repo at load and in the poll.
#[tokio::test]
async fn pass_two_finds_a_carrier_under_the_given_home() {
    let home = wt_scratch("id_elsewhere");
    let root = home.join("R");
    std::fs::create_dir_all(&root).unwrap();
    let clone = home.join("src").join("tool");
    id_repo(&clone, Some("git@github.com:Gaurgle/tool.git"), false);
    let entry = id_entry(
        Some("Gaurgle"),
        "tool",
        0,
        model::CiStatus::Passed,
        1,
        model::RepoKind::Private,
    );

    let (repos, _) = id_assemble(
        &home,
        std::slice::from_ref(&root),
        std::slice::from_ref(&entry),
        None,
    )
    .await;

    assert_eq!(repos.len(), 1, "{repos:#?}");
    let row = row_at(&repos, &clone);
    assert_eq!(row.location, RepoLocation::Elsewhere);
    assert_eq!(row.owner.as_deref(), Some("Gaurgle"));
    assert_eq!(row.ci_status, model::CiStatus::Passed);
    assert_eq!(row.pr_count, 1);

    let mut app = App::new(repos, "h".to_string(), 120, 44);
    let mut now = entry;
    now.ci_status = model::CiStatus::Failed;
    poll_entries(&mut app, vec![now]);
    assert_eq!(app.repos[0].ci_status, model::CiStatus::Failed);

    std::fs::remove_dir_all(&home).ok();
}

/// S3: a remote-less clone whose folder name names two owners' repos
/// claims neither. Both stay not cloned, the clone is a LocalOnly row
/// with no GitHub fields and no glyph, and the load says why. Before,
/// its pass 3 row took a glyph by name and hid both NotCloned rows.
#[tokio::test]
async fn an_ambiguous_remoteless_clone_carries_nothing_and_says_why() {
    let home = wt_scratch("id_s3");
    let root = home.join("R");
    let clone = root.join("api");
    id_repo(&clone, None, true);
    let dataset = vec![
        id_entry(
            Some("alice"),
            "api",
            0,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Public,
        ),
        id_entry(
            Some("company"),
            "api",
            0,
            model::CiStatus::Failed,
            0,
            model::RepoKind::Org,
        ),
    ];

    let (repos, notes) = id_assemble(&home, std::slice::from_ref(&root), &dataset, None).await;

    assert_eq!(repos.len(), 3, "{repos:#?}");
    assert!(not_cloned(&repos, "alice", "api").is_some());
    assert!(not_cloned(&repos, "company", "api").is_some());
    let local = row_at(&repos, &clone);
    assert_eq!(local.location, RepoLocation::LocalOnly);
    assert_carries_nothing(local);
    assert_eq!(
        notes.ambiguity.as_deref(),
        Some(
            format!(
                "api: {} matches 2 GitHub repos, add an origin remote",
                tilde_display(&clone)
            )
            .as_str()
        )
    );

    std::fs::remove_dir_all(&home).ok();
}

/// A load landing with `ambiguity` as its kind's result would.
fn land_load(
    app: &mut App,
    kind: LoadKind,
    ambiguity: Option<&str>,
    store: &window_state::WindowState,
) {
    apply_load_result_saving_to(
        app,
        &config::Config::default(),
        Ok(Box::new(LoadResult {
            kind,
            repos: Vec::new(),
            header: "last 2 days".to_string(),
            dataset: Some(Vec::new()),
            github_calendar: None,
            source: app::DatasetSource::Network,
            ambiguity: ambiguity.map(str::to_string),
            meta: None,
        })),
        store,
    );
}

/// S3's status line (N2). Set after the `Reloaded` and `Window: ...`
/// messages, so every kind of load leaves it on screen; with no
/// ambiguity each kind says what it said before.
#[test]
fn the_ambiguity_line_wins_over_every_loads_own_message() {
    let scratch = wt_scratch("id_status");
    let store = window_state::WindowState::at(scratch.join("window.toml"));
    let line = "api: ~/repos/api matches 2 GitHub repos, add an origin remote";
    let window = || time::TimeWindow::clamped(2, time::WindowUnit::Days);
    let status = |app: &App| app.status_message.as_ref().map(|m| m.text.clone());

    let kinds = [
        (LoadKind::Initial, None),
        (LoadKind::Reload, Some("Reloaded".to_string())),
        (
            LoadKind::WindowApply(window()),
            Some("Window: last 2 days".to_string()),
        ),
    ];
    for (kind, today) in kinds {
        let label = format!("{today:?}");
        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        let again = match &kind {
            LoadKind::Initial => LoadKind::Initial,
            LoadKind::Reload => LoadKind::Reload,
            LoadKind::WindowApply(_) => LoadKind::WindowApply(window()),
            LoadKind::FromMemory => LoadKind::FromMemory,
        };
        land_load(&mut app, kind, Some(line), &store);
        assert_eq!(status(&app).as_deref(), Some(line), "after {label}");

        let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
        land_load(&mut app, again, None, &store);
        assert_eq!(status(&app), today, "{label} without an ambiguity");
    }

    std::fs::remove_dir_all(&scratch).ok();
}

/// S5 load and poll twin (section 5, FZ-05d). Two clones of one remote:
/// the Primary is the carrier, and the dirty copy is a LocalOnly row
/// with local fields only, at load and after a poll. A push to the remote
/// fetches the carrier only.
#[tokio::test]
async fn an_extra_checkout_carries_nothing_at_load_or_in_the_poll() {
    let home = wt_scratch("id_s5");
    let root = home.join("R");
    let url = "git@github.com:Gaurgle/fleetz.git";
    id_repo(&root.join("fleetz"), Some(url), false);
    id_repo(&root.join("fleetz-copy"), Some(url), true);
    let entry = id_entry(
        Some("Gaurgle"),
        "fleetz",
        0,
        model::CiStatus::Passed,
        1,
        model::RepoKind::Private,
    );

    let (repos, _) = id_assemble(
        &home,
        std::slice::from_ref(&root),
        std::slice::from_ref(&entry),
        None,
    )
    .await;

    assert_eq!(repos.len(), 2, "{repos:#?}");
    let primary = row_at(&repos, &root.join("fleetz"));
    assert_eq!(primary.location, RepoLocation::Primary);
    assert_eq!(primary.owner.as_deref(), Some("Gaurgle"));
    assert_eq!(primary.ci_status, model::CiStatus::Passed);
    assert_eq!(primary.pr_count, 1);
    let copy = row_at(&repos, &root.join("fleetz-copy"));
    assert_eq!(copy.location, RepoLocation::LocalOnly);
    assert_carries_nothing(copy);

    let mut app = App::new(repos, "h".to_string(), 120, 44);
    let mut now = id_entry(
        Some("Gaurgle"),
        "fleetz",
        0,
        model::CiStatus::Failed,
        2,
        model::RepoKind::Private,
    );
    poll_entries(&mut app, vec![now.clone()]);
    let primary = row_at(&app.repos, &root.join("fleetz"));
    assert_eq!(primary.ci_status, model::CiStatus::Failed);
    assert_eq!(primary.pr_count, 2);
    assert_carries_nothing(row_at(&app.repos, &root.join("fleetz-copy")));

    let primary_index = app
        .repos
        .iter()
        .position(|r| r.location == RepoLocation::Primary)
        .unwrap();
    let before = app.dataset.clone();
    now.pushed_at += chrono::Duration::hours(1);
    assert_eq!(
        remotes_that_moved(&before, &[now], &app.repos),
        vec![primary_index]
    );

    std::fs::remove_dir_all(&home).ok();
}

/// S4's poll twin. The dirty copy at `old/fleetz` has the remote's own
/// name, so the name match wrote the poll onto it as well. Its load half
/// waits for FZ-05f: pass 3 hides it by name until then.
#[test]
fn an_extra_checkout_named_like_its_remote_takes_nothing_from_the_poll() {
    let mut primary = cloned("fleetz");
    primary.owner = Some("Gaurgle".to_string());
    primary.remote_url = Some("git@github.com:Gaurgle/fleetz.git".to_string());
    let mut copy = cloned("fleetz");
    copy.path = Some("~/repos/old/fleetz".to_string());
    copy.location = RepoLocation::LocalOnly;
    copy.remote_url = primary.remote_url.clone();
    let mut app = App::new(vec![primary, copy], "h".to_string(), 120, 44);

    let mut now = id_entry(
        Some("Gaurgle"),
        "fleetz",
        0,
        model::CiStatus::Failed,
        2,
        model::RepoKind::Private,
    );
    poll_entries(&mut app, vec![now.clone()]);

    assert_eq!(app.repos[0].ci_status, model::CiStatus::Failed);
    assert_eq!(app.repos[0].pr_count, 2);
    assert_carries_nothing(&app.repos[1]);
    let before = app.dataset.clone();
    now.pushed_at += chrono::Duration::hours(1);
    assert_eq!(remotes_that_moved(&before, &[now], &app.repos), vec![0]);
}

/// S14, load: a remote-less clone named like a repo outside the window
/// takes nothing from it, not even its glyph.
#[tokio::test]
async fn a_remoteless_clone_takes_no_glyph_from_a_same_named_repo() {
    let home = wt_scratch("id_s14");
    let root = home.join("R");
    let clone = root.join("api");
    id_repo(&clone, None, true);
    let entry = id_entry(
        Some("company"),
        "api",
        60,
        model::CiStatus::Passed,
        1,
        model::RepoKind::Org,
    );

    let (repos, _) = id_assemble(
        &home,
        std::slice::from_ref(&root),
        std::slice::from_ref(&entry),
        one_day(),
    )
    .await;

    assert_eq!(repos.len(), 1, "{repos:#?}");
    let local = row_at(&repos, &clone);
    assert_eq!(local.location, RepoLocation::LocalOnly);
    assert_carries_nothing(local);

    std::fs::remove_dir_all(&home).ok();
}

/// S14, poll (section 5, FZ-05d): a LocalOnly row with no owner and no
/// origin is never written by name, whether the entry has an owner or
/// is a legacy one without. This is F4: a stranger's CI badge on a local
/// repo on the first tick.
#[test]
fn a_remoteless_local_row_takes_nothing_from_the_poll() {
    for owner in [Some("company"), None] {
        let mut local = cloned("api");
        local.location = RepoLocation::LocalOnly;
        let mut app = App::new(vec![local], "h".to_string(), 120, 44);
        let entry = id_entry(
            owner,
            "api",
            0,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Org,
        );
        poll_entries(&mut app, vec![entry]);
        let row = &app.repos[0];
        assert_eq!(row.ci_status, model::CiStatus::Unknown, "owner {owner:?}");
        assert_eq!(row.pr_count, 0, "owner {owner:?}");
        assert_eq!(row.repo_kind, model::RepoKind::Unknown, "owner {owner:?}");
    }
}

/// S6: a dirty linked worktree beside its main clone. The main clone is
/// the Primary carrier; the worktree's stored origin names the same
/// remote, so it is a LocalOnly row with local fields only.
#[tokio::test]
async fn a_worktree_beside_its_main_clone_carries_nothing() {
    let home = wt_scratch("id_s6");
    let root = home.join("R");
    let main = root.join("site");
    id_repo(
        &main,
        Some("git@github.com:Gaurgle/portfolio-site.git"),
        false,
    );
    let worktree = root.join("site-feature");
    wt_sh(
        &main,
        &[
            "git",
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            &path_arg(&worktree),
        ],
    );
    std::fs::write(worktree.join("a.txt"), "two\n").unwrap();
    let entry = id_entry(
        Some("Gaurgle"),
        "portfolio-site",
        0,
        model::CiStatus::Passed,
        1,
        model::RepoKind::Public,
    );

    let (repos, _) = id_assemble(
        &home,
        std::slice::from_ref(&root),
        std::slice::from_ref(&entry),
        None,
    )
    .await;

    assert_eq!(repos.len(), 2, "{repos:#?}");
    let primary = row_at(&repos, &main);
    assert_eq!(primary.location, RepoLocation::Primary);
    assert_eq!(primary.owner.as_deref(), Some("Gaurgle"));
    assert_eq!(primary.ci_status, model::CiStatus::Passed);
    let local = row_at(&repos, &worktree);
    assert_eq!(local.location, RepoLocation::LocalOnly);
    assert!(
        local.remote_url.is_some(),
        "the worktree's origin was not read"
    );
    assert_carries_nothing(local);

    std::fs::remove_dir_all(&home).ok();
}

/// S6b: only the worktree is under a scan root, and its main clone is
/// outside every root and outside the home. It carries the remote
/// through its stored origin, with GitHub fields from the first frame,
/// and the repo is not also listed as not cloned.
#[tokio::test]
async fn a_lone_worktree_carries_its_remote_through_its_stored_origin() {
    let scratch = wt_scratch("id_s6b");
    let home = scratch.join("home");
    let root = home.join("R");
    std::fs::create_dir_all(&root).unwrap();
    let main = scratch.join("outside").join("site");
    id_repo(
        &main,
        Some("git@github.com:Gaurgle/portfolio-site.git"),
        false,
    );
    let worktree = root.join("site-feature");
    wt_sh(
        &main,
        &[
            "git",
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            &path_arg(&worktree),
        ],
    );
    std::fs::write(worktree.join("a.txt"), "two\n").unwrap();
    let entry = id_entry(
        Some("Gaurgle"),
        "portfolio-site",
        0,
        model::CiStatus::Passed,
        1,
        model::RepoKind::Public,
    );

    let (repos, _) = id_assemble(
        &home,
        std::slice::from_ref(&root),
        std::slice::from_ref(&entry),
        None,
    )
    .await;

    assert_eq!(repos.len(), 1, "one row, no NotCloned: {repos:#?}");
    let row = row_at(&repos, &worktree);
    assert_eq!(row.location, RepoLocation::LocalOnly);
    assert_eq!(row.name, "site-feature");
    assert_eq!(row.owner.as_deref(), Some("Gaurgle"));
    assert_eq!(row.ci_status, model::CiStatus::Passed);
    assert_eq!(row.pr_count, 1);
    assert_eq!(row.repo_kind, model::RepoKind::Public);

    std::fs::remove_dir_all(&scratch).ok();
}

/// S9d: a clone whose origin names a known repo outside the window, in a
/// folder named like another repo in it. The folder claims nothing; the
/// clone carries the repo its origin names, at load and in the poll, and
/// the repo its folder names stays not cloned. Before, the LocalOnly row
/// took the folder repo's glyph and hid its NotCloned row.
#[tokio::test]
async fn a_clone_carries_the_known_repo_its_origin_names() {
    let home = wt_scratch("id_s9d");
    let root = home.join("R");
    let clone = root.join("tool");
    id_repo(&clone, Some("git@github.com:Gaurgle/other.git"), true);
    let tool = id_entry(
        Some("Gaurgle"),
        "tool",
        0,
        model::CiStatus::Failed,
        0,
        model::RepoKind::Public,
    );
    let other = id_entry(
        Some("Gaurgle"),
        "other",
        60,
        model::CiStatus::Passed,
        1,
        model::RepoKind::Private,
    );

    let (repos, _) = id_assemble(
        &home,
        std::slice::from_ref(&root),
        &[tool.clone(), other.clone()],
        one_day(),
    )
    .await;

    assert_eq!(repos.len(), 2, "{repos:#?}");
    let folder_repo = not_cloned(&repos, "Gaurgle", "tool").expect("Gaurgle/tool");
    assert_eq!(folder_repo.ci_status, model::CiStatus::Failed);
    let local = row_at(&repos, &clone);
    assert_eq!(local.location, RepoLocation::LocalOnly);
    assert_eq!(local.name, "tool");
    assert_eq!(local.owner.as_deref(), Some("Gaurgle"));
    assert_eq!(local.ci_status, model::CiStatus::Passed);
    assert_eq!(local.pr_count, 1);
    assert_eq!(local.repo_kind, model::RepoKind::Private);

    // The poll keys the carrier by the repo its origin names, not by its
    // folder's name, which is the other repo's.
    let mut app = App::new(repos, "h".to_string(), 120, 44);
    let mut tool_now = tool;
    tool_now.ci_status = model::CiStatus::Pending;
    let mut other_now = other;
    other_now.ci_status = model::CiStatus::Failed;
    poll_entries(&mut app, vec![tool_now, other_now]);
    let local = row_at(&app.repos, &clone);
    assert_eq!(local.ci_status, model::CiStatus::Failed);
    let folder_repo = not_cloned(&app.repos, "Gaurgle", "tool").unwrap();
    assert_eq!(folder_repo.ci_status, model::CiStatus::Pending);

    std::fs::remove_dir_all(&home).ok();
}

/// S11a: a clone of a project on another host, in a folder named like a
/// GitHub repo. The GitHub repo is not cloned and carries its CI; the
/// clone is a LocalOnly row with no GitHub fields and no glyph. Before,
/// its pass 3 row took the glyph by name and hid the NotCloned row.
#[tokio::test]
async fn a_foreign_host_clone_leaves_the_github_repo_not_cloned() {
    let home = wt_scratch("id_s11a");
    let root = home.join("R");
    let clone = root.join("api");
    id_repo(&clone, Some("git@gitlab.com:someone/api.git"), true);
    let entry = id_entry(
        Some("company"),
        "api",
        0,
        model::CiStatus::Passed,
        1,
        model::RepoKind::Org,
    );

    let (repos, _) = id_assemble(
        &home,
        std::slice::from_ref(&root),
        std::slice::from_ref(&entry),
        None,
    )
    .await;

    assert_eq!(repos.len(), 2, "{repos:#?}");
    let carrier = not_cloned(&repos, "company", "api").expect("company/api");
    assert_eq!(carrier.ci_status, model::CiStatus::Passed);
    assert_eq!(carrier.pr_count, 1);
    let local = row_at(&repos, &clone);
    assert_eq!(local.location, RepoLocation::LocalOnly);
    assert_carries_nothing(local);

    std::fs::remove_dir_all(&home).ok();
}

/// S13b: `~/.dotfiles` as a scan root, with an origin, dirty, its repo
/// pushed outside the window. No other row has the repo, so the clone
/// carries it: glyph, CI and PRs at load, not first after a poll, and
/// the poll keeps writing them.
#[tokio::test]
async fn an_out_of_window_dotfiles_clone_carries_its_repo_from_the_first_frame() {
    let home = wt_scratch("id_s13b");
    let root = home.join("R");
    std::fs::create_dir_all(&root).unwrap();
    let dotfiles = home.join(".dotfiles");
    id_repo(&dotfiles, Some("git@github.com:Gaurgle/dotfiles.git"), true);
    let entry = id_entry(
        Some("Gaurgle"),
        "dotfiles",
        60,
        model::CiStatus::Passed,
        1,
        model::RepoKind::Public,
    );

    let (repos, _) = id_assemble(
        &home,
        &[root.clone(), dotfiles.clone()],
        std::slice::from_ref(&entry),
        one_day(),
    )
    .await;

    assert_eq!(repos.len(), 1, "{repos:#?}");
    let row = row_at(&repos, &dotfiles);
    assert_eq!(row.location, RepoLocation::LocalOnly);
    assert_eq!(row.name, "dotfiles");
    assert_eq!(row.owner.as_deref(), Some("Gaurgle"));
    assert_eq!(row.ci_status, model::CiStatus::Passed);
    assert_eq!(row.pr_count, 1);
    assert_eq!(row.repo_kind, model::RepoKind::Public);

    let mut app = App::new(repos, "h".to_string(), 120, 44);
    let mut now = entry;
    now.ci_status = model::CiStatus::Failed;
    poll_entries(&mut app, vec![now]);
    assert_eq!(app.repos[0].ci_status, model::CiStatus::Failed);

    std::fs::remove_dir_all(&home).ok();
}

/// S17: a dirty clone with an origin at depth 4 under the root, which
/// pass 1 (depth 3) misses and pass 3 (depth 4) reaches. It is the one
/// row for its repo and carries it from the first frame; no NotCloned
/// row beside it.
#[tokio::test]
async fn a_depth_four_clone_carries_its_repo_at_load() {
    let home = wt_scratch("id_s17");
    let root = home.join("R");
    let deep = root.join("a").join("b").join("c").join("fleetz");
    id_repo(&deep, Some("git@github.com:Gaurgle/fleetz.git"), true);
    let entry = id_entry(
        Some("Gaurgle"),
        "fleetz",
        0,
        model::CiStatus::Passed,
        1,
        model::RepoKind::Public,
    );

    let (repos, _) = id_assemble(
        &home,
        std::slice::from_ref(&root),
        std::slice::from_ref(&entry),
        None,
    )
    .await;

    assert_eq!(repos.len(), 1, "one row, no NotCloned: {repos:#?}");
    let row = row_at(&repos, &deep);
    assert_eq!(row.location, RepoLocation::LocalOnly);
    assert_eq!(row.owner.as_deref(), Some("Gaurgle"));
    assert_eq!(row.ci_status, model::CiStatus::Passed);
    assert_eq!(row.pr_count, 1);

    std::fs::remove_dir_all(&home).ok();
}

/// The first-poll baseline reads each clone's own repo: two same-named
/// repos of two owners are pushed at different times. By name, both
/// clones were judged by one of the two.
#[test]
fn the_startup_fetch_judges_each_clone_by_its_own_repo() {
    let dir = fetch_fixture("owners", &["alice-api", "company-api"], true);
    let target = |folder: &str, owner: &str| {
        (
            discovery::FoundRepo {
                name: "api".to_string(),
                path: dir.join(folder),
                owner: Some(owner.to_string()),
            },
            RepoLocation::Primary,
        )
    };
    let targets = vec![
        target("alice-api", "alice"),
        target("company-api", "company"),
    ];
    let mut alice = gh("api", "2099-01-01T00:00:00Z");
    alice.owner = Some("alice".to_string());
    let mut company = gh("api", "2001-01-01T00:00:00Z");
    company.owner = Some("company".to_string());

    assert_eq!(
        stale_remotes(&targets, &[alice.clone(), company.clone()]),
        vec![dir.join("alice-api")]
    );
    assert_eq!(
        stale_remotes(&targets, &[company, alice]),
        vec![dir.join("alice-api")]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Push order is per repo: two same-named repos of two owners each sort
/// by their own push. By name, both took one rank.
#[test]
fn same_named_repos_of_two_owners_sort_by_their_own_push() {
    let dir = Path::new("/tmp/x");
    let found = |name: &str, owner: Option<&str>| {
        (
            discovery::FoundRepo {
                name: name.to_string(),
                path: dir.join(format!("{}-{name}", owner.unwrap_or("none"))),
                owner: owner.map(str::to_string),
            },
            RepoLocation::Primary,
        )
    };
    let mut targets = vec![
        found("api", Some("company")),
        found("mid", None),
        found("api", Some("alice")),
    ];
    let mut alice = gh("api", "2026-08-24T10:00:00Z");
    alice.owner = Some("alice".to_string());
    let mid = gh("mid", "2026-08-20T10:00:00Z");
    let mut company = gh("api", "2026-08-01T10:00:00Z");
    company.owner = Some("company".to_string());

    sort_by_activity(&mut targets, &[alice, mid, company]);
    let order: Vec<Option<&str>> = targets.iter().map(|(f, _)| f.owner.as_deref()).collect();
    assert_eq!(order, [Some("alice"), None, Some("company")]);
}

/// A push is judged against the same repo's previous push, not a
/// same-named repo's. `company/api` moves from 09:00 to 10:00 while
/// `alice/api` sits at 12:00: by name, 10:00 was checked against 12:00
/// and company's clone was never fetched.
#[test]
fn a_push_is_judged_against_the_same_owners_previous_push() {
    let entry = |owner: &str, pushed: &str| {
        let mut repo = gh("api", pushed);
        repo.owner = Some(owner.to_string());
        repo
    };
    let mut alices = cloned("api");
    alices.owner = Some("alice".to_string());
    alices.path = Some("~/repos/alice-api".to_string());
    let mut companys = cloned("api");
    companys.owner = Some("company".to_string());
    let repos = vec![alices, companys];

    let previous = vec![
        entry("alice", "2026-08-06T12:00:00Z"),
        entry("company", "2026-08-06T09:00:00Z"),
    ];
    let current = vec![
        entry("alice", "2026-08-06T12:00:00Z"),
        entry("company", "2026-08-06T10:00:00Z"),
    ];
    assert_eq!(remotes_that_moved(&previous, &current, &repos), vec![1]);
}

/// A launch served from a payload without owners, then a first network
/// poll that has them. The legacy previous entry is still the baseline:
/// unmatched, the push would read as unknown, never be fetched, and the
/// next poll would compare against the owned dataset and miss it too.
#[test]
fn a_push_since_a_legacy_payload_is_still_fetched() {
    let previous = vec![gh("api", "2026-08-06T09:00:00Z")];
    let mut current = gh("api", "2026-08-06T10:00:00Z");
    current.owner = Some("company".to_string());
    let repos = vec![cloned("api")];
    assert_eq!(remotes_that_moved(&previous, &[current], &repos), vec![0]);
}

/// A launch from a payload without owners, then a poll holding both
/// `alice/api` and `company/api` (FZ-05e keeps both), each newer than the
/// legacy `api`. Both match that baseline and both resolve to the one
/// owner-less row, which came back twice and got two single sweeps.
#[test]
fn a_row_two_moved_entries_resolve_to_is_fetched_once() {
    let previous = vec![gh("api", "2026-08-06T09:00:00Z")];
    let owned = |owner: &str| {
        let mut repo = gh("api", "2026-08-06T10:00:00Z");
        repo.owner = Some(owner.to_string());
        repo
    };
    let current = vec![owned("alice"), owned("company")];
    let repos = vec![cloned("api")];
    assert_eq!(remotes_that_moved(&previous, &current, &repos), vec![0]);
}

/// The carrier among LocalOnly rows equal on every earlier step is
/// decided by the path, whichever order the walk listed them in.
#[test]
fn the_local_carrier_does_not_depend_on_listing_order() {
    let root = PathBuf::from("/tmp/fleetz_no_such_root/R");
    let a = root.join("fleetz-a");
    let b = root.join("fleetz-b");
    let row = |name: &str| {
        let mut repo = cloned(name);
        repo.location = RepoLocation::LocalOnly;
        repo.remote_url = Some("git@github.com:Gaurgle/fleetz.git".to_string());
        repo
    };
    let entry = id_entry(
        Some("Gaurgle"),
        "fleetz",
        0,
        model::CiStatus::Passed,
        1,
        model::RepoKind::Public,
    );
    let listings = [
        [(a.as_path(), "fleetz-a"), (b.as_path(), "fleetz-b")],
        [(b.as_path(), "fleetz-b"), (a.as_path(), "fleetz-a")],
    ];
    for listing in listings {
        let mut rows: Vec<(&Path, RepoStatus)> = listing
            .iter()
            .map(|(path, name)| (*path, row(name)))
            .collect();
        let mut carried = HashSet::new();
        mark_local_carriers(
            &mut rows,
            std::slice::from_ref(&entry),
            std::slice::from_ref(&root),
            &mut carried,
        );
        let carriers: Vec<&str> = rows
            .iter()
            .filter(|(_, r)| r.owner.is_some())
            .map(|(_, r)| r.name.as_str())
            .collect();
        assert_eq!(carriers, ["fleetz-a"], "listed as {listing:?}");
    }
}

/// Several LocalOnly clones naming one repo: the carrier is the first in
/// discovery's primary-checkout order, not the first the filesystem
/// lists. Pair one ties on every step but the path, so the path decides
/// (`fleetz-a`). Pair two is decided by depth against the path order:
/// `a/notes-deep` sorts first by path, but `z-notes` has fewer
/// components and wins. The repos are pushed outside the window, so
/// passes 1 and 2 never look for them and only pass 3 sees the clones.
#[tokio::test]
async fn the_carrier_among_local_clones_follows_the_primary_checkout_order() {
    let home = wt_scratch("id_order");
    let root = home.join("R");
    let fleetz = "git@github.com:Gaurgle/fleetz.git";
    let notes = "git@github.com:Gaurgle/notes.git";
    id_repo(&root.join("fleetz-b"), Some(fleetz), true);
    id_repo(&root.join("fleetz-a"), Some(fleetz), true);
    id_repo(&root.join("z-notes"), Some(notes), true);
    id_repo(&root.join("a").join("notes-deep"), Some(notes), true);
    let dataset = vec![
        id_entry(
            Some("Gaurgle"),
            "fleetz",
            60,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Public,
        ),
        id_entry(
            Some("Gaurgle"),
            "notes",
            60,
            model::CiStatus::Failed,
            2,
            model::RepoKind::Private,
        ),
    ];

    let (repos, _) = id_assemble(&home, std::slice::from_ref(&root), &dataset, one_day()).await;

    assert_eq!(repos.len(), 4, "{repos:#?}");
    let carrier = row_at(&repos, &root.join("fleetz-a"));
    assert_eq!(carrier.owner.as_deref(), Some("Gaurgle"));
    assert_eq!(carrier.ci_status, model::CiStatus::Passed);
    assert_eq!(carrier.pr_count, 1);
    assert_carries_nothing(row_at(&repos, &root.join("fleetz-b")));

    let carrier = row_at(&repos, &root.join("z-notes"));
    assert_eq!(carrier.owner.as_deref(), Some("Gaurgle"));
    assert_eq!(carrier.ci_status, model::CiStatus::Failed);
    assert_eq!(carrier.pr_count, 2);
    assert_eq!(carrier.repo_kind, model::RepoKind::Private);
    assert_carries_nothing(row_at(&repos, &root.join("a").join("notes-deep")));

    std::fs::remove_dir_all(&home).ok();
}

/// A row with an owner is written only by an entry of that owner: a
/// same-named entry without one is not its repo (3.5, D1).
#[test]
fn an_owned_row_takes_nothing_from_an_ownerless_entry() {
    let mut row = cloned("api");
    row.owner = Some("company".to_string());
    let mut app = App::new(vec![row], "h".to_string(), 120, 44);
    let entry = id_entry(
        None,
        "api",
        0,
        model::CiStatus::Failed,
        1,
        model::RepoKind::Org,
    );
    poll_entries(&mut app, vec![entry]);
    let row = &app.repos[0];
    assert_eq!(row.ci_status, model::CiStatus::Unknown);
    assert_eq!(row.pr_count, 0);
    assert_eq!(row.repo_kind, model::RepoKind::Unknown);
}

/// The mirror, which must keep working: a row without an owner, the
/// legacy case (m2), is written by a same-named entry, owned or not.
#[test]
fn an_ownerless_row_is_still_written_by_name() {
    let mut app = App::new(vec![cloned("api")], "h".to_string(), 120, 44);
    let entry = id_entry(
        Some("company"),
        "api",
        0,
        model::CiStatus::Passed,
        1,
        model::RepoKind::Org,
    );
    poll_entries(&mut app, vec![entry]);
    let row = &app.repos[0];
    assert_eq!(row.ci_status, model::CiStatus::Passed);
    assert_eq!(row.pr_count, 1);
    assert_eq!(row.repo_kind, model::RepoKind::Org);
}

/// A legacy payload has no owners, so no repo in it has a remote identity
/// for a LocalOnly row's origin to name: the row carries nothing.
#[tokio::test]
async fn a_local_clone_carries_nothing_from_a_legacy_payload() {
    let home = wt_scratch("id_legacy");
    let root = home.join("R");
    let clone = root.join("fleetz-copy");
    id_repo(&clone, Some("git@github.com:Gaurgle/fleetz.git"), true);
    let entry = id_entry(
        None,
        "fleetz",
        60,
        model::CiStatus::Passed,
        1,
        model::RepoKind::Public,
    );

    let (repos, _) = id_assemble(
        &home,
        std::slice::from_ref(&root),
        std::slice::from_ref(&entry),
        one_day(),
    )
    .await;

    assert_eq!(repos.len(), 1, "{repos:#?}");
    let row = row_at(&repos, &clone);
    assert_eq!(row.location, RepoLocation::LocalOnly);
    assert_carries_nothing(row);

    std::fs::remove_dir_all(&home).ok();
}

// --- FZ-05f: pass 3 dedupes by canonical path only ---

/// A dirty remote-less repo at `dir`.
fn dirty_remoteless(dir: &Path) {
    id_repo(dir, None, true);
}

/// S4, load and poll. Two clones of one remote with the same folder name:
/// the Primary at `R/fleetz` carries the repo, and the dirty `R/old/fleetz`
/// is a LocalOnly row with local fields only. A third checkout at
/// `R/older/fleetz` with nothing uncommitted or unpushed (cloned, so its
/// `origin/main` is its `main`) is not news and gets no row. Before, pass
/// 3 hid the dirty copy because its folder name was already claimed.
#[tokio::test]
async fn a_dirty_extra_checkout_named_like_its_remote_is_a_local_row() {
    let home = wt_scratch("id_s4");
    let root = home.join("R");
    let url = "git@github.com:Gaurgle/fleetz.git";
    id_repo(&root.join("fleetz"), Some(url), false);
    id_repo(&root.join("old").join("fleetz"), Some(url), true);
    let older = root.join("older");
    std::fs::create_dir_all(&older).unwrap();
    wt_sh(
        &older,
        &[
            "git",
            "clone",
            "-q",
            &path_arg(&root.join("fleetz")),
            "fleetz",
        ],
    );
    wt_sh(
        &older.join("fleetz"),
        &["git", "remote", "set-url", "origin", url],
    );
    let entry = id_entry(
        Some("Gaurgle"),
        "fleetz",
        0,
        model::CiStatus::Passed,
        1,
        model::RepoKind::Private,
    );

    let (repos, _) = id_assemble(
        &home,
        std::slice::from_ref(&root),
        std::slice::from_ref(&entry),
        None,
    )
    .await;

    assert_eq!(repos.len(), 2, "{repos:#?}");
    let primary = row_at(&repos, &root.join("fleetz"));
    assert_eq!(primary.location, RepoLocation::Primary);
    assert_eq!(primary.owner.as_deref(), Some("Gaurgle"));
    assert_eq!(primary.ci_status, model::CiStatus::Passed);
    assert_eq!(primary.pr_count, 1);
    let copy = row_at(&repos, &root.join("old").join("fleetz"));
    assert_eq!(copy.location, RepoLocation::LocalOnly);
    assert_eq!(copy.name, "fleetz");
    assert_eq!(copy.dirty, 1);
    assert_carries_nothing(copy);

    let mut app = App::new(repos, "h".to_string(), 120, 44);
    let now = id_entry(
        Some("Gaurgle"),
        "fleetz",
        0,
        model::CiStatus::Failed,
        2,
        model::RepoKind::Private,
    );
    poll_entries(&mut app, vec![now]);
    let primary = row_at(&app.repos, &root.join("fleetz"));
    assert_eq!(primary.ci_status, model::CiStatus::Failed);
    assert_eq!(primary.pr_count, 2);
    assert_carries_nothing(row_at(&app.repos, &root.join("old").join("fleetz")));

    std::fs::remove_dir_all(&home).ok();
}

/// S8b: two dirty remote-less repos both named `notes` are two LocalOnly
/// rows, one per path. Before, the second was hidden by name.
#[tokio::test]
async fn two_same_named_remoteless_repos_are_two_local_rows() {
    let home = wt_scratch("id_s8b");
    let root = home.join("R");
    let a = root.join("a").join("notes");
    let b = root.join("b").join("notes");
    dirty_remoteless(&a);
    dirty_remoteless(&b);

    let (repos, _) = id_assemble(&home, std::slice::from_ref(&root), &[], None).await;

    assert_eq!(repos.len(), 2, "{repos:#?}");
    for path in [&a, &b] {
        let row = row_at(&repos, path);
        assert_eq!(row.location, RepoLocation::LocalOnly);
        assert_eq!(row.name, "notes");
        assert_carries_nothing(row);
    }

    std::fs::remove_dir_all(&home).ok();
}

/// Two owners of one name in the dataset, a Primary clone of one of them,
/// and a dirty remote-less `api` elsewhere under the root. The clone
/// carries `company/api`, `alice/api` stays not cloned, and the remote-less
/// folder, ambiguous by name (rule 2 of 3.3), claims neither and is a
/// LocalOnly row with no GitHub fields, named in the status line. Before,
/// the Primary's name hid it.
#[tokio::test]
async fn a_remoteless_checkout_named_like_a_claimed_repo_is_a_local_row() {
    let home = wt_scratch("id_owners_p3");
    let root = home.join("R");
    let clone = root.join("api");
    id_repo(&clone, Some("git@github.com:company/api.git"), false);
    let scratch = root.join("x").join("api");
    dirty_remoteless(&scratch);
    let dataset = vec![
        id_entry(
            Some("alice"),
            "api",
            0,
            model::CiStatus::Passed,
            1,
            model::RepoKind::Public,
        ),
        id_entry(
            Some("company"),
            "api",
            0,
            model::CiStatus::Failed,
            0,
            model::RepoKind::Org,
        ),
    ];

    let (repos, notes) = id_assemble(&home, std::slice::from_ref(&root), &dataset, None).await;

    assert_eq!(repos.len(), 3, "{repos:#?}");
    let primary = row_at(&repos, &clone);
    assert_eq!(primary.location, RepoLocation::Primary);
    assert_eq!(primary.owner.as_deref(), Some("company"));
    assert_eq!(primary.ci_status, model::CiStatus::Failed);
    assert!(not_cloned(&repos, "alice", "api").is_some());
    let local = row_at(&repos, &scratch);
    assert_eq!(local.location, RepoLocation::LocalOnly);
    assert_eq!(local.name, "api");
    assert_carries_nothing(local);
    assert_eq!(
        notes.ambiguity.as_deref(),
        Some(
            format!(
                "api: {} matches 2 GitHub repos, add an origin remote",
                tilde_display(&scratch)
            )
            .as_str()
        )
    );

    std::fs::remove_dir_all(&home).ok();
}

/// One directory reached through two spellings of one root is one row,
/// whether pass 1 claimed it (`fleetz`, dirty, with its remote) or only
/// pass 3 lists it (`scratch`). Spellings: a symlink to the root, the
/// root with `sub/..` appended, and the root canonicalized (`/private/var`
/// against `/var` for a macOS temp dir). Comparing raw paths would list
/// each repo once per spelling.
#[cfg(unix)]
#[tokio::test]
async fn one_directory_through_two_root_spellings_is_one_row() {
    let home = wt_scratch("id_spellings");
    let root = home.join("R");
    id_repo(
        &root.join("fleetz"),
        Some("git@github.com:Gaurgle/fleetz.git"),
        true,
    );
    dirty_remoteless(&root.join("scratch"));
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let link = home.join("L");
    std::os::unix::fs::symlink(&root, &link).unwrap();
    let entry = id_entry(
        Some("Gaurgle"),
        "fleetz",
        0,
        model::CiStatus::Passed,
        1,
        model::RepoKind::Public,
    );

    let spellings = [
        link.clone(),
        root.join("sub").join(".."),
        root.canonicalize().unwrap(),
    ];
    for other in spellings {
        let roots = [root.clone(), other.clone()];
        let (repos, _) = id_assemble(&home, &roots, std::slice::from_ref(&entry), None).await;
        assert_eq!(repos.len(), 2, "roots {roots:?}: {repos:#?}");
        assert_eq!(
            row_at(&repos, &root.join("fleetz")).location,
            RepoLocation::Primary,
            "roots {roots:?}"
        );
        assert_eq!(
            row_at(&repos, &root.join("scratch")).location,
            RepoLocation::LocalOnly,
            "roots {roots:?}"
        );
    }

    std::fs::remove_dir_all(&home).ok();
}

/// The dedupe key is canonical, the display path is not: a repo found
/// under a symlinked root shows the path as the root was spelled.
#[cfg(unix)]
#[tokio::test]
async fn a_row_under_a_symlinked_root_displays_the_root_as_spelled() {
    let home = wt_scratch("id_symlink_display");
    let real = home.join("real");
    dirty_remoteless(&real.join("scratch"));
    let link = home.join("R");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let (repos, _) = id_assemble(&home, std::slice::from_ref(&link), &[], None).await;

    assert_eq!(repos.len(), 1, "{repos:#?}");
    assert_eq!(
        repos[0].path.as_deref(),
        Some(tilde_display(&link.join("scratch")).as_str())
    );
    assert_eq!(repos[0].name, "scratch");

    std::fs::remove_dir_all(&home).ok();
}

/// `c` switches the strip's source and ctrl-c quits. Match arms are
/// ordered, so an unguarded `c` arm placed above the modifier check
/// silently turns the quit key into a pane key. That regression happened
/// once already.
#[test]
fn ctrl_c_is_matched_before_the_plain_c_arm() {
    let source = include_str!("main.rs");
    let ctrl_c = source
        .find("KeyCode::Char('c') if key.modifiers")
        .expect("ctrl-c arm present");
    let plain_c = source
        .find("KeyCode::Char('c') => {")
        .expect("plain c arm present");
    assert!(
        ctrl_c < plain_c,
        "the ctrl-c arm must be matched before the plain c arm"
    );
}

/// The local histogram is the most expensive probe in the sweep: a year
/// of log per repo. It must not run for a strip showing GitHub's own
/// numbers.
#[test]
fn the_daily_probe_is_skipped_unless_the_local_source_is_showing() {
    let mut app = App::new(vec![test_repo()], "h".to_string(), 120, 44);
    app.contributions = app::ContributionsMode::Github;
    assert!(
        contrib_probe(&app).is_none(),
        "github source needs no local log"
    );
    app.contributions = app::ContributionsMode::Local;
    let probe = contrib_probe(&app).expect("local source probes");
    assert_eq!(
        probe.start,
        contributions::grid_start(chrono::Local::now().date_naive()),
        "every repo in one sweep shares a grid"
    );
}

#[test]
fn the_probe_carries_the_configured_author_override() {
    let mut app = App::new(vec![test_repo()], "h".to_string(), 120, 44);
    app.contributions = app::ContributionsMode::Local;
    app.contrib_author = Some(r"me@work.com\|me@home.com".to_string());
    let probe = contrib_probe(&app).unwrap();
    assert_eq!(probe.author.as_deref(), Some(r"me@work.com\|me@home.com"));
}

/// The startup fetch carries the calendar; dropping it here left the
/// strip blank until the first background poll landed minutes later.
#[test]
fn a_load_result_stores_its_calendar_on_the_app() {
    let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
    assert!(app.github_calendar.is_none());
    let today = chrono::Local::now().date_naive();
    let mut cal = contributions::Calendar::new(today, contributions::Source::Github);
    cal.add(today, 5);
    apply_load_result(
        &mut app,
        &config::Config::default(),
        Ok(Box::new(LoadResult {
            kind: LoadKind::Initial,
            repos: Vec::new(),
            header: "h".to_string(),
            dataset: Some(Vec::new()),
            github_calendar: Some(Box::new(cal)),
            source: app::DatasetSource::Network,
            ambiguity: None,
            meta: None,
        })),
    );
    assert_eq!(
        app.github_calendar.as_ref().map(|c| c.total()),
        Some(5),
        "the startup fetch must fill the strip"
    );
}

/// A cache-served launch must leave the re-poll clock unset, so the first
/// tick polls immediately.
///
/// Startup passes `CacheMode::AnyCached` and takes any payload however
/// old, so "the load returned a dataset" never meant "GitHub was just
/// asked". Starting the clock from it put the first real poll a whole
/// `github_poll_interval` away, and the dashboard spent it showing what
/// the previous session had left on disk. Seen for real: a launch at
/// 15:38:11 painted a stale contribution count and did not correct it
/// until 15:38:50.
#[test]
fn a_cache_served_launch_leaves_the_poll_due_immediately() {
    let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
    apply_load_result(
        &mut app,
        &config::Config::default(),
        Ok(Box::new(LoadResult {
            kind: LoadKind::Initial,
            repos: Vec::new(),
            header: "h".to_string(),
            dataset: Some(Vec::new()),
            github_calendar: None,
            source: app::DatasetSource::Cache { written: None },
            ambiguity: None,
            meta: None,
        })),
    );
    assert!(
        app.github_freshness.last_attempt.is_none(),
        "a cached payload started the re-poll clock, delaying the first \
         real poll by a whole interval"
    );
}

/// The other half: a load that did reach GitHub has just polled, so the
/// clock starts here rather than firing again a tick later.
#[test]
fn a_network_served_launch_starts_the_poll_clock() {
    let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
    apply_load_result(
        &mut app,
        &config::Config::default(),
        Ok(Box::new(LoadResult {
            kind: LoadKind::Initial,
            repos: Vec::new(),
            header: "h".to_string(),
            dataset: Some(Vec::new()),
            github_calendar: None,
            source: app::DatasetSource::Network,
            ambiguity: None,
            meta: None,
        })),
    );
    assert!(
        app.github_freshness.last_attempt.is_some(),
        "fresh data should not re-poll at once"
    );
}

/// The owner's viewer page (design 1.2): 50 of 95, cut off 237 days back.
fn owners_coverage() -> github::Coverage {
    github::Coverage {
        viewer: github::PageCoverage {
            returned: 50,
            total: Some(95),
            has_next_page: Some(true),
            boundary: Some(chrono::Utc::now() - chrono::Duration::days(237)),
        },
        org_list: None,
        org_pages: Vec::new(),
    }
}

/// CV19: a window apply fetches nothing, so it carries no `FetchMeta`;
/// the app keeps the load's, and the apply draws its note from it.
#[test]
fn cv19_a_window_apply_keeps_the_loads_coverage_and_draws_its_note() {
    let scratch = wt_scratch("cv19");
    let store = window_state::WindowState::at(scratch.join("window.toml"));
    let config = config::Config::default();
    let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
    let meta = github::FetchMeta {
        coverage: Some(owners_coverage()),
        ..Default::default()
    };
    apply_load_result_saving_to(
        &mut app,
        &config,
        Ok(Box::new(LoadResult {
            kind: LoadKind::Initial,
            repos: Vec::new(),
            header: "h".to_string(),
            dataset: Some(Vec::new()),
            github_calendar: None,
            source: app::DatasetSource::Network,
            ambiguity: None,
            meta: Some(meta.clone()),
        })),
        &store,
    );
    assert_eq!(app.github_meta, meta);

    // As `spawn_window_apply` builds it, from the app's coverage.
    let w = time::TimeWindow::clamped(1, time::WindowUnit::Years);
    let header = with_coverage_note(
        w.label(config.work_start, config.work_end),
        app.github_meta.coverage.as_ref(),
        Some(w.cutoff(config.work_start, config.work_end)),
        config.repo_limit,
    );
    let notes = ViewNotes {
        header,
        ambiguity: None,
    };
    let result = window_apply_result(w, Vec::new(), notes);
    assert!(result.meta.is_none(), "a window apply fetched nothing");
    apply_load_result_saving_to(&mut app, &config, Ok(Box::new(result)), &store);

    assert_eq!(app.github_meta, meta, "the load's coverage was dropped");
    assert!(
        app.header.ends_with("(fetched 50 of 95 repos)"),
        "no note at 365 days: {}",
        app.header
    );
    std::fs::remove_dir_all(&scratch).ok();
}

/// A successful poll replaces the dataset and its coverage with it; a
/// failed one changes neither (contract rule 4).
#[test]
fn a_poll_replaces_the_coverage_and_a_failed_one_keeps_it() {
    let mut app = app_with_pending_repo();
    let coverage = owners_coverage();
    let mut fetch = poll_says_passed().unwrap();
    fetch.meta.coverage = Some(coverage.clone());
    poll(&mut app, Ok(fetch));
    assert_eq!(app.github_meta.coverage.as_ref(), Some(&coverage));

    poll(&mut app, Err("boom".to_string()));
    assert!(app.github_freshness.is_failing(), "the failure landed");
    assert_eq!(app.github_meta.coverage.as_ref(), Some(&coverage));
}

/// A time-window apply re-filters what is already loaded and carries no
/// calendar. It must not blank the one already on screen.
#[test]
fn a_window_apply_does_not_blank_the_calendar() {
    let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
    let today = chrono::Local::now().date_naive();
    let mut cal = contributions::Calendar::new(today, contributions::Source::Github);
    cal.add(today, 9);
    app.github_calendar = Some(Box::new(cal));
    apply_load_result(
        &mut app,
        &config::Config::default(),
        Ok(Box::new(LoadResult {
            kind: LoadKind::Initial,
            repos: Vec::new(),
            header: "h".to_string(),
            dataset: None,
            github_calendar: None,
            source: app::DatasetSource::Network,
            ambiguity: None,
            meta: None,
        })),
    );
    assert_eq!(app.github_calendar.as_ref().map(|c| c.total()), Some(9));
}

/// repoz lists newest-pushed first (`sort_by(.pushedAt) | reverse`, then
/// iterated in that order). fleetz drew the same section in `read_dir`
/// order, because `find_in_primary` reduces the push-ordered names to a
/// HashSet and the walker pushes whatever the filesystem hands back. On
/// APFS that is hash order: arbitrary, and it reshuffles when any repo is
/// added or removed.
#[test]
fn discovered_repos_are_ordered_by_push_recency() {
    let dir = Path::new("/tmp/x");
    // Deliberately not the order the walker found them in.
    let mut targets = found_targets(dir, &["oldest", "newest", "middle"]);
    let active = vec![
        gh("newest", "2026-08-24T10:00:00Z"),
        gh("middle", "2026-08-20T10:00:00Z"),
        gh("oldest", "2026-08-01T10:00:00Z"),
    ];
    sort_by_activity(&mut targets, &active);
    let names: Vec<&str> = targets.iter().map(|(f, _)| f.name.as_str()).collect();
    assert_eq!(names, ["newest", "middle", "oldest"]);
}

/// A repo the GitHub fetch never matched has no push date to sort by. It
/// must fall to the end rather than to the front, and keep the order
/// discovery found it in.
#[test]
fn repos_without_a_github_match_sort_last_in_discovery_order() {
    let dir = Path::new("/tmp/x");
    let mut targets = found_targets(dir, &["local-a", "known", "local-b"]);
    let active = vec![gh("known", "2026-08-24T10:00:00Z")];
    sort_by_activity(&mut targets, &active);
    let names: Vec<&str> = targets.iter().map(|(f, _)| f.name.as_str()).collect();
    assert_eq!(names, ["known", "local-a", "local-b"]);
}

/// The sort runs after the pass-1/pass-2 dedupe, so it must not disturb
/// which location won. Discovery pass order is load-bearing: a local clone
/// has to beat the not-cloned fallback.
#[test]
fn sorting_by_activity_does_not_change_where_a_repo_was_found() {
    let dir = Path::new("/tmp/x");
    let mut targets = vec![
        (
            discovery::FoundRepo {
                name: "a".to_string(),
                path: dir.join("a"),
                owner: None,
            },
            RepoLocation::Elsewhere,
        ),
        (
            discovery::FoundRepo {
                name: "b".to_string(),
                path: dir.join("b"),
                owner: None,
            },
            RepoLocation::Primary,
        ),
    ];
    let active = vec![
        gh("b", "2026-08-24T10:00:00Z"),
        gh("a", "2026-08-01T10:00:00Z"),
    ];
    sort_by_activity(&mut targets, &active);
    assert_eq!(targets[0].0.name, "b");
    assert_eq!(targets[0].1, RepoLocation::Primary);
    assert_eq!(
        targets[1].1,
        RepoLocation::Elsewhere,
        "location was rewritten"
    );
}

/// Case is not a sort key: GitHub reports `Glanze`, the clone on disk may
/// be `glanze`, and both sides of the lookup lowercase for exactly that
/// reason elsewhere in discovery.
#[test]
fn activity_order_survives_a_case_mismatch() {
    let dir = Path::new("/tmp/x");
    let mut targets = found_targets(dir, &["old", "glanze"]);
    let active = vec![
        gh("Glanze", "2026-08-24T10:00:00Z"),
        gh("old", "2026-08-01T10:00:00Z"),
    ];
    sort_by_activity(&mut targets, &active);
    let names: Vec<&str> = targets.iter().map(|(f, _)| f.name.as_str()).collect();
    assert_eq!(
        names,
        ["glanze", "old"],
        "case mismatch dropped it to the end"
    );
}

// --- FZ-06c: the worktree list is re-read on every sweep ---
//
// Each scenario sweeps a real repo with `rebuild_repo_status`, feeding
// every sweep the previous one's result as its snapshot, as the app does.
// Unless a test says otherwise, the row asserted on is the MAIN
// checkout's: its `worktrees` lists the linked worktrees of its clone.

fn wt_sh(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new(args[0])
        .args(&args[1..])
        .current_dir(dir)
        .status()
        .expect("spawn");
    assert!(status.success(), "command failed: {args:?}");
}

/// A scratch directory unique to this test and this test process.
fn wt_scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fleetz_wt_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A repo with one commit on `main` and an idle branch `spare`, at
/// `<scratch>/main`.
fn wt_repo(scratch: &Path) -> PathBuf {
    let dir = scratch.join("main");
    std::fs::create_dir_all(&dir).unwrap();
    wt_sh(&dir, &["git", "init", "-q", "-b", "main"]);
    wt_sh(&dir, &["git", "config", "user.email", "t@example.com"]);
    wt_sh(&dir, &["git", "config", "user.name", "t"]);
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    wt_sh(&dir, &["git", "add", "."]);
    wt_sh(&dir, &["git", "commit", "-q", "-m", "init"]);
    wt_sh(&dir, &["git", "branch", "spare"]);
    dir
}

fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// One sweep of `path` against `old`, without the histogram probe.
async fn sweep_once(path: &Path, old: &RepoStatus) -> RepoStatus {
    let target = SweepTarget {
        display_path: path_arg(path),
        fs_path: path.to_path_buf(),
        old: Box::new(old.clone()),
    };
    rebuild_repo_status(&target, None, None).await
}

/// The first sweep of a row, from a snapshot with no key.
async fn first_sweep(path: &Path) -> RepoStatus {
    let mut row = test_repo();
    row.path = Some(path_arg(path));
    sweep_once(path, &row).await
}

/// The branches the row's linked worktrees have checked out, sorted.
fn worktree_branches(row: &RepoStatus) -> Vec<String> {
    let mut names: Vec<String> = row.worktrees.iter().map(|w| w.branch.clone()).collect();
    names.sort();
    names
}

fn common_worktrees_dir(path: &Path) -> PathBuf {
    git::repo_dirs(path)
        .expect("a repo")
        .common_dir
        .join("worktrees")
}

/// E4a: `worktree add -b` must show up on the next sweep.
#[tokio::test]
async fn a_worktree_added_on_a_new_branch_is_listed_on_the_next_sweep() {
    let scratch = wt_scratch("e4a");
    let repo = wt_repo(&scratch);
    let before = first_sweep(&repo).await;
    assert!(before.worktrees.is_empty());

    let wt = path_arg(&scratch.join("lw"));
    wt_sh(&repo, &["git", "worktree", "add", "-q", "-b", "wt1", &wt]);
    let after = sweep_once(&repo, &before).await;

    assert_eq!(worktree_branches(&after), ["wt1"]);
}

/// E4b: `worktree add` of an existing branch creates no ref, so nothing
/// in the key moves; the list must move anyway.
#[tokio::test]
async fn a_worktree_added_on_an_existing_branch_is_listed_on_the_next_sweep() {
    let scratch = wt_scratch("e4b");
    let repo = wt_repo(&scratch);
    let before = first_sweep(&repo).await;
    assert!(before.worktrees.is_empty());

    let wt = path_arg(&scratch.join("lw"));
    wt_sh(&repo, &["git", "worktree", "add", "-q", &wt, "spare"]);
    let after = sweep_once(&repo, &before).await;

    assert_eq!(worktree_branches(&after), ["spare"]);
}

/// E4c: `worktree remove` of one of two linked worktrees.
#[tokio::test]
async fn a_removed_worktree_drops_off_on_the_next_sweep() {
    let scratch = wt_scratch("e4c");
    let repo = wt_repo(&scratch);
    let (one, two) = (scratch.join("lw1"), scratch.join("lw2"));
    wt_sh(&repo, &["git", "branch", "other"]);
    wt_sh(
        &repo,
        &["git", "worktree", "add", "-q", &path_arg(&one), "spare"],
    );
    wt_sh(
        &repo,
        &["git", "worktree", "add", "-q", &path_arg(&two), "other"],
    );
    let before = first_sweep(&repo).await;
    assert_eq!(worktree_branches(&before), ["other", "spare"]);

    wt_sh(&repo, &["git", "worktree", "remove", &path_arg(&one)]);
    let after = sweep_once(&repo, &before).await;

    assert_eq!(worktree_branches(&after), ["other"]);
}

/// E4d: a linked worktree switches to a new branch.
#[tokio::test]
async fn a_linked_worktree_switching_to_a_new_branch_is_seen_on_the_next_sweep() {
    let scratch = wt_scratch("e4d");
    let repo = wt_repo(&scratch);
    let wt = scratch.join("lw");
    wt_sh(
        &repo,
        &["git", "worktree", "add", "-q", &path_arg(&wt), "spare"],
    );
    let before = first_sweep(&repo).await;
    assert_eq!(worktree_branches(&before), ["spare"]);

    wt_sh(&wt, &["git", "switch", "-q", "-c", "fresh"]);
    let after = sweep_once(&repo, &before).await;

    assert_eq!(worktree_branches(&after), ["fresh"]);
}

/// E4d's quieter form: switching a linked worktree to a branch that
/// already exists moves no ref and touches nothing of the main
/// checkout's, so the main row's key cannot see it.
#[tokio::test]
async fn a_linked_worktree_switching_to_an_existing_branch_is_seen_on_the_next_sweep() {
    let scratch = wt_scratch("e4d_existing");
    let repo = wt_repo(&scratch);
    wt_sh(&repo, &["git", "branch", "other"]);
    let wt = scratch.join("lw");
    wt_sh(
        &repo,
        &["git", "worktree", "add", "-q", &path_arg(&wt), "spare"],
    );
    let before = first_sweep(&repo).await;
    assert_eq!(worktree_branches(&before), ["spare"]);

    wt_sh(&wt, &["git", "switch", "-q", "other"]);
    let after = sweep_once(&repo, &before).await;

    assert_eq!(worktree_branches(&after), ["other"]);
}

/// Git deletes `<common>/worktrees` with the last linked worktree, so the
/// absent directory is the only trace of the removal. It must read as an
/// empty list, never as "reuse the last one".
#[tokio::test]
async fn removing_the_only_worktree_empties_the_list() {
    let scratch = wt_scratch("only");
    let repo = wt_repo(&scratch);
    let first = first_sweep(&repo).await;
    let wt = path_arg(&scratch.join("lw"));
    wt_sh(&repo, &["git", "worktree", "add", "-q", "-b", "solo", &wt]);
    let with_one = sweep_once(&repo, &first).await;
    assert_eq!(worktree_branches(&with_one), ["solo"]);

    wt_sh(&repo, &["git", "worktree", "remove", &wt]);
    let after = sweep_once(&repo, &with_one).await;

    assert!(
        !common_worktrees_dir(&repo).exists(),
        "git left <common>/worktrees behind, so this did not test the absent path"
    );
    assert!(
        after.worktrees.is_empty(),
        "the removed worktree is still listed: {:?}",
        after.worktrees
    );
}

/// A sweep of the linked worktree's own row: it lists the main checkout
/// and every other linked worktree, and sees a sibling added on an
/// existing branch.
#[tokio::test]
async fn a_linked_worktrees_own_row_sees_a_new_sibling() {
    let scratch = wt_scratch("own_row");
    let repo = wt_repo(&scratch);
    wt_sh(&repo, &["git", "branch", "other"]);
    let wt = scratch.join("lw");
    wt_sh(
        &repo,
        &["git", "worktree", "add", "-q", &path_arg(&wt), "spare"],
    );
    let before = first_sweep(&wt).await;
    assert_eq!(worktree_branches(&before), ["main"]);

    let sibling = path_arg(&scratch.join("lw2"));
    wt_sh(&repo, &["git", "worktree", "add", "-q", &sibling, "other"]);
    let after = sweep_once(&wt, &before).await;

    assert_eq!(worktree_branches(&after), ["main", "other"]);
}

// --- FZ-06d: the local histogram has its own reuse key ---
//
// Every commit is dated explicitly and every probe passes its grid start
// in, so no assertion depends on today's date.

const HIST_EMAIL: &str = "hist@example.com";

fn hist_date(text: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

/// A histogram probe for `start`, counting `author` (None resolves each
/// repo's `user.email`, as an unset `contributions_author` does).
fn hist_probe(start: &str, author: Option<&str>) -> ContribProbe {
    ContribProbe {
        start: hist_date(start),
        author: author.map(str::to_string),
    }
}

/// Commit an empty change at noon local time on `day`, as whatever
/// identity the repo is configured with.
fn hist_commit(dir: &Path, day: &str, message: &str) {
    let when = format!("{day}T12:00:00");
    let status = std::process::Command::new("git")
        .args(["commit", "-q", "--allow-empty", "-m", message])
        .env("GIT_AUTHOR_DATE", &when)
        .env("GIT_COMMITTER_DATE", &when)
        .current_dir(dir)
        .status()
        .expect("spawn");
    assert!(status.success(), "commit failed in {}", dir.display());
}

/// An empty repo at `<scratch>/<name>` with `email` as its local identity.
fn hist_repo(scratch: &Path, name: &str, email: &str) -> PathBuf {
    let dir = scratch.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    wt_sh(&dir, &["git", "init", "-q", "-b", "main"]);
    wt_sh(&dir, &["git", "config", "user.email", email]);
    wt_sh(&dir, &["git", "config", "user.name", "h"]);
    dir
}

/// One sweep of `path` against `old`, with or without the probe.
async fn hist_sweep(path: &Path, old: &RepoStatus, probe: Option<&ContribProbe>) -> RepoStatus {
    let target = SweepTarget {
        display_path: path_arg(path),
        fs_path: path.to_path_buf(),
        old: Box::new(old.clone()),
    };
    rebuild_repo_status(&target, None, probe).await
}

/// A row for `path` that no sweep has seen yet.
fn hist_row(path: &Path) -> RepoStatus {
    let mut row = test_repo();
    row.path = Some(path_arg(path));
    row
}

/// The non-empty slots of a row's histogram, as (day, count), dated by
/// the grid `start` the caller expects the slots to be offsets from.
fn hist_days(row: &RepoStatus, start: &str) -> Vec<(chrono::NaiveDate, u16)> {
    let daily = row.daily.as_ref().expect("the sweep left no histogram");
    let start = hist_date(start);
    daily
        .0
        .iter()
        .enumerate()
        .filter(|(_, n)| **n > 0)
        .map(|(slot, n)| (start + chrono::Duration::days(slot as i64), *n))
        .collect()
}

fn hist_total(row: &RepoStatus) -> u32 {
    let daily = row.daily.as_ref().expect("the sweep left no histogram");
    daily.0.iter().map(|n| u32::from(*n)).sum()
}

/// E7: the grid start rolls over a week while the repo stays put. The
/// slots are offsets from the start they were built with, so reusing them
/// under the new start draws every commit a week late.
#[tokio::test]
async fn a_new_grid_start_reprobes_an_unchanged_repo() {
    let scratch = wt_scratch("hist_e7");
    let repo = hist_repo(&scratch, "r", HIST_EMAIL);
    hist_commit(&repo, "2024-01-10", "one");

    let first = hist_sweep(
        &repo,
        &hist_row(&repo),
        Some(&hist_probe("2023-10-01", Some(HIST_EMAIL))),
    )
    .await;
    assert_eq!(
        hist_days(&first, "2023-10-01"),
        [(hist_date("2024-01-10"), 1)]
    );

    let rolled = hist_sweep(
        &repo,
        &first,
        Some(&hist_probe("2023-10-08", Some(HIST_EMAIL))),
    )
    .await;
    let days = hist_days(&rolled, "2023-10-08");
    let _ = std::fs::remove_dir_all(&scratch);

    assert_eq!(
        days,
        [(hist_date("2024-01-10"), 1)],
        "the histogram built for the old grid start was reused"
    );
}

/// E14: a sweep without the probe (the strip showing GitHub data) must
/// not record the repo as seen by the histogram, or the next Local sweep
/// reuses a histogram that predates the commit.
#[tokio::test]
async fn a_sweep_without_the_probe_does_not_bless_a_stale_histogram() {
    let scratch = wt_scratch("hist_e14");
    let repo = hist_repo(&scratch, "r", HIST_EMAIL);
    hist_commit(&repo, "2024-01-10", "one");
    let probe = hist_probe("2023-10-01", Some(HIST_EMAIL));

    let local = hist_sweep(&repo, &hist_row(&repo), Some(&probe)).await;
    assert_eq!(hist_total(&local), 1);

    hist_commit(&repo, "2024-01-11", "two");
    let github = hist_sweep(&repo, &local, None).await;
    assert_eq!(
        hist_total(&github),
        1,
        "a sweep without the probe keeps the histogram it had"
    );

    let back = hist_sweep(&repo, &github, Some(&probe)).await;
    let total = hist_total(&back);
    let _ = std::fs::remove_dir_all(&scratch);

    assert_eq!(
        total, 2,
        "the Local sweep reused a histogram from before the commit"
    );
}

/// `--all` walks HEAD, so a commit on a detached HEAD counts. The
/// histogram key must carry HEAD's object id, since the ref signature
/// only records that HEAD is detached.
#[tokio::test]
async fn a_commit_on_a_detached_head_reaches_the_histogram() {
    let scratch = wt_scratch("hist_detached");
    let repo = hist_repo(&scratch, "r", HIST_EMAIL);
    hist_commit(&repo, "2024-01-10", "one");
    let probe = hist_probe("2023-10-01", Some(HIST_EMAIL));

    let before = hist_sweep(&repo, &hist_row(&repo), Some(&probe)).await;
    assert_eq!(hist_total(&before), 1);

    wt_sh(&repo, &["git", "checkout", "-q", "--detach"]);
    hist_commit(&repo, "2024-01-11", "detached");
    let after = hist_sweep(&repo, &before, Some(&probe)).await;
    let total = hist_total(&after);
    let _ = std::fs::remove_dir_all(&scratch);

    assert_eq!(total, 2, "the detached commit was not counted");
}

/// The configured author is part of the key: changing
/// `contributions_author` on an unchanged repo must count the new one.
#[tokio::test]
async fn a_changed_configured_author_reprobes_an_unchanged_repo() {
    let scratch = wt_scratch("hist_author");
    let repo = hist_repo(&scratch, "r", "a@example.com");
    hist_commit(&repo, "2024-01-10", "as a");
    wt_sh(&repo, &["git", "config", "user.email", "b@example.com"]);
    hist_commit(&repo, "2024-01-12", "as b");

    let as_a = hist_sweep(
        &repo,
        &hist_row(&repo),
        Some(&hist_probe("2023-10-01", Some("a@example.com"))),
    )
    .await;
    assert_eq!(
        hist_days(&as_a, "2023-10-01"),
        [(hist_date("2024-01-10"), 1)]
    );

    let as_b = hist_sweep(
        &repo,
        &as_a,
        Some(&hist_probe("2023-10-01", Some("b@example.com"))),
    )
    .await;
    let days = hist_days(&as_b, "2023-10-01");
    let _ = std::fs::remove_dir_all(&scratch);

    assert_eq!(
        days,
        [(hist_date("2024-01-12"), 1)],
        "the histogram counted for the previous author was reused"
    );
}

/// OD4: with no configured author the histogram counts the repo's
/// `user.email` as resolved at the re-probe, not one remembered from an
/// earlier probe.
#[tokio::test]
async fn an_unset_author_counts_the_email_resolved_at_the_reprobe() {
    let scratch = wt_scratch("hist_od4");
    let repo = hist_repo(&scratch, "r", "a@example.com");
    hist_commit(&repo, "2024-01-10", "as a");
    let probe = hist_probe("2023-10-01", None);

    let as_a = hist_sweep(&repo, &hist_row(&repo), Some(&probe)).await;
    assert_eq!(
        hist_days(&as_a, "2023-10-01"),
        [(hist_date("2024-01-10"), 1)]
    );

    wt_sh(&repo, &["git", "config", "user.email", "b@example.com"]);
    hist_commit(&repo, "2024-01-12", "as b");
    let mut snapshot = as_a.clone();
    snapshot.work_key = None;
    let as_b = hist_sweep(&repo, &snapshot, Some(&probe)).await;
    let days = hist_days(&as_b, "2023-10-01");
    let _ = std::fs::remove_dir_all(&scratch);

    assert_eq!(
        days,
        [(hist_date("2024-01-12"), 1)],
        "the re-probe did not count the newly resolved email alone"
    );
}

/// E2c, a check rather than this ticket's evidence: a commit of yours
/// fetched from another clone moves a remote-tracking ref, which `--all`
/// walks, so the next Local sweep must count it.
#[tokio::test]
async fn a_fetched_commit_of_yours_reaches_the_histogram() {
    let scratch = wt_scratch("hist_e2c");
    let origin = hist_repo(&scratch, "origin", HIST_EMAIL);
    hist_commit(&origin, "2024-01-10", "one");
    let clone = |name: &str| {
        let dir = scratch.join(name);
        wt_sh(
            &scratch,
            &["git", "clone", "-q", &path_arg(&origin), &path_arg(&dir)],
        );
        wt_sh(&dir, &["git", "config", "user.email", HIST_EMAIL]);
        wt_sh(&dir, &["git", "config", "user.name", "h"]);
        dir
    };
    let mine = clone("mine");
    let other = clone("other");
    let probe = hist_probe("2023-10-01", Some(HIST_EMAIL));

    let before = hist_sweep(&mine, &hist_row(&mine), Some(&probe)).await;
    assert_eq!(hist_total(&before), 1);

    hist_commit(&other, "2024-01-11", "elsewhere");
    wt_sh(
        &other,
        &["git", "push", "-q", "origin", "HEAD:refs/heads/side"],
    );
    wt_sh(&mine, &["git", "fetch", "-q", "origin"]);
    let after = hist_sweep(&mine, &before, Some(&probe)).await;
    let total = hist_total(&after);
    let _ = std::fs::remove_dir_all(&scratch);

    assert_eq!(total, 2, "the fetched commit was not counted");
}

// FZ-13u: the icons row of the settings overlay. The set is a
// thread-local, and libtest gives each test its own thread; the guard
// puts back whatever the thread held anyway, panics included.

struct RestoreIcons(app::Icons);

impl RestoreIcons {
    fn capture() -> Self {
        Self(ui::badges::active_icons())
    }
}

impl Drop for RestoreIcons {
    fn drop(&mut self) {
        ui::badges::set_active_icons(self.0);
    }
}

const ICONS_PROBE: &str = "glyph-probe";

/// One public repo in the detail view with watch on, so a list row and
/// the header both draw a mark from the icon set.
fn app_for_icons() -> App {
    let mut repo = test_repo();
    repo.name = ICONS_PROBE.to_string();
    repo.repo_kind = model::RepoKind::Public;
    repo.owner = Some("Gaurgle".to_string());
    let mut app = App::new(vec![repo], "h".to_string(), 120, 40);
    app.view = app::View::Detail;
    app.watch_mode = true;
    app
}

/// The kind glyph left of the repo's name in the list, and the header's
/// watch mark, as the next frame draws them.
fn drawn_icon_marks(app: &App) -> (String, String) {
    use ratatui::backend::TestBackend;
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| ui::render(f, app)).unwrap();
    let buf = term.backend().buffer().clone();
    let list = ui::panel_rects(buf.area, app).repo_list;
    let name: Vec<String> = ICONS_PROBE.chars().map(String::from).collect();
    let (x, y) = (list.top()..list.bottom())
        .find_map(|y| {
            (list.left()..list.right())
                .find(|&x| {
                    name.iter().enumerate().all(|(i, c)| {
                        let cx = x + i as u16;
                        cx < list.right() && buf[(cx, y)].symbol() == c
                    })
                })
                .map(|x| (x, y))
        })
        .expect("the repo row is drawn");
    let header: String = (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect();
    let watch = header
        .split_whitespace()
        .take_while(|word| *word != "watch")
        .last()
        .unwrap_or_default()
        .to_string();
    (buf[(x - 2, y)].symbol().to_string(), watch)
}

fn press(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

/// `4` is the strip's key (back from `5`, FZ-30d). From another pane it
/// focuses a folded strip and unfolds it; on the focused strip it folds
/// it and keeps focus. Fails if the handler binds the strip to any other
/// key.
#[tokio::test]
async fn the_4_key_reaches_and_folds_the_strip() {
    let args = Args::parse_from(["fleetz"]);
    let config = config::Config::default();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let backend = CrosstermBackend::new(io::stdout());
    let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 120, 44));
    let mut terminal =
        Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap();
    let mut app = App::new(Vec::new(), String::new(), 120, 44);
    app.view = app::View::Detail;
    app.contributions = app::ContributionsMode::Github;
    app.focused_panel = FocusedPanel::Detail;
    app.contributions_folded = true;

    let repo_dir = Path::new("/nonexistent");
    handle_normal_input(
        &mut app,
        &mut terminal,
        press('4'),
        &tx,
        &args,
        &config,
        repo_dir,
    )
    .await
    .unwrap();
    assert_eq!(
        app.focused_panel,
        FocusedPanel::Contributions,
        "not focused"
    );
    assert!(!app.contributions_folded, "a folded strip stayed folded");

    handle_normal_input(
        &mut app,
        &mut terminal,
        press('4'),
        &tx,
        &args,
        &config,
        repo_dir,
    )
    .await
    .unwrap();
    assert!(app.contributions_folded, "the focused strip did not fold");
    assert_eq!(
        app.focused_panel,
        FocusedPanel::Contributions,
        "folding moved focus"
    );
}

/// Open the overlay as `,` does and walk to the icons row with `j`.
fn open_settings_on_the_icons_row(app: &mut App) {
    app.overlay = Overlay::Settings(settings_overlay(app, &config::Config::default()));
    for _ in 0..app::SETTING_FIELDS.len() {
        if let Overlay::Settings(ref so) = app.overlay
            && so.field() == app::SettingField::Icons
        {
            return;
        }
        handle_settings_input(app, press('j'));
    }
    panic!("no icons row in the settings overlay");
}

/// What Enter applies once the file is written.
fn apply_open_settings(app: &mut App) {
    let Overlay::Settings(ref so) = app.overlay else {
        panic!("the settings overlay is not open");
    };
    let saved = so.clone();
    apply_live_settings(app, &saved);
}

/// Toggling the row and applying it changes the list row's kind glyph
/// and the header's watch mark on the next frame, and toggling again
/// brings the Nerd set back, with no restart. Fails if applying the
/// overlay leaves the active set alone.
#[test]
fn toggling_the_icons_row_redraws_the_rows_and_the_header() {
    let _restore = RestoreIcons::capture();
    ui::badges::set_active_icons(app::Icons::Nerd);
    let mut app = app_for_icons();
    let nerd = drawn_icon_marks(&app);
    assert_eq!(
        nerd.0,
        ui::badges::kind_glyph(model::RepoKind::Public).unwrap(),
        "the control did not draw the Nerd set"
    );

    open_settings_on_the_icons_row(&mut app);
    handle_settings_input(&mut app, press('l'));
    app.needs_redraw = false;
    apply_open_settings(&mut app);
    assert!(app.needs_redraw, "applying did not ask for a frame");
    assert_eq!(ui::badges::active_icons(), app::Icons::Plain);
    assert_eq!(
        drawn_icon_marks(&app),
        ("+".to_string(), "o".to_string()),
        "the next frame still draws the old set"
    );

    handle_settings_input(&mut app, press('l'));
    apply_open_settings(&mut app);
    assert_eq!(ui::badges::active_icons(), app::Icons::Nerd);
    assert_eq!(drawn_icon_marks(&app), nerd);
}

/// The text of the settings row for icons, as the frame draws it.
fn drawn_icons_row(app: &App) -> String {
    use ratatui::backend::TestBackend;
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| ui::render(f, app)).unwrap();
    let buf = term.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .find(|line| line.contains("icons") && (line.contains("nerd") || line.contains("plain")))
        .expect("the settings overlay draws an icons row")
}

/// Stepping the icons row repaints the next frame at once, pane 1 and
/// the row both, with no Enter, no save and no config read in between,
/// and again the other way. Fails if the row only changes the overlay's
/// own copy until it is saved.
#[test]
fn stepping_the_icons_row_repaints_the_next_frame_without_a_save() {
    let _restore = RestoreIcons::capture();
    ui::badges::set_active_icons(app::Icons::Nerd);
    let mut app = app_for_icons();
    let nerd = drawn_icon_marks(&app);

    open_settings_on_the_icons_row(&mut app);
    assert!(drawn_icons_row(&app).contains("nerd"));
    handle_settings_input(&mut app, press('l'));
    assert_eq!(
        drawn_icon_marks(&app),
        ("+".to_string(), "o".to_string()),
        "pane 1 still draws the Nerd set after the step"
    );
    assert!(drawn_icons_row(&app).contains("plain"));

    handle_settings_input(&mut app, press('h'));
    assert_eq!(
        drawn_icon_marks(&app),
        nerd,
        "the step back did not repaint"
    );
    assert!(drawn_icons_row(&app).contains("nerd"));
}

/// Esc after toggling the row discards it, as it discards every row:
/// the row previews at once and Esc restores the set in force.
#[test]
fn cancelling_the_overlay_leaves_the_icon_set_as_it_was() {
    let _restore = RestoreIcons::capture();
    for before in [app::Icons::Nerd, app::Icons::Plain] {
        ui::badges::set_active_icons(before);
        let mut app = app_for_icons();
        let drawn = drawn_icon_marks(&app);

        open_settings_on_the_icons_row(&mut app);
        handle_settings_input(&mut app, press('l'));
        handle_settings_input(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(matches!(app.overlay, Overlay::None), "Esc did not close");
        assert_eq!(ui::badges::active_icons(), before);
        assert_eq!(drawn_icon_marks(&app), drawn);
    }
}

/// The overlay opens on the set the app is drawing with, not on a
/// default, so saving another row writes back the set in force. Fails
/// if a session switched to `plain` would save `nerd` the next time any
/// setting is saved.
#[test]
fn the_overlay_opens_on_the_icon_set_in_force() {
    let _restore = RestoreIcons::capture();
    let app = app_for_icons();
    for in_force in [app::Icons::Plain, app::Icons::Nerd] {
        ui::badges::set_active_icons(in_force);
        let so = settings_overlay(&app, &config::Config::default());
        assert_eq!(so.icons, in_force);
        let written = so
            .config_updates()
            .into_iter()
            .find(|(key, _)| *key == "icons")
            .map(|(_, value)| value);
        let expected = match in_force {
            app::Icons::Nerd => "nerd",
            app::Icons::Plain => "plain",
        };
        assert_eq!(written, Some(config::SettingValue::Str(expected)));
    }
}

// FZ-10b: the scenarios of `docs/api-budget-design.md` section 5, each
// named after its number. Clocks, jitter and responses are planted; the
// interval is the default 30 s and `repo_limit` 50, a poll costing 3.

/// A jitter source that always draws the same values.
struct Planted {
    factor: f64,
    resume: i64,
}

impl app::Jitter for Planted {
    fn backoff_factor(&mut self) -> f64 {
        self.factor
    }
    fn resume_secs(&mut self) -> i64 {
        self.resume
    }
}

fn at(h: u32, m: u32, s: u32) -> chrono::DateTime<chrono::Local> {
    use chrono::TimeZone;
    chrono::Local
        .with_ymd_and_hms(2026, 10, 1, h, m, s)
        .single()
        .expect("unambiguous local time")
}

fn secs(n: i64) -> chrono::Duration {
    chrono::Duration::seconds(n)
}

/// A reading of `remaining` of 5000 on `token`, resetting at `reset`, as
/// stated by a server whose clock read `date`.
fn reading(
    token: &str,
    remaining: u32,
    reset: chrono::DateTime<chrono::Local>,
    date: chrono::DateTime<chrono::Local>,
) -> github::BudgetReading {
    github::BudgetReading {
        limit: 5000,
        remaining,
        used: 5000 - remaining,
        reset: reset.timestamp(),
        date: Some(date.with_timezone(&chrono::Utc)),
        token_id: token.to_string(),
    }
}

/// A successful poll that changes no row, carrying `budget`.
fn ok_with(
    budget: Option<github::BudgetReading>,
) -> Result<Box<github::GithubFetch>, github::FetchError> {
    Ok(Box::new(github::GithubFetch {
        repos: Vec::new(),
        calendar: None,
        has_pr_counts: true,
        origin: github::Origin::Network,
        meta: github::FetchMeta {
            budget,
            ..Default::default()
        },
    }))
}

/// A successful poll that returns `budget_app`'s row `r`, still
/// `Pending`, so the row stays covered by the last poll (FZ-09d).
fn ok_with_pending_r(
    budget: Option<github::BudgetReading>,
) -> Result<Box<github::GithubFetch>, github::FetchError> {
    let mut fetch = ok_with(budget)?;
    fetch.repos.push(github::GithubRepo {
        name: "r".to_string(),
        pushed_at: chrono::DateTime::UNIX_EPOCH,
        pr_count: 0,
        prs: vec![],
        ci_status: model::CiStatus::Pending,
        ci_workflow: None,
        ci_duration: None,
        repo_kind: model::RepoKind::Unknown,
        owner: None,
        disk_usage: None,
    });
    Ok(fetch)
}

fn err_with(
    failure: github::Failure,
    budget: Option<github::BudgetReading>,
) -> Result<Box<github::GithubFetch>, github::FetchError> {
    Err(github::FetchError {
        failure,
        fallback: None,
        meta: Box::new(github::FetchMeta {
            budget,
            ..Default::default()
        }),
    })
}

fn failure(kind: github::FailureKind) -> github::Failure {
    github::Failure::new(kind, format!("{kind:?}"))
}

/// A 403 or 429 secondary limit, with or without `retry-after`.
fn secondary(retry_after: Option<u64>) -> github::Failure {
    github::Failure {
        rate_limited: true,
        limit: Some(github::RateLimit::Secondary {
            retry_after: retry_after.map(std::time::Duration::from_secs),
        }),
        ..github::Failure::new(github::FailureKind::Http, "HTTP 429")
    }
}

/// Start a poll and land `result` as its answer at `now`.
fn land_at(
    app: &mut App,
    result: Result<Box<github::GithubFetch>, github::FetchError>,
    now: chrono::DateTime<chrono::Local>,
) {
    let attempt = claim_github_poll(app).expect("no poll in flight");
    answer_at(app, attempt, result, now);
}

fn answer_at(
    app: &mut App,
    attempt: u64,
    result: Result<Box<github::GithubFetch>, github::FetchError>,
    now: chrono::DateTime<chrono::Local>,
) {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    apply_github_poll_at(app, attempt, result, &tx, &None, now);
}

/// A watching app with one row of `ci` (no path, so nothing is swept)
/// and planted jitter.
fn budget_app(ci: model::CiStatus, factor: f64, resume: i64) -> (App, config::Config) {
    let mut app = App::new(
        vec![RepoStatus {
            ci_status: ci,
            ..test_repo()
        }],
        "h".to_string(),
        120,
        44,
    );
    app.watch_mode = true;
    app.jitter = Box::new(Planted { factor, resume });
    (app, config::Config::default())
}

/// Seconds after `from` at which the next automatic attempt is first due.
fn next_due(app: &App, config: &config::Config, from: chrono::DateTime<chrono::Local>) -> i64 {
    (0..=4000)
        .find(|s| github_poll_due(app, config, from + secs(*s)))
        .expect("never due again")
}

fn status_text(app: &App) -> String {
    app.status_message
        .as_ref()
        .map(|m| m.text.clone())
        .unwrap_or_default()
}

/// The rate limit of S6 and S15: a `RATE_LIMITED` body at 17:00:43 whose
/// headers say 0 left until 17:23:06.
fn land_primary_limit(app: &mut App) {
    let budget = reading("A", 0, at(17, 23, 6), at(17, 0, 43));
    land_at(
        app,
        err_with(
            github::Failure::rate_limit("API rate limit exceeded"),
            Some(budget),
        ),
        at(17, 0, 43),
    );
}

#[test]
fn s2b_only_502s_since_launch_back_off_from_the_interval_without_pacing() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    let mut now = at(17, 0, 0);
    for expected in [30, 60, 120] {
        land_at(
            &mut app,
            err_with(failure(github::FailureKind::Server), None),
            now,
        );
        assert!(
            app.api_budget.known(now).is_none(),
            "a 502 made the budget known"
        );
        assert_eq!(github_poll_secs(&app, &config), 30, "paced on a guess");
        assert_eq!(next_due(&app, &config, now), expected);
        now += secs(expected);
    }
}

#[test]
fn s2c_a_known_budget_past_its_reset_is_unknown_and_the_floor_returns() {
    let (mut app, config) = budget_app(model::CiStatus::Pending, 1.0, 0);
    // 600 left with 23 minutes to go stretches the pace past 30 s.
    let budget = reading("A", 600, at(17, 23, 6), at(17, 0, 0));
    land_at(&mut app, ok_with_pending_r(Some(budget)), at(17, 0, 0));
    assert!(app.api_budget.known(at(17, 0, 0)).is_some());
    assert!(github_poll_secs(&app, &config) > 30);

    assert!(app.api_budget.known(at(17, 23, 6)).is_none());
    land_at(&mut app, ok_with_pending_r(None), at(17, 30, 0));
    assert_eq!(github_poll_secs(&app, &config), GITHUB_POLL_MIN as i64);
    assert_eq!(next_due(&app, &config, at(17, 30, 0)), 15);
}

#[test]
fn s3_a_healthy_budget_keeps_the_pending_floor() {
    let (mut app, config) = budget_app(model::CiStatus::Pending, 1.0, 0);
    let budget = reading("A", 3564, at(17, 23, 6), at(17, 0, 43));
    land_at(&mut app, ok_with_pending_r(Some(budget)), at(17, 0, 43));
    assert_eq!(github_poll_secs(&app, &config), 15);
    assert_eq!(next_due(&app, &config, at(17, 0, 43)), 15);
}

#[test]
fn s4_600_left_with_40_minutes_paces_at_72_seconds() {
    let (mut app, config) = budget_app(model::CiStatus::Pending, 1.0, 0);
    let budget = reading("A", 600, at(17, 40, 0), at(17, 0, 0));
    assert_eq!(
        app.api_budget.pace(3, at(17, 0, 0)),
        app::Pace::Unknown,
        "known before any reading"
    );
    land_at(&mut app, ok_with(Some(budget)), at(17, 0, 0));
    assert_eq!(app.api_budget.pace(3, at(17, 0, 0)), app::Pace::Every(72));
    // The Pending row would ask for 15 s: the floor is suspended.
    assert_eq!(github_poll_secs(&app, &config), 72);
    assert_eq!(next_due(&app, &config, at(17, 0, 0)), 72);
    // And so is the push shortcut.
    app.push_detected = true;
    assert!(!github_poll_due(&app, &config, at(17, 0, 1)));
    assert_eq!(next_due(&app, &config, at(17, 0, 0)), 72);
}

#[test]
fn s5_below_the_reserve_pauses_until_the_reset_plus_jitter_and_r_still_sends() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    let budget = reading("A", 480, at(17, 23, 6), at(17, 0, 0));
    land_at(&mut app, ok_with(Some(budget)), at(17, 0, 0));
    assert!(!github_poll_due(&app, &config, at(17, 23, 15)));
    assert!(github_poll_due(&app, &config, at(17, 23, 16)));
    assert_eq!(github_limit_until(&app, &config, at(17, 10, 0)), None);
}

#[test]
fn s5a_a_push_while_paused_waits_for_the_first_poll_after_the_resume() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    let budget = reading("A", 480, at(17, 23, 6), at(17, 0, 0));
    land_at(&mut app, ok_with(Some(budget)), at(17, 0, 0));
    app.push_detected = true;
    assert!(!github_poll_due(&app, &config, at(17, 10, 0)));
    assert_eq!(next_due(&app, &config, at(17, 10, 0)), 13 * 60 + 16);
    assert!(
        app.push_detected,
        "the push was dropped before a poll served it"
    );
}

#[test]
fn s5b_fewer_points_than_one_poll_is_a_known_limit_and_r_sends_nothing() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    let budget = reading("A", 2, at(17, 23, 6), at(17, 0, 0));
    land_at(&mut app, ok_with(Some(budget)), at(17, 0, 0));
    assert_eq!(
        github_limit_until(&app, &config, at(17, 10, 0)),
        Some(at(17, 23, 16))
    );
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let args = Args::parse_from(["fleetz"]);
    // Outside a runtime: a spawned request would panic here.
    refresh_key(&mut app, &tx, &args, &config, at(17, 10, 0));
    assert!(!app.github_poll_inflight, "a request was sent");
    assert_eq!(status_text(&app), "GitHub rate limited, resumes at 17:23");
}

#[test]
fn s6_a_rate_limited_body_holds_until_the_reset_plus_jitter() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    land_primary_limit(&mut app);
    assert!(matches!(
        &app.github_freshness.outcome,
        app::Outcome::Failed(f) if f.kind == github::FailureKind::Api && f.rate_limited
    ));
    assert_eq!(
        app.github_backoff.not_before(30),
        Some(at(17, 23, 16)),
        "not-before"
    );
    assert!(!github_poll_due(&app, &config, at(17, 23, 15)));
    assert!(github_poll_due(&app, &config, at(17, 23, 16)));
}

#[test]
fn s6a_a_local_clock_ten_minutes_ahead_anchors_the_reset_on_arrival() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    // The server's clock says 17:00:00 and the reset 17:23:06; the local
    // clock reads 17:10:00 when the answer arrives.
    let budget = reading("A", 0, at(17, 23, 6), at(17, 0, 0));
    land_at(
        &mut app,
        err_with(
            github::Failure::rate_limit("API rate limit exceeded"),
            Some(budget),
        ),
        at(17, 10, 0),
    );
    assert_eq!(app.api_budget.reset_at(), Some(at(17, 33, 6)));
    assert_eq!(app.github_backoff.not_before(30), Some(at(17, 33, 16)));
    assert!(!github_poll_due(&app, &config, at(17, 33, 15)));
    assert!(github_poll_due(&app, &config, at(17, 33, 16)));
}

#[test]
fn s8_a_403_with_retry_after_120_holds_120_seconds() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    land_at(&mut app, err_with(secondary(Some(120)), None), at(17, 0, 0));
    assert_eq!(next_due(&app, &config, at(17, 0, 0)), 120);
}

#[test]
fn s9_a_429_without_retry_after_holds_the_later_of_60_s_and_the_backoff() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    let mut now = at(17, 0, 0);
    // Steps 1 and 2 of the ladder (30, 60) are under the minute; step 3
    // (120) is over it.
    for expected in [60, 60, 120] {
        land_at(&mut app, err_with(secondary(None), None), now);
        assert_eq!(next_due(&app, &config, now), expected);
        now += secs(expected);
    }
}

#[test]
fn s10_six_consecutive_502s_climb_to_the_cap() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    let mut now = at(17, 0, 0);
    for expected in [30, 60, 120, 240, 480, 600] {
        land_at(
            &mut app,
            err_with(failure(github::FailureKind::Server), None),
            now,
        );
        assert_eq!(next_due(&app, &config, now), expected);
        now += secs(expected);
    }
    assert_eq!(app.github_backoff.failures(), 6);
    // The jitter applies on top of the cap: 480 to 720 there.
    assert_eq!(app::backoff_secs(6, 30, 0.8), 480);
    assert_eq!(app::backoff_secs(6, 30, 1.2), 720);
}

#[test]
fn s10a_step_one_has_no_jitter_and_step_two_does() {
    for factor in [0.8, 1.2] {
        let (mut app, config) = budget_app(model::CiStatus::Passed, factor, 0);
        land_at(
            &mut app,
            err_with(failure(github::FailureKind::Server), None),
            at(17, 0, 0),
        );
        assert_eq!(next_due(&app, &config, at(17, 0, 0)), 30, "draw {factor}");
    }
    let (mut app, config) = budget_app(model::CiStatus::Passed, 0.8, 0);
    land_at(
        &mut app,
        err_with(failure(github::FailureKind::Server), None),
        at(17, 0, 0),
    );
    land_at(
        &mut app,
        err_with(failure(github::FailureKind::Server), None),
        at(17, 0, 30),
    );
    assert_eq!(next_due(&app, &config, at(17, 0, 30)), 48);
}

#[test]
fn s11_a_success_at_step_three_resets_the_count() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    let mut now = at(17, 0, 0);
    for wait in [30, 60, 120] {
        land_at(
            &mut app,
            err_with(failure(github::FailureKind::Server), None),
            now,
        );
        now += secs(wait);
    }
    assert_eq!(app.github_backoff.failures(), 3);
    land_at(&mut app, ok_with(None), now);
    assert_eq!(app.github_backoff.failures(), 0);
    assert_eq!(app.github_backoff.not_before(30), None);
    assert_eq!(next_due(&app, &config, now), 30);
}

/// With O2 accepted `r` sends nothing under the limit, so the success
/// here is an answer applied while the secondary limit's not-before was
/// still pending, which transition 3 clears all the same.
#[test]
fn s11a_a_success_inside_a_secondary_limit_clears_it() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    land_at(
        &mut app,
        err_with(secondary(Some(120)), None),
        at(17, 10, 0),
    );
    assert_eq!(app.github_backoff.not_before(30), Some(at(17, 12, 0)));
    land_at(&mut app, ok_with(None), at(17, 10, 0));
    assert_eq!(app.github_backoff.not_before(30), None);
    assert_eq!(app.github_backoff.failures(), 0);
    assert_eq!(next_due(&app, &config, at(17, 10, 0)), 30);
}

#[test]
fn s12_three_offline_failures_each_wait_one_interval() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    let mut now = at(17, 0, 0);
    for _ in 0..3 {
        land_at(
            &mut app,
            err_with(failure(github::FailureKind::Offline), None),
            now,
        );
        assert_eq!(next_due(&app, &config, now), 30);
        now += secs(30);
    }
    assert_eq!(app.github_backoff.failures(), 0);
}

#[test]
fn s12a_offline_keeps_the_count_between_502s() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    let mut now = at(17, 0, 0);
    for (kind, expected) in [
        (github::FailureKind::Server, 30),
        (github::FailureKind::Server, 60),
        (github::FailureKind::Offline, 30),
        (github::FailureKind::Server, 120),
    ] {
        land_at(&mut app, err_with(failure(kind), None), now);
        assert_eq!(next_due(&app, &config, now), expected, "after {kind:?}");
        now += secs(expected);
    }
}

#[test]
fn s12b_an_offline_or_auth_r_during_backoff_leaves_it_in_place() {
    for interrupting in [github::FailureKind::Offline, github::FailureKind::Auth] {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        let mut now = at(17, 0, 0);
        for wait in [30, 60] {
            land_at(
                &mut app,
                err_with(failure(github::FailureKind::Server), None),
                now,
            );
            now += secs(wait);
        }
        // Step 3: not-before at +120 s.
        land_at(
            &mut app,
            err_with(failure(github::FailureKind::Server), None),
            now,
        );
        let step3 = now;
        land_at(
            &mut app,
            err_with(failure(interrupting), None),
            step3 + secs(20),
        );
        assert_eq!(app.github_backoff.failures(), 3, "{interrupting:?}");
        assert_eq!(
            next_due(&app, &config, step3),
            120,
            "{interrupting:?} moved the not-before"
        );
    }
}

#[test]
fn s13_no_token_retries_after_one_interval_and_a_token_then_polls() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    land_at(
        &mut app,
        err_with(failure(github::FailureKind::Auth), None),
        at(17, 0, 0),
    );
    assert_eq!(next_due(&app, &config, at(17, 0, 0)), 30);
    land_at(&mut app, ok_with(None), at(17, 0, 30));
    assert_eq!(app.github_freshness.outcome, app::Outcome::Success);
    assert_eq!(next_due(&app, &config, at(17, 0, 30)), 30);
}

#[test]
fn s14_three_401s_back_off_and_a_success_returns_to_normal() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    let mut now = at(17, 0, 0);
    for expected in [30, 60, 120] {
        land_at(
            &mut app,
            err_with(failure(github::FailureKind::Refused), None),
            now,
        );
        assert_eq!(next_due(&app, &config, now), expected);
        now += secs(expected);
    }
    land_at(&mut app, ok_with(None), now);
    assert_eq!(next_due(&app, &config, now), 30);
}

#[test]
fn s15_r_under_a_rate_limit_sweeps_and_sends_nothing() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    land_primary_limit(&mut app);
    let before = app.github_freshness.clone();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let args = Args::parse_from(["fleetz"]);
    // Outside a runtime: a spawned request would panic here.
    refresh_key(&mut app, &tx, &args, &config, at(17, 10, 0));
    assert!(!app.github_poll_inflight, "a request was sent");
    assert!(status_text(&app).contains("17:23"), "{}", status_text(&app));
    assert_eq!(app.github_freshness, before);
}

/// A primary limit relative to the real clock, for the paths that read
/// it (`apply_load_result`): 13 minutes left, jitter 10 s.
fn limited_now(app: &mut App) -> chrono::DateTime<chrono::Local> {
    let now = chrono::Local::now();
    let budget = reading("A", 0, now + secs(13 * 60), now);
    land_at(
        app,
        err_with(
            github::Failure::rate_limit("API rate limit exceeded"),
            Some(budget),
        ),
        now,
    );
    now + secs(13 * 60 + 10)
}

#[test]
fn s16_r_under_a_rate_limit_rebuilds_from_memory_and_names_the_resume() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    app.dataset = poll_says_passed().ok().unwrap().repos;
    let resume = limited_now(&mut app);
    assert_eq!(
        reload_action(&app, &config, chrono::Local::now()),
        ReloadAction::FromMemory
    );
    let before = app.github_freshness.clone();
    let notes = ViewNotes {
        header: "h".to_string(),
        ambiguity: None,
    };
    apply_load_result(
        &mut app,
        &config,
        Ok(Box::new(memory_rebuild_result(vec![test_repo()], notes))),
    );
    assert_eq!(app.github_freshness, before, "freshness moved");
    let status = status_text(&app);
    assert_ne!(status, "Reloaded");
    assert!(
        status.contains(&resume.format("%H:%M").to_string()),
        "{status:?}"
    );
    assert!(!app.github_poll_inflight);
    assert!(!app.loading);
}

#[test]
fn s16a_r_under_a_rate_limit_with_nothing_in_memory_only_names_the_resume() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    land_primary_limit(&mut app);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let args = Args::parse_from(["fleetz"]);
    // Outside a runtime: a spawned load would panic here.
    reload_key(
        &mut app,
        &tx,
        &args,
        &config,
        Path::new("/nonexistent"),
        at(17, 10, 0),
    );
    assert_eq!(status_text(&app), "GitHub rate limited, resumes at 17:23");
    assert!(!app.loading);
    assert!(!app.github_poll_inflight);
}

#[test]
fn s16b_r_under_a_rate_limit_with_a_poll_in_flight_waits_for_it() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    app.dataset = poll_says_passed().ok().unwrap().repos;
    land_primary_limit(&mut app);
    let in_flight = claim_github_poll(&mut app).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let args = Args::parse_from(["fleetz"]);
    reload_key(
        &mut app,
        &tx,
        &args,
        &config,
        Path::new("/nonexistent"),
        at(17, 10, 0),
    );
    assert_eq!(status_text(&app), REFRESH_RUNNING);
    assert!(!app.loading, "a rebuild started");

    let budget = reading("A", 0, at(17, 23, 6), at(17, 10, 5));
    answer_at(
        &mut app,
        in_flight,
        err_with(
            github::Failure::rate_limit("API rate limit exceeded"),
            Some(budget),
        ),
        at(17, 10, 5),
    );
    assert_eq!(
        reload_action(&app, &config, at(17, 10, 6)),
        ReloadAction::FromMemory
    );
}

#[test]
fn s17_r_during_backoff_sends_and_its_outcome_moves_the_ladder() {
    for succeeds in [false, true] {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        land_at(
            &mut app,
            err_with(failure(github::FailureKind::Server), None),
            at(17, 0, 0),
        );
        land_at(
            &mut app,
            err_with(failure(github::FailureKind::Server), None),
            at(17, 0, 30),
        );
        // Backing off, not limited: `r` goes to GitHub.
        assert_eq!(github_limit_until(&app, &config, at(17, 0, 40)), None);
        let result = match succeeds {
            true => ok_with(None),
            false => err_with(failure(github::FailureKind::Server), None),
        };
        land_at(&mut app, result, at(17, 0, 40));
        let expected = if succeeds { 0 } else { 3 };
        assert_eq!(app.github_backoff.failures(), expected);
    }
}

#[test]
fn s18_r_while_a_load_runs_is_ignored() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    app.loading = true;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let args = Args::parse_from(["fleetz"]);
    // Outside a runtime: a second load would panic here.
    reload_key(
        &mut app,
        &tx,
        &args,
        &config,
        Path::new("/nonexistent"),
        at(17, 0, 0),
    );
    assert_eq!(status_text(&app), REFRESH_RUNNING);
    assert!(!app.github_poll_inflight);
}

#[test]
fn s19b_one_failure_after_a_suspend_waits_exactly_one_interval() {
    for kind in [github::FailureKind::Offline, github::FailureKind::TimedOut] {
        // A 0.8 draw would show if step 1 were jittered.
        let (mut app, config) = budget_app(model::CiStatus::Passed, 0.8, 0);
        land_at(&mut app, ok_with(None), at(17, 0, 0));
        land_at(&mut app, err_with(failure(kind), None), at(17, 0, 30));
        assert_eq!(next_due(&app, &config, at(17, 0, 30)), 30, "{kind:?}");
        assert_eq!(github_limit_until(&app, &config, at(17, 0, 31)), None);
    }
}

/// Any sequence of outcomes and readings: the next automatic attempt is
/// never sooner than the rule 6 interval in force after the last one,
/// except for the push shortcut while healthy and unstretched.
#[test]
fn s20_no_automatic_attempt_sooner_than_the_interval_in_force() {
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut draw = |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    for _run in 0..20 {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
        app.jitter = Box::new(Planted {
            factor: 0.8 + 0.4 * (draw(100) as f64 / 99.0),
            resume: draw(31) as i64,
        });
        let mut now = at(12, 0, 0);
        for _step in 0..40 {
            let budget = match draw(3) {
                0 => None,
                _ => Some(reading(
                    "A",
                    draw(5001) as u32,
                    now + secs(draw(3600) as i64 + 1),
                    now,
                )),
            };
            let result = match draw(9) {
                0 | 1 => ok_with(budget),
                2 => err_with(failure(github::FailureKind::Server), budget),
                3 => err_with(failure(github::FailureKind::Offline), None),
                4 => err_with(failure(github::FailureKind::Auth), None),
                5 => err_with(github::Failure::rate_limit("limit"), budget),
                6 => err_with(secondary(Some(draw(200))), budget),
                7 => err_with(secondary(None), budget),
                _ => err_with(failure(github::FailureKind::Refused), budget),
            };
            land_at(&mut app, result, now);
            app.repos[0].ci_status = match draw(2) {
                0 => model::CiStatus::Pending,
                _ => model::CiStatus::Passed,
            };
            app.push_detected = draw(4) == 0;

            let failing = app.github_freshness.is_failing();
            let slowed = match app.api_budget.pace(3, now) {
                app::Pace::Unknown => false,
                app::Pace::Every(s) => s > 30,
                app::Pace::Paused { .. } => true,
            };
            let pending = app.repos[0].ci_status == model::CiStatus::Pending;
            let in_force = if !failing && !slowed && pending {
                15
            } else {
                30
            };
            let due = next_due(&app, &config, now);
            if due < in_force {
                assert!(
                    app.push_detected && !failing && !slowed,
                    "due at +{due}, under {in_force}, without the push exception"
                );
            }
            assert!(due > 0 || (app.push_detected && !failing && !slowed));
            app.push_detected = false;
            now += secs(next_due(&app, &config, now));
        }
    }
}

#[test]
fn s21_a_late_reading_never_raises_remaining_in_the_same_window() {
    let (mut app, _config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    let first = claim_github_poll(&mut app).unwrap();
    app.github_poll_started = std::time::Instant::now()
        .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
    expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, at(17, 1, 0));
    let second = claim_github_poll(&mut app).unwrap();
    let newer = reading("A", 2900, at(17, 23, 6), at(17, 1, 10));
    answer_at(&mut app, second, ok_with(Some(newer)), at(17, 1, 10));
    let applied = app.github_freshness.clone();

    let late = reading("A", 3000, at(17, 23, 6), at(17, 0, 30));
    answer_at(&mut app, first, ok_with(Some(late)), at(17, 1, 20));
    assert_eq!(app.api_budget.reading().map(|r| r.remaining), Some(2900));
    assert_eq!(app.github_freshness, applied, "the late outcome applied");
}

#[test]
fn s21a_a_late_reading_of_a_newer_window_replaces() {
    let (mut app, _config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    let first = claim_github_poll(&mut app).unwrap();
    app.github_poll_started = std::time::Instant::now()
        .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
    expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, at(17, 1, 0));
    let second = claim_github_poll(&mut app).unwrap();
    answer_at(
        &mut app,
        second,
        ok_with(Some(reading("A", 100, at(17, 23, 6), at(17, 1, 10)))),
        at(17, 1, 10),
    );
    let late = reading("A", 4990, at(18, 23, 6), at(17, 23, 30));
    answer_at(&mut app, first, ok_with(Some(late.clone())), at(17, 23, 30));
    assert_eq!(app.api_budget.reading(), Some(&late));
}

#[test]
fn s22_an_applied_reading_on_another_token_replaces_and_drops_the_limit() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    land_primary_limit(&mut app);
    assert_eq!(app.github_backoff.failures(), 1);
    assert!(github_limit_until(&app, &config, at(17, 1, 0)).is_some());
    // A body cut off mid-read is `Offline`, yet its headers came on the
    // new token: transparent to the ladder, but the switch drops A's
    // limit.
    let other = reading("B", 4000, at(17, 23, 6), at(17, 1, 0));
    land_at(
        &mut app,
        err_with(failure(github::FailureKind::Offline), Some(other.clone())),
        at(17, 1, 0),
    );
    assert_eq!(app.api_budget.reading(), Some(&other));
    assert_eq!(app.github_backoff.not_before(30), None, "A's limit kept");
    assert_eq!(app.github_backoff.failures(), 1, "the count was reset");
    assert_eq!(github_limit_until(&app, &config, at(17, 1, 1)), None);
}

#[test]
fn s22a_a_discarded_answer_on_the_old_token_undoes_nothing() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    let now = chrono::Local::now();
    let a_poll = claim_github_poll(&mut app).unwrap();
    let b = reading("B", 0, now + secs(600), now);
    apply_load_result(
        &mut app,
        &config,
        Ok(Box::new(LoadResult {
            kind: LoadKind::Reload,
            repos: Vec::new(),
            header: "h".to_string(),
            dataset: Some(Vec::new()),
            github_calendar: None,
            source: app::DatasetSource::Fallback {
                failure: github::Failure::rate_limit("API rate limit exceeded"),
                written: None,
            },
            ambiguity: None,
            meta: Some(github::FetchMeta {
                budget: Some(b.clone()),
                ..Default::default()
            }),
        })),
    );
    let held = app.api_budget.clone();
    let not_before = app.github_backoff.clone();
    assert_eq!(held.reading(), Some(&b));
    assert!(not_before.limit_until(30).is_some());

    let a = reading("A", 0, now + secs(1200), now);
    answer_at(
        &mut app,
        a_poll,
        err_with(
            github::Failure::rate_limit("API rate limit exceeded"),
            Some(a),
        ),
        now + secs(1),
    );
    assert_eq!(app.api_budget, held, "the old token came back");
    assert_eq!(app.github_backoff, not_before);
}

#[test]
fn s23_a_cache_served_launch_with_watch_off_polls_once_whatever_the_budget() {
    for limited in [false, true] {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
        app.watch_mode = false;
        launch(
            &mut app,
            LoadKind::Initial,
            app::DatasetSource::Cache {
                written: Some(at(8, 0, 0)),
            },
        );
        assert!(github_poll_due(&app, &config, at(17, 0, 0)), "not at once");
        let result = match limited {
            false => ok_with(Some(reading("A", 600, at(17, 40, 0), at(17, 0, 0)))),
            true => err_with(
                github::Failure::rate_limit("API rate limit exceeded"),
                Some(reading("A", 0, at(17, 23, 6), at(17, 0, 0))),
            ),
        };
        land_at(&mut app, result, at(17, 0, 0));
        for later in [at(17, 23, 30), at(23, 0, 0)] {
            assert!(
                !github_poll_due(&app, &config, later),
                "watch off polled again (limited: {limited})"
            );
        }
    }
}

#[test]
fn s24_two_instances_paused_together_resume_apart() {
    for jitter in [3, 21] {
        let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, jitter);
        let budget = reading("A", 400, at(17, 23, 6), at(17, 0, 0));
        land_at(&mut app, ok_with(Some(budget)), at(17, 0, 0));
        assert_eq!(
            next_due(&app, &config, at(17, 0, 0)),
            23 * 60 + 6 + jitter,
            "jitter {jitter}"
        );
    }
}

/// Design 4.1 rule 3: within one window `remaining` only falls, so the
/// lower reading is the newer one, from an applied answer or a discarded
/// one on the held token.
#[test]
fn a_lower_remaining_in_the_same_window_replaces_the_held_reading() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    let reset = at(17, 40, 0);
    land_at(
        &mut app,
        ok_with(Some(reading("A", 4000, reset, at(17, 0, 0)))),
        at(17, 0, 0),
    );
    assert_eq!(github_poll_secs(&app, &config), 30);
    land_at(
        &mut app,
        ok_with(Some(reading("A", 600, reset, at(17, 0, 0)))),
        at(17, 0, 0),
    );
    assert_eq!(app.api_budget.reading().map(|r| r.remaining), Some(600));
    assert_eq!(app.api_budget.pace(3, at(17, 0, 0)), app::Pace::Every(72));
    assert_eq!(github_poll_secs(&app, &config), 72);

    // A given-up poll answers after a newer one: discarded, but its lower
    // reading on the held token still applies.
    let first = claim_github_poll(&mut app).unwrap();
    app.github_poll_started = std::time::Instant::now()
        .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
    expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, at(17, 1, 0));
    let second = claim_github_poll(&mut app).unwrap();
    answer_at(&mut app, second, ok_with(None), at(17, 1, 10));
    let applied = app.github_freshness.clone();
    answer_at(
        &mut app,
        first,
        ok_with(Some(reading("A", 550, reset, at(17, 0, 50)))),
        at(17, 1, 20),
    );
    assert_eq!(app.api_budget.reading().map(|r| r.remaining), Some(550));
    assert_eq!(
        app.github_freshness, applied,
        "the discarded outcome applied"
    );
}

/// The pace never reaches past the resume: with one point above the
/// reserve, an hour to the reset and a cost of 3, the uncapped spacing
/// would be 10800 s.
#[test]
fn the_pace_is_capped_at_the_resume() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    land_at(
        &mut app,
        ok_with(Some(reading("A", 501, at(18, 0, 0), at(17, 0, 0)))),
        at(17, 0, 0),
    );
    let to_resume = (app.api_budget.resume_at().unwrap() - at(17, 0, 0)).num_seconds();
    assert_eq!(to_resume, 3600);
    match app.api_budget.pace(3, at(17, 0, 0)) {
        app::Pace::Every(secs) => assert!(secs <= to_resume, "paced {secs} s"),
        other => panic!("expected a pace, got {other:?}"),
    }
    assert!(github_poll_due(&app, &config, at(18, 0, 0)));
    assert_eq!(next_due(&app, &config, at(17, 0, 0)), 3600);
}

/// Transition 5: a give-up is a `TimedOut` failure and climbs the ladder.
#[test]
fn a_give_up_climbs_the_backoff_ladder() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    land_at(
        &mut app,
        err_with(failure(github::FailureKind::Server), None),
        at(17, 0, 0),
    );
    claim_github_poll(&mut app).unwrap();
    app.github_poll_started = std::time::Instant::now()
        .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
    expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, at(17, 0, 30));
    assert_eq!(app.github_backoff.failures(), 2);
    assert_eq!(next_due(&app, &config, at(17, 0, 30)), 60, "not step 2");
}

/// Transitions 3 and 6: an `R` that went to GitHub and succeeded clears
/// the ladder like a successful poll.
#[test]
fn a_successful_reload_clears_the_backoff_ladder() {
    let (mut app, _config) = budget_app(model::CiStatus::Passed, 1.0, 0);
    let mut now = at(17, 0, 0);
    for wait in [30, 60, 120, 240, 480] {
        land_at(
            &mut app,
            err_with(failure(github::FailureKind::Server), None),
            now,
        );
        now += secs(wait);
    }
    assert_eq!(app.github_backoff.failures(), 5);
    launch(&mut app, LoadKind::Reload, app::DatasetSource::Network);
    assert_eq!(app.github_backoff.failures(), 0);
    assert_eq!(app.github_backoff.not_before(30), None);
}

/// A launch refused by a rate limit with no cache to fall back on still
/// learns the reset from the refusal's headers.
#[test]
fn a_failed_load_carries_its_budget_reading() {
    let (mut app, config) = budget_app(model::CiStatus::Passed, 1.0, 10);
    let now = chrono::Local::now();
    let budget = reading("A", 0, now + secs(600), now);
    let limit = github::Failure::rate_limit("API rate limit exceeded");
    apply_load_result(
        &mut app,
        &config,
        Err(LoadError {
            message: limit.message.clone(),
            github_failure: Some(limit),
            budget: Some(Box::new(budget.clone())),
        }),
    );
    assert_eq!(app.api_budget.reading(), Some(&budget));
    let until = github_limit_until(&app, &config, now).expect("no known limit");
    assert!(until >= now + secs(610), "held only until {until}");
    assert!(
        until <= chrono::Local::now() + secs(610),
        "held until {until}"
    );
}

// --- FZ-09d: rows a poll did not cover, and an account change ---
//
// `docs/coverage-cache-design.md` 4.7 and scenarios CV9 to CV20.

/// A Primary carrier row of `owner`/`name` with CI `ci` and two PRs.
fn carrier_row(owner: &str, name: &str, ci: model::CiStatus) -> RepoStatus {
    RepoStatus {
        name: name.to_string(),
        owner: Some(owner.to_string()),
        ci_status: ci,
        ci_workflow: Some("ci".to_string()),
        pr_count: 2,
        repo_kind: model::RepoKind::Public,
        ..test_repo()
    }
}

/// The dataset entry for `owner`/`name`, with CI `ci` and no PRs.
fn entry(owner: &str, name: &str, ci: model::CiStatus) -> github::GithubRepo {
    github::GithubRepo {
        name: name.to_string(),
        pushed_at: chrono::DateTime::UNIX_EPOCH,
        pr_count: 0,
        prs: vec![],
        ci_status: ci,
        ci_workflow: Some("ci".to_string()),
        ci_duration: None,
        repo_kind: model::RepoKind::Public,
        owner: Some(owner.to_string()),
        disk_usage: None,
    }
}

/// A successful poll on `account` returning `repos` and, when given, a
/// GitHub calendar whose year totals `calendar_total`.
fn fetch_on(
    account: Option<&str>,
    repos: Vec<github::GithubRepo>,
    calendar_total: Option<u32>,
) -> Result<Box<github::GithubFetch>, github::FetchError> {
    Ok(Box::new(github::GithubFetch {
        repos,
        calendar: calendar_total.map(calendar_of),
        has_pr_counts: true,
        origin: github::Origin::Network,
        meta: github::FetchMeta {
            account: account.map(str::to_string),
            ..Default::default()
        },
    }))
}

fn calendar_of(total: u32) -> Box<contributions::Calendar> {
    let today = chrono::Local::now().date_naive();
    let mut calendar = contributions::Calendar::new(today, contributions::Source::Github);
    calendar.set(today, total);
    Box::new(calendar)
}

fn remote(owner: &str, name: &str) -> app::RemoteKey {
    (Some(owner.to_string()), name.to_string())
}

/// Rows for `alice/a` and `alice/b`, both passing, as CV9 gives them.
fn alice_app() -> App {
    App::new(
        vec![
            carrier_row("alice", "a", model::CiStatus::Passed),
            carrier_row("alice", "b", model::CiStatus::Passed),
        ],
        "h".to_string(),
        120,
        44,
    )
}

fn row<'a>(app: &'a App, name: &str) -> &'a RepoStatus {
    app.repos.iter().find(|r| r.name == name).expect("row")
}

/// Land a fetched load of `kind` on `account` with `rows` and `dataset`,
/// saving any window to `store`.
fn land_fetched(
    app: &mut App,
    kind: LoadKind,
    account: Option<&str>,
    rows: Vec<RepoStatus>,
    dataset: Vec<github::GithubRepo>,
    calendar_total: Option<u32>,
    store: &window_state::WindowState,
) {
    apply_load_result_saving_to(
        app,
        &config::Config::default(),
        Ok(Box::new(LoadResult {
            kind,
            repos: rows,
            header: "h".to_string(),
            dataset: Some(dataset),
            github_calendar: calendar_total.map(calendar_of),
            source: app::DatasetSource::Network,
            ambiguity: None,
            meta: Some(github::FetchMeta {
                account: account.map(str::to_string),
                ..Default::default()
            }),
        })),
        store,
    );
}

/// The rows assembly builds from `dataset` with nothing on disk: an
/// empty home and an empty root, no `git fetch`, as the account rebuild
/// asks `assemble_view` for them.
async fn rows_from(scratch: &Path, dataset: &[github::GithubRepo]) -> Vec<RepoStatus> {
    let home = scratch.join("home");
    let root = scratch.join("repos");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    id_assemble(&home, &[root], dataset, None).await.0
}

/// Land what the account rebuild delivers: a rebuild from memory over
/// `app.dataset`, saving any window to `store`.
async fn land_account_rebuild(app: &mut App, scratch: &Path, store: &window_state::WindowState) {
    let rows = rows_from(scratch, &app.dataset).await;
    let notes = ViewNotes {
        header: "h".to_string(),
        ambiguity: None,
    };
    let result = memory_rebuild_result(rows, notes);
    assert!(matches!(result.kind, LoadKind::FromMemory));
    assert!(result.meta.is_none() && result.dataset.is_none());
    apply_load_result_saving_to(app, &config::Config::default(), Ok(Box::new(result)), store);
}

#[test]
fn cv9_a_carrier_row_the_poll_did_not_return_is_marked_and_keeps_its_values() {
    let mut app = alice_app();
    poll_with(
        &mut app,
        fetch_on(
            None,
            vec![entry("alice", "a", model::CiStatus::Failed)],
            None,
        ),
    );
    assert!(app.not_in_last_poll.contains(&remote("alice", "b")));
    assert!(!app.not_in_last_poll.contains(&remote("alice", "a")));
    let b = row(&app, "b");
    assert_eq!(b.ci_status, model::CiStatus::Passed, "b's CI was cleared");
    assert_eq!(b.pr_count, 2, "b's PRs were cleared");
    assert!(app.is_not_in_last_poll(b));
    assert_eq!(row(&app, "a").ci_status, model::CiStatus::Failed);

    poll_with(
        &mut app,
        fetch_on(
            None,
            vec![
                entry("alice", "a", model::CiStatus::Failed),
                entry("alice", "b", model::CiStatus::Passed),
            ],
            None,
        ),
    );
    assert!(
        app.not_in_last_poll.is_empty(),
        "{:?}",
        app.not_in_last_poll
    );
}

#[test]
fn cv10_a_failed_poll_leaves_the_not_covered_set_unchanged() {
    let mut app = alice_app();
    poll_with(
        &mut app,
        fetch_on(
            None,
            vec![entry("alice", "a", model::CiStatus::Passed)],
            None,
        ),
    );
    let before = app.not_in_last_poll.clone();
    assert!(!before.is_empty());
    poll(&mut app, Err("HTTP 502".to_string()));
    assert_eq!(app.not_in_last_poll, before);
}

#[test]
fn cv11_a_pending_row_the_poll_did_not_cover_does_not_hold_the_floor() {
    let config = config::Config {
        github_poll_interval: 90,
        ..config::Config::default()
    };
    let mut app = App::new(
        vec![
            carrier_row("alice", "a", model::CiStatus::Passed),
            carrier_row("alice", "b", model::CiStatus::Pending),
        ],
        "h".to_string(),
        120,
        44,
    );
    app.watch_mode = true;
    assert_eq!(github_poll_secs(&app, &config), GITHUB_POLL_MIN as i64);
    poll_with(
        &mut app,
        fetch_on(
            None,
            vec![entry("alice", "a", model::CiStatus::Passed)],
            None,
        ),
    );
    assert_eq!(row(&app, "b").ci_status, model::CiStatus::Pending);
    assert_eq!(github_poll_secs(&app, &config), 90);
}

#[tokio::test]
async fn cv12_a_poll_on_another_account_rebuilds_the_rows_from_its_dataset() {
    let scratch = wt_scratch("cv12");
    let store = window_state::WindowState::at(scratch.join("window.toml"));
    let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
    let a_dataset = vec![
        entry("alice", "a", model::CiStatus::Passed),
        entry("alice", "b", model::CiStatus::Failed),
    ];
    let a_rows = rows_from(&scratch, &a_dataset).await;
    land_fetched(
        &mut app,
        LoadKind::Initial,
        Some("A"),
        a_rows,
        a_dataset,
        Some(3),
        &store,
    );
    assert_eq!(app.repos.len(), 2);
    let attempts_before = app.github_freshness.clone();

    poll_with(
        &mut app,
        fetch_on(
            Some("B"),
            vec![entry("bob", "x", model::CiStatus::Pending)],
            Some(11),
        ),
    );
    assert!(app.account_rebuild_due, "no rebuild asked for");
    // The poll itself marks A's rows, until the rebuild lands.
    assert!(app.not_in_last_poll.contains(&remote("alice", "a")));
    let after_poll = app.github_freshness.clone();
    assert_ne!(after_poll, attempts_before);

    land_account_rebuild(&mut app, &scratch, &store).await;
    let names: Vec<_> = app
        .repos
        .iter()
        .map(|r| (r.owner.clone(), r.name.clone(), r.ci_status.clone()))
        .collect();
    assert_eq!(
        names,
        vec![(
            Some("bob".to_string()),
            "x".to_string(),
            model::CiStatus::Pending
        )],
        "a row kept A-only data"
    );
    assert!(app.not_in_last_poll.is_empty());
    assert_eq!(
        app.github_calendar.as_ref().map(|c| c.total()),
        Some(11),
        "B's calendar is not shown"
    );
    // No request: freshness is what the poll left. No window written,
    // no "Window:" status.
    assert_eq!(app.github_freshness, after_poll);
    assert!(!scratch.join("window.toml").exists(), "window.toml written");
    let status = status_text(&app);
    assert!(!status.starts_with("Window:"), "{status}");
    std::fs::remove_dir_all(&scratch).ok();
}

#[test]
fn cv13_a_late_answer_on_the_old_account_triggers_no_rebuild() {
    let scratch = wt_scratch("cv13");
    let store = window_state::WindowState::at(scratch.join("window.toml"));
    let mut app = alice_app();
    land_fetched(
        &mut app,
        LoadKind::Initial,
        Some("A"),
        alice_app().repos,
        vec![entry("alice", "a", model::CiStatus::Passed)],
        None,
        &store,
    );
    let first = claim_github_poll(&mut app).unwrap();
    app.github_poll_started = std::time::Instant::now()
        .checked_sub(GITHUB_POLL_STUCK_AFTER + std::time::Duration::from_secs(1));
    expire_stuck_github_poll(&mut app, GITHUB_POLL_STUCK_AFTER, chrono::Local::now());
    let second = claim_github_poll(&mut app).unwrap();
    answer(
        &mut app,
        second,
        fetch_on(
            Some("B"),
            vec![entry("bob", "x", model::CiStatus::Passed)],
            None,
        ),
    );
    assert!(app.account_rebuild_due);
    // As `start_account_rebuild` takes the flag when it spawns.
    app.account_rebuild_due = false;

    answer(
        &mut app,
        first,
        fetch_on(
            Some("A"),
            vec![entry("alice", "a", model::CiStatus::Failed)],
            None,
        ),
    );
    assert!(!app.account_rebuild_due, "the discarded answer rebuilt");
    assert_eq!(app.github_meta.account.as_deref(), Some("B"));
    std::fs::remove_dir_all(&scratch).ok();
}

#[tokio::test]
async fn cv15_every_rebuild_from_one_dataset_clears_the_not_covered_set() {
    let scratch = wt_scratch("cv15");
    let store = window_state::WindowState::at(scratch.join("window.toml"));
    let alice = || {
        vec![
            entry("alice", "a", model::CiStatus::Passed),
            entry("alice", "b", model::CiStatus::Passed),
        ]
    };
    let marked = || {
        let mut app = alice_app();
        app.dataset = alice();
        poll_with(
            &mut app,
            fetch_on(
                None,
                vec![entry("alice", "a", model::CiStatus::Passed)],
                None,
            ),
        );
        assert!(app.not_in_last_poll.contains(&remote("alice", "b")));
        app
    };
    let notes = || ViewNotes {
        header: "h".to_string(),
        ambiguity: None,
    };
    let config = config::Config::default();

    let mut app = marked();
    land_fetched(
        &mut app,
        LoadKind::Initial,
        None,
        alice_app().repos,
        alice(),
        None,
        &store,
    );
    assert!(app.not_in_last_poll.is_empty(), "after a launch");

    let mut app = marked();
    land_fetched(
        &mut app,
        LoadKind::Reload,
        None,
        alice_app().repos,
        alice(),
        None,
        &store,
    );
    assert!(app.not_in_last_poll.is_empty(), "after R");

    let mut app = marked();
    let w = time::TimeWindow::clamped(2, time::WindowUnit::Days);
    let result = window_apply_result(w, alice_app().repos, notes());
    apply_load_result_saving_to(&mut app, &config, Ok(Box::new(result)), &store);
    assert!(app.not_in_last_poll.is_empty(), "after a window apply");

    let mut app = marked();
    let result = memory_rebuild_result(alice_app().repos, notes());
    apply_load_result_saving_to(&mut app, &config, Ok(Box::new(result)), &store);
    assert!(
        app.not_in_last_poll.is_empty(),
        "after a rebuild from memory"
    );

    let mut app = marked();
    poll_with(
        &mut app,
        fetch_on(
            Some("B"),
            vec![entry("bob", "x", model::CiStatus::Passed)],
            None,
        ),
    );
    assert!(app.account_rebuild_due);
    assert!(!app.not_in_last_poll.is_empty());
    land_account_rebuild(&mut app, &scratch, &store).await;
    assert!(app.not_in_last_poll.is_empty(), "after an account rebuild");
    std::fs::remove_dir_all(&scratch).ok();
}

#[tokio::test]
async fn cv16_a_launch_on_an_unknown_account_rebuilds_once_on_the_first_keyed_poll() {
    let scratch = wt_scratch("cv16");
    let store = window_state::WindowState::at(scratch.join("window.toml"));
    let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
    // Served from the legacy file, or under the signed-out rule: the
    // load's account is unknown.
    apply_load_result_saving_to(
        &mut app,
        &config::Config::default(),
        Ok(Box::new(LoadResult {
            kind: LoadKind::Initial,
            repos: alice_app().repos,
            header: "h".to_string(),
            dataset: Some(vec![entry("alice", "a", model::CiStatus::Passed)]),
            github_calendar: None,
            source: app::DatasetSource::Cache { written: None },
            ambiguity: None,
            meta: Some(github::FetchMeta::default()),
        })),
        &store,
    );
    assert_eq!(app.github_meta.account, None);
    let a_poll = || {
        fetch_on(
            Some("A"),
            vec![entry("alice", "a", model::CiStatus::Failed)],
            None,
        )
    };

    poll_with(&mut app, a_poll());
    assert!(
        app.account_rebuild_due,
        "the first keyed poll did not rebuild"
    );
    app.account_rebuild_due = false;
    land_account_rebuild(&mut app, &scratch, &store).await;
    let rows: Vec<_> = app.repos.iter().map(|r| r.name.clone()).collect();
    assert_eq!(rows, vec!["a".to_string()], "not rebuilt from A's dataset");

    poll_with(&mut app, a_poll());
    assert!(!app.account_rebuild_due, "a second rebuild");
    std::fs::remove_dir_all(&scratch).ok();
}

#[tokio::test]
async fn cv17_a_rotated_token_on_one_account_rebuilds_once() {
    let scratch = wt_scratch("cv17");
    let store = window_state::WindowState::at(scratch.join("window.toml"));
    let dataset = vec![
        entry("alice", "a", model::CiStatus::Passed),
        entry("alice", "b", model::CiStatus::Passed),
    ];
    let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
    let rows = rows_from(&scratch, &dataset).await;
    land_fetched(
        &mut app,
        LoadKind::Initial,
        Some("F1"),
        rows,
        dataset,
        None,
        &store,
    );

    let polled = vec![
        entry("alice", "a", model::CiStatus::Failed),
        entry("alice", "c", model::CiStatus::Passed),
    ];
    poll_with(&mut app, fetch_on(Some("F2"), polled.clone(), None));
    assert!(app.account_rebuild_due, "no rebuild on a new fingerprint");
    app.account_rebuild_due = false;
    land_account_rebuild(&mut app, &scratch, &store).await;

    let shape = |rows: &[RepoStatus]| -> Vec<_> {
        rows.iter()
            .map(|r| {
                (
                    r.owner.clone(),
                    r.name.clone(),
                    r.ci_status.clone(),
                    r.location.clone(),
                )
            })
            .collect()
    };
    let alone = rows_from(&scratch, &polled).await;
    assert_eq!(shape(&app.repos), shape(&alone));

    poll_with(&mut app, fetch_on(Some("F2"), polled, None));
    assert!(!app.account_rebuild_due, "a second rebuild on the same key");
    std::fs::remove_dir_all(&scratch).ok();
}

#[test]
fn cv18_a_poll_with_no_account_key_never_rebuilds() {
    let scratch = wt_scratch("cv18");
    let store = window_state::WindowState::at(scratch.join("window.toml"));
    // Never had a load, and had one whose account is unknown.
    let mut never_loaded = alice_app();
    let mut unknown = alice_app();
    land_fetched(
        &mut unknown,
        LoadKind::Initial,
        None,
        alice_app().repos,
        vec![],
        None,
        &store,
    );
    for app in [&mut never_loaded, &mut unknown] {
        let before = app.repos.clone();
        poll_with(
            app,
            fetch_on(
                None,
                vec![entry("alice", "a", model::CiStatus::Failed)],
                None,
            ),
        );
        assert!(!app.account_rebuild_due, "a keyless poll rebuilt");
        assert_eq!(row(app, "a").ci_status, model::CiStatus::Failed);
        assert_eq!(row(app, "a").pr_count, 0);
        let b = row(app, "b");
        let b_before = before.iter().find(|r| r.name == "b").unwrap();
        assert_eq!(b.ci_status, b_before.ci_status);
        assert_eq!(b.pr_count, b_before.pr_count);
        assert_eq!(b.repo_kind, b_before.repo_kind);
        assert_eq!(app.repos.len(), 2);
    }
    std::fs::remove_dir_all(&scratch).ok();
}

#[test]
fn cv20_a_window_apply_keeps_the_account_so_a_poll_on_it_does_not_rebuild() {
    let scratch = wt_scratch("cv20");
    let store = window_state::WindowState::at(scratch.join("window.toml"));
    let mut app = App::new(Vec::new(), "h".to_string(), 120, 44);
    land_fetched(
        &mut app,
        LoadKind::Initial,
        Some("A"),
        alice_app().repos,
        vec![entry("alice", "a", model::CiStatus::Passed)],
        None,
        &store,
    );
    let w = time::TimeWindow::clamped(2, time::WindowUnit::Days);
    let notes = ViewNotes {
        header: "h".to_string(),
        ambiguity: None,
    };
    apply_load_result_saving_to(
        &mut app,
        &config::Config::default(),
        Ok(Box::new(window_apply_result(w, alice_app().repos, notes))),
        &store,
    );
    assert_eq!(app.github_meta.account.as_deref(), Some("A"));
    poll_with(
        &mut app,
        fetch_on(
            Some("A"),
            vec![entry("alice", "a", model::CiStatus::Passed)],
            None,
        ),
    );
    assert!(
        !app.account_rebuild_due,
        "a poll on the same account rebuilt"
    );
    std::fs::remove_dir_all(&scratch).ok();
}

/// The rebuild waits while a load runs: outside a runtime a spawn would
/// panic, so returning at all says nothing was spawned.
#[test]
fn an_account_rebuild_waits_for_a_running_load() {
    let mut app = alice_app();
    app.loading = true;
    app.account_rebuild_due = true;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let args = Args::parse_from(["fleetz"]);
    start_account_rebuild(
        &mut app,
        &tx,
        &args,
        &config::Config::default(),
        Path::new("/nonexistent"),
    );
    assert!(app.account_rebuild_due, "the waiting rebuild was dropped");
}

/// A load that fetched rebuilt the rows on its own account, so a pending
/// account rebuild is dropped rather than repeated.
#[test]
fn a_fetched_load_settles_a_pending_account_rebuild() {
    let scratch = wt_scratch("fz09d_settle");
    let store = window_state::WindowState::at(scratch.join("window.toml"));
    let mut app = alice_app();
    app.account_rebuild_due = true;
    land_fetched(
        &mut app,
        LoadKind::Reload,
        Some("B"),
        vec![],
        vec![],
        None,
        &store,
    );
    assert!(!app.account_rebuild_due);
    std::fs::remove_dir_all(&scratch).ok();
}

#[test]
fn the_rebuild_rule_needs_a_known_poll_account() {
    assert!(!account_rebuild_needed(None, None));
    assert!(!account_rebuild_needed(Some("A"), None));
    assert!(!account_rebuild_needed(Some("A"), Some("A")));
    assert!(account_rebuild_needed(Some("A"), Some("B")));
    assert!(account_rebuild_needed(None, Some("A")));
}

#[test]
fn names_remote_matches_case_insensitively_by_owner_or_by_name_alone() {
    let entry = github::GithubRepo {
        owner: Some("gaurgle".to_string()),
        ..gh("fleetz", "2026-01-01T00:00:00Z")
    };
    assert!(names_remote(Some("GAURGLE"), "Fleetz", &entry));
    assert!(!names_remote(Some("someone"), "fleetz", &entry));
    assert!(!names_remote(Some("gaurgle"), "other", &entry));
    // No owner: the legacy path, the name alone decides.
    assert!(names_remote(None, "FLEETZ", &entry));
    assert!(!names_remote(None, "other", &entry));
    // An owned row never matches an entry without an owner.
    let legacy = gh("fleetz", "2026-01-01T00:00:00Z");
    assert!(!names_remote(Some("gaurgle"), "fleetz", &legacy));
    assert!(names_remote(None, "Fleetz", &legacy));
}

// FZ-37: `y` and a click in pane 2 copy a commit hash.

const YANK_HASHES: [&str; 3] = ["aaa1111", "bbb2222", "ccc3333"];

/// A repo whose every view lists the three `YANK_HASHES` commits, plus
/// `extra` more, newest first.
fn yank_repo(extra: usize) -> RepoStatus {
    let mut repo = test_repo();
    repo.path = Some("/tmp/yank".to_string());
    let hashes: Vec<String> = YANK_HASHES
        .iter()
        .map(|h| h.to_string())
        .chain((0..extra).map(|i| format!("{:07x}", 0xd000000 + i)))
        .collect();
    repo.graph = hashes
        .iter()
        .map(|h| model::GraphLine {
            lanes: vec![graph::commit_cell()],
            hash: h.clone(),
            is_merge: false,
            refs: vec![],
            author: "Gaurgle".to_string(),
            message: format!("subject of {h}"),
        })
        .collect();
    repo.reflog = hashes
        .iter()
        .map(|h| model::ReflogEntry {
            hash: h.clone(),
            action: "commit:".to_string(),
            description: format!("subject of {h}"),
        })
        .collect();
    repo.graph_expanded = hashes
        .iter()
        .flat_map(|h| {
            [
                format!("* commit {h}"),
                "| Author: Gaurgle <g@example.com>".to_string(),
                "|".to_string(),
                format!("|     subject of {h}"),
            ]
        })
        .collect();
    repo
}

fn yank_app(view: app::HistoryView, extra: usize) -> App {
    let mut app = App::new(vec![yank_repo(extra)], "h".to_string(), 120, 40);
    app.view = app::View::Detail;
    app.history_view = view;
    app
}

/// Draw a frame, as the event loop does before any click can arrive.
fn yank_frame(app: &App) -> ratatui::buffer::Buffer {
    use ratatui::backend::TestBackend;
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| ui::render(f, app)).unwrap();
    term.backend().buffer().clone()
}

/// The first cell of `needle` inside the detail pane, read off the
/// screen rather than out of the hit map under test.
fn on_screen(buf: &ratatui::buffer::Buffer, app: &App, needle: &str) -> (u16, u16) {
    let pane = ui::panel_rects(buf.area, app).detail.expect("detail pane");
    (pane.top()..pane.bottom())
        .find_map(|y| {
            let row: String = (pane.left()..pane.right())
                .map(|x| buf[(x, y)].symbol())
                .collect();
            row.find(needle)
                .map(|byte| (pane.left() + row[..byte].chars().count() as u16, y))
        })
        .unwrap_or_else(|| panic!("`{needle}` is not drawn in pane 2"))
}

fn yank_status(app: &App) -> Option<(String, bool)> {
    app.status_message
        .as_ref()
        .map(|m| (m.text.clone(), m.is_error))
}

#[test]
fn yank_copies_the_inspected_hash() {
    let mut app = yank_app(app::HistoryView::Commits, 0);
    app.set_detail_selected(1);
    let mut copied = Vec::new();
    app.yank_with(|h| {
        copied.push(h.to_string());
        Ok(())
    });
    assert_eq!(copied, ["bbb2222"]);
    assert_eq!(
        yank_status(&app),
        Some(("copied bbb2222".to_string(), false))
    );
}

#[test]
fn yank_follows_the_active_view() {
    let mut app = yank_app(app::HistoryView::Reflog, 0);
    app.set_detail_selected(2);
    let mut copied = String::new();
    app.yank_with(|h| {
        copied = h.to_string();
        Ok(())
    });
    assert_eq!(copied, "ccc3333");
}

#[test]
fn yank_with_nothing_inspected_copies_nothing() {
    let mut app = App::new(vec![test_repo()], "h".to_string(), 120, 40);
    let mut called = false;
    app.yank_with(|_| {
        called = true;
        Ok(())
    });
    assert!(!called, "the clipboard was asked to copy nothing");
    assert_eq!(
        yank_status(&app),
        Some(("nothing to copy".to_string(), false))
    );
}

#[test]
fn a_failed_copy_names_the_reason() {
    let mut app = yank_app(app::HistoryView::Commits, 0);
    app.yank_with(|_| Err("pbcopy timed out".to_string()));
    assert_eq!(
        yank_status(&app),
        Some(("copy failed: pbcopy timed out".to_string(), true))
    );
}

// FZ-74: `yy` and `yp` copy the thing under the focused cursor and its
// path or address.

/// Press `keys` one by one through the yank chord with a stand-in
/// clipboard, and return what was copied.
fn chord(app: &mut App, keys: &str) -> Vec<String> {
    let mut copied = Vec::new();
    for c in keys.chars() {
        app.press_yank_key(Some(c), |text| {
            copied.push(text.to_string());
            Ok(())
        });
    }
    copied
}

/// `yank_app`'s repo as GitHub's `Gaurgle/r`, focused on `panel`.
fn yank_on(view: app::HistoryView, panel: FocusedPanel) -> App {
    let mut app = yank_app(view, 0);
    app.repos[0].owner = Some("Gaurgle".to_string());
    app.focused_panel = panel;
    app
}

#[test]
fn yy_on_a_cloned_repo_row_copies_its_github_url() {
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    assert_eq!(chord(&mut app, "yy"), ["https://github.com/Gaurgle/r"]);
    assert_eq!(yank_status(&app), Some(("copied url".to_string(), false)));
}

#[test]
fn yy_on_a_local_row_reads_the_url_off_a_github_origin() {
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    app.repos[0].owner = None;
    app.repos[0].remote_url = Some("git@github.com:acme/tool.git".to_string());
    assert_eq!(chord(&mut app, "yy"), ["https://github.com/acme/tool"]);
    app.repos[0].remote_url = Some("git@gitlab.com:acme/tool.git".to_string());
    assert!(chord(&mut app, "yy").is_empty(), "not a GitHub remote");
    assert_eq!(
        yank_status(&app),
        Some(("nothing to copy".to_string(), false))
    );
}

#[test]
fn yp_on_a_cloned_repo_row_copies_its_expanded_path() {
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    app.repos[0].path = Some("~/repos/yank".to_string());
    let home = shellexpand::tilde("~/repos/yank").into_owned();
    assert!(!home.starts_with('~'), "HOME is set in the test run");
    assert_eq!(chord(&mut app, "yp"), [home]);
    assert_eq!(yank_status(&app), Some(("copied path".to_string(), false)));
}

#[test]
fn a_not_cloned_row_copies_its_url_and_has_no_path() {
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    app.repos[0].location = model::RepoLocation::NotCloned;
    app.repos[0].path = None;
    assert_eq!(chord(&mut app, "yy"), ["https://github.com/Gaurgle/r"]);
    assert!(chord(&mut app, "yp").is_empty(), "no path to copy");
    assert_eq!(
        yank_status(&app),
        Some(("nothing to copy".to_string(), false))
    );
}

/// Plain `y` on a commit list copies the hash at once, as it always
/// did, and arms the chord: a second `y` copies it again.
#[test]
fn plain_y_on_commits_still_copies_the_hash() {
    let mut app = yank_on(app::HistoryView::Commits, FocusedPanel::Detail);
    app.set_detail_selected(1);
    assert_eq!(chord(&mut app, "y"), ["bbb2222"]);
    assert_eq!(
        yank_status(&app),
        Some(("copied bbb2222".to_string(), false))
    );
    assert_eq!(chord(&mut app, "y"), ["bbb2222"], "yy copies it again");
    assert!(!app.pending_yank, "yy finished the chord");
}

#[test]
fn yy_and_yp_on_every_commit_list_copy_the_hash_and_its_url() {
    for view in [
        app::HistoryView::Info,
        app::HistoryView::Commits,
        app::HistoryView::Reflog,
        app::HistoryView::Graph,
    ] {
        let mut app = yank_on(view, FocusedPanel::Detail);
        app.set_detail_selected(2);
        assert_eq!(chord(&mut app, "yy"), ["ccc3333", "ccc3333"], "{view:?}");
        assert_eq!(
            yank_status(&app),
            Some(("copied ccc3333".to_string(), false))
        );
        assert_eq!(
            chord(&mut app, "yp"),
            ["ccc3333", "https://github.com/Gaurgle/r/commit/ccc3333"],
            "{view:?}"
        );
        assert_eq!(yank_status(&app), Some(("copied url".to_string(), false)));
    }
}

#[test]
fn the_branches_view_copies_the_name_and_its_tree_url() {
    let mut app = yank_on(app::HistoryView::Branches, FocusedPanel::Detail);
    app.repos[0].branches = vec![model::BranchStatus {
        name: "main".to_string(),
        upstream: None,
        behind: 0,
        ahead: 0,
        is_head: true,
        tip: "aaa1111".to_string(),
    }];
    app.repos[0].remote_branches = vec![model::RemoteBranch {
        name: "origin/feat/x".to_string(),
        tip: "ddd4444".to_string(),
    }];
    assert_eq!(chord(&mut app, "yy"), ["main"]);
    assert_eq!(yank_status(&app), Some(("copied main".to_string(), false)));
    assert_eq!(
        chord(&mut app, "yp"),
        ["https://github.com/Gaurgle/r/tree/main"]
    );
    app.set_detail_selected(1);
    assert_eq!(chord(&mut app, "yy"), ["origin/feat/x"]);
    assert_eq!(
        chord(&mut app, "yp"),
        ["https://github.com/Gaurgle/r/tree/feat/x"],
        "without the remote prefix"
    );
    assert_eq!(yank_status(&app), Some(("copied url".to_string(), false)));
}

/// FZ-77 on FZ-74: in `worktrees`, `yy` is the highlighted worktree's
/// branch and `yp` its absolute path, the main checkout first; a
/// detached one has no branch to copy.
#[test]
fn the_worktrees_view_copies_the_branch_and_its_absolute_path() {
    let mut app = yank_on(app::HistoryView::Worktrees, FocusedPanel::Detail);
    app.repos[0].branches = vec![model::BranchStatus {
        name: "main".to_string(),
        upstream: None,
        behind: 0,
        ahead: 0,
        is_head: true,
        tip: "aaa1111".to_string(),
    }];
    app.repos[0].worktrees = vec![
        model::Worktree {
            path: "~/wt/side".to_string(),
            branch: "side".to_string(),
            head: "bbb2222".to_string(),
            ..Default::default()
        },
        model::Worktree {
            path: "/tmp/wt-detached".to_string(),
            detached: true,
            head: "ccc3333".to_string(),
            ..Default::default()
        },
    ];
    assert_eq!(chord(&mut app, "yy"), ["main"]);
    assert_eq!(yank_status(&app), Some(("copied main".to_string(), false)));
    assert_eq!(chord(&mut app, "yp"), ["/tmp/yank"]);
    app.set_detail_selected(1);
    assert_eq!(chord(&mut app, "yy"), ["side"]);
    assert_eq!(
        chord(&mut app, "yp"),
        [shellexpand::tilde("~/wt/side").into_owned()],
        "the `~` expanded"
    );
    assert_eq!(yank_status(&app), Some(("copied path".to_string(), false)));
    app.set_detail_selected(2);
    assert!(chord(&mut app, "yy").is_empty(), "detached: no branch");
    assert_eq!(
        yank_status(&app),
        Some(("nothing to copy".to_string(), false))
    );
    assert_eq!(chord(&mut app, "yp"), ["/tmp/wt-detached"]);
}

/// `ci_app` on the `ci` view holding the fixture runs, the first
/// highlighted, focused on `panel`.
fn ci_yank_app(panel: FocusedPanel) -> App {
    let mut app = ci_app();
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Ci;
    app.inspector_folded = false;
    app.focused_panel = panel;
    app.ci.insert(
        ci_key(),
        app::CiState {
            runs: ci_runs(),
            ..Default::default()
        },
    );
    app
}

#[test]
fn the_ci_view_copies_the_run_url_and_its_id() {
    let mut app = ci_yank_app(FocusedPanel::Detail);
    let run = ci_runs()[0].clone();
    assert_eq!(chord(&mut app, "yy"), [run.url.as_str()]);
    assert_eq!(yank_status(&app), Some(("copied url".to_string(), false)));
    assert_eq!(chord(&mut app, "yp"), ["39590684061"]);
    assert_eq!(
        yank_status(&app),
        Some(("copied 39590684061".to_string(), false))
    );
}

#[test]
fn the_inspector_on_a_run_copies_the_shown_body_and_the_run_url() {
    let mut app = ci_yank_app(FocusedPanel::Inspector);
    let run = ci_runs()[0].clone();
    let sha = run.head_sha.clone().expect("fixture sha");
    app.ci.get_mut(&ci_key()).unwrap().runs[0].path = Some(".github/workflows/ci.yml".to_string());
    app.ci.get_mut(&ci_key()).unwrap().scripts.insert(
        (sha, ".github/workflows/ci.yml".to_string()),
        ci::Detail::Ready("on: push\njobs: {}\n".to_string()),
    );
    app.ci_body = app::CiBody::Script;
    assert_eq!(chord(&mut app, "yy"), ["on: push\njobs: {}\n"]);
    assert_eq!(yank_status(&app), Some(("copied 18 B".to_string(), false)));

    let jobs = ci::parse_jobs(include_str!("ci/fixtures/jobs-failed.json")).unwrap();
    let job = jobs[0].id;
    let state = app.ci.get_mut(&ci_key()).unwrap();
    state.jobs.insert(run.id, app::CiJobs { etag: None, jobs });
    state.full_logs.insert(
        job,
        ci::Detail::Ready(vec!["first".to_string(), "##[error]boom".to_string()]),
    );
    state
        .logs
        .insert(job, ci::LogTail::Lines(vec!["tail".to_string()]));
    app.ci_body = app::CiBody::Log;
    assert_eq!(chord(&mut app, "yy"), ["first\n##[error]boom"]);
    app.ci_body = app::CiBody::Jobs;
    assert_eq!(chord(&mut app, "yy"), ["tail"], "the marked job's tail");

    assert_eq!(chord(&mut app, "yp"), [run.url]);
    assert_eq!(yank_status(&app), Some(("copied url".to_string(), false)));
}

#[test]
fn the_inspector_on_a_commit_copies_its_message_and_its_hash() {
    let mut app = yank_on(app::HistoryView::Commits, FocusedPanel::Inspector);
    app.set_detail_selected(1);
    app.repos[0].commits = vec![model::Commit {
        hash: "bbb2222".to_string(),
        message: "fix: the subject".to_string(),
        body: "Why it changed.\n".to_string(),
        author: "Gaurgle".to_string(),
        date: None,
        insertions: 0,
        deletions: 0,
        files: vec![],
    }];
    assert_eq!(
        chord(&mut app, "yy"),
        ["fix: the subject\n\nWhy it changed."]
    );
    assert_eq!(yank_status(&app), Some(("copied 33 B".to_string(), false)));
    assert_eq!(chord(&mut app, "yp"), ["bbb2222"]);
    assert_eq!(
        yank_status(&app),
        Some(("copied bbb2222".to_string(), false))
    );
}

/// The tree, the reader and the `[3] file` card all copy the highlighted
/// file's content, capped as the reader caps it, and its path.
#[test]
fn the_notes_view_copies_the_file_and_its_path() {
    let scratch = std::env::temp_dir().join(format!("fleetz_yank_notes_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(scratch.join("notez")).unwrap();
    let file = scratch.join("notez/a.md");
    std::fs::write(&file, "x".repeat(4300)).unwrap();
    let root = scratch.to_string_lossy().into_owned();
    let mut app = yank_on(app::HistoryView::Notes, FocusedPanel::Detail);
    app.repos[0].path = Some(root.clone());
    app.inspector_folded = false;
    let walk = app::NotesWalk {
        key: root.clone(),
        root: scratch.clone(),
        generation: app.notes_generation,
    };
    let files = vec![notes::NoteFile {
        rel: "notez/a.md".into(),
        notez: Some("notez".into()),
    }];
    app.apply_notes_walk(walk, Ok(notes::Notes { files }));
    for panel in [
        FocusedPanel::Detail,
        FocusedPanel::Reader,
        FocusedPanel::Inspector,
    ] {
        app.focused_panel = panel;
        assert_eq!(chord(&mut app, "yy"), ["x".repeat(4300)], "{panel:?}");
        assert_eq!(
            yank_status(&app),
            Some(("copied 4.2 KB".to_string(), false)),
            "{panel:?}"
        );
        assert_eq!(
            chord(&mut app, "yp"),
            [file.to_string_lossy().into_owned()],
            "{panel:?}"
        );
        assert_eq!(yank_status(&app), Some(("copied path".to_string(), false)));
    }

    std::fs::write(&file, "y".repeat(notes::BODY_CAP + 10)).unwrap();
    app.focused_panel = FocusedPanel::Detail;
    let copied = chord(&mut app, "yy");
    assert_eq!(
        copied,
        ["y".repeat(notes::BODY_CAP)],
        "cut at the cap, no marker"
    );
    assert_eq!(
        yank_status(&app),
        Some(("copied 256.0 KB".to_string(), false))
    );
    let _ = std::fs::remove_dir_all(&scratch);
}

/// `y` and then a key that is not `y` or `p` copies nothing, leaves the
/// status line as it was and hands the key back to its own action.
#[test]
fn y_then_another_key_copies_nothing() {
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    let mut called = false;
    assert!(app.press_yank_key(Some('y'), |_| {
        called = true;
        Ok(())
    }));
    assert!(!app.press_yank_key(Some('j'), |_| {
        called = true;
        Ok(())
    }));
    assert!(!called, "the clipboard was asked to copy");
    assert_eq!(yank_status(&app), None, "the status line moved");
    assert!(!app.pending_yank, "the chord outlived the key after it");
    assert!(!app.press_yank_key(Some('p'), |_| {
        called = true;
        Ok(())
    }));
    assert!(!called, "a lone `p` is not a yank");
    assert!(!app.press_yank_key(None, |_| Ok(())));
}

#[test]
fn the_strip_and_the_compact_grid_copy_nothing() {
    let mut app = yank_on(app::HistoryView::Commits, FocusedPanel::Contributions);
    assert!(chord(&mut app, "yy").is_empty());
    assert_eq!(
        yank_status(&app),
        Some(("nothing to copy".to_string(), false))
    );
    let mut app = yank_on(app::HistoryView::Commits, FocusedPanel::RepoList);
    app.view = app::View::Compact;
    assert!(chord(&mut app, "yy").is_empty());
    assert!(chord(&mut app, "yp").is_empty());
}

/// A LocalOnly carrier's url names the repo its origin names, not the
/// folder it was cloned into.
#[test]
fn a_local_carrier_is_named_by_its_origin_not_its_folder() {
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    app.repos[0].location = model::RepoLocation::LocalOnly;
    app.repos[0].name = "my-folder".to_string();
    app.repos[0].remote_url = Some("git@github.com:Gaurgle/real-name.git".to_string());
    assert_eq!(
        chord(&mut app, "yy"),
        ["https://github.com/Gaurgle/real-name"]
    );
}

// FZ-81: a successful `yy` or `yp` flashes the row it copied from.

#[test]
fn yy_and_yp_start_a_flash_sweeping_their_way() {
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    let (before, ms) = (std::time::Instant::now(), std::time::Duration::from_millis);
    assert!(!app.yank_flash.is_active());
    chord(&mut app, "yy");
    assert_eq!(app.yank_flash.source(), Some(app.flash_source()));
    let row = |app: &App| -> Vec<_> {
        (0..10)
            .map(|c| app.yank_flash.cell_bg(before + ms(50), c, 10))
            .collect()
    };
    assert_eq!(row(&app)[0], ui::colors::YANKED_BG, "`yy` from the left");
    assert_eq!(row(&app)[9], ui::colors::SELECTED_BG);
    chord(&mut app, "yp");
    assert!(app.yank_flash.is_active(), "`yp` flashes too");
    assert_eq!(row(&app)[9], ui::colors::YANKED_BG, "`yp` from the right");
    assert_eq!(row(&app)[0], ui::colors::SELECTED_BG);
}

#[test]
fn a_failed_copy_and_nothing_to_copy_start_no_flash() {
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    app.yank(
        app::YankKind::Thing,
        |_| Err("pbcopy timed out".to_string()),
    );
    assert!(!app.yank_flash.is_active(), "a failed copy flashed");
    chord(&mut app, "yy");
    assert!(app.yank_flash.is_active());
    app.yank(
        app::YankKind::Thing,
        |_| Err("pbcopy timed out".to_string()),
    );
    assert!(!app.yank_flash.is_active(), "a failed copy left the flash");
    chord(&mut app, "yy");
    app.repos[0].owner = None;
    assert!(chord(&mut app, "yy").is_empty());
    assert_eq!(
        yank_status(&app),
        Some(("nothing to copy".to_string(), false))
    );
    assert!(!app.yank_flash.is_active(), "`nothing to copy` flashed");
}

#[test]
fn a_key_press_a_click_or_a_scroll_ends_the_flash_and_motion_does_not() {
    use crossterm::event::{KeyEventKind, KeyEventState};
    let mouse = |kind| {
        Event::Mouse(MouseEvent {
            kind,
            column: 3,
            row: 3,
            modifiers: KeyModifiers::NONE,
        })
    };
    let release = Event::Key(KeyEvent {
        code: KeyCode::Char('j'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Release,
        state: KeyEventState::NONE,
    });
    let keeps = [
        mouse(MouseEventKind::Moved),
        Event::Resize(80, 24),
        Event::FocusLost,
        Event::FocusGained,
        release,
    ];
    let ends = [
        Event::Key(press('j')),
        mouse(MouseEventKind::Down(MouseButton::Left)),
        mouse(MouseEventKind::ScrollDown),
        Event::Paste("x".to_string()),
    ];
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    for ev in &keeps {
        chord(&mut app, "yy");
        end_yank_flash_on(&mut app, ev);
        assert!(app.yank_flash.is_active(), "{ev:?} ended the flash");
    }
    for ev in &ends {
        chord(&mut app, "yy");
        end_yank_flash_on(&mut app, ev);
        assert!(!app.yank_flash.is_active(), "{ev:?} left the flash");
    }
}

/// A screen cell, `(x, y)`.
type Cell = (u16, u16);

/// Restart `app`'s flash at a known instant and draw it at `start + 150`
/// and `start + 400` ms. Returns the cells wearing the fading colour at
/// 150 ms, and the band cells inside `pane` at 400 ms.
fn flash_frames(
    app: &mut App,
    pane: impl Fn(&ui::PanelRects) -> ratatui::layout::Rect,
    from_right: bool,
) -> (Vec<Cell>, Vec<Cell>) {
    use ratatui::backend::TestBackend;
    assert!(app.yank_flash.is_active(), "the copy started no flash");
    let start = std::time::Instant::now();
    app.yank_flash.show(app.flash_source(), from_right, start);
    let draw = |at: u64| {
        let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
        term.draw(|f| ui::render_at(f, app, start + std::time::Duration::from_millis(at)))
            .unwrap();
        term.backend().buffer().clone()
    };
    let fade = ui::colors::mix(ui::colors::YANKED_BG, ui::colors::SELECTED_BG, 0.25);
    let at_150 = draw(150);
    let fading: Vec<_> = at_150
        .area
        .positions()
        .filter(|&p| at_150[p].bg == fade)
        .map(|p| (p.x, p.y))
        .collect();
    let at_400 = draw(400);
    let rect = pane(&ui::panel_rects(at_400.area, app));
    let band: Vec<_> = rect
        .positions()
        .filter(|&p| at_400[p].bg == ui::colors::SELECTED_BG)
        .map(|p| (p.x, p.y))
        .collect();
    assert!(
        !at_400.area.positions().any(|p| at_400[p].bg == fade),
        "the flash outlived 300 ms"
    );
    (fading, band)
}

#[test]
fn the_flash_fades_on_the_copied_repo_row_and_is_gone_by_400_ms() {
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    chord(&mut app, "yy");
    let (fading, band) = flash_frames(&mut app, |r| r.repo_list, false);
    assert!(!band.is_empty(), "no cursor band in pane 1");
    assert_eq!(fading, band, "the cursor row fades, and only it");
}

#[test]
fn the_flash_fades_on_a_commits_row() {
    let mut app = yank_on(app::HistoryView::Commits, FocusedPanel::Detail);
    app.set_detail_selected(1);
    chord(&mut app, "yy");
    let (fading, band) = flash_frames(&mut app, |r| r.detail.unwrap(), false);
    assert!(!band.is_empty(), "no cursor band in pane 2");
    assert_eq!(fading, band);
}

#[test]
fn the_flash_fades_on_a_notes_tree_row() {
    let scratch = std::env::temp_dir().join(format!("fleetz_flash_notes_{}", std::process::id()));
    let root = scratch.to_string_lossy().into_owned();
    let mut app = yank_on(app::HistoryView::Notes, FocusedPanel::Detail);
    app.repos[0].path = Some(root.clone());
    let walk = app::NotesWalk {
        key: root,
        root: scratch,
        generation: app.notes_generation,
    };
    let files = vec![notes::NoteFile {
        rel: "notez/a.md".into(),
        notez: Some("notez".into()),
    }];
    app.apply_notes_walk(walk, Ok(notes::Notes { files }));
    assert_eq!(chord(&mut app, "yp").len(), 1, "the path was copied");
    let (fading, band) = flash_frames(&mut app, |r| r.detail.unwrap(), true);
    assert!(!band.is_empty(), "no cursor band in the tree");
    assert_eq!(fading, band);
}

/// With a flash live and nothing queued, the loop wakes for a frame on
/// its own; with none it waits on the channel alone, as before FZ-81.
#[tokio::test]
async fn the_loop_wakes_for_the_flash_frame_and_only_while_it_lasts() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    let bound = std::time::Duration::from_millis(80);
    let idle = tokio::time::timeout(bound, next_event(&mut rx, &app.yank_flash)).await;
    assert!(idle.is_err(), "woke with no flash and nothing queued");
    chord(&mut app, "yy");
    let frame = tokio::time::timeout(bound, next_event(&mut rx, &app.yank_flash)).await;
    assert!(matches!(frame, Ok(Some(AppEvent::YankFrame))), "no frame");
    tx.send(AppEvent::Tick).unwrap();
    let queued = next_event(&mut rx, &app.yank_flash).await;
    assert!(
        matches!(queued, Some(AppEvent::Tick)),
        "a queued event waits"
    );
}

#[test]
fn a_copy_from_the_inspector_flashes_its_label_and_no_list_row() {
    let mut app = yank_on(app::HistoryView::Commits, FocusedPanel::Inspector);
    app.inspector_folded = false;
    // `yp`: the inspected hash; `yank_repo` loads no commit bodies.
    assert_eq!(chord(&mut app, "yp"), [YANK_HASHES[0]]);
    let (fading, _) = flash_frames(&mut app, |r| r.repo_list, true);
    let inspector = ui::panel_rects(ratatui::layout::Rect::new(0, 0, 120, 40), &app)
        .inspector
        .expect("inspector");
    let label = " [3] inspector ";
    let x0 = inspector.x + 1;
    let want: Vec<_> = (x0..x0 + label.len() as u16)
        .map(|x| (x, inspector.y))
        .collect();
    assert_eq!(fading, want, "the label, and nothing else");
}

/// The reader has no frame of its own: a copy from it flashes the
/// `notes preview` title on pane 2's frame, not the first text row.
#[test]
fn a_copy_from_the_reader_flashes_its_title_and_no_text_row() {
    let mut app = notes_flash_app(FocusedPanel::Reader);
    app.inspector_folded = false;
    assert_eq!(chord(&mut app, "yp").len(), 1, "the path was copied");
    let (fading, _) = flash_frames(&mut app, |r| r.repo_list, true);
    let mut term = Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    term.draw(|f| ui::render(f, &app)).unwrap();
    let buf = term.backend().buffer();
    let label = "notes preview";
    let want: Vec<_> = (0..buf.area.height)
        .flat_map(|y| {
            let row: String = (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            row.find(label)
                .map(|at| row[..at].chars().count() as u16)
                .map(|x| (0..label.len() as u16).map(move |i| (x + i, y)))
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(want.len(), label.len(), "the title is drawn once");
    assert_eq!(fading, want, "the title, and no text row");
}

/// `yank_on` in the `notes` view with one walked file, focused on
/// `panel`, so the tree and the reader are on screen.
fn notes_flash_app(panel: FocusedPanel) -> App {
    let scratch = std::env::temp_dir().join(format!("fleetz_flash_notes_{}", std::process::id()));
    let root = scratch.to_string_lossy().into_owned();
    let mut app = yank_on(app::HistoryView::Notes, panel);
    app.repos[0].path = Some(root.clone());
    let walk = app::NotesWalk {
        key: root,
        root: scratch,
        generation: app.notes_generation,
    };
    let files = vec![notes::NoteFile {
        rel: "notez/a.md".into(),
        notez: Some("notez".into()),
    }];
    app.apply_notes_walk(walk, Ok(notes::Notes { files }));
    app
}

/// A live flash from every source draws at every terminal size, down to
/// none at all: a resize does not end it, and a reader label on a
/// terminal a few rows tall lay outside the buffer and panicked.
#[test]
fn a_live_flash_draws_at_every_terminal_size() {
    use ratatui::backend::TestBackend;
    let sources = [
        yank_on(app::HistoryView::Info, FocusedPanel::RepoList),
        yank_on(app::HistoryView::Commits, FocusedPanel::Detail),
        yank_on(app::HistoryView::Commits, FocusedPanel::Inspector),
        notes_flash_app(FocusedPanel::Reader),
    ];
    let start = std::time::Instant::now();
    for mut app in sources {
        app.inspector_folded = false;
        app.yank_flash.show(app.flash_source(), false, start);
        for w in 0..=40u16 {
            for h in 0..=20u16 {
                let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                let at = start + std::time::Duration::from_millis(50);
                term.draw(|f| ui::render_at(f, &app, at)).unwrap();
            }
        }
    }
}

/// A live flash is an animation: its frames are painted through a burst
/// of queued background events, not held behind it.
#[test]
fn a_live_flash_is_never_held_behind_queued_events() {
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    assert!(!is_animating(&app));
    chord(&mut app, "yy");
    assert!(is_animating(&app), "the flash is not an animation");
    assert!(should_paint(false, 0, is_animating(&app)));
    app.yank_flash.clear();
    assert!(!is_animating(&app));
}

/// The flash belongs to the row it was copied from: a cursor moved by
/// anything other than input (which ends the flash first) leaves the
/// new row unflashed.
#[test]
fn a_flash_does_not_follow_the_cursor_to_another_row() {
    use ratatui::backend::TestBackend;
    let mut app = yank_on(app::HistoryView::Commits, FocusedPanel::Detail);
    app.set_detail_selected(1);
    chord(&mut app, "yy");
    assert!(app.yank_flash.is_active());
    app.set_detail_selected(2);
    let at = std::time::Instant::now() + std::time::Duration::from_millis(50);
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| ui::render_at(f, &app, at)).unwrap();
    let buf = term.backend().buffer();
    assert!(
        !buf.area
            .positions()
            .any(|p| buf[p].bg == ui::colors::YANKED_BG),
        "the flash moved to the new row"
    );
}

/// A terminal for driving the input handlers, as the `4` key test has.
fn yank_terminal() -> Terminal<CrosstermBackend<io::Stdout>> {
    let backend = CrosstermBackend::new(io::stdout());
    let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 120, 40));
    Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap()
}

/// The key handler hands keys to the chord: `y` on the compact grid
/// copies nothing and arms it, and the `j` after it ends it and still
/// moves the cursor. Fails if the handler stops calling the chord.
#[tokio::test]
async fn the_key_handler_feeds_the_yank_chord() {
    let args = Args::parse_from(["fleetz"]);
    let config = config::Config::default();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut terminal = yank_terminal();
    let mut app = App::new(vec![yank_repo(0), yank_repo(0)], String::new(), 120, 40);
    app.view = app::View::Compact;
    app.focused_panel = FocusedPanel::RepoList;
    let repo_dir = Path::new("/nonexistent");
    handle_normal_input(
        &mut app,
        &mut terminal,
        press('y'),
        &tx,
        &args,
        &config,
        repo_dir,
    )
    .await
    .unwrap();
    assert!(app.pending_yank, "`y` did not reach the chord");
    assert_eq!(yank_status(&app), None, "`y` alone copied something");
    handle_normal_input(
        &mut app,
        &mut terminal,
        press('j'),
        &tx,
        &args,
        &config,
        repo_dir,
    )
    .await
    .unwrap();
    assert!(!app.pending_yank, "`j` left the chord armed");
    assert_eq!(app.selected, 1, "`j` after `y` did not move the cursor");
}

/// A mouse press ends the chord: `y`, a click, then `p` copies nothing.
#[test]
fn a_click_ends_the_yank_chord() {
    let mut app = yank_on(app::HistoryView::Info, FocusedPanel::RepoList);
    let terminal = yank_terminal();
    assert!(chord(&mut app, "y").is_empty());
    assert!(app.pending_yank);
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse_input(&mut app, &terminal, click);
    assert!(!app.pending_yank, "the click left the chord armed");
    assert!(
        chord(&mut app, "p").is_empty(),
        "`p` after the click copied"
    );
}

/// Click on `needle` in pane 2, offset `dx` columns from its first cell,
/// and return what was copied.
fn click_at(app: &mut App, needle: &str, dx: u16) -> Vec<String> {
    let buf = yank_frame(app);
    let (x, y) = on_screen(&buf, app, needle);
    let (x, y) = (x + dx, y);
    assert_eq!(
        ui::panel_rects(buf.area, app).at(x, y),
        Some(FocusedPanel::Detail)
    );
    let mut copied = Vec::new();
    click_detail(app, x, y, |h| {
        copied.push(h.to_string());
        Ok(())
    });
    copied
}

#[test]
fn a_click_on_a_hash_moves_the_cursor_and_copies_it() {
    for view in [
        app::HistoryView::Info,
        app::HistoryView::Commits,
        app::HistoryView::Reflog,
        app::HistoryView::Graph,
    ] {
        for dx in [0, 6] {
            let mut app = yank_app(view, 0);
            let copied = click_at(&mut app, "bbb2222", dx);
            assert_eq!(copied, ["bbb2222"], "{view:?} at +{dx}");
            assert_eq!(app.inspected_hash(), Some("bbb2222"), "{view:?}");
            assert_eq!(
                yank_status(&app),
                Some(("copied bbb2222".to_string(), false)),
                "{view:?}"
            );
        }
    }
}

#[test]
fn a_click_on_the_row_off_the_hash_only_moves_the_cursor() {
    for view in [
        app::HistoryView::Info,
        app::HistoryView::Commits,
        app::HistoryView::Reflog,
    ] {
        let mut app = yank_app(view, 0);
        // The column just past the hash, and the subject further along.
        for (needle, dx) in [("bbb2222", 7), ("subject of ccc3333", 3)] {
            let copied = click_at(&mut app, needle, dx);
            assert!(copied.is_empty(), "{view:?} {needle}: copied {copied:?}");
            assert_eq!(app.status_message.as_ref().map(|m| m.text.as_str()), None);
        }
        assert_eq!(app.inspected_hash(), Some("ccc3333"), "{view:?}");
    }
    // The graph's cursor is a row: a click on a block's message line
    // moves the cursor into that block and copies nothing.
    let mut app = yank_app(app::HistoryView::Graph, 0);
    let copied = click_at(&mut app, "subject of ccc3333", 0);
    assert!(copied.is_empty());
    assert_eq!(app.inspected_hash(), Some("ccc3333"));
}

#[test]
fn a_click_outside_any_row_does_nothing() {
    let mut app = yank_app(app::HistoryView::Commits, 0);
    app.set_detail_selected(1);
    let buf = yank_frame(&app);
    let pane = ui::panel_rects(buf.area, &app).detail.unwrap();
    let (_, last) = on_screen(&buf, &app, "ccc3333");
    let (x, header) = on_screen(&buf, &app, "/tmp/yank");
    let mut copied = Vec::new();
    // Below the last row, in the pinned header, and on the border.
    for (cx, cy) in [(x, last + 2), (x, header), (pane.left(), last)] {
        click_detail(&mut app, cx, cy, |h| {
            copied.push(h.to_string());
            Ok(())
        });
    }
    assert!(copied.is_empty(), "copied {copied:?}");
    assert_eq!(app.inspected_hash(), Some("bbb2222"), "the cursor moved");
    assert!(app.status_message.is_none());
}

/// The map is read in body rows, so a scrolled list resolves to the row
/// drawn under the pointer, not to the row that many lines from the top.
#[test]
fn a_click_on_a_scrolled_list_lands_on_the_drawn_row() {
    let mut app = yank_app(app::HistoryView::Commits, 60);
    app.set_detail_selected(50);
    yank_frame(&app);
    assert!(app.detail_scroll() > 0, "the list did not scroll");
    let target = format!("{:07x}", 0xd000000 + 49);
    let copied = click_at(&mut app, &target, 0);
    assert_eq!(copied, std::slice::from_ref(&target));
    assert_eq!(app.inspected_hash(), Some(target.as_str()));
}

/// FZ-37 review: a click copies the hash that was on screen, even when
/// the list changed between the paint and the click (a `RepoUpdated`
/// landing in between).
#[test]
fn a_click_copies_the_painted_hash_after_the_list_changes() {
    let mut app = yank_app(app::HistoryView::Commits, 0);
    let buf = yank_frame(&app);
    let (x, y) = on_screen(&buf, &app, "bbb2222");
    // A new commit lands on top: index 1 now names `aaa1111`.
    let mut newer = app.repos[0].graph[0].clone();
    newer.hash = "fff0000".to_string();
    app.repos[0].graph.insert(0, newer);
    let mut copied = Vec::new();
    click_detail(&mut app, x, y, |h| {
        copied.push(h.to_string());
        Ok(())
    });
    assert_eq!(copied, ["bbb2222"]);
    assert_eq!(
        yank_status(&app),
        Some(("copied bbb2222".to_string(), false))
    );
}

// FZ-30b: the `ci` view's data layer, its trigger and its answers.

fn ci_app() -> App {
    let mut app = App::new(Vec::new(), String::new(), 120, 40);
    app.repos = vec![RepoStatus {
        name: "fleetz".to_string(),
        owner: Some("Gaurgle".to_string()),
        ..test_repo()
    }];
    app.jitter = Box::new(Planted {
        factor: 1.0,
        resume: 7,
    });
    app
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

/// FZ-59 owner ruling: `p` flips the `notes` reader between rendered
/// and source from the tree and from the reader, and `r` refreshes there
/// as everywhere. Fails while `r` is the toggle or `p` is unbound.
#[tokio::test]
async fn p_flips_the_notes_reader_and_r_still_refreshes() {
    let args = Args::parse_from(["fleetz"]);
    let config = config::Config::default();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let backend = CrosstermBackend::new(io::stdout());
    let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 120, 44));
    let mut terminal =
        Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap();
    let mut app = App::new(Vec::new(), String::new(), 120, 44);
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Notes;
    // No poll leaves the process: `r` below only sets its status.
    app.dirty_mode = true;
    let repo_dir = Path::new("/nonexistent");

    for (panel, raw) in [(FocusedPanel::Detail, true), (FocusedPanel::Reader, false)] {
        app.focused_panel = panel;
        handle_normal_input(
            &mut app,
            &mut terminal,
            press('p'),
            &tx,
            &args,
            &config,
            repo_dir,
        )
        .await
        .unwrap();
        assert_eq!(app.reader_raw, raw, "p from {panel:?}");

        app.status_message = None;
        handle_normal_input(
            &mut app,
            &mut terminal,
            press('r'),
            &tx,
            &args,
            &config,
            repo_dir,
        )
        .await
        .unwrap();
        assert_eq!(app.reader_raw, raw, "r flipped the reader from {panel:?}");
        let status = app.status_message.as_ref().map(|m| m.text.as_str());
        assert_eq!(
            status,
            Some("Refreshing…"),
            "r did not refresh from {panel:?}"
        );
    }
}

/// An app in the `notes` layout on a `w` by `h` screen, its tree
/// holding `n` files under `notez/`.
fn notes_tree_app(w: u16, h: u16, n: usize) -> App {
    let mut app = App::new(Vec::new(), String::new(), w, h);
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
    let files = (0..n)
        .map(|i| notes::NoteFile {
            rel: format!("notez/f{i:02}.md").into(),
            notez: Some("notez".into()),
        })
        .collect();
    app.apply_notes_walk(walk, Ok(notes::Notes { files }));
    assert!(app.notes_layout());
    app
}

/// Draw `app` on a `w` by `h` screen and return the row of `rect`, as a
/// screen row, that carries the tree's cursor mark, if any is drawn.
fn drawn_cursor_row(app: &App, w: u16, h: u16, rect: ratatui::layout::Rect) -> Option<u16> {
    use ratatui::backend::TestBackend;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| ui::render(f, app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    (rect.y..rect.y + rect.height).find(|&y| {
        (rect.x..rect.x + rect.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
            .contains("▸ ")
    })
}

/// FZ-65: the wheel over the `notes` tree keeps the highlighted file
/// inside the rows the tree draws, at a narrow, short pane and with his
/// terminal's two events per notch. Fails while the wheel only scrolls
/// the tree's view and leaves the highlight behind, off the drawn rows
/// within one notch.
#[test]
fn the_notes_tree_wheel_keeps_the_highlight_on_the_drawn_rows() {
    const W: u16 = 64;
    const H: u16 = 26;
    let area = ratatui::layout::Rect::new(0, 0, W, H);
    let mut app = notes_tree_app(W, H, 40);
    let tree = ui::panel_rects(area, &app).detail.expect("the tree");
    // Inside the border: the rows a file can be drawn on.
    let body = ratatui::layout::Rect::new(tree.x + 1, tree.y + 1, tree.width - 2, tree.height - 2);
    // The first frame measures the pane the wheel steps against.
    drawn_cursor_row(&app, W, H, body);
    let wheel = |kind| MouseEvent {
        kind,
        column: tree.x + tree.width / 2,
        row: tree.y + tree.height / 2,
        modifiers: KeyModifiers::NONE,
    };

    for (kind, notches) in [
        (MouseEventKind::ScrollDown, 20),
        (MouseEventKind::ScrollUp, 20),
    ] {
        for notch in 0..notches {
            let before = app.detail_selected();
            // Two events a notch, as his terminal sends them, and one
            // frame after both.
            handle_mouse_in(&mut app, area, wheel(kind));
            handle_mouse_in(&mut app, area, wheel(kind));
            let row = drawn_cursor_row(&app, W, H, body);
            assert!(
                row.is_some(),
                "{kind:?} notch {notch}: the highlight (file {}) left the drawn rows",
                app.detail_selected()
            );
            assert_ne!(
                app.detail_selected(),
                before,
                "{kind:?} notch {notch} moved nothing"
            );
        }
    }
    assert_eq!(app.detail_selected(), 0, "the wheel walks back to the top");
}

/// FZ-65 keeps the rule the wheel had before it: on the commit list it
/// scrolls the view and leaves the highlighted commit where it is.
#[test]
fn the_wheel_on_the_commit_list_still_leaves_its_highlight() {
    const W: u16 = 64;
    const H: u16 = 26;
    let area = ratatui::layout::Rect::new(0, 0, W, H);
    let mut app = notes_tree_app(W, H, 40);
    app.history_view = app::HistoryView::Commits;
    app.detail_max_scroll.set(50);
    let pane = ui::panel_rects(area, &app).detail.expect("pane 2");
    let wheel = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: pane.x + pane.width / 2,
        row: pane.y + pane.height / 2,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse_in(&mut app, area, wheel);
    handle_mouse_in(&mut app, area, wheel);
    assert_eq!(app.detail_selected(), 0, "the highlight stayed");
    assert_eq!(
        app.detail_scroll(),
        2 * DETAIL_WHEEL_ROWS as u16,
        "the view scrolled"
    );
}

/// A notes tree shaped like his: a `todos` section of three boards,
/// `notez/` holding two directories of dated notes, and a second, deep
/// notes directory, 36 files in all. Names, a directory and a section
/// are long enough to wrap in the tree's column.
fn his_notes() -> Vec<notes::NoteFile> {
    const SLUGS: [&str; 6] = [
        "repository-identity-design",
        "cache-invalidation-signature",
        "api-budget",
        "freshness-contract-amendments",
        "notes",
        "module-split-design-review-of-the-target-layers",
    ];
    let board = |rel: &str| notes::NoteFile {
        rel: rel.into(),
        notez: None,
    };
    let mut files = vec![
        board("TODO.md"),
        board("docs/TODO.md"),
        board("src/ui/TODO.md"),
    ];
    for (dir, n) in [("plans", 18), ("design-specs-and-accepted-contracts", 14)] {
        files.extend((0..n).map(|i| notes::NoteFile {
            rel: format!("notez/{dir}/2026-10-{:02}-{}.md", i + 1, SLUGS[i % 6]).into(),
            notez: Some("notez".into()),
        }));
    }
    files.push(notes::NoteFile {
        rel: "docs/superpowers/archive/notez/old.md".into(),
        notez: Some("docs/superpowers/archive/notez".into()),
    });
    files
}

/// FZ-65 rework 1, his look on the branch build: at his window the
/// highlight ran six wheel notches past the tree's last drawn row before
/// the tree moved. Long names wrap in the tree's column, and the follow
/// counted lines where the tree draws wrapped rows. Walks the tree down
/// and back up a notch at a time at two real window sizes and asserts
/// the highlight is drawn inside the tree's rows on every notch, and
/// inside the `SCROLLOFF` margin while the tree has rows past it.
#[test]
fn the_notes_tree_wheel_follows_wrapped_rows_at_his_window() {
    for (w, h) in [(200u16, 50u16), (120, 40), (64, 26), (80, 24), (100, 30)] {
        let area = ratatui::layout::Rect::new(0, 0, w, h);
        let mut app = notes_tree_app(w, h, 0);
        let walk = app::NotesWalk {
            key: "/repo".to_string(),
            root: "/repo".into(),
            generation: 1,
        };
        app.apply_notes_walk(walk, Ok(notes::Notes { files: his_notes() }));
        let pane2 = ui::panel_rects(area, &app).detail.expect("pane 2");
        let body = ui::detail::notes_pane_body(pane2, &app)
            .expect("a tree")
            .body;
        drawn_cursor_row(&app, w, h, body);
        let files = app.selected_commit_count();
        assert_eq!(files, 36, "{w}x{h}: one stop per file");
        // The rows the tree scrolls in: under the mode line where it is
        // pinned, the whole body where it is not.
        let rows = app.detail_view_height.get();
        let (top, bottom) = (body.bottom() - rows, body.bottom() - 1);
        let pad = app::scrolloff_for(rows);
        let wheel = |kind| MouseEvent {
            kind,
            column: body.x + body.width / 2,
            row: body.y + body.height / 2,
            modifiers: KeyModifiers::NONE,
        };
        for kind in [MouseEventKind::ScrollDown, MouseEventKind::ScrollUp] {
            let mut past = Vec::new();
            let mut misplaced = Vec::new();
            let mut in_margin = Vec::new();
            for notch in 0..files {
                let before = app.detail_selected();
                handle_mouse_in(&mut app, area, wheel(kind));
                handle_mouse_in(&mut app, area, wheel(kind));
                let drawn = drawn_cursor_row(&app, w, h, body);
                // Past either end the wheel scrolls the header or the
                // tail in instead (round 1), which on a pane too short
                // to pin the header takes the first file off the foot.
                if app.detail_selected() == before {
                    continue;
                }
                let Some(row) = drawn else {
                    past.push(notch);
                    continue;
                };
                // Where the follow counted it: its line less the offset.
                let line = app.detail_row_tops.borrow()[app.detail_selected()];
                if row != top + line - app.detail_scroll() {
                    misplaced.push(notch);
                }
                let more_below = app.detail_scroll() < app.detail_max_scroll.get();
                let more_above = app.detail_scroll() > 0;
                let margin = match kind {
                    MouseEventKind::ScrollDown => more_below && row > bottom - pad,
                    _ => more_above && row < top + pad,
                };
                if margin {
                    in_margin.push(notch);
                }
            }
            assert!(
                past.is_empty(),
                "{w}x{h} {kind:?}: the highlight ran {} notches past the drawn rows \
                 (notches {past:?})",
                past.len()
            );
            assert!(
                misplaced.is_empty(),
                "{w}x{h} {kind:?}: the highlight was drawn off the row the follow \
                 counted on notches {misplaced:?}"
            );
            assert!(
                in_margin.is_empty(),
                "{w}x{h} {kind:?}: the view did not follow at the margin on notches \
                 {in_margin:?}"
            );
        }
        assert_eq!(app.detail_selected(), 0, "{w}x{h}: back at the top");
    }
}

/// The next answer `spawn_note_read` sent, split into its two kinds.
async fn next_note_answer(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Result<
    (app::NoteRead, Result<String, String>, notes::NoteFacts),
    (app::NoteRead, notes::LastCommit),
> {
    match rx.recv().await {
        Some(AppEvent::NoteRead {
            read,
            text,
            facts,
            rendered: _,
        }) => Ok((read, text, facts)),
        Some(AppEvent::NoteCommit { read, last }) => Err((read, last)),
        _ => panic!("not a note answer"),
    }
}

/// The file's facts are read off the UI thread (FZ-62): `spawn_note_read`
/// returns before anything is sent. The body and its metadata come
/// first and git's last commit after, so the reader never waits on git
/// (review F4): a date for a committed file, "untracked" for one git
/// never saw, and a name with glob characters read literally (review
/// F5: `notez/*.md` would otherwise match the committed `plan.md`). A
/// commit answer for a file no longer highlighted is dropped.
#[tokio::test]
async fn spawn_note_read_reads_the_body_and_facts_off_the_ui_thread() {
    let scratch = std::env::temp_dir().join("fleetz_test_note_read_seam");
    let _ = std::fs::remove_dir_all(&scratch);
    let repo = hist_repo(&scratch, "repo", "n@example.com");
    std::fs::create_dir_all(repo.join("notez")).unwrap();
    std::fs::create_dir_all(repo.join(".notez")).unwrap();
    std::fs::write(repo.join("notez/plan.md"), "# Plan\n").unwrap();
    std::fs::write(repo.join("notez/*.md"), "star\n").unwrap();
    std::fs::write(repo.join(".notez/scratch.md"), "scratch\n").unwrap();
    wt_sh(&repo, &["git", "add", "notez/plan.md"]);
    hist_commit(&repo, "2026-10-01", "plan");

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let answer = |rel: &str| app::NoteRead {
        path: repo.join(rel),
        root: repo.clone(),
        generation: 0,
        render_width: None,
    };
    spawn_note_read(&tx, answer("notez/plan.md"));
    // A current-thread test runtime has not run the task yet: had the
    // read run inline, its answer would already be here.
    assert!(rx.try_recv().is_err(), "read on the calling thread");
    let Ok((read, text, facts)) = next_note_answer(&mut rx).await else {
        panic!("the body comes first");
    };
    assert_eq!(read.path, repo.join("notez/plan.md"));
    assert_eq!(text, Ok("# Plan\n".to_string()));
    assert_eq!(facts.size, Some(7));
    assert!(facts.modified.is_some());
    let Err((read, last)) = next_note_answer(&mut rx).await else {
        panic!("then the commit");
    };
    assert_eq!(read.path, repo.join("notez/plan.md"));
    let notes::LastCommit::At(date) = last else {
        panic!("committed: {last:?}");
    };
    assert!(date.starts_with("2026-10-01T12:00:00"), "{date}");

    for (rel, size) in [(".notez/scratch.md", 8), ("notez/*.md", 5)] {
        spawn_note_read(&tx, answer(rel));
        let Ok((_, _, facts)) = next_note_answer(&mut rx).await else {
            panic!("{rel}: the body comes first");
        };
        assert_eq!(facts.size, Some(size), "{rel}");
        let Err((_, last)) = next_note_answer(&mut rx).await else {
            panic!("{rel}: then the commit");
        };
        assert_eq!(last, notes::LastCommit::Untracked, "{rel}");
    }

    // Landed in the app: the commit of a file the cursor has left is
    // dropped, and the highlighted file's lands.
    let key = repo.to_string_lossy().into_owned();
    let mut app = App::new(Vec::new(), String::new(), 120, 40);
    app.repos = vec![RepoStatus {
        path: Some(key.clone()),
        ..test_repo()
    }];
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Notes;
    let walk = app::NotesWalk {
        key: key.clone(),
        root: repo.clone(),
        generation: 0,
    };
    let files = ["notez/plan.md", "notez/zz.md"]
        .map(|rel| notes::NoteFile {
            rel: rel.into(),
            notez: Some("notez".into()),
        })
        .to_vec();
    app.apply_notes_walk(walk, Ok(notes::Notes { files }));
    let plan = app.claim_note_read().expect("plan");
    spawn_note_read(&tx, plan.clone());
    app.select_commit_down();
    let other = app.claim_note_read().expect("the other file");
    let Ok((read, text, facts)) = next_note_answer(&mut rx).await else {
        panic!("body");
    };
    app.apply_note_facts(&read, facts);
    app.apply_note_read(read, text);
    let Err((read, last)) = next_note_answer(&mut rx).await else {
        panic!("commit");
    };
    app.apply_note_commit(&read, last);
    let held = app.note_body.as_ref().expect("held");
    assert_eq!(held.path, other.path);
    assert_eq!(
        (&held.text, &held.facts, &held.last_commit),
        (&None, &None, &None),
        "nothing of the file the cursor left"
    );
    app.apply_note_commit(&other, notes::LastCommit::Untracked);
    assert_eq!(
        app.note_body.as_ref().and_then(|b| b.last_commit.clone()),
        Some(notes::LastCommit::Untracked)
    );
    let _ = std::fs::remove_dir_all(&scratch);
}

/// FZ-59: a capped markdown file costs more to render than a keypress
/// frame (`docs/performance-baseline.md`), so it is rendered off the UI
/// thread: with the read, at the read's width, in the same answer as
/// the body, and again at a new width by `spawn_note_render`. Neither
/// call has sent anything when it returns.
#[tokio::test]
async fn markdown_renders_off_the_ui_thread_with_the_read_and_on_a_resize() {
    let scratch = std::env::temp_dir().join("fleetz_test_note_render_seam");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(scratch.join("notez")).unwrap();
    let path = scratch.join("notez/plan.md");
    std::fs::write(&path, "# Plan\n\nship it\n").unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let read = app::NoteRead {
        path: path.clone(),
        root: scratch.clone(),
        generation: 3,
        render_width: Some(30),
    };
    spawn_note_read(&tx, read);
    assert!(rx.try_recv().is_err(), "rendered on the calling thread");
    let Some(AppEvent::NoteRead { text, rendered, .. }) = rx.recv().await else {
        panic!("the body comes first");
    };
    let body = text.expect("read");
    let rendered = rendered.expect("rendered with the read");
    assert_eq!(rendered, render_note(&body, 30));
    let first: String = rendered.lines[0]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(first, "Plan", "rendered, not the source");

    let render = app::NoteRender {
        path: path.clone(),
        generation: 3,
        width: 12,
        text: body.clone(),
    };
    spawn_note_render(&tx, render.clone());
    assert!(rx.try_recv().is_err(), "re-rendered on the calling thread");
    loop {
        match rx.recv().await {
            Some(AppEvent::NoteRendered {
                render: got,
                rendered,
            }) => {
                assert_eq!(got, render);
                assert_eq!(rendered, render_note(&body, 12));
                break;
            }
            Some(AppEvent::NoteCommit { .. }) => continue,
            _ => panic!("not a render"),
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
}

/// FZ-59 review: a capped body's `… cut at 256 KiB` line, as
/// `notes::read_body` really writes it, is not parsed as markdown. Cut
/// inside an open fence or a table it stays its own last line, plain,
/// rather than a code line or a table cell; the line above it is still
/// the fence's or the table's.
#[test]
fn the_cut_marker_is_its_own_plain_line_after_a_fence_or_a_table() {
    let scratch = std::env::temp_dir().join("fleetz_test_note_cut_marker");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    let text_of = |line: &ratatui::text::Line<'_>| -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    };
    let fence = format!("```\n{}", "x\n".repeat(notes::BODY_CAP));
    let table = format!("| a | b |\n|---|---|\n{}", "| 1 | 2 |\n".repeat(30_000));
    for (name, source, above) in [("fence.md", fence, "  x"), ("table.md", table, "1 │")] {
        let path = scratch.join(name);
        std::fs::write(&path, source).unwrap();
        let body = notes::read_body(&path).expect("read");
        assert!(body.ends_with("\n… cut at 256 KiB"), "{name}: cut");
        let lines = render_note(&body, 40).lines;
        let last = lines.last().expect("lines");
        assert_eq!(text_of(last), "… cut at 256 KiB", "{name}");
        assert_eq!(last.spans.len(), 1, "{name}: one plain span");
        assert_eq!(last.spans[0].style.fg, Some(ui::colors::TEXT), "{name}");
        let marked = lines
            .iter()
            .filter(|l| text_of(l).contains("cut at"))
            .count();
        assert_eq!(marked, 1, "{name}: the marker is nowhere else");
        let before = text_of(&lines[lines.len() - 2]);
        assert!(before.starts_with(above), "{name}: {before:?}");
    }
    let short = render_note("# T\n\nbody", 40).lines;
    assert!(!short.iter().any(|l| text_of(l).contains("cut at")));
    let _ = std::fs::remove_dir_all(&scratch);
}

fn ci_key() -> app::RemoteKey {
    (Some("gaurgle".to_string()), "fleetz".to_string())
}

fn ci_runs() -> Vec<ci::WorkflowRun> {
    ci::parse_runs(include_str!("ci/fixtures/runs.json")).unwrap()
}

fn ci_answer(request: &ci::CiRequest) -> ci::CiAnswer {
    ci::CiAnswer {
        key: request.key.clone(),
        attempt: request.attempt,
        runs: None,
        jobs: None,
        log: None,
        full_log: None,
        failure: None,
        budget: None,
        tip: None,
    }
}

fn fresh_runs(etag: &str) -> Option<ci::Fetched<Vec<ci::WorkflowRun>>> {
    Some(ci::Fetched::Fresh {
        data: ci_runs(),
        etag: Some(etag.to_string()),
    })
}

/// Claim and land one successful list poll at `now`.
fn ci_landed(app: &mut App, now: chrono::DateTime<chrono::Local>) {
    let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
    let answer = ci::CiAnswer {
        runs: fresh_runs("W/\"6b13\""),
        ..ci_answer(&request)
    };
    app.apply_ci(answer, now);
}

#[test]
fn a_ci_target_needs_an_owner() {
    let legacy = RepoStatus {
        name: "fleetz".to_string(),
        ..test_repo()
    };
    assert_eq!(ci::target(&legacy), None, "known by name only: no URL");
    let (key, owner, name) = ci::target(&ci_app().repos[0]).unwrap();
    assert_eq!(key, ci_key());
    assert_eq!((owner.as_str(), name.as_str()), ("gaurgle", "fleetz"));
}

#[test]
fn the_ci_poll_waits_for_the_ci_view() {
    let mut app = ci_app();
    assert!(!app.ci_wanted(), "pane 2 opens on info");
    let start = std::time::Instant::now();
    for ms in [0, 300, 60_000] {
        let instant = start + std::time::Duration::from_millis(ms);
        assert_eq!(ci_poll_request(&mut app, at(12, 0, 0), instant), None);
    }
    assert!(app.ci.is_empty());
}

#[test]
fn the_ci_view_polls_the_selected_repo_once_the_cursor_dwells() {
    let mut app = ci_app();
    app.history_view = app::HistoryView::Ci;
    app.view = app::View::Detail;
    let start = std::time::Instant::now();
    let after = |ms| start + std::time::Duration::from_millis(ms);
    assert_eq!(ci_poll_request(&mut app, at(12, 0, 0), after(0)), None);
    assert_eq!(ci_poll_request(&mut app, at(12, 0, 0), after(299)), None);
    let request = ci_poll_request(&mut app, at(12, 0, 0), after(300)).expect("settled");
    assert_eq!(request.key, ci_key());
    assert_eq!(request.runs_etag, None);
    assert_eq!(
        ci_poll_request(&mut app, at(12, 0, 1), after(1300)),
        None,
        "one poll out at a time"
    );
}

#[test]
fn a_row_without_a_github_identity_sends_no_ci_poll() {
    let mut app = ci_app();
    app.history_view = app::HistoryView::Ci;
    app.view = app::View::Detail;
    app.repos[0].owner = None;
    let start = std::time::Instant::now();
    ci_poll_request(&mut app, at(12, 0, 0), start);
    let later = start + std::time::Duration::from_secs(5);
    assert_eq!(ci_poll_request(&mut app, at(12, 0, 5), later), None);
}

#[test]
fn the_ci_poll_follows_the_cadence_of_the_shown_runs() {
    let mut app = ci_app();
    app.history_view = app::HistoryView::Ci;
    app.view = app::View::Detail;
    ci_landed(&mut app, at(12, 0, 0));
    let start = std::time::Instant::now();
    let settled = start + std::time::Duration::from_secs(1);
    ci_poll_request(&mut app, at(12, 0, 0), start);
    // The fixture's newest run is in progress: 10 s.
    assert_eq!(ci_poll_request(&mut app, at(12, 0, 9), settled), None);
    let request = ci_poll_request(&mut app, at(12, 0, 10), settled).expect("10 s on");
    assert_eq!(request.runs_etag.as_deref(), Some("W/\"6b13\""));
    // Every shown run finished: 60 s.
    let mut finished = ci_runs();
    finished.retain(|r| !r.is_live());
    let answer = ci::CiAnswer {
        runs: Some(ci::Fetched::Fresh {
            data: finished,
            etag: None,
        }),
        ..ci_answer(&request)
    };
    app.apply_ci(answer, at(12, 0, 10));
    assert_eq!(ci_poll_request(&mut app, at(12, 1, 9), settled), None);
    assert!(ci_poll_request(&mut app, at(12, 1, 10), settled).is_some());
}

#[test]
fn a_304_keeps_the_runs_and_is_a_successful_attempt() {
    let mut app = ci_app();
    ci_landed(&mut app, at(12, 0, 0));
    let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
    assert_eq!(
        request.runs_etag.as_deref(),
        Some("W/\"6b13\""),
        "sent back"
    );
    let answer = ci::CiAnswer {
        runs: Some(ci::Fetched::NotModified),
        ..ci_answer(&request)
    };
    app.apply_ci(answer, at(12, 0, 10));
    let state = &app.ci[&ci_key()];
    assert_eq!(state.runs, ci_runs(), "the data is kept");
    assert_eq!(state.runs_etag.as_deref(), Some("W/\"6b13\""));
    assert_eq!(state.freshness.last_attempt, Some(at(12, 0, 10)));
    assert_eq!(state.freshness.data_time, app::DataTime::At(at(12, 0, 10)));
    assert_eq!(state.freshness.outcome, app::Outcome::Success);
    assert!(!state.inflight);
}

/// What a CI answer must never move: the rows, and the fleet poll's
/// freshness, budget, backoff, lock, dataset and coverage.
fn fleet_snapshot(app: &App) -> String {
    format!(
        "{:?}|{:?}|{:?}|{:?}|{}|{}|{:?}|{:?}",
        app.repos,
        app.github_freshness,
        app.api_budget,
        app.github_backoff,
        app.github_poll_inflight,
        app.dataset.len(),
        app.github_meta,
        app.not_in_last_poll,
    )
}

fn ci_failure_lands(failure: github::Failure, budget: Option<github::BudgetReading>) -> App {
    let mut app = ci_app();
    ci_landed(&mut app, at(12, 0, 0));
    let before = fleet_snapshot(&app);
    let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
    let answer = ci::CiAnswer {
        failure: Some(failure.clone()),
        budget,
        ..ci_answer(&request)
    };
    app.apply_ci(answer, at(12, 0, 10));
    assert_eq!(fleet_snapshot(&app), before, "{failure:?} moved the fleet");
    let state = &app.ci[&ci_key()];
    assert_eq!(state.runs, ci_runs(), "a failure keeps the data");
    assert_eq!(state.freshness.data_time, app::DataTime::At(at(12, 0, 0)));
    assert_eq!(state.freshness.last_attempt, Some(at(12, 0, 10)));
    assert_eq!(state.freshness.outcome, app::Outcome::Failed(failure));
    assert!(!state.inflight);
    app
}

#[test]
fn an_offline_ci_poll_keeps_the_runs_and_leaves_the_fleet_alone() {
    let failure = github::Failure::new(github::FailureKind::Offline, "no route");
    let app = ci_failure_lands(failure, None);
    assert_eq!(
        app.ci[&ci_key()].backoff.not_before(60),
        None,
        "offline counts nothing"
    );
}

#[test]
fn a_rate_limited_ci_poll_holds_until_the_reset_and_leaves_the_fleet_alone() {
    let reset = at(12, 40, 0);
    let budget = reading("tok", 0, reset, at(12, 0, 10));
    let failure = github::Failure::rate_limit("API rate limit exceeded");
    let mut app = ci_failure_lands(failure, Some(budget));
    let resume = reset + secs(7);
    assert_eq!(app.ci[&ci_key()].backoff.limit_until(60), Some(resume));
    assert_eq!(app.ci_budget.known(at(12, 0, 10)).unwrap().remaining, 0);
    app.history_view = app::HistoryView::Ci;
    app.view = app::View::Detail;
    let start = std::time::Instant::now();
    let settled = start + std::time::Duration::from_secs(1);
    ci_poll_request(&mut app, at(12, 0, 10), start);
    assert_eq!(ci_poll_request(&mut app, at(12, 39, 0), settled), None);
    assert!(ci_poll_request(&mut app, resume, settled).is_some());
}

#[test]
fn a_404_on_a_gone_run_marks_it_until_the_next_list_drops_it() {
    let mut app = ci_app();
    ci_landed(&mut app, at(12, 0, 0));
    let gone = ci_runs()[2].id;
    app.ci.get_mut(&ci_key()).unwrap().expanded = Some(gone);
    let before = fleet_snapshot(&app);
    let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
    assert_eq!(request.expanded.as_ref().map(|e| e.run_id), Some(gone));
    let answer = ci::CiAnswer {
        runs: Some(ci::Fetched::NotModified),
        jobs: Some((gone, ci::JobsAnswer::Gone)),
        ..ci_answer(&request)
    };
    app.apply_ci(answer, at(12, 0, 10));
    assert_eq!(fleet_snapshot(&app), before);
    let state = &app.ci[&ci_key()];
    assert!(state.gone.contains(&gone), "the pane can say `run gone`");
    assert_eq!(state.runs.len(), 3, "until the next list");
    assert_eq!(state.freshness.outcome, app::Outcome::Success);

    let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
    let answer = ci::CiAnswer {
        runs: fresh_runs("W/\"next\""),
        ..ci_answer(&request)
    };
    app.apply_ci(answer, at(12, 0, 20));
    let state = &app.ci[&ci_key()];
    assert!(state.runs.iter().all(|r| r.id != gone), "dropped");
    assert!(state.gone.is_empty());
    assert_eq!(state.expanded, None, "nothing left to expand");
}

#[test]
fn a_late_ci_answer_does_not_move_the_data_backwards() {
    let mut app = ci_app();
    let first = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
    let second = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
    let newer = ci::CiAnswer {
        runs: fresh_runs("W/\"new\""),
        ..ci_answer(&second)
    };
    app.apply_ci(newer, at(12, 0, 10));
    let older = ci::CiAnswer {
        runs: Some(ci::Fetched::Fresh {
            data: Vec::new(),
            etag: Some("W/\"old\"".to_string()),
        }),
        ..ci_answer(&first)
    };
    app.apply_ci(older, at(12, 0, 20));
    let state = &app.ci[&ci_key()];
    assert_eq!(state.runs_etag.as_deref(), Some("W/\"new\""));
    assert_eq!(state.runs.len(), 3);
    assert_eq!(state.freshness.last_attempt, Some(at(12, 0, 10)));
}

#[test]
fn a_failed_log_is_read_once_per_job() {
    let mut app = ci_app();
    ci_landed(&mut app, at(12, 0, 0));
    let run = ci_runs()[2].id;
    app.ci.get_mut(&ci_key()).unwrap().expanded = Some(run);
    let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
    let jobs = ci::parse_jobs(include_str!("ci/fixtures/jobs-failed.json")).unwrap();
    let answer = ci::CiAnswer {
        runs: Some(ci::Fetched::NotModified),
        jobs: Some((
            run,
            ci::JobsAnswer::Fetched(ci::Fetched::Fresh {
                data: jobs.clone(),
                etag: Some("W/\"jobs\"".to_string()),
            }),
        )),
        log: Some((jobs[0].id, ci::LogTail::Lines(vec!["boom".to_string()]))),
        ..ci_answer(&request)
    };
    app.apply_ci(answer, at(12, 0, 10));
    let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
    let expanded = request.expanded.expect("the run is still expanded");
    assert_eq!(expanded.jobs_etag.as_deref(), Some("W/\"jobs\""));
    assert_eq!(expanded.held_jobs, jobs);
    assert_eq!(expanded.logs_held, vec![jobs[0].id]);
}

// FZ-30c: the follow-ups to #90, kept by the `ci` view.

/// One repo's failures climb that repo's ladder only: a second repo the
/// cursor moves to polls as soon as it dwells. Fails while the backoff
/// is one for the whole session.
#[test]
fn one_repos_ci_failure_does_not_hold_another_repos_first_poll() {
    let mut app = ci_app();
    app.repos.push(RepoStatus {
        name: "repoz".to_string(),
        owner: Some("Gaurgle".to_string()),
        ..test_repo()
    });
    app.history_view = app::HistoryView::Ci;
    app.view = app::View::Detail;
    let server = github::Failure::new(github::FailureKind::Server, "502");
    for n in 0..3 {
        let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
        let answer = ci::CiAnswer {
            failure: Some(server.clone()),
            ..ci_answer(&request)
        };
        app.apply_ci(answer, at(12, 0, n));
    }
    assert!(app.ci[&ci_key()].backoff.not_before(60).is_some());
    let other = app
        .visible_repos()
        .iter()
        .position(|(_, r)| r.name == "repoz")
        .unwrap();
    app.selected = other;
    let start = std::time::Instant::now();
    ci_poll_request(&mut app, at(12, 0, 3), start);
    let settled = start + std::time::Duration::from_secs(1);
    let request = ci_poll_request(&mut app, at(12, 0, 3), settled).expect("not held");
    assert_eq!(request.name, "repoz");
}

/// A CI poll that never answers is given up on, as the fleet poll is:
/// the lock is released and the attempt is a timed-out failure on that
/// repo. Fails while `inflight` can stay set for the session.
#[test]
fn a_stuck_ci_poll_is_given_up_on() {
    let mut app = ci_app();
    ci_landed(&mut app, at(12, 0, 0));
    let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
    let claimed = std::time::Instant::now();
    app.expire_stuck_ci_polls(CI_POLL_STUCK_AFTER, claimed, at(12, 0, 30));
    assert!(app.ci[&ci_key()].inflight, "not yet");
    let later = claimed + CI_POLL_STUCK_AFTER + std::time::Duration::from_secs(1);
    app.expire_stuck_ci_polls(CI_POLL_STUCK_AFTER, later, at(12, 2, 1));
    let state = &app.ci[&ci_key()];
    assert!(!state.inflight);
    assert_eq!(state.started, None);
    assert!(matches!(
        &state.freshness.outcome,
        app::Outcome::Failed(f) if f.kind == github::FailureKind::TimedOut
    ));
    assert_eq!(state.freshness.last_attempt, Some(at(12, 2, 1)));
    assert_eq!(state.runs, ci_runs(), "the data is kept");
    assert!(
        state.backoff.not_before(60).is_some(),
        "it climbs the ladder"
    );
    // An answer that still arrives is applied like any.
    let answer = ci::CiAnswer {
        runs: Some(ci::Fetched::NotModified),
        ..ci_answer(&request)
    };
    app.apply_ci(answer, at(12, 2, 5));
    assert_eq!(app.ci[&ci_key()].freshness.outcome, app::Outcome::Success);
}

/// The list answered and a later call failed: the runs on screen are
/// that current, so the data time advances while the outcome is the
/// failure. Fails while a failed attempt never moves the data time.
#[test]
fn a_fresh_list_advances_the_data_time_when_a_later_call_fails() {
    let mut app = ci_app();
    ci_landed(&mut app, at(12, 0, 0));
    app.ci.get_mut(&ci_key()).unwrap().expanded = Some(ci_runs()[2].id);
    let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
    let offline = github::Failure::new(github::FailureKind::Offline, "jobs: no route");
    let answer = ci::CiAnswer {
        runs: fresh_runs("W/\"next\""),
        failure: Some(offline.clone()),
        ..ci_answer(&request)
    };
    app.apply_ci(answer, at(12, 0, 10));
    let state = &app.ci[&ci_key()];
    assert_eq!(state.freshness.data_time, app::DataTime::At(at(12, 0, 10)));
    assert_eq!(state.freshness.outcome, app::Outcome::Failed(offline));
    assert_eq!(state.runs_etag.as_deref(), Some("W/\"next\""));
}

/// An answer landing is something to draw. Fails while `AppEvent::Ci`
/// waits for the next tick's repaint.
#[test]
fn a_ci_answer_asks_for_a_redraw() {
    let mut app = ci_app();
    let request = app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
    app.needs_redraw = false;
    app.apply_ci(
        ci::CiAnswer {
            runs: fresh_runs("W/\"6b13\""),
            ..ci_answer(&request)
        },
        at(12, 0, 0),
    );
    assert!(app.needs_redraw);
}

// FZ-30d: the `ci` view in pane 2 is the only thing that polls.

/// `]` from `branches` reaches `ci`, and there `j`/`k` walk its runs in
/// the view's own selection slot, never the repo list. Fails while `ci`
/// is not a pane 2 view.
#[tokio::test]
async fn j_and_k_walk_the_runs_in_the_ci_view() {
    let args = Args::parse_from(["fleetz"]);
    let config = config::Config::default();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let backend = CrosstermBackend::new(io::stdout());
    let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 120, 44));
    let mut terminal =
        Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap();
    let mut app = ci_app();
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Branches;
    ci_landed(&mut app, at(12, 0, 0));
    app.focused_panel = FocusedPanel::Detail;
    let repo_dir = Path::new("/nonexistent");
    handle_normal_input(
        &mut app,
        &mut terminal,
        press(']'),
        &tx,
        &args,
        &config,
        repo_dir,
    )
    .await
    .unwrap();
    assert_eq!(app.history_view, app::HistoryView::Ci);
    assert!(app.ci_wanted());
    let runs = app.ci[&ci_key()].runs.len();
    assert!(runs >= 3, "fixture has {runs} runs");
    for (key, cursor) in [('j', 1), ('j', 2), ('k', 1)] {
        handle_normal_input(
            &mut app,
            &mut terminal,
            press(key),
            &tx,
            &args,
            &config,
            repo_dir,
        )
        .await
        .unwrap();
        assert_eq!(app.detail_selected(), cursor, "after {key}");
    }
    assert_eq!(app.selected, 0, "the repo list did not move");
}

/// FZ-30f: on the focused inspector in `ci`, `]` steps the body and `j`
/// moves the job mark; pane 2's view and the run cursor stay put.
#[tokio::test]
async fn the_inspector_keys_step_the_bodies_and_the_job_mark() {
    let args = Args::parse_from(["fleetz"]);
    let config = config::Config::default();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let backend = CrosstermBackend::new(io::stdout());
    let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 120, 44));
    let mut terminal =
        Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap();
    let mut app = ci_app();
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Ci;
    ci_landed(&mut app, at(12, 0, 0));
    let run = ci_runs()[2].id;
    let mut jobs = ci::parse_jobs(include_str!("ci/fixtures/jobs-failed.json")).unwrap();
    let mut second = jobs[0].clone();
    second.id = 7;
    jobs.push(second);
    app.ci
        .get_mut(&ci_key())
        .unwrap()
        .jobs
        .insert(run, app::CiJobs { etag: None, jobs });
    app.set_detail_selected(2);
    app.focused_panel = FocusedPanel::Inspector;
    let repo_dir = Path::new("/nonexistent");
    for key in ['j', ']'] {
        handle_normal_input(
            &mut app,
            &mut terminal,
            press(key),
            &tx,
            &args,
            &config,
            repo_dir,
        )
        .await
        .unwrap();
    }
    assert_eq!(
        app.ci_marked_job().map(|j| j.id),
        Some(7),
        "j moved the mark"
    );
    assert_eq!(app.ci_body, app::CiBody::Log, "] stepped the body");
    assert_eq!(app.history_view, app::HistoryView::Ci, "not the view");
    assert_eq!(app.detail_selected(), 2, "not the run");
}

/// FZ-30f fix cycle 3: `]` from the focused inspector in `branches`
/// enters `ci` with its runs held and the next `[` goes back to `branches`;
/// back in `ci`, any other key ends the streak and `]` steps the bodies.
/// Fails while the second `[` sticks on the bodies.
#[tokio::test]
async fn a_bracket_streak_cycles_views_through_ci_until_another_key() {
    let args = Args::parse_from(["fleetz"]);
    let config = config::Config::default();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let backend = CrosstermBackend::new(io::stdout());
    let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 120, 44));
    let mut terminal =
        Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap();
    let mut app = ci_app();
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Ci;
    ci_landed(&mut app, at(12, 0, 0));
    app.history_view = app::HistoryView::Branches;
    app.focused_panel = FocusedPanel::Inspector;
    let repo_dir = Path::new("/nonexistent");
    let mut views = Vec::new();
    for key in [']', '[', ']', 'j', ']'] {
        handle_normal_input(
            &mut app,
            &mut terminal,
            press(key),
            &tx,
            &args,
            &config,
            repo_dir,
        )
        .await
        .unwrap();
        views.push((app.history_view, app.ci_body));
    }
    use app::{CiBody, HistoryView};
    assert_eq!(
        views,
        vec![
            (HistoryView::Ci, CiBody::Jobs),
            (HistoryView::Branches, CiBody::Jobs),
            (HistoryView::Ci, CiBody::Jobs),
            (HistoryView::Ci, CiBody::Jobs),
            (HistoryView::Ci, CiBody::Log),
        ]
    );
}

/// The tick polls only while pane 2 shows `ci`: never on `info` (whose
/// `ci` section draws what is held), never in Compact, and switching the
/// view away stops it. The held runs survive the switch. Fails while the
/// trigger ignores the view.
#[test]
fn the_ci_tick_polls_only_while_pane_2_shows_ci() {
    let mut app = ci_app();
    app.view = app::View::Detail;
    let start = std::time::Instant::now();
    let after = |s| start + std::time::Duration::from_secs(s);
    for view in app::HistoryView::ALL {
        if view == app::HistoryView::Ci {
            continue;
        }
        app.history_view = view;
        ci_tick(&mut app, at(12, 0, 0), start);
        assert_eq!(ci_tick(&mut app, at(12, 0, 1), after(1)), None, "{view:?}");
    }
    assert!(app.ci.is_empty(), "nothing was claimed outside `ci`");

    app.history_view = app::HistoryView::Ci;
    app.view = app::View::Compact;
    assert_eq!(ci_tick(&mut app, at(12, 0, 2), after(2)), None, "compact");
    app.view = app::View::Detail;
    // The cursor's dwell starts on the first tick that wants CI.
    assert_eq!(ci_tick(&mut app, at(12, 0, 3), after(3)), None, "dwell");
    let request = ci_tick(&mut app, at(12, 0, 4), after(4)).expect("ci polls");
    let answer = ci::CiAnswer {
        runs: fresh_runs("W/\"6b13\""),
        ..ci_answer(&request)
    };
    app.apply_ci(answer, at(12, 0, 5));

    app.history_view = app::HistoryView::Info;
    assert_eq!(
        ci_tick(&mut app, at(12, 5, 0), after(300)),
        None,
        "info stops it"
    );
    assert_eq!(app.ci[&ci_key()].runs, ci_runs(), "held runs survive");
    app.history_view = app::HistoryView::Ci;
    assert!(
        ci_tick(&mut app, at(12, 5, 0), after(300)).is_some(),
        "back in `ci`, the overdue poll goes at once"
    );
}

/// A stuck CI poll is given up on by the tick only while `ci` is on
/// screen, like the trigger; back in the view the expiry runs first, so
/// the lock never holds the view's next poll.
#[test]
fn the_ci_tick_expires_a_stuck_poll_only_in_the_ci_view() {
    let mut app = ci_app();
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Info;
    app.claim_ci_poll(ci_key(), "gaurgle".into(), "fleetz".into());
    let late = std::time::Instant::now() + CI_POLL_STUCK_AFTER * 2;
    ci_tick(&mut app, at(12, 5, 0), late);
    assert!(app.ci[&ci_key()].inflight, "info leaves the CI layer alone");
    app.history_view = app::HistoryView::Ci;
    ci_tick(&mut app, at(12, 5, 0), late);
    assert!(
        app.ci[&ci_key()].freshness.last_attempt.is_some(),
        "expired as a failed attempt"
    );
    assert!(matches!(
        &app.ci[&ci_key()].freshness.outcome,
        app::Outcome::Failed(f) if f.kind == github::FailureKind::TimedOut
    ));
}

// FZ-30e: the `ci` view's handoffs to `gh` and the browser.

/// `ci_app` with its runs landed, pane 2 focused on `ci`, the cursor on
/// run `index` of the fixture (in progress, success, failure).
fn handoff_app(index: usize) -> App {
    let mut app = ci_app();
    ci_landed(&mut app, at(12, 0, 0));
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Ci;
    app.focused_panel = FocusedPanel::Detail;
    app.detail_selections[app::HistoryView::Ci.index()] = index;
    app
}

fn argv(args: &[&str]) -> RunHandoff {
    RunHandoff::Gh(args.iter().map(|a| a.to_string()).collect())
}

#[test]
fn the_highlighted_run_is_the_one_the_ci_cursor_is_on() {
    let app = handoff_app(1);
    assert_eq!(app.highlighted_ci_run(), Some(&ci_runs()[1]));
    let past = handoff_app(9);
    assert_eq!(
        past.highlighted_ci_run(),
        Some(&ci_runs()[2]),
        "clamped onto the last run, as the view draws the cursor"
    );
    assert_eq!(ci_app().highlighted_ci_run(), None, "nothing held");
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

/// The inspector opens if folded, takes focus and shows the `log`
/// body of the run under the cursor, whose failed job is the marked one.
#[test]
fn l_focuses_the_inspector_on_the_runs_log_body() {
    let mut app = handoff_app(2);
    app.inspector_folded = true;
    app.inspect_ci_log();
    assert_eq!(app.focused_panel, FocusedPanel::Inspector);
    assert!(!app.inspector_folded, "a folded inspector opens");
    assert_eq!(app.ci_body, app::CiBody::Log);
    assert!(app.inspects_ci_run());
    assert_eq!(
        app.ci_inspected.map(|(id, _)| id),
        Some(ci_runs()[2].id),
        "the highlighted run"
    );
    assert_eq!(
        app.selected_ci()
            .and_then(|(_, s)| s)
            .and_then(|s| s.expanded),
        Some(ci_runs()[2].id),
        "the poll reads the run's jobs now the inspector is open"
    );
    assert!(app.needs_redraw);
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

/// Back from `gh`, the repo polls at once rather than at its next 10 or
/// 60 s slot, so a rerun or a cancel shows up without waiting.
#[test]
fn back_from_gh_the_runs_repo_polls_at_once() {
    let mut app = handoff_app(1);
    let start = std::time::Instant::now();
    let settled = start + std::time::Duration::from_secs(1);
    ci_poll_request(&mut app, at(12, 0, 0), start);
    assert_eq!(
        ci_poll_request(&mut app, at(12, 0, 5), settled),
        None,
        "not due on its own until 12:00:10"
    );
    let target = ci::target(&app.repos[0]).unwrap();
    let request =
        ci_poll_after_handoff(&mut app, target.clone(), at(12, 0, 5)).expect("polled at once");
    assert_eq!(request.key, ci_key());
    assert!(app.ci[&ci_key()].inflight);
    assert_eq!(
        ci_poll_after_handoff(&mut app, target, at(12, 0, 6)),
        None,
        "one poll out at a time"
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

// --- FZ-86: pane 1's order follows activity after a refresh ---

/// A row of Gaurgle's at `location`, with no path, so no poll fetches it.
fn owned_row(name: &str, location: RepoLocation) -> RepoStatus {
    RepoStatus {
        name: name.to_string(),
        owner: Some("Gaurgle".to_string()),
        location,
        ..test_repo()
    }
}

/// Land a successful poll whose dataset lists `names` newest push first,
/// the order the GraphQL `orderBy` and `merge_sources` give it.
fn poll_newest_first(app: &mut App, names: &[&str]) {
    let repos = names
        .iter()
        .map(|name| github::GithubRepo {
            owner: Some("Gaurgle".to_string()),
            ..gh(name, "2026-11-06T10:00:00Z")
        })
        .collect();
    poll_with(
        app,
        Ok(Box::new(github::GithubFetch {
            repos,
            calendar: None,
            has_pr_counts: true,
            origin: github::Origin::Network,
            meta: Default::default(),
        })),
    );
}

fn shown(app: &App) -> Vec<String> {
    app.visible_repos()
        .iter()
        .map(|(_, r)| r.name.clone())
        .collect()
}

fn app_a_b_c_on_b() -> App {
    let rows = ["a", "b", "c"].map(|n| owned_row(n, RepoLocation::Primary));
    let mut app = App::new(rows.to_vec(), "h".to_string(), 120, 44);
    app.selected = 1;
    app
}

/// The reported case: launched A B C, then C is pushed. The poll that
/// says so re-orders the list as a launch would, and the cursor stays on
/// B rather than on whatever row now sits second.
#[test]
fn a_poll_moves_the_newest_pushed_repo_up_and_the_cursor_stays_on_its_repo() {
    let mut app = app_a_b_c_on_b();
    poll_newest_first(&mut app, &["c", "a", "b"]);
    assert_eq!(shown(&app), ["c", "a", "b"]);
    assert_eq!(app.selected_repo().map(|r| r.name.as_str()), Some("b"));
    assert!(app.needs_redraw);
}

/// A text search ranks its own list and breaks ties by row order, so an
/// answer landing under it must not reshuffle the results being read.
#[test]
fn a_poll_under_a_text_search_leaves_its_ranked_order_alone() {
    let mut app = app_a_b_c_on_b();
    // Every row matches on its owner alike: the ranking ties throughout.
    app.search_query = "gaurgle".to_string();
    let before = shown(&app);
    assert_eq!(before.len(), 3, "premise: the query matches every row");
    poll_newest_first(&mut app, &["c", "a", "b"]);
    assert_eq!(shown(&app), before);
    assert_eq!(app.selected_repo().map(|r| r.name.as_str()), Some("b"));
}

/// A sweep landing applies the order too: one a search held back lands
/// on the next sweep after the search ends, with the cursor on its repo.
#[test]
fn a_sweep_landing_applies_an_order_a_search_held_back() {
    let mut app = app_a_b_c_on_b();
    app.search_query = "gaurgle".to_string();
    poll_newest_first(&mut app, &["c", "a", "b"]);
    app.clear_search();
    assert_eq!(shown(&app), ["a", "b", "c"], "premise: held back");
    app.selected = 1;
    app.needs_redraw = false;

    apply_sweep_done(&mut app, chrono::Local::now());
    assert_eq!(shown(&app), ["c", "a", "b"]);
    assert_eq!(app.selected_repo().map(|r| r.name.as_str()), Some("b"));
}

/// The re-sort keeps the launch's layout: pass 3's rows stay in
/// discovery order however recent the repo one of them carries, and in
/// "also active" the clones elsewhere stay ahead of the not-cloned rows,
/// each by activity, as `assemble_view` appends them.
#[test]
fn a_poll_re_sorts_each_section_the_way_the_launch_assembled_it() {
    let mut carrier = owned_row("x", RepoLocation::LocalOnly);
    carrier.remote_url = Some("git@github.com:Gaurgle/x.git".to_string());
    let rows = vec![
        owned_row("p1", RepoLocation::Primary),
        owned_row("p2", RepoLocation::Primary),
        owned_row("e1", RepoLocation::Elsewhere),
        owned_row("e2", RepoLocation::Elsewhere),
        owned_row("plain", RepoLocation::LocalOnly),
        carrier,
        owned_row("n1", RepoLocation::NotCloned),
        owned_row("n2", RepoLocation::NotCloned),
    ];
    let mut app = App::new(rows, "h".to_string(), 120, 44);
    poll_newest_first(&mut app, &["n2", "x", "e2", "n1", "p2", "e1", "p1"]);
    assert_eq!(
        shown(&app),
        ["p2", "p1", "plain", "x", "e2", "e1", "n2", "n1"]
    );
}
