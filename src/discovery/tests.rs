use super::*;
use std::fs;

fn setup_test_repos(base: &Path) -> Vec<String> {
    // Create fake repos
    for name in ["alpha", "beta", "gamma"] {
        let repo = base.join(name);
        fs::create_dir_all(repo.join(".git")).unwrap();
    }
    vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
}

fn by_name(names: &[String]) -> Wanted {
    Wanted::new(names.iter().map(|n| (None, n.as_str())))
}

/// A `.git/config` with `origin` pointing at `url`, the way `git clone`
/// leaves it.
fn clone_with_origin(path: &Path, url: &str) {
    fs::create_dir_all(path.join(".git")).unwrap();
    fs::write(
        path.join(".git").join("config"),
        format!(
            "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = {url}\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n"
        ),
    )
    .unwrap();
}

#[test]
fn find_in_primary_matches_a_clone_by_its_remote_whatever_the_folder_is_called() {
    // The regression: `~/repos/DevOps_Portfolio` is a clone of
    // `Gaurgle/portfolio-site`, and matching on the folder name alone
    // reported the repo as not cloned. The row takes the GitHub name, since
    // everything downstream keys on it.
    let dir = std::env::temp_dir().join("fleetz_test_remote_match");
    let _ = fs::remove_dir_all(&dir);
    let clone = dir.join("DevOps_Portfolio");
    clone_with_origin(&clone, "git@github.com:Gaurgle/portfolio-site.git");

    let wanted = Wanted::new([(Some("Gaurgle"), "portfolio-site")]);
    let found = find_in_primary(std::slice::from_ref(&dir), &wanted);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "portfolio-site");
    assert_eq!(found[0].path, clone);

    fs::remove_dir_all(dir).ok();
}

#[test]
fn a_remote_under_another_owner_does_not_match() {
    // The owner is part of the key, so a clone of someone else's `fleetz`
    // can't claim yours through its remote.
    let dir = std::env::temp_dir().join("fleetz_test_remote_owner");
    let _ = fs::remove_dir_all(&dir);
    clone_with_origin(&dir.join("their-fork"), "git@github.com:someone/fleetz.git");

    let wanted = Wanted::new([(Some("Gaurgle"), "fleetz")]);
    assert!(find_in_primary(std::slice::from_ref(&dir), &wanted).is_empty());

    fs::remove_dir_all(dir).ok();
}

#[test]
fn a_linked_worktree_does_not_match_through_its_main_clones_remote() {
    // A worktree shares its main clone's remote. Matched that way it would
    // race the real clone for the row, so only the clone may match.
    let dir = std::env::temp_dir().join("fleetz_test_remote_worktree");
    let _ = fs::remove_dir_all(&dir);
    let main = dir.join("site");
    clone_with_origin(&main, "git@github.com:Gaurgle/portfolio-site.git");
    let wt = dir.join("site-feature");
    fs::create_dir_all(&wt).unwrap();
    fs::write(
        wt.join(".git"),
        format!(
            "gitdir: {}\n",
            main.join(".git/worktrees/site-feature").display()
        ),
    )
    .unwrap();

    let wanted = Wanted::new([(Some("Gaurgle"), "portfolio-site")]);
    let found = find_in_primary(std::slice::from_ref(&dir), &wanted);
    let paths: Vec<_> = found.iter().map(|f| f.path.clone()).collect();
    assert_eq!(paths, vec![main]);

    fs::remove_dir_all(dir).ok();
}

#[test]
fn find_elsewhere_matches_by_remote_too() {
    let home = std::env::temp_dir().join("fleetz_test_remote_elsewhere");
    let _ = fs::remove_dir_all(&home);
    let clone = home.join("code").join("website");
    clone_with_origin(&clone, "https://github.com/Gaurgle/portfolio-site.git");

    let wanted = Wanted::new([(Some("Gaurgle"), "portfolio-site")]);
    let found = find_elsewhere(&home, &[], &wanted, &HashSet::new());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "portfolio-site");
    assert_eq!(found[0].path, clone);

    fs::remove_dir_all(home).ok();
}

