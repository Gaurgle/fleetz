use super::*;
use crate::engine::with_coverage_note;
use crate::input::{ReloadAction, reload_action};
use crate::tests::{
    alice_app, budget_app, entry, fetch_failed, fetch_on, fleet_app, gh, hold_sizes, land,
    land_account_rebuild, land_fetched, limited_now, owners_coverage, poll_says_passed, poll_with,
    remote, status_text, test_repo, wt_scratch,
};
use crate::{contributions, git};

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
