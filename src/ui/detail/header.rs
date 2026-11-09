use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::{fit_spans, headliner, pad_spans, right_spans, span_width};
use crate::app::App;
use crate::model::{CiStatus, RepoStatus};
use crate::ui::badges;
use crate::ui::colors;

/// PR numbers shown inline on the header's remote row before the rest are
/// summarised with a `+`. Two keeps the row short enough that CI still fits
/// beside it on a normal pane.
const PR_NUMBERS_SHOWN: usize = 2;

/// The detail pane's line under the header for a repo the last GitHub poll
/// did not return (`App::is_not_in_last_poll`): its CI and PRs are kept, not
/// cleared, since a blank lamp would read as "no CI". Provisional words (O4).
const EARLIER_REFRESH_NOTE: &str = "GitHub data from an earlier refresh: not in the last poll";

/// Minimum blank columns between the path and the right-aligned owner group.
/// Below this the two read as one run-on string, so the group is dropped.
const MIN_IDENTITY_GAP: usize = 2;

/// Blank columns to insert so the identity group lands flush right, or None
/// when the pane is too narrow to hold it with a readable gap (in which case
/// the caller drops the group entirely).
fn identity_gap(width: usize, left_w: usize, identity_w: usize) -> Option<usize> {
    width
        .checked_sub(left_w + identity_w)
        .filter(|gap| *gap >= MIN_IDENTITY_GAP)
}

/// The owner group shown flush right on the detail pane's status row: the
/// kind glyph plus the owner login. Empty when no owner is known (no GitHub
/// match and no parseable remote), which also suppresses the name row below
/// it, since `name` alone would just repeat the panel title.
///
/// The glyph and the owner share `badges::kind_color`, so org/private/public
/// reads the same here as in the repo list.
fn owner_spans(repo: &RepoStatus) -> (Vec<Span<'static>>, usize) {
    let Some(owner) = repo.owner_display() else {
        return (Vec::new(), 0);
    };

    let color = badges::kind_color(repo.repo_kind);
    let mut spans = Vec::new();

    if let Some(glyph) = badges::kind_icon(repo.repo_kind) {
        spans.push(Span::styled(
            format!("{glyph} "),
            Style::default().fg(color),
        ));
    }
    spans.push(Span::styled(
        owner,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ));

    let width = span_width(&spans);
    (spans, width)
}

/// The repo name, rendered flush right on the row under the owner. Paired
/// with `owner_spans`: empty whenever that one is, so the block is all or
/// nothing.
fn name_spans(repo: &RepoStatus) -> (Vec<Span<'static>>, usize) {
    if repo.owner_display().is_none() {
        return (Vec::new(), 0);
    }
    let spans = vec![Span::styled(
        repo.name.clone(),
        Style::default()
            .fg(colors::SAPPHIRE)
            .add_modifier(Modifier::BOLD),
    )];
    let width = span_width(&spans);
    (spans, width)
}

/// The header's CI word (FZ-83): `CI ◉ passed` while the default branch's
/// name is not known, `CI ◉ main passed` once it is, and the culprit after a
/// state that is not green, `CI ◉ main failed: Vercel`.
fn ci_header_text(sym: &str, label: &str, shown: &crate::app::CiShown) -> String {
    let mut text = match &shown.branch {
        Some(branch) => format!("CI {sym} {branch} {label}"),
        None => format!("CI {sym} {label}"),
    };
    if let Some(culprit) = &shown.culprit {
        text.push_str(": ");
        text.push_str(culprit);
    }
    text
}

/// Columns of the divider between two sections of pane 2's header.
const SECTION_DIVIDER: &str = " │ ";

/// The narrowest the local section of pane 2's header may be before the
/// header gives way to its plain two-sided rows. The remote section, the
/// other half, is at most a column narrower.
const MIN_LOCAL_SECTION: usize = 20;

/// Pane 2's header sections at one width (FZ-95).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Sections {
    local: usize,
    remote: usize,
}

