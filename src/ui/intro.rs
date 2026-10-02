//! The loading screen shown while the first load runs.
//!
//! A field of scrambled characters with `LOADING` held still in block capitals
//! in the middle. It is painted instead of `ui::render` until the first
//! `Loaded` lands, and then the finished screen replaces it in one frame.
//!
//! The module keeps its old name, as does the `intro` config key that switches
//! it: it used to also play a wave over the first finished screen, removed on
//! 2026-10-01 (see "The intro wave" in `docs/ideas.md`).

use std::time::{Duration, Instant};

use ratatui::buffer::{Buffer, Cell};
use ratatui::style::{Color, Modifier, Style};

use crate::ui::colors;

/// How often a frame is asked for while the loading field is up.
///
/// 60fps. The event loop's own 100ms tick is far too coarse for this, and it
/// cannot simply be lowered, because it *is* the spinner rate and
/// `BLINK_TICKS` and `SLOW_REPAINT_TICKS` are expressed in it. So the field
/// runs on a timer of its own that stops when the load lands.
pub const FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// Characters the noise is drawn from.
///
/// Deliberately all single-width ASCII, so every noise cell occupies exactly
/// the one cell it is written into and the grid stays consistent.
const SCRAMBLE: &[u8] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/\\|_-+=*<>[]{}()#$%&@?!;:.~^";

/// What a cell fades toward when it is at its dimmest: Catppuccin `base`, the
/// ground the terminal is already drawing on.
///
/// The ladder used to run from `SURFACE` -- one shared grey -- up to each
/// cell's own colour. That made every resting cell the same grey whatever hue
/// it was meant to be, and the whole field read as grey static. Fading toward
/// the *background* instead, with a floor, keeps every cell on its own hue.
const BACKDROP: Color = Color::Rgb(30, 30, 46);

/// How far a resting cell is carried from the backdrop toward its own colour,
/// out of 255: slightly muted, so the field reads as a backdrop rather than as
/// content.
const REST_MIX: u32 = 190;

/// What the white share of the noise wears: a half-step past `TEXT` toward
/// white.
const NOISE_WHITE: Color = Color::Rgb(232, 238, 255);

/// The hues a noise cell can wear, the Catppuccin palette the rest of the
/// dashboard is drawn from. White is the *other* option, and the rarer one --
/// see `ACCENT_SHARE`.
const ACCENTS: [Color; 7] = [
    colors::MAUVE,
    colors::SAPPHIRE,
    colors::LAVENDER,
    colors::PEACH,
    colors::GREEN,
    colors::YELLOW,
    colors::RED,
];

/// What share of the noise wears a hue rather than white. Compared against a
/// fixed per-cell value, so a cell keeps its hue or its white for as long as
/// the field is up.
const ACCENT_SHARE: f32 = 0.9;

/// How far a cell's resting brightness may stray, so the loading field has
/// grain rather than one flat shade.
const JITTER: f32 = 0.18;

/// A cell's fixed brightness offset, so the field has darker and lighter
/// nuances rather than one flat wash.
fn grain(x: u16, y: u16) -> f32 {
    (unit(hash(x, y, SALT_GRAIN)) * 2.0 - 1.0) * JITTER
}

/// A noise cell's resting brightness: grain alone.
fn resting_intensity(x: u16, y: u16) -> f32 {
    grain(x, y).clamp(0.0, 1.0)
}

/// A noise cell: a character that churns frame to frame, in its own colour
/// carried up from the backdrop by the resting shade and its brightness.
fn noise_cell(
    x: u16,
    y: u16,
    frame: u32,
    intensity: f32,
    accent_share: f32,
    rest_mix: u32,
) -> Cell {
    let symbol = SCRAMBLE[(hash(x, y, frame) % SCRAMBLE.len() as u32) as usize] as char;
    let lit = match unit(hash(x, y, SALT_ACCENT)) < accent_share {
        true => ACCENTS[(hash(x, y, SALT_HUE) % ACCENTS.len() as u32) as usize],
        false => NOISE_WHITE,
    };
    let mut cell = Cell::default();
    cell.set_symbol(&symbol.to_string());
    // From this cell's resting shade up to its full hue, rather than from a
    // shared grey: see `BACKDROP`.
    let mix = rest_mix + ((255 - rest_mix) as f32 * intensity) as u32;
    cell.set_fg(crate::ui::blend(BACKDROP, lit, mix.min(255), 255));
    cell
}

