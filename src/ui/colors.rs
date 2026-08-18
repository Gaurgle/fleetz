use ratatui::style::Color;

/// Catppuccin Mocha palette: matches repoz bash script colors.
pub const PEACH: Color = Color::Rgb(250, 179, 135); // behind / warnings
pub const GREEN: Color = Color::Rgb(166, 227, 161); // ahead / synced
pub const YELLOW: Color = Color::Rgb(249, 226, 175); // uncommitted
pub const SAPPHIRE: Color = Color::Rgb(116, 199, 236); // repo names
pub const LAVENDER: Color = Color::Rgb(180, 190, 254); // headers
pub const MAUVE: Color = Color::Rgb(203, 166, 247); // section labels / PRs
pub const HASH_CLR: Color = Color::Rgb(140, 155, 180); // commit hashes
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

/// Contribution intensity for the GitHub source, empty through busiest.
///
/// Step 0 is `SURFACE`, the same near-invisible tone the dividers use, so an
/// empty day reads as structure rather than as data. Steps 1 to 4 are
/// github.com's own dark-theme greens, which cover a far wider range than an
/// interpolation between two palette colours does: a busy day should not be
/// a slightly brighter version of a quiet one, it should be obviously
/// different from across the room.
const CONTRIB_LADDER_GITHUB: [Color; 5] = [
    Color::Rgb(69, 71, 90),
    Color::Rgb(14, 68, 41),
    Color::Rgb(0, 109, 50),
    Color::Rgb(38, 166, 65),
    Color::Rgb(57, 211, 83),
];

/// The same ladder for the local source, in blue, over the same span of
/// darkness to brightness. The two sources count different things and will
/// never agree, so they should not look identical: the colour says which one
/// you are reading before the title does. Blue rather than red because red
/// already means "CI failed" everywhere else.
const CONTRIB_LADDER_LOCAL: [Color; 5] = [
    Color::Rgb(69, 71, 90),
    Color::Rgb(16, 52, 92),
    Color::Rgb(13, 88, 163),
    Color::Rgb(48, 140, 220),
    Color::Rgb(96, 190, 255),
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

/// Dull, distinct hues per graph "lane" -- a different palette from author
/// colors so the two don't get visually confused on the same line.
const GRAPH_PALETTE: [Color; 6] = [
    Color::Rgb(140, 170, 190), // dull cyan
    Color::Rgb(190, 150, 140), // dull orange
    Color::Rgb(150, 180, 140), // dull green
    Color::Rgb(180, 140, 180), // dull magenta
    Color::Rgb(190, 185, 140), // dull yellow
    Color::Rgb(140, 150, 190), // dull blue
];

/// Color for one character column of a `git log --graph` ASCII prefix. Each
/// column holds a single branch "lane" for as long as that lane is alive, so
/// coloring by column index gives parallel lines distinct dull hues -- similar
/// to how a real terminal colorizes `git log --graph --color`, instead of the
/// previous flat near-invisible grey where every lane looked the same.
pub fn graph_lane_color(col: usize) -> Color {
    GRAPH_PALETTE[col % GRAPH_PALETTE.len()]
}
