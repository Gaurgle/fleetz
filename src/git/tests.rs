use super::*;
use crate::model::RefKind;

/// FZ-60: porcelain's `-z` form names a rename's destination first and
/// its source in the next field, keeps both status columns, and leaves
/// a path with spaces and non-ASCII as it is named.
#[test]
fn porcelain_z_parses_status_columns_renames_and_raw_paths() {
    let out = "R  new näme.txt\0old name.txt\0 M a b.txt\0A  s.txt\0?? scratch/\0";
    let parsed = parse_porcelain_z(out);
    let row = |status: &str, path: &str, from: Option<&str>| ChangedFile {
        status: status.to_string(),
        path: path.to_string(),
        from: from.map(str::to_string),
        churn: None,
    };
    assert_eq!(
        parsed,
        vec![
            row("R ", "new näme.txt", Some("old name.txt")),
            row(" M", "a b.txt", None),
            row("A ", "s.txt", None),
            row("??", "scratch/", None),
        ]
    );
}

/// `--name-status -z`: one letter per file, a rename's score dropped and
/// its two paths read as source then destination.
#[test]
fn name_status_z_reads_renames_as_source_then_destination() {
    let parsed = parse_name_status_z("M\0a.txt\0R100\0old.txt\0new.txt\0D\0gone.txt\0");
    let paths: Vec<_> = parsed
        .iter()
        .map(|f| (f.status.as_str(), f.path.as_str(), f.from.as_deref()))
        .collect();
    assert_eq!(
        paths,
        vec![
            ("M", "a.txt", None),
            ("R", "new.txt", Some("old.txt")),
            ("D", "gone.txt", None),
        ]
    );
}

/// `--numstat -z`: a rename keyed by its destination, a binary file
/// with no churn.
#[test]
fn numstat_z_keys_renames_by_destination_and_binaries_by_none() {
    let churn = parse_numstat_z("3\t1\ta.txt\x000\t0\t\0old.txt\0new.txt\0-\t-\tlogo.png\0");
    assert_eq!(churn.get("a.txt"), Some(&Some((3, 1))));
    assert_eq!(churn.get("new.txt"), Some(&Some((0, 0))));
    assert_eq!(churn.get("old.txt"), None);
    assert_eq!(churn.get("logo.png"), Some(&None));
}

/// A local branch for the `unfetched_branches` tests. Only the name and
/// the upstream take part in that decision.
fn local_branch(name: &str, upstream: Option<&str>) -> BranchStatus {
    BranchStatus {
        name: name.to_string(),
        upstream: upstream.map(|u| u.to_string()),
        behind: 0,
        ahead: 0,
        is_head: false,
        tip: String::new(),
    }
}

/// The failure this closes: a branch row had no way to reach its own
/// commit, so the branches view could not feed the inspector. The hash
/// rides the `for-each-ref` that was already asking for `%(HEAD)` rather
/// than costing a second subprocess.
#[test]
fn branch_output_carries_the_tip_hash() {
    let out = "*\tmain\torigin/main\t[ahead 2]\ta1b2c3d\n \tbeta\t\t\te4f5a6b\n";
    let parsed = parse_branch_output(out);
    assert_eq!(parsed[0].name, "main");
    assert_eq!(parsed[0].tip, "a1b2c3d");
    assert_eq!(parsed[1].name, "beta");
    assert_eq!(parsed[1].tip, "e4f5a6b");
}

/// A repo with no commits yields no object name. The field is empty rather
/// than the row being dropped: the branch exists and must still be listed.
#[test]
fn a_branch_with_no_tip_parses_with_an_empty_hash() {
    let parsed = parse_branch_output("*\tmain\t\t\t\n");
    assert_eq!(parsed[0].name, "main");
    assert_eq!(parsed[0].tip, "");
}

/// Remote branches for the `unfetched_branches` tests. Only the name takes
/// part in that decision; the tip rides along for the branches view.
fn remotes(names: &[&str]) -> Vec<RemoteBranch> {
    names
        .iter()
        .map(|n| RemoteBranch {
            name: (*n).to_string(),
            tip: String::new(),
        })
        .collect()
}

