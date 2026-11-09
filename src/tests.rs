use super::*;
use crate::engine::{LoadError, SweepTarget, ViewNotes, assemble_view_in, tilde_display};
use crate::event::LoadResult;
use crate::handoff::{RunHandoff, ci_poll_after_handoff};
use crate::input::{
    apply_live_settings, click_detail, handle_settings_input, refresh_key, settings_overlay,
};
use crate::load::{apply_load_result, apply_load_result_saving_to, memory_rebuild_result};
use crate::model::RepoStatus;
use crate::poll::{
    CI_POLL_STUCK_AFTER, apply_github_poll, apply_github_poll_at, ci_poll_request,
    claim_github_poll, github_limit_until, github_poll_due,
};
use crate::sweep::{ContribProbe, apply_repo_updated, rebuild_repo_status};
use std::path::PathBuf;

/// An empty app at launch, with the loading field up as `run_event_loop`
/// puts it.
pub(crate) fn app_under_loading_field() -> App {
    let mut app = App::new(Vec::new(), String::new(), 120, 40);
    app.loading = true;
    app.loading_field = Some(ui::intro::Loading::new());
    app
}

/// Whether any row of `buf` holds the top row of the `LOADING` banner.
pub(crate) fn shows_loading_banner(buf: &ratatui::buffer::Buffer) -> bool {
    (0..buf.area.height).any(|y| {
        let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
        row.contains("█    ████ ████ ███  ███ █  █ ████")
    })
}

