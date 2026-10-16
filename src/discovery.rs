use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::model::RemoteRef;

/// Result of discovering a local clone of a GitHub repo.
#[derive(Debug, Clone)]
pub struct FoundRepo {
    pub name: String,
    pub path: PathBuf,
    /// The owner of the GitHub repo this clone was matched to, as the fetch
    /// spells it. None when that repo came without an owner (a payload with
    /// no `login`), the legacy case matched by name alone.
    pub owner: Option<String>,
}

impl FoundRepo {
    /// The key of the GitHub repo this clone claimed, as `find_elsewhere`'s
    /// `already_found` expects it: lowercased `owner/name`, or the lowercased
    /// name alone when the repo has no owner.
    pub fn key(&self) -> String {
        repo_key(self.owner.as_deref(), &self.name)
    }
}

/// A clone that claimed no row because its folder name, the only evidence it
/// has, names repos of two or more owners in the full dataset (D4 in
/// `docs/repository-identity-design.md`). Ordered by path first, so a sorted
/// list names the same checkout first on every launch.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ambiguity {
    pub path: PathBuf,
    /// The folder's spelling, leading dot stripped, as a match would report.
    pub name: String,
}

/// Lowercased `owner/name`, or the lowercased name for a repo with no owner.
/// The two cannot collide, since a repo name holds no slash.
fn repo_key(owner: Option<&str>, name: &str) -> String {
    match crate::model::identity_key(owner, name) {
        (Some(owner), name) => format!("{owner}/{name}"),
        (None, name) => name,
    }
}

/// One GitHub repo discovery is looking for, spelled as the fetch reports it.
#[derive(Debug, Clone)]
struct WantedRepo {
    owner: Option<String>,
    name: String,
}

impl WantedRepo {
    fn key(&self) -> String {
        repo_key(self.owner.as_deref(), &self.name)
    }
}

/// The GitHub repos discovery is looking for, indexed both ways a local clone
/// can be recognized: by its `origin` remote, and by its folder name. It also
/// holds what the full dataset knows, since whether a folder name is
/// ambiguous, or an origin names a known repo, must not depend on the time
/// window (M1).
#[derive(Debug, Default)]
pub struct Wanted {
    /// Lowercased `owner/name` to the wanted repo. Only repos the fetch
    /// reported an owner for; the rest can still match by folder name.
    by_remote: HashMap<String, WantedRepo>,
    /// Lowercased name to every wanted repo of that name, owned or not.
    by_name: HashMap<String, Vec<WantedRepo>>,
    /// Full dataset: lowercased name to the lowercased owners that have a
    /// repo of that name. An owner-less entry adds no owner.
    full_owners: HashMap<String, HashSet<String>>,
    /// Full dataset: lowercased `owner/name` of every repo with an owner.
    full_remotes: HashSet<String>,
}

