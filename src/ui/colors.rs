use ratatui::style::Color;

/// Catppuccin Mocha palette: matches repoz bash script colors.
pub const PEACH: Color = Color::Rgb(250, 179, 135); // behind / warnings
pub const GREEN: Color = Color::Rgb(166, 227, 161); // ahead / synced
pub const YELLOW: Color = Color::Rgb(249, 226, 175); // uncommitted
pub const SAPPHIRE: Color = Color::Rgb(116, 199, 236); // repo names
pub const LAVENDER: Color = Color::Rgb(180, 190, 254); // headers
pub const MAUVE: Color = Color::Rgb(203, 166, 247); // section labels
pub const MAROON: Color = Color::Rgb(235, 160, 172); // the `ci` view's tab
pub const PINK: Color = Color::Rgb(245, 194, 231); // the `worktrees` view's tab

/// Open pull requests, everywhere one is counted or listed.
///
/// A warmer orange than `PEACH`, which already means behind/ahead in the
/// counters, so a PR badge cannot be mistaken for a sync state at a glance.
/// Split out of `MAUVE` because that is the section-label colour: a PR count
/// wearing it read as furniture rather than as news.
pub const PR_CLR: Color = Color::Rgb(240, 152, 88);
/// Commit hashes, everywhere one is drawn: the history graph, the reflog,
/// and the recent-commits list.
///
/// Gold rather than the grey-blue this used to be, because `git log`'s own
/// hash colour is yellow and the pane is read next to a terminal that draws
/// it that way. Deliberately *not* `YELLOW`: that one already means
/// "uncommitted" in the badges and the footer, and a hash sharing it would
/// put two meanings on one colour.
pub const HASH_CLR: Color = Color::Rgb(214, 179, 106);
pub const DIFF_ADD: Color = Color::Rgb(134, 194, 134); // +N lines
pub const DIFF_DEL: Color = Color::Rgb(210, 140, 140); // -N lines
pub const OVERLAY: Color = Color::Rgb(127, 132, 156); // paths, subtle text

/// Catppuccin `subtext0`. For text that should recede without becoming work to
/// read: hints, key legends, the swept-ago clock.
///
/// `SURFACE` used to do this job as well as its own, but it is tuned for
/// *structure* (dots, dividers, borders), where near-invisible is the point. At
/// that brightness actual words are a strain, so the two uses are separate
/// constants now. Rule of thumb: if it spells something, it is not SURFACE.
pub const SUBTEXT: Color = Color::Rgb(166, 173, 200);

pub const SURFACE: Color = Color::Rgb(69, 71, 90); // dots, dividers, borders

/// Background of the selected row in the repo pane's history lists, and of
/// the cursor's row in the repo list (FZ-66).
///
/// A whole-row wash rather than a bolded hash. Bold marks one token; the eye
/// still has to find which token, and in a list two hundred rows long that is
/// the work. A band says it at a glance, which is what lazygit does and why
/// its lists read so quickly. A lift off the pane rather than a colour, so it
/// cannot be mistaken for status.
///
/// Catppuccin `surface0` (49, 50, 68), halfway from `surface1` (`SURFACE`) to
/// the base (30, 30, 46): "the list highlight can be 50% more dimmed"
/// (Andreas, 2026-11-04). Strictly between the two, so it stays a visible lift
/// but is no longer `SURFACE`'s RGB.
pub const SELECTED_BG: Color = Color::Rgb(49, 50, 68);

/// The cursor row's background for the one flash after `yy` or `yp`
/// (FZ-81), swept over the band and then faded back to `SELECTED_BG`.
///
/// Catppuccin `surface2`, the colour notez's copy flash uses: a clear step
/// up from the band and from `SURFACE`, still well below `OVERLAY` so the
/// row's own text stays readable for the 300 ms it lasts.
pub const YANKED_BG: Color = Color::Rgb(88, 91, 112);

/// `from` moved `fraction` (clamped to 0..=1) of the way to `to`, channel by
/// channel. A colour that is not RGB does not mix: `to` is returned.
pub fn mix(from: Color, to: Color, fraction: f64) -> Color {
    let (Color::Rgb(r0, g0, b0), Color::Rgb(r1, g1, b1)) = (from, to) else {
        return to;
    };
    let t = fraction.clamp(0.0, 1.0);
    let channel = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as u8;
    Color::Rgb(channel(r0, r1), channel(g0, g1), channel(b0, b1))
}

