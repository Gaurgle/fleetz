use super::assembly::{assemble_view_with, sort_by_activity, stale_remotes};
use super::fetch::{ProgressThrottle, fetch_under_claim};
use super::status::{local_only_status, local_repo_is_current};
use super::*;
use crate::app::tests::fetch_failed;
use crate::cli::{Args, Mode};
use crate::model::{self, RepoLocation, RepoStatus};
use crate::tests::{
    assert_carries_nothing, cloned, gh, id_assemble, id_entry, id_repo, not_cloned, one_day,
    path_arg, row_at, wt_scratch, wt_sh,
};
use crate::{app, config, discovery, git, github, time};
use clap::Parser;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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

/// FZ-35a (#84), criterion 4: a fetch that waited in the queue past the
/// stuck limit keeps its claim once its permit is granted, because the
/// claim is timed from the permit, and progress keeps it alive after.
#[test]
fn a_claim_is_timed_from_its_permit_and_kept_alive_by_progress() {
    let claims = FetchClaims::default();
    let t0 = std::time::Instant::now();
    let limit = std::time::Duration::from_secs(180);
    let token = claims.reserve("~/r", t0).expect("free");
    // Queued 170 s, then granted its permit.
    assert!(claims.start("~/r", token, t0 + std::time::Duration::from_secs(170)));
    claims.expire(limit, t0 + std::time::Duration::from_secs(200));
    assert!(claims.contains_key("~/r"), "expired while it ran");
    // Running 160 s more, reporting progress.
    claims.touch("~/r", token, t0 + std::time::Duration::from_secs(330));
    claims.expire(limit, t0 + std::time::Duration::from_secs(400));
    assert!(claims.contains_key("~/r"), "expired while it progressed");
    // Silent past the limit: the safety net still frees it.
    claims.expire(limit, t0 + std::time::Duration::from_secs(511));
    assert!(!claims.contains_key("~/r"));
}

/// FZ-35a (#84), criterion 4: a queued fetch whose reservation expired
/// takes the path again at its permit when it is free, and does not run
/// when a newer claim holds it, so one repo never has two fetches.
#[test]
fn a_permit_takes_a_free_path_and_refuses_a_held_one() {
    let claims = FetchClaims::default();
    let t0 = std::time::Instant::now();
    let limit = std::time::Duration::from_secs(180);
    let old = claims.reserve("~/r", t0).unwrap();
    claims.expire(limit, t0 + std::time::Duration::from_secs(181));
    assert!(!claims.contains_key("~/r"));
    assert!(claims.start("~/r", old, t0 + std::time::Duration::from_secs(182)));
    assert_eq!(claims.token("~/r"), Some(old));

    let claims = FetchClaims::default();
    let old = claims.reserve("~/r", t0).unwrap();
    claims.expire(limit, t0 + std::time::Duration::from_secs(181));
    let newer = claims
        .reserve("~/r", t0 + std::time::Duration::from_secs(182))
        .unwrap();
    assert_ne!(old, newer, "a token is never reused");
    assert!(!claims.start("~/r", old, t0 + std::time::Duration::from_secs(183)));
    assert!(claims.start("~/r", newer, t0 + std::time::Duration::from_secs(183)));
    // The refused fetch's progress, had it any, would not touch it.
    claims.touch("~/r", old, t0 + std::time::Duration::from_secs(400));
    claims.expire(limit, t0 + std::time::Duration::from_secs(364));
    assert!(!claims.contains_key("~/r"));
}

/// An update that did not fetch is not the fetch being waited on.
#[test]
fn a_tally_is_not_settled_by_an_update_that_did_not_fetch() {
    let mut tally = FetchTally::default();
    tally.expect(["a".to_string()]);
    let not_fetched = crate::git::FetchOutcome::NotAttempted;
    assert_eq!(tally.land("a", "a", &not_fetched), None);
    let report = tally.land("a", "a", &fetch_failed(crate::git::FetchFailureKind::Other));
    assert_eq!(report.map(|r| r.total), Some(1));
}

/// `r` and `F` pressed together report once, over both batches, and a
/// path in both counts once.
#[test]
fn a_tally_joins_a_batch_still_in_flight() {
    let mut tally = FetchTally::default();
    tally.expect(["a".to_string(), "b".to_string()]);
    tally.expect(["b".to_string(), "c".to_string()]);
    let failed = fetch_failed(crate::git::FetchFailureKind::Auth);
    assert_eq!(tally.land("a", "a", &failed), None);
    assert_eq!(
        tally.land("b", "b", &crate::git::FetchOutcome::Succeeded),
        None
    );
    let report = tally.land("c", "c", &crate::git::FetchOutcome::Succeeded);
    assert_eq!(report.map(|r| (r.total, r.failed.len())), Some((3, 1)));
}

/// A fetch that never lands cannot hold a report open for the session.
#[test]
fn a_tally_that_outlives_its_limit_is_dropped() {
    let mut tally = FetchTally::default();
    tally.expect(["a".to_string()]);
    tally.expire(std::time::Duration::from_secs(60));
    assert!(tally.started.is_some(), "expired inside its limit");
    tally.started = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(120));
    tally.expire(std::time::Duration::from_secs(60));
    let late = tally.land("a", "a", &fetch_failed(crate::git::FetchFailureKind::Auth));
    assert_eq!(late, None);
}
