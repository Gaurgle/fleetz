use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

/// Numbers every temporary file this process creates, so two writes in one
/// process (`R` and a background poll) never share a name.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A cached payload and the time its file was written, `None` when unknown.
pub struct CachedBody {
    pub body: String,
    pub written: Option<SystemTime>,
}

/// Disk-based cache for GitHub API responses.
///
/// Stores the raw response body at `github-{repo_limit}.json` in the `fleetz`
/// directory under `dirs::cache_dir()` (`~/Library/Caches/fleetz` on macOS,
/// `$XDG_CACHE_HOME/fleetz` or `~/.cache/fleetz` on Linux). Without a cache
/// directory it falls back to the relative path `~/.cache/fleetz`, which is
/// not tilde-expanded and so lands under the current directory.
pub struct ApiCache {
    path: PathBuf,
}

impl ApiCache {
    /// Keyed by `repo_limit` alone.
    ///
    /// It used to be keyed by `-p` as well, from when that flag decided
    /// whether PR data was fetched. It now decides only whether PR data is
    /// *drawn*, so the query is identical either way -- but the key was never
    /// updated, and the two halves of the app disagreed about it: startup
    /// passed `args.prs` (false by default) while the background poll
    /// hardcoded true. They wrote two files and each only ever read its own,
    /// so the poll's fresh payload could not warm the next launch.
    pub fn new(repo_limit: u32) -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("~/.cache"))
            .join("fleetz");
        remove_obsolete_names(&cache_dir, repo_limit);
        Self {
            path: cache_dir.join(legacy_name(repo_limit)),
        }
    }

    /// A cache at an explicit path, for the keyed files of `KeyedCache`.
    fn located(path: PathBuf) -> Self {
        Self { path }
    }

    /// The file this cache reads and writes.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Test constructor with an explicit path, so a test can plant a payload
    /// of a chosen age without going near the real cache directory.
    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// The body alone, kept so the existing tests' assertions stay untouched.
    #[cfg(test)]
    pub fn read(&self) -> Option<String> {
        self.read_stale().map(|cached| cached.body)
    }

    /// Read cached data however old, with the time the file was written.
    /// Launch paints from it and the offline fallback is built from it.
    ///
    /// The write time is the payload's data time: only an accepted GitHub
    /// answer is ever written (`github::accept_response`), so the file is as
    /// old as the data in it. `None` when the filesystem cannot say, which the
    /// caller must treat as unknown rather than as now.
    pub fn read_stale(&self) -> Option<CachedBody> {
        let body = std::fs::read_to_string(&self.path).ok()?;
        Some(CachedBody {
            body,
            written: self.written(),
        })
    }

    /// When the cache file was last written, if the filesystem can say.
    pub fn written(&self) -> Option<SystemTime> {
        std::fs::metadata(&self.path).ok()?.modified().ok()
    }

    /// Write data to cache, replacing the file atomically.
    ///
    /// The body goes to a temporary file of its own in the same directory,
    /// which is then renamed over the cache file. A reader, in this process
    /// or another instance, sees the old body or the new one, never a prefix,
    /// and the renamed file's mtime is this write's time (the data time).
    /// Errors are ignored: a cache write must never fail a poll, and a failed
    /// one leaves the old file as it was.
    pub fn write(&self, data: &str) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let Some(temp) = temp_path_for(&self.path) else {
            return;
        };
        let _ = replace_atomically(
            &self.path,
            &temp,
            |file| file.write_all(data.as_bytes()),
            |from, to| std::fs::rename(from, to),
        );
    }
}

/// Pre-keying versions wrote a single github.json for every query shape, and
/// the split-by-PR versions wrote two; remove both so neither can be served
/// for a query shape that no longer exists. These three names are the only
/// deletion `ApiCache::new` makes, and it makes them on every call, so no
/// other may join them: `cargo test` reaches the real cache directory through
/// it (design 2.2).
fn remove_obsolete_names(cache_dir: &Path, repo_limit: u32) {
    let _ = std::fs::remove_file(cache_dir.join("github.json"));
    for stale in ["prs", "noprs"] {
        let _ = std::fs::remove_file(cache_dir.join(format!("github-{repo_limit}-{stale}.json")));
    }
}

/// The format version of the keyed cache file name. Bumped only when an
/// older payload would be misread, not when a field is added (serde defaults
/// and `usable` cover additions); a bump costs one cold launch, since launch
/// never reads an older version. The unkeyed `github-{n}.json` is version 1
/// by implication.
pub const FORMAT_VERSION: u32 = 2;

