use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Result of discovering a local clone of a GitHub repo.
#[derive(Debug, Clone)]
pub struct FoundRepo {
    pub name: String,
    pub path: PathBuf,
}

/// The GitHub repos discovery is looking for, indexed both ways a local clone
/// can be recognized: by its `origin` remote, and by its folder name.
#[derive(Debug, Default)]
pub struct Wanted {
    /// Lowercased `owner/name` to the GitHub name. Only repos the fetch
    /// reported an owner for; the rest can still match by folder name.
    by_remote: HashMap<String, String>,
    /// Lowercased GitHub names.
    names: HashSet<String>,
}

impl Wanted {
    /// Built from `(owner, name)` pairs as the GitHub fetch reports them.
    pub fn new<'a>(repos: impl IntoIterator<Item = (Option<&'a str>, &'a str)>) -> Self {
        let mut wanted = Self::default();
        for (owner, name) in repos {
            if let Some(owner) = owner {
                wanted
                    .by_remote
                    .insert(format!("{owner}/{name}").to_lowercase(), name.to_string());
            }
            wanted.names.insert(name.to_lowercase());
        }
        wanted
    }

    /// The repos not in `found`, which holds lowercased GitHub names.
    fn without(&self, found: &HashSet<String>) -> Self {
        Self {
            by_remote: self
                .by_remote
                .iter()
                .filter(|(_, name)| !found.contains(&name.to_lowercase()))
                .map(|(slug, name)| (slug.clone(), name.clone()))
                .collect(),
            names: self.names.difference(found).cloned().collect(),
        }
    }

    fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// The GitHub repo name the clone at `path` stands for, or None when it is
    /// none of these.
    ///
    /// The remote is tried first because a folder name is only a convention: a
    /// clone of `portfolio-site` can sit in `DevOps_Portfolio`. The folder name
    /// stays as the fallback for a clone with no `origin`, a remote naming a
    /// repo the fetch has no owner for, and a linked worktree (see
    /// `origin_slug`).
    ///
    /// A repo renamed on GitHub is not covered by the remote either: the old
    /// URL keeps working through GitHub's redirect, so the clone's config still
    /// names the old repo until `git remote set-url` updates it.
    fn match_clone(&self, path: &Path) -> Option<String> {
        if let Some(name) = origin_slug(path).and_then(|slug| self.by_remote.get(&slug)) {
            return Some(name.clone());
        }
        let dir_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        matched_name(dir_name, &self.names)
    }
}

/// Pass 1: Find repos under the configured scan roots (the primary repo_dir
/// plus any `extra_dirs`, maxdepth 3).
///
/// Extra roots are searched *here*, not just in `find_all_local`, so a repo
/// living outside the main tree still gets a full GitHub-matched status row.
/// This is what reaches a clone under a hidden path (`~/.dotfiles`), which
/// the `$HOME` walker of pass 2 prunes: a root handed in directly is never
/// subject to the hidden-dir skip. Mirrors repoz's pass 1, which falls back
/// to `EXTRA_DIRS` when `REPO_DIR` has no match.
///
/// De-duplicated by name, first root winning, so overlapping roots can't
/// produce two rows for one repo.
pub fn find_in_primary(roots: &[PathBuf], wanted: &Wanted) -> Vec<FoundRepo> {
    let mut found = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for root in roots {
        scan_for_git_repos(root, 3, false, &mut |path| {
            let Some(name) = wanted.match_clone(path) else {
                return;
            };
            if seen.insert(name.to_lowercase()) {
                found.push(FoundRepo {
                    name,
                    path: path.to_path_buf(),
                });
            }
        });
    }

    found
}

