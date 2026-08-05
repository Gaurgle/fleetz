use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub work_start: u8,
    pub work_end: u8,
    pub repo_limit: u32,
    pub repo_dir: String,
    pub refresh_interval: u64,
    pub cache_ttl: u64,
    /// Extra directories to scan for local repos alongside `repo_dir` --
    /// e.g. `~/.dotfiles`. Mirrors repoz's `EXTRA_DIRS`; lets repos outside
    /// the main repo dir (or under a hidden path the walker skips) surface
    /// under "local changes".
    #[serde(default)]
    pub extra_dirs: Vec<String>,
    #[serde(default)]
    pub groups: HashMap<String, Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            work_start: 9,
            work_end: 18,
            repo_limit: 50,
            repo_dir: "~/repos".to_string(),
            refresh_interval: 300,
            cache_ttl: 120,
            extra_dirs: Vec::new(),
            groups: HashMap::new(),
        }
    }
}

impl Config {
    /// Load config from the first candidate path that exists, else defaults.
    /// XDG `~/.config/fleetz/` is checked before the platform config dir so
    /// macOS behaves like repoz (whose config lives in `~/.config/repoz/`)
    /// rather than burying the file under `~/Library/Application Support`.
    /// The legacy `epoz` name (pre-rename) is checked last in both roots.
    pub fn load() -> Self {
        for path in config_candidates() {
            if path.exists() {
                return Self::load_from(&path).unwrap_or_default();
            }
        }
        Self::default()
    }

    /// Load from a specific path. Returns Err if file can't be read or parsed.
    pub fn load_from(path: &Path) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("failed to read config: {e}"))?;
        toml::from_str(&content).map_err(|e| format!("failed to parse config: {e}"))
    }

    /// Resolved repo directory with ~ expanded to an absolute path.
    pub fn resolved_repo_dir(&self) -> PathBuf {
        let expanded = shellexpand::tilde(&self.repo_dir);
        PathBuf::from(expanded.as_ref())
    }

    /// Extra scan directories with `~` expanded, dropping any that don't
    /// exist -- so a stale config entry silently no-ops rather than erroring
    /// the whole discovery pass (matching repoz's `[ -d "$_d" ]` guard).
    pub fn resolved_extra_dirs(&self) -> Vec<PathBuf> {
        self.extra_dirs
            .iter()
            .map(|d| PathBuf::from(shellexpand::tilde(d).as_ref()))
            .filter(|p| p.is_dir())
            .collect()
    }
}

/// Candidate config paths in priority order: XDG `~/.config/<app>/` first
/// (repoz parity), then the platform config dir, each for `fleetz` then the
/// legacy `epoz` name.
fn config_candidates() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".config"));
    }
    if let Some(cfg) = dirs::config_dir() {
        // Avoid a duplicate probe when the platform dir *is* ~/.config (Linux).
        if !roots.contains(&cfg) {
            roots.push(cfg);
        }
    }
    let mut paths = Vec::new();
    for root in &roots {
        for app in ["fleetz", "epoz"] {
            paths.push(root.join(app).join("config.toml"));
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sane_values() {
        let c = Config::default();
        assert_eq!(c.work_start, 9);
        assert_eq!(c.work_end, 18);
        assert_eq!(c.repo_limit, 50);
        assert_eq!(c.repo_dir, "~/repos");
    }

    #[test]
    fn parses_minimal_toml() {
        let toml = r#"
            work_start = 8
            repo_dir = "~/code"
        "#;
        let c: Config = toml::from_str(toml).unwrap();
        assert_eq!(c.work_start, 8);
        assert_eq!(c.repo_dir, "~/code");
        // defaults for unspecified fields
        assert_eq!(c.work_end, 18);
        assert_eq!(c.repo_limit, 50);
    }

    #[test]
    fn parses_groups() {
        let toml = r#"
            [groups]
            work = ["api", "infra"]
            personal = ["dotfiles"]
        "#;
        let c: Config = toml::from_str(toml).unwrap();
        assert_eq!(c.groups["work"], vec!["api", "infra"]);
        assert_eq!(c.groups["personal"], vec!["dotfiles"]);
    }

    #[test]
    fn resolved_repo_dir_expands_tilde() {
        let c = Config::default();
        let resolved = c.resolved_repo_dir();
        assert!(!resolved.to_string_lossy().contains('~'));
        assert!(resolved.to_string_lossy().contains("repos"));
    }

    #[test]
    fn parses_extra_dirs() {
        let toml = r#"
            extra_dirs = ["~/.dotfiles", "~/work"]
        "#;
        let c: Config = toml::from_str(toml).unwrap();
        assert_eq!(c.extra_dirs, vec!["~/.dotfiles", "~/work"]);
    }

    #[test]
    fn resolved_extra_dirs_expands_tilde_and_drops_missing() {
        let dir = std::env::temp_dir().join("fleetz_test_extra_dirs");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let c = Config {
            extra_dirs: vec![
                dir.to_string_lossy().to_string(),
                "/definitely/not/a/real/path".to_string(),
            ],
            ..Config::default()
        };
        let resolved = c.resolved_extra_dirs();
        // Missing dir dropped, existing one kept.
        assert_eq!(resolved, vec![dir.clone()]);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn resolved_extra_dirs_defaults_empty() {
        assert!(Config::default().resolved_extra_dirs().is_empty());
    }

    #[test]
    fn invalid_toml_returns_error() {
        let dir = std::env::temp_dir().join("fleetz_test_bad_config");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "not = [valid toml {{").unwrap();
        let result = Config::load_from(&path);
        assert!(result.is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
