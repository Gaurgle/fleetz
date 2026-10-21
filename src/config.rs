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
    /// Extra directories to scan for local repos alongside `repo_dir` --
    /// e.g. `~/.dotfiles`. Mirrors repoz's `EXTRA_DIRS`; lets repos outside
    /// the main repo dir (or under a hidden path the walker skips) surface
    /// under "local changes".
    #[serde(default)]
    pub extra_dirs: Vec<String>,
    /// Which source the contribution strip starts on: "github" or "local",
    /// or "off" to start it folded. `c` switches source and `5` folds it
    /// live; this is what a fresh launch starts from.
    #[serde(default)]
    pub contributions: Option<String>,
    /// Whether the CI pane (pane 4) starts unfolded. On by default, so a
    /// first launch opens every pane; `false` starts it folded to its title
    /// row, where it polls nothing. `4` folds and unfolds it live. Anything
    /// but a boolean keeps the default.
    #[serde(default = "default_ci_pane", deserialize_with = "lenient_ci_pane")]
    pub ci_pane: bool,
    /// Git author regex the local contribution source filters by, overriding
    /// each repo's own `user.email`. `--author` takes a regex, so several
    /// identities can be joined: `me@work.com\|me@home.com`.
    #[serde(default)]
    pub contributions_author: Option<String>,
    /// Which Nerd Font square the strip draws a day as: "small", "medium"
    /// (the default) or "large". Three icons of measured size; see
    /// `app::CellSize`.
    #[serde(default)]
    pub contributions_cell_size: Option<String>,
    /// Which glyphs mark a repo's kind, its stashes and the header's watch
    /// state: "nerd" (the default) draws Nerd Font icons, "plain" draws ASCII
    /// characters any monospace font has. An unknown value keeps the default.
    /// See `app::Icons`.
    #[serde(default)]
    pub icons: Option<String>,
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
    /// Whether the first load of a launch is spent on the loading screen (a
    /// field of scrambled characters with `LOADING` in the middle) rather than
    /// on the empty dashboard. On by default, and one key to retire: an
    /// animation you meet several times a day should be trivial to turn off.
    /// The key keeps its name from the intro wave it used to also switch.
    #[serde(default = "default_intro")]
    pub intro: bool,
    /// The most an automatic fetch may download, in MB (FZ-35b): launch, the
    /// watch sweep, a moved remote, the recovery refetch and `F`. A fetch
    /// past it is cut and its row waits for `f`, which fetches uncapped. `0`
    /// is no cap. Anything but a whole number from 0 up keeps the default.
    #[serde(
        default = "default_fetch_max_mb",
        deserialize_with = "lenient_fetch_max_mb"
    )]
    pub fetch_max_mb: u64,
}

fn default_fetch_max_mb() -> u64 {
    200
}

/// `fetch_max_mb` as written, or the default for a value that is not a whole
/// number from 0 up, so a typo there does not throw away the whole file.
fn lenient_fetch_max_mb<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    Ok(match toml::Value::deserialize(d)? {
        toml::Value::Integer(mb) => u64::try_from(mb).unwrap_or_else(|_| default_fetch_max_mb()),
        _ => default_fetch_max_mb(),
    })
}

fn default_ci_pane() -> bool {
    true
}

/// `ci_pane` as written, or the default for a value that is not a boolean,
/// so a typo there does not throw away the whole file.
fn lenient_ci_pane<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    Ok(match toml::Value::deserialize(d)? {
        toml::Value::Boolean(open) => open,
        _ => default_ci_pane(),
    })
}

fn default_watch() -> bool {
    true
}

