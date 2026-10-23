//! How print mode draws, byte for byte as bash repoz does (D8 of
//! `docs/repoz-print-design.md`): one palette table, one width rule, and the
//! line builders for the header, the dividers, a row, a commit line and the
//! summary. Every builder returns its line with the trailing newline, so a
//! section is assembled as a string and written once.
//!
//! Lengths are counted in characters, as bash's `${#var}` counts them under
//! a UTF-8 locale; escapes never enter a length.

/// The Catppuccin Mocha escapes bash repoz paints with, as it spells them.
pub mod palette {
    /// Behind and ahead, and the default row colour.
    pub const PEACH: &str = "\x1b[38;2;250;179;135m";
    /// Synced, and public repos.
    pub const GREEN: &str = "\x1b[38;2;166;227;161m";
    /// Uncommitted files.
    pub const YELLOW: &str = "\x1b[38;2;249;226;175m";
    /// Repo names.
    pub const SAPPHIRE: &str = "\x1b[38;2;116;199;236m";
    /// The header's tool name.
    pub const LAVENDER: &str = "\x1b[38;2;180;190;254m";
    /// Section labels, stashes, PRs and org repos.
    pub const MAUVE: &str = "\x1b[38;2;203;166;247m";
    /// Commit hashes.
    pub const HASH: &str = "\x1b[38;2;140;155;180m";
    /// A commit's `+N`.
    pub const DIFF_ADD: &str = "\x1b[38;2;134;194;134m";
    /// A commit's `-N`.
    pub const DIFF_DEL: &str = "\x1b[38;2;210;140;140m";
    /// Paths and quiet text.
    pub const OVERLAY: &str = "\x1b[38;2;127;132;156m";
    /// Dots and dividers.
    pub const SURFACE: &str = "\x1b[38;2;69;71;90m";
    /// Not cloned, operations and failed fetches.
    pub const RED: &str = "\x1b[38;2;243;139;168m";
    pub const BOLD: &str = "\x1b[1m";
    pub const RESET: &str = "\x1b[0m";
}

use palette::*;

/// The repo-type glyphs (Font Awesome codepoints, as bash's defaults).
pub const GLYPH_ORG: &str = "\u{f1ad}";
pub const GLYPH_PRIVATE: &str = "\u{f023}";
pub const GLYPH_PUBLIC: &str = "\u{f0ac}";
/// The countable states' symbols.
pub const SYM_BEHIND: &str = "\u{2193}";
pub const SYM_AHEAD: &str = "\u{2191}";
pub const SYM_DIRTY: &str = "!";
pub const SYM_STASH: &str = "\u{2691}";

/// The em dash after the tool name in the header, as bash prints it.
const HEADER_DASH: &str = "\u{2014}";
const RULE: char = '\u{2500}';
const DOT: char = '\u{b7}';

fn len(s: &str) -> usize {
    s.chars().count()
}

fn repeat(c: char, n: usize) -> String {
    std::iter::repeat_n(c, n).collect()
}

/// The terminal width by bash repoz's rule: `stty size </dev/tty`, else
/// `COLUMNS`, else 0; under 40, `tput cols` (80 when it fails); still under
/// 40, 80.
pub fn term_width() -> usize {
    width_from(tty_columns(), std::env::var("COLUMNS").ok(), tput_columns)
}

/// The rule of [`term_width`] over its three sources. A value bash's
/// arithmetic would not read as a number counts as 0. A tty that answers
/// wins even when its answer is small, as `stty` does: `COLUMNS` is only
/// read when there is no answer at all.
pub fn width_from(
    tty: Option<i64>,
    columns: Option<String>,
    tput: impl FnOnce() -> Option<i64>,
) -> usize {
    let mut width = match tty {
        Some(cols) => cols,
        None => columns
            .filter(|c| !c.is_empty())
            .map_or(0, |c| c.trim().parse().unwrap_or(0)),
    };
    if width < 40 {
        width = tput().unwrap_or(80);
    }
    if width < 40 {
        width = 80;
    }
    width as usize
}

/// The columns of the controlling terminal, or None when there is none.
fn tty_columns() -> Option<i64> {
    use std::os::fd::AsRawFd;
    let tty = std::fs::File::open("/dev/tty").ok()?;
    // SAFETY: `winsize` is plain data, and the fd stays open for the call.
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::ioctl(tty.as_raw_fd(), libc::TIOCGWINSZ, &mut size) } == 0;
    ok.then_some(i64::from(size.ws_col))
}