/// Pass 2: Find repos elsewhere under $HOME (maxdepth 4), excluding
/// Library, .cache, node_modules, .Trash, and every pass 1 scan root (which
/// has already been searched, and searched deeper than a `$HOME` sweep would
/// reach it).
pub fn find_elsewhere(
    home: &Path,
    roots: &[PathBuf],
    wanted: &Wanted,
    already_found: &HashSet<String>,
) -> Vec<FoundRepo> {
    let remaining = wanted.without(already_found);

    if remaining.is_empty() {
        return Vec::new();
    }

    let mut found = Vec::new();

    scan_for_git_repos(home, 4, true, &mut |path| {
        // Skip excluded directories
        if is_excluded(path, roots) {
            return;
        }

        if let Some(name) = remaining.match_clone(path) {
            found.push(FoundRepo {
                name,
                path: path.to_path_buf(),
            });
        }
    });

    found
}

/// The GitHub repo name a local directory stands for, or None when it matches
/// none of `names_lower` (which must already be lowercased).
///
/// A clone can carry a leading dot its GitHub repo doesn't -- the repo
/// `dotfiles` conventionally lives at `~/.dotfiles` -- so the dotted spelling
/// is tried too, mirroring repoz's `-iname "$name" -o -iname ".$name"`. The
/// dot is stripped from the returned name because everything downstream (GitHub
/// match, glyph lookup, dedupe) keys on the GitHub name, not the folder name.
fn matched_name(dir_name: &str, names_lower: &HashSet<String>) -> Option<String> {
    if names_lower.contains(&dir_name.to_lowercase()) {
        return Some(dir_name.to_string());
    }
    let undotted = dir_name.strip_prefix('.')?;
    names_lower
        .contains(&undotted.to_lowercase())
        .then(|| undotted.to_string())
}

/// The lowercased `owner/name` the clone's `origin` remote points at.
///
/// Read straight from `.git/config` rather than through `git remote get-url`:
/// this runs for every candidate in both sweeps, and a process spawn each would
/// dwarf the walk itself. The cost is that `url.<base>.insteadOf` rewrites are
/// not applied.
///
/// Only a `.git` *directory* has a config to read. A `.git` file marks a linked
/// worktree or submodule; a worktree shares its main clone's remote, so
/// matching it that way would let it race the real clone for the row. Those
/// keep the folder-name match.
fn origin_slug(repo: &Path) -> Option<String> {
    let config = std::fs::read_to_string(repo.join(".git").join("config")).ok()?;
    let (owner, name) = crate::model::repo_from_remote_url(origin_url(&config)?)?;
    Some(format!("{owner}/{name}").to_lowercase())
}

/// The `url` of `[remote "origin"]` in the text of a git config file.
fn origin_url(config: &str) -> Option<&str> {
    let mut in_origin = false;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            in_origin = line == r#"[remote "origin"]"#;
        } else if in_origin
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("url")
        {
            return Some(value.trim().trim_matches('"'));
        }
    }
    None
}

/// The name a local-only repo reports as: its directory basename with a
/// leading dot stripped, so `~/.dotfiles` lines up with the GitHub repo
/// `dotfiles` for both glyph lookup and dedupe against passes 1 and 2.
pub fn repo_name_for_path(path: &Path) -> String {
    let base = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    base.strip_prefix('.').unwrap_or(base).to_string()
}

/// Pass 3: Find all local git repos under each of `roots` (maxdepth 4),
/// regardless of whether they have GitHub activity. `roots` is the primary
/// repo_dir plus any configured extra dirs (e.g. `~/.dotfiles`). A root that
/// is itself a git repo is picked up directly, since the hidden-dir skip in
/// the walker only prunes *descended* entries, not the root handed in.
/// Returns de-duplicated paths: the caller checks git status to filter for
/// the dirty ones.
pub fn find_all_local(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut repos = Vec::new();
    for root in roots {
        scan_for_git_repos(root, 4, false, &mut |path| {
            if seen.insert(path.to_path_buf()) {
                repos.push(path.to_path_buf());
            }
        });
    }
    repos
}

