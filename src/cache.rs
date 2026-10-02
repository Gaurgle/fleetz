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
        // Pre-keying versions wrote a single github.json for every query
        // shape, and the split-by-PR versions wrote two; remove both so
        // neither can be served for a query shape that no longer exists.
        let _ = std::fs::remove_file(cache_dir.join("github.json"));
        for stale in ["prs", "noprs"] {
            let _ =
                std::fs::remove_file(cache_dir.join(format!("github-{repo_limit}-{stale}.json")));
        }
        Self {
            path: cache_dir.join(format!("github-{repo_limit}.json")),
            ttl: Duration::from_secs(ttl_secs),
        }
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
}
