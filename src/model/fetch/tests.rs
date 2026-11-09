use super::*;
use crate::model::tests::make_repo;

// --- FZ-04: the fetch facts (docs/fetch-outcome-contract.md) ---

fn at(h: u32) -> DateTime<chrono::Local> {
    use chrono::TimeZone;
    chrono::Local
        .with_ymd_and_hms(2026, 10, 1, h, 0, 0)
        .unwrap()
}

fn failed(kind: crate::git::FetchFailureKind) -> crate::git::FetchOutcome {
    crate::git::FetchOutcome::Failed(crate::git::FetchFailure { kind, detail: None })
}

/// Rule 3: a failure records the attempt and leaves the last success
/// where it was.
#[test]
fn only_a_successful_fetch_advances_the_last_success() {
    let mut facts = FetchFacts::default();
    facts.record(&crate::git::FetchOutcome::Succeeded, at(9));
    facts.record(&failed(crate::git::FetchFailureKind::Unreachable), at(10));
    assert_eq!(facts.last_success, Some(at(9)));
    assert_eq!(facts.last_attempt, Some(at(10)));
    assert!(facts.failure().is_some());
}

/// Rule 3: unknown stays unknown through a failure.
#[test]
fn a_failure_with_no_known_success_leaves_it_unknown() {
    let mut facts = FetchFacts::from_disk(None);
    facts.record(&failed(crate::git::FetchFailureKind::NoRemote), at(10));
    assert_eq!(facts.last_success, None);
}

/// Recovery: the next fetch that works clears the failure.
#[test]
fn a_success_after_a_failure_recovers() {
    let mut facts = FetchFacts::default();
    facts.record(&failed(crate::git::FetchFailureKind::Auth), at(9));
    facts.record(&crate::git::FetchOutcome::Succeeded, at(10));
    assert_eq!(facts.failure(), None);
    assert_eq!(facts.last_success, Some(at(10)));
}

/// Rule 2: an update that did not fetch says nothing about fetching.
#[test]
fn a_sweep_that_did_not_fetch_changes_no_fetch_fact() {
    let mut facts = FetchFacts::default();
    facts.record(&failed(crate::git::FetchFailureKind::TimedOut), at(9));
    let before = facts.clone();
    facts.record(&crate::git::FetchOutcome::NotAttempted, at(10));
    assert_eq!(facts, before);
}

/// Rule 2 under the FZ-02 ownership split: the fetch facts are their own
/// owner, so a sweep result never carries its snapshot's copy onto the
/// row, in either direction.
#[test]
fn a_local_sweep_does_not_carry_the_fetch_facts() {
    let mut row = make_repo("r", 0, 0, 0);
    row.fetch
        .record(&failed(crate::git::FetchFailureKind::Unreachable), at(10));
    let mut fresh = make_repo("r", 2, 0, 0);
    fresh.fetch = FetchFacts::from_disk(Some(std::time::SystemTime::now()));
    let kept = row.fetch.clone();
    row.apply_local_sweep(fresh);
    assert_eq!(row.behind, 2, "the sweep's own fields still land");
    assert_eq!(row.fetch, kept);
}

/// A time-window apply rebuilds every row and fetches nothing. The
/// failure has to survive it, or the counts read as current again.
#[test]
fn a_load_that_did_not_fetch_keeps_the_sessions_failure() {
    let mut session = FetchFacts::from_disk(None);
    session.record(&crate::git::FetchOutcome::Succeeded, at(9));
    session.record(&failed(crate::git::FetchFailureKind::Auth), at(10));
    // The failure emptied FETCH_HEAD, so the disk knows nothing.
    let loaded = FetchFacts::from_disk(None);
    assert_eq!(session.carried_into(&loaded), session);
}

/// An `R` whose own fetch worked clears the failure; one whose fetch
/// failed keeps the last success the session saw.
#[test]
fn a_load_that_fetched_records_its_fetch_on_the_sessions_facts() {
    let mut session = FetchFacts::default();
    session.record(&crate::git::FetchOutcome::Succeeded, at(9));
    session.record(&failed(crate::git::FetchFailureKind::Auth), at(10));

    let mut ok = FetchFacts::from_disk(None);
    ok.record(&crate::git::FetchOutcome::Succeeded, at(11));
    let carried = session.carried_into(&ok);
    assert_eq!(carried.failure(), None);
    assert_eq!(carried.last_success, Some(at(11)));

    let mut bad = FetchFacts::from_disk(None);
    bad.record(&failed(crate::git::FetchFailureKind::Unreachable), at(11));
    let carried = session.carried_into(&bad);
    assert_eq!(
        carried.failure().map(|f| f.kind),
        Some(crate::git::FetchFailureKind::Unreachable)
    );
    assert_eq!(
        carried.last_success,
        Some(at(9)),
        "the session's success was lost"
    );
}