#[test]
fn origin_url_reads_only_the_origin_remote() {
    let config = "[remote \"upstream\"]\n\turl = git@github.com:acme/tool.git\n\
                  [remote \"origin\"]\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n\
                  \turl = git@github.com:me/tool.git\n[branch \"main\"]\n\tremote = origin\n";
    assert_eq!(origin_url(config), Some("git@github.com:me/tool.git"));
    assert_eq!(origin_url("[core]\n\tbare = false\n"), None);
}

#[test]
fn find_in_primary_finds_matching_repos() {
    let dir = std::env::temp_dir().join("fleetz_test_primary");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let names = setup_test_repos(&dir);

    let found = find_in_primary(std::slice::from_ref(&dir), &by_name(&names));
    let found_names: HashSet<_> = found.iter().map(|f| f.name.clone()).collect();
    assert!(found_names.contains("alpha"));
    assert!(found_names.contains("beta"));
    assert!(found_names.contains("gamma"));

    fs::remove_dir_all(dir).ok();
}

#[test]
fn find_in_primary_is_case_insensitive() {
    let dir = std::env::temp_dir().join("fleetz_test_case");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("MyRepo").join(".git")).unwrap();

    let found = find_in_primary(
        std::slice::from_ref(&dir),
        &by_name(&["myrepo".to_string()]),
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "MyRepo");

    fs::remove_dir_all(dir).ok();
}

#[test]
fn find_in_primary_ignores_non_matching() {
    let dir = std::env::temp_dir().join("fleetz_test_nomatch");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("other").join(".git")).unwrap();

    let found = find_in_primary(
        std::slice::from_ref(&dir),
        &by_name(&["nothere".to_string()]),
    );
    assert!(found.is_empty());

    fs::remove_dir_all(dir).ok();
}

#[test]
fn find_in_primary_searches_every_root() {
    // The regression this guards: a repo in an extra root (repoz's
    // EXTRA_DIRS) must get a real status row, not fall through to
    // "not cloned".
    let base = std::env::temp_dir().join("fleetz_test_roots");
    let _ = fs::remove_dir_all(&base);
    let primary = base.join("repos");
    let extra = base.join("elsewhere");
    fs::create_dir_all(primary.join("alpha").join(".git")).unwrap();
    fs::create_dir_all(extra.join("beta").join(".git")).unwrap();

    let found = find_in_primary(
        &[primary, extra],
        &by_name(&["alpha".to_string(), "beta".to_string()]),
    );
    let names: HashSet<_> = found.iter().map(|f| f.name.clone()).collect();
    assert!(names.contains("alpha"));
    assert!(names.contains("beta"));

    fs::remove_dir_all(base).ok();
}

#[test]
fn find_in_primary_matches_a_dot_prefixed_clone() {
    // The GitHub repo `dotfiles` lives at `~/.dotfiles`; handing the
    // hidden dir in as a root must match it and report the *undotted*
    // name, since the GitHub match downstream keys on that.
    let base = std::env::temp_dir().join("fleetz_test_dotname");
    let _ = fs::remove_dir_all(&base);
    let hidden = base.join(".dotfiles");
    fs::create_dir_all(hidden.join(".git")).unwrap();

    let found = find_in_primary(
        std::slice::from_ref(&hidden),
        &by_name(&["dotfiles".to_string()]),
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "dotfiles", "leading dot stripped");
    assert_eq!(found[0].path, hidden);

    fs::remove_dir_all(base).ok();
}

#[test]
fn find_in_primary_dedupes_across_overlapping_roots() {
    let base = std::env::temp_dir().join("fleetz_test_overlap");
    let _ = fs::remove_dir_all(&base);
    let nested = base.join("nested");
    fs::create_dir_all(nested.join("alpha").join(".git")).unwrap();

    let found = find_in_primary(&[base.clone(), nested], &by_name(&["alpha".to_string()]));
    assert_eq!(found.len(), 1, "one repo, one row");

    fs::remove_dir_all(base).ok();
}