/// The widths of pane 2's header sections at `width`, from the pane's
/// width alone, never from what a repo puts in them or from the window's
/// label on pane 1's border, so the divider stands in the same column for
/// every repo and every window (FZ-95). The divider splits the row in the
/// middle. `None` where the local half would fall under its floor; the
/// header then draws its plain two-sided rows.
fn section_widths(width: usize) -> Option<Sections> {
    let both = width.saturating_sub(SECTION_DIVIDER.chars().count());
    let (local, remote) = (both.div_ceil(2), both / 2);
    (local >= MIN_LOCAL_SECTION).then_some(Sections { local, remote })
}

/// The sections pane 2's header is drawn in for this frame, or `None` for
/// its plain two-sided rows: the `notes` layout, whose header sits left of
/// its divider, and a pane too narrow for them.
pub(super) fn header_sections(app: &App, width: usize) -> Option<Sections> {
    (!app.notes_layout())
        .then(|| section_widths(width))
        .flatten()
}

fn section_divider() -> Span<'static> {
    Span::styled(SECTION_DIVIDER, Style::default().fg(colors::SURFACE))
}

/// Pane 2's header in fixed sections (FZ-95): the repo's local facts
/// leaning left, its remote ones right-aligned against the pane's edge.
/// Every row draws the divider.
fn sectioned_rows(
    rows: Vec<(Vec<Span<'static>>, Vec<Span<'static>>)>,
    sections: Sections,
) -> Vec<Line<'static>> {
    rows.into_iter()
        .map(|(left, right)| {
            let mut line = pad_spans(fit_spans(left, sections.local), sections.local);
            line.push(section_divider());
            line.extend(right_spans(right, sections.remote));
            Line::from(line)
        })
        .collect()
}

/// The row above pane 2's sectioned header, on the pane's top padding: a
/// small subtitle over each section, `local` and `remote` (Andreas,
/// 2026-11-06), with the divider through it.
pub(super) fn subtitle_row(sections: Sections) -> Line<'static> {
    let style = Style::default().fg(colors::OVERLAY);
    let mut line = pad_spans(
        vec![Span::styled(HEADER_SUBTITLES[0], style)],
        sections.local,
    );
    line.push(section_divider());
    line.extend(right_spans(
        vec![Span::styled(HEADER_SUBTITLES[1], style)],
        sections.remote,
    ));
    Line::from(line)
}

/// The local and remote sections' subtitles.
const HEADER_SUBTITLES: [&str; 2] = ["local", "remote"];

/// The long notes a repo can carry (FZ-95): a failed fetch, git's reason
/// for it, the earlier-refresh note. Too long for a section, so the
/// sectioned header draws them on one banner row under its block
/// (`banner_line`) and `info` spells them out whole (`long_note_lines`).
pub(super) fn long_notes(app: &App, repo: &RepoStatus) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(text) = crate::ui::fetch_failure_line(&repo.fetch, chrono::Local::now()) {
        notes.push(text);
        if let Some(detail) = repo.fetch.failure().and_then(|f| f.detail.as_deref()) {
            notes.push(detail.to_string());
        }
    }
    if app.is_not_in_last_poll(repo) {
        notes.push(EARLIER_REFRESH_NOTE.to_string());
    }
    notes
}

/// The long notes on one orange row under the header's block (Andreas,
/// 2026-11-06: "a banner of sorts to put longer information like this"),
/// joined by ` · ` and cut with `…` at the pane's edge. No divider crosses
/// it. `None` when there are none.
fn banner_line(notes: &[String], width: usize) -> Option<Line<'static>> {
    if notes.is_empty() {
        return None;
    }
    let text = format!("  {}", notes.join(" · "));
    Some(Line::from(fit_spans(
        vec![Span::styled(text, Style::default().fg(colors::PEACH))],
        width,
    )))
}