/// The failure this closes: only the *unfetched* subset survived the
/// sweep, so a branches view had no way to list the remotes that are
/// fetched. The full list was already being computed and thrown away.
#[test]
fn remote_branch_output_carries_names_and_tips() {
    let out = "refs/remotes/origin/HEAD\torigin\tdeadbee\n\
               refs/remotes/origin/main\torigin/main\ta1b2c3d\n\
               refs/remotes/origin/wip\torigin/wip\te4f5a6b\n";
    let parsed = parse_remote_branch_output(out);
    assert_eq!(parsed.len(), 2, "the HEAD symref must still be dropped");
    assert_eq!(parsed[0].name, "origin/main");
    assert_eq!(parsed[0].tip, "a1b2c3d");
    assert_eq!(parsed[1].name, "origin/wip");
    assert_eq!(parsed[1].tip, "e4f5a6b");
}

#[test]
fn unfetched_branches_skips_ones_with_local_counterpart() {
    let remote = remotes(&["origin/main", "origin/feature-x"]);
    let local = vec![local_branch("main", Some("origin/main"))];
    assert_eq!(
        unfetched_branches(&remote, &local),
        vec!["origin/feature-x"]
    );
}

#[test]
fn unfetched_branches_empty_when_all_fetched() {
    let remote = remotes(&["origin/main"]);
    let local = vec![local_branch("main", Some("origin/main"))];
    assert!(unfetched_branches(&remote, &local).is_empty());
}

#[test]
fn unfetched_branches_handles_nested_remote_names() {
    // A remote name with slashes in the branch itself (e.g. "origin/feat/x")
    // should still compare against the bare local branch "feat/x".
    let remote = remotes(&["origin/feat/x"]);
    let local = vec![local_branch("feat/x", None)];
    assert!(unfetched_branches(&remote, &local).is_empty());
}

#[test]
fn unfetched_branches_counts_a_tracked_remote_as_fetched_under_any_name() {
    // The regression: a local branch may be named nothing like the ref it
    // tracks. `origin/wip` is fetched and tracked, so it is not unfetched,
    // even though no local branch is called "wip".
    let remote = remotes(&["origin/wip", "origin/stale"]);
    let local = vec![local_branch("feature/who-goes-first", Some("origin/wip"))];
    assert_eq!(unfetched_branches(&remote, &local), vec!["origin/stale"]);
}

#[test]
fn unfetched_branches_matches_upstream_on_the_full_remote_name() {
    // The upstream test compares against the "origin/x" form, not the bare
    // branch, so a same-named branch on a *different* remote stays
    // unfetched rather than being covered by this one.
    let remote = remotes(&["upstream/wip"]);
    let local = vec![local_branch("mine", Some("origin/wip"))];
    assert_eq!(unfetched_branches(&remote, &local), vec!["upstream/wip"]);
}

#[test]
fn parse_track_info_ahead_and_behind() {
    assert_eq!(parse_track_info("[ahead 2, behind 3]"), (2, 3));
}

#[test]
fn parse_track_info_ahead_only() {
    assert_eq!(parse_track_info("[ahead 5]"), (5, 0));
}

#[test]
fn parse_track_info_behind_only() {
    assert_eq!(parse_track_info("[behind 1]"), (0, 1));
}

#[test]
fn parse_track_info_empty() {
    assert_eq!(parse_track_info(""), (0, 0));
}

#[test]
fn parse_branch_output_basic() {
    // `%(HEAD)` leads each row: "*" on the checked-out branch, a single
    // space on the others. Taken from real `for-each-ref` output.
    let output = " \tmain\torigin/main\t[behind 3]\n*\tfeat/x\torigin/feat/x\t[ahead 1]\n";
    let branches = parse_branch_output(output);
    assert_eq!(branches.len(), 2);
    assert_eq!(branches[0].name, "main");
    assert_eq!(branches[0].behind, 3);
    assert_eq!(branches[0].ahead, 0);
    assert_eq!(branches[1].name, "feat/x");
    assert_eq!(branches[1].behind, 0);
    assert_eq!(branches[1].ahead, 1);
}

/// Which branch you are on. Nothing carried it before, so the detail pane
/// drew two level branches identically and could not say which was HEAD.
#[test]
fn parse_branch_output_marks_the_checked_out_branch() {
    let output = " \tmain\torigin/main\t\n*\tfeat/x\torigin/feat/x\t\n";
    let branches = parse_branch_output(output);
    assert!(!branches[0].is_head, "main is not checked out");
    assert!(branches[1].is_head, "feat/x is");
    assert_eq!(
        branches.iter().filter(|b| b.is_head).count(),
        1,
        "exactly one branch can be HEAD"
    );
}

