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
    ttl: Duration,
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
    pub fn new(ttl_secs: u64, repo_limit: u32) -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("~/.cache"))
            .join("fleetz");
        remove_obsolete_names(&cache_dir, repo_limit);
        Self {
            path: cache_dir.join(legacy_name(repo_limit)),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// A cache at an explicit path, for the keyed files of `KeyedCache`.
    fn located(path: PathBuf, ttl_secs: u64) -> Self {
        Self {
            path,
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// The file this cache reads and writes.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Test constructor with an explicit path, so a test can plant a payload
    /// of a chosen age without going near the real cache directory.
    #[cfg(test)]
    pub fn at(path: PathBuf, ttl_secs: u64) -> Self {
        Self {
            path,
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// Read cached data if it exists and is within TTL.
    pub fn read(&self) -> Option<String> {
        let metadata = std::fs::metadata(&self.path).ok()?;
        let modified = metadata.modified().ok()?;
        let age = SystemTime::now().duration_since(modified).ok()?;

        if age > self.ttl {
            return None;
        }

        std::fs::read_to_string(&self.path).ok()
    }

    /// Read cached data even if expired (for offline fallback), with the time
    /// the file was written.
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
    format!("h_{:016x}", crate::github::fnv1a_64(lower.bytes()))
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
    ttl_secs: u64,
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
    pub fn new(ttl_secs: u64, repo_limit: u32, host: &str) -> Self {
        let legacy = ApiCache::new(ttl_secs, repo_limit);
        let dir = legacy
            .path
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf);
        Self {
            dir,
            cleanable: cache_root().is_some(),
            host: host_part(host),
            repo_limit,
            ttl_secs,
            legacy,
        }
    }

    /// A cache in an explicit directory, so a test can plant files without
    /// going near the real one.
    #[cfg(test)]
    pub fn at(dir: PathBuf, ttl_secs: u64, repo_limit: u32, host: &str) -> Self {
        let legacy = ApiCache::located(dir.join(legacy_name(repo_limit)), ttl_secs);
        Self {
            dir,
            cleanable: true,
            host: host_part(host),
            repo_limit,
            ttl_secs,
            legacy,
        }
    }

    /// The current version's file for `account`, a `token_fingerprint`.
    pub fn for_account(&self, account: &str) -> ApiCache {
        debug_assert!(is_lower_hex_16(account), "not a fingerprint");
        let name = keyed_name(FORMAT_VERSION, &self.host, account, self.repo_limit);
        ApiCache::located(self.dir.join(name), self.ttl_secs)
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
                        cache: ApiCache::located(path.clone(), self.ttl_secs),
                        own: false,
                    });
                }
            }
        }
        if keyed.is_empty() {
            reads.push(LaunchRead {
                cache: ApiCache::located(self.legacy.path.clone(), self.ttl_secs),
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
mod tests {
    use super::*;

    #[test]
    fn cache_path_varies_by_query_shape() {
        // `repo_limit` is the only thing left that changes what is asked for,
        // so it is the only thing the key may vary by. This used to also split
        // on `-p`, back when that flag decided whether PR data was fetched;
        // it now decides only whether PR data is drawn. Keeping the split
        // meant startup and the background poll wrote two files and each read
        // only its own, so a launch could never be warmed by the poll before
        // it.
        assert_ne!(ApiCache::new(300, 50).path, ApiCache::new(300, 100).path);
        assert_eq!(ApiCache::new(300, 50).path, ApiCache::new(999, 50).path);
    }

    #[test]
    fn cache_write_and_read() {
        let dir = std::env::temp_dir().join("fleetz_cache_test");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = ApiCache {
            path: dir.join("test.json"),
            ttl: Duration::from_secs(300),
        };

        cache.write(r#"{"test": true}"#);
        let data = cache.read();
        assert!(data.is_some());
        assert!(data.unwrap().contains("test"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn cache_expired_returns_none() {
        let dir = std::env::temp_dir().join("fleetz_cache_expired");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = ApiCache {
            path: dir.join("test.json"),
            ttl: Duration::from_secs(0), // immediate expiry
        };

        cache.write("data");
        // TTL is 0, so read should return None
        let data = cache.read();
        assert!(data.is_none());

        // But stale read should still work
        let stale = cache.read_stale();
        assert!(stale.is_some());

        std::fs::remove_dir_all(dir).ok();
    }

    /// The stale read is what a fallback is built from, and the age it
    /// reports has to be the file's, not the moment it was read: a cache
    /// written hours ago is hours old however recently fleetz looked at it.
    #[test]
    fn a_stale_read_reports_when_the_file_was_written() {
        let dir = std::env::temp_dir().join(format!("fleetz_cache_written_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = ApiCache {
            path: dir.join("test.json"),
            ttl: Duration::from_secs(0),
        };
        cache.write("data");
        // Planted, because a test cannot wait three hours for a file to age.
        let planted = SystemTime::now() - Duration::from_secs(3 * 3600);
        std::fs::File::options()
            .write(true)
            .open(&cache.path)
            .unwrap()
            .set_modified(planted)
            .unwrap();

        let stale = cache.read_stale().expect("the file is there");
        assert_eq!(stale.body, "data");
        let written = stale.written.expect("the write time is readable");
        // Filesystems round mtimes, so compare to the second.
        let drift = match written.duration_since(planted) {
            Ok(d) => d,
            Err(e) => e.duration(),
        };
        assert!(
            drift < Duration::from_secs(1),
            "read {written:?}, planted {planted:?}"
        );

        std::fs::remove_dir_all(dir).ok();
    }

    /// A fresh directory of its own under the system temp dir, removed when
    /// dropped, so a failing assertion cleans up too. On Unix the drop first
    /// gives the owner its permissions back, which CW6 takes away.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("fleetz_{tag}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        /// Every name in the directory, sorted, so a test can assert that no
        /// temporary file was left beside the cache file.
        fn names(&self) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(&self.0)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
            }
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// CW3: the last write wins whole, and the file's mtime is that write's,
    /// since the mtime is the data time the stale read reports. The first
    /// write is aged three hours so a second write that kept the old mtime
    /// cannot pass.
    #[test]
    fn cw3_two_writes_in_sequence_leave_the_second_body_and_its_mtime() {
        let scratch = Scratch::new("cw3");
        let cache = ApiCache::at(scratch.0.join("github-50.json"), 0);
        cache.write("first body, the longer of the two");
        let aged = SystemTime::now() - Duration::from_secs(3 * 3600);
        std::fs::File::options()
            .write(true)
            .open(&cache.path)
            .unwrap()
            .set_modified(aged)
            .unwrap();

        let before = SystemTime::now();
        cache.write("second");
        let after = SystemTime::now();

        assert_eq!(std::fs::read_to_string(&cache.path).unwrap(), "second");
        let written = cache.written().expect("the write time is readable");
        // Filesystems round mtimes, so allow a second either side.
        assert!(
            written + Duration::from_secs(1) >= before && written <= after + Duration::from_secs(1),
            "mtime {written:?} is not the second write's ({before:?} to {after:?})"
        );
        assert_eq!(scratch.names(), ["github-50.json"]);
    }

    /// CW5: a prefix left by a torn write is replaced by the full body, byte
    /// for byte, and nothing else is left in the directory.
    #[test]
    fn cw5_a_full_write_replaces_a_truncated_file_byte_for_byte() {
        let scratch = Scratch::new("cw5");
        let path = scratch.0.join("github-50.json");
        let full: String = (0..400)
            .map(|i| format!(r#"{{"name":"repo-{i}","stars":{i}}},"#))
            .collect();
        assert!(full.len() > 1000);
        std::fs::write(&path, &full.as_bytes()[..1000]).unwrap();

        ApiCache::at(path.clone(), 86_400).write(&full);

        assert_eq!(std::fs::read(&path).unwrap(), full.as_bytes());
        assert_eq!(scratch.names(), ["github-50.json"]);
    }

    /// CW6: a directory the process cannot create files in. The old cache
    /// must survive whole and the write must neither panic nor leave
    /// anything behind. A plain `fs::write` truncated and rewrote the
    /// existing file here, since writing to a file needs no directory
    /// permission.
    #[cfg(unix)]
    #[test]
    fn cw6_a_write_into_a_read_only_directory_leaves_the_old_file() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("cw6");
        let path = scratch.0.join("github-50.json");
        std::fs::write(&path, "old body").unwrap();
        std::fs::set_permissions(&scratch.0, std::fs::Permissions::from_mode(0o555)).unwrap();
        // Root ignores directory permissions, so the scenario cannot be set
        // up; say so rather than pass without having tested anything.
        let probe = scratch.0.join("probe");
        if std::fs::File::create(&probe).is_ok() {
            let _ = std::fs::remove_file(&probe);
            eprintln!("SKIPPED cw6: the directory is writable anyway (running as root?)");
            return;
        }

        ApiCache::at(path.clone(), 86_400).write("new body");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old body");
        assert_eq!(scratch.names(), ["github-50.json"]);
    }

    /// CW8: the saved copy is readable only by its owner (O14). Masked by
    /// the umask like any created file, so it holds for every umask that
    /// leaves the owner's read and write bits alone.
    #[cfg(unix)]
    #[test]
    fn cw8_a_write_into_an_empty_directory_gives_mode_0600() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("cw8");
        let path = scratch.0.join("github-50.json");

        ApiCache::at(path.clone(), 86_400).write("body");

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "mode was {mode:o}");
    }

    /// The rename puts a new inode in place, so a cache file saved before
    /// O14 at 0644 is 0600 after its next write rather than keeping its mode.
    #[cfg(unix)]
    #[test]
    fn a_replaced_0644_cache_file_ends_up_0600() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("mode_replace");
        let path = scratch.0.join("github-50.json");
        std::fs::write(&path, "old body").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        ApiCache::at(path.clone(), 86_400).write("new body");

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "mode was {mode:o}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new body");
    }

    /// CW1: the rename fails after the new body is fully written. The old
    /// body stays and the temporary file is removed. The temporary file is
    /// beside the target and named as FZ-09c's cleanup expects.
    #[test]
    fn cw1_a_failed_rename_leaves_the_old_body_and_no_temporary_file() {
        let scratch = Scratch::new("cw1");
        let target = scratch.0.join("github-50.json");
        std::fs::write(&target, "body W").unwrap();
        let temp = temp_path_for(&target).unwrap();
        let prefix = format!(".github-50.json.tmp-{}-", std::process::id());
        let temp_name = temp.file_name().unwrap().to_string_lossy().into_owned();
        assert!(temp_name.starts_with(&prefix), "temporary name {temp_name}");
        assert_eq!(temp.parent(), target.parent());

        let mut renamed_from = None;
        let result = replace_atomically(
            &target,
            &temp,
            |file| file.write_all(b"body X"),
            |from, _| {
                assert_eq!(std::fs::read_to_string(from).unwrap(), "body X");
                renamed_from = Some(from.to_path_buf());
                Err(io::Error::other("injected rename failure"))
            },
        );

        assert!(result.is_err());
        assert_eq!(renamed_from.as_deref(), Some(temp.as_path()));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "body W");
        assert_eq!(scratch.names(), ["github-50.json"]);
    }

    /// The write itself fails part-way (a full disk): the half-written
    /// temporary file is removed, the rename never runs, the old body stays.
    #[test]
    fn a_write_that_fails_part_way_leaves_the_old_body_and_no_temporary_file() {
        let scratch = Scratch::new("partial");
        let target = scratch.0.join("github-50.json");
        std::fs::write(&target, "body W").unwrap();
        let temp = temp_path_for(&target).unwrap();

        let result = replace_atomically(
            &target,
            &temp,
            |file| {
                file.write_all(b"half of X")?;
                Err(io::Error::other("injected write failure"))
            },
            |_, _| panic!("a failed write must not be renamed into place"),
        );

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "body W");
        assert_eq!(scratch.names(), ["github-50.json"]);
    }

    /// A file already at the temporary name (another process's, or a
    /// crash's) is neither reused nor deleted: `create_new` refuses it and
    /// the write is skipped with the old body in place.
    #[test]
    fn a_taken_temporary_name_skips_the_write_and_keeps_both_files() {
        let scratch = Scratch::new("taken");
        let target = scratch.0.join("github-50.json");
        std::fs::write(&target, "body W").unwrap();
        let temp = temp_path_for(&target).unwrap();
        std::fs::write(&temp, "someone else's").unwrap();

        let result = replace_atomically(
            &target,
            &temp,
            |_| panic!("a taken name must not be written"),
            |_, _| panic!("a taken name must not be renamed"),
        );

        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "body W");
        assert_eq!(std::fs::read_to_string(&temp).unwrap(), "someone else's");
    }

    /// CW7: `R` and a background poll write at once in one process. Writer 1
    /// fills its temporary file; writer 2 then creates its own and renames;
    /// writer 1 renames last. Built through the rename seam, not threads, so
    /// the interleaving is the one named. With a shared name writer 2 would
    /// have clobbered or renamed away writer 1's file.
    #[test]
    fn cw7_two_interleaved_writes_in_one_process_leave_writer_ones_body() {
        let scratch = Scratch::new("cw7");
        let target = scratch.0.join("github-50.json");
        let body_1 = "writer 1's complete body, longer than writer 2's";
        let body_2 = "writer 2's body";
        let temp_1 = temp_path_for(&target).unwrap();
        let temp_2 = temp_path_for(&target).unwrap();
        assert_ne!(temp_1, temp_2, "two writes shared a temporary name");

        let result = replace_atomically(
            &target,
            &temp_1,
            |file| file.write_all(body_1.as_bytes()),
            |from, to| {
                replace_atomically(
                    &target,
                    &temp_2,
                    |file| file.write_all(body_2.as_bytes()),
                    |from, to| std::fs::rename(from, to),
                )?;
                assert_eq!(std::fs::read_to_string(&target).unwrap(), body_2);
                assert_eq!(std::fs::read_to_string(from).unwrap(), body_1);
                std::fs::rename(from, to)
            },
        );

        assert!(result.is_ok());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), body_1);
        assert_eq!(scratch.names(), ["github-50.json"]);
    }

    // --- FZ-09c: the keyed file, the launch read order and cleanup ---

    /// Dummy tokens. A is the design's example token, B the same shape.
    const TOKEN_A: &str = "gho_EXAMPLEexampleEXAMPLEexample0000";
    const TOKEN_B: &str = "gho_EXAMPLEexampleEXAMPLEexample1111";
    const DAY: Duration = Duration::from_secs(24 * 3600);

    fn account(token: &str) -> String {
        crate::github::token_fingerprint(token)
    }

    /// Writes `body` at `path` and dates it `age` ago.
    fn plant(path: &Path, body: &str, age: Duration) {
        std::fs::write(path, body).unwrap();
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() - age)
            .unwrap();
    }

    fn keyed_at(scratch: &Scratch, version: u32, account: &str, repo_limit: u32) -> PathBuf {
        scratch
            .0
            .join(keyed_name(version, "github.com", account, repo_limit))
    }

    fn file_name(path: &Path) -> String {
        path.file_name().unwrap().to_string_lossy().into_owned()
    }

    /// CK1: two tokens, one `repo_limit`, two files; writing B's leaves A's
    /// byte for byte.
    #[test]
    fn ck1_two_tokens_get_two_files_and_writing_one_leaves_the_other() {
        let scratch = Scratch::new("ck1");
        let cache = KeyedCache::at(scratch.0.clone(), 86_400, 50, "github.com");
        let file_a = cache.for_account(&account(TOKEN_A));
        let file_b = cache.for_account(&account(TOKEN_B));
        assert_ne!(file_a.path(), file_b.path());

        file_a.write("account A's body");
        let before = std::fs::read(file_a.path()).unwrap();
        file_b.write("account B's body, longer");

        assert_eq!(std::fs::read(file_a.path()).unwrap(), before);
        assert_eq!(
            std::fs::read_to_string(file_b.path()).unwrap(),
            "account B's body, longer"
        );
    }

    /// CK3: one token, `repo_limit` 50 and 100, two files.
    #[test]
    fn ck3_one_token_two_repo_limits_get_two_files() {
        let dir = std::env::temp_dir().join("fleetz_ck3_unused");
        let at_50 = KeyedCache::at(dir.clone(), 86_400, 50, "github.com");
        let at_100 = KeyedCache::at(dir, 86_400, 100, "github.com");
        let a = account(TOKEN_A);
        assert_ne!(at_50.for_account(&a).path(), at_100.for_account(&a).path());
        assert!(file_name(at_50.for_account(&a).path()).ends_with("-50.json"));
        assert!(file_name(at_100.for_account(&a).path()).ends_with("-100.json"));
    }

    /// CK5: the host part is the host key `RemoteRef` compares a github.com
    /// remote by, whatever case the remote was written in.
    #[test]
    fn ck5_the_host_part_is_the_remote_ref_host_key() {
        for url in [
            "git@github.com:Gaurgle/fleetz.git",
            "https://GitHub.com/Gaurgle/fleetz",
        ] {
            let remote = crate::model::RemoteRef::parse(url).unwrap();
            assert_eq!(host_part(&remote.host), remote.key().host, "{url}");
        }
        assert_eq!(host_part(crate::github::HOST), "github.com");
    }

    /// A host that is not plain `[a-z0-9.-]` is hashed, never written raw:
    /// nothing it holds reaches the file name.
    #[test]
    fn a_host_with_other_characters_is_hashed() {
        for host in ["git hub.com", "evil/../x", "a_b.com", "", "[::1]"] {
            let part = host_part(host);
            assert!(part.starts_with("h_"), "{host:?} gave {part:?}");
            assert!(is_lower_hex_16(&part[2..]), "{host:?} gave {part:?}");
            assert!(is_host_part(&part));
        }
        assert_ne!(host_part("a_b.com"), host_part("a_c.com"));
        // Case folds before hashing, as it does for a plain host.
        assert_eq!(host_part("A_B.com"), host_part("a_b.com"));
        assert_eq!(host_part("ghe.corp-1.example"), "ghe.corp-1.example");
    }

    /// CK6: the name carries the pinned fingerprint, bare or padded token, and
    /// the empty token's; the design's example name exactly.
    #[test]
    fn ck6_the_file_name_carries_the_pinned_fingerprint() {
        let cache = KeyedCache::at(PathBuf::from("/nonexistent"), 0, 50, "github.com");
        let named = |token: &str| file_name(cache.for_account(&account(token)).path());
        assert_eq!(
            named(TOKEN_A),
            "github-v2-github.com-2a8d4ab865f3ca20-50.json"
        );
        assert_eq!(
            named(&format!("  {TOKEN_A}\n")),
            "github-v2-github.com-2a8d4ab865f3ca20-50.json"
        );
        assert_eq!(named(""), "github-v2-github.com-5fccb9a3d0a45785-50.json");
    }

    /// A keyed name parses back into its parts, a host with dashes included,
    /// and near misses do not parse.
    #[test]
    fn keyed_names_parse_exactly() {
        let a = account(TOKEN_A);
        let name = keyed_name(2, "ghe-1.example", &a, 50);
        assert_eq!(
            parse_keyed_name(&name),
            Some(KeyedName {
                version: 2,
                host: "ghe-1.example",
                account: &a,
                repo_limit: 50
            })
        );
        for near_miss in [
            format!("github-v2-github.com-{a}-50.json.bak"),
            format!("github-v2-github.com-{a}-+50.json"),
            format!("github-v-github.com-{a}-50.json"),
            "github-v2-github.com-2A8D4AB865F3CA20-50.json".to_string(),
            "github-v2-github.com-2a8d4ab865f3ca2-50.json".to_string(),
            format!("github-v2-Git_Hub-{a}-50.json"),
            format!("github-v2-{a}-50.json"),
        ] {
            assert_eq!(parse_keyed_name(&near_miss), None, "{near_miss}");
        }
    }

    /// CK9: `ApiCache::new`'s deletions are today's three obsolete names and
    /// nothing else. Driven through the helper it calls, in a scratch
    /// directory, since `new` itself reaches the real one.
    #[test]
    fn ck9_api_cache_new_removes_only_the_three_obsolete_names() {
        let scratch = Scratch::new("ck9");
        let a = account(TOKEN_A);
        let kept = [
            "github-50.json".to_string(),
            "github-100-prs.json".to_string(),
            keyed_name(FORMAT_VERSION, "github.com", &a, 50),
            keyed_name(1, "github.com", &a, 50),
            ".github-50.json.tmp-1-1".to_string(),
            "window.toml".to_string(),
        ];
        for name in kept.iter().map(String::as_str).chain([
            "github.json",
            "github-50-prs.json",
            "github-50-noprs.json",
        ]) {
            plant(&scratch.0.join(name), "x", 40 * DAY);
        }

        remove_obsolete_names(&scratch.0, 50);

        let mut expected = kept.to_vec();
        expected.sort();
        assert_eq!(scratch.names(), expected);
    }

    /// What `launch_reads` offers, as file names and whether each is the
    /// credential's own.
    fn offered(cache: &KeyedCache, account: Option<&str>) -> Vec<(String, bool)> {
        cache
            .launch_reads(account)
            .into_iter()
            .map(|read| (file_name(read.cache.path()), read.own))
            .collect()
    }

    /// Step 1, then step 2 only while no account has a current keyed file.
    #[test]
    fn launch_offers_the_own_file_then_the_legacy_one_only_while_nothing_is_keyed() {
        let scratch = Scratch::new("reads_known");
        let cache = KeyedCache::at(scratch.0.clone(), 86_400, 50, "github.com");
        let (a, b) = (account(TOKEN_A), account(TOKEN_B));
        let own = keyed_name(FORMAT_VERSION, "github.com", &a, 50);
        plant(&scratch.0.join("github-50.json"), "legacy", DAY);

        // Nothing keyed: own, then legacy.
        assert_eq!(
            offered(&cache, Some(&a)),
            [(own.clone(), true), ("github-50.json".to_string(), false)]
        );
        // An older version, another limit or another host is not "keyed" here.
        plant(&keyed_at(&scratch, 1, &b, 50), "v1", DAY);
        plant(&keyed_at(&scratch, FORMAT_VERSION, &b, 100), "100", DAY);
        plant(
            &scratch
                .0
                .join(keyed_name(FORMAT_VERSION, "ghe.example", &b, 50)),
            "ghe",
            DAY,
        );
        assert_eq!(offered(&cache, Some(&a)).len(), 2);
        // Another account's current file ends the legacy step.
        plant(&keyed_at(&scratch, FORMAT_VERSION, &b, 50), "b", DAY);
        assert_eq!(offered(&cache, Some(&a)), [(own, true)]);
    }

    /// Signed out: the newest current keyed file of any account, else the
    /// legacy file, never both.
    #[test]
    fn launch_signed_out_offers_the_newest_keyed_file_or_else_the_legacy_one() {
        let scratch = Scratch::new("reads_unknown");
        let cache = KeyedCache::at(scratch.0.clone(), 86_400, 50, "github.com");
        let (a, b) = (account(TOKEN_A), account(TOKEN_B));
        assert_eq!(
            offered(&cache, None),
            [("github-50.json".to_string(), false)]
        );

        plant(&keyed_at(&scratch, FORMAT_VERSION, &a, 50), "a", 2 * DAY);
        plant(&keyed_at(&scratch, FORMAT_VERSION, &b, 50), "b", DAY);
        plant(&scratch.0.join("github-50.json"), "legacy", Duration::ZERO);
        assert_eq!(
            offered(&cache, None),
            [(keyed_name(FORMAT_VERSION, "github.com", &b, 50), false)]
        );
    }

    /// Every kind of superseded file, each past 30 days, beside the current
    /// account's current file: the positive control the rule tests below
    /// each break one condition of.
    fn plant_superseded(scratch: &Scratch) -> (PathBuf, Vec<PathBuf>) {
        let (a, b) = (account(TOKEN_A), account(TOKEN_B));
        let current = keyed_at(scratch, FORMAT_VERSION, &a, 50);
        plant(&current, "mine", Duration::ZERO);
        let superseded = vec![
            scratch.0.join("github-50.json"),
            keyed_at(scratch, 1, &a, 50),
            keyed_at(scratch, FORMAT_VERSION, &b, 50),
        ];
        for path in &superseded {
            plant(path, "old", 40 * DAY);
        }
        (current, superseded)
    }

    fn remove_now(dir: Option<&Path>, account: Option<&str>, served: Option<&Path>) {
        remove_superseded_in(dir, "github.com", account, served, SystemTime::now());
    }

    #[test]
    fn superseded_files_go_when_every_rule_holds() {
        let scratch = Scratch::new("sup_all");
        let (current, _) = plant_superseded(&scratch);
        remove_now(Some(&scratch.0), Some(&account(TOKEN_A)), None);
        assert_eq!(scratch.names(), [file_name(&current)]);
    }

    /// CM7: signed out, or the lookup timed out: no account, nothing goes.
    #[test]
    fn cm7_without_a_known_account_nothing_is_deleted() {
        let scratch = Scratch::new("cm7");
        plant_superseded(&scratch);
        let before = scratch.names();
        remove_now(Some(&scratch.0), None, None);
        assert_eq!(scratch.names(), before);
    }

    /// CM8: the file this launch served stays, even matching every other rule.
    #[test]
    fn cm8_the_file_just_served_is_not_deleted() {
        let scratch = Scratch::new("cm8");
        let (current, superseded) = plant_superseded(&scratch);
        let served = &superseded[0];
        remove_now(Some(&scratch.0), Some(&account(TOKEN_A)), Some(served));
        let mut expected = vec![file_name(&current), file_name(served)];
        expected.sort();
        assert_eq!(scratch.names(), expected);
    }

    /// CM9: names outside the explicit patterns are never touched, however
    /// old: `window.toml` lives here.
    #[test]
    fn cm9_unrelated_files_are_never_deleted() {
        let scratch = Scratch::new("cm9");
        let a = account(TOKEN_A);
        plant(
            &keyed_at(&scratch, FORMAT_VERSION, &a, 50),
            "mine",
            Duration::ZERO,
        );
        for name in [
            "window.toml",
            "notes.json",
            "github-50.json.bak",
            ".github-50.json.tmp-1-1",
        ] {
            plant(&scratch.0.join(name), "keep", 40 * DAY);
        }
        let before = scratch.names();
        remove_now(Some(&scratch.0), Some(&a), None);
        assert_eq!(scratch.names(), before);
    }

    /// A newer binary's version, another host's file and the account's own
    /// current file are not superseded either.
    #[test]
    fn newer_versions_other_hosts_and_the_own_file_stay() {
        let scratch = Scratch::new("sup_newer");
        let (a, b) = (account(TOKEN_A), account(TOKEN_B));
        plant(
            &keyed_at(&scratch, FORMAT_VERSION, &a, 50),
            "mine",
            40 * DAY,
        );
        plant(
            &keyed_at(&scratch, FORMAT_VERSION + 1, &b, 50),
            "v3",
            40 * DAY,
        );
        plant(
            &scratch.0.join(keyed_name(1, "ghe.example", &a, 50)),
            "ghe",
            40 * DAY,
        );
        let before = scratch.names();
        remove_now(Some(&scratch.0), Some(&a), None);
        assert_eq!(scratch.names(), before);
    }

    /// CM10: no platform cache directory, no cleanup.
    #[test]
    fn cm10_without_a_cache_directory_cleanup_is_skipped() {
        let scratch = Scratch::new("cm10");
        plant_superseded(&scratch);
        let before = scratch.names();
        remove_now(None, Some(&account(TOKEN_A)), None);
        assert_eq!(scratch.names(), before);
    }

    /// A superseded file written within 30 days stays; at 30 days it goes.
    #[test]
    fn a_superseded_file_written_within_30_days_stays() {
        let scratch = Scratch::new("sup_age");
        let a = account(TOKEN_A);
        let current = keyed_at(&scratch, FORMAT_VERSION, &a, 50);
        plant(&current, "mine", Duration::ZERO);
        let recent = keyed_at(&scratch, 1, &a, 50);
        plant(&recent, "29 days", 29 * DAY);
        let legacy = scratch.0.join("github-50.json");
        plant(&legacy, "30 days", 30 * DAY);

        remove_now(Some(&scratch.0), Some(&a), None);

        let mut expected = vec![file_name(&current), file_name(&recent)];
        expected.sort();
        assert_eq!(scratch.names(), expected);
    }

    /// The current account's current file must exist for the superseded
    /// file's own `repo_limit`, not just for some other one.
    #[test]
    fn a_superseded_file_stays_until_its_limit_has_a_current_file() {
        let scratch = Scratch::new("sup_limit");
        let a = account(TOKEN_A);
        let current_100 = keyed_at(&scratch, FORMAT_VERSION, &a, 100);
        plant(&current_100, "mine at 100", Duration::ZERO);
        let legacy_50 = scratch.0.join("github-50.json");
        plant(&legacy_50, "old", 40 * DAY);
        plant(&scratch.0.join("github-100.json"), "old", 40 * DAY);

        remove_now(Some(&scratch.0), Some(&a), None);

        let mut expected = vec![file_name(&current_100), file_name(&legacy_50)];
        expected.sort();
        assert_eq!(scratch.names(), expected);
    }

    /// CW2: a crash's temporary file over an hour old goes; a 10 minute old
    /// one, possibly a live write, stays, and so does anything not named as
    /// a cache file's temporary.
    #[test]
    fn cw2_startup_removes_temporary_files_older_than_an_hour() {
        let scratch = Scratch::new("cw2");
        let a = account(TOKEN_A);
        let abandoned = format!(".{}.tmp-4242-7", keyed_name(2, "github.com", &a, 50));
        plant(
            &scratch.0.join(&abandoned),
            "half",
            Duration::from_secs(2 * 3600),
        );
        plant(
            &scratch.0.join(".github-50.json.tmp-4242-8"),
            "live",
            Duration::from_secs(600),
        );
        for name in [
            ".window.toml.tmp-1-1",
            ".github-50.json.tmp-x-1",
            ".github-50.json.tmp-1",
            "github-50.json",
            "notes.json",
        ] {
            plant(&scratch.0.join(name), "keep", Duration::from_secs(2 * 3600));
        }
        let mut expected: Vec<String> = scratch
            .names()
            .into_iter()
            .filter(|name| *name != abandoned)
            .collect();
        expected.sort();

        remove_abandoned_temp_files_in(Some(&scratch.0), SystemTime::now());

        assert_eq!(scratch.names(), expected);
        remove_abandoned_temp_files_in(None, SystemTime::now());
    }

    /// A temporary file `write` itself would leave is the pattern startup
    /// looks for.
    #[test]
    fn the_temporary_names_writes_use_are_the_ones_cleanup_matches() {
        let cache = KeyedCache::at(PathBuf::from("/nonexistent"), 0, 50, "github.com");
        let target = cache.for_account(&account(TOKEN_A)).path().to_path_buf();
        let temp = temp_path_for(&target).unwrap();
        assert!(is_cache_temp_name(&file_name(&temp)));
        assert!(is_cache_temp_name(&file_name(
            &temp_path_for(Path::new("/x/github-50.json")).unwrap()
        )));
    }
}
