use ratatui::style::{Color, Style};
use ratatui::text::Span;

use crate::model::{CiStatus, RepoKind, RepoStatus};
use crate::ui::colors;

/// Width of the org/private/public glyph column: the glyph itself plus a
/// trailing space, always reserved -- even for repos with no GitHub
/// metadata -- so the name column starts at the same place on every row.
pub const GLYPH_W: usize = 2;

/// Display width of the selection marker column ("▸ " selected, "  " not).
/// A DISPLAY width, deliberately not `"▸ ".len()`: the arrow is 3 UTF-8
/// bytes but one screen cell, so byte length would over-count the selected
/// row by 2 and shift its whole badge group left when hovered.
pub const MARKER_W: usize = 2;

/// The repoz-style org/private/public glyph shown before a repo's name (see
/// repoz's `get_repo_glyph`): a Nerd-Font building/lock/globe icon, colored
/// by kind. Rendered at a fixed `GLYPH_W` columns (the glyph is a single
/// display cell in a Nerd Font, plus a trailing space); a blank placeholder
/// of the same width stands in when there's no GitHub metadata to go on
/// (e.g. local-only repos), so the name column starts at the same place on
/// every row regardless.
pub fn glyph_span(repo: &RepoStatus) -> Span<'static> {
    let Some(glyph) = kind_glyph(repo.repo_kind) else {
        return Span::raw(" ".repeat(GLYPH_W));
    };
    Span::styled(
        format!("{glyph} "),
        Style::default().fg(kind_color(repo.repo_kind)),
    )
}

/// The Nerd Font icon for a repo kind, or None for `Unknown` (which has no
/// icon and renders as blank padding).
pub fn kind_glyph(kind: RepoKind) -> Option<&'static str> {
    match kind {
        RepoKind::Org => Some("\u{f1ad}"),     // building
        RepoKind::Private => Some("\u{f023}"), // lock
        RepoKind::Public => Some("\u{f0ac}"),  // globe
        RepoKind::Unknown => None,
    }
}

/// The color for a repo kind. Shared by the list glyph and the detail pane's
/// owner line so the two can never disagree about what "org" looks like.
pub fn kind_color(kind: RepoKind) -> Color {
    match kind {
        RepoKind::Org => colors::MAUVE,
        RepoKind::Private => colors::PEACH,
        RepoKind::Public => colors::GREEN,
        RepoKind::Unknown => colors::OVERLAY,
    }
}

/// The glyph, color, and spelled-out name for a CI state, for the detail
/// pane's named badges ("◉ CI passed"). Single source, so the panel title and
/// the body line can never disagree about what "failed" looks like.
pub fn ci_display(status: &CiStatus) -> (&'static str, Color, &'static str) {
    match status {
        CiStatus::Passed => ("◉", colors::GREEN, "passed"),
        CiStatus::Failed => ("◉", colors::RED, "failed"),
        CiStatus::Pending => ("◉", colors::PEACH, "pending"),
        CiStatus::Unknown => ("", colors::OVERLAY, ""),
    }
}

fn ci_text(repo: &RepoStatus) -> String {
    let sym = repo.ci_status.symbol();
    if sym.is_empty() {
        String::new()
    } else {
        format!("{sym} ")
    }
}

/// Octicon `archive`. Stash has no formal glyph, but VS Code and GitHub
/// Desktop both draw it as an archive box, so this is borrowed rather than
/// invented -- which is the whole test for putting a symbol on screen.
pub const STASH_GLYPH: &str = "\u{f411}";

/// Worktrees get letters, not a glyph, because git worktrees have no
/// conventional symbol at all. `⎇` was the obvious candidate and is wrong:
/// U+2387 is the Alt key, adopted by shell prompts to mean *branch*, so it
/// would read as "branch" in a column sitting beside actual branch counts.
/// Two unambiguous letters beat a symbol a reader has to be taught.
const WORKTREE_ABBR: &str = "wt";

fn pr_text(repo: &RepoStatus) -> String {
    if repo.pr_count > 0 {
        format!("{}PR ", repo.pr_count)
    } else {
        String::new()
    }
}