pub(crate) fn test_repo() -> RepoStatus {
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

pub(crate) fn key(head: &str) -> git::WorkKey {
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

// --- FZ-02: a local sweep result must not undo a newer GitHub poll ---
//
// The race, driven with controlled events: a sweep snapshots the row
// (`sweep_targets`) while CI is pending, a poll lands and says passed,
// then the sweep's result lands carrying the snapshot's GitHub fields.

pub(crate) const SWEPT_PATH: &str = "~/repos/r";

/// An app with one cloned repo whose CI is pending, as at launch.
pub(crate) fn app_with_pending_repo() -> App {
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
pub(crate) fn rebuilt_from(snapshot: &RepoStatus) -> RepoStatus {
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
pub(crate) fn poll_says_passed() -> Result<Box<github::GithubFetch>, String> {
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

pub(crate) fn poll(app: &mut App, result: Result<Box<github::GithubFetch>, String>) {
    let result = result.map_err(|message| github::FetchError {
        failure: github::Failure::new(github::FailureKind::Http, message),
        fallback: None,
        meta: Default::default(),
    });
    poll_with(app, result);
}

/// Start an attempt and land `result` as its answer.
pub(crate) fn poll_with(
    app: &mut App,
    result: Result<Box<github::GithubFetch>, github::FetchError>,
) {
    let attempt = claim_github_poll(app).expect("no poll in flight");
    answer(app, attempt, result);
}

/// Land `result` as the answer to `attempt`.
pub(crate) fn answer(
    app: &mut App,
    attempt: u64,
    result: Result<Box<github::GithubFetch>, github::FetchError>,
) {
    // No previous dataset, so no remote "moved" and nothing is spawned:
    // the receiver is never read and no runtime is needed.
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    apply_github_poll(app, attempt, result, &tx, &None);
}

pub(crate) fn assert_poll_fields(repo: &RepoStatus) {
    assert_eq!(repo.ci_status, model::CiStatus::Passed);
    assert_eq!(repo.ci_workflow.as_deref(), Some("release"));
    assert_eq!(repo.ci_duration.as_deref(), Some("1m 4s"));
    assert_eq!(repo.pr_count, 1);
    assert_eq!(repo.prs.len(), 1);
    assert_eq!(repo.prs[0].number, 7);
    assert_eq!(repo.repo_kind, model::RepoKind::Private);
}

pub(crate) fn assert_sweep_fields(repo: &RepoStatus) {
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

pub(crate) fn local(h: u32, m: u32) -> chrono::DateTime<chrono::Local> {
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

pub(crate) fn watching(app: &mut App) -> config::Config {
    app.watch_mode = true;
    config::Config {
        github_poll_interval: 90,
        ..config::Config::default()
    }
}

pub(crate) fn launch(app: &mut App, kind: LoadKind, source: app::DatasetSource) {
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

/// An `R` that went to GitHub, landing over the row `app_with_pending_repo`
/// draws, the way a reload rebuilds it: CI as the reload saw it.
pub(crate) fn reload_lands(app: &mut App, source: app::DatasetSource) {
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
pub(crate) fn stale_answer() -> Result<Box<github::GithubFetch>, github::FetchError> {
    let mut old = poll_says_passed().ok().unwrap();
    old.repos[0].ci_status = model::CiStatus::Failed;
    Ok(old)
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

// --- FZ-04: fetch outcomes land as outcomes, not as "requested" ---

pub(crate) fn fetch_failed(kind: git::FetchFailureKind) -> git::FetchOutcome {
    git::FetchOutcome::Failed(git::FetchFailure {
        kind,
        detail: Some("fatal: '/gone' does not appear to be a git repository".to_string()),
    })
}

/// `r` pressed while the watch timer's sweep and poll are both in
/// flight, which with a short `refresh_interval` is most of the time.
/// Neither is started again, so nothing is spawned and no runtime is
/// needed. Returns the poll's attempt, for its answer.
pub(crate) fn r_during_a_watch_sweep(app: &mut App) -> u64 {
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

/// An app with `n` cloned repos at `~/repos/repo-<i>`.
pub(crate) fn fleet_app(n: usize) -> App {
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

pub(crate) fn land(app: &mut App, i: usize, fetch: git::FetchOutcome) {
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

pub(crate) fn offline_outcome() -> app::Outcome {
    app::Outcome::Failed(github::Failure::new(
        github::FailureKind::Offline,
        "no route",
    ))
}

/// An app whose rows sit at paths that do not exist, so a sweep spawned
/// from a test touches nothing on disk.
pub(crate) fn missing_paths_app(n: usize, tag: &str) -> App {
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

pub(crate) fn timed_out_at(app: &mut App, i: usize) -> String {
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

pub(crate) const MIB: u64 = 1024 * 1024;

/// A row grown past the cap (the pre-check) and a row whose fetch was
/// cut at the cap, as the poll and a landing leave them.
pub(crate) fn hold_sizes(app: &mut App, grown: usize, cut: usize) {
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

pub(crate) fn gh(name: &str, pushed: &str) -> github::GithubRepo {
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

pub(crate) fn cloned(name: &str) -> RepoStatus {
    RepoStatus {
        name: name.to_string(),
        path: Some(format!("~/repos/{name}")),
        ..test_repo()
    }
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

pub(crate) fn app_with(ci: &[model::CiStatus]) -> App {
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

// --- FZ-05d: GitHub fields live on one carrier row per remote ---
//
// Scenarios of section 4 of `docs/repository-identity-design.md`. The
// load half runs through `assemble_view_in` with a temp home and root, so
// the real `$HOME` is never walked; `git_fetch: false` and a dataset
// handed in directly keep the network, the cache and `gh` out of it.

/// A repo at `dir` with one commit, `origin` set to `url` when given, and
/// a modified tracked file when `dirty`.
pub(crate) fn id_repo(dir: &Path, url: Option<&str>, dirty: bool) {
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
pub(crate) fn id_entry(
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
pub(crate) fn one_day() -> Option<chrono::DateTime<chrono::Utc>> {
    Some(chrono::Utc::now() - chrono::Duration::days(1))
}

pub(crate) async fn id_assemble(
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

pub(crate) fn row_at<'a>(repos: &'a [RepoStatus], path: &Path) -> &'a RepoStatus {
    let want = tilde_display(path);
    repos
        .iter()
        .find(|r| r.path.as_deref() == Some(want.as_str()))
        .unwrap_or_else(|| panic!("no row at {want}: {repos:#?}"))
}

pub(crate) fn not_cloned<'a>(
    repos: &'a [RepoStatus],
    owner: &str,
    name: &str,
) -> Option<&'a RepoStatus> {
    repos.iter().find(|r| {
        r.location == RepoLocation::NotCloned && r.name == name && r.owner.as_deref() == Some(owner)
    })
}

/// A row with no GitHub fields: what every non-carrier LocalOnly row is.
pub(crate) fn assert_carries_nothing(repo: &RepoStatus) {
    assert_eq!(repo.owner, None, "{} was given an owner", repo.name);
    assert_eq!(repo.ci_status, model::CiStatus::Unknown, "{}", repo.name);
    assert_eq!(repo.pr_count, 0, "{}", repo.name);
    assert!(repo.prs.is_empty(), "{}", repo.name);
    assert_eq!(repo.repo_kind, model::RepoKind::Unknown, "{}", repo.name);
}

/// Land a successful poll carrying `repos` on `app`.
pub(crate) fn poll_entries(app: &mut App, repos: Vec<github::GithubRepo>) {
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

/// `c` switches the strip's source and ctrl-c quits. Match arms are
/// ordered, so an unguarded `c` arm placed above the modifier check
/// silently turns the quit key into a pane key. That regression happened
/// once already.
#[test]
fn ctrl_c_is_matched_before_the_plain_c_arm() {
    let source = include_str!("input.rs");
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

/// The owner's viewer page (design 1.2): 50 of 95, cut off 237 days back.
pub(crate) fn owners_coverage() -> github::Coverage {
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

// --- FZ-06c: the worktree list is re-read on every sweep ---
//
// Each scenario sweeps a real repo with `rebuild_repo_status`, feeding
// every sweep the previous one's result as its snapshot, as the app does.
// Unless a test says otherwise, the row asserted on is the MAIN
// checkout's: its `worktrees` lists the linked worktrees of its clone.

pub(crate) fn wt_sh(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new(args[0])
        .args(&args[1..])
        .current_dir(dir)
        .status()
        .expect("spawn");
    assert!(status.success(), "command failed: {args:?}");
}

/// A scratch directory unique to this test and this test process.
pub(crate) fn wt_scratch(tag: &str) -> PathBuf {
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

pub(crate) fn path_arg(path: &Path) -> String {
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
pub(crate) fn hist_commit(dir: &Path, day: &str, message: &str) {
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
pub(crate) fn hist_repo(scratch: &Path, name: &str, email: &str) -> PathBuf {
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

pub(crate) struct RestoreIcons(app::Icons);

impl RestoreIcons {
    pub(crate) fn capture() -> Self {
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
pub(crate) fn app_for_icons() -> App {
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
pub(crate) fn drawn_icon_marks(app: &App) -> (String, String) {
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

pub(crate) fn press(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

/// Open the overlay as `,` does and walk to the icons row with `j`.
pub(crate) fn open_settings_on_the_icons_row(app: &mut App) {
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
pub(crate) fn apply_open_settings(app: &mut App) {
    let Overlay::Settings(ref so) = app.overlay else {
        panic!("the settings overlay is not open");
    };
    let saved = so.clone();
    apply_live_settings(app, &saved);
}

/// The text of the settings row for icons, as the frame draws it.
pub(crate) fn drawn_icons_row(app: &App) -> String {
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

pub(crate) fn at(h: u32, m: u32, s: u32) -> chrono::DateTime<chrono::Local> {
    use chrono::TimeZone;
    chrono::Local
        .with_ymd_and_hms(2026, 10, 1, h, m, s)
        .single()
        .expect("unambiguous local time")
}

pub(crate) fn secs(n: i64) -> chrono::Duration {
    chrono::Duration::seconds(n)
}

/// A reading of `remaining` of 5000 on `token`, resetting at `reset`, as
/// stated by a server whose clock read `date`.
pub(crate) fn reading(
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
pub(crate) fn ok_with(
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
pub(crate) fn ok_with_pending_r(
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

pub(crate) fn err_with(
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

pub(crate) fn failure(kind: github::FailureKind) -> github::Failure {
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
pub(crate) fn land_at(
    app: &mut App,
    result: Result<Box<github::GithubFetch>, github::FetchError>,
    now: chrono::DateTime<chrono::Local>,
) {
    let attempt = claim_github_poll(app).expect("no poll in flight");
    answer_at(app, attempt, result, now);
}

pub(crate) fn answer_at(
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
pub(crate) fn budget_app(ci: model::CiStatus, factor: f64, resume: i64) -> (App, config::Config) {
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
pub(crate) fn next_due(
    app: &App,
    config: &config::Config,
    from: chrono::DateTime<chrono::Local>,
) -> i64 {
    (0..=4000)
        .find(|s| github_poll_due(app, config, from + secs(*s)))
        .expect("never due again")
}

pub(crate) fn status_text(app: &App) -> String {
    app.status_message
        .as_ref()
        .map(|m| m.text.clone())
        .unwrap_or_default()
}

/// The rate limit of S6 and S15: a `RATE_LIMITED` body at 17:00:43 whose
/// headers say 0 left until 17:23:06.
pub(crate) fn land_primary_limit(app: &mut App) {
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

/// A primary limit relative to the real clock, for the paths that read
/// it (`apply_load_result`): 13 minutes left, jitter 10 s.
pub(crate) fn limited_now(app: &mut App) -> chrono::DateTime<chrono::Local> {
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
pub(crate) fn carrier_row(owner: &str, name: &str, ci: model::CiStatus) -> RepoStatus {
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
pub(crate) fn entry(owner: &str, name: &str, ci: model::CiStatus) -> github::GithubRepo {
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
pub(crate) fn fetch_on(
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

pub(crate) fn remote(owner: &str, name: &str) -> app::RemoteKey {
    (Some(owner.to_string()), name.to_string())
}

/// Rows for `alice/a` and `alice/b`, both passing, as CV9 gives them.
pub(crate) fn alice_app() -> App {
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

pub(crate) fn row<'a>(app: &'a App, name: &str) -> &'a RepoStatus {
    app.repos.iter().find(|r| r.name == name).expect("row")
}

/// Land a fetched load of `kind` on `account` with `rows` and `dataset`,
/// saving any window to `store`.
pub(crate) fn land_fetched(
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
pub(crate) async fn land_account_rebuild(
    app: &mut App,
    scratch: &Path,
    store: &window_state::WindowState,
) {
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

// FZ-37: `y` and a click in pane 2 copy a commit hash.

const YANK_HASHES: [&str; 3] = ["aaa1111", "bbb2222", "ccc3333"];

/// A repo whose every view lists the three `YANK_HASHES` commits, plus
/// `extra` more, newest first.
pub(crate) fn yank_repo(extra: usize) -> RepoStatus {
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

pub(crate) fn yank_app(view: app::HistoryView, extra: usize) -> App {
    let mut app = App::new(vec![yank_repo(extra)], "h".to_string(), 120, 40);
    app.view = app::View::Detail;
    app.history_view = view;
    app
}

/// Draw a frame, as the event loop does before any click can arrive.
pub(crate) fn yank_frame(app: &App) -> ratatui::buffer::Buffer {
    use ratatui::backend::TestBackend;
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| ui::render(f, app)).unwrap();
    term.backend().buffer().clone()
}

/// The first cell of `needle` inside the detail pane, read off the
/// screen rather than out of the hit map under test.
pub(crate) fn on_screen(buf: &ratatui::buffer::Buffer, app: &App, needle: &str) -> (u16, u16) {
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

pub(crate) fn yank_status(app: &App) -> Option<(String, bool)> {
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
pub(crate) fn chord(app: &mut App, keys: &str) -> Vec<String> {
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
pub(crate) fn yank_on(view: app::HistoryView, panel: FocusedPanel) -> App {
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
pub(crate) fn yank_terminal() -> Terminal<CrosstermBackend<io::Stdout>> {
    let backend = CrosstermBackend::new(io::stdout());
    let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, 120, 40));
    Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap()
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

// FZ-30b: the `ci` view's data layer, its trigger and its answers.

pub(crate) fn ci_app() -> App {
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

/// An app in the `notes` layout on a `w` by `h` screen, its tree
/// holding `n` files under `notez/`.
pub(crate) fn notes_tree_app(w: u16, h: u16, n: usize) -> App {
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
pub(crate) fn drawn_cursor_row(
    app: &App,
    w: u16,
    h: u16,
    rect: ratatui::layout::Rect,
) -> Option<u16> {
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

/// A notes tree shaped like his: a `todos` section of three boards,
/// `notez/` holding two directories of dated notes, and a second, deep
/// notes directory, 36 files in all. Names, a directory and a section
/// are long enough to wrap in the tree's column.
pub(crate) fn his_notes() -> Vec<notes::NoteFile> {
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

/// The next answer `spawn_note_read` sent, split into its two kinds.
pub(crate) async fn next_note_answer(
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

pub(crate) fn ci_key() -> app::RemoteKey {
    (Some("gaurgle".to_string()), "fleetz".to_string())
}

pub(crate) fn ci_runs() -> Vec<ci::WorkflowRun> {
    ci::parse_runs(include_str!("ci/fixtures/runs.json")).unwrap()
}

pub(crate) fn ci_answer(request: &ci::CiRequest) -> ci::CiAnswer {
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

pub(crate) fn fresh_runs(etag: &str) -> Option<ci::Fetched<Vec<ci::WorkflowRun>>> {
    Some(ci::Fetched::Fresh {
        data: ci_runs(),
        etag: Some(etag.to_string()),
    })
}

/// Claim and land one successful list poll at `now`.
pub(crate) fn ci_landed(app: &mut App, now: chrono::DateTime<chrono::Local>) {
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

pub(crate) fn ci_failure_lands(
    failure: github::Failure,
    budget: Option<github::BudgetReading>,
) -> App {
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

// FZ-30e: the `ci` view's handoffs to `gh` and the browser.

/// `ci_app` with its runs landed, pane 2 focused on `ci`, the cursor on
/// run `index` of the fixture (in progress, success, failure).
pub(crate) fn handoff_app(index: usize) -> App {
    let mut app = ci_app();
    ci_landed(&mut app, at(12, 0, 0));
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Ci;
    app.focused_panel = FocusedPanel::Detail;
    app.detail_selections[app::HistoryView::Ci.index()] = index;
    app
}

pub(crate) fn argv(args: &[&str]) -> RunHandoff {
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
pub(crate) fn poll_newest_first(app: &mut App, names: &[&str]) {
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

pub(crate) fn shown(app: &App) -> Vec<String> {
    app.visible_repos()
        .iter()
        .map(|(_, r)| r.name.clone())
        .collect()
}

pub(crate) fn app_a_b_c_on_b() -> App {
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
