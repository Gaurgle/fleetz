use super::*;

#[test]
fn identity_key_lowers_both_parts_and_keeps_no_owner_apart() {
    assert_eq!(
        identity_key(Some("GaurGLE"), "FleetZ"),
        (Some("gaurgle".to_string()), "fleetz".to_string())
    );
    assert_eq!(identity_key(None, "FleetZ"), (None, "fleetz".to_string()));
    assert_ne!(
        identity_key(Some("gaurgle"), "fleetz"),
        identity_key(None, "fleetz")
    );
}

#[test]
fn owner_parsed_from_every_remote_url_form() {
    for (url, want) in [
        ("git@github.com:Gaurgle/fleetz.git", "Gaurgle"),
        ("git@github.com:Gaurgle/fleetz", "Gaurgle"),
        ("https://github.com/Gaurgle/fleetz.git", "Gaurgle"),
        ("https://github.com/Gaurgle/fleetz/", "Gaurgle"),
        ("ssh://git@github.com/acme/service.git", "acme"),
        // Self-hosted, and a non-default port, which must not be read as
        // the scp-style path separator.
        ("ssh://git@git.internal:2222/acme/service.git", "acme"),
        ("https://gitlab.example.com/team/tool.git", "team"),
    ] {
        assert_eq!(
            owner_from_remote_url(url).as_deref(),
            Some(want),
            "parsing {url}"
        );
    }
}

#[test]
fn owner_is_none_for_urls_without_an_owner_repo_pair() {
    for url in [
        "",
        "not a url",
        "https://github.com",
        "https://github.com/",
        // An owner with no repo segment is not an owner.
        "https://github.com/Gaurgle",
        "git@github.com:",
        "/local/path/repo",
    ] {
        assert_eq!(owner_from_remote_url(url), None, "parsing {url:?}");
    }
}

#[test]
fn repo_parsed_with_the_git_suffix_stripped() {
    for url in [
        "git@github.com:Gaurgle/portfolio-site.git",
        "git@github.com:Gaurgle/portfolio-site",
        "https://github.com/Gaurgle/portfolio-site/",
        "ssh://git@github.com/Gaurgle/portfolio-site.git",
        // An SSH host alias from ~/.ssh/config: the host is never checked.
        "git@github-work:Gaurgle/portfolio-site.git",
    ] {
        assert_eq!(
            repo_from_remote_url(url),
            Some(("Gaurgle".to_string(), "portfolio-site".to_string())),
            "parsing {url}"
        );
    }
}

/// The S10 forms of the identity design, each pinned to the exact
/// `(owner, repo)` it yields, spelling included: case is kept here and
/// only folded by whoever compares.
#[test]
fn repo_parsed_from_every_s10_remote_form() {
    for (url, owner, repo) in [
        ("git@github.com:Gaurgle/fleetz.git", "Gaurgle", "fleetz"),
        ("git@github.com:Gaurgle/fleetz", "Gaurgle", "fleetz"),
        ("https://github.com/Gaurgle/fleetz.git", "Gaurgle", "fleetz"),
        ("https://github.com/Gaurgle/fleetz", "Gaurgle", "fleetz"),
        ("https://github.com/gaurgle/FLEETZ/", "gaurgle", "FLEETZ"),
        (
            "ssh://git@github.com:22/Gaurgle/fleetz.git",
            "Gaurgle",
            "fleetz",
        ),
        ("git@github-work:Gaurgle/fleetz.git", "Gaurgle", "fleetz"),
    ] {
        assert_eq!(
            repo_from_remote_url(url),
            Some((owner.to_string(), repo.to_string())),
            "parsing {url}"
        );
    }
}

/// Two edges the identity design could only reason about, pinned as they
/// behave: a `file://` URL reads its first two path segments as owner and
/// repo, and only a lowercase `.git` suffix is stripped. Neither is a
/// claim that this is right, only that it is what discovery sees today.
#[test]
fn file_url_and_uppercase_git_suffix_parse_as_they_always_have() {
    assert_eq!(
        repo_from_remote_url("file:///Users/me/repo"),
        Some(("Users".to_string(), "me".to_string()))
    );
    assert_eq!(
        repo_from_remote_url("git@github.com:Gaurgle/fleetz.GIT"),
        Some(("Gaurgle".to_string(), "fleetz.GIT".to_string()))
    );
}

