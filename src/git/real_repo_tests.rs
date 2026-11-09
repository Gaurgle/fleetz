use super::*;
use std::path::PathBuf;
use std::process::Command as StdCommand;

fn sh(dir: &Path, args: &[&str]) {
    let status = StdCommand::new(args[0])
        .args(&args[1..])
        .current_dir(dir)
        .status()
        .expect("spawn");
    assert!(status.success(), "command failed: {args:?}");
}

fn temp_repo(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fleetz_it_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    sh(&dir, &["git", "init", "-q", "-b", "main"]);
    sh(&dir, &["git", "config", "user.email", "t@example.com"]);
    sh(&dir, &["git", "config", "user.name", "t"]);
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    sh(&dir, &["git", "add", "."]);
    sh(&dir, &["git", "commit", "-q", "-m", "init"]);
    dir
}

/// FZ-30f: the `script` body reads a run's workflow file from the clone
/// when it has the commit, and gets `None` (so it asks GitHub) for a
/// commit or a file the clone lacks, or a sha that is not hex.
#[tokio::test]
async fn show_file_reads_a_file_at_a_commit_the_clone_has() {
    let dir = temp_repo("showfile");
    let head = StdCommand::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let sha = String::from_utf8_lossy(&head.stdout).trim().to_string();
    std::fs::write(dir.join("a.txt"), "two\n").unwrap();
    sh(&dir, &["git", "commit", "-qam", "two"]);
    assert_eq!(
        show_file(&dir, &sha, "a.txt").await.as_deref(),
        Some("one\n"),
        "the file as that commit has it, not the worktree's"
    );
    let absent = "0123456789abcdef0123456789abcdef01234567";
    assert_eq!(
        show_file(&dir, absent, "a.txt").await,
        None,
        "no such commit"
    );
    assert_eq!(
        show_file(&dir, &sha, "missing.yml").await,
        None,
        "no such file"
    );
    assert_eq!(show_file(&dir, "--help", "a.txt").await, None, "not a sha");
    assert_eq!(show_file(&dir, "HEAD", "a.txt").await, None);
}

/// The failure this closes: creating a tag moves no commit, no index and
/// no stash, so `WorkKey` was identical across the sweep and fleetz skipped
/// the log and graph probes entirely. The tag never reached the pane, which
/// is exactly what the release flow produces: the `chore: release` commit
/// lands first and the tag is created after it, with nothing else moving.
#[tokio::test]
async fn a_new_tag_changes_the_work_key() {
    let dir = temp_repo("tagkey");
    let before = work_key(&dir).await.expect("a key for a real repo");
    sh(&dir, &["git", "tag", "v9.9.9"]);
    let after = work_key(&dir).await.expect("a key after tagging");
    assert_ne!(
        before, after,
        "a new tag left the fingerprint unchanged, so the graph is skipped"
    );
}

/// The other half: the fingerprint must not churn on its own, or every
/// sweep pays for all six probes in every repo and the gate is pointless.
#[tokio::test]
async fn an_idle_repo_keeps_the_same_work_key() {
    let dir = temp_repo("idlekey");
    let first = work_key(&dir).await.expect("a key for a real repo");
    let second = work_key(&dir).await.expect("a second key");
    assert_eq!(
        first, second,
        "the fingerprint moved with nothing to move it"
    );
}

/// Deleting a tag has to count too: the decoration has to come off the
/// pane as readily as it went on.
#[tokio::test]
async fn deleting_a_tag_changes_the_work_key() {
    let dir = temp_repo("untagkey");
    sh(&dir, &["git", "tag", "v9.9.9"]);
    let before = work_key(&dir).await.expect("a key");
    sh(&dir, &["git", "tag", "-d", "v9.9.9"]);
    let after = work_key(&dir).await.expect("a key");
    assert_ne!(
        before, after,
        "a deleted tag left the fingerprint unchanged"
    );
}

/// A linked worktree of `main_dir`, checked out on a new branch.
fn linked_worktree(main_dir: &Path, tag: &str) -> PathBuf {
    let wt = std::env::temp_dir().join(format!("fleetz_it_{tag}_wt_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&wt);
    let wt_arg = wt.to_string_lossy().into_owned();
    sh(
        main_dir,
        &["git", "worktree", "add", "-q", "-b", tag, &wt_arg],
    );
    wt
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).expect("path exists")
}

/// The gate in front of `git worktree list`: a clone that never had a
/// linked worktree, or lost its last one, spawns no process for it, and
/// one that has one always asks git. `linked_worktrees` runs the process
/// only when this says so, which is how an idle sweep of a repo without
/// linked worktrees pays nothing for the list.
#[tokio::test]
async fn worktree_list_runs_only_while_common_worktrees_exists() {
    let dir = temp_repo("wtgate");
    assert!(
        !worktree_probe_needed(&dir),
        "a plain clone has nothing to list"
    );
    assert!(linked_worktrees(&dir).await.is_empty());

    let wt = linked_worktree(&dir, "wtgate");
    assert!(
        worktree_probe_needed(&dir),
        "the main checkout must ask git"
    );
    assert!(worktree_probe_needed(&wt), "a linked worktree must ask git");
    assert_eq!(linked_worktrees(&dir).await.len(), 1);

    sh(&dir, &["git", "worktree", "remove", &wt.to_string_lossy()]);
    assert!(!dir.join(".git/worktrees").exists());
    assert!(
        !worktree_probe_needed(&dir),
        "the last one went, so did the probe"
    );
    assert!(linked_worktrees(&dir).await.is_empty());
}

/// The two per-worktree probes on a real linked worktree: the dirty count
/// follows a file written there, and the HEAD time is the commit's.
#[tokio::test]
async fn worktree_state_reads_the_dirty_count_and_head_time() {
    let dir = temp_repo("wtstate");
    let wt = linked_worktree(&dir, "wtstate");
    sh_at(
        &wt,
        "2024-03-05T10:20:30+00:00",
        &["git", "commit", "-q", "--allow-empty", "-m", "wt work"],
    );
    let (clean, head_at) = worktree_state(&wt).await;
    assert_eq!(clean, Some(0), "a fresh worktree is clean");
    assert_eq!(
        head_at,
        Some(
            chrono::DateTime::parse_from_rfc3339("2024-03-05T10:20:30+00:00")
                .unwrap()
                .with_timezone(&chrono::Utc)
        )
    );

    std::fs::write(wt.join("new.txt"), "wip\n").unwrap();
    std::fs::write(wt.join("a.txt"), "changed\n").unwrap();
    let (dirty, _) = worktree_state(&wt).await;

    let listed = linked_worktrees(&dir).await;
    let _ = std::fs::remove_dir_all(&wt);
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(dirty, Some(2), "one new file and one change");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].dirty, Some(2), "the list carries the probe");
    assert!(listed[0].head_at.is_some());
    assert_eq!(listed[0].head.len(), 7);
}

