use ratatui::style::Color;

/// Catppuccin Mocha palette: matches repoz bash script colors.
pub const PEACH: Color = Color::Rgb(250, 179, 135); // behind / warnings
pub const GREEN: Color = Color::Rgb(166, 227, 161); // ahead / synced
pub const YELLOW: Color = Color::Rgb(249, 226, 175); // uncommitted
pub const SAPPHIRE: Color = Color::Rgb(116, 199, 236); // repo names
pub const LAVENDER: Color = Color::Rgb(180, 190, 254); // headers
pub const MAUVE: Color = Color::Rgb(203, 166, 247); // section labels

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

/// Contribution intensity for the GitHub source, empty through busiest.
///
/// Sampled straight off github.com's dark-dimmed contribution graph rather
/// than picked by eye or interpolated between two palette colours. The muted
/// sage greens are the ones anyone who has looked at their own profile is
/// expecting; the saturated standard-dark set reads as a different product.
/// Step 0 is GitHub's own empty-square slate, which is a lift above the page
/// rather than a colour, so an empty day stays structure rather than data.
const CONTRIB_LADDER_GITHUB: [Color; 5] = [
    Color::Rgb(43, 49, 59),
    Color::Rgb(39, 70, 37),
    Color::Rgb(60, 105, 55),
    Color::Rgb(91, 147, 83),
    Color::Rgb(129, 194, 118),
];

/// The same ladder for the local source, translated to blue. The two sources
/// count different things and will never agree, so they should not look
/// identical: the colour says which one you are reading before the title
/// does. Blue rather than red because red already means "CI failed".
///
/// Derived by holding each green step's saturation and solving its lightness
/// for the same relative luminance, so the two ladders climb at identical
/// rates. A plain hue rotation was tried first and put step 1 within 4% of
/// the empty slate, which is itself blue-hued: a whole intensity level
/// vanished. Keep the luminances in step if either ladder changes.
const CONTRIB_LADDER_LOCAL: [Color; 5] = [
    Color::Rgb(43, 49, 59),
    Color::Rgb(45, 64, 85),
    Color::Rgb(67, 95, 128),
    Color::Rgb(104, 134, 170),
    Color::Rgb(151, 178, 209),
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

/// Color for one character column of a `git log --graph` ASCII prefix. Each
/// column holds a single branch "lane" for as long as that lane is alive, so
/// coloring by column index gives parallel lines distinct dull hues -- similar
/// to how a real terminal colorizes `git log --graph --color`, instead of the
/// previous flat near-invisible grey where every lane looked the same.
pub fn graph_lane_color(col: usize) -> Color {
    GRAPH_PALETTE[col % GRAPH_PALETTE.len()]
}
