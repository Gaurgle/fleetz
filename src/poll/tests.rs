use super::*;
use crate::event::LoadKind;
use crate::model::RepoLocation;
use crate::tests::{
    alice_app, answer, answer_at, app_with, app_with_pending_repo, assert_carries_nothing,
    assert_poll_fields, at, budget_app, carrier_row, ci_answer, ci_app, ci_failure_lands, ci_key,
    ci_landed, ci_runs, cloned, entry, err_with, failure, fetch_failed, fetch_on, fleet_app,
    fresh_runs, gh, hist_commit, hist_repo, id_assemble, id_entry, id_repo, key, land, land_at,
    land_fetched, land_primary_limit, launch, local, next_due, next_note_answer, offline_outcome,
    ok_with, ok_with_pending_r, poll, poll_entries, poll_says_passed, poll_with, reading,
    reload_lands, row, row_at, secs, stale_answer, test_repo, watching, wt_scratch, wt_sh,
};
use crate::window_state;
use clap::Parser;

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

#[test]
fn the_rebuild_rule_needs_a_known_poll_account() {
    assert!(!account_rebuild_needed(None, None));
    assert!(!account_rebuild_needed(Some("A"), None));
    assert!(!account_rebuild_needed(Some("A"), Some("A")));
    assert!(account_rebuild_needed(Some("A"), Some("B")));
    assert!(account_rebuild_needed(None, Some("A")));
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
