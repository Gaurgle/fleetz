use ratatui::style::Color;

/// Catppuccin Mocha palette — matches repoz bash script colors.
pub const PEACH: Color = Color::Rgb(250, 179, 135);     // behind / warnings
pub const GREEN: Color = Color::Rgb(166, 227, 161);     // ahead / synced
pub const YELLOW: Color = Color::Rgb(249, 226, 175);    // uncommitted
pub const SAPPHIRE: Color = Color::Rgb(116, 199, 236);  // repo names
pub const LAVENDER: Color = Color::Rgb(180, 190, 254);  // headers
pub const MAUVE: Color = Color::Rgb(203, 166, 247);     // section labels / PRs
pub const HASH_CLR: Color = Color::Rgb(140, 155, 180);  // commit hashes
pub const DIFF_ADD: Color = Color::Rgb(134, 194, 134);  // +N lines
pub const DIFF_DEL: Color = Color::Rgb(210, 140, 140);  // -N lines
pub const OVERLAY: Color = Color::Rgb(127, 132, 156);   // paths, subtle text
pub const SURFACE: Color = Color::Rgb(69, 71, 90);      // dots, dividers
pub const BASE: Color = Color::Rgb(30, 30, 46);         // background
pub const TEXT: Color = Color::Rgb(205, 214, 244);       // default text
pub const RED: Color = Color::Rgb(243, 139, 168);        // CI failed

/// Dim a color toward the background while keeping its hue, for an inactive
/// state that still hints at what it would look like active.
pub fn dim(c: Color) -> Color {
    match c {
        Color::Rgb(r, g, b) => Color::Rgb(r / 3, g / 3, b / 3),
        other => other,
    }
}