fn tput_columns() -> Option<i64> {
    let out = std::process::Command::new("tput")
        .arg("cols")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Which glyph a row draws, from its GitHub owner type and visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Org,
    Private,
    Public,
    /// No GitHub metadata: a blank column when glyphs are on.
    None,
}

impl Glyph {
    pub fn of(kind: crate::model::RepoKind) -> Self {
        use crate::model::RepoKind;
        match kind {
            RepoKind::Org => Self::Org,
            RepoKind::Private => Self::Private,
            RepoKind::Public => Self::Public,
            RepoKind::Unknown => Self::None,
        }
    }

    fn painted(self) -> Option<String> {
        let (color, glyph) = match self {
            Self::Org => (MAUVE, GLYPH_ORG),
            Self::Private => (PEACH, GLYPH_PRIVATE),
            Self::Public => (GREEN, GLYPH_PUBLIC),
            Self::None => return None,
        };
        Some(format!("{color}{glyph}{RESET}"))
    }
}

/// Everything a draw needs to know about the screen.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub width: usize,
    pub glyphs: bool,
}

impl Layout {
    fn div_width(self) -> usize {
        self.width.saturating_sub(4)
    }

    /// The blank line, then the tool name and the mode, then the divider.
    pub fn header(self, mode: &str) -> String {
        format!(
            "\n  {LAVENDER}{BOLD}repoz{RESET} {OVERLAY}{HEADER_DASH} {mode}{RESET}\n{}",
            self.divider()
        )
    }

    /// A full-width rule, indented two.
    pub fn divider(self) -> String {
        format!("  {SURFACE}{}{RESET}\n", repeat(RULE, self.div_width()))
    }

    /// bash's `section_div`: the label inside the rule.
    pub fn section(self, label: &str) -> String {
        let prefix = len(&format!("{RULE}{RULE} {label} "));
        let remain = self.div_width().saturating_sub(prefix);
        format!(
            "  {SURFACE}{RULE}{RULE} {MAUVE}{label} {SURFACE}{}{RESET}\n",
            repeat(RULE, remain)
        )
    }

    /// bash's `print_row`: name, path, a dotted leader and the status flush
    /// right, each status token in its own colour, `fallback` for the words.
    pub fn row(
        self,
        glyph: Glyph,
        name: &str,
        path: Option<&str>,
        status: &str,
        fallback: &str,
    ) -> String {
        let glyph_w = if self.glyphs { 2 } else { 0 };
        let label_len = match path {
            Some(path) => len(name) + 1 + len(path),
            None => len(name),
        };
        let w = self.div_width() as i64 - 4;
        let pad = (w - glyph_w - label_len as i64 - len(status) as i64).max(3) as usize;
        let mut line = String::from("    ");
        if self.glyphs {
            match glyph.painted() {
                Some(painted) => {
                    line.push_str(&painted);
                    line.push(' ');
                }
                None => line.push_str("  "),
            }
        }
        line.push_str(&format!("{SAPPHIRE}{name}{RESET}"));
        if let Some(path) = path {
            line.push_str(&format!(" {OVERLAY}{path}{RESET}"));
        }
        line.push_str(&format!(
            " {SURFACE}{}{RESET} {}{RESET}\n",
            repeat(DOT, pad),
            colorize_status(status, fallback)
        ));
        line
    }

    /// bash's `print_commit_line`: hash and subject, truncated to fit, a
    /// dotted leader, then the diff stat and the author.
    pub fn commit(self, hash: &str, message: &str, author: &str, ins: u32, del: u32) -> String {
        let stat = match (ins, del) {
            (0, 0) => String::new(),
            (i, 0) => format!("+{i}"),
            (0, d) => format!("-{d}"),
            (i, d) => format!("+{i} -{d}"),
        };
        let right_len = if stat.is_empty() {
            len(author)
        } else {
            len(&stat) + 2 + len(author)
        } as i64;
        let avail = self.width as i64 - 11;
        let mut left = format!("{hash} {message}");
        let max_left = avail - right_len - 3;
        if len(&left) as i64 > max_left && max_left > 10 {
            left = left.chars().take(max_left as usize - 1).collect();
            left.push_str("..");
        }
        let pad = (avail - len(&left) as i64 - right_len).max(2) as usize;

        let mut colored = String::new();
        if ins > 0 {
            colored.push_str(&format!("{DIFF_ADD}+{ins}{RESET}"));
        }
        if del > 0 {
            if !colored.is_empty() {
                colored.push(' ');
            }
            colored.push_str(&format!("{DIFF_DEL}-{del}{RESET}"));
        }
        if !colored.is_empty() {
            colored.push_str("  ");
        }
        colored.push_str(&format!("{OVERLAY}{author}{RESET}"));

        // `${left#* }`: what follows the first space, the subject as cut.
        let subject = left.split_once(' ').map_or(left.as_str(), |(_, s)| s);
        format!(
            "    {SURFACE}\u{2514}{RULE}{RESET} {HASH}{hash}{RESET} {subject} {SURFACE}{}{RESET} {colored}\n",
            repeat(DOT, pad)
        )
    }