/// The long notes whole, each on its own row, at the top of `info`'s body
/// while the header draws them cut on its banner: the full text stays on
/// screen somewhere.
pub(super) fn long_note_lines(notes: &[String]) -> Vec<Line<'static>> {
    if notes.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<Line<'static>> = notes
        .iter()
        .map(|note| {
            Line::from(Span::styled(
                note.clone(),
                Style::default().fg(colors::PEACH),
            ))
        })
        .collect();
    lines.push(Line::from(""));
    lines
}

/// A remote as the header names it (FZ-95): `owner/name` for GitHub, the
/// host before it for any other, never userinfo. Anything that is not a
/// host and exactly `owner/name` (a local path, a GitLab subgroup, an ssh
/// host alias) keeps its redacted URL (review 1, N2). Display only:
/// matching and `owner_display` read the stored URL.
fn remote_display(url: &str) -> String {
    let shown = crate::git::redact_userinfo(url);
    let Some(remote) = crate::model::RemoteRef::parse(&shown) else {
        return shown;
    };
    let host = remote.host.to_ascii_lowercase();
    // `RemoteRef::parse` reads the first two path segments and ignores the
    // rest, so a deeper path ends in something else; a host with no dot is
    // an ssh alias, not a place.
    let tail = shown.trim().trim_end_matches('/');
    let tail = tail.strip_suffix(".git").unwrap_or(tail);
    let pair = format!("{}/{}", remote.owner, remote.name);
    // What is left before `/owner/name` must be the host itself, with at
    // most a scheme, a user and a port around it.
    let authority = tail
        .strip_suffix(pair.as_str())
        .and_then(|before| before.strip_suffix(['/', ':']))
        .map(|before| before.rsplit_once("://").map_or(before, |(_, a)| a))
        .map(|a| a.rsplit_once('@').map_or(a, |(_, h)| h))
        .map(|a| a.split_once(':').map_or(a, |(h, _)| h));
    if host.is_empty() || !host.contains('.') || authority != Some(remote.host.as_str()) {
        return shown;
    }
    if host == "github.com" || host == "www.github.com" {
        format!("{}/{}", remote.owner, remote.name)
    } else {
        format!("{}/{}/{}", remote.host, remote.owner, remote.name)
    }
}