/// The host comes out of every S10 form without user or port, and an
/// SSH alias is carried as written: nothing resolves or checks it.
#[test]
fn remote_ref_carries_the_host_of_every_s10_form() {
    for (url, host, owner, name) in [
        (
            "git@github.com:Gaurgle/fleetz.git",
            "github.com",
            "Gaurgle",
            "fleetz",
        ),
        (
            "git@github.com:Gaurgle/fleetz",
            "github.com",
            "Gaurgle",
            "fleetz",
        ),
        (
            "https://github.com/Gaurgle/fleetz.git",
            "github.com",
            "Gaurgle",
            "fleetz",
        ),
        (
            "https://github.com/Gaurgle/fleetz",
            "github.com",
            "Gaurgle",
            "fleetz",
        ),
        (
            "https://github.com/gaurgle/FLEETZ/",
            "github.com",
            "gaurgle",
            "FLEETZ",
        ),
        (
            "ssh://git@github.com:22/Gaurgle/fleetz.git",
            "github.com",
            "Gaurgle",
            "fleetz",
        ),
        (
            "git@github-work:Gaurgle/fleetz.git",
            "github-work",
            "Gaurgle",
            "fleetz",
        ),
        ("file:///Users/me/repo", "", "Users", "me"),
    ] {
        assert_eq!(
            RemoteRef::parse(url),
            Some(RemoteRef {
                host: host.to_string(),
                owner: owner.to_string(),
                name: name.to_string(),
            }),
            "parsing {url}"
        );
    }
}

#[test]
fn remote_ref_keeps_the_url_spelling_for_display() {
    let mixed = RemoteRef::parse("https://GitHub.com/gaurgle/FLEETZ/").unwrap();
    assert_eq!(
        (
            mixed.host.as_str(),
            mixed.owner.as_str(),
            mixed.name.as_str()
        ),
        ("GitHub.com", "gaurgle", "FLEETZ")
    );
}

#[test]
fn remote_ref_key_folds_case_and_leaves_the_spelling_alone() {
    let mixed = RemoteRef::parse("https://GitHub.com/gaurgle/FLEETZ/").unwrap();
    let lower = RemoteRef::parse("git@github.com:Gaurgle/fleetz.git").unwrap();
    assert_eq!(
        mixed.key(),
        RemoteRef {
            host: "github.com".to_string(),
            owner: "gaurgle".to_string(),
            name: "fleetz".to_string(),
        }
    );
    assert_eq!(mixed.key(), lower.key(), "one repo, one key");
    assert_eq!(mixed.name, "FLEETZ", "the parsed spelling is kept");
    assert_ne!(
        mixed.key(),
        RemoteRef::parse("git@github.com:someone/fleetz.git")
            .unwrap()
            .key(),
        "the owner is part of the key"
    );
}

#[test]
fn remote_ref_is_none_where_repo_from_remote_url_is() {
    for url in [
        "",
        "not a url",
        "https://github.com",
        "https://github.com/Gaurgle",
        "git@github.com:",
        "/local/path/repo",
    ] {
        assert_eq!(RemoteRef::parse(url), None, "parsing {url:?}");
    }
}

#[test]
fn owner_display_prefers_the_api_over_the_remote_url() {
    // The API login is authoritative: a remote can point at a fork or a
    // mirror whose path no longer matches the owner GitHub reports.
    let mut repo = make_repo("fleetz", 0, 0, 0);
    repo.remote_url = Some("git@github.com:someone-else/fleetz.git".to_string());
    assert_eq!(repo.owner_display().as_deref(), Some("someone-else"));

    repo.owner = Some("Gaurgle".to_string());
    assert_eq!(repo.owner_display().as_deref(), Some("Gaurgle"));
}

#[test]
fn owner_display_is_none_without_api_or_remote() {
    let repo = make_repo("orphan", 0, 0, 0);
    assert_eq!(repo.owner_display(), None);
}

/// A repo whose behind count is spread over several branches, as
/// `long_status` sees it.
fn repo_behind_on(branches: &[(&str, u32)]) -> RepoStatus {
    let total = branches.iter().map(|(_, n)| n).sum();
    let mut repo = make_repo("spread", total, 0, 0);
    repo.branches = branches
        .iter()
        .map(|(name, behind)| BranchStatus {
            name: (*name).to_string(),
            upstream: Some(format!("origin/{name}")),
            behind: *behind,
            ahead: 0,
            is_head: false,
            tip: String::new(),
        })
        .collect();
    repo
}

#[test]
fn long_status_names_the_spread_when_several_branches_are_behind() {
    // The bare "40 behind" reads as one pull away, and on a repo whose
    // current branch is level git says there is nothing to pull.
    let r = repo_behind_on(&[("beta", 21), ("feature/who-goes-first", 19)]);
    assert_eq!(r.long_status(), "40 behind on 2 branches");
}

#[test]
fn long_status_leaves_a_single_behind_branch_unqualified() {
    let r = repo_behind_on(&[("main", 3), ("beta", 0)]);
    assert_eq!(r.long_status(), "3 behind");
}

fn make_repo(name: &str, behind: u32, ahead: u32, dirty: u32) -> RepoStatus {
    RepoStatus {
        last_local: None,
        daily: None,
        daily_key: None,
        name: name.to_string(),
        path: Some(format!("~/repos/{name}")),
        location: RepoLocation::Primary,
        branches: vec![],
        commits: vec![],
        behind,
        ahead,
        dirty,
        pr_count: 0,
        prs: vec![],
        ci_status: CiStatus::Unknown,
        ci_workflow: None,
        ci_duration: None,
        groups: vec![],
        stash_count: 0,
        stash_entries: vec![],
        worktrees: vec![],
        reflog: vec![],
        graph: vec![],
        graph_expanded: Vec::new(),
        repo_kind: RepoKind::Unknown,
        owner: None,
        remote_url: None,
        remote_only_branches: vec![],
        remote_branches: vec![],
        work_key: None,
        fetch: FetchFacts::default(),
        authors: Default::default(),
    }
}

