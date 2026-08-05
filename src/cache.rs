use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// Disk-based cache for GitHub API responses.
/// Stores raw JSON at `~/.cache/fleetz/github.json`.
pub struct ApiCache {
    path: PathBuf,
    ttl: Duration,
}

impl ApiCache {
    pub fn new(ttl_secs: u64, repo_limit: u32, include_prs: bool) -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("~/.cache"))
            .join("fleetz");
        // Pre-keying versions wrote a single github.json for every query
        // shape; remove it so it cannot be served for the wrong flags.
        let _ = std::fs::remove_file(cache_dir.join("github.json"));
        let prs = if include_prs { "prs" } else { "noprs" };
        Self {
            path: cache_dir.join(format!("github-{repo_limit}-{prs}.json")),
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
        let a = ApiCache::new(300, 50, false);
        let b = ApiCache::new(300, 50, true);
        let c = ApiCache::new(300, 100, false);
        assert_ne!(a.path, b.path);
        assert_ne!(a.path, c.path);
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