impl Wanted {
    /// Built from `(owner, name)` pairs as the GitHub fetch reports them,
    /// standing for both the wanted repos and the full dataset. Production
    /// passes the two separately through `with_full_dataset`, so this is
    /// built for tests only.
    #[cfg(test)]
    pub fn new<'a>(repos: impl IntoIterator<Item = (Option<&'a str>, &'a str)>) -> Self {
        let repos: Vec<_> = repos.into_iter().collect();
        Self::with_full_dataset(repos.iter().copied(), repos.iter().copied())
    }

    /// `active` is what discovery looks for (the window-filtered repos);
    /// `full` is the whole dataset, which decides whether a folder name is
    /// ambiguous and whether an origin names a repo fleetz knows. `active`
    /// is expected to be a subset of `full`.
    pub fn with_full_dataset<'a>(
        active: impl IntoIterator<Item = (Option<&'a str>, &'a str)>,
        full: impl IntoIterator<Item = (Option<&'a str>, &'a str)>,
    ) -> Self {
        let mut wanted = Self::default();
        for (owner, name) in active {
            let repo = WantedRepo {
                owner: owner.map(str::to_string),
                name: name.to_string(),
            };
            if repo.owner.is_some() {
                wanted.by_remote.insert(repo.key(), repo.clone());
            }
            wanted
                .by_name
                .entry(name.to_lowercase())
                .or_default()
                .push(repo);
        }
        for (owner, name) in full {
            let owners = wanted.full_owners.entry(name.to_lowercase()).or_default();
            if let Some(owner) = owner {
                owners.insert(owner.to_lowercase());
                wanted.full_remotes.insert(repo_key(Some(owner), name));
            }
        }
        wanted
    }

    /// The repos not in `found`, which holds keys from `FoundRepo::key`. The
    /// full dataset is kept whole.
    fn without(&self, found: &HashSet<String>) -> Self {
        Self {
            by_remote: self
                .by_remote
                .iter()
                .filter(|(key, _)| !found.contains(*key))
                .map(|(key, repo)| (key.clone(), repo.clone()))
                .collect(),
            by_name: self
                .by_name
                .iter()
                .filter_map(|(name, repos)| {
                    let left: Vec<WantedRepo> = repos
                        .iter()
                        .filter(|repo| !found.contains(&repo.key()))
                        .cloned()
                        .collect();
                    (!left.is_empty()).then(|| (name.clone(), left))
                })
                .collect(),
            full_owners: self.full_owners.clone(),
            full_remotes: self.full_remotes.clone(),
        }
    }

    fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// What the clone at `path` stands for, by the rules of section 3.3 of
    /// `docs/repository-identity-design.md`:
    ///
    /// 1. Its `origin` names a wanted repo: that repo.
    /// 2. It has no usable `origin` (none, unparseable, or a linked worktree,
    ///    see `origin_ref`): its folder name, when the full dataset has at
    ///    most one owner for that name. Two or more owners make it ambiguous
    ///    and it claims nothing.
    /// 3. Its `origin` parses but names nothing wanted: its folder name, only
    ///    when the origin's owner is the owner of the repo the folder names
    ///    and the origin's repo is not in the full dataset. That keeps a repo
    ///    renamed on GitHub matching from a renamed folder (the old URL keeps
    ///    working through GitHub's redirect), and stops a fork, a transfer or
    ///    a foreign host from claiming a row. A repo without an owner keeps
    ///    the unconditional fallback.
    ///
    /// The remote comes first because a folder name is only a convention: a
    /// clone of `portfolio-site` can sit in `DevOps_Portfolio`.
    fn match_clone(&self, path: &Path) -> Match {
        let origin = origin_ref(path).map(|origin| origin.key());
        if let Some(origin) = &origin
            && let Some(repo) = self
                .by_remote
                .get(&repo_key(Some(&origin.owner), &origin.name))
        {
            return Match::Claimed {
                repo: repo.clone(),
                name: repo.name.clone(),
                rank: MatchRank {
                    by_folder: false,
                    off_github: off_github(Some(origin)),
                },
            };
        }

        let dir_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let Some(name) = matched_name(dir_name, |lower| self.by_name.contains_key(lower)) else {
            return Match::Nothing;
        };
        let lower = name.to_lowercase();
        let candidates = &self.by_name[&lower];

        let claimed = match &origin {
            None => {
                let owners = self.full_owners.get(&lower).map_or(0, HashSet::len);
                if owners > 1 {
                    return Match::Ambiguous(name);
                }
                candidates
                    .iter()
                    .find(|repo| repo.owner.is_some())
                    .or_else(|| candidates.first())
            }
            Some(origin) => {
                let known = self
                    .full_remotes
                    .contains(&repo_key(Some(&origin.owner), &origin.name));
                candidates
                    .iter()
                    .find(|repo| {
                        !known
                            && repo
                                .owner
                                .as_deref()
                                .is_some_and(|owner| owner.eq_ignore_ascii_case(&origin.owner))
                    })
                    .or_else(|| candidates.iter().find(|repo| repo.owner.is_none()))
            }
        };
        match claimed {
            Some(repo) => Match::Claimed {
                repo: repo.clone(),
                name,
                rank: MatchRank {
                    by_folder: true,
                    off_github: off_github(origin.as_ref()),
                },
            },
            None => Match::Nothing,
        }
    }
}

/// What `Wanted::match_clone` made of one clone.
enum Match {
    /// The clone stands for `repo` and reports as `name`: the GitHub name for
    /// a remote match, the folder's spelling for a folder match.
    Claimed {
        repo: WantedRepo,
        name: String,
        rank: MatchRank,
    },
    /// The folder name, which names repos of several owners.
    Ambiguous(String),
    Nothing,
}

/// The first two steps of the primary-checkout order. `false` sorts first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MatchRank {
    /// Matched by its folder name rather than its exact remote.
    by_folder: bool,
    /// Its origin is on a host other than `github.com`, or it has none (o2).
    off_github: bool,
}

