use super::*;
use crate::engine::PROGRESS_EVENT_GAP;
use crate::github;
use crate::tests::{
    MIB, SWEPT_PATH, answer, app_a_b_c_on_b, app_with_pending_repo, assert_poll_fields,
    assert_sweep_fields, fetch_failed, fleet_app, gh, hold_sizes, id_repo, key, land,
    missing_paths_app, ok_with, poll, poll_newest_first, poll_says_passed, poll_with,
    r_during_a_watch_sweep, rebuilt_from, shown, test_repo, timed_out_at, wt_scratch, wt_sh,
};

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