/// FZ-56: the cached author set is the distinct, non-empty authors.
#[test]
fn commit_authors_are_distinct_and_skip_empty_names() {
    let mut r = make_repo("r", 0, 0, 0);
    assert!(r.commit_authors().is_empty(), "no commits, no authors");
    let mut r2 = make_repo("r", 0, 0, 0);
    r2.commits = ["Pia Holm", "", "Andreas Roos", "Pia Holm"]
        .into_iter()
        .map(|author| Commit {
            hash: String::new(),
            message: String::new(),
            body: String::new(),
            author: author.to_string(),
            date: None,
            insertions: 0,
            deletions: 0,
            files: vec![],
        })
        .collect();
    assert_eq!(r2.commit_authors(), ["Andreas Roos", "Pia Holm"]);
    r.apply_local_sweep(r2);
    assert_eq!(
        r.commit_authors(),
        ["Andreas Roos", "Pia Holm"],
        "rebuilt after a sweep"
    );
}

#[test]
fn synced_repo_reports_synced() {
    let r = make_repo("clean", 0, 0, 0);
    assert!(r.is_synced());
    assert_eq!(r.short_status(), "✓");
    assert_eq!(r.long_status(), "synced");
}

#[test]
fn behind_repo_status_strings() {
    let r = make_repo("behind", 3, 0, 0);
    assert!(!r.is_synced());
    assert_eq!(r.short_status(), "3↓");
    assert_eq!(r.long_status(), "3 behind");
}

#[test]
fn mixed_status_strings() {
    let r = make_repo("messy", 2, 1, 4);
    assert_eq!(r.short_status(), "2↓ 1↑ 4!");
    assert_eq!(r.long_status(), "2 behind, 1 ahead, 4 uncommitted");
}

/// `short_status` is `counters_text` on the repo's own counts, so a
/// worktree's counters spelled from bare numbers read like a repo's.
#[test]
fn counters_text_spells_a_repos_counts_the_way_short_status_does() {
    for (behind, ahead, dirty) in [(0, 0, 0), (3, 0, 0), (0, 2, 0), (0, 0, 4), (2, 1, 4)] {
        let r = make_repo("r", behind, ahead, dirty);
        assert_eq!(counters_text(behind, ahead, dirty), r.short_status());
    }
    assert_eq!(counters_text(2, 1, 4), "2↓ 1↑ 4!");
    assert_eq!(counters_text(0, 0, 0), "✓");
}

#[test]
fn ci_and_sync_never_share_a_glyph() {
    // The original double-checkmark bug: a clean repo with green CI drew
    // two identical green ✓. CI owns the ringed disk, sync owns the check.
    let mut r = make_repo("clean", 0, 0, 0);
    r.ci_status = CiStatus::Passed;
    assert_eq!(r.short_status(), "✓");
    assert_eq!(r.ci_status.symbol(), "◉");
    assert_ne!(r.short_status(), r.ci_status.symbol());
}

#[test]
fn every_known_ci_state_is_the_same_shape() {
    // Deliberate: the states differ by color, not shape, so the disk can
    // never be mistaken for a sync status. Unknown stays empty so a repo
    // with no CI reserves no column.
    assert_eq!(CiStatus::Passed.symbol(), "◉");
    assert_eq!(CiStatus::Failed.symbol(), "◉");
    assert_eq!(CiStatus::Pending.symbol(), "◉");
    assert_eq!(CiStatus::Unknown.symbol(), "");
}

#[test]
fn not_cloned_repo_never_reads_as_synced() {
    // Regression: all three counters are zero for a repo with no working
    // tree, which used to render as a green "✓ / synced" -- visually
    // identical to a repo that really was up to date.
    let mut r = make_repo("ghost", 0, 0, 0);
    r.location = RepoLocation::NotCloned;
    r.path = None;

    assert!(r.is_not_cloned());
    assert!(!r.is_synced());
    assert_eq!(r.short_status(), "not cloned");
    assert_eq!(r.long_status(), "not cloned");
}

#[test]
fn summary_counts_correctly() {
    let repos = [
        make_repo("a", 3, 0, 0),
        make_repo("b", 0, 0, 0),
        make_repo("c", 0, 2, 1),
        {
            let mut r = make_repo("d", 0, 0, 0);
            r.location = RepoLocation::NotCloned;
            r.path = None;
            r
        },
    ];
    let s = Summary::from_refs(repos.iter());
    assert_eq!(s.behind_count, 1);
    assert_eq!(s.ahead_count, 1);
    assert_eq!(s.dirty_count, 1);
    assert_eq!(s.synced_count, 1);
    assert_eq!(s.not_cloned_count, 1);
}

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