/// How long a superseded cache file must have gone unwritten before launch
/// deletes it (C4). An installed older binary, or an account still in use,
/// keeps its file alive by writing it.
const SUPERSEDED_AFTER: Duration = Duration::from_secs(30 * 24 * 3600);

/// How old a temporary file must be before startup takes it for a crash's
/// leftover. A live write lasts milliseconds.
const ABANDONED_TEMP_AFTER: Duration = Duration::from_secs(3600);

/// The unkeyed name every version before FZ-09c wrote.
fn legacy_name(repo_limit: u32) -> String {
    format!("github-{repo_limit}.json")
}

/// The keyed name: format version, host, account and `repo_limit`, for
/// example `github-v2-github.com-2a8d4ab865f3ca20-50.json`.
fn keyed_name(version: u32, host: &str, account: &str, repo_limit: u32) -> String {
    format!("github-v{version}-{host}-{account}-{repo_limit}.json")
}

/// FNV-1a, 64 bits: offset basis `0xcbf29ce484222325`, prime
/// `0x100000001b3`, wrapping multiplication.
pub(crate) fn fnv1a_64(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The host as a cache file name spells it: `model::host_key`, the same
/// function `model::RemoteRef::key` compares remotes by (`github.com`). A host that
/// is not plain `[a-z0-9.-]` is written as `h_` and a hash of the lowercased
/// host instead, so no host string can put a separator, a path or anything
/// else surprising into a file name. A plain host never contains `_`, so the
/// two forms cannot collide.
pub fn host_part(host: &str) -> String {
    let lower = crate::model::host_key(host);
    if is_plain_host(&lower) {
        return lower;
    }
    format!("h_{:016x}", fnv1a_64(lower.bytes()))
}

fn is_plain_host(host: &str) -> bool {
    !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

/// Whether `host` is something `host_part` can produce.
fn is_host_part(host: &str) -> bool {
    is_plain_host(host) || host.strip_prefix("h_").is_some_and(is_lower_hex_16)
}

fn is_lower_hex_16(text: &str) -> bool {
    text.len() == 16
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Decimal digits only, so `+5` or an empty string never reads as a number.
fn parse_digits(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// The parts of a keyed cache file name.
#[derive(Debug, PartialEq, Eq)]
struct KeyedName<'a> {
    version: u32,
    host: &'a str,
    account: &'a str,
    repo_limit: u32,
}

/// Parses `github-v{k}-{host}-{fingerprint}-{n}.json` exactly: a decimal
/// version, a host `host_part` can produce, a 16 lowercase hex digit
/// fingerprint and a decimal `repo_limit`. The host may hold `-`, so the name
/// is split from the right, where the fingerprint and the limit hold none.
fn parse_keyed_name(name: &str) -> Option<KeyedName<'_>> {
    let rest = name.strip_prefix("github-v")?.strip_suffix(".json")?;
    let (version, rest) = rest.split_once('-')?;
    let (rest, repo_limit) = rest.rsplit_once('-')?;
    let (host, account) = rest.rsplit_once('-')?;
    if !is_host_part(host) || !is_lower_hex_16(account) {
        return None;
    }
    Some(KeyedName {
        version: parse_digits(version)?,
        host,
        account,
        repo_limit: parse_digits(repo_limit)?,
    })
}

/// The `repo_limit` of a legacy name, `github-{n}.json`.
fn parse_legacy_name(name: &str) -> Option<u32> {
    parse_digits(name.strip_prefix("github-")?.strip_suffix(".json")?)
}

/// Whether `name` is a temporary file `temp_path_for` makes for a cache file,
/// legacy or keyed: `.{cache file name}.tmp-{pid}-{n}`.
fn is_cache_temp_name(name: &str) -> bool {
    let Some((target, suffix)) = name
        .strip_prefix('.')
        .and_then(|rest| rest.rsplit_once(".tmp-"))
    else {
        return false;
    };
    let is_counter = suffix
        .split_once('-')
        .is_some_and(|(pid, n)| parse_digits(pid).is_some() && parse_digits(n).is_some());
    is_counter && (parse_legacy_name(target).is_some() || parse_keyed_name(target).is_some())
}

/// Whether the regular file at `path` was last written at least `age` before
/// `now`. A symlink, a directory, an unreadable mtime or one in the future is
/// never old enough: when in doubt, keep.
fn unwritten_for(path: &Path, age: Duration, now: SystemTime) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    metadata
        .modified()
        .ok()
        .and_then(|written| now.duration_since(written).ok())
        .is_some_and(|unwritten| unwritten >= age)
}

/// The fleetz cache directory, `None` when the platform has no cache
/// directory: cleanup never runs under `ApiCache::new`'s relative fallback.
fn cache_root() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("fleetz"))
}