/// Salts, so one hash function can answer several independent questions about
/// the same cell without them correlating.
const SALT_GRAIN: u32 = 0x9E37_79B9;
const SALT_ACCENT: u32 = 0x85EB_CA6B;
const SALT_HUE: u32 = 0xC2B2_AE35;
const SALT_PHASE: u32 = 0x1656_67B1;

/// FNV-1a over the cell's coordinates and a salt, with an avalanche step so the
/// low bits are usable for a modulo.
///
/// Deterministic on purpose: the same cell in the same frame is always the same
/// character and colour, which is what makes the field testable. A real PRNG
/// would be a dependency for no gain -- nothing here needs to be
/// unpredictable, only unpatterned.
fn hash(x: u16, y: u16, salt: u32) -> u32 {
    let mut h: u32 = 2166136261;
    for value in [x as u32, y as u32, salt] {
        h ^= value;
        h = h.wrapping_mul(16777619);
    }
    h ^= h >> 15;
    h = h.wrapping_mul(2246822519);
    h ^= h >> 13;
    h
}

/// A hash as a fraction in 0..1.
fn unit(h: u32) -> f32 {
    h as f32 / u32::MAX as f32
}

const LOADING_CHURN: u32 = 12;

/// The word held in the middle of the loading field, as block capitals.
///
/// Deliberately still, while everything around it moves. A field that shimmers
/// everywhere has nothing to rest on; one still thing in the middle of it is
/// what makes the rest read as motion rather than as static -- and at this size
/// it also gives the noise a shape to break around.
///
/// Every row is the same length; `the_banner_is_a_rectangle` holds that, since
/// a short row would leave the field showing through the middle of a letter.
const LOADING_ART: [&str; 5] = [
    "█    ████ ████ ███  ███ █  █ ████",
    "█    █  █ █  █ █  █  █  ██ █ █   ",
    "█    █  █ ████ █  █  █  █ ██ █ ██",
    "█    █  █ █  █ █  █  █  █  █ █  █",
    "████ ████ █  █ ███  ███ █  █ ████",
];

/// Blank cells around the banner, so the noise does not crowd the letters.
const LABEL_PAD_X: u16 = 3;
const LABEL_PAD_Y: u16 = 1;

/// The field shown while the dashboard is still loading.
///
/// The screen fills with noise, the noise sits and shimmers while the repos are
/// probed, and the finished screen replaces it in one frame when they land.
/// Previously this time was spent looking at an empty frame.
pub struct Loading {
    started: Instant,
}

impl Default for Loading {
    fn default() -> Self {
        Self::new()
    }
}

impl Loading {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }

    /// Paint the field over `buf`.
    pub fn paint(&self, buf: &mut Buffer) {
        let elapsed = self.started.elapsed().as_millis() as u32;
        let frame = elapsed / FRAME_INTERVAL.as_millis().max(1) as u32;
        self.compose(buf, frame);
    }

    /// The field at a given frame. Split from `paint` so it can be looked at
    /// rather than raced against a clock.
    fn compose(&self, buf: &mut Buffer, frame: u32) {
        let area = buf.area;
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)] = noise_cell(
                    x,
                    y,
                    churn_step(x, y, frame),
                    resting_intensity(x, y),
                    ACCENT_SHARE,
                    REST_MIX,
                );
            }
        }
        stamp_label(buf);
    }
}

/// The block the banner occupies, padding included: `(top, left, width,
/// height)`. `None` when the screen cannot hold it, in which case the field is
/// drawn without it rather than with a clipped word.
fn label_span(area: ratatui::layout::Rect) -> Option<(u16, u16, u16, u16)> {
    let width = LOADING_ART[0].chars().count() as u16 + LABEL_PAD_X * 2;
    let height = LOADING_ART.len() as u16 + LABEL_PAD_Y * 2;
    if area.width < width || area.height < height {
        return None;
    }
    let top = area.top() + (area.height - height) / 2;
    let left = area.left() + (area.width - width) / 2;
    Some((top, left, width, height))
}