/// Stashed work, in the repo list's badge and in the repo pane alike.
///
/// Catppuccin `flamingo`. Grey was the wrong answer twice over: `OVERLAY` is
/// the colour of paths and other things shown because they must be, so a
/// stash rendered in it reads as "nothing to see", and the badge and the pane
/// have to agree anyway. Warm rather than cool because a stash is your own
/// parked work, and adjacent to `YELLOW` without being it: uncommitted
/// changes and changes you deliberately put away are neighbours, not the same
/// state. Clear of `RED`, which is CI failing.
pub const STASH_CLR: Color = Color::Rgb(242, 205, 205);

/// The linked-worktree count in the repo list's badge.
///
/// An alias of `LAVENDER`, no new color. Neither an alarm color nor gray: the
/// count is information, not a state that needs action, so it avoids
/// `PEACH`, `RED` and `YELLOW`, and `OVERLAY` made it read as a footnote.
pub const WORKTREE_CLR: Color = LAVENDER;

/// Contribution intensity for the GitHub source, empty through busiest.
///
/// Grown out of github.com's dark-dimmed graph rather than sampled from it.
/// The sampled ladder decelerated -- 1.51x, 1.42x, then 1.34x per step -- so
/// the three busiest levels were the *closest together*, which is the wrong
/// way round for a heatmap: the days worth spotting were the hardest to tell
/// apart. This one spans 3.31x from quietest to busiest, against 2.86x before,
/// and its four steps are evenly spaced in CIELAB L* -- 18.5, 19.0, 18.6 apart
/// -- which is the spacing the eye actually reads. An even *luminance ratio*
/// was tried first and looked wrong: it puts L* gaps of 13.7, 19.3, 23.0 on
/// the ladder, so the middles sat too dark and the jump to the top step was
/// nearly twice the jump off the bottom.
///
/// Step 1 is GitHub's own, unchanged, so a quiet day still looks like a quiet
/// day. Step 0 is GitHub's empty-square slate, a lift above the page rather
/// than a colour, so an empty day stays structure rather than data.
///
/// The extra range is taken in saturation as well as lightness (0.31 at step
/// 1 ramping to 0.58 at step 4), because lightness alone cannot deliver it: a
/// fully saturated green tops out near luma 182 and step 4 was already at
/// 174.7, so brightening in lightness alone desaturates toward white and the
/// busiest day arrives as pale mint. Hue is held at GitHub's, which is what
/// keeps this reading as the same sage family rather than the saturated
/// standard-dark set, which reads as a different product.
const CONTRIB_LADDER_GITHUB: [Color; 5] = [
    Color::Rgb(43, 49, 59),
    Color::Rgb(39, 70, 37),
    Color::Rgb(59, 120, 52),
    Color::Rgb(75, 176, 60),
    Color::Rgb(149, 224, 136),
];

/// The same ladder for the local source, translated to blue. The two sources
/// count different things and will never agree, so they should not look
/// identical: the colour says which one you are reading before the title
/// does. Blue rather than red because red already means "CI failed".
///
/// Shares the green's empty slate and its step 1, then climbs evenly to a top
/// that is still recognisably blue. Saturation ramps 0.31 to a full 1.0.
///
/// This ladder deliberately does *not* match the green's luminance step for
/// step, which it used to. Blue carries 0.0722 of luminance against green's
/// 0.7152, so reaching a green's luminance drives blue far lighter, and sRGB
/// has no colour left up there to spend: at the green top's luminance of 202
/// the bluest colour that exists is (166, 208, 255), chroma 89, which reads as
/// paper. Two attempts died on this. Taking the green's saturation gave
/// (179, 205, 235); taking full saturation gave (166, 208, 255). Both are the
/// best answer to the wrong question.
///
/// So the blue tops out at luminance 165, where full saturation still buys
/// chroma 151, reaching it over the same evenly spaced L* steps (14.5, 14.7,
/// 14.2) at an average 1.390x luminance per step against the green's 1.490x. That 6.5% is the whole cost, and it is the right thing to spend:
/// the ladders are never on screen together (`c` toggles the source, and the
/// legend names it), so what matters is that each reads evenly against itself.
/// A top step that has turned white has stopped reading as data at all.
///
/// Do not "restore" luminance parity without re-checking chroma. A plain hue
/// rotation, tried before any of this, put step 1 within 4% of the empty
/// slate, which is itself blue-hued: a whole intensity level vanished.
const CONTRIB_LADDER_LOCAL: [Color; 5] = [
    Color::Rgb(43, 49, 59),
    Color::Rgb(45, 64, 84),
    Color::Rgb(47, 98, 157),
    Color::Rgb(54, 134, 229),
    Color::Rgb(104, 174, 255),
];

