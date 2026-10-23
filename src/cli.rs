use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "fleetz",
    // Bare `version` takes CARGO_PKG_VERSION, the same string the TUI header
    // reads through `ui::VERSION`. Naming it explicitly here would give the
    // two a way to disagree.
    version,
    about = "Multi-repo dashboard TUI: see what changed across your repos"
)]
pub struct Args {
    /// Show all activity since DATE (e.g. 2026-03-15, yesterday, 3d, 1w)
    #[arg(long)]
    pub since: Option<String>,

    /// Only show repos pushed before DATE (same formats as --since)
    #[arg(long)]
    pub before: Option<String>,

    /// Show all activity since midnight
    #[arg(short = 't', long)]
    pub today: bool,

    /// Show all divergence regardless of time
    #[arg(short = 'a', long)]
    pub all: bool,

    /// Local only: show uncommitted/untracked/unpushed (no GitHub)
    #[arg(short = 'd', long)]
    pub dirty: bool,

    /// Filter commits by author (substring match)
    #[arg(long)]
    pub author: Option<String>,

    /// Show the full open-PR list (title, author, age, review state).
    ///
    /// PR counts and numbers always show; this only controls the detailed
    /// section in the repo pane. The data is fetched either way, because a
    /// one-PR page costs exactly what a five-PR page with every field costs.
    #[arg(short = 'p', long)]
    pub prs: bool,

    /// Auto-refresh mode for persistent tmux pane (config `watch` sets the default)
    #[arg(long)]
    pub watch: bool,

    /// Start with watch off, overriding the `watch` config key
    #[arg(long, conflicts_with = "watch")]
    pub no_watch: bool,

    /// Watch refresh interval in seconds (default: config refresh_interval)
    #[arg(long)]
    pub interval: Option<u64>,

    /// Print a one-shot report, as repoz does, instead of opening the TUI
    #[arg(long)]
    pub print: bool,

    /// With --print: report one repo, `/owner/repo`, or `/repo` for one of
    /// your own
    #[arg(requires = "print", value_parser = target_spec)]
    pub target: Option<String>,
}

/// A print target as repoz spells it: a leading `/`, then `owner/repo` or
/// `repo`.
fn target_spec(value: &str) -> Result<String, String> {
    match value.strip_prefix('/') {
        Some(rest) if !rest.is_empty() => Ok(value.to_string()),
        _ => Err("a target is /owner/repo or /repo".to_string()),
    }
}

/// Resolved operating mode from the CLI args.
#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Auto,
    Since(String),
    Today,
    All,
    Dirty,
}

impl Args {
    pub fn mode(&self) -> Mode {
        if self.dirty {
            Mode::Dirty
        } else if self.all {
            Mode::All
        } else if self.today {
            Mode::Today
        } else if let Some(ref s) = self.since {
            Mode::Since(s.clone())
        } else {
            Mode::Auto
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn default_mode_is_auto() {
        let args = Args::parse_from(["fleetz"]);
        assert_eq!(args.mode(), Mode::Auto);
    }

    #[test]
    fn dirty_flag() {
        let args = Args::parse_from(["fleetz", "-d"]);
        assert_eq!(args.mode(), Mode::Dirty);
    }

    #[test]
    fn since_flag() {
        let args = Args::parse_from(["fleetz", "--since", "3d"]);
        assert_eq!(args.mode(), Mode::Since("3d".to_string()));
    }

    #[test]
    fn today_flag() {
        let args = Args::parse_from(["fleetz", "-t"]);
        assert_eq!(args.mode(), Mode::Today);
    }

    #[test]
    fn all_flag() {
        let args = Args::parse_from(["fleetz", "-a"]);
        assert_eq!(args.mode(), Mode::All);
    }

    #[test]
    fn combined_flags() {
        let args = Args::parse_from(["fleetz", "--since", "1w", "--author", "andreas", "-p"]);
        assert_eq!(args.mode(), Mode::Since("1w".to_string()));
        assert_eq!(args.author.as_deref(), Some("andreas"));
        assert!(args.prs);
    }

    /// `fleetz --version` did not exist: the header knew the version and the
    /// CLI did not, because `#[command(..)]` declared `name` and `about` and
    /// stopped there. Both sides read `CARGO_PKG_VERSION` now, so the flag
    /// cannot report something the running TUI disagrees with.
    #[test]
    fn the_cli_reports_the_same_version_the_header_shows() {
        assert_eq!(
            Args::command().get_version(),
            Some(crate::ui::VERSION),
            "--version is missing or disagrees with the header"
        );
    }

    /// D7: the target positional is print-only, so the TUI's own argument
    /// parsing is what it was.
    #[test]
    fn a_target_is_rejected_without_print() {
        assert!(Args::try_parse_from(["fleetz", "/owner/repo"]).is_err());
        assert!(Args::try_parse_from(["fleetz", "-a", "/repo"]).is_err());
        let args = Args::try_parse_from(["fleetz", "--print", "/owner/repo"]).unwrap();
        assert_eq!(args.target.as_deref(), Some("/owner/repo"));
        let args = Args::try_parse_from(["fleetz", "--print", "/repo"]).unwrap();
        assert_eq!(args.target.as_deref(), Some("/repo"));
        assert_eq!(Args::parse_from(["fleetz"]).target, None);
    }

    /// bash repoz takes only a leading `/` as a target; anything else is an
    /// unknown argument.
    #[test]
    fn a_target_needs_its_leading_slash() {
        assert!(Args::try_parse_from(["fleetz", "--print", "owner/repo"]).is_err());
        assert!(Args::try_parse_from(["fleetz", "--print", "/"]).is_err());
    }

    #[test]
    fn the_parser_accepts_a_plain_invocation() {
        // Clap panics at build time on a malformed derive, so parsing once in
        // a test is what turns that into a failing test rather than a crash
        // on someone's first run.
        Args::command().debug_assert();
    }
}
