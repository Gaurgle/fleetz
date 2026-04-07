use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub work_start: u8,
    pub work_end: u8,
    pub repo_limit: u32,
    pub repo_dir: String,
    pub refresh_interval: u64,
    pub cache_ttl: u64,
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
            groups: HashMap::new(),
        }
    }
}

impl Config {
    /// Load config from `~/.config/epoz/config.toml`, falling back to defaults.
    pub fn load() -> Self {
        let path = config_path();
        if path.exists() {
            Self::load_from(&path).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Load from a specific path. Returns Err if file can't be read or parsed.
    pub fn load_from(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read config: {e}"))?;
        toml::from_str(&content)
            .map_err(|e| format!("failed to parse config: {e}"))
    }

    /// Resolved repo directory with ~ expanded to an absolute path.
    pub fn resolved_repo_dir(&self) -> PathBuf {
        let expanded = shellexpand::tilde(&self.repo_dir);
        PathBuf::from(expanded.as_ref())
    }
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("epoz")
        .join("config.toml")
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
    fn invalid_toml_returns_error() {
        let dir = std::env::temp_dir().join("epoz_test_bad_config");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "not = [valid toml {{").unwrap();
        let result = Config::load_from(&path);
        assert!(result.is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