fn stash_text(repo: &RepoStatus) -> String {
    if repo.stash_count > 0 {
        format!("{}{STASH_GLYPH} ", repo.stash_count)
    } else {
        String::new()
    }
}

fn worktree_text(repo: &RepoStatus) -> String {
    if !repo.worktrees.is_empty() {
        format!("{}{WORKTREE_ABBR} ", repo.worktrees.len())
    } else {
        String::new()
    }
}

/// The color a repo's status token renders in. Shared by the list badge and
/// the detail pane so the two can never disagree about what a state looks
/// like. Not-cloned is checked first and gets the alert color: its counters
/// are all zero, so it would otherwise fall through to the synced green.
pub fn status_color(repo: &RepoStatus) -> Color {
    if repo.is_not_cloned() {
        colors::RED
    } else if repo.behind > 0 {
        colors::PEACH
    } else if repo.dirty > 0 {
        colors::YELLOW
    } else if repo.ahead > 0 {
        colors::PEACH
    } else {
        colors::GREEN
    }
}

/// Build a repo's badge group tight (no inter-field padding), returning the
/// colored spans and their total display width. The caller right-aligns this
/// flush to the pane's inner edge with a dot-fill before it -- so, repoz
/// style, every row's badges end at the same right edge and the dots run all
/// the way out to them (rather than stopping at a fixed column and leaving a
/// gap).
///
/// **Sync status goes last, so it is the one token flush against the right
/// edge on every row.** Every other badge (CI, PRs, stashes, worktrees) is
/// optional, and with any of them trailing the status token, a repo that had
/// one pushed its status left while a repo without kept it flush right: the
/// column that appears on *every* row was the one that failed to line up. CI
/// made this most visible, being both common and only one cell wide.
///
/// Each optional field carries its own *trailing* space, so the group stays
/// self-separating with nothing dangling on the right.
///
/// `dim_pending` is the off phase of the pending-CI blink: callers pass
/// `app.watch_mode && !app.blink_on`, so a running job pulses only while the
/// dashboard is actually live.
pub fn badge_spans(repo: &RepoStatus, dim_pending: bool) -> (Vec<Span<'static>>, usize) {
    let mut spans = Vec::new();
    let mut width = 0usize;

    let ci = ci_text(repo);
    if !ci.is_empty() {
        width += ci.chars().count();
        // Pending is orange; under watch it blinks (dim phase drops to
        // SURFACE, same glyph, so column widths never jitter). Passed and
        // Failed stay steady.
        let ci_color = match repo.ci_status {
            CiStatus::Passed => colors::GREEN,
            CiStatus::Failed => colors::RED,
            CiStatus::Pending => {
                if dim_pending {
                    colors::SURFACE
                } else {
                    colors::PEACH
                }
            }
            CiStatus::Unknown => colors::OVERLAY,
        };
        spans.push(Span::styled(ci, Style::default().fg(ci_color)));
    }

    let pr = pr_text(repo);
    if !pr.is_empty() {
        width += pr.chars().count();
        spans.push(Span::styled(pr, Style::default().fg(colors::PR_CLR)));
    }

    let stash = stash_text(repo);
    if !stash.is_empty() {
        width += stash.chars().count();
        spans.push(Span::styled(stash, Style::default().fg(colors::OVERLAY)));
    }

    let worktree = worktree_text(repo);
    if !worktree.is_empty() {
        width += worktree.chars().count();
        spans.push(Span::styled(worktree, Style::default().fg(colors::OVERLAY)));
    }

    // Last, and never optional: this is the token every row aligns on.
    let status = repo.short_status();
    width += status.chars().count();
    spans.push(Span::styled(
        status,
        Style::default().fg(status_color(repo)),
    ));

    (spans, width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RepoKind, RepoLocation};

    fn repo(name: &str) -> RepoStatus {
        RepoStatus {
            daily: None,
            name: name.to_string(),
            path: Some("~/repos/x".to_string()),
            location: RepoLocation::Primary,
            branches: vec![],
            commits: vec![],
            behind: 0,
            ahead: 0,
            dirty: 0,
            pr_count: 0,
            prs: vec![],
            ci_status: CiStatus::Unknown,
            ci_workflow: None,
            ci_duration: None,
            groups: vec![],
            stash_count: 0,
            stash_entries: vec![],
            worktrees: vec![],
            reflog: vec![],
            graph: vec![],
            repo_kind: RepoKind::Unknown,
            owner: None,
            remote_url: None,
            remote_only_branches: vec![],
            work_key: None,
        }
    }

    fn rendered(repo: &RepoStatus) -> (String, usize) {
        let (spans, width) = badge_spans(repo, false);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        (text, width)
    }

    #[test]
    fn status_is_last_so_it_aligns_across_rows() {
        // The reported bug: CI is only on some repos, so with CI trailing the
        // status token the ✓ column zig-zagged between rows that had CI and
        // rows that didn't. The group is right-aligned, so whatever is last is
        // what lines up, and status is the only badge present on every row.
        let plain = repo("plain");
        let mut with_ci = repo("with-ci");
        with_ci.ci_status = CiStatus::Passed;

        let (plain_text, _) = rendered(&plain);
        let (ci_text, _) = rendered(&with_ci);

        assert!(plain_text.ends_with('✓'));
        assert!(ci_text.ends_with('✓'), "CI must not trail the status");
        assert_eq!(ci_text, "◉ ✓");
    }

    #[test]
    fn every_optional_badge_sits_left_of_the_status() {
        let mut busy = repo("busy");
        busy.ci_status = CiStatus::Failed;
        busy.pr_count = 2;
        busy.stash_count = 1;
        busy.dirty = 3;

        let (text, width) = rendered(&busy);
        assert!(text.ends_with("3!"), "got {text:?}");
        assert!(text.starts_with('◉'), "CI leads the group: {text:?}");
        assert_eq!(width, text.chars().count(), "width matches what is drawn");
    }

    /// Regression: stash rendered as `3s` and worktrees as `1w`, which are
    /// the same shorthand the age column uses for seconds and weeks. On a row
    /// reading `dotfiles ··· 3s ✓` there was nothing to say whether that was
    /// three stashes or three seconds.
    #[test]
    fn no_counter_badge_reads_as_a_unit_of_time() {
        let mut busy = repo("busy");
        busy.pr_count = 2;
        busy.stash_count = 3;
        busy.worktrees = vec![crate::model::Worktree {
            path: "~/repos/x-hotfix".to_string(),
            branch: "hotfix".to_string(),
        }];

        let (text, _) = rendered(&busy);
        // Whole tokens, not substrings: `1wt` legitimately contains `1w`, and
        // asserting on the substring would forbid the very fix being tested.
        // The real property is that no complete badge token is digits
        // followed by a lone duration letter.
        for token in text.split_whitespace() {
            let digits: String = token.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() {
                continue;
            }
            let rest = &token[digits.len()..];
            assert!(
                !matches!(rest, "s" | "m" | "h" | "d" | "w"),
                "badge token {token:?} in {text:?} reads as a duration"
            );
        }
        assert!(text.contains(&format!("3{STASH_GLYPH}")), "got {text:?}");
        assert!(text.contains("1wt"), "got {text:?}");
        // `2PR` is letters, but `PR` is not a duration, so it was never the
        // problem the other two were.
        assert!(text.contains("2PR"), "got {text:?}");
    }

    /// The badge column is right-aligned off `badge_spans`' own width, so a
    /// glyph counted wrong shifts every status token on the row.
    #[test]
    fn the_reported_width_matches_the_glyphs_drawn() {
        let mut busy = repo("busy");
        busy.pr_count = 2;
        busy.stash_count = 3;
        let (text, width) = rendered(&busy);
        assert_eq!(width, text.chars().count(), "got {text:?}");
    }

    #[test]
    fn a_bare_repo_renders_only_its_status() {
        // No leading or trailing padding when nothing optional is present, so
        // the token sits flush against the dot fill.
        let (text, width) = rendered(&repo("bare"));
        assert_eq!(text, "✓");
        assert_eq!(width, 1);
    }
}
