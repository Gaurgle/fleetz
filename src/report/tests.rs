use super::parity::{Fixture, strip_ansi};
use super::*;

fn text(bytes: &[u8]) -> String {
    strip_ansi(&String::from_utf8_lossy(bytes))
}

fn parse(args: &[&str]) -> Args {
    use clap::Parser;
    Args::parse_from(std::iter::once("fleetz").chain(args.iter().copied()))
}

/// The line of `report` naming `name`, without its escapes.
fn line_of<'a>(report: &'a str, name: &str) -> &'a str {
    report
        .lines()
        .find(|line| line.split_whitespace().any(|word| word == name))
        .unwrap_or_else(|| panic!("no row for {name} in\n{report}"))
}

/// D9: the default mode's cutoff and header are `time.rs`'s slot.
#[test]
fn the_default_mode_cuts_off_at_the_slot_time_rs_gives() {
    let config = crate::config::Config::default();
    let before = crate::time::auto_cutoff(config.work_start, config.work_end);
    let (cutoff, header) = window(&parse(&["--print"]), &Mode::Auto, &config).unwrap();
    let after = crate::time::auto_cutoff(config.work_start, config.work_end);
    // Read on both sides, so a slot boundary crossed mid-test still has
    // one side to agree with.
    let ours = (cutoff.unwrap(), header);
    assert!(ours == before || ours == after, "{ours:?} vs {before:?}");
}

#[test]
fn today_cuts_off_at_local_midnight() {
    let config = crate::config::Config::default();
    let (cutoff, header) = window(&parse(&["-t"]), &Mode::Today, &config).unwrap();
    let midnight = crate::time::date_to_utc_midnight(chrono::Local::now().date_naive());
    assert_eq!((cutoff, header.as_str()), (Some(midnight), "today"));
}

/// bash names a relative date by the date it resolved to.
#[test]
fn a_relative_since_and_before_read_as_their_dates() {
    let config = crate::config::Config::default();
    let args = parse(&["--since", "3d", "--before", "yesterday"]);
    let (_, header) = window(&args, &args.mode(), &config).unwrap();
    let today = chrono::Local::now().date_naive();
    let since = today - chrono::Duration::days(3);
    let before = today - chrono::Duration::days(1);
    assert_eq!(header, format!("since {since} \u{2192} before {before}"));
}

/// No terminal state, structurally: the `--print` branch in `main` sits
/// before the first terminal call.
#[test]
fn main_branches_to_print_mode_before_any_terminal_call() {
    let main = include_str!("../main.rs");
    let body = &main[main.find("async fn main()").expect("no main")..];
    let branch = body.find("if args.print").expect("no --print branch");
    for call in [
        "crossterm::terminal::size()",
        "enable_raw_mode()?",
        "EnterAlternateScreen",
        "spawn_input_thread(",
    ] {
        let at = body.find(call).unwrap_or_else(|| panic!("no {call}"));
        assert!(branch < at, "--print branches after {call}");
    }
}

/// No terminal state, at run time: the report goes to the `Write` it is
/// handed, and nothing of it reaches the process's own stdout or stderr.
#[test]
fn print_mode_writes_only_to_the_writer_it_is_given() {
    let fixture = Fixture::build("writer");
    let (report, child) = fixture.run_print_mode_full(&["-a"]);
    assert!(text(&report).contains("alpha-tool"));
    for (stream, bytes) in [("stdout", &child.stdout), ("stderr", &child.stderr)] {
        let bytes = String::from_utf8_lossy(bytes);
        assert!(
            !bytes.contains('\u{1b}') && !bytes.contains("alpha-tool"),
            "print mode wrote to {stream}: {bytes}"
        );
    }
}

/// D5: a poll through the fixture seam leaves this credential's keyed
/// cache behind for the next TUI launch.
#[test]
fn a_print_run_writes_the_keyed_cache() {
    let fixture = Fixture::build("keyed");
    fixture.run_print_mode(&["-a"]);
    let written: Vec<PathBuf> = ["Library/Caches/fleetz", ".cache/fleetz"]
        .iter()
        .filter_map(|dir| std::fs::read_dir(fixture.home.join(dir)).ok())
        .flatten()
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    let account = github::token_fingerprint("fixture");
    assert!(
        written.iter().any(|path| path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().contains(&account))),
        "no keyed cache file for the account in {written:?}"
    );
}