#[test]
fn parse_branch_output_no_upstream() {
    let output = "*\tlocal-only\t\t\n";
    let branches = parse_branch_output(output);
    assert_eq!(branches.len(), 1);
    assert_eq!(branches[0].name, "local-only");
    assert!(branches[0].upstream.is_none());
    assert!(branches[0].is_head);
}

/// Regression: `refs/remotes/origin/HEAD` shortens to bare `origin`, so
/// filtering the short name for a "/HEAD" suffix never fired and the
/// remote's default-branch symref rendered as a branch you had never
/// fetched. It is not a branch, and it points at `origin/main`, which is
/// fetched. Taken verbatim from a live repo.
#[test]
fn remote_branches_drop_the_default_branch_symref() {
    let stdout = "refs/remotes/origin/HEAD\torigin\n\
                  refs/remotes/origin/main\torigin/main\n";
    let remotes = parse_remote_branch_output(stdout);
    assert!(
        !remotes.iter().any(|r| r.name == "origin"),
        "the symref leaked: {remotes:?}"
    );
    let names: Vec<&str> = remotes.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["origin/main"]);
}

/// The widened query lists every ref. The branches view gets only the
/// remote-tracking ones; the key gets everything outside `refs/heads/`.
#[test]
fn the_widened_ref_query_splits_by_prefix() {
    let stdout = "refs/heads/main\tmain\taaa\taaaa1\n\
                  refs/notes/commits\tnotes/commits\tbbb\tbbbb2\n\
                  refs/remotes/origin/HEAD\torigin\tccc\tcccc3\n\
                  refs/remotes/origin/main\torigin/main\tccc\tcccc3\n\
                  refs/stash\tstash\tddd\tdddd4\n\
                  refs/tags/rc/1\trc/1\teee\teeee5\n";
    let remotes = parse_remote_branch_output(stdout);
    assert_eq!(
        remotes,
        vec![RemoteBranch {
            name: "origin/main".to_string(),
            tip: "ccc".to_string(),
        }]
    );
    let other = parse_other_refs(stdout);
    let names: Vec<&str> = other.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "refs/notes/commits",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            "refs/stash",
            "refs/tags/rc/1",
        ]
    );
    assert_eq!(other[3].1, "dddd4", "the full id");
}

/// The key's half of the branch query: full refnames and ids, and the
/// branch marked `*` as the HEAD symref.
#[test]
fn parse_branch_refs_keeps_full_ids_and_the_head_symref() {
    let output = " \tside\t\t\taaa\trefs/heads/side\taaaa1\n\
                  *\tmain\torigin/main\t\tbbb\trefs/heads/main\tbbbb2\n";
    let (refs, head) = parse_branch_refs(output);
    assert_eq!(
        refs,
        vec![
            ("refs/heads/side".to_string(), "aaaa1".to_string()),
            ("refs/heads/main".to_string(), "bbbb2".to_string()),
        ]
    );
    assert_eq!(head.as_deref(), Some("refs/heads/main"));

    let detached = " \tmain\t\t\tbbb\trefs/heads/main\tbbbb2\n";
    assert_eq!(parse_branch_refs(detached).1, None);
}

/// A branch legitimately called `.../HEAD-something` must survive: the
/// filter matches the suffix, not the substring.
#[test]
fn a_branch_whose_name_merely_contains_head_survives() {
    let stdout = "refs/remotes/origin/HEADroom\torigin/HEADroom\n\
                  refs/remotes/origin/fix/HEAD-parsing\torigin/fix/HEAD-parsing\n";
    let remotes = parse_remote_branch_output(stdout);
    assert_eq!(remotes.len(), 2, "got {remotes:?}");
}

/// Real `git worktree list --porcelain` output, one entry per blank-line
/// separated block. Built with `concat!` rather than a continued string
/// literal: rustfmt bakes a continuation's indentation into the value,
/// which silently stops every line matching its prefix.
fn worktree_block(path: &str, sha: &str, branch: &str) -> String {
    format!("worktree {path}\nHEAD {sha}\nbranch refs/heads/{branch}\n")
}