/// The GitHub cache, one file per format version, host, account and
/// `repo_limit` (`docs/coverage-cache-design.md` 4.3 and 4.5).
///
/// The account is `github::token_fingerprint` of the token the attempt uses,
/// never the token. The unkeyed legacy file is only ever read at launch, and
/// only while no account has a keyed file for this host and `repo_limit`;
/// writes go only to the current version's keyed file.
pub struct KeyedCache {
    dir: PathBuf,
    /// Whether `dir` came from `dirs::cache_dir()`. Cleanup is skipped
    /// otherwise, since the fallback is a relative path.
    cleanable: bool,
    /// Already through `host_part`.
    host: String,
    repo_limit: u32,
    legacy: ApiCache,
}

/// One file launch may serve from, in the order it is tried.
pub struct LaunchRead {
    pub cache: ApiCache,
    /// This credential's own keyed file, so the load's account is known. The
    /// legacy file and the signed-out rule's newest file belong to no known
    /// account.
    pub own: bool,
}

impl KeyedCache {
    /// The production cache under `dirs::cache_dir()`. `ApiCache::new` names
    /// the legacy file, and with it the directory.
    pub fn new(repo_limit: u32, host: &str) -> Self {
        let legacy = ApiCache::new(repo_limit);
        let dir = legacy
            .path
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf);
        Self {
            dir,
            cleanable: cache_root().is_some(),
            host: host_part(host),
            repo_limit,
            legacy,
        }
    }

    /// A cache in an explicit directory, so a test can plant files without
    /// going near the real one.
    #[cfg(test)]
    pub fn at(dir: PathBuf, repo_limit: u32, host: &str) -> Self {
        let legacy = ApiCache::located(dir.join(legacy_name(repo_limit)));
        Self {
            dir,
            cleanable: true,
            host: host_part(host),
            repo_limit,
            legacy,
        }
    }

    /// The current version's file for `account`, a `token_fingerprint`.
    pub fn for_account(&self, account: &str) -> ApiCache {
        debug_assert!(is_lower_hex_16(account), "not a fingerprint");
        let name = keyed_name(FORMAT_VERSION, &self.host, account, self.repo_limit);
        ApiCache::located(self.dir.join(name))
    }

    /// The files launch may serve, in order (design 4.5):
    ///
    /// 1. this credential's current keyed file, when the account is known;
    ///    signed out, or after a lookup that timed out, the newest current
    ///    keyed file of any account instead (O7);
    /// 2. the legacy file, only when no current keyed file of any account
    ///    exists for this host and `repo_limit`.
    ///
    /// There is no step for an older keyed version: a bump means an older
    /// payload would be misread.
    pub fn launch_reads(&self, account: Option<&str>) -> Vec<LaunchRead> {
        let keyed = self.current_keyed_files();
        let mut reads = Vec::new();
        match account {
            Some(account) => reads.push(LaunchRead {
                cache: self.for_account(account),
                own: true,
            }),
            None => {
                if let Some((path, _)) = keyed
                    .iter()
                    .max_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)))
                {
                    reads.push(LaunchRead {
                        cache: ApiCache::located(path.clone()),
                        own: false,
                    });
                }
            }
        }
        if keyed.is_empty() {
            reads.push(LaunchRead {
                cache: ApiCache::located(self.legacy.path.clone()),
                own: false,
            });
        }
        reads
    }

    /// Every current-version keyed file for this host and `repo_limit`, any
    /// account, with its write time.
    fn current_keyed_files(&self) -> Vec<(PathBuf, Option<SystemTime>)> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|entry| {
                entry.file_type().is_ok_and(|kind| kind.is_file())
                    && entry
                        .file_name()
                        .to_str()
                        .and_then(parse_keyed_name)
                        .is_some_and(|name| {
                            name.version == FORMAT_VERSION
                                && name.host == self.host
                                && name.repo_limit == self.repo_limit
                        })
            })
            .map(|entry| {
                let written = entry.metadata().ok().and_then(|m| m.modified().ok());
                (entry.path(), written)
            })
            .collect()
    }

    /// Launch's cleanup of superseded files; see `remove_superseded_in`.
    pub fn remove_superseded(&self, account: Option<&str>, served: Option<&Path>) {
        let dir = self.cleanable.then_some(self.dir.as_path());
        remove_superseded_in(dir, &self.host, account, served, SystemTime::now());
    }
}