fn default_intro() -> bool {
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
            ci_pane: default_ci_pane(),
            contributions_author: None,
            contributions_cell_size: None,
            icons: None,
            work_start: 9,
            work_end: 18,
            repo_limit: 50,
            repo_dir: "~/repos".to_string(),
            refresh_interval: 60,
            extra_dirs: Vec::new(),
            groups: HashMap::new(),
            view: None,
            watch: default_watch(),
            github_poll_interval: default_github_poll_interval(),
            persist_pane_sizes: false,
            repo_list_pct: default_repo_list_pct(),
            inspector_pct: default_inspector_pct(),
            intro: default_intro(),
            fetch_max_mb: default_fetch_max_mb(),
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
        Self::load_from_candidates(&config_candidates())
    }

    /// The first candidate that exists wins; a winner that fails to load
    /// yields defaults rather than falling through. No candidate means defaults.
    fn load_from_candidates(candidates: &[PathBuf]) -> Self {
        for path in candidates {
            if path.exists() {
                return Self::load_from(path).unwrap_or_default();
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

    /// The automatic-fetch cap in bytes (MiB, git's unit), `None` when
    /// `fetch_max_mb` is 0.
    pub fn fetch_cap_bytes(&self) -> Option<u64> {
        (self.fetch_max_mb > 0).then(|| self.fetch_max_mb.saturating_mul(1024 * 1024))
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

    /// #77 removed the expiry key. A file written before that still sets it
    /// and must keep loading: unknown keys are ignored, so adding
    /// `deny_unknown_fields` would break every such config.
    #[test]
    fn a_config_still_setting_the_removed_expiry_key_parses() {
        let c: Config = toml::from_str("cache_ttl = 120\nrepo_limit = 70")
            .expect("a config with the removed key still parses");
        assert_eq!(c.repo_limit, 70, "the keys beside it are still read");
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

    /// FZ-35b, criterion 1: 200 MB unless set, `0` is no cap.
    #[test]
    fn fetch_max_mb_defaults_to_200_and_zero_means_no_cap() {
        let c = Config::default();
        assert_eq!(c.fetch_max_mb, 200);
        assert_eq!(c.fetch_cap_bytes(), Some(200 * 1024 * 1024));
        let unset: Config = toml::from_str("repo_limit = 70").unwrap();
        assert_eq!(unset.fetch_max_mb, 200);
        let set: Config = toml::from_str("fetch_max_mb = 50").unwrap();
        assert_eq!(set.fetch_cap_bytes(), Some(50 * 1024 * 1024));
        let off: Config = toml::from_str("fetch_max_mb = 0").unwrap();
        assert_eq!(off.fetch_cap_bytes(), None);
    }

    /// A bad value keeps the default and the rest of the file.
    #[test]
    fn a_bad_fetch_max_mb_keeps_the_default_and_the_other_keys() {
        for bad in ["-5", "\"big\"", "1.5", "true"] {
            let c: Config = toml::from_str(&format!("fetch_max_mb = {bad}\nrepo_limit = 70"))
                .unwrap_or_else(|e| panic!("{bad} broke the file: {e}"));
            assert_eq!(c.fetch_max_mb, 200, "{bad}");
            assert_eq!(c.repo_limit, 70, "{bad}");
        }
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

    /// `load_from` is the strict path: a missing file is an error naming the
    /// read failure, not silent defaults (only `load` falls back to those).
    #[test]
    fn load_from_a_missing_file_reports_the_read_failure() {
        let dir = std::env::temp_dir().join("fleetz_test_missing_config");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");

        let err = Config::load_from(&path).unwrap_err();

        assert!(
            err.starts_with("failed to read config:"),
            "a read failure, not a parse failure: {err}"
        );
        assert!(!path.exists(), "loading must not create the file");
        std::fs::remove_dir_all(dir).ok();
    }

    /// A missing parent directory fails the same way and is not created.
    #[test]
    fn load_from_a_missing_directory_reports_the_read_failure() {
        let dir = std::env::temp_dir().join("fleetz_test_missing_config_dir");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("config.toml");

        let err = Config::load_from(&path).unwrap_err();

        assert!(err.starts_with("failed to read config:"), "{err}");
        assert!(!dir.exists(), "loading must not create the directory");
    }

    /// Fresh empty directory for the `load_from_candidates` tests.
    fn candidate_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn load_with_no_candidate_file_returns_defaults_and_creates_nothing() {
        let dir = candidate_dir("fleetz_test_no_candidates");
        let first = dir.join("fleetz").join("config.toml");
        let second = dir.join("epoz").join("config.toml");

        let c = Config::load_from_candidates(&[first.clone(), second.clone()]);

        assert_eq!(format!("{c:?}"), format!("{:?}", Config::default()));
        assert!(!first.exists() && !second.exists());
        assert!(!dir.join("fleetz").exists() && !dir.join("epoz").exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn load_falls_through_a_missing_first_candidate_to_the_second() {
        let dir = candidate_dir("fleetz_test_second_candidate");
        let first = dir.join("missing.toml");
        let second = dir.join("second.toml");
        std::fs::write(&second, "refresh_interval = 77").unwrap();

        let c = Config::load_from_candidates(&[first.clone(), second]);

        assert_eq!(c.refresh_interval, 77);
        assert!(!first.exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn load_prefers_the_first_candidate_when_both_exist() {
        let dir = candidate_dir("fleetz_test_first_wins");
        let first = dir.join("first.toml");
        let second = dir.join("second.toml");
        std::fs::write(&first, "refresh_interval = 11").unwrap();
        std::fs::write(&second, "refresh_interval = 22").unwrap();

        let c = Config::load_from_candidates(&[first, second]);

        assert_eq!(c.refresh_interval, 11);
        std::fs::remove_dir_all(dir).ok();
    }

    /// Pins CURRENT behavior, not a rule being endorsed: an existing but
    /// invalid config is swallowed and defaults are returned, without falling
    /// through to a later valid candidate. Change this test if that is fixed.
    #[test]
    fn current_behavior_an_invalid_first_candidate_yields_defaults_silently() {
        let dir = candidate_dir("fleetz_test_invalid_candidate");
        let first = dir.join("first.toml");
        let second = dir.join("second.toml");
        std::fs::write(&first, "not = [valid toml {{").unwrap();
        std::fs::write(&second, "refresh_interval = 22").unwrap();

        let c = Config::load_from_candidates(&[first, second]);

        assert_eq!(format!("{c:?}"), format!("{:?}", Config::default()));
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

    /// An explicit "off" still parses as off, which starts the strip folded:
    /// the new default must not override someone who turned it off on
    /// purpose.
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

    /// The `icons` key: absent means the Nerd Font set, "plain" selects the
    /// plain set, and a typo keeps the default rather than failing the whole
    /// config. Fails if the key stops parsing, the default changes, or an
    /// unknown value picks `plain`.
    #[test]
    fn the_icons_key_defaults_to_nerd_and_falls_back_on_a_typo() {
        let icons = |toml: &str| {
            let cfg: Config = toml::from_str(toml).expect("config parses");
            cfg.icons
                .as_deref()
                .map(crate::app::Icons::from_config)
                .unwrap_or_default()
        };
        assert_eq!(Config::default().icons, None);
        assert_eq!(icons(""), crate::app::Icons::Nerd);
        assert_eq!(icons(r#"icons = "nerd""#), crate::app::Icons::Nerd);
        assert_eq!(icons(r#"icons = "plain""#), crate::app::Icons::Plain);
        assert_eq!(icons(r#"icons = "plian""#), crate::app::Icons::Nerd);
    }

    /// FZ-30c: a missing config or key opens the CI pane, `false` starts it
    /// folded, and a typo keeps the default rather than failing the file.
    #[test]
    fn the_ci_pane_key_defaults_to_open_and_is_lenient() {
        let ci_pane = |toml: &str| {
            let cfg: Config = toml::from_str(toml).expect("config parses");
            cfg.ci_pane
        };
        assert!(Config::default().ci_pane, "no config file opens it");
        assert!(ci_pane(""), "an absent key opens it");
        assert!(ci_pane("ci_pane = true"));
        assert!(!ci_pane("ci_pane = false"));
        assert!(ci_pane(r#"ci_pane = "no""#), "a typo keeps the default");
        let cfg: Config = toml::from_str("ci_pane = 0\nrefresh_interval = 7").unwrap();
        assert!(cfg.ci_pane);
        assert_eq!(cfg.refresh_interval, 7, "the rest of the file still reads");
    }

    #[test]
    fn contributions_author_is_optional() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.contributions_author, None);
        let cfg: Config = toml::from_str(r#"contributions_author = "a@b\\|c@d""#).unwrap();
        assert_eq!(cfg.contributions_author.as_deref(), Some(r"a@b\|c@d"));
    }
}