/// `git worktree list` always reports the main worktree first. It is just
/// the repo, whose path the pane header already prints, so listing it
/// again under a branch duplicated that line in a second spelling.
#[test]
fn parse_worktree_output_skips_the_main_worktree() {
    let main = std::path::Path::new("/Users/me/repos/pinz");
    let output = worktree_block("/Users/me/repos/pinz", "16d99c6", "feat/header-version");
    assert!(parse_worktree_output(&output, main).is_empty());
}

#[test]
fn parse_worktree_output_keeps_linked_worktrees() {
    let main = std::path::Path::new("/Users/me/repos/pinz");
    let output = format!(
        "{}\n{}",
        worktree_block("/Users/me/repos/pinz", "16d99c6", "main"),
        worktree_block("/Users/me/repos/pinz-hotfix", "abc1234", "hotfix"),
    );
    let wts = parse_worktree_output(&output, main);
    assert_eq!(wts.len(), 1, "only the linked one, got {wts:?}");
    assert_eq!(wts[0].branch, "hotfix");
    // Trailing entry with no blank line after it still lands.
    assert!(wts[0].path.ends_with("pinz-hotfix"));
}

/// A detached worktree is a bisect or a review checkout, exactly the thing
/// that is forgotten. It was dropped for having no `branch` line; it is
/// kept now, named by its short HEAD.
#[test]
fn parse_worktree_output_keeps_a_detached_worktree() {
    let main = std::path::Path::new("/Users/me/repos/pinz");
    let output = format!(
        "{}\nworktree /Users/me/scratch/bisect\nHEAD 9a2eb79c0ffee1234567890abcdef1234567890\ndetached\n",
        worktree_block("/Users/me/repos/pinz", "16d99c6", "main"),
    );
    let wts = parse_worktree_output(&output, main);
    assert_eq!(wts.len(), 1, "the detached one is listed, got {wts:?}");
    assert!(wts[0].detached);
    assert_eq!(wts[0].head, "9a2eb79");
    assert_eq!(wts[0].branch, "");
}

#[test]
fn a_branch_worktree_carries_its_head_and_is_not_detached() {
    let main = std::path::Path::new("/Users/me/repos/pinz");
    let output = format!(
        "{}\n{}",
        worktree_block("/Users/me/repos/pinz", "16d99c6", "main"),
        worktree_block(
            "/Users/me/repos/pinz-hotfix",
            "abc1234def5678abc1234def5678abc1234def56",
            "hotfix"
        ),
    );
    let wts = parse_worktree_output(&output, main);
    assert_eq!(wts.len(), 1);
    assert!(!wts[0].detached);
    assert_eq!(wts[0].head, "abc1234");
    assert_eq!(wts[0].branch, "hotfix");
}

/// Regression: `format!("~{}", ..)` dropped the separator and printed
/// `~Repos/pinz`, which is not a path anything would accept.
#[test]
fn a_worktree_under_home_keeps_its_path_separator() {
    let home = dirs::home_dir().expect("home");
    let linked = home.join("work").join("side");
    let output = worktree_block(&linked.display().to_string(), "abc1234", "side");
    let wts = parse_worktree_output(&output, std::path::Path::new("/nowhere"));
    assert_eq!(wts[0].path, "~/work/side", "got {}", wts[0].path);
}

/// The main-worktree filter compares paths that reach the same directory
/// by different spellings: git reports the real casing while a configured
/// `repo_dir` may carry another, and on a case-insensitive filesystem
/// both resolve. A raw `==` missed that and leaked the repo root.
#[test]
fn same_dir_sees_through_a_symlinked_spelling() {
    let base = std::env::temp_dir().join("fleetz_same_dir_test");
    let _ = std::fs::remove_dir_all(&base);
    let real = base.join("real");
    std::fs::create_dir_all(&real).expect("create");
    let link = base.join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &link).expect("symlink");

    assert!(same_dir(&real, &real), "a path matches itself");
    #[cfg(unix)]
    assert!(same_dir(&link, &real), "a symlink reaches the same dir");
    assert!(
        !same_dir(&real, &base.join("other")),
        "different dirs stay different"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn parse_commit_output_without_stat() {
    // Records: RS h US author US epoch US subject US body
    let output = "\u{1e}a1b2c3d\u{1f}Alice\u{1f}1712000000\u{1f}feat: add feature\u{1f}\u{1e}e4f5g6h\u{1f}Bob\u{1f}1712080000\u{1f}fix: bug\u{1f}";
    let commits = parse_commit_output(output);
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].hash, "a1b2c3d");
    assert_eq!(commits[0].author, "Alice");
    assert_eq!(
        commits[0].date,
        DateTime::from_timestamp(1712000000, 0),
        "%at is parsed into an absolute timestamp, not a frozen string"
    );
    assert_eq!(commits[0].message, "feat: add feature");
    assert_eq!(commits[0].body, "");
    assert_eq!(commits[0].insertions, 0);
}

