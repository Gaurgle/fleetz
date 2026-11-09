use super::*;

fn git_out(fixture: &Fixture, dir: &Path, args: &[&str]) -> String {
    let out = fixture
        .git_command(dir)
        .args(args)
        .output()
        .expect("spawn git");
    assert!(out.status.success(), "git {args:?} in {dir:?} failed");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
fn the_version_line_is_dropped_and_nothing_else() {
    // The header's second word is an em dash in bash repoz, spelled as an
    // escape here.
    let header = "  \x1b[38;2;180;190;254m\x1b[1mrepoz\x1b[0m \x1b[38;2;127;132;156m\u{2014} all divergence\x1b[0m\n";
    let version = "                                      \x1b[38;2;69;71;90mrepoz 0.1.0\x1b[0m\n";
    let output = format!("\n{header}  rows\n  legend   \n{version}\n");
    let expected = format!("\n{header}  rows\n  legend   \n\n");
    assert_eq!(
        without_version_line(output.as_bytes()),
        Some(expected.into_bytes())
    );
}

#[test]
fn output_without_a_version_line_does_not_normalise() {
    assert_eq!(
        without_version_line(b"\n  repoz \xe2\x80\x94 today\n\n"),
        None
    );
    assert_eq!(without_version_line(b""), None);
}

#[test]
fn only_the_last_version_line_is_dropped() {
    let output = b"  repoz 0.1.0\n  repoz 0.1.0\n";
    assert_eq!(
        without_version_line(output),
        Some(b"  repoz 0.1.0\n".to_vec())
    );
}

#[test]
fn the_payload_carries_every_field_bash_repoz_reads() {
    let payload: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/graphql.json")).unwrap();
    let viewer = &payload["data"]["viewer"];
    let mut nodes: Vec<&serde_json::Value> = viewer["repositories"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .collect();
    for org in viewer["organizations"]["nodes"].as_array().unwrap() {
        nodes.extend(org["repositories"]["nodes"].as_array().unwrap());
    }
    let mut names: Vec<&str> = Vec::new();
    for node in nodes {
        let name = node["name"].as_str().unwrap();
        let owner = node["owner"]["login"].as_str().unwrap();
        assert_eq!(node["nameWithOwner"], format!("{owner}/{name}"), "{name}");
        assert!(node["isPrivate"].is_boolean(), "{name}: isPrivate");
        assert!(
            node["owner"]["__typename"].is_string(),
            "{name}: owner type"
        );
        assert!(node["pushedAt"].is_string(), "{name}: pushedAt");
        assert!(node["pullRequests"]["totalCount"].is_u64(), "{name}: PRs");
        assert_eq!(node["isArchived"], false, "{name}: archived (D3)");
        names.push(name);
    }
    names.sort_unstable();
    assert_eq!(
        names,
        ["alpha-tool", "elsewhere-app", NOT_CLONED, "orgproj"]
    );
}

/// Through fleetz's own parser, from a cache file, so no request is made:
/// `AnyCached` serves a payload with a calendar and PR counts.
#[tokio::test]
async fn the_payload_parses_as_fleetz_reads_it() {
    let dir = std::env::temp_dir().join(format!("fleetz_report_payload_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("github-50.json");
    std::fs::write(&path, include_str!("../fixtures/graphql.json")).unwrap();
    let cache = crate::cache::ApiCache::at(path);

    let fetch = crate::github::fetch_repos(50, &cache, crate::github::CacheMode::AnyCached).await;
    let _ = std::fs::remove_dir_all(&dir);
    let fetch = fetch.expect("the payload should be served from the cache");

    assert!(fetch.calendar.is_some());
    let repos: Vec<(&str, Option<&str>, crate::model::RepoKind, u32)> = fetch
        .repos
        .iter()
        .map(|r| (r.name.as_str(), r.owner.as_deref(), r.repo_kind, r.pr_count))
        .collect();
    use crate::model::RepoKind::{Org, Private, Public};
    assert_eq!(
        repos,
        [
            ("alpha-tool", Some(LOGIN), Private, 1),
            ("orgproj", Some(ORG), Org, 0),
            ("elsewhere-app", Some(LOGIN), Public, 0),
            (NOT_CLONED, Some(LOGIN), Public, 0),
        ]
    );
}

#[test]
fn the_fixture_builds_the_four_cases_offline() {
    let fixture = Fixture::build("cases");
    let clones = [
        (&fixture.user_clone, LOGIN, "alpha-tool"),
        (&fixture.org_clone, ORG, "orgproj"),
        (&fixture.elsewhere_clone, LOGIN, "elsewhere-app"),
    ];
    for (dir, owner, name) in clones {
        assert_eq!(dir.file_name().unwrap(), name);
        assert_eq!(
            git_out(&fixture, dir, &["config", "remote.origin.url"]),
            github_url(owner, name),
            "the origin must agree with the folder and owner"
        );
        assert!(
            dir.join(".git/FETCH_HEAD").is_file(),
            "{name} never fetched"
        );
        assert_eq!(git_out(&fixture, dir, &["status", "--porcelain"]), "");
    }
    assert!(fixture.user_clone.starts_with(&fixture.repo_dir));
    assert!(fixture.org_clone.starts_with(&fixture.extra_dir));
    assert!(fixture.elsewhere_clone.starts_with(&fixture.home));
    assert!(!fixture.elsewhere_clone.starts_with(&fixture.repo_dir));
    assert!(!fixture.elsewhere_clone.starts_with(&fixture.extra_dir));

    let counts = |dir: &Path| {
        git_out(
            &fixture,
            dir,
            &["rev-list", "--left-right", "--count", "main...origin/main"],
        )
    };
    assert_eq!(counts(&fixture.user_clone), "0\t1", "user repo: 1 behind");
    assert_eq!(counts(&fixture.org_clone), "0\t0", "org repo: synced");
    assert_eq!(
        counts(&fixture.elsewhere_clone),
        "1\t0",
        "elsewhere: 1 ahead"
    );

    let mut clones_on_disk = crate::discovery::find_all_local(std::slice::from_ref(&fixture.root));
    clones_on_disk.sort();
    let mut expected = vec![
        fixture.user_clone.clone(),
        fixture.org_clone.clone(),
        fixture.elsewhere_clone.clone(),
    ];
    expected.sort();
    assert_eq!(
        clones_on_disk, expected,
        "one clone per repo, no seeds left"
    );
    assert!(!fixture.root.join("seed").exists());

    let root = fixture.root.clone();
    drop(fixture);
    assert!(!root.exists(), "the fixture cleans up after itself");
}

/// Proves the fixture against bash repoz before print mode relies on it,
/// and keeps the recordings honest.
#[test]
fn bash_repoz_renders_the_fixture() {
    let Some(bin) = bash_with_hook() else {
        return;
    };
    let fixture = Fixture::build("bash");
    for &(case, args) in CASES {
        let live = fixture.run_bash(&bin, args);
        let text = strip_ansi(&String::from_utf8_lossy(&live));
        let mut rows = vec!["alpha-tool", "orgproj", "elsewhere-app", "also active"];
        if case == "all" {
            rows.extend([NOT_CLONED, "not cloned"]);
        } else {
            assert!(
                !text.contains(NOT_CLONED),
                "{case}: pushed before --since\n{text}"
            );
        }
        for row in rows {
            assert!(text.contains(row), "{case}: no {row:?} in\n{text}");
        }

        let live_normal = without_version_line(&live).expect("bash printed no version line");
        let matches = recorded(case)
            .and_then(|r| without_version_line(&r))
            .is_some_and(|r| r == live_normal);
        if !matches {
            let fresh = std::env::temp_dir().join(format!("fleetz-parity-{case}.bash.out"));
            std::fs::write(&fresh, &live).unwrap();
            panic!(
                "{case}: bash repoz no longer prints {}; if the change is intended, \
                 copy {} over it\n{text}",
                recording_path(case).display(),
                fresh.display()
            );
        }
    }
}

/// The print mode child process runs and hands back what it wrote.
#[test]
fn print_mode_runs_in_a_child_process() {
    let fixture = Fixture::build("child");
    fixture.run_print_mode(&["-a"]);
}

#[test]
fn print_mode_matches_bash_repoz() {
    let fixture = Fixture::build("golden");
    for &(case, args) in CASES {
        let theirs = recorded(case).expect("no recording");
        let ours = fixture.run_print_mode(args);
        let theirs = without_version_line(&theirs).expect("recording has no version line");
        let ours = without_version_line(&ours).expect("print mode printed no version line");
        assert!(
            ours == theirs,
            "{case}: print mode differs from bash repoz\n--- bash repoz\n{}\n--- print mode\n{}",
            String::from_utf8_lossy(&theirs),
            String::from_utf8_lossy(&ours)
        );
    }
}

/// [`BEFORE_CASES`] against bash repoz: `alpha-tool`, pushed on the
/// 12th, is out of the window, and the recordings still match.
#[test]
fn bash_repoz_renders_the_before_cases() {
    let Some(bin) = bash_with_hook() else {
        return;
    };
    let fixture = Fixture::build("bash_before");
    for &(case, args) in BEFORE_CASES {
        let live = fixture.run_bash(&bin, args);
        let text = strip_ansi(&String::from_utf8_lossy(&live));
        assert!(
            text.contains("orgproj") && !text.contains("alpha-tool"),
            "{case}: --before kept the wrong rows\n{text}"
        );
        let live_normal = without_version_line(&live).expect("bash printed no version line");
        if recorded(case).and_then(|r| without_version_line(&r)) != Some(live_normal) {
            let fresh = std::env::temp_dir().join(format!("fleetz-parity-{case}.bash.out"));
            std::fs::write(&fresh, &live).unwrap();
            panic!(
                "{case}: bash repoz no longer prints {}; if the change is intended, \
                 copy {} over it\n{text}",
                recording_path(case).display(),
                fresh.display()
            );
        }
    }
}

/// [`LOCAL_CASES`] against bash repoz: every [`LOCAL_REPOS`] row is in
/// `local changes` under a window none of them is dated in, and the
/// recordings still match.
#[test]
fn bash_repoz_renders_the_local_cases() {
    let Some(bin) = bash_with_hook() else {
        return;
    };
    let fixture = Fixture::build("bash_local");
    fixture.add_local_changes();
    for &(case, args) in LOCAL_CASES {
        let live = fixture.run_bash(&bin, args);
        let text = strip_ansi(&String::from_utf8_lossy(&live));
        let section = text
            .split_once("local changes")
            .unwrap_or_else(|| panic!("{case}: no local changes section\n{text}"))
            .1;
        for (name, status) in LOCAL_REPOS.iter().zip(["1!", "1\u{2691}", "rebasing"]) {
            let row = section
                .lines()
                .find(|line| line.split_whitespace().next() == Some(name))
                .unwrap_or_else(|| panic!("{case}: no {name} row\n{text}"));
            assert!(row.ends_with(status), "{case}: {row}");
        }
        let live_normal = without_version_line(&live).expect("bash printed no version line");
        if recorded(case).and_then(|r| without_version_line(&r)) != Some(live_normal) {
            let fresh = std::env::temp_dir().join(format!("fleetz-parity-{case}.bash.out"));
            std::fs::write(&fresh, &live).unwrap();
            panic!(
                "{case}: bash repoz no longer prints {}; if the change is intended, \
                 copy {} over it\n{text}",
                recording_path(case).display(),
                fresh.display()
            );
        }
    }
}

/// bash's pass 3 byte for byte: the window hides none of the
/// [`LOCAL_REPOS`], and their stash, operation and dirty counts read as
/// bash reads them.
#[test]
fn print_mode_matches_bash_repoz_with_local_changes() {
    let fixture = Fixture::build("golden_local");
    fixture.add_local_changes();
    for &(case, args) in LOCAL_CASES {
        let theirs = recorded(case).expect("no recording");
        let theirs = without_version_line(&theirs).expect("recording has no version line");
        let ours = fixture.run_print_mode(args);
        let ours = without_version_line(&ours).expect("print mode printed no version line");
        assert!(
            ours == theirs,
            "{case}: print mode differs from bash repoz\n--- bash repoz\n{}\n--- print mode\n{}",
            String::from_utf8_lossy(&theirs),
            String::from_utf8_lossy(&ours)
        );
    }
}

/// Run from a subdirectory of a dirty repo outside every root, both tools
/// scan its toplevel (bash's `add_scan_dir "$_toplevel"`) and print the
/// same bytes. Compared live, since the working directory is the input.
#[test]
fn print_mode_matches_bash_repoz_from_inside_a_repo() {
    let Some(bin) = bash_with_hook() else {
        return;
    };
    let fixture = Fixture::build("cwd_parity");
    let loose = fixture.home.join("loose/scratch");
    let inside = loose.join("src");
    fixture.init(&loose);
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::write(inside.join("wip.txt"), "wip\n").unwrap();

    let args = ["--since", "2026-03-01"];
    let out = fixture
        .command(&bin)
        .current_dir(&inside)
        .args(args)
        .output()
        .expect("run repoz");
    assert!(out.status.success(), "repoz failed");
    let text = strip_ansi(&String::from_utf8_lossy(&out.stdout));
    assert!(
        text.contains("~/loose/scratch"),
        "bash skipped the cwd\n{text}"
    );
    let theirs = without_version_line(&out.stdout).expect("bash printed no version line");
    let (ours, _) = fixture.run_print_mode_from(&inside, &args);
    let ours = without_version_line(&ours).expect("print mode printed no version line");
    assert!(
        ours == theirs,
        "print mode differs from bash repoz\n--- bash repoz\n{}\n--- print mode\n{}",
        String::from_utf8_lossy(&theirs),
        String::from_utf8_lossy(&ours)
    );
}

/// `live` against the recording for `case`: whole when `versioned` is
/// false (and then neither may carry a version line), else without it.
/// On a mismatch, leaves `live` in the temp directory to copy over the
/// recording.
fn check_recording(case: &str, live: &[u8], versioned: bool) {
    let text = strip_ansi(&String::from_utf8_lossy(live));
    let normal = |bytes: &[u8]| match versioned {
        true => without_version_line(bytes),
        false => without_version_line(bytes)
            .is_none()
            .then(|| bytes.to_vec()),
    };
    let live_normal = normal(live).unwrap_or_else(|| panic!("{case}: version line\n{text}"));
    if recorded(case).and_then(|r| normal(&r)) != Some(live_normal) {
        let fresh = std::env::temp_dir().join(format!("fleetz-parity-{case}.bash.out"));
        std::fs::write(&fresh, live).unwrap();
        panic!(
            "{case}: bash repoz no longer prints {}; if the change is intended, \
             copy {} over it\n{text}",
            recording_path(case).display(),
            fresh.display()
        );
    }
}

/// Print mode's `ours` against the recording for `case`, as
/// [`check_recording`] compares.
fn assert_matches_recording(case: &str, ours: &[u8], versioned: bool) {
    let theirs = recorded(case).unwrap_or_else(|| panic!("{case}: no recording"));
    let (theirs, ours) = match versioned {
        true => (
            without_version_line(&theirs).expect("recording has no version line"),
            without_version_line(ours).expect("print mode printed no version line"),
        ),
        false => {
            assert!(without_version_line(&theirs).is_none(), "{case}: versioned");
            (theirs, ours.to_vec())
        }
    };
    assert!(
        ours == theirs,
        "{case}: print mode differs from bash repoz\n--- bash repoz\n{}\n--- print mode\n{}",
        String::from_utf8_lossy(&theirs),
        String::from_utf8_lossy(&ours)
    );
}

/// [`UNVERSIONED_CASES`] against bash repoz: `-d` finds nothing to say
/// on the plain fixture, and each target prints one row.
#[test]
fn bash_repoz_renders_the_unversioned_cases() {
    let Some(bin) = bash_with_hook() else {
        return;
    };
    let fixture = Fixture::build("bash_unversioned");
    for &(case, args) in UNVERSIONED_CASES {
        let live = fixture.run_bash(&bin, args);
        check_recording(case, &live, false);
    }
}

#[test]
fn print_mode_matches_bash_repoz_unversioned() {
    let fixture = Fixture::build("golden_unversioned");
    for &(case, args) in UNVERSIONED_CASES {
        assert_matches_recording(case, &fixture.run_print_mode(args), false);
    }
}

/// [`WORKTREE_CASES`] against bash repoz.
#[test]
fn bash_repoz_renders_the_worktree_cases() {
    let Some(bin) = bash_with_hook() else {
        return;
    };
    let fixture = Fixture::build("bash_worktree");
    fixture.add_worktree_and_prefix();
    for &(case, args, versioned) in WORKTREE_CASES {
        let live = fixture.run_bash(&bin, args);
        check_recording(case, &live, versioned);
    }
}

/// The linked worktree and the dirty prefix repo byte for byte, in `-d`,
/// `-a` and the target mode.
#[test]
fn print_mode_matches_bash_repoz_with_worktree_and_prefix() {
    let fixture = Fixture::build("golden_worktree");
    fixture.add_worktree_and_prefix();
    for &(case, args, versioned) in WORKTREE_CASES {
        assert_matches_recording(case, &fixture.run_print_mode(args), versioned);
    }
}

/// The fixture the [`OPTION_CASES`] were recorded on.
fn options_fixture(tag: &str) -> Fixture {
    let mut fixture = Fixture::build(tag);
    fixture.add_local_changes();
    fixture.add_releases();
    fixture.add_other_author();
    fixture.add_pull_requests();
    fixture
}

/// What each option case must show in bash repoz, so a recording that
/// misses the option's point cannot pass.
fn check_option_case(case: &str, text: &str) {
    let wants: &[&str] = match case {
        "opt-releases" => &["released v1.0.0", "1 since v0.1.0", "unreleased"],
        "opt-prs" => &["2\u{2193} 1PR", "2PR", "not cloned 3PR", "6 open PRs"],
        "opt-author" => &["(author: Other)", "Fix the lexer", "Sketch the layout"],
        "opt-stat-releases-prs" => &["released v1.0.0", "6 open PRs", "+1  Fixture Author"],
        _ => &[],
    };
    for want in wants {
        assert!(text.contains(want), "{case}: no {want:?} in\n{text}");
    }
    if case == "opt-stat" {
        // The stat sits after a dot leader, so check one line, not a substring.
        assert!(
            text.lines()
                .any(|l| l.contains("Org release follow-up") && l.contains("+1")),
            "{case}: no stat on the unchanged row in\n{text}"
        );
    }
    if case == "opt-author" {
        assert!(
            !text.contains("Fixture Author"),
            "{case}: unfiltered\n{text}"
        );
    }
    if case.contains("no-glyphs") {
        for glyph in ["\u{f1ad}", "\u{f023}", "\u{f0ac}"] {
            assert!(!text.contains(glyph), "{case}: a glyph\n{text}");
        }
    }
    if !case.contains("prs") && !case.contains("target") {
        assert!(!text.contains("PR"), "{case}: PR counts without -p\n{text}");
    }
}

/// [`OPTION_CASES`] against bash repoz.
#[test]
fn bash_repoz_renders_the_option_cases() {
    let Some(bin) = bash_with_hook() else {
        return;
    };
    let fixture = options_fixture("bash_options");
    for &(case, args, versioned) in OPTION_CASES {
        let live = fixture.run_bash(&bin, args);
        check_option_case(case, &strip_ansi(&String::from_utf8_lossy(&live)));
        check_recording(case, &live, versioned);
    }
}

/// `-s`, `-r`, `-p`, `--author` and `--no-glyphs` byte for byte, alone
/// and together.
#[test]
fn print_mode_matches_bash_repoz_with_options() {
    let fixture = options_fixture("golden_options");
    for &(case, args, versioned) in OPTION_CASES {
        assert_matches_recording(case, &fixture.run_print_mode(args), versioned);
    }
}

/// What each fetch-failed case must show in bash repoz, so a recording
/// that lost the failure (a remote that came back, a fetch skipped)
/// cannot pass: the note in front of the counts, never `synced` for a
/// failed clone, and the summary's count.
fn check_fetch_failed_case(case: &str, text: &str) {
    let row = |name: &str| {
        text.lines()
            .find(|line| line.split_whitespace().any(|word| word == name))
            .unwrap_or_else(|| panic!("{case}: no {name} row in\n{text}"))
    };
    let orgproj = row("orgproj");
    assert!(orgproj.ends_with(" fetch failed"), "{case}: {orgproj}");
    if case == "fetch-failed-all" {
        let alpha = row("alpha-tool");
        assert!(
            alpha.ends_with(" fetch failed, 1\u{2193}"),
            "{case}: {alpha}"
        );
        assert!(text.contains("2 fetch failed"), "{case}: summary\n{text}");
        assert!(!text.contains("synced"), "{case}: synced\n{text}");
    }
}

/// [`FETCH_FAILED_CASES`] against bash repoz.
#[test]
fn bash_repoz_renders_the_fetch_failed_cases() {
    let Some(bin) = bash_with_hook() else {
        return;
    };
    let fixture = Fixture::build("bash_fetch_failed");
    fixture.add_missing_remotes();
    for &(case, args, versioned) in FETCH_FAILED_CASES {
        let live = fixture.run_bash(&bin, args);
        check_fetch_failed_case(case, &strip_ansi(&String::from_utf8_lossy(&live)));
        check_recording(case, &live, versioned);
    }
}

/// D4's failure path byte for byte: a clone whose remote does not exist
/// prints `fetch failed` in front of its counts, is never `synced`, and
/// the summary counts it.
#[test]
fn print_mode_matches_bash_repoz_with_failed_fetches() {
    let fixture = Fixture::build("golden_fetch_failed");
    fixture.add_missing_remotes();
    for &(case, args, versioned) in FETCH_FAILED_CASES {
        assert_matches_recording(case, &fixture.run_print_mode(args), versioned);
    }
}

#[test]
fn print_mode_matches_bash_repoz_with_before() {
    let fixture = Fixture::build("golden_before");
    for &(case, args) in BEFORE_CASES {
        let theirs = recorded(case).expect("no recording");
        let theirs = without_version_line(&theirs).expect("recording has no version line");
        let ours = fixture.run_print_mode(args);
        let ours = without_version_line(&ours).expect("print mode printed no version line");
        assert!(
            ours == theirs,
            "{case}: print mode differs from bash repoz\n--- bash repoz\n{}\n--- print mode\n{}",
            String::from_utf8_lossy(&theirs),
            String::from_utf8_lossy(&ours)
        );
    }
}