    /// The divider, the counts in words, the glyph legend and the version
    /// flush right, then a blank line.
    pub fn summary(self, counts: &Counts, version: &str) -> String {
        let mut out = format!("\n{}", self.divider());
        out.push_str(&format!("  {}\n", counts.words()));
        if self.glyphs {
            out.push_str(&format!(
                "  {PEACH}{GLYPH_PRIVATE}{RESET} {OVERLAY}private{RESET} \u{b7} \
                 {GREEN}{GLYPH_PUBLIC}{RESET} {OVERLAY}public{RESET} \u{b7} \
                 {MAUVE}{GLYPH_ORG}{RESET} {OVERLAY}org{RESET}   \n"
            ));
        }
        let pad = (self.width as i64 - len(version) as i64 - 1).max(2) as usize;
        out.push_str(&format!("{}{SURFACE}{version}{RESET}\n\n", " ".repeat(pad)));
        out
    }
}

/// bash's `colorize_status`: each whitespace-separated token in its own
/// colour, joined by single spaces.
pub fn colorize_status(plain: &str, fallback: &str) -> String {
    plain
        .split_whitespace()
        .map(|tok| {
            let color = if tok.ends_with(SYM_BEHIND) || tok.ends_with(SYM_AHEAD) {
                PEACH
            } else if tok.ends_with(SYM_STASH) || tok.ends_with("PR") {
                MAUVE
            } else if tok.ends_with('!') {
                YELLOW
            } else if tok == "fetch" || tok.starts_with("failed") {
                RED
            } else {
                fallback
            };
            format!("{color}{tok}{RESET}")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// bash's `status_symbols`: the non-zero counts as `2↓ 1↑ 3! 1⚑`.
pub fn status_symbols(behind: u32, ahead: u32, dirty: u32, stashes: u32) -> String {
    [
        (behind, SYM_BEHIND),
        (ahead, SYM_AHEAD),
        (dirty, SYM_DIRTY),
        (stashes, SYM_STASH),
    ]
    .iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, sym)| format!("{n}{sym}"))
    .collect::<Vec<_>>()
    .join(" ")
}

/// bash's `status_with_op`: `op, symbols`, or whichever of the two is there.
pub fn with_op(op: &str, symbols: &str) -> String {
    match (op.is_empty(), symbols.is_empty()) {
        (false, false) => format!("{op}, {symbols}"),
        _ => format!("{op}{symbols}"),
    }
}

/// The summary's tallies, in bash repoz's order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Counts {
    pub in_progress: u32,
    pub fetch_failed: u32,
    pub behind: u32,
    pub ahead: u32,
    pub uncommitted: u32,
    pub stashed: u32,
    pub synced: u32,
    pub not_local: u32,
}