/// Directory names never descended into, mirroring repoz's prune list in
/// `scan_home_git_dirs`. Pruning by name stops descent into dependency and
/// build trees entirely rather than walking them and filtering after.
///
/// Tradeoff, and repoz makes the same one: a repo literally named `target` or
/// `build` is skipped.
const PRUNE_DIRS: [&str; 14] = [
    "Library",
    ".Trash",
    ".cache",
    "node_modules",
    "vendor",
    "target",
    ".venv",
    "venv",
    "dist",
    "build",
    "Pods",
    "DerivedData",
    ".terraform",
    ".gradle",
];

/// Walk directories up to `max_depth` looking for `.git` dirs.
/// Calls `callback` with the parent of each `.git` found.
///
/// `skip_hidden` blanket-skips dot-directories. That is right for the `$HOME`
/// sweep, where descending into every `~/.local`, `~/.npm` and friend would
/// cost far more than it finds. It is wrong for a root the user named
/// explicitly: pointing `repo_dir` or an `extra_dir` at a directory should find
/// the repos in it whether or not they happen to be hidden, which is what
/// repoz does. `PRUNE_DIRS` is applied either way.
fn scan_for_git_repos(
    root: &Path,
    max_depth: usize,
    skip_hidden: bool,
    callback: &mut dyn FnMut(&Path),
) {
    scan_recursive(root, 0, max_depth, skip_hidden, callback);
}

fn scan_recursive(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    skip_hidden: bool,
    callback: &mut dyn FnMut(&Path),
) {
    if depth > max_depth {
        return;
    }

    let git_dir = dir.join(".git");
    // A .git *file* (gitdir pointer) marks a linked worktree or submodule
    // checkout; both are real working trees worth listing.
    if git_dir.is_dir() || git_dir.is_file() {
        callback(dir);
        return; // don't descend into git repos
    }

    if depth == max_depth {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if PRUNE_DIRS.contains(&name_str.as_ref()) {
                continue;
            }
            if skip_hidden && name_str.starts_with('.') {
                continue;
            }
            scan_recursive(&path, depth + 1, max_depth, skip_hidden, callback);
        }
    }
}

fn is_excluded(path: &Path, roots: &[PathBuf]) -> bool {
    let s = path.to_string_lossy();
    roots.iter().any(|root| path.starts_with(root))
        || s.contains("/Library/")
        || s.contains("/.Trash/")
        || s.contains("/.cache/")
        || s.contains("/node_modules/")
}

#[cfg(test)]
mod tests {
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
}

/// Parity checks against repoz, which implements the same discovery rules in
/// bash and has had them right for longer.
///
/// The two tools are kept in agreement by hand today, and that is exactly how
/// the `~/.dotfiles` bug survived for months: repoz matched `-iname ".$name"`
/// from the start, fleetz never transcribed it. These tests mechanise the
/// comparison so the next divergence fails a build instead of being noticed
/// by eye.
///
/// Every fixture repo is made dirty on purpose: `repoz -d` lists dirty repos
/// only and needs no GitHub access, so a fixture where everything is dirty
/// yields the full discovered set from a pure-local run.
///
/// Skipped when repoz is not on PATH, which is visible in the test output
/// rather than silently passing.
#[cfg(test)]
mod repoz_parity {
    use super::*;
    use std::process::Command;

    fn repoz_bin() -> Option<PathBuf> {
        // The clone next to this one first, so the test checks the repoz you
        // are actually developing rather than whatever is installed.
        let sibling = dirs::home_dir()?.join("repos/repoz/repoz");
        if sibling.is_file() {
            return Some(sibling);
        }
        let out = Command::new("sh")
            .args(["-c", "command -v repoz"])
            .output()
            .ok()?;
        let path = String::from_utf8(out.stdout).ok()?.trim().to_string();
        (!path.is_empty()).then(|| PathBuf::from(path))
    }

    fn sh(dir: &Path, args: &[&str]) {
        let status = Command::new(args[0])
            .args(&args[1..])
            .current_dir(dir)
            .status()
            .expect("spawn");
        assert!(status.success(), "command failed: {args:?}");
    }

