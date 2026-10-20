use ratatui::style::{Color, Style};
use ratatui::text::Span;

use crate::app::Icons;
use crate::model::{CiStatus, RepoKind, RepoStatus};
use crate::ui::colors;

thread_local! {
    /// The glyph set every glyph choice (`kind_icon`, `stash_icon`,
    /// `ui::watch_icon`) draws from: the single source, written only by
    /// `set_active_icons`. Ambient rather than an argument because those are
    /// reached from functions whose signatures many tests pin.
    ///
    /// A thread-local, not a process global: libtest runs each test on its
    /// own thread, so a test that selects `Plain` cannot change what another
    /// test draws. Correct in production because the startup call and the
    /// event loop both run inside the future `#[tokio::main]` polls on the
    /// main thread, every draw happens in that loop, and spawned tasks only
    /// send events. The rule that follows: rendering must stay on the thread
    /// that set the icons, so the event loop must not move into a spawned task
    /// without this setting moving with it.
    static ACTIVE_ICONS: std::cell::Cell<Icons> = const { std::cell::Cell::new(Icons::Nerd) };
}

/// The glyph set this thread draws with. `Nerd` until `set_active_icons`.
pub fn active_icons() -> Icons {
    ACTIVE_ICONS.with(std::cell::Cell::get)
}

/// Select the glyph set this thread draws with: the `icons` config key, once
/// at startup.
pub fn set_active_icons(icons: Icons) {
    ACTIVE_ICONS.with(|active| active.set(icons));
}

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
    let Some(glyph) = kind_icon(repo.repo_kind) else {
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

/// The plain-font stand-in for a repo kind, or None for `Unknown`. ASCII, so
/// one column in every monospace font and never East Asian Ambiguous. The
/// colour (`kind_color`) still carries most of the meaning; the shapes only
/// have to differ from each other and from every mark a row already draws.
/// `@` is how an org is written on GitHub; `+` and `-` read as open and
/// closed.
fn plain_kind_glyph(kind: RepoKind) -> Option<&'static str> {
    match kind {
        RepoKind::Org => Some("@"),
        RepoKind::Private => Some("-"),
        RepoKind::Public => Some("+"),
        RepoKind::Unknown => None,
    }
}