/// The intensity ladder for a source. One function, so the cells and the
/// Less/More legend cannot drift apart, and so adding a source cannot leave
/// half the strip on the old palette.
pub fn contrib_ladder(source: crate::contributions::Source) -> [Color; 5] {
    match source {
        crate::contributions::Source::Github => CONTRIB_LADDER_GITHUB,
        crate::contributions::Source::Local => CONTRIB_LADDER_LOCAL,
    }
}
pub const TEXT: Color = Color::Rgb(205, 214, 244); // default text
pub const RED: Color = Color::Rgb(243, 139, 168); // CI failed

/// The two phases of the watch-mode pulse: Catppuccin `red` and `maroon`, a
/// half-step apart. Two neighbouring hues rather than a colour and a dimmed
/// twin, because the indicator's job is to say the dashboard is live, and a
/// pulse that drops towards the background reads as something failing to
/// draw rather than as a heartbeat.
///
/// `WATCH_ON` is the same value as `RED` and is deliberately not that
/// constant: `RED` means CI failed, and one name carrying two meanings is how
/// a palette starts drifting. If either changes it should be free to.
pub const WATCH_ON: Color = Color::Rgb(243, 139, 168); // Catppuccin red
pub const WATCH_OFF: Color = Color::Rgb(235, 160, 172); // Catppuccin maroon

/// Muted, low-saturation hues for per-author commit coloring -- distinct
/// enough to tell people apart at a glance, dull enough not to compete with
/// the rest of the palette (unlike a flat grey, which reads as "everyone is
/// the same person").
const AUTHOR_PALETTE: [Color; 6] = [
    Color::Rgb(190, 140, 140), // dull red
    Color::Rgb(140, 170, 140), // dull green
    Color::Rgb(160, 150, 190), // dull violet
    Color::Rgb(140, 160, 180), // dull blue
    Color::Rgb(190, 165, 130), // dull amber
    Color::Rgb(175, 140, 160), // dull rose
];

/// Deterministically pick a dull color for an author name, so the same
/// person always gets the same color across a session (and across repos).
pub fn author_color(name: &str) -> Color {
    let hash = name
        .bytes()
        .fold(5381u32, |h, b| h.wrapping_mul(33).wrapping_add(b as u32));
    AUTHOR_PALETTE[hash as usize % AUTHOR_PALETTE.len()]
}

/// Distinct hues per graph "lane", in the rotation `git log --graph --color`
/// uses, so a topology read here and in a terminal side by side has its lanes
/// in the same order.
///
/// Saturated rather than the muted set this started as: the lanes are the
/// structure of the pane, and at low saturation parallel branches were only
/// nominally distinguishable. Held at Catppuccin brightness instead of raw
/// ANSI so the graph still belongs to the rest of the palette. Kept clear of
/// `AUTHOR_PALETTE`, which is dull on purpose, so the lane and the author
/// code on the same row never read as one colour.
const GRAPH_PALETTE: [Color; 6] = [
    Color::Rgb(137, 220, 235), // sky
    Color::Rgb(250, 179, 135), // peach
    Color::Rgb(166, 227, 161), // green
    Color::Rgb(245, 194, 231), // pink
    Color::Rgb(249, 226, 175), // yellow
    Color::Rgb(137, 180, 250), // blue
];

/// Colour for one commit decoration, by what the ref actually is.
///
/// The four kinds answer different questions -- where am I, what do I have,
/// what does the remote have, what is pinned -- so they get four colours
/// rather than the single flat mauve the whole `(...)` blob used to take.
/// The pairings are `git log --decorate`'s own: HEAD teal, local green,
/// remote peach (git uses red, which means "CI failed" here), tag yellow.
pub fn ref_color(kind: crate::model::RefKind) -> Color {
    use crate::model::RefKind;
    match kind {
        RefKind::Head => TEAL,
        RefKind::Local => GREEN,
        RefKind::Remote => PEACH,
        RefKind::Tag => YELLOW,
        RefKind::Other => OVERLAY,
    }
}

/// Catppuccin `teal`. The HEAD marker, and nothing else: it has to be
/// instantly separable from the branch name it points at.
pub const TEAL: Color = Color::Rgb(148, 226, 213);