/// Stamp the still banner over the middle of the field.
fn stamp_label(buf: &mut Buffer) {
    let Some((top, left, width, height)) = label_span(buf.area) else {
        return;
    };
    // Cleared whole, padding included, so the banner sits in its own space
    // rather than having noise show through the counters of the letters.
    for dy in 0..height {
        for dx in 0..width {
            buf[(left + dx, top + dy)] = Cell::default();
        }
    }
    let style = Style::default()
        .fg(colors::TEXT)
        .add_modifier(Modifier::BOLD);
    for (row, line) in LOADING_ART.iter().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            if ch == ' ' {
                continue;
            }
            let mut cell = Cell::default();
            cell.set_symbol(&ch.to_string());
            cell.set_style(style);
            buf[(
                left + LABEL_PAD_X + col as u16,
                top + LABEL_PAD_Y + row as u16,
            )] = cell;
        }
    }
}

/// Which character a loading cell is showing: its own slow counter, offset per
/// cell so they do not all turn over on the same frame.
fn churn_step(x: u16, y: u16, frame: u32) -> u32 {
    (frame + hash(x, y, SALT_PHASE) % LOADING_CHURN) / LOADING_CHURN
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn area() -> Rect {
        Rect::new(0, 0, 40, 20)
    }

    /// The columns the banner's block occupies on a given row.
    fn banner_rows(area: Rect) -> std::ops::Range<u16> {
        let (top, _, _, height) = label_span(area).expect("a banner");
        top..top + height
    }

    /// The loading field fills the screen around its banner. That is its whole
    /// job: the wait used to be spent looking at an empty frame.
    #[test]
    fn the_loading_field_covers_the_screen_around_its_banner() {
        let mut buf = Buffer::empty(area());
        Loading::new().compose(&mut buf, 0);
        let rows = banner_rows(area());
        let blank = (0..20u16)
            .filter(|y| !rows.contains(y))
            .flat_map(|y| (0..40u16).map(move |x| (x, y)))
            .filter(|(x, y)| buf[(*x, *y)].symbol() == " ")
            .count();
        assert_eq!(blank, 0, "{blank} cells of the loading field were blank");
    }

    /// Every row of the banner is the same length. A short one would leave the
    /// field showing through the middle of a letter, since the block is cleared
    /// by its declared width rather than per row.
    #[test]
    fn the_banner_is_a_rectangle() {
        let width = LOADING_ART[0].chars().count();
        for (i, row) in LOADING_ART.iter().enumerate() {
            assert_eq!(row.chars().count(), width, "row {i} is a different length");
        }
    }

    /// The banner's fill is one cell wide by ratatui's own measurement, which
    /// is the measurement the layout uses. Same requirement as the commit node
    /// in the graph column: a glyph the terminal widens would shear the word.
    #[test]
    fn the_banner_fill_occupies_one_column() {
        for row in LOADING_ART {
            for ch in row.chars().filter(|c| *c != ' ') {
                assert_eq!(
                    ratatui::text::Span::from(ch.to_string()).width(),
                    1,
                    "{ch:?} is not one column wide"
                );
            }
        }
    }

    /// The banner is there, in the middle, and it does not move.
    #[test]
    fn the_banner_is_centred_and_still() {
        let field = Loading::new();
        let (top, left, width, height) = label_span(area()).expect("a banner");
        // Only the banner's own block: the noise around it churns like the rest
        // of the field, so a whole-row compare would be testing the field.
        let block = |frame| {
            let mut buf = Buffer::empty(area());
            field.compose(&mut buf, frame);
            (top..top + height)
                .map(|y| {
                    (left..left + width)
                        .map(|x| buf[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
        };
        let first = block(0);
        for frame in [1u32, 7, 40, 199] {
            assert_eq!(block(frame), first, "the banner moved on frame {frame}");
        }

        // The art is in there, padded, and the padding rows are clear.
        assert!(
            first[LABEL_PAD_Y as usize].contains(LOADING_ART[0]),
            "the banner is not drawn: {:?}",
            first[LABEL_PAD_Y as usize]
        );
        assert!(
            first[0].trim().is_empty(),
            "the noise crowds the banner: {:?}",
            first[0]
        );
    }

    /// A screen too narrow for the word gets the field without it, rather than
    /// a clipped word or a panic.
    #[test]
    fn a_narrow_screen_drops_the_banner_rather_than_clipping_it() {
        let narrow = Rect::new(0, 0, 6, 4);
        assert!(label_span(narrow).is_none());
        let mut buf = Buffer::empty(narrow);
        Loading::new().compose(&mut buf, 0);
        let blank = (0..4u16)
            .flat_map(|y| (0..6u16).map(move |x| (x, y)))
            .filter(|(x, y)| buf[(*x, *y)].symbol() == " ")
            .count();
        assert_eq!(blank, 0, "the narrow field has holes in it");
    }

    /// A twelfth of the field turns over on any given frame, not all of it.
    /// A field that may sit for several seconds must not be a full-screen
    /// repaint sixty times a second, and a shimmer reads better than a boil.
    #[test]
    fn the_loading_field_shimmers_rather_than_boiling() {
        let field = Loading::new();
        let mut a = Buffer::empty(area());
        let mut b = Buffer::empty(area());
        field.compose(&mut a, 0);
        field.compose(&mut b, 1);

        let total = 40 * 20;
        let changed = (0..20u16)
            .flat_map(|y| (0..40u16).map(move |x| (x, y)))
            .filter(|(x, y)| a[(*x, *y)].symbol() != b[(*x, *y)].symbol())
            .count();
        assert!(changed > 0, "the field is frozen");
        assert!(changed > 0, "the field is frozen");
        assert!(
            changed * 4 < total,
            "{changed} of {total} cells turned over in one frame"
        );
    }

    /// Noise is single-width ASCII, always. The buffer underneath holds wide
    /// glyphs, and a narrow character written into a continuation cell shifts
    /// the rest of its row.
    #[test]
    fn every_noise_character_is_one_cell_wide() {
        for byte in SCRAMBLE {
            let c = *byte as char;
            assert!(c.is_ascii_graphic(), "{c:?} is not single-width ASCII");
        }
    }

    /// The same cell in the same frame is always the same character, which is
    /// what lets any of this be tested at all.
    #[test]
    fn the_noise_is_deterministic() {
        let a = noise_cell(5, 6, 3, 0.5, 0.5, 255);
        let b = noise_cell(5, 6, 3, 0.5, 0.5, 255);
        assert_eq!(a.symbol(), b.symbol());
        assert_eq!(a.fg, b.fg);
    }

    /// ...but it churns between frames, or it is a static field of letters
    /// rather than noise.
    #[test]
    fn the_noise_churns_between_frames() {
        let changed = (0..40u16)
            .filter(|x| {
                noise_cell(*x, 4, 1, 0.5, 0.5, 255).symbol()
                    != noise_cell(*x, 4, 2, 0.5, 0.5, 255).symbol()
            })
            .count();
        assert!(
            changed > 20,
            "only {changed} of 40 cells changed between frames"
        );
    }

    /// Most of the field wears a hue and the rest is white, so the white reads
    /// as a sparkle rather than as the field's colour. The share is what the
    /// field showed while it opened the removed intro wave.
    #[test]
    fn the_loading_field_is_mostly_colour_with_some_white() {
        let coloured = (0..20u16)
            .flat_map(|y| (0..40u16).map(move |x| (x, y)))
            .filter(|(x, y)| unit(hash(*x, *y, SALT_ACCENT)) < ACCENT_SHARE)
            .count();
        let share = coloured as f32 / (40.0 * 20.0);
        assert!(
            (0.85..0.95).contains(&share),
            "the field is {:.0}% colour",
            share * 100.0
        );
    }
}