/// Deletes superseded cache files from `dir` (C4, design 4.5). A file goes
/// only when every rule holds:
///
/// - its name is `github-{n}.json`, or `github-v{k}-{host}-{fingerprint}-{n}.json`
///   for this host with `k` older than `FORMAT_VERSION`, or the current `k`
///   and another account's fingerprint. Nothing else is touched;
///   `window.toml` lives in the same directory;
/// - it has gone unwritten for 30 days;
/// - the current account is known and its current-version file for the same
///   `n` exists, so a signed-out or timed-out launch deletes nothing;
/// - it is not `served`, the file this launch painted from;
/// - `dir` is `Some`: there is no cleanup under the relative fallback path.
///
/// Called at launch only, never on a failure path. Errors are ignored.
fn remove_superseded_in(
    dir: Option<&Path>,
    host: &str,
    account: Option<&str>,
    served: Option<&Path>,
    now: SystemTime,
) {
    let (Some(dir), Some(account)) = (dir, account) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Some(repo_limit) = superseded_limit(name, host, account) else {
            continue;
        };
        let path = entry.path();
        let current = dir.join(keyed_name(FORMAT_VERSION, host, account, repo_limit));
        if served == Some(path.as_path())
            || !current.is_file()
            || !unwritten_for(&path, SUPERSEDED_AFTER, now)
        {
            continue;
        }
        let _ = std::fs::remove_file(&path);
    }
}

/// The `repo_limit` of a file name the current account's file supersedes,
/// `None` for any other name.
fn superseded_limit(name: &str, host: &str, account: &str) -> Option<u32> {
    if let Some(repo_limit) = parse_legacy_name(name) {
        return Some(repo_limit);
    }
    let keyed = parse_keyed_name(name)?;
    let older = keyed.version < FORMAT_VERSION;
    let other_account = keyed.version == FORMAT_VERSION && keyed.account != account;
    (keyed.host == host && (older || other_account)).then_some(keyed.repo_limit)
}

/// Startup's removal of temporary files a crash left between write and
/// rename, from the production startup path only (never `ApiCache::new`,
/// which tests reach). Skipped without a platform cache directory.
pub fn remove_abandoned_temp_files() {
    remove_abandoned_temp_files_in(cache_root().as_deref(), SystemTime::now());
}

/// Removes every file in `dir` named as `temp_path_for` names a cache file's
/// temporary and unwritten for an hour. Nothing else is touched.
fn remove_abandoned_temp_files_in(dir: Option<&Path>, now: SystemTime) {
    let Some(dir) = dir else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let is_temp = entry.file_name().to_str().is_some_and(is_cache_temp_name);
        let path = entry.path();
        if is_temp && unwritten_for(&path, ABANDONED_TEMP_AFTER, now) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// A temporary name beside `target` that no other write in this process
/// uses: `.{file name}.tmp-{pid}-{n}`. Startup cleanup (FZ-09c) matches this
/// pattern to remove files a crash left behind. `None` when the target has no
/// file name.
fn temp_path_for(target: &Path) -> Option<PathBuf> {
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut name = OsString::from(".");
    name.push(target.file_name()?);
    name.push(format!(".tmp-{}-{n}", std::process::id()));
    Some(target.with_file_name(name))
}

/// Creates `temp`, fills it, and moves it over `target` with `rename`.
///
/// `fill` and `rename` are parameters so a test can fail either step or run
/// another write in between; production passes `write_all` and
/// `fs::rename`. `create_new` refuses a name that already exists, so a stale
/// file someone else left there is never reused or deleted: the write is
/// skipped. Once `temp` is ours, any failure removes it, and `target` is
/// untouched until the rename succeeds. No `fsync`: the cache is a
/// convenience, and a power loss costs one cold launch (design 4.4).
fn replace_atomically(
    target: &Path,
    temp: &Path,
    fill: impl FnOnce(&mut File) -> io::Result<()>,
    rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let mut options = File::options();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(temp)?;
    let filled = fill(&mut file);
    // Closed before the rename, which Windows needs.
    drop(file);
    let result = filled.and_then(|()| rename(temp, target));
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
mod tests;