impl Counts {
    /// The non-zero tallies, each in its colour, separated by a dim dot.
    fn words(&self) -> String {
        [
            (self.in_progress, RED, "in progress"),
            (self.fetch_failed, RED, "fetch failed"),
            (self.behind, PEACH, "behind"),
            (self.ahead, PEACH, "ahead"),
            (self.uncommitted, YELLOW, "uncommitted"),
            (self.stashed, MAUVE, "stashed"),
            (self.synced, GREEN, "synced"),
            (self.not_local, OVERLAY, "not local"),
        ]
        .iter()
        .filter(|(n, _, _)| *n > 0)
        .map(|(n, color, word)| format!("{color}{n} {word}{RESET}"))
        .collect::<Vec<_>>()
        .join(&format!(" {SURFACE}{DOT}{RESET} "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::parity::strip_ansi;

    const AT_100: Layout = Layout {
        width: 100,
        glyphs: true,
    };

    #[test]
    fn the_width_follows_bash_repoz_rule() {
        let never = || -> Option<i64> { panic!("tput asked when it need not be") };
        assert_eq!(width_from(Some(120), Some("100".into()), never), 120);
        assert_eq!(width_from(None, Some("100".into()), never), 100);
        // A tty that answers small goes to tput, not to COLUMNS.
        assert_eq!(width_from(Some(0), Some("100".into()), || Some(90)), 90);
        assert_eq!(width_from(None, None, || Some(132)), 132);
        assert_eq!(width_from(None, Some("wide".into()), || None), 80);
        assert_eq!(width_from(None, Some("39".into()), || Some(20)), 80);
        assert_eq!(width_from(None, Some(String::new()), || Some(40)), 40);
    }

    #[test]
    fn each_status_token_takes_its_own_colour() {
        let painted = colorize_status("rebasing, fetch failed, 2\u{2193} 3! 1\u{2691}", RED);
        assert_eq!(
            painted,
            format!(
                "{RED}rebasing,{RESET} {RED}fetch{RESET} {RED}failed,{RESET} \
                 {PEACH}2\u{2193}{RESET} {YELLOW}3!{RESET} {MAUVE}1\u{2691}{RESET}"
            )
        );
        assert_eq!(
            colorize_status("not cloned", RED),
            format!("{RED}not{RESET} {RED}cloned{RESET}")
        );
    }

    #[test]
    fn symbols_skip_zero_counts_and_an_operation_goes_in_front() {
        assert_eq!(status_symbols(0, 0, 0, 0), "");
        assert_eq!(
            status_symbols(2, 1, 3, 1),
            "2\u{2193} 1\u{2191} 3! 1\u{2691}"
        );
        assert_eq!(with_op("merging", "1!"), "merging, 1!");
        assert_eq!(with_op("merging", ""), "merging");
        assert_eq!(with_op("", "1!"), "1!");
    }

    #[test]
    fn a_row_puts_its_status_flush_right_at_the_divider_minus_four() {
        let row = AT_100.row(
            Glyph::Private,
            "alpha-tool",
            Some("~/a"),
            "1\u{2193}",
            PEACH,
        );
        // The indent, the glyph and its space, then label, leader and status
        // filling W (width - 8) less the glyph's two, and the two spaces
        // around the leader: 98 at 100 columns.
        let text = strip_ansi(&row);
        assert_eq!(len(text.trim_end_matches('\n')), 4 + 2 + (92 - 2) + 2);
        let blank = AT_100.row(Glyph::None, "x", None, "synced", GREEN);
        assert!(blank.starts_with(&format!("      {SAPPHIRE}x")));
        let plain = Layout {
            glyphs: false,
            ..AT_100
        }
        .row(Glyph::Org, "x", None, "synced", GREEN);
        assert!(plain.starts_with(&format!("    {SAPPHIRE}x")));
    }

    #[test]
    fn a_row_keeps_three_dots_however_long_its_label() {
        let row = AT_100.row(Glyph::None, &"n".repeat(120), None, "synced", GREEN);
        assert!(strip_ansi(&row).contains(" \u{b7}\u{b7}\u{b7} synced"));
    }

    #[test]
    fn a_long_subject_is_cut_with_two_dots() {
        let subject = "s".repeat(200);
        let line = strip_ansi(&AT_100.commit("abc1234", &subject, "Ann", 1, 2));
        // avail 89, right "+1 -2  Ann" 10, so the left part may be 76; bash
        // keeps 75 and adds two dots, one over, and the leader takes the
        // floor of two.
        let expected_subject = format!("{}..", "s".repeat(75 - 8));
        assert!(
            line.contains(&format!(
                "abc1234 {expected_subject} \u{b7}\u{b7} +1 -2  Ann"
            )),
            "{line}"
        );
    }

    #[test]
    fn the_summary_joins_its_counts_with_a_dim_dot() {
        let counts = Counts {
            behind: 1,
            not_local: 2,
            ..Counts::default()
        };
        assert_eq!(
            counts.words(),
            format!("{PEACH}1 behind{RESET} {SURFACE}\u{b7}{RESET} {OVERLAY}2 not local{RESET}")
        );
        assert_eq!(Counts::default().words(), "");
    }
}
