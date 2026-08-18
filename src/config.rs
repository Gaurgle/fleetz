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
    /// Whether the contribution strip starts open, and from which source:
    /// "off", "github", or "local". `c` and `C` toggle it live; this is what
    /// a fresh launch starts from.
    #[serde(default)]
    pub contributions: Option<String>,
    /// Git author regex the local contribution source filters by, overriding
    /// each repo's own `user.email`. `--author` takes a regex, so several
    /// identities can be joined: `me@work.com\|me@home.com`.
    #[serde(default)]
    pub contributions_author: Option<String>,
    #[serde(default)]
    pub groups: HashMap<String, Vec<String>>,
    /// Pin the startup view: "detail", "compact", or "auto" (the default,
    /// which picks Compact only on a terminal too small for three panels).
    /// `v` still toggles it for the session.
    #[serde(default)]
    pub view: Option<String>,
    /// Start in watch mode. On by default: a dashboard you leave open should
    /// keep itself current without being asked. `--watch` and `--no-watch`
    /// override it for one session, and `w` toggles it live.
    ///
    /// Note this multiplies with `refresh_interval`: watch sweeps *fetch* from
    /// every remote on that cadence, so a low interval here means real network
    /// traffic from launch.
    #[serde(default = "default_watch")]
    pub watch: bool,
    /// Seconds between GitHub re-polls while watching, which is what keeps CI
    /// status current. Separate from `refresh_interval` because the two have
    /// different costs: a sweep is local git plus a fetch, a poll is one
    /// GraphQL call. Floored by `GITHUB_POLL_MIN` so a typo cannot spend the
    /// rate limit.
    #[serde(default = "default_github_poll_interval")]
    pub github_poll_interval: u64,
    /// Whether pane sizes changed with the resize keys are written back on
    /// exit and restored on the next launch. Off by default: a layout nudged
    /// for one repo shouldn't silently become permanent.
    #[serde(default)]
    pub persist_pane_sizes: bool,
    /// Last saved pane sizes. Only read when `persist_pane_sizes` is set, so
    /// leaving stale values in the file is harmless.
    #[serde(default = "default_repo_list_pct")]
    pub repo_list_pct: u16,
    #[serde(default = "default_inspector_pct")]
    pub inspector_pct: u16,
}

fn default_watch() -> bool {
    true
}

fn default_github_poll_interval() -> u64 {
    30
}

fn default_repo_list_pct() -> u16 {
    crate::app::DEFAULT_REPO_LIST_PCT
}

fn default_inspector_pct() -> u16 {
    crate::app::DEFAULT_INSPECTOR_PCT
}

impl Default for Config {
    fn default() -> Self {
        Self {
            contributions: None,
            contributions_author: None,
            work_start: 9,
            work_end: 18,
            repo_limit: 50,
            repo_dir: "~/repos".to_string(),
            refresh_interval: 60,
            cache_ttl: 120,
            extra_dirs: Vec::new(),
            groups: HashMap::new(),
            view: None,
            watch: default_watch(),
            github_poll_interval: default_github_poll_interval(),
            persist_pane_sizes: false,
            repo_list_pct: default_repo_list_pct(),
            inspector_pct: default_inspector_pct(),
        }
    }
}

/// A value the settings menu writes back into `config.toml`.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Int(i64),
    Str(&'static str),
    Bool(bool),
}

/// The file the settings menu writes to: whichever candidate already exists,
/// else the preferred XDG path, created on first save.
pub fn settings_path() -> PathBuf {
    let candidates = config_candidates();
    candidates
        .iter()
        .find(|p| p.exists())
        .cloned()
        .or_else(|| candidates.first().cloned())
        .unwrap_or_else(|| PathBuf::from("fleetz.toml"))
}