#[test]
fn repo_name_for_path_strips_a_leading_dot() {
    assert_eq!(
        repo_name_for_path(Path::new("/home/me/.dotfiles")),
        "dotfiles"
    );
    assert_eq!(repo_name_for_path(Path::new("/home/me/repos/pinz")), "pinz");
}

#[test]
fn find_all_local_returns_all_git_dirs() {
    let dir = std::env::temp_dir().join("fleetz_test_all");
    let _ = fs::remove_dir_all(&dir);
    setup_test_repos(&dir);

    let all = find_all_local(std::slice::from_ref(&dir));
    assert_eq!(all.len(), 3);

    fs::remove_dir_all(dir).ok();
}

#[test]
fn find_all_local_includes_gitdir_pointer_worktrees() {
    let dir = std::env::temp_dir().join("fleetz_test_wt_pointer");
    let _ = fs::remove_dir_all(&dir);
    let wt = dir.join("wt");
    fs::create_dir_all(&wt).unwrap();
    // A linked worktree has a .git *file* pointing at the real git dir.
    fs::write(wt.join(".git"), "gitdir: /somewhere/.git/worktrees/wt\n").unwrap();

    let all = find_all_local(std::slice::from_ref(&dir));
    assert_eq!(all, vec![wt]);

    fs::remove_dir_all(dir).ok();
}

#[test]
fn scan_respects_max_depth() {
    let dir = std::env::temp_dir().join("fleetz_test_depth");
    let _ = fs::remove_dir_all(&dir);
    // Create a repo at depth 5: should not be found at maxdepth 3
    let deep = dir.join("a").join("b").join("c").join("d").join("deep");
    fs::create_dir_all(deep.join(".git")).unwrap();

    let found = find_in_primary(std::slice::from_ref(&dir), &by_name(&["deep".to_string()]));
    assert!(found.is_empty());

    fs::remove_dir_all(dir).ok();
}

// Repository identity scenarios (docs/repository-identity-design.md,
// section 4). Each fixture lives in its own directory, unique per process.

fn scenario_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fleetz_test_fz05b_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A linked worktree: a `.git` file pointing at a git dir elsewhere.
fn linked_worktree(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::write(path.join(".git"), "gitdir: /elsewhere/.git/worktrees/wt\n").unwrap();
}

/// A clone with `origin` at `origin_url` and a second remote `other`.
fn clone_with_two_remotes(path: &Path, origin_url: &str, other: &str, other_url: &str) {
    fs::create_dir_all(path.join(".git")).unwrap();
    fs::write(
        path.join(".git").join("config"),
        format!(
            "[remote \"{other}\"]\n\turl = {other_url}\n[remote \"origin\"]\n\turl = {origin_url}\n"
        ),
    )
    .unwrap();
}

fn paths(found: &[FoundRepo]) -> Vec<PathBuf> {
    found.iter().map(|f| f.path.clone()).collect()
}