/// Whether a clone's origin is somewhere other than `github.com`, or missing
/// (o2). Compared on the lowercased key, since `RemoteRef::parse` keeps the
/// host as the URL spelled it (`GitHub.com` stays `GitHub.com`).
fn off_github(origin: Option<&RemoteRef>) -> bool {
    origin.is_none_or(|o| o.key().host != "github.com")
}

/// Where a candidate stands in the primary-checkout order of section 3.3:
/// exact remote, then an origin on `github.com`, then a `.git` directory over
/// a `.git` file, then an earlier root, then fewer path components, then the
/// path itself. Lower wins, so the result does not depend on the order the
/// filesystem lists directories in.
///
/// Shared with assembly, which orders the LocalOnly rows naming one remote
/// by it (`exact_origin`), so the order has one definition.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CheckoutRank {
    matched: MatchRank,
    git_file: bool,
    root: usize,
    depth: usize,
    path: PathBuf,
}

impl CheckoutRank {
    /// The rank of the clone at `path`, matched as `matched`, found under the
    /// scan root at index `root`.
    fn new(matched: MatchRank, path: &Path, root: usize) -> Self {
        Self {
            matched,
            git_file: !path.join(".git").is_dir(),
            root,
            depth: path.components().count(),
            path: path.to_path_buf(),
        }
    }

    /// The rank of a clone at `path`, under the scan root at index `root`,
    /// that stands for its repo through its exact `origin`.
    pub(crate) fn exact_origin(origin: &RemoteRef, path: &Path, root: usize) -> Self {
        let matched = MatchRank {
            by_folder: false,
            off_github: off_github(Some(origin)),
        };
        Self::new(matched, path, root)
    }
}

/// The best clone per wanted repo, and the ambiguous clones, as a walk finds
/// them.
#[derive(Default)]
struct Collector {
    best: HashMap<String, (CheckoutRank, FoundRepo)>,
    ambiguous: Vec<Ambiguity>,
}

impl Collector {
    fn offer(&mut self, wanted: &Wanted, path: &Path, root: usize) {
        let (repo, name, matched) = match wanted.match_clone(path) {
            Match::Claimed { repo, name, rank } => (repo, name, rank),
            Match::Ambiguous(name) => {
                self.ambiguous.push(Ambiguity {
                    path: path.to_path_buf(),
                    name,
                });
                return;
            }
            Match::Nothing => return,
        };
        let rank = CheckoutRank::new(matched, path, root);
        let found = FoundRepo {
            name,
            path: path.to_path_buf(),
            owner: repo.owner.clone(),
        };
        match self.best.get(&repo.key()) {
            Some((best, _)) if *best <= rank => {}
            _ => {
                self.best.insert(repo.key(), (rank, found));
            }
        }
    }