/// An `R` whose fetch finished before a sweep's fetch of the same repo
/// landed: the newer outcome and attempt time stay.
#[test]
fn a_load_fetch_older_than_the_sessions_attempt_is_not_recorded() {
    let mut session = FetchFacts::default();
    session.record(&crate::git::FetchOutcome::Succeeded, at(11));
    let mut loaded = FetchFacts::from_disk(None);
    loaded.record(&failed(crate::git::FetchFailureKind::Unreachable), at(10));
    assert_eq!(session.carried_into(&loaded), session);
}

/// A fetch run in a terminal after the failure fixed it; the disk says so.
#[test]
fn a_newer_success_on_disk_clears_an_older_failure() {
    let mut session = FetchFacts::default();
    session.record(&failed(crate::git::FetchFailureKind::Auth), at(10));
    let loaded = FetchFacts {
        last_success: Some(at(11)),
        ..FetchFacts::default()
    };
    let carried = session.carried_into(&loaded);
    assert_eq!(carried.failure(), None);
    assert_eq!(carried.last_success, Some(at(11)));
}

// --- FZ-35b: the size hold ---

const MIB: u64 = 1024 * 1024;

fn with_baseline(kb: Option<u64>) -> FetchFacts {
    FetchFacts {
        disk_baseline: kb,
        ..FetchFacts::default()
    }
}

/// Criterion 2: growth past the cap holds the repo and names the growth.
#[test]
fn a_disk_usage_grown_past_the_cap_holds_the_repo() {
    let mut facts = with_baseline(Some(1_000));
    facts.assess_growth(Some(1_000 + 300 * 1024), Some(200 * MIB));
    assert_eq!(facts.size_hold(), Some(SizeHold::Grown(300 * MIB)));
}

/// Growth up to the cap, an unchanged and a shrunken size decide nothing.
#[test]
fn a_disk_usage_within_the_cap_unchanged_or_shrunken_holds_nothing() {
    for current in [1_000 + 200 * 1024, 1_000, 10] {
        let mut facts = with_baseline(Some(1_000));
        facts.assess_growth(Some(current), Some(200 * MIB));
        assert_eq!(facts.size_hold(), None, "current {current} kB");
    }
}

/// A missing current value, an unknown baseline or no cap decide nothing,
/// and lift an earlier hold of this kind.
#[test]
fn a_missing_disk_usage_baseline_or_cap_holds_nothing() {
    let mut facts = with_baseline(Some(1_000));
    facts.assess_growth(Some(900_000), Some(MIB));
    assert!(facts.size_hold().is_some());
    facts.assess_growth(None, Some(MIB));
    assert_eq!(facts.size_hold(), None, "missing current value");
    let mut unknown = with_baseline(None);
    unknown.assess_growth(Some(900_000), Some(MIB));
    assert_eq!(unknown.size_hold(), None, "unknown baseline");
    let mut uncapped = with_baseline(Some(1_000));
    uncapped.assess_growth(Some(900_000), None);
    assert_eq!(uncapped.size_hold(), None, "fetch_max_mb = 0");
}

/// Criterion 3: a fetch cut at the cap holds the repo until a success,
/// whatever GitHub says about growth.
#[test]
fn a_fetch_cut_at_the_cap_holds_until_a_success() {
    let mut facts = with_baseline(Some(1_000));
    facts.assess_growth(Some(900_000), Some(MIB));
    facts.record(
        &crate::git::FetchOutcome::Failed(crate::git::FetchFailure::too_big(2 * MIB)),
        at(10),
    );
    assert_eq!(facts.size_hold(), Some(SizeHold::OverCap(2 * MIB)));
    facts.record(&crate::git::FetchOutcome::Succeeded, at(11));
    assert_eq!(facts.size_hold(), None);
}

/// Criterion 4: an `R` whose own fetch succeeded hands its baseline to
/// the row it replaces; one that fetched nothing keeps the row's.
#[test]
fn a_load_carries_the_baseline_of_its_own_successful_fetch() {
    let session = with_baseline(Some(1_000));
    let mut loaded = with_baseline(Some(5_000));
    loaded.record(&crate::git::FetchOutcome::Succeeded, at(10));
    assert_eq!(session.carried_into(&loaded).disk_baseline, Some(5_000));
    let untouched = FetchFacts::from_disk(None);
    assert_eq!(session.carried_into(&untouched).disk_baseline, Some(1_000));
}

/// Only a `TooBig` failure carries a byte count, and only in the shape
/// `too_big` wrote.
#[test]
fn only_a_too_big_failure_reports_received_bytes() {
    assert_eq!(FetchFailure::too_big(0).received_bytes(), Some(0));
    let other = FetchFailure {
        kind: FetchFailureKind::Other,
        detail: Some(FetchFailure::too_big(5).detail.unwrap()),
    };
    assert_eq!(other.received_bytes(), None);
    let mangled = FetchFailure {
        kind: FetchFailureKind::TooBig,
        detail: Some("lots".to_string()),
    };
    assert_eq!(mangled.received_bytes(), None);
}
