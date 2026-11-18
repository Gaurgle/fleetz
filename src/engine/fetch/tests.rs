use super::*;
use crate::app::tests::fetch_failed;

/// Regression: the `r` spinner keyed on the local sweep alone, so it
/// stopped while the GitHub poll and the fetches it set off were still
/// landing rows one at a time.
#[test]
fn a_manual_refresh_spins_until_its_poll_and_fetches_land() {
    let mut fetch = FetchState::new(None);
    fetch.set_sweep_started(Some(std::time::Instant::now()));
    fetch.set_sweep_manual(true);
    fetch.await_poll(true);
    assert!(fetch.is_refreshing());

    // The local sweep finishing is not the end of it.
    fetch.set_sweep_started(None);
    assert!(fetch.is_refreshing(), "stopped before the poll landed");

    fetch.settle_refresh_poll(["~/repos/repo-0".to_string(), "~/repos/repo-2".to_string()]);
    assert!(fetch.is_refreshing(), "stopped before the fetches landed");

    fetch.settle_refresh_fetch("~/repos/repo-0", &crate::git::FetchOutcome::Succeeded);
    assert!(fetch.is_refreshing());
    fetch.settle_refresh_fetch("~/repos/repo-2", &crate::git::FetchOutcome::Succeeded);
    assert!(!fetch.is_refreshing());
    assert!(fetch.refresh_fetches_started().is_none());
}

#[test]
fn a_manual_refresh_is_local_until_its_sweep_ends_then_remote() {
    let mut fetch = FetchState::new(None);
    assert_eq!(fetch.refresh_phase(), None);
    fetch.set_sweep_started(Some(std::time::Instant::now()));
    fetch.set_sweep_manual(true);
    fetch.await_poll(true);
    assert_eq!(fetch.refresh_phase(), Some(RefreshPhase::Local));
    fetch.set_sweep_started(None);
    assert_eq!(fetch.refresh_phase(), Some(RefreshPhase::Remote));
    fetch.settle_refresh_poll(std::iter::empty());
    assert_eq!(fetch.refresh_phase(), None);
}

#[test]
fn a_manual_refresh_ends_with_its_poll_when_nothing_moved() {
    let mut fetch = FetchState::new(None);
    fetch.await_poll(true);
    fetch.settle_refresh_poll(std::iter::empty());
    assert!(!fetch.is_refreshing());
    assert!(fetch.refresh_fetches_started().is_none());
}

/// The local sweep can report a repo before that repo's fetch has run.
#[test]
fn an_unfetched_update_does_not_settle_a_refresh_fetch() {
    let mut fetch = FetchState::new(None);
    fetch.await_poll(true);
    fetch.settle_refresh_poll(["~/repos/repo-0".to_string()]);
    fetch.settle_refresh_fetch("~/repos/repo-0", &crate::git::FetchOutcome::NotAttempted);
    assert!(fetch.is_refreshing());
}

/// Rule 4: a fetch that failed is still a fetch that finished, so it
/// settles the refresh waiting on it.
#[test]
fn a_failed_fetch_settles_a_refresh_fetch() {
    let mut fetch = FetchState::new(None);
    fetch.await_poll(true);
    fetch.settle_refresh_poll(["~/repos/repo-0".to_string()]);
    fetch.settle_refresh_fetch(
        "~/repos/repo-0",
        &fetch_failed(crate::git::FetchFailureKind::Unreachable),
    );
    assert!(!fetch.is_refreshing());
    assert!(fetch.refresh_fetches_started().is_none());
}

/// The fetches `r`'s poll sets off are the ones its report counts.
#[test]
fn a_manual_refresh_tallies_the_fetches_its_poll_set_off() {
    let mut fetch = FetchState::new(None);
    fetch.await_poll(true);
    fetch.settle_refresh_poll(["~/repos/repo-0".to_string(), "~/repos/repo-1".to_string()]);
    let ok = crate::git::FetchOutcome::Succeeded;
    assert_eq!(fetch.land_manual("~/repos/repo-0", "repo-0", &ok), None);
    let report = fetch.land_manual(
        "~/repos/repo-1",
        "repo-1",
        &fetch_failed(crate::git::FetchFailureKind::Auth),
    );
    assert_eq!(
        report,
        Some(FetchReport {
            total: 2,
            failed: vec![("repo-1".to_string(), crate::git::FetchFailureKind::Auth)],
        })
    );
}

/// A watch timer's poll is not a keypress: its fetches report nothing.
#[test]
fn a_poll_nobody_pressed_r_for_tallies_nothing() {
    let mut fetch = FetchState::new(None);
    fetch.settle_refresh_poll(["~/repos/repo-0".to_string()]);
    let report = fetch.land_manual(
        "~/repos/repo-0",
        "repo-0",
        &fetch_failed(crate::git::FetchFailureKind::Auth),
    );
    assert_eq!(report, None);
}

/// The watch timer's poll fetches silently: no spinner, nothing to wait on.
#[test]
fn a_poll_nobody_pressed_r_for_adds_no_refresh_fetches() {
    let mut fetch = FetchState::new(None);
    fetch.settle_refresh_poll(["~/repos/repo-0".to_string()]);
    assert!(!fetch.is_refreshing());
    assert!(fetch.refresh_fetches().is_empty());
}

#[test]
fn a_watch_timer_sweep_is_not_a_refresh() {
    let mut fetch = FetchState::new(None);
    fetch.set_sweep_started(Some(std::time::Instant::now()));
    fetch.set_sweep_manual(false);
    assert!(!fetch.is_refreshing());
}

#[test]
fn a_refresh_fetch_that_never_returns_is_given_up_on() {
    let mut fetch = FetchState::new(None);
    fetch.await_poll(true);
    fetch.settle_refresh_poll(["~/repos/repo-0".to_string()]);
    fetch.expire_refresh(std::time::Duration::from_secs(60));
    assert!(
        fetch.is_refreshing(),
        "expired a fetch still inside its limit"
    );
    fetch.set_refresh_fetches_started(Some(
        std::time::Instant::now() - std::time::Duration::from_secs(61),
    ));
    fetch.expire_refresh(std::time::Duration::from_secs(60));
    assert!(!fetch.is_refreshing());
}
