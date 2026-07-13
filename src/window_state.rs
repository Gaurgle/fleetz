use std::path::PathBuf;

use crate::time::TimeWindow;

/// Persists the last applied time window so the next plain `fleetz` launch
/// reuses it ("remember last choice"). Lives next to the API cache rather
/// than the config file: it's machine-local state, not user configuration.
pub struct WindowState {
    path: PathBuf,
}

impl WindowState {
    pub fn new() -> Self {
        let dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("~/.cache"))
            .join("fleetz");
        Self { path: dir.join("window.toml") }
    }

    /// Test constructor with an explicit path.
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// Read the saved window. Missing or corrupt files fall back to None
    /// (caller uses the default); `n` is re-clamped in case the file was
    /// hand-edited.
    pub fn load(&self) -> Option<TimeWindow> {
        let content = std::fs::read_to_string(&self.path).ok()?;
        let w: TimeWindow = toml::from_str(&content).ok()?;
        Some(TimeWindow::clamped(w.n, w.unit))
    }

    /// Write the window. Errors are ignored on purpose: losing persistence
    /// must never break the running TUI.
    pub fn save(&self, window: &TimeWindow) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(s) = toml::to_string(window) {
            let _ = std::fs::write(&self.path, s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::WindowUnit;

    fn temp_state(name: &str) -> WindowState {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        WindowState::at(dir.join("window.toml"))
    }

    #[test]
    fn save_and_load_roundtrip() {
        let state = temp_state("fleetz_window_roundtrip");
        let w = TimeWindow { n: 2, unit: WindowUnit::Weeks };
        state.save(&w);
        assert_eq!(state.load(), Some(w));
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("fleetz_window_roundtrip"));
    }

    #[test]
    fn load_missing_file_is_none() {
        let state = temp_state("fleetz_window_missing");
        assert_eq!(state.load(), None);
    }

    #[test]
    fn load_corrupt_file_is_none() {
        let state = temp_state("fleetz_window_corrupt");
        state.save(&TimeWindow::default());
        std::fs::write(
            std::env::temp_dir().join("fleetz_window_corrupt").join("window.toml"),
            "not = [valid {{",
        )
        .unwrap();
        assert_eq!(state.load(), None);
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("fleetz_window_corrupt"));
    }

    #[test]
    fn load_clamps_hand_edited_n() {
        let state = temp_state("fleetz_window_clamp");
        std::fs::create_dir_all(std::env::temp_dir().join("fleetz_window_clamp")).unwrap();
        std::fs::write(
            std::env::temp_dir().join("fleetz_window_clamp").join("window.toml"),
            "n = 99\nunit = \"days\"\n",
        )
        .unwrap();
        assert_eq!(
            state.load(),
            Some(TimeWindow { n: 31, unit: WindowUnit::Days })
        );
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("fleetz_window_clamp"));
    }
}