/// Write `updates` into the TOML document at `path`, creating the file and its
/// parent directory if absent.
///
/// Uses `toml_edit` rather than serializing the whole `Config`: the file is
/// hand-written and commented, and a round trip through serde would flatten
/// every comment and reorder every key. Only the named keys are touched;
/// anything else in the file is preserved byte for byte.
pub fn save_settings(path: &Path, updates: &[(&str, SettingValue)]) -> Result<(), String> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = existing
        .parse()
        .map_err(|e| format!("config is not valid TOML: {e}"))?;

    for (key, value) in updates {
        doc[*key] = match value {
            SettingValue::Int(i) => toml_edit::value(*i),
            SettingValue::Str(s) => toml_edit::value(*s),
            SettingValue::Bool(b) => toml_edit::value(*b),
        };
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {parent:?}: {e}"))?;
    }
    std::fs::write(path, doc.to_string()).map_err(|e| format!("cannot write config: {e}"))
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

    /// The view the user pinned in config, if any. Anything unrecognized
    /// (including "auto") means "decide from the terminal size".
    pub fn forced_view(&self) -> Option<crate::app::View> {
        match self.view.as_deref()?.trim().to_lowercase().as_str() {
            "detail" => Some(crate::app::View::Detail),
            "compact" => Some(crate::app::View::Compact),
            _ => None,
        }
    }

    /// The view to start in for a terminal of this size: the pinned one when
    /// the config names it, otherwise whatever fits. Single source of truth
    /// for both the first frame and every later resize.
    pub fn startup_view(&self, width: u16, height: u16) -> crate::app::View {
        self.forced_view()
            .unwrap_or_else(|| crate::app::initial_view(width, height))
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
    fn forced_view_reads_the_config_key() {
        let mut c = Config::default();
        assert_eq!(c.forced_view(), None, "unset means decide by size");

        c.view = Some("detail".to_string());
        assert_eq!(c.forced_view(), Some(crate::app::View::Detail));

        c.view = Some("  COMPACT ".to_string());
        assert_eq!(c.forced_view(), Some(crate::app::View::Compact));

        // "auto" and typos both fall back to size-based selection rather
        // than erroring the whole config.
        c.view = Some("auto".to_string());
        assert_eq!(c.forced_view(), None);
        c.view = Some("detial".to_string());
        assert_eq!(c.forced_view(), None);
    }

    #[test]
    fn pinned_view_beats_a_small_terminal() {
        // The whole point of the key: a narrow window must not override an
        // explicit choice, in either direction.
        let mut c = Config::default();
        assert_eq!(c.startup_view(40, 15), crate::app::View::Compact);

        c.view = Some("detail".to_string());
        assert_eq!(c.startup_view(40, 15), crate::app::View::Detail);

        c.view = Some("compact".to_string());
        assert_eq!(c.startup_view(200, 60), crate::app::View::Compact);
    }

    #[test]
    fn partial_config_keeps_defaults_for_everything_else() {
        // A config naming only `view` must parse: serde(default) on the
        // struct fills the rest, so a one-line file is valid.
        let c: Config = toml::from_str("view = \"detail\"").expect("partial config parses");
        assert_eq!(c.forced_view(), Some(crate::app::View::Detail));
        assert_eq!(c.repo_limit, 50, "untouched keys keep their defaults");
        assert_eq!(c.refresh_interval, 60);
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
    fn save_settings_preserves_comments_and_untouched_keys() {
        // The whole reason for toml_edit over serializing Config: this file is
        // hand-written, and a serde round trip would flatten every comment and
        // drop keys the settings menu doesn't know about.
        let dir = std::env::temp_dir().join("fleetz_test_save_settings");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "# my hand-written note\nrefresh_interval = 60\nrepo_limit = 99\n\n[groups]\nwork = [\"api\"]\n",
        )
        .unwrap();

        save_settings(
            &path,
            &[
                ("refresh_interval", SettingValue::Int(30)),
                ("view", SettingValue::Str("detail")),
            ],
        )
        .unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("# my hand-written note"), "comment kept");
        assert!(written.contains("repo_limit = 99"), "unknown key kept");
        assert!(written.contains("work = [\"api\"]"), "table kept");

        // And it must still parse back into a Config with the new values.
        let c = Config::load_from(&path).unwrap();
        assert_eq!(c.refresh_interval, 30);
        assert_eq!(c.repo_limit, 99);
        assert_eq!(c.forced_view(), Some(crate::app::View::Detail));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn save_settings_creates_a_missing_file_and_its_directory() {
        let dir = std::env::temp_dir().join("fleetz_test_save_fresh");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("config.toml");

        save_settings(&path, &[("refresh_interval", SettingValue::Int(45))]).unwrap();

        let c = Config::load_from(&path).unwrap();
        assert_eq!(c.refresh_interval, 45);
        assert_eq!(c.repo_limit, 50, "unset keys still take their defaults");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn pane_size_keys_default_when_absent() {
        // A config written before pane persistence existed must still load.
        let c: Config = toml::from_str("refresh_interval = 60").unwrap();
        assert!(!c.persist_pane_sizes);
        assert_eq!(c.repo_list_pct, crate::app::DEFAULT_REPO_LIST_PCT);
        assert_eq!(c.inspector_pct, crate::app::DEFAULT_INSPECTOR_PCT);
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

    /// An absent key opens the strip on github. A dashboard whose headline
    /// feature needs a keypress to appear reads as a broken feature.
    #[test]
    fn an_absent_contributions_key_opens_the_strip_on_github() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.contributions, None);
        assert_eq!(
            crate::app::ContributionsMode::from_config(
                cfg.contributions
                    .as_deref()
                    .unwrap_or(crate::app::DEFAULT_CONTRIBUTIONS)
            ),
            crate::app::ContributionsMode::Github
        );
    }

    /// An explicit "off" still closes it: the new default must not override
    /// someone who turned it off on purpose.
    #[test]
    fn an_explicit_off_still_closes_the_strip() {
        let cfg: Config = toml::from_str(r#"contributions = "off""#).unwrap();
        assert_eq!(
            crate::app::ContributionsMode::from_config(
                cfg.contributions
                    .as_deref()
                    .unwrap_or(crate::app::DEFAULT_CONTRIBUTIONS)
            ),
            crate::app::ContributionsMode::Off
        );

        let cfg: Config = toml::from_str(r#"contributions = "local""#).unwrap();
        assert_eq!(
            crate::app::ContributionsMode::from_config(cfg.contributions.as_deref().unwrap()),
            crate::app::ContributionsMode::Local
        );
    }

    /// A typo must not be a crash, and must not silently mean "github".
    #[test]
    fn an_unknown_contributions_value_falls_back_to_off() {
        assert_eq!(
            crate::app::ContributionsMode::from_config("gitub"),
            crate::app::ContributionsMode::Off
        );
    }

    #[test]
    fn contributions_author_is_optional() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.contributions_author, None);
        let cfg: Config = toml::from_str(r#"contributions_author = "a@b\\|c@d""#).unwrap();
        assert_eq!(cfg.contributions_author.as_deref(), Some(r"a@b\|c@d"));
    }
}