/// D3, matching: a clone in a folder named for a payload repo whose
/// origin names another owner's repo does not claim the row. bash repoz
/// matches by folder name and would list it under the roots.
#[test]
fn a_folder_name_does_not_claim_a_repo_its_origin_disowns() {
    let fixture = Fixture::build("d3_owner");
    fixture.publish(
        "someone-else",
        super::parity::NOT_CLONED,
        "2026-02-01T09:00:00Z",
    );
    let stranger = fixture.repo_dir.join(super::parity::NOT_CLONED);
    fixture.clone_into("someone-else", super::parity::NOT_CLONED, &stranger);

    let report = text(&fixture.run_print_mode(&["-a"]));
    let row = line_of(&report, super::parity::NOT_CLONED);
    assert!(row.ends_with("not cloned"), "{row}");
    assert!(!report.contains("~/repos/ghost-repo"), "{report}");
}

/// D3, several clones: the exact origin outranks a folder match in an
/// earlier root (`CheckoutRank`). bash repoz takes the first folder
/// match under `REPO_CHECK_DIR`.
#[test]
fn of_two_clones_the_ranked_one_is_the_row() {
    let fixture = Fixture::build("d3_rank");
    let unlinked = fixture.repo_dir.join("orgproj");
    fixture.clone_into(super::parity::ORG, "orgproj", &unlinked);
    fixture.git(&unlinked, &["remote", "remove", "origin"]);

    let report = text(&fixture.run_print_mode(&["-a"]));
    let row = line_of(&report, "orgproj");
    assert!(row.contains("~/extra/orgproj"), "{row}");
    assert!(!report.contains("~/repos/orgproj"), "{report}");
}

/// D3, which clones fetch: only a remote GitHub says moved since the
/// clone's last fetch, and a clone that never fetched. bash repoz
/// fetches every matched clone.
#[test]
fn only_stale_remotes_are_fetched() {
    let fixture = Fixture::build("d3_stale");
    fixture.push_upstream(super::parity::LOGIN, "alpha-tool", "Second parser pass");
    fixture.push_upstream(super::parity::ORG, "orgproj", "Org follow-up");
    // Pushed before its last fetch by the payload's clock: not fetched,
    // so the new commit stays unseen and the row stays at 1 behind.
    // Never fetched: fetched, and the new commit shows.
    std::fs::remove_file(fixture.org_clone.join(".git/FETCH_HEAD")).unwrap();

    let report = text(&fixture.run_print_mode(&["-a"]));
    assert!(
        line_of(&report, "alpha-tool").ends_with(" 1\u{2193}"),
        "{report}"
    );
    assert!(
        line_of(&report, "orgproj").ends_with(" 1\u{2193}"),
        "{report}"
    );
    assert!(report.contains("Org follow-up"), "{report}");
    assert!(!report.contains("Second parser pass"), "{report}");
}