    /// A git repo at `base/rel` with one commit and one uncommitted change.
    fn dirty_repo(base: &Path, rel: &str) -> PathBuf {
        let dir = base.join(rel);
        std::fs::create_dir_all(&dir).unwrap();
        sh(&dir, &["git", "init", "-q", "-b", "main"]);
        sh(&dir, &["git", "config", "user.email", "t@example.com"]);
        sh(&dir, &["git", "config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        sh(&dir, &["git", "add", "."]);
        sh(&dir, &["git", "commit", "-q", "-m", "init"]);
        std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        dir
    }

    /// Absolute repo paths repoz reports in dirty mode. Rows are
    /// `<name> <path> ··· <status>`, colored, so the escapes come off first
    /// and the path is the field that starts with a slash.
    fn repoz_paths(bin: &Path, root: &Path) -> Vec<PathBuf> {
        let out = Command::new(bin)
            .arg("-d")
            .env("REPO_CHECK_DIR", root)
            // A real config would add the user's own EXTRA_DIRS to the scan.
            .env("XDG_CONFIG_HOME", root.join("no-config"))
            .output()
            .expect("run repoz");
        let stdout = String::from_utf8_lossy(&out.stdout);

        let mut paths: Vec<PathBuf> = Vec::new();
        for line in strip_ansi(&stdout).lines() {
            for word in line.split_whitespace() {
                if word.starts_with('/') && Path::new(word).join(".git").exists() {
                    paths.push(PathBuf::from(word));
                }
            }
        }
        paths.sort();
        paths
    }

    fn strip_ansi(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c != '\u{1b}' {
                out.push(c);
                continue;
            }
            // Consume through the final byte of the CSI sequence.
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        }
        out
    }

    fn fixture(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("fleetz_parity_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // Resolved, because on macOS `temp_dir()` hands back `/var/...` while
        // `/var` is a symlink to `/private/var`. repoz's `find` reports the
        // resolved path and fleetz reports the one it was given, so the two
        // agreed about every repo and still failed on the spelling of the
        // root. Neither tool is wrong; the fixture was handing them different
        // questions.
        std::fs::canonicalize(&base).unwrap_or(base)
    }

    #[test]
    fn discovers_the_same_repos_as_repoz() {
        let Some(bin) = repoz_bin() else {
            eprintln!("SKIP: repoz not found, parity unchecked");
            return;
        };
        let base = fixture("same");

        // The shapes that have actually diverged or plausibly could: a plain
        // repo, a nested one, a dot-prefixed clone (the `~/.dotfiles` case),
        // and a repo inside an otherwise ordinary hidden directory.
        let mut expected = vec![
            dirty_repo(&base, "alpha"),
            dirty_repo(&base, "nested/beta"),
            dirty_repo(&base, ".dotfiles"),
            dirty_repo(&base, ".config/gamma"),
        ];
        // A pruned directory must be invisible to both.
        dirty_repo(&base, "node_modules/should-not-appear");
        expected.sort();

        let theirs = repoz_paths(&bin, &base);
        let mut ours = find_all_local(std::slice::from_ref(&base));
        ours.sort();

        assert_eq!(
            ours, theirs,
            "\nfleetz found: {ours:#?}\nrepoz found:  {theirs:#?}"
        );
        assert_eq!(ours, expected, "neither tool found the expected set");

        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn agrees_with_repoz_on_a_hidden_scan_root() {
        // `repo_dir` or an `extra_dir` pointed straight at a hidden directory:
        // the shape that made `~/.dotfiles` invisible in fleetz.
        let Some(bin) = repoz_bin() else {
            eprintln!("SKIP: repoz not found, parity unchecked");
            return;
        };
        let base = fixture("hidden_root");
        let root = base.join(".dotfiles");
        let expected = vec![dirty_repo(&base, ".dotfiles")];

        let theirs = repoz_paths(&bin, &root);
        let ours = find_all_local(std::slice::from_ref(&root));

        assert_eq!(ours, theirs, "\nfleetz: {ours:#?}\nrepoz:  {theirs:#?}");
        assert_eq!(ours, expected);

        std::fs::remove_dir_all(base).ok();
    }
}
