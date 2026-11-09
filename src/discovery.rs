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
pub const PRUNE_DIRS: [&str; 14] = [
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
mod tests;

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
mod repoz_parity;