#[test]
fn s2_a_remote_of_another_owner_does_not_claim_a_row_through_its_folder() {
    // S2 while the dataset still holds only `alice/api`: the clone of
    // `company/api` used to claim alice's row by its folder name.
    let dir = scenario_dir("s2_interim");
    clone_with_origin(&dir.join("api"), "git@github.com:company/api.git");

    let wanted = Wanted::new([(Some("alice"), "api")]);
    assert!(find_in_primary(std::slice::from_ref(&dir), &wanted).is_empty());

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s2_the_remote_beats_the_folder() {
    let dir = scenario_dir("s2");
    let clone = dir.join("api");
    clone_with_origin(&clone, "git@github.com:company/api.git");

    let wanted = Wanted::new([(Some("alice"), "api"), (Some("company"), "api")]);
    let found = find_in_primary(std::slice::from_ref(&dir), &wanted);
    assert_eq!(paths(&found), vec![clone]);
    assert_eq!(found[0].name, "api");
    assert_eq!(found[0].owner.as_deref(), Some("company"));

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s3_a_remote_less_clone_of_a_name_two_owners_share_claims_no_row() {
    let dir = scenario_dir("s3");
    fs::create_dir_all(dir.join("api").join(".git")).unwrap();

    let wanted = Wanted::new([(Some("alice"), "api"), (Some("company"), "api")]);
    assert!(find_in_primary(std::slice::from_ref(&dir), &wanted).is_empty());

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s7_a_linked_worktree_keeps_the_folder_fallback() {
    // D10: a worktree has no config of its own, so its folder decides.
    let dir = scenario_dir("s7");
    let wt = dir.join("api");
    linked_worktree(&wt);

    let wanted = Wanted::new([(Some("company"), "api")]);
    let found = find_in_primary(std::slice::from_ref(&dir), &wanted);
    assert_eq!(paths(&found), vec![wt]);
    assert_eq!(found[0].name, "api");
    assert_eq!(found[0].owner.as_deref(), Some("company"));

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s9a_a_clone_on_the_old_name_in_the_old_folder_matches_nothing() {
    let dir = scenario_dir("s9a");
    clone_with_origin(&dir.join("old-name"), "git@github.com:Gaurgle/old-name.git");

    let wanted = Wanted::new([(Some("Gaurgle"), "new-name")]);
    assert!(find_in_primary(std::slice::from_ref(&dir), &wanted).is_empty());

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s9b_a_renamed_repo_in_a_renamed_folder_still_matches() {
    // Same owner, and `old-name` is not a repo fleetz knows: rule 3.
    let dir = scenario_dir("s9b");
    let clone = dir.join("new-name");
    clone_with_origin(&clone, "git@github.com:gaurgle/old-name.git");

    let wanted = Wanted::new([(Some("Gaurgle"), "new-name")]);
    let found = find_in_primary(std::slice::from_ref(&dir), &wanted);
    assert_eq!(paths(&found), vec![clone]);
    assert_eq!(found[0].name, "new-name");
    assert_eq!(found[0].owner.as_deref(), Some("Gaurgle"));

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s9c_a_clone_of_a_transferred_repo_does_not_claim_the_new_owners_row() {
    let dir = scenario_dir("s9c");
    clone_with_origin(&dir.join("x"), "git@github.com:Gaurgle/x.git");

    let wanted = Wanted::new([(Some("acme"), "x")]);
    assert!(find_in_primary(std::slice::from_ref(&dir), &wanted).is_empty());

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s11a_a_foreign_host_project_does_not_claim_a_row_through_its_folder() {
    let dir = scenario_dir("s11a");
    clone_with_origin(&dir.join("api"), "git@gitlab.com:someone/api.git");

    let wanted = Wanted::new([(Some("company"), "api")]);
    assert!(find_in_primary(std::slice::from_ref(&dir), &wanted).is_empty());

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s12_a_fork_does_not_claim_its_parents_row() {
    // Until the dataset keeps both, only the parent `acme/tool` is wanted.
    let dir = scenario_dir("s12_interim");
    clone_with_two_remotes(
        &dir.join("tool"),
        "git@github.com:Gaurgle/tool.git",
        "upstream",
        "git@github.com:acme/tool.git",
    );

    let wanted = Wanted::new([(Some("acme"), "tool")]);
    assert!(find_in_primary(std::slice::from_ref(&dir), &wanted).is_empty());

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s12_a_fork_matches_its_own_row_when_both_are_wanted() {
    let dir = scenario_dir("s12");
    let clone = dir.join("tool");
    clone_with_two_remotes(
        &clone,
        "git@github.com:Gaurgle/tool.git",
        "upstream",
        "git@github.com:acme/tool.git",
    );

    let wanted = Wanted::new([(Some("Gaurgle"), "tool"), (Some("acme"), "tool")]);
    let found = find_in_primary(std::slice::from_ref(&dir), &wanted);
    assert_eq!(paths(&found), vec![clone]);
    assert_eq!(found[0].name, "tool");
    assert_eq!(
        found[0].owner.as_deref(),
        Some("Gaurgle"),
        "the fork, not acme"
    );

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s12b_a_clone_whose_origin_is_the_upstream_does_not_claim_the_fork() {
    // Accepted regression (m3): only `origin` is read, and its owner is
    // not the fork's.
    let dir = scenario_dir("s12b");
    clone_with_two_remotes(
        &dir.join("tool"),
        "git@github.com:acme/tool.git",
        "mine",
        "git@github.com:Gaurgle/tool.git",
    );

    let wanted = Wanted::new([(Some("Gaurgle"), "tool")]);
    assert!(find_in_primary(std::slice::from_ref(&dir), &wanted).is_empty());

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s13_a_dot_prefixed_clone_matches_with_and_without_an_origin() {
    let base = scenario_dir("s13");
    let hidden = base.join(".dotfiles");
    let wanted = Wanted::new([(Some("Gaurgle"), "dotfiles")]);

    clone_with_origin(&hidden, "git@github.com:Gaurgle/dotfiles.git");
    let found = find_in_primary(std::slice::from_ref(&hidden), &wanted);
    assert_eq!(paths(&found), vec![hidden.clone()], "by its origin");
    assert_eq!(found[0].name, "dotfiles");

    fs::remove_file(hidden.join(".git").join("config")).unwrap();
    let found = find_in_primary(std::slice::from_ref(&hidden), &wanted);
    assert_eq!(paths(&found), vec![hidden.clone()], "by its folder");
    assert_eq!(found[0].name, "dotfiles", "leading dot stripped");

    fs::remove_dir_all(base).ok();
}

#[test]
fn a_legacy_repo_without_an_owner_keeps_the_folder_fallback() {
    // A payload without `login` gives the wanted repo no owner, so rule 3's
    // owner condition cannot be judged and today's fallback stays.
    let dir = scenario_dir("legacy");
    let clone = dir.join("tool");
    clone_with_origin(&clone, "git@github.com:someone/tool.git");

    let wanted = Wanted::new([(None, "tool")]);
    let found = find_in_primary(std::slice::from_ref(&dir), &wanted);
    assert_eq!(paths(&found), vec![clone]);
    assert_eq!(found[0].owner, None);

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s4_the_clone_with_fewer_path_components_is_primary() {
    // The deeper clone sorts first by path, so only depth picks `near`.
    let dir = scenario_dir("s4");
    let near = dir.join("fleetz");
    clone_with_origin(&near, "git@github.com:Gaurgle/fleetz.git");
    clone_with_origin(
        &dir.join("a").join("fleetz"),
        "git@github.com:Gaurgle/fleetz.git",
    );

    let wanted = Wanted::new([(Some("Gaurgle"), "fleetz")]);
    let found = find_in_primary(std::slice::from_ref(&dir), &wanted);
    assert_eq!(paths(&found), vec![near]);

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s5_of_two_equally_deep_clones_the_first_path_is_primary() {
    let dir = scenario_dir("s5");
    let clone = dir.join("fleetz");
    clone_with_origin(&clone, "git@github.com:Gaurgle/fleetz.git");
    clone_with_origin(
        &dir.join("fleetz-copy"),
        "https://github.com/Gaurgle/fleetz",
    );

    let wanted = Wanted::new([(Some("Gaurgle"), "fleetz")]);
    let found = find_in_primary(std::slice::from_ref(&dir), &wanted);
    assert_eq!(paths(&found), vec![clone]);

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s5_the_first_path_is_primary_whichever_clone_is_offered_first() {
    // Equal on every earlier step, so only the path decides. The walk's
    // order is the filesystem's, so the offers are made directly.
    let dir = scenario_dir("s5_offer");
    let (first, later) = (dir.join("fleetz"), dir.join("fleetz-copy"));
    clone_with_origin(&first, "git@github.com:Gaurgle/fleetz.git");
    clone_with_origin(&later, "git@github.com:Gaurgle/fleetz.git");
    let wanted = Wanted::new([(Some("Gaurgle"), "fleetz")]);

    for order in [[&first, &later], [&later, &first]] {
        let mut collector = Collector::default();
        for path in order {
            collector.offer(&wanted, path, 0);
        }
        let (found, _) = collector.finish();
        assert_eq!(paths(&found), vec![first.clone()], "offered {order:?}");
    }

    fs::remove_dir_all(dir).ok();
}

#[test]
fn an_exact_remote_beats_a_folder_match_in_an_earlier_root() {
    let base = scenario_dir("order_exact");
    let (first, second) = (base.join("one"), base.join("two"));
    fs::create_dir_all(first.join("api").join(".git")).unwrap();
    let exact = second.join("company-api");
    clone_with_origin(&exact, "git@github.com:company/api.git");

    let wanted = Wanted::new([(Some("company"), "api")]);
    let found = find_in_primary(&[first, second], &wanted);
    assert_eq!(paths(&found), vec![exact]);

    fs::remove_dir_all(base).ok();
}

#[test]
fn an_exact_remote_on_another_host_beats_a_github_folder_match() {
    // `api` claims by rule 3 (same owner, `api-old` unknown) with an origin
    // on github.com; `x` matches exactly but on gitlab.com. Only the exact
    // remote step, ahead of the host step, picks `x`.
    let base = scenario_dir("order_exact_host");
    let (first, second) = (base.join("one"), base.join("two"));
    clone_with_origin(&first.join("api"), "git@github.com:company/api-old.git");
    let exact = second.join("x");
    clone_with_origin(&exact, "git@gitlab.com:company/api.git");

    let wanted = Wanted::new([(Some("company"), "api")]);
    let found = find_in_primary(&[first.clone(), second], &wanted);
    assert_eq!(paths(&found), vec![exact]);

    let alone = find_in_primary(std::slice::from_ref(&first), &wanted);
    assert_eq!(
        paths(&alone),
        vec![first.join("api")],
        "the folder clone claims by rule 3 on its own"
    );

    fs::remove_dir_all(base).ok();
}

#[test]
fn an_origin_on_github_com_beats_another_host_in_an_earlier_root() {
    // o2. Matching is host-agnostic (D2), so both clones match exactly.
    let base = scenario_dir("order_host");
    let (first, second) = (base.join("one"), base.join("two"));
    clone_with_origin(&first.join("mirror"), "git@gitlab.com:Gaurgle/fleetz.git");
    let github = second.join("fleetz");
    clone_with_origin(&github, "git@github.com:Gaurgle/fleetz.git");

    let wanted = Wanted::new([(Some("Gaurgle"), "fleetz")]);
    let found = find_in_primary(&[first, second], &wanted);
    assert_eq!(paths(&found), vec![github]);

    fs::remove_dir_all(base).ok();
}

#[test]
fn a_git_directory_beats_a_linked_worktree_in_an_earlier_root() {
    let base = scenario_dir("order_gitdir");
    let (first, second) = (base.join("one"), base.join("two"));
    linked_worktree(&first.join("api"));
    let clone = second.join("api");
    fs::create_dir_all(clone.join(".git")).unwrap();

    let wanted = Wanted::new([(Some("company"), "api")]);
    let found = find_in_primary(&[first, second], &wanted);
    assert_eq!(paths(&found), vec![clone]);

    fs::remove_dir_all(base).ok();
}

#[test]
fn an_earlier_root_beats_a_shallower_path_in_a_later_one() {
    let base = scenario_dir("order_root");
    let (first, second) = (base.join("one"), base.join("two"));
    let deep = first.join("a").join("b").join("fleetz");
    clone_with_origin(&deep, "git@github.com:Gaurgle/fleetz.git");
    clone_with_origin(&second.join("fleetz"), "git@github.com:Gaurgle/fleetz.git");

    let wanted = Wanted::new([(Some("Gaurgle"), "fleetz")]);
    let found = find_in_primary(&[first, second], &wanted);
    assert_eq!(paths(&found), vec![deep]);

    fs::remove_dir_all(base).ok();
}

#[test]
fn find_elsewhere_returns_one_clone_per_repo() {
    // The deeper clone sorts first by path, so only depth picks `near`.
    let home = scenario_dir("elsewhere_one");
    let near = home.join("code").join("fleetz");
    clone_with_origin(&near, "git@github.com:Gaurgle/fleetz.git");
    clone_with_origin(
        &home.join("code").join("a").join("fleetz"),
        "git@github.com:Gaurgle/fleetz.git",
    );

    let wanted = Wanted::new([(Some("Gaurgle"), "fleetz")]);
    let found = find_elsewhere(&home, &[], &wanted, &HashSet::new());
    assert_eq!(paths(&found), vec![near]);

    fs::remove_dir_all(home).ok();
}

#[test]
fn s3_the_ambiguous_clone_comes_back_as_data() {
    // The ambiguous clone does not stop a clone with a remote from
    // claiming its own row.
    let dir = scenario_dir("s3_data");
    let ambiguous = dir.join("api");
    fs::create_dir_all(ambiguous.join(".git")).unwrap();
    let alice = dir.join("alice-api");
    clone_with_origin(&alice, "https://github.com/alice/api");

    let wanted = Wanted::new([(Some("alice"), "api"), (Some("company"), "api")]);
    let (found, ambiguities) =
        find_in_primary_with_ambiguities(std::slice::from_ref(&dir), &wanted);
    assert_eq!(paths(&found), vec![alice]);
    assert_eq!(found[0].owner.as_deref(), Some("alice"));
    assert_eq!(
        ambiguities,
        vec![Ambiguity {
            path: ambiguous,
            name: "api".to_string(),
        }]
    );

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s3b_ambiguity_is_judged_on_the_full_dataset_not_the_window() {
    // A one-day window holds only `alice/api`; `company/api` was pushed
    // 60 days ago and is in the full dataset only.
    let dir = scenario_dir("s3b");
    let clone = dir.join("api");
    fs::create_dir_all(clone.join(".git")).unwrap();

    let wanted = Wanted::with_full_dataset(
        [(Some("alice"), "api")],
        [(Some("alice"), "api"), (Some("company"), "api")],
    );
    let (found, ambiguities) =
        find_in_primary_with_ambiguities(std::slice::from_ref(&dir), &wanted);
    assert!(found.is_empty(), "claimed {found:?}");
    assert_eq!(paths_of(&ambiguities), vec![clone]);

    fs::remove_dir_all(dir).ok();
}

#[test]
fn a_name_whose_owner_is_unique_in_the_full_dataset_is_not_ambiguous() {
    let dir = scenario_dir("unique_owner");
    let clone = dir.join("api");
    fs::create_dir_all(clone.join(".git")).unwrap();

    let wanted = Wanted::with_full_dataset(
        [(Some("alice"), "api")],
        [(Some("alice"), "api"), (Some("alice"), "other")],
    );
    let (found, ambiguities) =
        find_in_primary_with_ambiguities(std::slice::from_ref(&dir), &wanted);
    assert_eq!(paths(&found), vec![clone]);
    assert_eq!(found[0].owner.as_deref(), Some("alice"));
    assert!(ambiguities.is_empty());

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s9d_an_origin_naming_a_known_repo_of_the_same_owner_does_not_claim_by_folder() {
    // `Gaurgle/other` is outside the window but known to fleetz, so the
    // clone is that repo's, not `Gaurgle/tool`'s.
    let dir = scenario_dir("s9d");
    clone_with_origin(&dir.join("tool"), "git@github.com:Gaurgle/other.git");

    let wanted = Wanted::with_full_dataset(
        [(Some("Gaurgle"), "tool")],
        [(Some("Gaurgle"), "tool"), (Some("Gaurgle"), "other")],
    );
    assert!(find_in_primary(std::slice::from_ref(&dir), &wanted).is_empty());

    fs::remove_dir_all(dir).ok();
}

#[test]
fn s9d_with_the_origins_repo_in_the_window_the_remote_matches() {
    let dir = scenario_dir("s9d_window");
    let clone = dir.join("tool");
    clone_with_origin(&clone, "git@github.com:Gaurgle/other.git");

    let both = [(Some("Gaurgle"), "tool"), (Some("Gaurgle"), "other")];
    let wanted = Wanted::with_full_dataset(both, both);
    let found = find_in_primary(std::slice::from_ref(&dir), &wanted);
    assert_eq!(paths(&found), vec![clone]);
    assert_eq!(found[0].name, "other");

    fs::remove_dir_all(dir).ok();
}

#[test]
fn ambiguities_are_sorted_by_path_across_roots() {
    // o3: the first entry must not depend on root or walk order.
    let base = scenario_dir("ambiguity_order");
    let (first, second) = (base.join("z-root"), base.join("a-root"));
    for clone in [
        first.join("api"),
        second.join("b").join("api"),
        second.join("a").join(".api"),
    ] {
        fs::create_dir_all(clone.join(".git")).unwrap();
    }

    let wanted = Wanted::new([(Some("alice"), "api"), (Some("company"), "api")]);
    let (found, ambiguities) =
        find_in_primary_with_ambiguities(&[first.clone(), second.clone(), first.clone()], &wanted);
    assert!(found.is_empty());
    assert_eq!(
        paths_of(&ambiguities),
        vec![
            second.join("a").join(".api"),
            second.join("b").join("api"),
            first.join("api"),
        ],
        "sorted by path, a root listed twice reports once"
    );
    assert_eq!(ambiguities[0].name, "api", "leading dot stripped");

    fs::remove_dir_all(base).ok();
}

#[test]
fn find_elsewhere_still_looks_for_another_owners_repo_of_a_found_name() {
    // Pass 1 found `alice/api`; `company/api` is still wanted in pass 2.
    let home = scenario_dir("elsewhere_keyed");
    let root = home.join("repos");
    let alice = root.join("api");
    clone_with_origin(&alice, "git@github.com:alice/api.git");
    let company = home.join("work").join("api");
    clone_with_origin(&company, "git@github.com:company/api.git");

    let wanted = Wanted::new([(Some("alice"), "api"), (Some("company"), "api")]);
    let roots = vec![root];
    let primary = find_in_primary(&roots, &wanted);
    assert_eq!(paths(&primary), vec![alice]);
    let found_set: HashSet<String> = primary.iter().map(FoundRepo::key).collect();

    let elsewhere = find_elsewhere(&home, &roots, &wanted, &found_set);
    assert_eq!(paths(&elsewhere), vec![company]);
    assert_eq!(elsewhere[0].owner.as_deref(), Some("company"));

    fs::remove_dir_all(home).ok();
}

#[test]
fn find_elsewhere_skips_a_repo_pass_one_found() {
    let home = scenario_dir("elsewhere_skip");
    clone_with_origin(
        &home.join("work").join("fleetz"),
        "git@github.com:Gaurgle/fleetz.git",
    );
    fs::create_dir_all(home.join("work").join("legacy").join(".git")).unwrap();

    let wanted = Wanted::new([(Some("Gaurgle"), "fleetz"), (None, "legacy")]);
    let found_set: HashSet<String> = [
        FoundRepo {
            name: "Fleetz".to_string(),
            path: PathBuf::from("/r/Fleetz"),
            owner: Some("gaurgle".to_string()),
        },
        FoundRepo {
            name: "Legacy".to_string(),
            path: PathBuf::from("/r/Legacy"),
            owner: None,
        },
    ]
    .iter()
    .map(FoundRepo::key)
    .collect();
    assert!(find_elsewhere(&home, &[], &wanted, &found_set).is_empty());

    fs::remove_dir_all(home).ok();
}

#[test]
fn found_repo_key_is_owner_and_name_lowercased_or_the_name_alone() {
    let found = |owner: Option<&str>| FoundRepo {
        name: "FleetZ".to_string(),
        path: PathBuf::from("/r/FleetZ"),
        owner: owner.map(str::to_string),
    };
    assert_eq!(found(Some("Gaurgle")).key(), "gaurgle/fleetz");
    assert_eq!(found(None).key(), "fleetz");
}

fn paths_of(ambiguities: &[Ambiguity]) -> Vec<PathBuf> {
    ambiguities.iter().map(|a| a.path.clone()).collect()
}