/// A path that is not a worktree is unknown, never clean.
#[tokio::test]
async fn worktree_state_is_unknown_off_a_worktree() {
    let dir = std::env::temp_dir().join(format!("fleetz_it_notwt_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let state = worktree_state(&dir).await;
    let gone = worktree_state(&dir.join("missing")).await;
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(state, (None, None));
    assert_eq!(gone, (None, None));
}

/// The parts of `WorkKey` shared by every checkout of one clone: the
/// stash log and shallow file read from the common dir, and the refs.
fn shared_fields(key: &WorkKey) -> (u64, u64) {
    (key.stash_size, key.ref_signature)
}

/// Tag and stash in `main_dir`, then check that `checkout`, another
/// checkout of the same clone, reads them through the common dir: the
/// tag moves its ref signature and the stash log has a size there.
async fn assert_reads_the_common_dir(main_dir: &Path, checkout: &Path) {
    let before = work_key(checkout).await.expect("checkout key");
    sh(main_dir, &["git", "tag", "v1"]);
    std::fs::write(main_dir.join("a.txt"), "stash me\n").unwrap();
    sh(main_dir, &["git", "stash", "-q"]);
    let main_key = work_key(main_dir).await.expect("main key");
    let key = work_key(checkout).await.expect("checkout key");
    assert_eq!(shared_fields(&main_key), shared_fields(&key));
    assert_ne!(
        before.ref_signature, key.ref_signature,
        "a tag made in the main checkout did not reach this one"
    );
    assert!(
        key.stash_size > 0,
        "the stash log was not read from the common dir"
    );

    // The shallow file lives in the common dir too.
    let common = repo_dirs(checkout).expect("resolves").common_dir;
    std::fs::write(common.join("shallow"), format!("{}\n", key.head_sha)).unwrap();
    let shallow = work_key(checkout).await.expect("checkout key");
    assert_ne!(
        key.ref_signature, shallow.ref_signature,
        "the shallow file was not read from the common dir"
    );
}

#[tokio::test]
async fn a_plain_clone_keeps_both_dirs_in_dot_git() {
    let dir = temp_repo("dirs_plain");
    let dot = dir.join(".git");

    let dirs = repo_dirs(&dir).expect("a plain clone resolves");
    assert_eq!(dirs.git_dir, dot);
    assert_eq!(dirs.common_dir, dot);
    assert_eq!(resolve_git_dir(&dir), Some(dot.clone()));

    assert_eq!(last_fetch_at(&dir), None, "a fresh repo never fetched");
    std::fs::write(dot.join("FETCH_HEAD"), "").unwrap();
    assert!(last_fetch_at(&dir).is_some());

    std::fs::remove_dir_all(&dir).ok();
}

/// git writes a linked worktree's `commondir` as the relative `../..`,
/// and its `FETCH_HEAD` beside its own `HEAD`, not in the main clone.
#[tokio::test]
async fn a_linked_worktree_shares_the_main_clones_common_dir() {
    let main_dir = temp_repo("dirs_linked");
    let wt = linked_worktree(&main_dir, "dirs_linked");

    let dirs = repo_dirs(&wt).expect("a linked worktree resolves");
    assert_eq!(Some(dirs.git_dir.clone()), resolve_git_dir(&wt));
    // git names the admin dir after the worktree's folder.
    let admin = main_dir
        .join(".git/worktrees")
        .join(wt.file_name().expect("named"));
    assert_eq!(canonical(&dirs.git_dir), canonical(&admin));
    assert_eq!(dirs.common_dir, dirs.git_dir.join("../.."));
    assert_eq!(
        canonical(&dirs.common_dir),
        canonical(&main_dir.join(".git"))
    );

    // The key reads refs and the stash log from the common dir.
    assert_reads_the_common_dir(&main_dir, &wt).await;

    // FETCH_HEAD is per worktree.
    std::fs::write(main_dir.join(".git/FETCH_HEAD"), "").unwrap();
    assert_eq!(last_fetch_at(&wt), None, "main clone's fetch is not ours");
    std::fs::write(dirs.git_dir.join("FETCH_HEAD"), "").unwrap();
    assert!(last_fetch_at(&wt).is_some());

    std::fs::remove_dir_all(&wt).ok();
    std::fs::remove_dir_all(&main_dir).ok();
}

/// An absolute `commondir` replaces the git dir outright, and the
/// trailing newline git writes is not part of the path.
#[tokio::test]
async fn an_absolute_commondir_is_used_as_is() {
    let main_dir = temp_repo("dirs_abs");
    let wt = linked_worktree(&main_dir, "dirs_abs");
    let git_dir = resolve_git_dir(&wt).expect("worktree git dir");
    let common = canonical(&main_dir.join(".git"));
    std::fs::write(git_dir.join("commondir"), format!("{}\n", common.display())).unwrap();

    let dirs = repo_dirs(&wt).expect("resolves");
    assert_eq!(dirs.git_dir, git_dir);
    assert_eq!(dirs.common_dir, common);

    assert_reads_the_common_dir(&main_dir, &wt).await;

    std::fs::remove_dir_all(&wt).ok();
    std::fs::remove_dir_all(&main_dir).ok();
}

/// A hand-built layout: a relative `gitdir:` pointer and a relative
/// `commondir` padded with whitespace. Both joins stay unnormalized.
#[test]
fn a_relative_commondir_is_trimmed_and_joined_onto_the_git_dir() {
    let root = std::env::temp_dir().join(format!("fleetz_it_dirs_rel_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let checkout = root.join("checkout");
    let store = root.join("store");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(checkout.join(".git"), "gitdir: ../store\n").unwrap();
    std::fs::write(store.join("commondir"), "  ../common \n").unwrap();

    let git_dir = checkout.join("../store");
    let dirs = repo_dirs(&checkout).expect("resolves");
    assert_eq!(dirs.git_dir, git_dir);
    assert_eq!(dirs.common_dir, git_dir.join("../common"));
    assert_eq!(resolve_git_dir(&checkout), Some(git_dir));

    std::fs::remove_dir_all(&root).ok();
}

/// A submodule checkout (here `git init --separate-git-dir`, the same
/// shape) has a `gitdir:` file and no `commondir`: one directory.
#[tokio::test]
async fn a_gitdir_file_without_commondir_has_one_dir() {
    let root = std::env::temp_dir().join(format!("fleetz_it_dirs_sub_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let checkout = root.join("sub");
    let store = root.join("modules").join("sub");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::create_dir_all(root.join("modules")).unwrap();
    let store_arg = store.to_string_lossy().into_owned();
    sh(
        &checkout,
        &[
            "git",
            "init",
            "-q",
            "-b",
            "main",
            "--separate-git-dir",
            &store_arg,
        ],
    );
    sh(&checkout, &["git", "config", "user.email", "t@example.com"]);
    sh(&checkout, &["git", "config", "user.name", "t"]);
    std::fs::write(checkout.join("a.txt"), "one\n").unwrap();
    sh(&checkout, &["git", "add", "."]);
    sh(&checkout, &["git", "commit", "-q", "-m", "init"]);
    assert!(checkout.join(".git").is_file(), "a gitdir: pointer file");
    assert!(!store.join("commondir").exists());

    let dirs = repo_dirs(&checkout).expect("resolves");
    assert_eq!(Some(dirs.git_dir.clone()), resolve_git_dir(&checkout));
    assert_eq!(canonical(&dirs.git_dir), canonical(&store));
    assert_eq!(dirs.common_dir, dirs.git_dir);

    // Refs, stash log and index all come from the one dir.
    assert_reads_the_common_dir(&checkout, &checkout).await;
    let key = work_key(&checkout).await.expect("a key");
    assert!(key.index_size > 0, "index read from the one dir");

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn branch_statuses_and_dirty_count_on_real_repo() {
    let dir = temp_repo("basic");

    let branches = branch_statuses(&dir).await;
    assert_eq!(branches.len(), 1);
    assert_eq!(branches[0].name, "main");
    assert_eq!(branches[0].upstream, None);

    assert_eq!(dirty_count(&dir).await, 0);
    std::fs::write(dir.join("b.txt"), "two\n").unwrap();
    assert_eq!(dirty_count(&dir).await, 1);

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn merged_branches_resolve_against_default_not_current() {
    let dir = temp_repo("merged");

    // Land a feature on main, then park on an unrelated branch. Asking
    // about the current branch would miss the merge; the default branch
    // is the answer the overlay actually wants.
    sh(&dir, &["git", "switch", "-q", "-c", "feature"]);
    std::fs::write(dir.join("f.txt"), "feat\n").unwrap();
    sh(&dir, &["git", "add", "."]);
    sh(&dir, &["git", "commit", "-q", "-m", "feature work"]);
    sh(&dir, &["git", "switch", "-q", "main"]);
    sh(
        &dir,
        &["git", "merge", "-q", "--no-ff", "-m", "merge", "feature"],
    );
    sh(&dir, &["git", "switch", "-q", "-c", "elsewhere"]);

    let base = default_branch(&dir).await;
    assert_eq!(base.as_deref(), Some("main"), "no origin, so main fallback");

    let merged = merged_branches(&dir, "main").await;
    assert!(
        merged.iter().any(|b| b == "feature"),
        "feature merged into main, got {merged:?}"
    );

    let from_current = merged_branches(&dir, "elsewhere").await;
    assert!(
        from_current.iter().any(|b| b == "feature"),
        "sanity: elsewhere descends from the merge too"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Regression: a feature branch created locally and never pushed has an
/// empty `upstream:track`, so fleetz reported ahead=0 and the repo
/// vanished from "local changes" the moment the work was committed. The
/// unpushed commits are the whole point of that section.
#[tokio::test]
async fn unpushed_branch_without_upstream_is_ahead() {
    let dir = temp_repo("noupstream");
    let bare = std::env::temp_dir().join(format!(
        "fleetz_it_noupstream_remote_{}.git",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&bare);
    std::fs::create_dir_all(&bare).unwrap();
    sh(&bare, &["git", "init", "-q", "--bare"]);
    sh(
        &dir,
        &["git", "remote", "add", "origin", &bare.to_string_lossy()],
    );
    sh(&dir, &["git", "push", "-q", "-u", "origin", "main"]);

    sh(&dir, &["git", "switch", "-q", "-c", "feat/x"]);
    std::fs::write(dir.join("b.txt"), "two\n").unwrap();
    sh(&dir, &["git", "add", "."]);
    sh(&dir, &["git", "commit", "-q", "-m", "work"]);

    let branches = branch_statuses(&dir).await;
    let feat = branches
        .iter()
        .find(|b| b.name == "feat/x")
        .expect("feat/x");
    assert_eq!(feat.upstream, None, "branch was never pushed");
    assert_eq!(feat.ahead, 1, "the unpushed commit must be counted");

    let main = branches.iter().find(|b| b.name == "main").expect("main");
    assert_eq!(main.ahead, 0, "main is level with its upstream");

    // Pushing it settles the count back to zero.
    sh(&dir, &["git", "push", "-q", "-u", "origin", "feat/x"]);
    let after = branch_statuses(&dir).await;
    let feat = after.iter().find(|b| b.name == "feat/x").expect("feat/x");
    assert_eq!(feat.ahead, 0, "pushed branch is not ahead");

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&bare).ok();
}

/// A repo with no remote at all has nowhere to push, so every commit is
/// technically "unreachable from remotes". Reporting that would make every
/// scratch repo shout an ahead count forever.
#[tokio::test]
async fn local_repo_without_remotes_is_never_ahead() {
    let dir = temp_repo("noremote");

    let branches = branch_statuses(&dir).await;
    assert_eq!(branches.len(), 1);
    assert_eq!(branches[0].upstream, None);
    assert_eq!(
        branches[0].ahead, 0,
        "no remote means nothing to be ahead of"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// The expanded graph must carry an absolute timestamp, not the relative
/// one git renders once and freezes. The renderer turns it back into an
/// age per frame; with `--date=relative` an idle repo showed "3 minutes
/// ago" for as long as it stayed idle.
#[tokio::test]
async fn expanded_graph_dates_are_absolute_and_parseable() {
    let dir = temp_repo("expdate");

    let lines = graph_expanded(&dir, &["main".to_string()], 5).await;
    let date = lines
        .iter()
        .find_map(|l| l.split_once("Date:"))
        .map(|(_, rest)| rest.trim().to_string())
        .expect("the block has a Date row");
    assert!(
        chrono::DateTime::parse_from_rfc3339(&date).is_ok(),
        "not an absolute timestamp the renderer can re-age: {date:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// The regression: the detail pane draws the graph, which is the union of
/// the local branches, and then looks each row up in this list by hash for
/// its churn and for the inspector's body. Reading HEAD alone here left a
/// side-branch commit drawn but unfindable -- no `+N -M` on the row, and
/// "outside the loaded commit range" in the pane below it.
#[tokio::test]
async fn commits_cover_every_branch_the_graph_draws() {
    let dir = temp_repo("branchunion");
    sh(&dir, &["git", "checkout", "-q", "-b", "side"]);
    std::fs::write(dir.join("b.txt"), "on the side\n").unwrap();
    sh(&dir, &["git", "add", "."]);
    sh(
        &dir,
        &["git", "commit", "-q", "-m", "only on the side branch"],
    );
    sh(&dir, &["git", "checkout", "-q", "main"]);

    let branches = vec!["main".to_string(), "side".to_string()];
    let commits = commits(&dir, "-20", true, None, &branches).await;
    let graph = graph(&dir, &branches, 20).await;

    assert!(
        commits
            .iter()
            .any(|c| c.message == "only on the side branch"),
        "the side branch commit is missing: {:?}",
        commits.iter().map(|c| &c.message).collect::<Vec<_>>()
    );
    // Every drawn row is findable, which is the property the pane needs.
    for row in &graph {
        assert!(
            commits.iter().any(|c| c.hash == row.hash),
            "graph draws {} but the commit list has no such hash",
            row.hash
        );
    }

    // HEAD alone does not, which is what makes the assertion above worth
    // making: the two lists really do differ on this repo.
    let head_only = super::commits(&dir, "-20", true, None, &[]).await;
    assert!(
        !head_only
            .iter()
            .any(|c| c.message == "only on the side branch"),
        "the side branch is not reachable from HEAD, so this is not a test"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn work_key_stable_then_changes_on_commit_and_stash() {
    let dir = temp_repo("workkey");

    let k1 = work_key(&dir).await.expect("key");
    let k2 = work_key(&dir).await.expect("key");
    assert_eq!(k1, k2, "key must be stable when nothing changed");

    std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
    sh(&dir, &["git", "add", "."]);
    sh(&dir, &["git", "commit", "-q", "-m", "second"]);
    let k3 = work_key(&dir).await.expect("key");
    assert_ne!(k1.head_sha, k3.head_sha, "commit must change the key");

    std::fs::write(dir.join("a.txt"), "stash me\n").unwrap();
    sh(&dir, &["git", "stash", "-q"]);
    let k4 = work_key(&dir).await.expect("key");
    assert_ne!(k3.stash_size, k4.stash_size, "stash must change the key");

    std::fs::remove_dir_all(&dir).ok();
}

/// Commit at a fixed old date so the assertion is exact rather than
/// "somewhere near now".
fn sh_at(dir: &Path, iso: &str, args: &[&str]) {
    let status = StdCommand::new(args[0])
        .args(&args[1..])
        .current_dir(dir)
        .env("GIT_AUTHOR_DATE", iso)
        .env("GIT_COMMITTER_DATE", iso)
        .status()
        .expect("spawn");
    assert!(status.success(), "command failed: {args:?}");
}

/// FZ-45: the digest counts commits since the cutoff on the local and
/// the remote-tracking branches, and none from before it.
#[tokio::test]
async fn commits_since_reads_branches_and_remotes_after_the_cutoff() {
    let dir = empty_repo("since");
    std::fs::write(dir.join("a.txt"), "old\n").unwrap();
    sh(&dir, &["git", "add", "."]);
    sh_at(
        &dir,
        "2026-10-20T10:00:00Z",
        &["git", "commit", "-qm", "before"],
    );
    std::fs::write(dir.join("a.txt"), "new\n").unwrap();
    sh_at(
        &dir,
        "2026-10-27T10:00:00Z",
        &["git", "commit", "-qam", "on main"],
    );
    // A remote-tracking ref ahead of every local branch: what a fetch of
    // someone else's push leaves behind.
    sh(&dir, &["git", "checkout", "-qb", "side"]);
    std::fs::write(dir.join("b.txt"), "b\n").unwrap();
    sh(&dir, &["git", "add", "."]);
    sh_at(
        &dir,
        "2026-10-27T11:00:00Z",
        &["git", "commit", "-qm", "pushed elsewhere", "-m", "a body"],
    );
    sh(
        &dir,
        &["git", "update-ref", "refs/remotes/origin/side", "HEAD"],
    );
    sh(&dir, &["git", "checkout", "-q", "main"]);
    sh(&dir, &["git", "branch", "-qD", "side"]);

    let cutoff = DateTime::parse_from_rfc3339("2026-10-27T09:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let read = commits_since(&dir, cutoff).await;
    let got: Vec<&str> = read.iter().map(|c| c.message.as_str()).collect();
    assert_eq!(got, vec!["pushed elsewhere", "on main"]);
    assert!(read.iter().all(|c| c.body.is_empty()), "no bodies are held");
    std::fs::remove_dir_all(&dir).ok();
}

/// A bare `git init` with nothing in it, which is where the three repos
/// that prompted this all started.
fn empty_repo(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fleetz_lla_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    sh(&dir, &["git", "init", "-q", "-b", "main"]);
    sh(&dir, &["git", "config", "user.email", "t@example.com"]);
    sh(&dir, &["git", "config", "user.name", "t"]);
    dir
}

#[tokio::test]
async fn last_local_activity_reports_the_last_commit() {
    let dir = empty_repo("commit");
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    sh(&dir, &["git", "add", "."]);
    sh_at(
        &dir,
        "2026-03-21T19:12:00+00:00",
        &["git", "commit", "-q", "-m", "init"],
    );

    let at = last_local_activity(&dir).await.expect("repo has a commit");
    assert_eq!(at.to_rfc3339(), "2026-03-21T19:12:00+00:00");

    std::fs::remove_dir_all(&dir).ok();
}

/// `~/Repos/Rust`, `bonsai` and `AoC-2025` are all `git init` with
/// untracked files and zero commits, so `git log` reports nothing at all.
/// Falling back to the dirty paths is the only way they get a timestamp,
/// and without one the time window can never evict them.
#[tokio::test]
async fn a_repo_with_no_commits_falls_back_to_its_dirty_files() {
    let dir = empty_repo("nocommits");
    std::fs::write(dir.join("junk.txt"), "scratch\n").unwrap();

    let at = last_local_activity(&dir)
        .await
        .expect("untracked files are activity");
    let age = (chrono::Utc::now() - at).num_seconds();
    assert!(age < 60, "file written just now, got {at} ({age}s ago)");

    std::fs::remove_dir_all(&dir).ok();
}

// --- #43: the local histogram walks branches, not stash or notes ---

/// Grid start for the histogram tests: every fixed date below falls
/// inside the grid that starts here.
const DAILY_START: &str = "2023-10-01";

/// Total of `daily_commits` over the grid, counting the test identity.
async fn daily_total(dir: &Path) -> u32 {
    let start = chrono::NaiveDate::parse_from_str(DAILY_START, "%Y-%m-%d").unwrap();
    let counts = daily_commits(dir, start, "t@example.com")
        .await
        .expect("daily_commits answered");
    counts.0.iter().map(|n| u32::from(*n)).sum()
}

/// One tracked file committed on `main` at a fixed date, so a stash has
/// something to save and every count is exact.
fn daily_repo(tag: &str) -> PathBuf {
    let dir = empty_repo(tag);
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    sh(&dir, &["git", "add", "."]);
    sh_at(
        &dir,
        "2024-01-10T12:00:00+00:00",
        &["git", "commit", "-q", "-m", "init"],
    );
    dir
}

/// E5a: a stash is a WIP commit and an index commit authored by you.
/// Walking `--all` counted both, so stashing raised the strip by two.
#[tokio::test]
async fn a_stash_does_not_raise_the_local_count() {
    let dir = daily_repo("daily_stash");
    let before = daily_total(&dir).await;
    std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
    sh_at(
        &dir,
        "2024-01-11T12:00:00+00:00",
        &["git", "stash", "push", "-q"],
    );
    let after = daily_total(&dir).await;
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(before, 1);
    assert_eq!(after, before, "the stash's commits were counted");
}

/// A note is metadata about a commit, stored as a commit on
/// `refs/notes/*`. Neither the default notes ref nor a nested one counts.
#[tokio::test]
async fn a_note_does_not_raise_the_local_count() {
    let dir = daily_repo("daily_notes");
    let before = daily_total(&dir).await;
    sh_at(
        &dir,
        "2024-01-11T12:00:00+00:00",
        &["git", "notes", "add", "-m", "a note", "HEAD"],
    );
    sh_at(
        &dir,
        "2024-01-11T12:00:00+00:00",
        &[
            "git",
            "notes",
            "--ref=team/review",
            "add",
            "-m",
            "nested",
            "HEAD",
        ],
    );
    let after = daily_total(&dir).await;
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(before, 1);
    assert_eq!(after, before, "the notes commits were counted");
}

/// The reason the walk is wider than HEAD: work on a branch that is not
/// checked out is still work.
#[tokio::test]
async fn a_commit_on_a_branch_not_checked_out_still_counts() {
    let dir = daily_repo("daily_feature");
    sh(&dir, &["git", "checkout", "-q", "-b", "feature"]);
    sh_at(
        &dir,
        "2024-01-11T12:00:00+00:00",
        &["git", "commit", "-q", "--allow-empty", "-m", "feature work"],
    );
    sh(&dir, &["git", "checkout", "-q", "main"]);
    let total = daily_total(&dir).await;
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(total, 2, "the feature branch's commit was not counted");
}

/// One commit reachable from two branches, a remote-tracking ref and the
/// remote's `HEAD` symref is still one commit.
#[tokio::test]
async fn a_commit_reachable_from_several_refs_counts_once() {
    let dir = daily_repo("daily_dedupe");
    sh(&dir, &["git", "branch", "other"]);
    sh(
        &dir,
        &["git", "update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    sh(
        &dir,
        &[
            "git",
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    let total = daily_total(&dir).await;
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(total, 1, "a commit was counted once per ref");
}

/// Kept from `--all`: a commit reachable only from a tag still counts.
#[tokio::test]
async fn a_commit_reachable_only_from_a_tag_still_counts() {
    let dir = daily_repo("daily_tag");
    sh(&dir, &["git", "checkout", "-q", "--detach"]);
    sh_at(
        &dir,
        "2024-01-11T12:00:00+00:00",
        &["git", "commit", "-q", "--allow-empty", "-m", "tagged"],
    );
    sh(&dir, &["git", "tag", "only-tag"]);
    sh(&dir, &["git", "checkout", "-q", "main"]);
    let total = daily_total(&dir).await;
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(total, 2, "the tag-only commit was not counted");
}

/// Kept from `--all`: work in progress on a detached HEAD, on no branch,
/// still counts.
#[tokio::test]
async fn a_commit_on_a_detached_head_still_counts() {
    let dir = daily_repo("daily_detached");
    sh(&dir, &["git", "checkout", "-q", "--detach"]);
    sh_at(
        &dir,
        "2024-01-11T12:00:00+00:00",
        &["git", "commit", "-q", "--allow-empty", "-m", "detached"],
    );
    let total = daily_total(&dir).await;
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(total, 2, "the detached HEAD's commit was not counted");
}

/// Kept from `--all`: a linked worktree's detached HEAD is walked from the
/// main checkout too, so its commits on no branch still count.
#[tokio::test]
async fn a_commit_on_a_linked_worktrees_detached_head_still_counts() {
    let dir = daily_repo("daily_wt");
    let linked = dir.with_file_name(format!(
        "{}_linked",
        dir.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_dir_all(&linked);
    sh(
        &dir,
        &[
            "git",
            "worktree",
            "add",
            "-q",
            "--detach",
            &linked.to_string_lossy(),
            "main",
        ],
    );
    sh_at(
        &linked,
        "2024-01-11T12:00:00+00:00",
        &["git", "commit", "-q", "--allow-empty", "-m", "linked wip"],
    );
    let total = daily_total(&dir).await;
    std::fs::remove_dir_all(&linked).ok();
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(total, 2, "the linked worktree's HEAD was not walked");
}

/// Kept from `--all`: only stash and notes are excluded, so a commit held
/// by any other namespace (here `refs/original`, as `filter-branch` leaves
/// behind) still counts.
#[tokio::test]
async fn a_commit_held_by_another_namespace_still_counts() {
    let dir = daily_repo("daily_other_ns");
    sh(&dir, &["git", "checkout", "-q", "--detach"]);
    sh_at(
        &dir,
        "2024-01-11T12:00:00+00:00",
        &[
            "git",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "rewritten away",
        ],
    );
    sh(
        &dir,
        &["git", "update-ref", "refs/original/refs/heads/main", "HEAD"],
    );
    sh(&dir, &["git", "checkout", "-q", "main"]);
    let total = daily_total(&dir).await;
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(total, 2, "the refs/original commit was not counted");
}

#[tokio::test]
async fn a_clean_repo_with_no_commits_has_no_local_activity() {
    let dir = empty_repo("clean");
    assert_eq!(last_local_activity(&dir).await, None);
    std::fs::remove_dir_all(&dir).ok();
}

/// The AoC-2025 shape: committed (or staged) long ago, touched since.
/// Taking the commit alone would date the repo to March and evict work
/// that is actually current.
#[tokio::test]
async fn a_dirty_file_newer_than_the_last_commit_wins() {
    let dir = empty_repo("newer");
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    sh(&dir, &["git", "add", "."]);
    sh_at(
        &dir,
        "2026-03-21T19:12:00+00:00",
        &["git", "commit", "-q", "-m", "init"],
    );
    std::fs::write(dir.join("b.txt"), "touched today\n").unwrap();

    let at = last_local_activity(&dir).await.expect("dirty file");
    let age = (chrono::Utc::now() - at).num_seconds();
    assert!(age < 60, "newest change is the untracked file, got {at}");

    std::fs::remove_dir_all(&dir).ok();
}

/// A clone of a local upstream, fetched once so `FETCH_HEAD` holds
/// something. No network: the remote is a path on this disk. Returns the
/// upstream and the clone.
fn fetched_clone(tag: &str) -> (PathBuf, PathBuf) {
    let upstream = temp_repo(&format!("{tag}_up"));
    let dir = std::env::temp_dir().join(format!("fleetz_it_{tag}_down_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (from, to) = (
        upstream.to_string_lossy().into_owned(),
        dir.to_string_lossy().into_owned(),
    );
    sh(&std::env::temp_dir(), &["git", "clone", "-q", &from, &to]);
    sh(&dir, &["git", "fetch", "-q"]);
    (upstream, dir)
}

/// Point `origin` somewhere that is not a repository.
fn break_origin(dir: &Path) {
    let missing = dir.join("no-such-remote").to_string_lossy().into_owned();
    sh(dir, &["git", "remote", "set-url", "origin", &missing]);
}

/// Pins the git behaviour rule 7 of `docs/fetch-outcome-contract.md`
/// rests on: a failed fetch truncates `FETCH_HEAD` to nothing and moves
/// its mtime, so the mtime alone counts a failure as a fetch.
#[tokio::test]
async fn a_failed_fetch_empties_fetch_head() {
    let (upstream, dir) = fetched_clone("emptyhead");
    let fetch_head = repo_dirs(&dir).unwrap().git_dir.join("FETCH_HEAD");
    assert!(std::fs::metadata(&fetch_head).unwrap().len() > 0);
    break_origin(&dir);
    fetch(&dir).await;
    assert_eq!(
        std::fs::metadata(&fetch_head).unwrap().len(),
        0,
        "git no longer truncates FETCH_HEAD on a failed fetch"
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&upstream).ok();
}

/// Rule 1, the success half: a fetch that worked says so, and the disk
/// baseline then has a time to give.
#[tokio::test]
async fn a_fetch_from_a_reachable_remote_succeeds() {
    let (upstream, dir) = fetched_clone("fetchok");
    sh(
        &upstream,
        &["git", "commit", "-q", "--allow-empty", "-m", "two"],
    );
    assert_eq!(fetch(&dir).await, FetchOutcome::Succeeded);
    assert!(last_successful_fetch_at(&dir).is_some());
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&upstream).ok();
}

/// Rule 1, the failure half, and rule 7 on a real clone: a remote that is
/// not a repository fails with `NoRemote`, git's own line as the detail,
/// and leaves no successful-fetch time on disk.
#[tokio::test]
async fn a_fetch_from_a_missing_remote_fails_as_no_remote() {
    let (upstream, dir) = fetched_clone("fetchgone");
    assert!(last_successful_fetch_at(&dir).is_some());
    break_origin(&dir);
    let FetchOutcome::Failed(failure) = fetch(&dir).await else {
        panic!("a fetch from a missing path did not fail");
    };
    assert_eq!(failure.kind, FetchFailureKind::NoRemote);
    let detail = failure.detail.expect("git said why");
    assert!(
        detail.contains("does not appear to be a git repository"),
        "{detail:?}"
    );
    assert_eq!(
        last_successful_fetch_at(&dir),
        None,
        "a failed fetch was read as a fetch"
    );
    // The raw mtime still moves, which is why it cannot be the baseline.
    assert!(last_fetch_at(&dir).is_some());
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&upstream).ok();
}

/// A remote that never answers is given up on as `TimedOut`, not lumped
/// in with a failure to start. The upload-pack here waits on its input
/// forever, so the limit is what ends it, deterministically.
#[tokio::test]
async fn a_fetch_that_never_answers_times_out() {
    let (upstream, dir) = fetched_clone("fetchhang");
    sh(
        &dir,
        &[
            "git",
            "config",
            "remote.origin.uploadpack",
            "cat >/dev/null; :",
        ],
    );
    let outcome = fetch_within(&dir, std::time::Duration::from_millis(300)).await;
    assert_eq!(
        outcome,
        FetchOutcome::Failed(FetchFailure {
            kind: FetchFailureKind::TimedOut,
            detail: None,
        })
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&upstream).ok();
}

/// Whether a pid still names a live process, waiting up to `bound` for
/// it to go: a killed orphan is reaped by init, not by us, so it can
/// linger as a zombie for a moment.
fn alive_after(pid: &str, bound: std::time::Duration) -> bool {
    let gone = || {
        let out = StdCommand::new("ps")
            .args(["-o", "stat=", "-p", pid])
            .output()
            .expect("ps runs");
        let stat = String::from_utf8_lossy(&out.stdout);
        !out.status.success() || stat.trim().is_empty() || stat.trim().starts_with('Z')
    };
    let start = std::time::Instant::now();
    while start.elapsed() < bound {
        if gone() {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    !gone()
}

/// Every `tmp_*` name in a clone's pack directory, read independently of
/// `temp_pack_names` so a prefix it misses still shows up here.
fn temp_packs(dir: &Path) -> Vec<String> {
    let pack_dir = repo_dirs(dir).unwrap().common_dir.join("objects/pack");
    let mut names: Vec<String> = std::fs::read_dir(pack_dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("tmp_"))
        .collect();
    names.sort();
    names
}

/// FZ-34 (#83), criteria 1 and 2: a timed-out fetch leaves nothing
/// running and none of its own temp packs. The upload-pack hook stands in
/// for a slow remote: it starts a grandchild of `git fetch` (the
/// `ssh`/`index-pack` that `kill_on_drop` used to orphan, writing its pid
/// to a file) and creates the temp files a half-done download leaves,
/// then never answers. A temp pack that was there before the fetch is
/// someone else's and stays.
#[tokio::test]
async fn a_timed_out_fetch_kills_its_process_group_and_its_temp_packs() {
    let (upstream, dir) = fetched_clone("fetchleak");
    let pack_dir = repo_dirs(&dir).unwrap().common_dir.join("objects/pack");
    std::fs::write(pack_dir.join("tmp_pack_before"), b"theirs").unwrap();
    let pid_file = dir.join("hook.pid");
    let hook = format!(
        "sleep 600 </dev/null >/dev/null 2>&1 & echo $! > '{pid}'; \
         : > '{pack}/tmp_pack_hook'; : > '{pack}/tmp_idx_hook'; : > '{pack}/tmp_rev_hook'; \
         cat >/dev/null; :",
        pid = pid_file.display(),
        pack = pack_dir.display(),
    );
    sh(
        &dir,
        &["git", "config", "remote.origin.uploadpack", hook.as_str()],
    );
    let outcome = fetch_within(&dir, std::time::Duration::from_millis(2000)).await;
    assert!(
        matches!(&outcome, FetchOutcome::Failed(f) if f.kind == FetchFailureKind::TimedOut),
        "{outcome:?}"
    );
    let pid = std::fs::read_to_string(&pid_file).expect("the hook ran within the limit");
    let pid = pid.trim();
    let survived = alive_after(pid, std::time::Duration::from_secs(3));
    if survived {
        let _ = StdCommand::new("kill").args(["-9", pid]).status();
    }
    assert!(
        !survived,
        "the fetch's grandchild {pid} outlived the timeout"
    );
    assert_eq!(
        temp_packs(&dir),
        ["tmp_pack_before"],
        "the fetch's own temp files must go, the earlier one must stay"
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&upstream).ok();
}

/// FZ-34 (#83): a fetch dropped mid-run (its task cancelled, fleetz
/// quitting) kills its process group and removes its temp packs too,
/// without waiting.
#[tokio::test]
async fn a_cancelled_fetch_kills_its_process_group_and_its_temp_packs() {
    let (upstream, dir) = fetched_clone("fetchcancel");
    let pack_dir = repo_dirs(&dir).unwrap().common_dir.join("objects/pack");
    std::fs::write(pack_dir.join("tmp_pack_before"), b"theirs").unwrap();
    let pid_file = dir.join("hook.pid");
    let hook = format!(
        ": > '{pack}/tmp_pack_hook'; : > '{pack}/tmp_rev_hook'; \
         sleep 600 </dev/null >/dev/null 2>&1 & echo $! > '{pid}'; cat >/dev/null; :",
        pid = pid_file.display(),
        pack = pack_dir.display(),
    );
    sh(
        &dir,
        &["git", "config", "remote.origin.uploadpack", hook.as_str()],
    );
    let fetch_dir = dir.clone();
    let task =
        tokio::spawn(
            async move { fetch_within(&fetch_dir, std::time::Duration::from_secs(60)).await },
        );
    let start = std::time::Instant::now();
    while std::fs::read_to_string(&pid_file).map_or(true, |p| p.trim().is_empty()) {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "the hook never ran"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let pid = std::fs::read_to_string(&pid_file).unwrap();
    let pid = pid.trim();
    let survived = alive_after(pid, std::time::Duration::from_secs(3));
    if survived {
        let _ = StdCommand::new("kill").args(["-9", pid]).status();
    }
    assert!(!survived, "the cancelled fetch's grandchild {pid} survived");
    assert_eq!(temp_packs(&dir), ["tmp_pack_before"]);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&upstream).ok();
}

/// FZ-34 (#83), criterion 5d: a fetch that ends by itself removes
/// nothing. git cleans up after its own exit, and a temp file there is
/// another process's.
#[tokio::test]
async fn a_fetch_that_finishes_leaves_other_temp_packs_alone() {
    let (upstream, dir) = fetched_clone("fetchkeep");
    let pack_dir = repo_dirs(&dir).unwrap().common_dir.join("objects/pack");
    std::fs::write(pack_dir.join("tmp_pack_other"), b"theirs").unwrap();
    sh(
        &upstream,
        &["git", "commit", "-q", "--allow-empty", "-m", "two"],
    );
    assert_eq!(fetch(&dir).await, FetchOutcome::Succeeded);
    assert_eq!(temp_packs(&dir), ["tmp_pack_other"]);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&upstream).ok();
}

/// An upload-pack hook that writes `lines` progress-shaped lines to
/// stderr, one every 300 ms, the way git's `Receiving objects` updates
/// arrive (`\r`-separated, growing bytes), then runs `tail`. git prints
/// its own progress only when asked, so the hook's lines stand in for it.
fn progress_hook(lines: u32, tail: &str) -> String {
    format!(
        "i=1; while [ $i -le {lines} ]; do \
         printf 'Receiving objects: %d%% (%d/{lines}), %d.00 MiB | 1.00 MiB/s\\r' \
         $((i * 100 / {lines})) $i $i >&2; sleep 0.3; i=$((i + 1)); done; {tail}"
    )
}

/// FZ-35a (#84), criterion 2: a fetch that keeps reporting progress is
/// never cut, however long it runs. Here it runs about 2.7 s under a
/// 1 s stall limit, the small-scale version of a large pack outliving
/// the old 120 s wall clock, and finishes.
#[tokio::test]
async fn a_fetch_that_keeps_reporting_progress_outlives_the_stall_limit() {
    let (upstream, dir) = fetched_clone("fetchslow");
    sh(
        &upstream,
        &["git", "commit", "-q", "--allow-empty", "-m", "two"],
    );
    let hook = progress_hook(9, "git upload-pack");
    sh(
        &dir,
        &["git", "config", "remote.origin.uploadpack", hook.as_str()],
    );
    let start = std::time::Instant::now();
    let mut seen = Vec::new();
    let outcome = fetch_watched(&dir, std::time::Duration::from_secs(1), |p| seen.push(p)).await;
    assert_eq!(
        outcome,
        FetchOutcome::Succeeded,
        "after {:?}",
        start.elapsed()
    );
    assert!(
        start.elapsed() > std::time::Duration::from_secs(2),
        "the hook did not run past the limit: {:?}",
        start.elapsed()
    );
    let last = seen
        .iter()
        .rev()
        .find(|p| p.phase == FetchPhase::Receiving)
        .expect("the hook's progress reached the callback");
    assert_eq!(last.bytes, 9 * 1024 * 1024);
    assert_eq!(seen.first().map(|p| p.bytes), Some(1024 * 1024));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&upstream).ok();
}

/// FZ-35a (#84), criterion 2: a fetch whose progress stops is cut one
/// stall limit after its last update, not one limit after it started,
/// and is cleaned up exactly as FZ-34 cleans a timed-out fetch: its
/// process group dead, its own temp packs gone, an earlier one kept.
#[tokio::test]
async fn a_fetch_whose_progress_stops_is_cut_at_the_stall_limit() {
    let (upstream, dir) = fetched_clone("fetchstall");
    let pack_dir = repo_dirs(&dir).unwrap().common_dir.join("objects/pack");
    std::fs::write(pack_dir.join("tmp_pack_before"), b"theirs").unwrap();
    let pid_file = dir.join("hook.pid");
    let tail = format!(
        "sleep 600 </dev/null >/dev/null 2>&1 & echo $! > '{pid}'; \
         : > '{pack}/tmp_pack_hook'; : > '{pack}/tmp_idx_hook'; : > '{pack}/tmp_rev_hook'; \
         cat >/dev/null; :",
        pid = pid_file.display(),
        pack = pack_dir.display(),
    );
    // Five updates, the last about 1.5 s in, then silence.
    let hook = progress_hook(5, &tail);
    sh(
        &dir,
        &["git", "config", "remote.origin.uploadpack", hook.as_str()],
    );
    let start = std::time::Instant::now();
    let outcome = fetch_watched(&dir, std::time::Duration::from_secs(1), |_| {}).await;
    let took = start.elapsed();
    assert_eq!(
        outcome,
        FetchOutcome::Failed(FetchFailure {
            kind: FetchFailureKind::TimedOut,
            detail: None,
        })
    );
    assert!(
        took > std::time::Duration::from_millis(2200),
        "cut before the last update was a limit old: {took:?}"
    );
    assert!(
        took < std::time::Duration::from_secs(20),
        "never cut: {took:?}"
    );
    let pid = std::fs::read_to_string(&pid_file).expect("the hook ran");
    let pid = pid.trim();
    let survived = alive_after(pid, std::time::Duration::from_secs(3));
    if survived {
        let _ = StdCommand::new("kill").args(["-9", pid]).status();
    }
    assert!(!survived, "the stalled fetch's grandchild {pid} survived");
    assert_eq!(temp_packs(&dir), ["tmp_pack_before"]);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&upstream).ok();
}

/// FZ-35b (#85), criterion 3: a fetch whose progress passes the cap is
/// cut at the first update over it, not at its end, and cleaned as a
/// stalled one is: its process group dead, its own temp packs gone, an
/// earlier one kept. The outcome is `TooBig` with the bytes seen.
#[tokio::test]
async fn a_fetch_past_the_cap_is_killed_cleaned_and_reported_too_big() {
    let (upstream, dir) = fetched_clone("fetchcap");
    let pack_dir = repo_dirs(&dir).unwrap().common_dir.join("objects/pack");
    std::fs::write(pack_dir.join("tmp_pack_before"), b"theirs").unwrap();
    let pid_file = dir.join("hook.pid");
    // A grandchild and the temp packs first, then nine updates of 1 MiB
    // more each, 300 ms apart, then a real upload-pack.
    let hook = format!(
        "sleep 600 </dev/null >/dev/null 2>&1 & echo $! > '{pid}'; \
         : > '{pack}/tmp_pack_hook'; : > '{pack}/tmp_idx_hook'; {progress}",
        pid = pid_file.display(),
        pack = pack_dir.display(),
        progress = progress_hook(9, "git upload-pack"),
    );
    sh(
        &dir,
        &["git", "config", "remote.origin.uploadpack", hook.as_str()],
    );
    let cap = 3 * 1024 * 1024;
    let start = std::time::Instant::now();
    let mut seen = Vec::new();
    let outcome = fetch_capped(&dir, std::time::Duration::from_secs(10), Some(cap), |p| {
        seen.push(p.bytes)
    })
    .await;
    let took = start.elapsed();
    assert_eq!(
        outcome,
        FetchOutcome::Failed(FetchFailure::too_big(4 * 1024 * 1024))
    );
    let FetchOutcome::Failed(failure) = &outcome else {
        unreachable!()
    };
    assert_eq!(failure.received_bytes(), Some(4 * 1024 * 1024));
    assert_eq!(
        seen.last(),
        Some(&(4 * 1024 * 1024)),
        "cut at the first update over"
    );
    assert!(
        took < std::time::Duration::from_millis(2500),
        "ran on past the cap: {took:?}"
    );
    let pid = std::fs::read_to_string(&pid_file).expect("the hook ran");
    let pid = pid.trim();
    let survived = alive_after(pid, std::time::Duration::from_secs(3));
    if survived {
        let _ = StdCommand::new("kill").args(["-9", pid]).status();
    }
    assert!(!survived, "the capped fetch's grandchild {pid} survived");
    assert_eq!(temp_packs(&dir), ["tmp_pack_before"]);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&upstream).ok();
}

/// FZ-35b (#85), criterion 4: with no cap (`f`) the same fetch runs to
/// its end.
#[tokio::test]
async fn a_fetch_with_no_cap_runs_past_any_size() {
    let (upstream, dir) = fetched_clone("fetchnocap");
    sh(
        &upstream,
        &["git", "commit", "-q", "--allow-empty", "-m", "two"],
    );
    let hook = progress_hook(4, "git upload-pack");
    sh(
        &dir,
        &["git", "config", "remote.origin.uploadpack", hook.as_str()],
    );
    let outcome = fetch_reporting(&dir, None, |_| {}).await;
    assert_eq!(outcome, FetchOutcome::Succeeded);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&upstream).ok();
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

// --- #70: ssh gets a connect timeout and batch mode, unless the user
// chose their own ssh ---

#[test]
fn a_fetch_with_no_ssh_override_adds_the_ssh_options() {
    assert_eq!(
        fetch_ssh_command(None, None, None),
        Some("ssh -o ConnectTimeout=10 -o BatchMode=yes")
    );
}

#[test]
fn a_user_git_ssh_command_adds_nothing() {
    assert_eq!(fetch_ssh_command(Some("ssh -i key"), None, None), None);
}

#[test]
fn a_user_git_ssh_adds_nothing() {
    assert_eq!(fetch_ssh_command(None, Some("/usr/bin/myssh"), None), None);
}

#[test]
fn a_user_core_ssh_command_adds_nothing() {
    assert_eq!(fetch_ssh_command(None, None, Some("ssh -i key")), None);
}

/// git treats each of the three as set when it holds an empty string: it
/// tries to run the empty command rather than fall back to plain `ssh`.
/// So fleetz does too, and leaves the user's (broken) choice alone.
#[test]
fn an_empty_ssh_override_counts_as_set() {
    assert_eq!(fetch_ssh_command(Some(""), None, None), None);
    assert_eq!(fetch_ssh_command(None, Some(""), None), None);
    assert_eq!(fetch_ssh_command(None, None, Some("")), None);
}

/// The `-c core.sshCommand=...` a built fetch command carries, if any.
fn ssh_config_args(cmd: &Command) -> Vec<String> {
    cmd.as_std()
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .filter(|a| a.starts_with("core.sshCommand="))
        .collect()
}

#[tokio::test]
async fn a_fetch_in_a_repo_without_core_ssh_command_carries_the_ssh_options() {
    let dir = temp_repo("sshplain");
    let configured = configured_ssh_command(&dir).await;
    assert_eq!(configured, None, "no core.sshCommand in any scope");
    let cmd = fetch_cmd(&dir, fetch_ssh_command(None, None, configured.as_deref()));
    assert_eq!(
        ssh_config_args(&cmd),
        ["core.sshCommand=ssh -o ConnectTimeout=10 -o BatchMode=yes"]
    );
    let args: Vec<_> = cmd.as_std().get_args().collect();
    let fetch_at = args.iter().position(|a| *a == "fetch").expect("a fetch");
    let config_at = args
        .iter()
        .position(|a| a.to_string_lossy().starts_with("core.sshCommand="))
        .expect("the ssh option");
    assert!(config_at < fetch_at, "-c must precede the subcommand");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn a_fetch_in_a_repo_with_core_ssh_command_adds_nothing() {
    let dir = temp_repo("sshown");
    sh(&dir, &["git", "config", "core.sshCommand", "ssh -i mykey"]);
    let configured = configured_ssh_command(&dir).await;
    assert_eq!(configured.as_deref(), Some("ssh -i mykey"));
    let cmd = fetch_cmd(&dir, fetch_ssh_command(None, None, configured.as_deref()));
    assert!(ssh_config_args(&cmd).is_empty(), "{cmd:?}");
    std::fs::remove_dir_all(&dir).ok();
}

// --- FZ-06b: the key follows ref content (scenario ids from
// docs/cache-invalidation-design.md, 3.1) ---

/// A commit touching a file, so the history really moves.
fn commit(dir: &Path, msg: &str) {
    std::fs::write(dir.join("a.txt"), format!("{msg}\n")).unwrap();
    sh(dir, &["git", "add", "."]);
    sh(dir, &["git", "commit", "-q", "-m", msg]);
}

/// A bare repository under a unique name, for pushes and fetches.
fn bare_remote(tag: &str) -> PathBuf {
    let bare =
        std::env::temp_dir().join(format!("fleetz_it_{tag}_bare_{}.git", std::process::id()));
    let _ = std::fs::remove_dir_all(&bare);
    std::fs::create_dir_all(&bare).unwrap();
    sh(&bare, &["git", "init", "-q", "--bare", "-b", "main"]);
    bare
}

/// `temp_repo` with `origin` set to a fresh bare remote and `main`
/// pushed with an upstream. Returns the clone and the remote.
fn repo_with_remote(tag: &str) -> (PathBuf, PathBuf) {
    let dir = temp_repo(tag);
    let bare = bare_remote(tag);
    sh(
        &dir,
        &["git", "remote", "add", "origin", &bare.to_string_lossy()],
    );
    sh(&dir, &["git", "push", "-q", "-u", "origin", "main"]);
    (dir, bare)
}

async fn key_of(dir: &Path) -> WorkKey {
    work_key(dir).await.expect("a key for a real repo")
}

/// E2b, the everyday push: it moves `refs/remotes/origin/main` and
/// nothing else, and the graph's `origin/main` decoration stayed one
/// commit behind until HEAD moved again.
#[tokio::test]
async fn a_push_changes_the_work_key() {
    let (dir, bare) = repo_with_remote("fz06b_push");
    commit(&dir, "two");
    let before = key_of(&dir).await;
    sh(&dir, &["git", "push", "-q", "origin", "main"]);
    let after = key_of(&dir).await;
    assert_ne!(before, after, "a push moved origin/main, the key did not");
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&bare).ok();
}

/// E2c: a fetch bringing another clone's push.
#[tokio::test]
async fn fetching_another_clones_push_changes_the_work_key() {
    let (dir, bare) = repo_with_remote("fz06b_fetch");
    let other = std::env::temp_dir().join(format!(
        "fleetz_it_fz06b_fetch_other_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&other);
    sh(
        &std::env::temp_dir(),
        &[
            "git",
            "clone",
            "-q",
            &bare.to_string_lossy(),
            &other.to_string_lossy(),
        ],
    );
    sh(&other, &["git", "config", "user.email", "o@example.com"]);
    sh(&other, &["git", "config", "user.name", "o"]);
    commit(&other, "from the other clone");
    sh(&other, &["git", "push", "-q", "origin", "main"]);

    let before = key_of(&dir).await;
    sh(&dir, &["git", "fetch", "-q", "--prune"]);
    let after = key_of(&dir).await;
    assert_ne!(before, after, "a fetch moved origin/main, the key did not");
    for d in [&dir, &bare, &other] {
        std::fs::remove_dir_all(d).ok();
    }
}

/// E1a: another branch moved while HEAD stays put.
#[tokio::test]
async fn moving_another_branch_changes_the_work_key() {
    let dir = temp_repo("fz06b_branchf");
    sh(&dir, &["git", "branch", "side"]);
    commit(&dir, "two");
    let before = key_of(&dir).await;
    sh(&dir, &["git", "branch", "-f", "side", "main"]);
    let after = key_of(&dir).await;
    assert_ne!(before, after, "side moved, the key did not");
    std::fs::remove_dir_all(&dir).ok();
}

/// E1b: a branch created at an older commit.
#[tokio::test]
async fn creating_a_branch_changes_the_work_key() {
    let dir = temp_repo("fz06b_branchnew");
    commit(&dir, "two");
    let before = key_of(&dir).await;
    sh(&dir, &["git", "branch", "feat", "HEAD~1"]);
    let after = key_of(&dir).await;
    assert_ne!(before, after, "a new branch left the key unchanged");
    std::fs::remove_dir_all(&dir).ok();
}

/// E8b: a packed branch moved, which writes a loose ref beside it.
#[tokio::test]
async fn moving_a_packed_branch_changes_the_work_key() {
    let dir = temp_repo("fz06b_packedmove");
    sh(&dir, &["git", "branch", "side"]);
    commit(&dir, "two");
    sh(&dir, &["git", "pack-refs", "--all"]);
    let before = key_of(&dir).await;
    sh(&dir, &["git", "branch", "-f", "side", "main"]);
    let after = key_of(&dir).await;
    assert_ne!(before, after, "a packed branch moved, the key did not");
    std::fs::remove_dir_all(&dir).ok();
}

/// E8a: packing moves no ref, so it must not cost a re-probe.
#[tokio::test]
async fn packing_refs_keeps_the_work_key() {
    let dir = temp_repo("fz06b_pack");
    sh(&dir, &["git", "branch", "side"]);
    sh(&dir, &["git", "tag", "v1"]);
    let before = key_of(&dir).await;
    sh(&dir, &["git", "pack-refs", "--all"]);
    let after = key_of(&dir).await;
    assert_eq!(before, after, "pack-refs alone moved the key");
    std::fs::remove_dir_all(&dir).ok();
}

/// E8d: nor does `gc`.
#[tokio::test]
async fn gc_keeps_the_work_key() {
    let dir = temp_repo("fz06b_gc");
    sh(&dir, &["git", "branch", "side"]);
    sh(&dir, &["git", "tag", "v1"]);
    let before = key_of(&dir).await;
    sh(&dir, &["git", "gc", "-q"]);
    let after = key_of(&dir).await;
    assert_eq!(before, after, "gc alone moved the key");
    std::fs::remove_dir_all(&dir).ok();
}

/// E3d: a nested tag added beside an existing one in its directory.
#[tokio::test]
async fn a_nested_tag_changes_the_work_key() {
    let dir = temp_repo("fz06b_nestedtag");
    sh(&dir, &["git", "tag", "rc/1"]);
    let before = key_of(&dir).await;
    sh(&dir, &["git", "tag", "rc/2"]);
    let after = key_of(&dir).await;
    assert_ne!(before, after, "rc/2 was created, the key did not move");
    std::fs::remove_dir_all(&dir).ok();
}

/// E3e: a nested tag moved to another commit.
#[tokio::test]
async fn moving_a_nested_tag_changes_the_work_key() {
    let dir = temp_repo("fz06b_nestedmove");
    commit(&dir, "two");
    sh(&dir, &["git", "tag", "rc/1"]);
    sh(&dir, &["git", "tag", "rc/2"]);
    let before = key_of(&dir).await;
    sh(&dir, &["git", "tag", "-f", "rc/2", "HEAD~1"]);
    let after = key_of(&dir).await;
    assert_ne!(before, after, "rc/2 moved, the key did not");
    std::fs::remove_dir_all(&dir).ok();
}

/// E9a: a commit on `main` in the main checkout, seen from a linked
/// worktree's row. Its HEAD, index and stash are untouched.
#[tokio::test]
async fn a_commit_in_the_main_checkout_changes_a_linked_worktrees_key() {
    let main_dir = temp_repo("fz06b_e9a");
    let wt = linked_worktree(&main_dir, "fz06b_e9a");
    let before = key_of(&wt).await;
    commit(&main_dir, "two");
    let after = key_of(&wt).await;
    assert_ne!(before, after, "main moved, the worktree's key did not");
    std::fs::remove_dir_all(&wt).ok();
    std::fs::remove_dir_all(&main_dir).ok();
}

/// E9d: a commit in the linked worktree, seen from the main checkout.
#[tokio::test]
async fn a_commit_in_a_linked_worktree_changes_the_main_checkouts_key() {
    let main_dir = temp_repo("fz06b_e9d");
    let wt = linked_worktree(&main_dir, "fz06b_e9d");
    sh(&wt, &["git", "config", "user.email", "t@example.com"]);
    sh(&wt, &["git", "config", "user.name", "t"]);
    let before = key_of(&main_dir).await;
    commit(&wt, "on the worktree branch");
    let after = key_of(&main_dir).await;
    assert_ne!(
        before, after,
        "the worktree's branch moved, main's key did not"
    );
    std::fs::remove_dir_all(&wt).ok();
    std::fs::remove_dir_all(&main_dir).ok();
}

/// E1e: HEAD repointed at another branch on the same commit, with no
/// index write. The reflog and the HEAD decoration both change.
#[tokio::test]
async fn repointing_head_changes_the_work_key() {
    let dir = temp_repo("fz06b_symref");
    sh(&dir, &["git", "branch", "same"]);
    let before = key_of(&dir).await;
    sh(&dir, &["git", "symbolic-ref", "HEAD", "refs/heads/same"]);
    let after = key_of(&dir).await;
    assert_ne!(
        before, after,
        "HEAD names another branch, the key did not move"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// E15a: the HEAD reflog emptied by expiry, which the reflog pane draws.
#[tokio::test]
async fn expiring_the_reflog_changes_the_work_key() {
    let dir = temp_repo("fz06b_expire");
    commit(&dir, "two");
    let before = key_of(&dir).await;
    sh(&dir, &["git", "reflog", "expire", "--expire=now", "--all"]);
    let after = key_of(&dir).await;
    assert_ne!(
        before, after,
        "the reflog was emptied, the key did not move"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// E5f: a stash replaced at the same size in the main checkout, seen
/// from a linked worktree, whose own index does not move.
#[tokio::test]
async fn a_stash_replaced_at_equal_size_changes_a_linked_worktrees_key() {
    let main_dir = temp_repo("fz06b_e5f");
    let wt = linked_worktree(&main_dir, "fz06b_e5f");
    std::fs::write(main_dir.join("a.txt"), "first\n").unwrap();
    sh(&main_dir, &["git", "stash", "push", "-q", "-m", "aaaa"]);
    let stash_log = main_dir.join(".git/logs/refs/stash");
    let size_before = std::fs::metadata(&stash_log).unwrap().len();
    let before = key_of(&wt).await;

    sh(&main_dir, &["git", "stash", "drop", "-q"]);
    std::fs::write(main_dir.join("a.txt"), "other\n").unwrap();
    sh(&main_dir, &["git", "stash", "push", "-q", "-m", "bbbb"]);
    assert_eq!(
        std::fs::metadata(&stash_log).unwrap().len(),
        size_before,
        "the fixture must replace the stash at equal size"
    );
    let after = key_of(&wt).await;
    assert_ne!(
        before, after,
        "the stash was replaced, the key did not move"
    );
    std::fs::remove_dir_all(&wt).ok();
    std::fs::remove_dir_all(&main_dir).ok();
}

/// E16a: a note, which `log --graph` prints and `--all` walks.
#[tokio::test]
async fn adding_a_note_changes_the_work_key() {
    let dir = temp_repo("fz06b_notes");
    let before = key_of(&dir).await;
    sh(&dir, &["git", "notes", "add", "-m", "hello", "HEAD"]);
    let after = key_of(&dir).await;
    assert_ne!(before, after, "a note was added, the key did not move");
    std::fs::remove_dir_all(&dir).ok();
}

/// E16b: `fetch --unshallow` moves no ref, only the shallow boundary,
/// and the history gains rows.
#[tokio::test]
async fn unshallowing_changes_the_work_key() {
    let (upstream, bare) = repo_with_remote("fz06b_shallow");
    commit(&upstream, "two");
    commit(&upstream, "three");
    sh(&upstream, &["git", "push", "-q", "origin", "main"]);
    let clone = std::env::temp_dir().join(format!(
        "fleetz_it_fz06b_shallow_clone_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&clone);
    let url = format!("file://{}", bare.to_string_lossy());
    sh(
        &std::env::temp_dir(),
        &[
            "git",
            "clone",
            "-q",
            "--depth",
            "1",
            &url,
            &clone.to_string_lossy(),
        ],
    );
    assert!(clone.join(".git/shallow").exists(), "a shallow clone");

    let before = key_of(&clone).await;
    sh(&clone, &["git", "fetch", "-q", "--unshallow"]);
    assert!(!clone.join(".git/shallow").exists(), "unshallowed");
    let after = key_of(&clone).await;
    assert_ne!(before, after, "the shallow boundary moved, the key did not");
    for d in [&upstream, &bare, &clone] {
        std::fs::remove_dir_all(d).ok();
    }
}

/// E12a: working-tree edits never touch the key, and are still seen,
/// because `dirty_count` is not gated by it.
#[tokio::test]
async fn a_dirty_tree_keeps_the_key_and_still_reports_dirty() {
    let dir = temp_repo("fz06b_dirty");
    let before = key_of(&dir).await;
    std::fs::write(dir.join("a.txt"), "edited\n").unwrap();
    std::fs::write(dir.join("new.txt"), "untracked\n").unwrap();
    let after = key_of(&dir).await;
    assert_eq!(before, after, "working-tree edits moved the key");
    assert_eq!(dirty_count(&dir).await, 2);
    std::fs::remove_dir_all(&dir).ok();
}

/// A `.git` git cannot read yields no key, which the sweep reads as
/// changed.
#[tokio::test]
async fn an_unreadable_git_dir_has_no_work_key() {
    let dir = temp_repo("fz06b_unreadable");
    let dot = dir.join(".git");
    sh(&dir, &["chmod", "000", &dot.to_string_lossy()]);
    let key = work_key(&dir).await;
    sh(&dir, &["chmod", "755", &dot.to_string_lossy()]);
    assert_eq!(key, None);
    std::fs::remove_dir_all(&dir).ok();
}

/// A ref query that failed composes into no key, so the sweep cannot
/// mistake it for an answer with no refs in it.
#[tokio::test]
async fn a_failed_ref_query_yields_no_work_key() {
    let dir = temp_repo("fz06b_failedquery");
    let local = try_branch_statuses(&dir).await.expect("branch query");
    let other = try_ref_listing(&dir).await.expect("ref query");
    assert!(
        work_key_from(&dir, Some(&local), Some(&other))
            .await
            .is_some()
    );
    assert_eq!(work_key_from(&dir, None, Some(&other)).await, None);
    assert_eq!(work_key_from(&dir, Some(&local), None).await, None);
    std::fs::remove_dir_all(&dir).ok();
}

/// The widened query lists every ref, but the branches view must still
/// get only remote-tracking branches: no tag, stash or note.
#[tokio::test]
async fn remote_branches_list_only_remote_tracking_refs() {
    let (dir, bare) = repo_with_remote("fz06b_onlyremotes");
    sh(&dir, &["git", "tag", "v1"]);
    sh(&dir, &["git", "notes", "add", "-m", "n", "HEAD"]);
    std::fs::write(dir.join("a.txt"), "stash me\n").unwrap();
    sh(&dir, &["git", "stash", "-q"]);
    let names: Vec<String> = remote_branches(&dir)
        .await
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert_eq!(names, vec!["origin/main".to_string()]);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&bare).ok();
}

/// `tip` stays abbreviated: the branches view matches it against the
/// graph's `%h` hashes (`App::inspected_hash`).
#[tokio::test]
async fn branch_and_remote_tips_stay_abbreviated() {
    let (dir, bare) = repo_with_remote("fz06b_shorttip");
    let short = {
        let out = StdCommand::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(&dir)
            .output()
            .expect("spawn");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let local = branch_statuses(&dir).await;
    assert_eq!(local[0].tip, short);
    let remote = remote_branches(&dir).await;
    assert_eq!(remote[0].tip, short);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&bare).ok();
}

fn changed(status: &str, path: &str, churn: Option<(u32, u32)>) -> ChangedFile {
    ChangedFile {
        status: status.to_string(),
        path: path.to_string(),
        from: None,
        churn,
    }
}

fn commit_user(dir: &Path) {
    sh(dir, &["git", "config", "user.email", "t@example.com"]);
    sh(dir, &["git", "config", "user.name", "t"]);
}

/// A clone two commits past its upstream, carrying every kind of
/// uncommitted change: a staged addition, an unstaged modification, an
/// untracked file, an untracked directory with files beneath it, and a
/// staged rename to a name with a space and non-ASCII. Returns the
/// upstream and the clone.
fn changes_fixture(tag: &str) -> (PathBuf, PathBuf) {
    let (upstream, dir) = fetched_clone(tag);
    commit_user(&dir);
    std::fs::write(dir.join("old name.txt"), "x\n").unwrap();
    std::fs::write(dir.join("ü.txt"), "1\n").unwrap();
    sh(&dir, &["git", "add", "."]);
    sh(&dir, &["git", "commit", "-q", "-m", "c1"]);
    std::fs::write(dir.join("ü.txt"), "1\n2\n").unwrap();
    sh(&dir, &["git", "commit", "-qam", "c2"]);
    sh(&dir, &["git", "mv", "old name.txt", "new näme.txt"]);
    std::fs::write(dir.join("staged.txt"), "s\n").unwrap();
    sh(&dir, &["git", "add", "staged.txt"]);
    std::fs::write(dir.join("a.txt"), "two\n").unwrap();
    std::fs::write(dir.join("loose.txt"), "loose body\n").unwrap();
    std::fs::create_dir_all(dir.join("scratch")).unwrap();
    std::fs::write(dir.join("scratch/x.txt"), "x\n").unwrap();
    std::fs::write(dir.join("scratch/y.txt"), "y\n").unwrap();
    (upstream, dir)
}

/// FZ-60: the two sections hold exactly the fixture's rows, with
/// porcelain's two status columns, the churn, the directory as one row,
/// the rename by its destination, and the paths as named.
#[tokio::test]
async fn changes_lists_every_uncommitted_kind_and_the_unpushed_files() {
    let (upstream, dir) = changes_fixture("changes");
    let read = changes(&dir, ChangesBase::Upstream).await.expect("reads");
    let mut uncommitted = read.uncommitted.clone();
    uncommitted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut rename = changed("R ", "new näme.txt", Some((0, 0)));
    rename.from = Some("old name.txt".to_string());
    assert_eq!(
        uncommitted,
        vec![
            changed(" M", "a.txt", Some((1, 1))),
            changed("??", "loose.txt", None),
            rename,
            changed("??", "scratch/", None),
            changed("A ", "staged.txt", Some((1, 0))),
        ]
    );
    assert_eq!(
        read.outgoing,
        Outgoing::Unpushed {
            upstream: "origin/main".to_string(),
            ahead: 2,
            files: vec![
                changed("A", "old name.txt", Some((1, 0))),
                changed("A", "ü.txt", Some((2, 0))),
            ],
        }
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&upstream).ok();
}

/// The inspector's reads: the uncommitted diff against HEAD (a rename
/// with both names, unquoted), an untracked file's body, an untracked
/// directory's one line, the other section's diff since the merge base,
/// and a failing read's error.
#[tokio::test]
async fn changed_file_diff_reads_each_kind_by_its_own_path() {
    let (upstream, dir) = changes_fixture("changesdiff");
    let read = changes(&dir, ChangesBase::Upstream).await.expect("reads");
    let find = |files: &[ChangedFile], path: &str| {
        files.iter().find(|f| f.path == path).cloned().expect(path)
    };
    let modified = changed_file_diff(&dir, None, &find(&read.uncommitted, "a.txt"))
        .await
        .expect("a.txt");
    assert!(modified.contains("-one\n+two"), "{modified}");
    let renamed = changed_file_diff(&dir, None, &find(&read.uncommitted, "new näme.txt"))
        .await
        .expect("rename");
    assert!(renamed.contains("rename to new näme.txt"), "{renamed}");
    let loose = changed_file_diff(&dir, None, &find(&read.uncommitted, "loose.txt")).await;
    assert_eq!(loose, Ok("loose body\n".to_string()));
    let scratch = changed_file_diff(&dir, None, &find(&read.uncommitted, "scratch/"))
        .await
        .expect("directory");
    assert!(scratch.contains("untracked directory"), "{scratch}");
    let pushed = find(read.outgoing.files(), "ü.txt");
    let ahead = changed_file_diff(&dir, Some("origin/main"), &pushed)
        .await
        .expect("ü.txt");
    assert!(ahead.contains("+1\n+2"), "{ahead}");
    assert!(
        changed_file_diff(&dir, Some("no-such-ref"), &pushed)
            .await
            .is_err(),
        "a failing read is an error to show"
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&upstream).ok();
}

/// A branch with no upstream and a detached HEAD both say "no upstream"
/// and list nothing as unpushed; the toggle on the same repo lists the
/// branch's files against the default branch.
#[tokio::test]
async fn no_upstream_and_detached_head_say_so_and_the_toggle_compares_with_default() {
    let dir = temp_repo("changesnoup");
    sh(&dir, &["git", "checkout", "-q", "-b", "feat"]);
    std::fs::write(dir.join("f.txt"), "f\n").unwrap();
    sh(&dir, &["git", "add", "."]);
    sh(&dir, &["git", "commit", "-q", "-m", "f"]);
    let vs_main = Outgoing::VsDefault {
        base: "main".to_string(),
        files: vec![changed("A", "f.txt", Some((1, 0)))],
    };

    let read = changes(&dir, ChangesBase::Upstream).await.expect("reads");
    assert_eq!(
        read.outgoing,
        Outgoing::NoUpstream {
            branch: Some("feat".to_string())
        }
    );
    assert!(read.uncommitted.is_empty());
    let toggled = changes(&dir, ChangesBase::DefaultBranch).await.unwrap();
    assert_eq!(toggled.outgoing, vs_main);

    sh(&dir, &["git", "checkout", "-q", "--detach"]);
    let read = changes(&dir, ChangesBase::Upstream).await.unwrap();
    assert_eq!(read.outgoing, Outgoing::NoUpstream { branch: None });
    let toggled = changes(&dir, ChangesBase::DefaultBranch).await.unwrap();
    assert_eq!(toggled.outgoing, vs_main);
    std::fs::remove_dir_all(&dir).ok();
}

/// A branch both behind and ahead of its upstream lists only its own
/// commits' files as unpushed: the range starts at the merge base, so
/// the upstream's new file is not drawn as one this branch deleted.
#[tokio::test]
async fn a_branch_behind_and_ahead_lists_only_its_own_files() {
    let (upstream, dir) = fetched_clone("changesbehind");
    std::fs::write(upstream.join("up.txt"), "up\n").unwrap();
    sh(&upstream, &["git", "add", "."]);
    sh(&upstream, &["git", "commit", "-q", "-m", "up"]);
    commit_user(&dir);
    std::fs::write(dir.join("mine.txt"), "mine\n").unwrap();
    sh(&dir, &["git", "add", "."]);
    sh(&dir, &["git", "commit", "-q", "-m", "mine"]);
    sh(&dir, &["git", "fetch", "-q"]);
    let read = changes(&dir, ChangesBase::Upstream).await.expect("reads");
    assert_eq!(
        read.outgoing,
        Outgoing::Unpushed {
            upstream: "origin/main".to_string(),
            ahead: 1,
            files: vec![changed("A", "mine.txt", Some((1, 0)))],
        }
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&upstream).ok();
}
