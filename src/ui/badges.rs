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
    let (glyph, color) = match repo.repo_kind {
        RepoKind::Org => ("\u{f1ad}", colors::MAUVE),      // building
        RepoKind::Private => ("\u{f023}", colors::PEACH),  // lock
        RepoKind::Public => ("\u{f0ac}", colors::GREEN),   // globe
        RepoKind::Unknown => return Span::raw(" ".repeat(GLYPH_W)),
    };
    Span::styled(format!("{glyph} "), Style::default().fg(color))
}

fn ci_text(repo: &RepoStatus) -> String {
    let sym = repo.ci_status.symbol();
    if sym.is_empty() {
        String::new()
    } else {
        format!(" {sym}")
    }
}

fn pr_text(repo: &RepoStatus) -> String {
    if repo.pr_count > 0 {
        format!(" {}PR", repo.pr_count)
    } else {
        String::new()
    }
}

fn stash_text(repo: &RepoStatus) -> String {
    if repo.stash_count > 0 {
        format!(" {}s", repo.stash_count)
    } else {
        String::new()
    }
}

fn worktree_text(repo: &RepoStatus) -> String {
    if !repo.worktrees.is_empty() {
        format!(" {}w", repo.worktrees.len())
    } else {
        String::new()
    }
}

fn status_color(repo: &RepoStatus) -> Color {
    if repo.behind > 0 {
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
/// gap). Order is status, CI, PRs, stashes, worktrees; each optional field
/// carries its own leading space, so the group is self-separating.
pub fn badge_spans(repo: &RepoStatus) -> (Vec<Span<'static>>, usize) {
    let mut spans = Vec::new();
    let mut width = 0usize;

    let status = repo.short_status();
    width += status.chars().count();
    spans.push(Span::styled(status, Style::default().fg(status_color(repo))));

    let ci = ci_text(repo);
    if !ci.is_empty() {
        width += ci.chars().count();
        let ci_color = match repo.ci_status {
            CiStatus::Passed => colors::GREEN,
            CiStatus::Failed => colors::RED,
            CiStatus::Pending => colors::YELLOW,
            CiStatus::Unknown => colors::OVERLAY,
        };
        spans.push(Span::styled(ci, Style::default().fg(ci_color)));
    }

    let pr = pr_text(repo);
    if !pr.is_empty() {
        width += pr.chars().count();
        spans.push(Span::styled(pr, Style::default().fg(colors::MAUVE)));
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

    (spans, width)
}