#[test]
fn parse_commit_output_survives_an_unparseable_date() {
    // A git that hands back something other than %at must cost the age,
    // not the whole commit row.
    let output = "\u{1e}a1b2c3d\u{1f}Alice\u{1f}not-a-timestamp\u{1f}feat: x\u{1f}";
    let commits = parse_commit_output(output);
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].hash, "a1b2c3d");
    assert_eq!(commits[0].date, None);
}

#[test]
fn parse_commit_output_with_stat() {
    let output = "\u{1e}a1b2c3d\u{1f}Alice\u{1f}1712000000\u{1f}feat: add feature\u{1f} 3 files changed, 18 insertions(+), 4 deletions(-)\n\u{1e}e4f5g6h\u{1f}Bob\u{1f}1712080000\u{1f}fix: bug\u{1f} 1 file changed, 2 insertions(+)\n";
    let commits = parse_commit_output(output);
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].insertions, 18);
    assert_eq!(commits[0].deletions, 4);
    assert_eq!(commits[1].insertions, 2);
    assert_eq!(commits[1].deletions, 0);
}

#[test]
fn parse_commit_output_multiline_body_and_stat() {
    // A body with its own newlines, then a shortstat line -- the body is
    // preserved and the stat is pulled out, not treated as body text.
    let output = "\u{1e}a1b2c3d\u{1f}Alice\u{1f}1 day ago\u{1f}feat: thing\u{1f}Longer explanation.\n\nSecond paragraph.\n 2 files changed, 9 insertions(+)\n";
    let commits = parse_commit_output(output);
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].message, "feat: thing");
    assert_eq!(commits[0].body, "Longer explanation.\n\nSecond paragraph.");
    assert_eq!(commits[0].insertions, 9);
}

#[test]
fn parse_commit_output_collects_numstat_files() {
    // Body, then numstat lines, then the shortstat summary -- as emitted
    // by `git log --numstat --shortstat`. Files land in `files`, totals in
    // insertions/deletions, and neither leaks into the body.
    let output = "\u{1e}a1b2c3d\u{1f}Alice\u{1f}1712000000\u{1f}feat: add feature\u{1f}Body text.\n12\t3\tsrc/main.rs\n0\t7\tREADME.md\n-\t-\tlogo.png\n 3 files changed, 12 insertions(+), 10 deletions(-)\n";
    let commits = parse_commit_output(output);
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].body, "Body text.");
    assert_eq!(commits[0].insertions, 12);
    assert_eq!(commits[0].deletions, 10);
    assert_eq!(commits[0].files.len(), 3);
    assert_eq!(
        commits[0].files[0],
        FileStat {
            path: "src/main.rs".into(),
            added: 12,
            removed: 3
        }
    );
    assert_eq!(
        commits[0].files[1],
        FileStat {
            path: "README.md".into(),
            added: 0,
            removed: 7
        }
    );
    // Binary file: numstat reports `-` for both counts, stored as 0/0.
    assert_eq!(
        commits[0].files[2],
        FileStat {
            path: "logo.png".into(),
            added: 0,
            removed: 0
        }
    );
}

#[test]
fn parse_numstat_line_rejects_non_numstat() {
    // A body line that merely contains a tab is not a numstat line.
    assert!(parse_numstat_line("just some\tprose here").is_none());
    assert!(parse_numstat_line("no tabs at all").is_none());
    assert!(parse_numstat_line("12\t3").is_none()); // missing path
    assert_eq!(
        parse_numstat_line("5\t2\tsrc/app.rs"),
        Some(FileStat {
            path: "src/app.rs".into(),
            added: 5,
            removed: 2
        })
    );
}