/// The repo kind's glyph in the active set (`active_icons`): the one
/// place that decides between `kind_glyph` and its plain stand-in, so the
/// list row, the compact grid and the detail pane's owner line cannot
/// disagree.
pub fn kind_icon(kind: RepoKind) -> Option<&'static str> {
    match active_icons() {
        Icons::Nerd => kind_glyph(kind),
        Icons::Plain => plain_kind_glyph(kind),
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
///
/// `dim_pending` is the off phase of the pending-CI blink, on the same terms
/// as `badge_spans`: callers pass `app.watch_mode && !app.blink_on`. The two
/// panes show the same repo's CI, so a lamp that pulses in the list and holds
/// still in the detail pane would read as two different states rather than one
/// running job. The glyph is unchanged between phases -- only the colour
/// moves, so nothing beside it shifts.
pub fn ci_display(status: &CiStatus, dim_pending: bool) -> (&'static str, Color, &'static str) {
    match status {
        CiStatus::Passed => ("◉", colors::GREEN, "passed"),
        CiStatus::Failed => ("◉", colors::RED, "failed"),
        CiStatus::Pending if dim_pending => ("◉", colors::SURFACE, "pending"),
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

/// The plain-font stash mark: `$` is what git's own prompt
/// (`git-prompt.sh`, `GIT_PS1_SHOWSTASHSTATE`) prints when something is
/// stashed, so it is borrowed rather than invented, like `STASH_GLYPH`.
const PLAIN_STASH_GLYPH: &str = "$";

/// The stash mark in the active set (`active_icons`). The list badge and
/// the detail pane's stash row both draw it, so the same symbol means the
/// same thing in both panes.
pub fn stash_icon() -> &'static str {
    match active_icons() {
        Icons::Nerd => STASH_GLYPH,
        Icons::Plain => PLAIN_STASH_GLYPH,
    }
}

/// Worktrees get letters, not a glyph, because git worktrees have no
/// conventional symbol at all. `⎇` was the obvious candidate and is wrong:
/// U+2387 is the Alt key, adopted by shell prompts to mean *branch*, so it
/// would read as "branch" in a column sitting beside actual branch counts.
/// Two unambiguous letters beat a symbol a reader has to be taught.
const WORKTREE_ABBR: &str = "WT";

fn pr_text(repo: &RepoStatus) -> String {
    if repo.pr_count > 0 {
        format!("{}PR ", repo.pr_count)
    } else {
        String::new()
    }
}

fn stash_text(repo: &RepoStatus) -> String {
    if repo.stash_count > 0 {
        format!("{}{} ", repo.stash_count, stash_icon())
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

/// The `WT` badge's color: the dirty counter's when any linked worktree holds
/// uncommitted work, so a clean main checkout does not read as nothing to do.
/// Clean, unknown or `None` keeps the worktree color. Colour only: the `!`
/// counter still counts the main checkout.
fn worktree_color(repo: &RepoStatus) -> Color {
    if repo
        .worktrees
        .iter()
        .any(|w| w.dirty.is_some_and(|n| n > 0))
    {
        counter_color(0, 0, 1)
    } else {
        colors::WORKTREE_CLR
    }
}

/// The color a repo's status token renders in. Shared by the list badge and
/// the detail pane so the two can never disagree about what a state looks
/// like. Not-cloned is checked first and gets the alert color: its counters
/// are all zero, so it would otherwise fall through to the synced green.
pub fn status_color(repo: &RepoStatus) -> Color {
    if repo.is_not_cloned() {
        colors::RED
    } else {
        counter_color(repo.behind, repo.ahead, repo.dirty)
    }
}

/// The sync-state ladder on bare counts: behind first, then dirty, then
/// ahead, else synced green. The single source `status_color` delegates to,
/// and what a linked worktree's counters wear in the Info view, so the two
/// cannot drift. Knows nothing of not-cloned, which only a repo can be.
pub fn counter_color(behind: u32, ahead: u32, dirty: u32) -> Color {
    if behind > 0 {
        colors::PEACH
    } else if dirty > 0 {
        colors::YELLOW
    } else if ahead > 0 {
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
    badge_spans_marked(repo, dim_pending, false)
}

/// The colour of GitHub values from an earlier refresh: a carrier row the
/// last poll did not return (`App::is_not_in_last_poll`). Its CI lamp and PR
/// count keep their values and take this grey, steady, so a `Pending` lamp
/// stops blinking: it says nothing about a run now. Provisional (O4 in
/// `docs/coverage-cache-design.md`); an existing colour, not a new one.
pub const EARLIER_REFRESH: Color = colors::OVERLAY;

/// `badge_spans`, with the CI lamp and PR count drawn in `EARLIER_REFRESH`
/// when `not_in_last_poll`. Widths and glyphs are unchanged, so a marked row
/// lines up with the rest.
pub fn badge_spans_marked(
    repo: &RepoStatus,
    dim_pending: bool,
    not_in_last_poll: bool,
) -> (Vec<Span<'static>>, usize) {
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
        let ci_color = if not_in_last_poll {
            EARLIER_REFRESH
        } else {
            ci_color
        };
        spans.push(Span::styled(ci, Style::default().fg(ci_color)));
    }

    let pr = pr_text(repo);
    if !pr.is_empty() {
        width += pr.chars().count();
        let pr_color = if not_in_last_poll {
            EARLIER_REFRESH
        } else {
            colors::PR_CLR
        };
        spans.push(Span::styled(pr, Style::default().fg(pr_color)));
    }

    let stash = stash_text(repo);
    if !stash.is_empty() {
        width += stash.chars().count();
        spans.push(Span::styled(stash, Style::default().fg(colors::STASH_CLR)));
    }

    let worktree = worktree_text(repo);
    if !worktree.is_empty() {
        width += worktree.chars().count();
        spans.push(Span::styled(
            worktree,
            Style::default().fg(worktree_color(repo)),
        ));
    }

    // A fetch is running and has bytes in: how many (FZ-35a). It says more
    // than an earlier failure's marker, which the fetch's landing replaces.
    // Otherwise, a repo the fetch cap holds says how big and that `f` pulls
    // it (FZ-35b), in the failure marker's colour: its counts are stale too.
    // Otherwise, the last fetch failed, so behind and ahead are counted
    // against old remote refs. Beside the status it qualifies, and left of
    // it so the status keeps the right edge.
    if let Some(label) = crate::ui::fetch_progress_label(&repo.fetch) {
        let label = format!("{label} ");
        width += label.chars().count();
        spans.push(Span::styled(label, Style::default().fg(colors::SUBTEXT)));
    } else if let Some(label) = crate::ui::fetch_hold_label(&repo.fetch) {
        let label = format!("{label} ");
        width += label.chars().count();
        spans.push(Span::styled(label, Style::default().fg(colors::PEACH)));
    } else if let Some(marker) = crate::ui::fetch_marker(&repo.fetch) {
        let marker = format!("{marker} ");
        width += marker.chars().count();
        spans.push(Span::styled(marker, Style::default().fg(colors::PEACH)));
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
            daily_key: None,
            last_local: None,
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
            graph_expanded: Vec::new(),
            repo_kind: RepoKind::Unknown,
            owner: None,
            remote_url: None,
            remote_only_branches: vec![],
            remote_branches: vec![],
            work_key: None,
            fetch: crate::model::FetchFacts::default(),
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

    fn with_failed_fetch(mut r: RepoStatus, kind: crate::git::FetchFailureKind) -> RepoStatus {
        let failure = crate::git::FetchFailure { kind, detail: None };
        r.fetch.record(
            &crate::git::FetchOutcome::Failed(failure),
            chrono::Local::now(),
        );
        r
    }

    /// Rule 5 on the row: a failed fetch is marked, with its kind, beside
    /// the counts it makes stale, and the status keeps the right edge.
    #[test]
    fn a_failed_fetch_is_marked_beside_the_status() {
        let mut behind = repo("behind");
        behind.behind = 3;
        let r = with_failed_fetch(behind, crate::git::FetchFailureKind::Unreachable);
        let (text, width) = rendered(&r);
        assert_eq!(text, "⚠ unreachable 3↓");
        assert_eq!(width, text.chars().count(), "width matches what is drawn");
        let (spans, _) = badge_spans(&r, false);
        assert_eq!(spans[0].style.fg, Some(colors::PEACH));
        assert_eq!(
            spans[1].style.fg,
            Some(status_color(&r)),
            "status keeps its colour"
        );
    }

    fn receiving(bytes: u64) -> crate::git::FetchProgress {
        crate::git::FetchProgress {
            phase: crate::git::FetchPhase::Receiving,
            objects_done: 1,
            objects_total: 2,
            bytes,
        }
    }

    /// FZ-35a (#84), criterion 3: a running fetch with bytes in says how
    /// many, in the fetch marker's place, over an earlier failure's marker;
    /// before the first byte, and once it lands, the row is as it was.
    #[test]
    fn a_running_fetch_shows_its_received_size_on_the_row() {
        let mut behind = repo("behind");
        behind.behind = 3;
        let mut r = with_failed_fetch(behind, crate::git::FetchFailureKind::TimedOut);
        r.fetch.progress = Some(receiving(0));
        assert_eq!(rendered(&r).0, "⚠ timeout 3↓");
        r.fetch.progress = Some(receiving(312 * 1024 * 1024 + 5));
        let (text, width) = rendered(&r);
        assert_eq!(text, "fetching 312 MB 3↓");
        assert_eq!(width, text.chars().count(), "width matches what is drawn");
        let (spans, _) = badge_spans(&r, false);
        assert_eq!(spans[0].style.fg, Some(colors::SUBTEXT));
        r.fetch
            .record(&crate::git::FetchOutcome::Succeeded, chrono::Local::now());
        assert_eq!(rendered(&r).0, "3↓");
    }

    /// FZ-35b (#85), criteria 2 and 3: a held repo says how big and that
    /// `f` pulls it, in the failure marker's place and colour; a running `f`
    /// shows its progress over it, and its success clears it.
    #[test]
    fn a_repo_the_fetch_cap_holds_says_so_on_the_row() {
        const MIB: u64 = 1024 * 1024;
        let mut behind = repo("behind");
        behind.behind = 3;
        let mut grown = behind.clone();
        grown.fetch.disk_baseline = Some(1_000);
        grown
            .fetch
            .assess_growth(Some(1_000 + 300 * 1024), Some(200 * MIB));
        let (text, width) = rendered(&grown);
        assert_eq!(text, "fetch ~300 MB, press f 3↓");
        assert_eq!(width, text.chars().count(), "width matches what is drawn");
        let (spans, _) = badge_spans(&grown, false);
        assert_eq!(spans[0].style.fg, Some(colors::PEACH));

        let mut cut = behind;
        cut.fetch.record(
            &crate::git::FetchOutcome::Failed(crate::git::FetchFailure::too_big(200 * MIB + 7)),
            chrono::Local::now(),
        );
        assert_eq!(rendered(&cut).0, "fetch over 200 MB, press f 3↓");
        cut.fetch.progress = Some(receiving(48 * MIB));
        assert_eq!(rendered(&cut).0, "fetching 48 MB 3↓");
        cut.fetch
            .record(&crate::git::FetchOutcome::Succeeded, chrono::Local::now());
        assert_eq!(rendered(&cut).0, "3↓");
    }

    /// Nothing extra on the row while fetches work.
    #[test]
    fn a_successful_fetch_adds_nothing_to_the_row() {
        let mut r = repo("ok");
        r.fetch
            .record(&crate::git::FetchOutcome::Succeeded, chrono::Local::now());
        assert_eq!(rendered(&r).0, "✓");
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
            ..Default::default()
        }];

        let (text, _) = rendered(&busy);
        // Whole tokens, not substrings: what matters is what a complete badge
        // reads as, and a longer token may share a prefix with a duration
        // without being one (`1wt` contained `1w` before it was capitalised).
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
        assert!(text.contains("1WT"), "got {text:?}");
        // `2PR` is letters, but `PR` is not a duration, so it was never the
        // problem the other two were.
        assert!(text.contains("2PR"), "got {text:?}");
    }

    fn wt(dirty: Option<u32>) -> crate::model::Worktree {
        crate::model::Worktree {
            path: "~/repos/x-hotfix".to_string(),
            branch: "hotfix".to_string(),
            dirty,
            ..Default::default()
        }
    }

    fn wt_color(repo: &RepoStatus) -> Option<Color> {
        let (spans, _) = badge_spans(repo, false);
        spans
            .iter()
            .find(|s| s.content.contains("WT"))
            .expect("a WT span")
            .style
            .fg
    }

    #[test]
    fn a_dirty_linked_worktree_draws_the_badge_in_the_dirty_color() {
        let mut r = repo("wt");
        r.worktrees = vec![wt(Some(2))];
        assert_eq!(wt_color(&r), Some(counter_color(0, 0, 1)));
        assert_ne!(wt_color(&r), Some(colors::WORKTREE_CLR));

        // One clean and one dirty: any dirty worktree is enough.
        r.worktrees = vec![wt(Some(0)), wt(Some(3))];
        assert_eq!(wt_color(&r), Some(counter_color(0, 0, 1)));
    }

    #[test]
    fn a_clean_or_unknown_worktree_keeps_the_worktree_color() {
        for dirty in [Some(0), None] {
            let mut r = repo("wt");
            r.worktrees = vec![wt(dirty)];
            assert_eq!(wt_color(&r), Some(colors::WORKTREE_CLR), "{dirty:?}");
        }
    }

    #[test]
    fn a_linked_worktree_draws_its_count_in_the_worktree_color() {
        let mut with_worktree = repo("wt");
        with_worktree.worktrees = vec![crate::model::Worktree {
            path: "~/repos/x-hotfix".to_string(),
            branch: "hotfix".to_string(),
            ..Default::default()
        }];
        let (spans, _) = badge_spans(&with_worktree, false);
        let span = spans
            .iter()
            .find(|s| s.content == "1WT ")
            .expect("a 1WT span");
        assert_eq!(span.style.fg, Some(colors::WORKTREE_CLR));
        for excluded in [
            colors::OVERLAY,
            colors::SUBTEXT,
            colors::SURFACE,
            colors::RED,
            colors::PEACH,
            colors::YELLOW,
            colors::GREEN,
        ] {
            assert_ne!(
                span.style.fg,
                Some(excluded),
                "the count is information: not gray, not an alarm, not the synced green"
            );
        }

        let (spans, _) = badge_spans(&repo("plain"), false);
        assert!(spans.iter().all(|s| !s.content.contains("WT")));
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

    /// `status_color` is `counter_color` on a cloned repo's own counts, so a
    /// worktree's counters coloured from bare numbers wear the repo list's
    /// ladder; only not-cloned stays the repo's own case.
    #[test]
    fn counter_color_is_the_ladder_status_color_uses() {
        for (behind, ahead, dirty) in [
            (0, 0, 0),
            (3, 0, 0),
            (0, 2, 0),
            (0, 0, 4),
            (2, 1, 4),
            (0, 1, 4),
        ] {
            let mut r = repo("r");
            r.behind = behind;
            r.ahead = ahead;
            r.dirty = dirty;
            assert_eq!(counter_color(behind, ahead, dirty), status_color(&r));
        }
        assert_eq!(counter_color(0, 0, 4), colors::YELLOW);
        assert_eq!(counter_color(0, 0, 0), colors::GREEN);
    }

    #[test]
    fn a_bare_repo_renders_only_its_status() {
        // No leading or trailing padding when nothing optional is present, so
        // the token sits flush against the dot fill.
        let (text, width) = rendered(&repo("bare"));
        assert_eq!(text, "✓");
        assert_eq!(width, 1);
    }

    /// The detail pane's lamp pulses on the same phase as the repo list's.
    /// They show one repo's CI between them, so a lamp that blinks in one pane
    /// and holds still in the other reads as two states rather than one job.
    #[test]
    fn the_detail_pane_ci_lamp_pulses_while_pending() {
        assert_eq!(ci_display(&CiStatus::Pending, false).1, colors::PEACH);
        assert_eq!(ci_display(&CiStatus::Pending, true).1, colors::SURFACE);
    }

    /// Only pending pulses. Passed and failed are results rather than
    /// activity, and a blinking result reads as something still running.
    #[test]
    fn a_settled_ci_lamp_holds_still_through_the_blink() {
        for status in [CiStatus::Passed, CiStatus::Failed, CiStatus::Unknown] {
            assert_eq!(
                ci_display(&status, true).1,
                ci_display(&status, false).1,
                "{status:?} changed colour on the blink"
            );
        }
    }

    /// The six plain glyphs, through the choice functions with the plain set
    /// selected: the three kinds, the stash, and the watch eye open and shut.
    fn plain_glyphs() -> Vec<&'static str> {
        set_active_icons(Icons::Plain);
        let mut glyphs: Vec<&str> = [RepoKind::Org, RepoKind::Private, RepoKind::Public]
            .into_iter()
            .filter_map(kind_icon)
            .collect();
        glyphs.push(stash_icon());
        glyphs.push(crate::ui::watch_icon(true));
        glyphs.push(crate::ui::watch_icon(false));
        glyphs
    }

    /// Each plain glyph is one printable ASCII character: one column as
    /// ratatui measures it, and East Asian Narrow, never Ambiguous (the
    /// `●○•◆⊙` trap the graph's node tests describe) and never private-use.
    /// Fails if a stand-in is swapped for a wider, Ambiguous or Nerd Font
    /// character, or if one goes missing.
    #[test]
    fn the_plain_glyphs_are_one_column_ascii() {
        let glyphs = plain_glyphs();
        assert_eq!(glyphs.len(), 6);
        for glyph in glyphs {
            let mut chars = glyph.chars();
            let c = chars.next().expect("a glyph");
            assert!(chars.next().is_none(), "{glyph:?} is one character");
            assert!(c.is_ascii_graphic(), "{glyph:?} is printable ASCII");
            assert_eq!(Span::raw(glyph).width(), 1, "{glyph:?}");
        }
    }

    /// The plain glyphs cannot be mistaken for each other or for a mark a row
    /// already draws: the counters, the CI lamp, the fetch warning, the
    /// selection marker, the dot fill, the `WT` and `PR` letters and digits.
    /// Fails if a stand-in collides with any of them.
    #[test]
    fn the_plain_glyphs_differ_from_every_mark_on_a_row() {
        let row_marks = ['↓', '↑', '!', '✓', '◉', '⚠', '▸', '·', 'W', 'T', 'P', 'R'];
        let glyphs = plain_glyphs();
        let row_glyphs = &glyphs[..4];
        for glyph in row_glyphs {
            let c = glyph.chars().next().unwrap();
            assert!(!row_marks.contains(&c), "{glyph:?} is already a row mark");
            assert!(!c.is_ascii_alphanumeric(), "{glyph:?} reads as text");
        }
        for (i, a) in row_glyphs.iter().enumerate() {
            for b in &row_glyphs[i + 1..] {
                assert_ne!(a, b, "two row glyphs share a character");
            }
        }
        assert_ne!(glyphs[4], glyphs[5], "watch on and off look the same");
    }

    /// Without a call to `set_active_icons`, every choice is the Nerd Font
    /// set: `icons = "nerd"` is the default and draws what fleetz always drew.
    /// Reads the fresh test thread's state on purpose; that is its subject.
    #[test]
    fn the_default_set_is_the_nerd_font_set() {
        for kind in [
            RepoKind::Org,
            RepoKind::Private,
            RepoKind::Public,
            RepoKind::Unknown,
        ] {
            assert_eq!(kind_icon(kind), kind_glyph(kind));
        }
        assert_eq!(stash_icon(), STASH_GLYPH);
    }

    /// In `plain` the kind glyph keeps `GLYPH_W` and the badge group reports
    /// the width it draws, which the dot fill right-aligns the status by.
    /// Fails if a plain glyph measured more than one column.
    #[test]
    fn plain_badges_report_the_width_they_draw() {
        set_active_icons(Icons::Plain);
        let mut busy = repo("busy");
        busy.repo_kind = RepoKind::Private;
        busy.pr_count = 2;
        busy.stash_count = 3;
        busy.worktrees = vec![crate::model::Worktree {
            path: "~/repos/x-hotfix".to_string(),
            branch: "hotfix".to_string(),
            ..Default::default()
        }];
        let (text, width) = rendered(&busy);
        assert!(text.contains(&format!("3{PLAIN_STASH_GLYPH}")), "{text:?}");
        assert_eq!(width, text.chars().count(), "{text:?}");
        assert_eq!(width, Span::raw(text.clone()).width(), "{text:?}");

        let glyph = glyph_span(&busy);
        assert_eq!(glyph.width(), GLYPH_W);
        assert_eq!(glyph.content.chars().count(), GLYPH_W);
    }

    /// Only the colour moves. The detail pane right-aligns the group this sits
    /// in, so a phase that measured differently would shift the row beside it
    /// twice a second.
    #[test]
    fn the_ci_lamp_keeps_its_glyph_and_label_through_the_blink() {
        for status in [
            CiStatus::Passed,
            CiStatus::Failed,
            CiStatus::Pending,
            CiStatus::Unknown,
        ] {
            let (lit_glyph, _, lit_label) = ci_display(&status, false);
            let (dim_glyph, _, dim_label) = ci_display(&status, true);
            assert_eq!(lit_glyph, dim_glyph, "{status:?} changed glyph");
            assert_eq!(lit_label, dim_label, "{status:?} changed label");
        }
    }
}
