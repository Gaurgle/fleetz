use super::*;

fn at(h: u32, m: u32) -> DateTime<Local> {
    use chrono::TimeZone;
    Local
        .with_ymd_and_hms(2026, 10, 1, h, m, 0)
        .single()
        .expect("unambiguous local time")
}

fn offline() -> crate::github::Failure {
    crate::github::Failure::new(crate::github::FailureKind::Offline, "no route")
}

/// Rule 1: reading the cache sets the data time from the file and is not
/// an attempt, so the retry clock stays unset and the poll is due.
#[test]
fn a_cache_read_is_not_an_attempt() {
    let mut f = GithubFreshness::default();
    f.loaded(
        &DatasetSource::Cache {
            written: Some(at(6, 0)),
        },
        at(9, 0),
    );
    assert_eq!(f.data_time, DataTime::At(at(6, 0)));
    assert_eq!(f.last_attempt, None);
    assert_eq!(f.outcome, Outcome::NotAttempted);
    assert!(f.retry_due(at(9, 0), 90));
}

/// An unreadable write time stays unknown, never the time it was read.
#[test]
fn an_unreadable_cache_age_stays_unknown() {
    let mut f = GithubFreshness::default();
    f.loaded(&DatasetSource::Cache { written: None }, at(9, 0));
    assert_eq!(f.data_time, DataTime::Unknown);
    f.failed(offline(), at(9, 1));
    assert_eq!(f.data_time, DataTime::Unknown, "a failure made the age up");
}

/// Rule 3: only a success advances the data time. Every kind of failure,
/// the give-up among them, leaves it where it was.
#[test]
fn a_failure_of_any_kind_leaves_the_data_time() {
    use crate::github::{Failure, FailureKind};
    for kind in [
        FailureKind::Offline,
        FailureKind::Http,
        FailureKind::Api,
        FailureKind::Auth,
        FailureKind::Malformed,
    ] {
        let mut f = GithubFreshness::default();
        f.succeeded(at(9, 0));
        f.failed(Failure::new(kind, "x"), at(9, 5));
        assert_eq!(f.data_time, DataTime::At(at(9, 0)), "{kind:?}");
        assert_eq!(f.outcome, Outcome::Failed(Failure::new(kind, "x")));
    }
    let mut f = GithubFreshness::default();
    f.succeeded(at(9, 0));
    f.gave_up(std::time::Duration::from_secs(60), at(9, 5));
    assert_eq!(f.data_time, DataTime::At(at(9, 0)));
}

/// Rule 6: every finished attempt advances the retry clock, whatever it
/// ended in, so a failure waits out an interval like a success does.
#[test]
fn every_finished_attempt_advances_the_retry_clock() {
    let mut f = GithubFreshness::default();
    f.failed(offline(), at(9, 0));
    assert_eq!(f.last_attempt, Some(at(9, 0)));
    assert!(!f.retry_due(at(9, 1), 90));
    assert!(f.retry_due(at(9, 2), 90));
    f.gave_up(std::time::Duration::from_secs(60), at(9, 10));
    assert_eq!(f.last_attempt, Some(at(9, 10)));
    assert!(matches!(
        &f.outcome,
        Outcome::Failed(x) if x.kind == crate::github::FailureKind::TimedOut
    ));
}

/// Rule 7: a success clears the failure and makes the data current.
#[test]
fn a_success_clears_the_failure() {
    let mut f = GithubFreshness::default();
    f.failed(offline(), at(9, 0));
    assert!(f.is_failing());
    f.succeeded(at(9, 2));
    assert!(!f.is_failing());
    assert_eq!(f.outcome, Outcome::Success);
    assert_eq!(f.data_time, DataTime::At(at(9, 2)));
    assert_eq!(f.last_attempt, Some(at(9, 2)));
}

/// The `R` fallback row of the transition table: the rows are rebuilt
/// from the cache, so the data time is the file's, and the attempt is a
/// failure.
#[test]
fn a_reload_fallback_takes_the_files_time_and_records_the_failure() {
    let mut f = GithubFreshness::default();
    f.succeeded(at(9, 0));
    f.loaded(
        &DatasetSource::Fallback {
            failure: offline(),
            written: Some(at(9, 0)),
        },
        at(10, 0),
    );
    assert_eq!(f.data_time, DataTime::At(at(9, 0)));
    assert_eq!(f.last_attempt, Some(at(10, 0)));
    assert_eq!(f.outcome, Outcome::Failed(offline()));
}

/// A load with no GitHub fetch behind it (`-d`, a window apply) says
/// nothing about freshness.
#[test]
fn a_local_load_changes_no_freshness_fact() {
    let mut f = GithubFreshness::default();
    f.failed(offline(), at(9, 0));
    let before = f.clone();
    f.loaded(&DatasetSource::Local, at(10, 0));
    assert_eq!(f, before);
}

/// A load that went to GitHub supersedes every poll started before it;
/// a cache read or a local load supersedes none.
#[test]
fn only_a_load_from_github_supersedes_earlier_polls() {
    for (source, supersedes) in [
        (DatasetSource::Network, true),
        (
            DatasetSource::Fallback {
                failure: offline(),
                written: None,
            },
            true,
        ),
        (DatasetSource::Cache { written: None }, false),
        (DatasetSource::Local, false),
    ] {
        let mut f = GithubFreshness::default();
        let poll = f.begin_attempt();
        f.loaded(&source, at(9, 0));
        assert_eq!(!f.accept_answer(poll), supersedes, "{source:?}");
    }
}

/// Rule 10: an answer is applied only when no newer attempt's answer
/// has been, in either completion order.
#[test]
fn an_answer_older_than_the_one_applied_is_discarded() {
    let mut f = GithubFreshness::default();
    let first = f.begin_attempt();
    let second = f.begin_attempt();
    assert!(f.is_latest(second) && !f.is_latest(first));
    assert!(f.accept_answer(second));
    assert!(!f.accept_answer(first), "an older answer was applied");

    let mut g = GithubFreshness::default();
    let first = g.begin_attempt();
    let second = g.begin_attempt();
    assert!(g.accept_answer(first), "nothing newer had landed");
    assert!(g.accept_answer(second));
}

/// D3: the startup path reads the two numbers the poll decisions used to
/// take a `Config` for. A configured interval under the floor is raised,
/// and the poll is priced at the configured `repo_limit`.
#[test]
fn from_config_floors_the_interval_and_prices_the_poll() {
    let sync = GithubSync::from_config(
        &Config {
            github_poll_interval: 1,
            repo_limit: 100,
            ..Config::default()
        },
        Box::new(ClockJitter::new()),
    );
    assert_eq!(sync.interval_secs(), 15);
    assert_eq!(sync.poll_cost(), crate::github::poll_cost(100));
}