    /// The winners in root order, then by path, and the ambiguities sorted by
    /// path with the duplicates of overlapping roots removed.
    fn finish(self) -> (Vec<FoundRepo>, Vec<Ambiguity>) {
        let mut winners: Vec<(CheckoutRank, FoundRepo)> = self.best.into_values().collect();
        winners.sort_by(|(a, _), (b, _)| (a.root, &a.path).cmp(&(b.root, &b.path)));
        let mut ambiguous = self.ambiguous;
        ambiguous.sort();
        ambiguous.dedup();
        (
            winners.into_iter().map(|(_, found)| found).collect(),
            ambiguous,
        )
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
/// One clone per wanted repo, chosen by the primary-checkout order
/// (`CheckoutRank`), so overlapping roots can't produce two rows for one repo.
///
/// Production uses `find_in_primary_with_ambiguities`; this wrapper stays for
/// the existing tests.
#[cfg(test)]
pub fn find_in_primary(roots: &[PathBuf], wanted: &Wanted) -> Vec<FoundRepo> {
    find_in_primary_with_ambiguities(roots, wanted).0
}

/// `find_in_primary`, plus the clones that claimed nothing because their
/// folder name is ambiguous, sorted by path.
pub fn find_in_primary_with_ambiguities(
    roots: &[PathBuf],
    wanted: &Wanted,
) -> (Vec<FoundRepo>, Vec<Ambiguity>) {
    let mut collector = Collector::default();
    for (index, root) in roots.iter().enumerate() {
        scan_for_git_repos(root, 3, false, &mut |path| {
            collector.offer(wanted, path, index);
        });
    }
    collector.finish()
}

/// Pass 2: Find repos elsewhere under $HOME (maxdepth 4), excluding
/// Library, .cache, node_modules, .Trash, and every pass 1 scan root (which
/// has already been searched, and searched deeper than a `$HOME` sweep would
/// reach it).
///
/// `already_found` holds the `FoundRepo::key` of every pass 1 match; those
/// repos are not looked for again. One clone per remaining repo, by the same
/// order as pass 1.
///
/// Production uses `find_elsewhere_with_ambiguities`; this wrapper stays for
/// the existing tests.
#[cfg(test)]
pub fn find_elsewhere(
    home: &Path,
    roots: &[PathBuf],
    wanted: &Wanted,
    already_found: &HashSet<String>,
) -> Vec<FoundRepo> {
    find_elsewhere_with_ambiguities(home, roots, wanted, already_found).0
}

/// `find_elsewhere`, plus the clones that claimed nothing because their
/// folder name is ambiguous, sorted by path.
pub fn find_elsewhere_with_ambiguities(
    home: &Path,
    roots: &[PathBuf],
    wanted: &Wanted,
    already_found: &HashSet<String>,
) -> (Vec<FoundRepo>, Vec<Ambiguity>) {
    let remaining = wanted.without(already_found);

    if remaining.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let mut collector = Collector::default();
    scan_for_git_repos(home, 4, true, &mut |path| {
        // Skip excluded directories
        if is_excluded(path, roots) {
            return;
        }
        collector.offer(&remaining, path, 0);
    });
    collector.finish()
}

/// The GitHub repo name a local directory stands for, or None when
/// `is_wanted` accepts neither spelling. `is_wanted` is asked lowercased names.
///
/// A clone can carry a leading dot its GitHub repo doesn't -- the repo
/// `dotfiles` conventionally lives at `~/.dotfiles` -- so the dotted spelling
/// is tried too, mirroring repoz's `-iname "$name" -o -iname ".$name"`. The
/// dot is stripped from the returned name because everything downstream (GitHub
/// match, glyph lookup, dedupe) keys on the GitHub name, not the folder name.
fn matched_name(dir_name: &str, is_wanted: impl Fn(&str) -> bool) -> Option<String> {
    if is_wanted(&dir_name.to_lowercase()) {
        return Some(dir_name.to_string());
    }
    let undotted = dir_name.strip_prefix('.')?;
    is_wanted(&undotted.to_lowercase()).then(|| undotted.to_string())
}

/// What the clone's `origin` remote points at.
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
fn origin_ref(repo: &Path) -> Option<RemoteRef> {
    let config = std::fs::read_to_string(repo.join(".git").join("config")).ok()?;
    RemoteRef::parse(origin_url(&config)?)
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

    // Repository identity scenarios (docs/repository-identity-design.md,
    // section 4). Each fixture lives in its own directory, unique per process.

    fn scenario_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("fleetz_test_fz05b_{tag}_{}", std::process::id()));
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
        let (found, ambiguities) = find_in_primary_with_ambiguities(
            &[first.clone(), second.clone(), first.clone()],
            &wanted,
        );
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
            // As in `repoz_paths`: inherited from a git hook, these would
            // aim the fixture's git calls at the checkout running the tests.
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
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
    ///
    /// Run from the parent of `root`, outside any repository and outside the
    /// scan root, because repoz adds the git toplevel of its working
    /// directory to the scan. Inherited from `cargo test`, that was the fleetz
    /// checkout running the tests, and a dirty one showed up next to the
    /// fixture; under `$HOME` it was hidden only by luck, since repoz prints
    /// such a path as `~/...` and the slash filter below drops it. Run from
    /// `root` itself, the hidden-root test's `root` is a repo, and its
    /// toplevel would give repoz a second route to the fixture, hiding a
    /// repoz that stopped scanning a hidden `REPO_CHECK_DIR`. The parent is
    /// the system temp directory or the plain fixture base, so
    /// `REPO_CHECK_DIR` is repoz's only way in. `GIT_DIR` and `GIT_WORK_TREE`
    /// go too: a git hook running `cargo test` would redirect repoz's git.
    fn repoz_paths(bin: &Path, root: &Path) -> Vec<PathBuf> {
        let out = Command::new(bin)
            .arg("-d")
            .current_dir(root.parent().unwrap_or(root))
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
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