/// D3, archived repos: print mode polls with fleetz's query, which asks
/// GitHub to leave archived repos out of both connections (O3).
#[tokio::test]
async fn the_poll_leaves_archived_repos_out() {
    let dir = std::env::temp_dir().join(format!("fleetz_report_archived_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let payload = include_str!("fixtures/graphql.json").to_string();
    let (url, request) = serve_capturing(payload).await;
    let cache = crate::cache::KeyedCache::at(dir.clone(), 50, github::HOST);

    let fetch = poll_at(50, &cache, || async { Ok("tok".to_string()) }, &url).await;
    let request = request.await.unwrap();
    let keyed = cache.for_account(&github::token_fingerprint("tok"));
    let cached = keyed.read();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(fetch.expect("the poll failed").repos.len(), 4);
    assert_eq!(request.matches("isArchived: false").count(), 2, "{request}");
    assert!(cached.is_some(), "the poll wrote no keyed cache");
}

/// O4: the poll goes to GitHub even when this credential's keyed cache
/// holds a payload (`CacheMode::Bypass`); a cache-served answer would
/// never reach the listener.
#[tokio::test]
async fn the_poll_asks_github_past_a_cached_payload() {
    let dir = std::env::temp_dir().join(format!("fleetz_report_bypass_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let payload = include_str!("fixtures/graphql.json").to_string();
    let cache = crate::cache::KeyedCache::at(dir.clone(), 50, github::HOST);
    cache
        .for_account(&github::token_fingerprint("tok"))
        .write(&payload);
    let (url, request) = serve_capturing(payload).await;

    let fetch = poll_at(50, &cache, || async { Ok("tok".to_string()) }, &url).await;
    let request = tokio::time::timeout(std::time::Duration::from_secs(5), request).await;
    let _ = std::fs::remove_dir_all(&dir);

    assert!(fetch.is_ok(), "the poll failed");
    assert!(
        request.is_ok_and(|r| r.is_ok()),
        "the poll was answered from the keyed cache, not GitHub"
    );
}

/// bash adds the working directory's git toplevel to its scan roots
/// (`add_scan_dir "$_toplevel"`), so a dirty repo outside every root is
/// in `local changes` when print mode runs from inside it.
#[test]
fn the_working_directorys_repo_is_scanned() {
    let fixture = Fixture::build("cwd_root");
    let loose = fixture.home.join("loose/scratch");
    let inside = loose.join("src");
    std::fs::create_dir_all(&inside).unwrap();
    fixture.git(&loose, &["init", "-q"]);
    std::fs::write(inside.join("wip.txt"), "wip\n").unwrap();

    let elsewhere = text(&fixture.run_print_mode(&["-a"]));
    assert!(!elsewhere.contains("scratch"), "{elsewhere}");

    let (report, _) = fixture.run_print_mode_from(&inside, &["-a"]);
    let report = text(&report);
    let (_, local) = report
        .split_once("local changes")
        .unwrap_or_else(|| panic!("no local changes section\n{report}"));
    let row = line_of(local, "scratch");
    assert!(
        row.contains("~/loose/scratch") && row.ends_with(" 1!"),
        "{row}"
    );
}

/// D5: `-d` and the target mode never enter the poll. The payload seam
/// names a file that does not exist, so a poll would end as `Failed to
/// reach GitHub` (and could not reach the network either), and a
/// successful one would leave a keyed cache behind.
#[test]
fn local_mode_and_a_target_send_no_request() {
    let fixture = Fixture::build("no_request");
    let missing = fixture.root.join("no-payload.json");
    let env = [(GRAPHQL_FIXTURE, missing.as_os_str())];
    for args in [&["-d"][..], &["/fixture-user/alpha-tool"], &["/alpha-tool"]] {
        let (report, _) = fixture.run_print_mode_with(&fixture.root, &env, args);
        let report = text(&report);
        assert!(
            !report.contains("Failed to reach GitHub"),
            "{args:?}\n{report}"
        );
        assert!(
            report.contains("repoz"),
            "{args:?} printed nothing\n{report}"
        );
    }
    for dir in ["Library/Caches/fleetz", ".cache/fleetz"] {
        assert!(!fixture.home.join(dir).exists(), "a poll wrote {dir}");
    }

    // With the valid payload (no override) a stray poll would succeed
    // and write a keyed cache, so each run is checked on its own.
    for args in [&["-d"][..], &["/fixture-user/alpha-tool"], &["/alpha-tool"]] {
        fixture.run_print_mode_with(&fixture.root, &[], args);
        for dir in ["Library/Caches/fleetz", ".cache/fleetz"] {
            assert!(
                !fixture.home.join(dir).exists(),
                "{args:?} polled and wrote {dir}"
            );
        }
    }
}

/// The target's owner is the login when it has one segment, and the
/// row is the clone that login's repo matched, not a same-named clone of
/// another owner's (D3, matching).
#[test]
fn a_one_segment_target_resolves_against_the_login() {
    let fixture = Fixture::build("target_login");
    let login = std::ffi::OsString::from("someone-else");
    let env = [(GH_LOGIN_FIXTURE, login.as_os_str())];
    let (report, _) = fixture.run_print_mode_with(&fixture.root, &env, &["/alpha-tool"]);
    let report = text(&report);
    assert!(
        report
            .lines()
            .any(|l| l.contains("someone-else/alpha-tool") && l.ends_with("not cloned")),
        "{report}"
    );

    let report = text(&fixture.run_print_mode(&["/alpha-tool"]));
    assert!(report.contains("fixture-user/alpha-tool"), "{report}");
    assert!(line_of(&report, "alpha-tool").contains("~/repos/alpha-tool"));
}

/// A clone of the active `ghost-repo` outside `HOME`, with a dirty
/// linked worktree `ghost-wt` under `repo_dir`. The clone's origin is an
/// ssh URL the fixture's `insteadOf` leaves alone, so `git remote
/// get-url` reads it as GitHub's and the worktree can carry the repo.
fn ghost_worktree(fixture: &Fixture) {
    use super::parity::{LOGIN, NOT_CLONED};
    fixture.publish(LOGIN, NOT_CLONED, "2026-02-01T09:00:00Z");
    let main = fixture.root.join("outside").join(NOT_CLONED);
    fixture.clone_into(LOGIN, NOT_CLONED, &main);
    let origin = format!("git@github.com:{LOGIN}/{NOT_CLONED}.git");
    fixture.git(&main, &["remote", "set-url", "origin", &origin]);
    let linked = fixture.repo_dir.join("ghost-wt");
    fixture.git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "wip",
            linked.to_str().unwrap(),
        ],
    );
    std::fs::write(linked.join("wip.txt"), "wip\n").unwrap();
}

/// FZ-47 review follow-up 1: a pass 3 row carries an active repo
/// through its origin, its main clone outside the roots, so no
/// `not cloned` row is printed for that repo. bash repoz matches by
/// folder name only and prints both the worktree's row and
/// `ghost-repo ... not cloned`.
#[test]
fn a_pass_3_worktree_carries_its_repo_through_its_origin() {
    let fixture = Fixture::build("wt_carrier");
    ghost_worktree(&fixture);

    let report = text(&fixture.run_print_mode(&["-a"]));
    let (_, local) = report
        .split_once("local changes")
        .unwrap_or_else(|| panic!("no local changes section\n{report}"));
    let row = line_of(local, "ghost-wt");
    assert!(
        row.contains("~/repos/ghost-wt") && row.ends_with(" 1!"),
        "{row}"
    );
    assert!(!report.contains("not cloned"), "{report}");
    assert!(!report.contains("not local"), "{report}");
}

/// FZ-47 review follow-up 2: a linked worktree in pass 3 counts only
/// its own branch's unpushed commit and no stashes, while its main clone
/// beside it counts every branch (its side branch and the worktree's)
/// and the shared stash. bash repoz prints the same
/// (`worktree-all.bash.out`).
#[test]
fn a_pass_3_worktree_counts_its_own_branch_and_no_stashes() {
    use super::parity::{WT_LINKED, WT_MAIN};
    let fixture = Fixture::build("wt_counts");
    fixture.add_worktree_and_prefix();

    let report = text(&fixture.run_print_mode(&["-a"]));
    let (_, local) = report
        .split_once("local changes")
        .unwrap_or_else(|| panic!("no local changes section\n{report}"));
    let linked = line_of(local, WT_LINKED);
    assert!(linked.ends_with(" 1\u{2191}"), "{linked}");
    let main = line_of(local, WT_MAIN);
    assert!(main.ends_with(" 2\u{2191} 1\u{2691}"), "{main}");
    assert!(report.contains("1 stashed"), "{report}");
}

/// `orgproj`'s row as print mode draws a pass 1 row, its fetch facts
/// holding `outcome`, and the summary after it. No glyphs, no escapes.
async fn checked_row_after(
    fixture: &Fixture,
    outcome: crate::git::FetchOutcome,
) -> (String, Counts, String) {
    let clone = &fixture.org_clone;
    let mut row = bare_row(
        "orgproj".to_string(),
        Some(clone.display().to_string()),
        RepoLocation::Primary,
    );
    row.dirty = crate::git::dirty_count(clone).await;
    row.branches = crate::git::branch_statuses(clone).await;
    row.fetch.outcome = outcome;
    let layout = Layout {
        width: 100,
        glyphs: false,
    };
    let mut report = Report {
        layout,
        home: &fixture.home,
        active: &[],
        author: None,
        stat: false,
        releases: false,
        prs: false,
        counts: Counts::default(),
    };
    let line = strip_ansi(&report.checked_row(&row).await);
    let summary = strip_ansi(&layout.summary(&report.counts, "fleetz 0.0.0"));
    (line, report.counts, summary)
}

/// D4 with FZ-35a's stall limit, through its seam
/// (`git::fetch_watched`, here at 1 s; print mode's fetches run under
/// `FETCH_STALL_LIMIT` through `fetch_reporting`): a fetch whose remote
/// goes silent is cut at the limit as a failed fetch, and the row it
/// lands on reads `fetch failed` where it would read `synced`, and is
/// counted as a failed fetch, never as synced. No retry: one fetch, one
/// outcome.
#[tokio::test]
async fn a_stalled_fetch_ends_at_the_limit_and_its_row_is_never_synced() {
    use crate::git::{FetchFailure, FetchFailureKind, FetchOutcome};
    let fixture = Fixture::build("stall");
    let clone = &fixture.org_clone;
    // The bare remote by path, so the fetch needs no fixture `HOME` to
    // stay offline, and an upload-pack that never answers.
    let bare = fixture.remotes.join(super::parity::ORG).join("orgproj.git");
    fixture.git(
        clone,
        &["remote", "set-url", "origin", bare.to_str().unwrap()],
    );
    fixture.git(
        clone,
        &["config", "remote.origin.uploadpack", "sleep 600; :"],
    );

    let (synced, _, _) = checked_row_after(&fixture, FetchOutcome::Succeeded).await;
    assert!(
        synced.contains(" synced"),
        "not synced to begin with: {synced}"
    );

    let limit = std::time::Duration::from_secs(1);
    let start = std::time::Instant::now();
    let outcome = crate::git::fetch_watched(clone, limit, |_| {}).await;
    let took = start.elapsed();
    assert_eq!(
        outcome,
        FetchOutcome::Failed(FetchFailure {
            kind: FetchFailureKind::TimedOut,
            detail: None,
        })
    );
    assert!(took >= limit, "cut before the limit: {took:?}");
    assert!(
        took < std::time::Duration::from_secs(10),
        "not cut at the limit: {took:?}"
    );

    let (line, counts, summary) = checked_row_after(&fixture, outcome).await;
    let row = line.lines().next().unwrap_or_default();
    assert!(row.ends_with(" fetch failed"), "{line}");
    assert!(!line.contains("synced"), "{line}");
    assert_eq!((counts.fetch_failed, counts.synced), (1, 0), "{counts:?}");
    assert!(summary.contains("1 fetch failed"), "{summary}");
    assert!(!summary.contains("synced"), "{summary}");
}

/// Fetch outcome contract rule 8: a clone whose origin carries a token
/// fails its fetch (nothing listens on the port) and prints `fetch
/// failed`, and no byte print mode writes, nor anything its process
/// prints, holds the token or the user it rides with. In the GitHub
/// mode and the target mode, the two that fetch.
#[test]
fn no_credential_reaches_a_printed_line() {
    const USER: &str = "fz-bot";
    const TOKEN: &str = "ghp_FixtureSecret0123456789";
    let fixture = Fixture::build("credential");
    let clone = &fixture.org_clone;
    let url = format!(
        "https://{USER}:{TOKEN}@127.0.0.1:9/{}/orgproj.git",
        super::parity::ORG
    );
    fixture.git(clone, &["remote", "set-url", "origin", &url]);
    // Never fetched, so the GitHub mode fetches it too (`stale_remotes`).
    std::fs::remove_file(clone.join(".git/FETCH_HEAD")).unwrap();

    for args in [&["-a"][..], &["/fixture-org/orgproj"]] {
        let (report, child) = fixture.run_print_mode_full(args);
        let shown = text(&report);
        assert!(
            line_of(&shown, "orgproj").ends_with(" fetch failed"),
            "{args:?}: the fetch did not fail\n{shown}"
        );
        for (stream, bytes) in [
            ("report", &report),
            ("stdout", &child.stdout),
            ("stderr", &child.stderr),
        ] {
            let bytes = String::from_utf8_lossy(bytes);
            for secret in [TOKEN, USER] {
                assert!(
                    !bytes.contains(secret),
                    "{args:?}: {secret} in the {stream}\n{bytes}"
                );
            }
        }
    }
}

/// A loopback listener answering one request with `body`, handing back
/// the request it read.
async fn serve_capturing(body: String) -> (String, tokio::sync::oneshot::Receiver<String>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (sent, received) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut seen = Vec::new();
        let mut chunk = [0u8; 16 * 1024];
        loop {
            let text = String::from_utf8_lossy(&seen).to_lowercase();
            if let Some(end) = text.find("\r\n\r\n") {
                let length = text
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if seen.len() >= end + 4 + length {
                    break;
                }
            }
            match socket.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => seen.extend_from_slice(&chunk[..n]),
            }
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
        let _ = sent.send(String::from_utf8_lossy(&seen).into_owned());
    });
    (format!("http://{addr}/graphql"), received)
}