pub(super) fn header_lines(app: &App, repo: &RepoStatus, width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    // The pane's header block reads left-to-right as local-to-remote: what is
    // on this disk sits on the left of each row, what is on GitHub sits flush
    // right. The identity pair already worked that way; the rest of the block
    // now says so out loud rather than mixing "how it is doing" with "where
    // it lives" in one run of tokens.
    //
    //   ○ 3 behind, 1 ahead                               ● Gaurgle
    //   ~/repos/fleetz              git@github.com:Gaurgle/fleetz.git
    //   * main · 1 stash          2 open PRs #13 · CI ◉ main passed
    //
    // Each row right-aligns its own remote half independently and drops it
    // when it will not fit. Owner and name used to be placed together or not
    // at all, because they were one fact stacked over two rows; row two now
    // carries the remote URL instead, which spells the name out anyway.
    let status_color = badges::status_color(repo);
    // `✓` when synced, matching the list badge, and never `◉`: the ringed disk
    // belongs to CI alone now.
    let status_icon = if repo.is_synced() { "✓" } else { "○" };
    let path_str = repo.path.as_deref().unwrap_or("not cloned");

    // Row 1 left: how it is doing relative to the remote.
    let row1_left = vec![Span::styled(
        format!("{status_icon} {}", repo.long_status()),
        Style::default().fg(status_color),
    )];

    // Under row 1, while the last fetch failed: the counts beside it are
    // against the remote refs of the last fetch that worked, and when that
    // was. Then git's own reason, already stripped of credentials.
    let mut fetch_rows: Vec<Line> = Vec::new();
    if let Some(text) = crate::ui::fetch_failure_line(&repo.fetch, chrono::Local::now()) {
        fetch_rows.push(Line::from(Span::styled(
            format!("  {text}"),
            Style::default().fg(colors::PEACH),
        )));
        if let Some(detail) = repo.fetch.failure().and_then(|f| f.detail.as_deref()) {
            fetch_rows.push(Line::from(Span::styled(
                format!("    {detail}"),
                Style::default().fg(colors::OVERLAY),
            )));
        }
    }

    // Row 2 left: where it lives on this machine.
    let row2_left = vec![Span::styled(
        format!("  {path_str}"),
        Style::default().fg(colors::OVERLAY),
    )];

    // Row 3 left: facts that exist only on this disk.
    let mut row3_left: Vec<Span> = Vec::new();
    if let Some(head) = repo.branches.iter().find(|b| b.is_head) {
        row3_left.push(Span::styled(
            "  * ",
            Style::default().fg(colors::ref_color(crate::model::RefKind::Head)),
        ));
        row3_left.push(Span::styled(
            head.name.clone(),
            Style::default().fg(colors::ref_color(crate::model::RefKind::Head)),
        ));
    }
    // Stashes carry the same glyph the repo list uses, so the eye can find
    // them here the way it learned to find them there. Neither is drawn in
    // OVERLAY: that is for paths and furniture, and at that brightness real
    // news reads as decoration you have already dismissed.
    // Each carries the colour its badge uses (one colour per concept): the
    // stash count is STASH_CLR and the worktree count is WORKTREE_CLR, so the
    // header fact is recognised as the same thing the list row flags.
    let mut local_extra: Vec<(String, ratatui::style::Color)> = Vec::new();
    if repo.stash_count > 0 {
        let noun = if repo.stash_count == 1 {
            "stash"
        } else {
            "stashes"
        };
        local_extra.push((
            format!("{} {} {noun}", badges::stash_icon(), repo.stash_count),
            colors::STASH_CLR,
        ));
    }
    if !repo.worktrees.is_empty() {
        let noun = if repo.worktrees.len() == 1 {
            "worktree"
        } else {
            "worktrees"
        };
        local_extra.push((
            format!("{} {noun}", repo.worktrees.len()),
            colors::WORKTREE_CLR,
        ));
    }
    for (extra, color) in local_extra {
        if row3_left.is_empty() {
            row3_left.push(Span::raw("  "));
        } else {
            row3_left.push(Span::styled(" · ", Style::default().fg(colors::SURFACE)));
        }
        row3_left.push(Span::styled(extra, Style::default().fg(color)));
    }

    // Row 3 right: facts that live on GitHub. From an earlier refresh when
    // the last poll did not return this repo: the same values, in the list's
    // grey, the lamp held still.
    let not_in_last_poll = app.is_not_in_last_poll(repo);
    let mut row3_right: Vec<Span> = Vec::new();
    if repo.pr_count > 0 {
        let label = if repo.pr_count == 1 { "PR" } else { "PRs" };
        let mut text = format!("{} open {label}", repo.pr_count);
        // The numbers themselves, so the pane names the PR rather than only
        // counting it. Capped so a busy repo cannot push CI off the row.
        if !repo.prs.is_empty() {
            let shown: Vec<String> = repo
                .prs
                .iter()
                .take(PR_NUMBERS_SHOWN)
                .map(|pr| format!("#{}", pr.number))
                .collect();
            text.push(' ');
            text.push_str(&shown.join(" "));
            if repo.prs.len() > PR_NUMBERS_SHOWN {
                text.push('+');
            }
        }
        let pr_color = if not_in_last_poll {
            badges::EARLIER_REFRESH
        } else {
            colors::PR_CLR
        };
        row3_right.push(Span::styled(text, Style::default().fg(pr_color)));
    }
    let shown = app.ci_shown(repo);
    if shown.status != CiStatus::Unknown {
        let (ci_sym, ci_color, ci_label) =
            badges::ci_display(&shown.status, app.watch_mode && !app.blink_on);
        let ci_color = if not_in_last_poll {
            badges::EARLIER_REFRESH
        } else {
            ci_color
        };
        if !row3_right.is_empty() {
            row3_right.push(Span::styled(" · ", Style::default().fg(colors::SURFACE)));
        }
        row3_right.push(Span::styled(
            ci_header_text(ci_sym, ci_label, &shown),
            Style::default().fg(ci_color),
        ));
    }

    // Right halves. Owner on row 1, the remote locator on row 2, GitHub's own
    // state on row 3.
    let (owner_group, owner_w) = owner_spans(repo);
    // Drawn without userinfo, so a token kept in an HTTPS remote never
    // reaches the screen. Display only: matching and `owner_display` read
    // the stored URL.
    let (remote_loc, remote_loc_w) = match repo.remote_url.as_deref() {
        Some(url) => {
            let shown = remote_display(url);
            let shown_w = shown.chars().count();
            (
                vec![Span::styled(shown, Style::default().fg(colors::OVERLAY))],
                shown_w,
            )
        }
        None => name_spans(repo),
    };
    let row3_right_w = span_width(&row3_right);

    let note = not_in_last_poll.then(|| {
        vec![Span::styled(
            format!("  {EARLIER_REFRESH_NOTE}"),
            Style::default().fg(badges::EARLIER_REFRESH),
        )]
    });
    // Outside the `notes` layout, whose header sits left of its divider,
    // fixed sections (FZ-95): local and remote.
    if let Some(sections) = header_sections(app, width) {
        let mut rows = vec![(row1_left, owner_group), (row2_left, remote_loc)];
        if !row3_left.is_empty() || !row3_right.is_empty() {
            rows.push((row3_left, row3_right));
        }
        lines.extend(sectioned_rows(rows, sections));
        // The fetch failure, its git detail and the earlier-refresh note do
        // not fit a section: one banner row under the block, cut at the
        // pane's edge, no divider on it; `info` has them whole (Andreas,
        // 2026-11-06, replacing "whole, under the block").
        lines.extend(banner_line(&long_notes(app, repo), width));
        lines.push(Line::from(""));
    } else {
        let mut row1 = row1_left;
        let mut row2 = row2_left;
        let mut row3 = row3_left;
        for (line, right, right_w) in [
            (&mut row1, owner_group, owner_w),
            (&mut row2, remote_loc, remote_loc_w),
            (&mut row3, row3_right, row3_right_w),
        ] {
            if right.is_empty() {
                continue;
            }
            if let Some(gap) = identity_gap(width, span_width(line), right_w) {
                line.push(Span::raw(" ".repeat(gap)));
                line.extend(right);
            }
        }
        lines.push(Line::from(row1));
        lines.extend(fetch_rows);
        lines.push(Line::from(row2));
        if !row3.is_empty() {
            lines.push(Line::from(row3));
        }
        lines.extend(note.map(Line::from));
        lines.push(Line::from(""));
    }
    // The view's own title, in the colour its tab wears up in the frame. The
    // tab is what you aim at; this is what confirms you landed, and saying it
    // twice in two colours would have made the pair read as two things.
    lines.extend(headliner(app.history_view, width));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RepoKind;
    use crate::ui::detail::pane::render_detail_pane;
    use crate::ui::detail::tests::{
        app_with_a_stale_row, cloned_repo, frame_rows, header_rows, pane_two_dividers, pane2_rows,
        repo,
    };
    use ratatui::layout::Rect;
    use ratatui::style::Color as TermColor;

    /// The reported width must match what actually gets drawn, or the group
    /// stops being flush right.
    fn drawn_width(spans: &[Span<'_>]) -> usize {
        spans.iter().map(|s| s.content.chars().count()).sum()
    }

    #[test]
    fn identity_splits_owner_above_name() {
        let r = repo("fleetz", RepoKind::Public, Some("Gaurgle"));
        let (owner, owner_w) = owner_spans(&r);
        let (name, name_w) = name_spans(&r);

        let owner_text: String = owner.iter().map(|s| s.content.as_ref()).collect();
        let name_text: String = name.iter().map(|s| s.content.as_ref()).collect();
        assert!(owner_text.ends_with("Gaurgle"), "got {owner_text:?}");
        assert!(
            !owner_text.contains("fleetz"),
            "name belongs on its own row: {owner_text:?}"
        );
        assert_eq!(name_text, "fleetz");

        assert_eq!(owner_w, drawn_width(&owner));
        assert_eq!(name_w, drawn_width(&name));
    }

    #[test]
    fn identity_is_empty_without_an_owner() {
        // Name alone would just repeat the panel title, so neither row
        // renders: the block is all or nothing.
        let r = repo("fleetz", RepoKind::Unknown, None);
        assert!(owner_spans(&r).0.is_empty());
        assert_eq!(owner_spans(&r).1, 0);
        assert!(name_spans(&r).0.is_empty());
        assert_eq!(name_spans(&r).1, 0);
    }

    #[test]
    fn identity_falls_back_to_the_remote_url() {
        let mut r = repo("fleetz", RepoKind::Unknown, None);
        r.remote_url = Some("git@github.com:Gaurgle/fleetz.git".to_string());
        let owner_text: String = owner_spans(&r)
            .0
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(owner_text, "Gaurgle");
    }

    #[test]
    fn identity_kind_glyph_is_only_drawn_when_known() {
        let (org, org_w) = owner_spans(&repo("svc", RepoKind::Org, Some("acme")));
        let (unknown, unknown_w) = owner_spans(&repo("svc", RepoKind::Unknown, Some("acme")));
        // Same text, but the org row carries the leading building glyph.
        assert_eq!(org_w - unknown_w, badges::GLYPH_W);
        assert_eq!(org_w, drawn_width(&org));
        assert_eq!(unknown_w, drawn_width(&unknown));
    }

    #[test]
    fn identity_gap_pushes_the_group_flush_right() {
        // 40 wide, 10 used on the left, 14 for the group: 16 blank between.
        assert_eq!(identity_gap(40, 10, 14), Some(16));
    }

    #[test]
    fn identity_gap_drops_the_group_when_it_would_not_fit() {
        // Exactly filling the row leaves no separation, so it is dropped
        // rather than run on against the path.
        assert_eq!(identity_gap(24, 10, 14), None);
        assert_eq!(identity_gap(25, 10, 14), None, "one column is not a gap");
        assert_eq!(identity_gap(26, 10, 14), Some(2), "two columns is enough");
        // Overflowing must not underflow the subtraction.
        assert_eq!(identity_gap(20, 10, 14), None);
    }

    /// FZ-95: the header's two sections depend on the pane's width alone,
    /// so the divider stands still between repos and windows: the row is
    /// split in the middle at every width, and below the local floor there
    /// are no sections at all (his look, 2026-11-05: "static in size, only
    /// depending on window width"; the window moved to pane 1's border on
    /// 2026-11-09, so no label length enters them).
    #[test]
    fn the_sections_depend_on_the_pane_width_alone() {
        let mut last = 0;
        for width in [42, 44, 60, 81, 120, 140, 160] {
            let s = section_widths(width).unwrap_or_else(|| panic!("sections at {width}"));
            assert_eq!(s.local + 3 + s.remote, width, "rows fill the pane: {s:?}");
            assert!(
                s.local >= s.remote && s.local - s.remote <= 1,
                "the divider in the middle at {width}: {s:?}"
            );
            assert!(s.local > last, "they grow with the pane: {s:?}");
            last = s.local;
        }
        // The local half one column under its floor: plain rows.
        assert_eq!(section_widths(2 * (MIN_LOCAL_SECTION - 1) + 3), None);
        assert!(section_widths(2 * MIN_LOCAL_SECTION - 1 + 3).is_some());
    }

    #[test]
    /// On a narrow pane the local half always survives: it is the half you
    /// cannot look up anywhere else. Since FZ-95 the remote section has a
    /// fixed width, so a remote longer than it is cut with `…` at the
    /// pane's edge, never run past it (review 1, N1); where the sections
    /// give way and the plain rows have no room, it is dropped whole.
    fn identity_is_dropped_whole_on_a_narrow_pane() {
        let full = "Gaurgle/a-much-longer-repository-name";
        let long = || {
            let mut r = cloned_repo();
            r.remote_url = Some(format!("git@github.com:{full}.git"));
            r
        };
        let mut cut_seen = false;
        for width in [24u16, 30, 34, 48, 60, 80, 120] {
            let (_, location, _) = header_rows(long(), width);
            assert!(
                location.contains("~/repos/fleetz"),
                "w{width}: local path lost: {location:?}"
            );
            let app = crate::app::App::new(Vec::new(), "test".to_string(), width, 40);
            let sections = header_sections(&app, usize::from(width - 4)).filter(|s| s.remote > 0);
            let Some(s) = sections else {
                assert!(
                    !location.contains("Gaurgle/"),
                    "w{width}: remote should have been dropped: {location:?}"
                );
                continue;
            };
            // The row as drawn starts at the pane's padding column.
            let chars: Vec<char> = location.chars().collect();
            let start = 1 + s.local + SECTION_DIVIDER.chars().count();
            let remote: String = chars[start..start + s.remote].iter().collect();
            // `header_rows` trims the row's end, so the remote section's
            // last column is the row's last.
            assert_eq!(
                chars.len(),
                start + s.remote,
                "w{width}: the remote section moved: {location:?}"
            );
            let shown = remote.trim_start();
            if shown != full {
                let kept = shown.strip_suffix('…').expect("cut with …");
                assert!(full.starts_with(kept), "w{width}: {shown:?}");
                cut_seen = true;
            }
        }
        assert!(cut_seen, "no width cut the remote");
        // And at a width with no room for it, it really is gone.
        let (_, location, _) = header_rows(long(), 34);
        assert!(
            !location.contains("Gaurgle/"),
            "remote should have been dropped: {location:?}"
        );
    }

    /// FZ-95 review 1, B3 and N2: every GitHub form reads `owner/name`, any
    /// other host keeps its host, a credential never shows, and a remote
    /// that is not a host and exactly `owner/name` keeps its redacted URL.
    #[test]
    fn remote_display_names_owner_and_name_in_every_form() {
        for (url, want) in [
            ("https://github.com/o/r.git", "o/r"),
            ("https://www.github.com/o/r", "o/r"),
            ("git@github.com:o/r.git", "o/r"),
            ("ssh://git@github.com/o/r", "o/r"),
            ("ssh://git@github.com:22/o/r.git", "o/r"),
            ("https://gitlab.com/o/r", "gitlab.com/o/r"),
            ("git@gitlab.com:o/r.git", "gitlab.com/o/r"),
            ("https://user:tok3n@github.com/o/r.git", "o/r"),
            ("https://tok3n@www.github.com/o/r", "o/r"),
            ("ssh://git:tok3n@github.com/o/r", "o/r"),
            ("https://user:tok3n@gitlab.com/o/r.git", "gitlab.com/o/r"),
        ] {
            assert_eq!(remote_display(url), want, "{url}");
        }
        for (url, want) in [
            ("file:///srv/git/o/r.git", "file:///srv/git/o/r.git"),
            (
                "https://gitlab.com/group/sub/r.git",
                "https://gitlab.com/group/sub/r.git",
            ),
            ("myalias:o/r.git", "myalias:o/r.git"),
            (
                "https://user:tok3n@gitlab.com/group/sub/r",
                "https://gitlab.com/group/sub/r",
            ),
            (
                "https://user:tok3n@github.com/o/r/tree/main",
                "https://github.com/o/r/tree/main",
            ),
        ] {
            assert_eq!(remote_display(url), want, "{url}");
            assert!(!remote_display(url).contains("tok3n"), "{url}");
        }
    }

    /// FZ-95: on a narrow pane the header keeps local and remote, and the
    /// window stays off it (it is on pane 1's border).
    #[test]
    fn a_narrow_header_keeps_its_sections() {
        let s = section_widths(60).expect("sections at 60");
        assert!(s.remote >= MIN_LOCAL_SECTION - 1, "{s:?}");
        let rows = pane2_rows(cloned_repo(), 64, |_| {});
        assert!(rows[2].contains("~/repos/fleetz"), "{rows:#?}");
        assert!(rows[2].contains("Gaurgle/fleetz"), "{rows:#?}");
        assert!(!rows.iter().any(|r| r.contains('◷')), "{rows:#?}");
    }

    /// FZ-95: the long notes join on one banner row, cut at the pane's
    /// width.
    #[test]
    fn the_banner_is_one_row_cut_with_an_ellipsis() {
        assert_eq!(banner_line(&[], 40), None);
        let notes = vec!["⚠ fetch failed: timed out".to_string(), "x".repeat(60)];
        let line = banner_line(&notes, 40).expect("a banner");
        let text = line.to_string();
        assert_eq!(text.chars().count(), 40, "{text:?}");
        assert!(
            text.starts_with("  ⚠ fetch failed: timed out · x"),
            "{text:?}"
        );
        assert!(text.ends_with('…'), "{text:?}");
        assert!(line.spans.iter().all(|s| s.style.fg == Some(colors::PEACH)));
    }

    /// FZ-95 (Andreas, 2026-11-09): the window on pane 1's top border,
    /// `[1] repos · ◷ since 2 weeks [t]`, and pane 2's header two sections
    /// split in the middle, one divider.
    #[test]
    fn the_window_is_on_pane_ones_border_and_pane_two_has_two_sections() {
        let mut app =
            crate::app::App::new(vec![cloned_repo()], "since 2 weeks".to_string(), 160, 30);
        app.view = crate::app::View::Detail;
        let (rows, _) = frame_rows(&app, 160, 30);
        assert!(
            rows[1].contains("[1] repos · ◷ since 2 weeks [t]"),
            "{:?}",
            rows[1]
        );
        let pane2 = rows
            .iter()
            .position(|r| r.contains("Gaurgle/fleetz"))
            .expect("pane 2's location row");
        // Pane 2's own section divider, once, in the middle of its row.
        let detail = crate::ui::panel_rects(Rect::new(0, 0, 160, 30), &app)
            .detail
            .expect("pane 2");
        let s2 = section_widths(usize::from(detail.width - 4)).expect("sections");
        assert!(s2.local.abs_diff(s2.remote) <= 1, "{s2:?}");
        assert_eq!(
            pane_two_dividers(&rows[pane2], detail),
            vec![usize::from(detail.x) + 2 + s2.local + 1],
            "{:?}",
            rows[pane2]
        );
    }

    /// The detail pane of the selected repo, as text, and the colour of its
    /// CI lamp.
    fn detail_of(app: &crate::app::App) -> (String, TermColor) {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (w, h) = (100u16, 24u16);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), app)).unwrap();
        let buf = term.backend().buffer().clone();
        let text = (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        let lamp = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .find(|&(x, y)| buf[(x, y)].symbol() == "◉")
            .map(|(x, y)| buf[(x, y)].fg)
            .expect("lamp drawn");
        (text, lamp)
    }

    #[test]
    fn the_detail_pane_says_a_repo_not_in_the_last_poll_is_from_an_earlier_refresh() {
        let mut app = app_with_a_stale_row();
        app.blink_on = true;
        // The first row, `stale`, is selected.
        let (text, lamp) = detail_of(&app);
        assert!(text.contains(EARLIER_REFRESH_NOTE), "{text}");
        assert_eq!(lamp, badges::EARLIER_REFRESH);

        app.not_in_last_poll.clear();
        let (text, lamp) = detail_of(&app);
        assert!(!text.contains(EARLIER_REFRESH_NOTE), "{text}");
        assert_eq!(lamp, colors::PEACH);
    }
}