#[test]
fn extract_stat_number_works() {
    assert_eq!(
        extract_stat_number(
            " 3 files changed, 18 insertions(+), 4 deletions(-)",
            "insertion"
        ),
        18
    );
    assert_eq!(
        extract_stat_number(" 1 file changed, 2 insertions(+)", "deletion"),
        0
    );
}

#[test]
fn parse_reflog_output_splits_action_from_description() {
    let output =
        "REFLOG|a1b2c3d|commit: add feature\nREFLOG|e4f5g6h|checkout: moving from main to feat\n";
    let entries = parse_reflog_output(output);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].hash, "a1b2c3d");
    assert_eq!(entries[0].action, "commit");
    assert_eq!(entries[0].description, "add feature");
    assert_eq!(entries[1].action, "checkout");
    assert_eq!(entries[1].description, "moving from main to feat");
}

#[test]
fn parse_reflog_output_no_colon_keeps_whole_subject_as_description() {
    let output = "REFLOG|a1b2c3d|pull origin main\n";
    let entries = parse_reflog_output(output);
    assert_eq!(entries[0].action, "");
    assert_eq!(entries[0].description, "pull origin main");
}

#[test]
fn parse_graph_output_commit_line() {
    // `%D` under `--decorate=full`: no parens, no leading space, and every
    // ref spelled out to its refs/ prefix.
    let output =
        "a1b2c3d|9999999|HEAD -> refs/heads/main, refs/remotes/origin/main|Alice|feat: add feature";
    let lines = parse_graph_output(output);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].hash, "a1b2c3d");
    assert_eq!(lines[0].author, "Alice");
    assert_eq!(lines[0].message, "feat: add feature");

    let refs = &lines[0].refs;
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].kind, RefKind::Local);
    assert_eq!(refs[0].name, "main");
    assert!(refs[0].is_head, "HEAD is attached to main");
    assert_eq!(refs[1].kind, RefKind::Remote);
    assert_eq!(refs[1].name, "origin/main");
    assert!(!refs[1].is_head);
}

/// The prefixes are what make the classification safe. A local branch
/// literally named `origin/thing` is legal, and the short `%d` form draws
/// it identically to the remote-tracking ref of the same name -- which is
/// why this reads `--decorate=full` rather than guessing from the name.
#[test]
fn a_local_branch_named_like_a_remote_is_not_mistaken_for_one() {
    let output = "a1b2c3d|9999999|refs/heads/origin/thing|Alice|wip";
    let refs = &parse_graph_output(output)[0].refs;
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].kind, RefKind::Local);
    assert_eq!(refs[0].name, "origin/thing");
}

#[test]
fn parse_graph_output_classifies_tags_and_a_detached_head() {
    // git keeps its "tag: " marker under --decorate=full; the ref is
    // spelled out behind it rather than in place of it.
    let output = "a1b2c3d|9999999|HEAD, tag: refs/tags/v1.0|Alice|release";
    let refs = &parse_graph_output(output)[0].refs;
    assert_eq!(refs.len(), 2);
    // Detached: HEAD stands alone rather than riding a branch, so it must
    // not pick up the arrow that would claim it points at the tag.
    assert_eq!(refs[0].kind, RefKind::Head);
    assert_eq!(refs[0].name, "HEAD");
    assert!(!refs[0].is_head);
    assert_eq!(refs[1].kind, RefKind::Tag);
    assert_eq!(refs[1].name, "v1.0");
}

/// The real shape of a tagged release commit, taken from a live repo
/// rather than written from memory: HEAD attached to a branch, a tag, and
/// two remote refs, all in one decoration list.
#[test]
fn parse_graph_output_handles_a_tagged_release_commit() {
    let output = "78cdfac|9999999 8888888|HEAD -> refs/heads/main, tag: refs/tags/v0.4.0, refs/remotes/origin/main, refs/remotes/origin/HEAD|AR|release";
    let refs = &parse_graph_output(output)[0].refs;
    let got: Vec<_> = refs.iter().map(|r| (r.kind, r.name.as_str())).collect();
    assert_eq!(
        got,
        vec![
            (RefKind::Local, "main"),
            (RefKind::Tag, "v0.4.0"),
            (RefKind::Remote, "origin/main"),
            (RefKind::Remote, "origin/HEAD"),
        ]
    );
    assert!(refs[0].is_head);
    // The regression: "tag: " survived the strip and the raw
    // "tag: refs/tags/v0.4.0" was drawn in the pane.
    assert!(
        !refs[1].name.contains("refs/"),
        "prefix leaked into the drawn name: {}",
        refs[1].name
    );
}

