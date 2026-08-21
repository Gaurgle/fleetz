use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// Disk-based cache for GitHub API responses.
/// Stores raw JSON at `~/.cache/fleetz/github.json`.
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

    /// Read cached data even if expired (for offline fallback).
    pub fn read_stale(&self) -> Option<String> {
        std::fs::read_to_string(&self.path).ok()
    }

    /// Write data to cache.
    pub fn write(&self, data: &str) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&self.path, data);
    }
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
}
