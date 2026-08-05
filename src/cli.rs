use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "fleetz",
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

    /// Local only — show uncommitted/untracked/unpushed (no GitHub)
    #[arg(short = 'd', long)]
    pub dirty: bool,

    /// Filter commits by author (substring match)
    #[arg(long)]
    pub author: Option<String>,

    /// Include open pull request counts
    #[arg(short = 'p', long)]
    pub prs: bool,

    /// Auto-refresh mode for persistent tmux pane
    #[arg(long)]
    pub watch: bool,

    /// Watch refresh interval in seconds (default: config refresh_interval)
    #[arg(long)]
    pub interval: Option<u64>,
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
}