/// Git's `--graph` draws every commit as `*`, so a merge is only
/// distinguishable by its parent count. The root commit has none, which
/// must not be mistaken for a merge by counting anything other than the
/// number of entries.
#[test]
fn a_merge_is_recognised_by_its_parent_count() {
    let cases = [
        ("9999999", false, "one parent, ordinary"),
        ("9999999 8888888", true, "two parents, a merge"),
        ("9999999 8888888 7777777", true, "an octopus merge"),
        ("", false, "the root commit has no parents"),
    ];
    for (parents, expected, why) in cases {
        let output = format!("a1b2c3d|{parents}||Alice|subject");
        let lines = parse_graph_output(&output);
        assert_eq!(lines[0].is_merge, expected, "{why}");
    }
}

#[test]
fn a_commit_with_no_decorations_has_no_refs() {
    let output = "a1b2c3d|9999999||Alice|feat: add feature";
    assert!(parse_graph_output(output)[0].refs.is_empty());
}

/// The property that replaced the connector-row handling this parser used
/// to need. `git log --graph` interleaves rows carrying no commit, which
/// forced every consumer to check for an empty hash; dropping `--graph`
/// means a row and a commit are now the same thing.
#[test]
fn every_row_carries_a_commit() {
    let output =
        "a1b2c3d|9999999||Alice|feat: add feature\n\nd4e5f6g|a1b2c3d||Bob|other branch commit\n";
    let lines = parse_graph_output(output);
    assert_eq!(lines.len(), 2, "the blank line is not a row");
    assert!(
        lines.iter().all(|l| !l.hash.is_empty()),
        "a row with no commit on it got through"
    );
    assert_eq!(lines[1].hash, "d4e5f6g");
    assert_eq!(lines[1].author, "Bob");
}

/// The graph column is computed here, not read out of git, so the parser
/// owes each row its lanes. A child and its parent share a lane.
#[test]
fn parsing_lays_out_the_graph_column() {
    let output = "a1b2c3d|d4e5f6g||Alice|child\nd4e5f6g|7777777||Alice|parent";
    let lines = parse_graph_output(output);
    let drawn: Vec<String> = lines
        .iter()
        .map(|l| l.lanes.iter().map(|c| c.glyph).collect())
        .collect();
    assert_eq!(drawn, vec![crate::graph::COMMIT_NODE.to_string(); 2]);
}

fn start_day() -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str("2025-08-17", "%Y-%m-%d").unwrap()
}

#[test]
fn parse_daily_accumulates_repeated_days() {
    let out = "2025-08-17\n2025-08-17\n2025-08-18\n";
    let counts = parse_daily(out, start_day());
    assert_eq!(counts.0[0], 2);
    assert_eq!(counts.0[1], 1);
    assert_eq!(counts.0[2], 0, "a day with no commits stays zero");
}

#[test]
fn parse_daily_ignores_days_outside_the_grid() {
    let out = "2020-01-01\n2025-08-16\n2025-08-17\n";
    let counts = parse_daily(out, start_day());
    assert_eq!(counts.0.iter().map(|n| *n as u32).sum::<u32>(), 1);
}

#[test]
fn parse_daily_survives_junk_lines() {
    // A git that printed a warning to stdout must not poison the year.
    let out = "warning: something\n\n2025-08-17\n";
    let counts = parse_daily(out, start_day());
    assert_eq!(counts.0[0], 1);
}

#[test]
fn parse_daily_fills_the_last_slot() {
    let last = start_day() + chrono::Duration::days((crate::contributions::SLOTS - 1) as i64);
    let out = format!("{}\n", last.format("%Y-%m-%d"));
    let counts = parse_daily(&out, start_day());
    assert_eq!(counts.0[crate::contributions::SLOTS - 1], 1);
}