/// The colour one history view answers to, worn by its tab in the pane frame
/// and by its title inside the pane. One function so the two cannot disagree:
/// the tab is what you aim at and the title is what confirms you landed.
///
/// Four separable hues -- blue, green, purple, orange -- because telling them
/// apart at a glance is the job. Where an echo of the view's own content was
/// available it was taken: `info` wears the repo-name blue it summarises,
/// `commits` the green of the `+N` column only it and `info` draw, `reflog`
/// the mauve of its own action words. `graph` is the odd one out and is
/// picked for contrast rather than echo: its lanes cycle six colours, so
/// there is nothing for it to echo, and its natural first-lane sky sat right
/// on top of `info`'s blue. `ci` wears maroon, a tab colour only: red's
/// family without the failing lamp's `RED`, so the tab never reads as a
/// verdict.
///
/// These are not new meanings, so the one-colour-per-concept rule holds. The
/// tab strip is also its own context: a peach `graph` tab cannot be mistaken
/// for the peach `3↓` in a badge column two panes away.
pub fn history_view_color(view: crate::app::HistoryView) -> Color {
    use crate::app::HistoryView;
    match view {
        HistoryView::Info => SAPPHIRE,
        HistoryView::Commits => GREEN,
        HistoryView::Reflog => MAUVE,
        HistoryView::Graph => PEACH,
        // Lavender rather than teal: teal is the HEAD marker and nothing
        // else, and this view draws HEAD markers. Separated from the mauve
        // `reflog` tab by peach in the strip order, so the two purples never
        // sit side by side.
        HistoryView::Branches => LAVENDER,
        // Maroon (his "perhaps red", FZ-30f): none of the CI lamp's own
        // colours (green, red, peach) is free here, and a lamp colour on the
        // tab would read as a verdict. `WATCH_OFF` holds the same value and
        // is deliberately not this constant.
        HistoryView::Ci => MAROON,
        // Yellow, the paper of a notes pad (FZ-53): the one tab hue left
        // that is neither the HEAD marker's teal nor a lamp's colour.
        HistoryView::Notes => YELLOW,
        // Pink, not the `WT` badge's lavender: lavender is the `branches`
        // tab already, and the tabs must stay distinct (FZ-77).
        HistoryView::Worktrees => PINK,
    }
}

/// Color for one character column of a `git log --graph` ASCII prefix. Each
/// column holds a single branch "lane" for as long as that lane is alive, so
/// coloring by column index gives parallel lines distinct dull hues -- similar
/// to how a real terminal colorizes `git log --graph --color`, instead of the
/// previous flat near-invisible grey where every lane looked the same.
pub fn graph_lane_color(col: usize) -> Color {
    GRAPH_PALETTE[col % GRAPH_PALETTE.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(c: Color) -> (u8, u8, u8) {
        match c {
            Color::Rgb(r, g, b) => (r, g, b),
            other => panic!("not an RGB colour: {other:?}"),
        }
    }

    /// The list band is dimmer than `SURFACE` yet still lifts off the pane
    /// (FZ-73): every channel lies strictly between the base and `SURFACE`.
    #[test]
    fn selected_band_lies_between_base_and_surface() {
        let b = (30u8, 30u8, 46u8); // Catppuccin Mocha `base`, the pane
        let (s, band) = (rgb(SURFACE), rgb(SELECTED_BG));
        assert!(b.0 < band.0 && band.0 < s.0, "red {band:?}");
        assert!(b.1 < band.1 && band.1 < s.1, "green {band:?}");
        assert!(b.2 < band.2 && band.2 < s.2, "blue {band:?}");
    }

    /// The copy flash (FZ-81) is a brighter wash than the band: every
    /// channel lies strictly between `SURFACE` and `OVERLAY`.
    #[test]
    fn yanked_wash_lies_between_surface_and_overlay() {
        let (s, o, y) = (rgb(SURFACE), rgb(OVERLAY), rgb(YANKED_BG));
        assert!(s.0 < y.0 && y.0 < o.0, "red {y:?}");
        assert!(s.1 < y.1 && y.1 < o.1, "green {y:?}");
        assert!(s.2 < y.2 && y.2 < o.2, "blue {y:?}");
    }

    #[test]
    fn mix_runs_from_one_colour_to_the_other() {
        let (a, b) = (Color::Rgb(0, 100, 200), Color::Rgb(100, 0, 100));
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 0.5), Color::Rgb(50, 50, 150));
        assert_eq!(mix(a, b, 1.0), b);
        assert_eq!(mix(a, b, 7.0), b, "clamped");
        assert_eq!(mix(Color::Reset, b, 0.2), b, "not RGB");
    }
}
