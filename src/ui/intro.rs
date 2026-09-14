//! The one-time intro wave.
//!
//! Once per launch, the first painted screen arrives as a diagonal wave of
//! scrambled characters that resolves into the real content behind it. The
//! screen starts *full* of noise and ends sparse: most cells resolve to blank,
//! and the few that carry content are what is left standing when the wave has
//! passed.
//!
//! It is a post-pass over a finished buffer, not a change to how anything is
//! drawn. `ui::render` paints once into the target; every frame after that
//! composes from that clone. Nothing in the twenty-odd render functions knows
//! this exists, and the animation costs one pass over the cell grid rather than
//! forty full re-renders.

use std::time::{Duration, Instant};

use ratatui::buffer::{Buffer, Cell};
use ratatui::style::{Color, Modifier, Style};

use crate::ui::colors;

/// How long the whole sweep takes.
pub const DURATION: Duration = Duration::from_millis(1500);

/// How often a frame is asked for while the wave is running.
///
/// 60fps. The event loop's own 100ms tick is far too coarse for this -- it
/// would give seven frames and read as a stutter -- and it cannot simply be
/// lowered, because it *is* the spinner rate and `BLINK_TICKS` and
/// `SLOW_REPAINT_TICKS` are expressed in it. So the wave runs on a timer of its
/// own that stops when it is done.
pub const FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// Width of the resolving band, as a fraction of the diagonal.
///
/// The crest is the brightest line; cells within this distance of it are lit in
/// proportion, and a cell that falls this far *behind* it has certainly
/// resolved. Too narrow and the wave is a hard edge; too wide and there is no
/// wave, just the screen fading over.
const BAND: f32 = 0.3;

/// Where in the band a cell stops churning and shows its real character.
///
/// Drawn per cell rather than shared, which is the whole point: a single
/// threshold would flip the entire band at once and read as a moving edge,
/// where scattered thresholds have characters locking one by one across the
/// width of the wave -- noise resolving into text rather than being wiped away
/// by it. Held clear of both ends so nothing locks before the wave arrives and
/// nothing is still churning after it has gone.
const LOCK_FLOOR: f32 = 0.02;
const LOCK_SPREAD: f32 = 0.96;

/// Characters the noise is drawn from.
///
/// Deliberately all single-width ASCII. The buffer underneath holds wide
/// glyphs, and writing a narrow character into a wide glyph's continuation cell
/// shifts the rest of the row; keeping the noise one cell wide means the grid
/// stays consistent no matter which cells have settled.
const SCRAMBLE: &[u8] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/\\|_-+=*<>[]{}()#$%&@?!;:.~^";

/// What a cell fades toward when it is furthest from the crest: Catppuccin
/// `base`, the ground the terminal is already drawing on.
///
/// The ladder used to run from `SURFACE` -- one shared grey -- up to each
/// cell's own colour. That made every resting cell the same grey whatever hue
/// it was meant to be, and since most of the field is at rest most of the time,
/// the whole thing read as grey static with a coloured line moving through it.
/// Fading toward the *background* instead, with a floor, keeps every cell on
/// its own hue the whole way down.
const BACKDROP: Color = Color::Rgb(30, 30, 46);

/// How far a resting cell is carried from the backdrop toward its own colour,
/// out of 255, at each end of the wave's opening.
///
/// The loading field sits at `LIT`, slightly muted. Once the wave is under way
/// the field is at `CLEAR`, every cell in its full colour, and the wave stands
/// out by being lighter and darker than that (`FOAM`, `SHADOW_DEPTH`) rather
/// than by dimming the field around it.
///
/// So the wave opens at `LIT` -- matching the field exactly, so the handover has
/// no seam -- and clears to `CLEAR` over `CLEAR_IN`.
const REST_MIX_LIT: u32 = 190;
const REST_MIX_CLEAR: u32 = 255;
/// The wave has to be clearer than the field it comes out of, or the opening
/// runs backwards into a dim. Checked at compile time, since both are constants
/// and a runtime assert would be theatre.
const _: () = assert!(REST_MIX_LIT < REST_MIX_CLEAR);

/// How much of the sweep is spent clearing from the field's shade to full
/// colour. Short: it is a transition into the effect, not part of it.
const CLEAR_IN: f32 = 0.18;

/// What the white share of the noise wears. A half-step past `TEXT` toward
/// white, and deliberately short of `FOAM`, so white noise never reads as a
/// piece of the crest.
const NOISE_WHITE: Color = Color::Rgb(232, 238, 255);

/// The foam on top of the breaking crest: pure white, and bold, which is the
/// terminal's own "brighter". Solid across the whole crest rather than a fade
/// into it, so the crest reads as foam rather than as a glow, and it is past
/// every colour the field holds, white noise included.
const FOAM: Color = Color::Rgb(255, 255, 255);

/// Half-width of the crest, in diagonal units.
///
/// Narrow next to `BAND`, so it reads as a line breaking rather than a flood.
/// It cannot go much narrower: a cell recomputes only every `CELL_UPDATE`
/// frames, and the front covers about 0.07 of the diagonal in that time, so a
/// crest thinner than that would skip cells and break into dashes. At this
/// width every cell the crest crosses is sampled on it at least twice.
const CREST_WIDTH: f32 = 0.08;

/// How far toward the backdrop the noise under the wave is shaded, at its
/// deepest: from the crest back to a `CREST_WIDTH` behind it, lifting to
/// nothing by the end of `BAND`, where the last of the noise has locked.
///
/// Only noise is shaded. A locked cell shows the content exactly as it will be
/// drawn, so the dashboard never flickers dark on its way in.
const SHADOW_DEPTH: f32 = 0.65;

/// The hues a noise cell can wear, the Catppuccin palette the rest of the
/// dashboard is drawn from. White is the *other* option, and increasingly the
/// rarer one -- see `accent_share`.
const ACCENTS: [Color; 7] = [
    colors::MAUVE,
    colors::SAPPHIRE,
    colors::LAVENDER,
    colors::PEACH,
    colors::GREEN,
    colors::YELLOW,
    colors::RED,
];

/// What share of the noise wears a hue rather than white, at the start of the
/// sweep and at the end.
///
/// It climbs as the wave goes, so the field is mostly white when the wave
/// arrives and mostly colour by the time the last of it is washed away. The
/// threshold is compared against a fixed per-cell value, so a cell that has
/// turned colour stays colour: the field gains hues rather than flickering
/// between them.
const ACCENT_START: f32 = 0.9;
const ACCENT_END: f32 = 1.0;

/// How far a cell's resting brightness may stray, so the loading field has
/// grain rather than one flat shade. At `CLEAR` there is no headroom left for
/// it, which is the point: the wave's field is every cell's full colour.
const JITTER: f32 = 0.18;

/// A running intro wave over the screen it was started from.
pub struct Intro {
    started: Instant,
    /// The finished screen the wave resolves into.
    target: Buffer,
}

impl Intro {
    /// Start a wave that resolves into `target`, which is the buffer
    /// `ui::render` has just painted.
    pub fn new(target: Buffer) -> Self {
        Self {
            started: Instant::now(),
            target,
        }
    }

    /// How far through the sweep, 0 to 1.
    pub fn progress(&self) -> f32 {
        let elapsed = self.started.elapsed().as_secs_f32();
        (elapsed / DURATION.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// Whether the wave has finished and the pane should go back to painting
    /// itself.
    pub fn finished(&self) -> bool {
        self.progress() >= 1.0
    }

    /// Compose this frame into `buf`.
    ///
    /// Returns false when the wave cannot be drawn against this buffer -- the
    /// terminal was resized out from under it, so the captured screen no longer
    /// describes the space. The caller ends the intro rather than painting a
    /// stale layout.
    pub fn paint(&self, buf: &mut Buffer) -> bool {
        self.compose(buf, self.progress())
    }

    /// The frame at an arbitrary point through the sweep. `paint` is this with
    /// the clock supplying `t`; separating them is what lets the wave be
    /// inspected frame by frame instead of raced against a timer.
    fn compose(&self, buf: &mut Buffer, t: f32) -> bool {
        if buf.area != self.target.area {
            return false;
        }
        let area = self.target.area;
        if area.width == 0 || area.height == 0 {
            return false;
        }
        for y in area.top()..area.bottom() {
            let mut x = area.left();
            while x < area.right() {
                // A wide glyph and its continuation cells are one unit: they
                // settle together or not at all. Split across a frame boundary
                // they would leave a narrow character sitting in a continuation
                // cell, which shifts everything to its right.
                let span = self.glyph_span(x, y, area.right());
                // One time for the whole glyph, from its leading cell: a wide
                // glyph and its continuation must agree about everything, the
                // slot included.
                let ct = self.cell_time(x, y, t);
                let frame = (ct * (DURATION.as_secs_f32() / FRAME_INTERVAL.as_secs_f32())) as u32;
                let settled = self.locked(x, y, area, ct);
                for i in 0..span {
                    let cx = x + i;
                    buf[(cx, y)] = match settled {
                        true => self.target[(cx, y)].clone(),
                        false => {
                            // Only cells the wave is actually over churn, and
                            // that is keyed off the crest rather than the lit
                            // intensity: the grain lifts resting cells above
                            // zero too, so intensity would have half the field
                            // flickering.
                            //
                            // Ratatui redraws a cell whose symbol or style
                            // changed, so churning everywhere makes every frame
                            // a full-screen repaint -- fine locally, unkind
                            // over ssh at 60fps for the length of the sweep. A
                            // still field costs nothing to draw and reads
                            // better anyway: the churn is the wave, not the
                            // ground it crosses.
                            //
                            // The `CLEAR_IN` opening is the exception, since the
                            // resting shade is moving for all of it. That is a
                            // fifth of the sweep rather than all of it, and it
                            // is the transition, so it is worth the frames.
                            let churn = match self.crest(cx, y, area, ct) > 0.0 {
                                true => frame,
                                false => 0,
                            };
                            let mut cell = noise_cell(
                                cx,
                                y,
                                churn,
                                resting_intensity(cx, y),
                                accent_share(ct),
                                rest_mix(ct),
                            );
                            let (fg, bold) = light(cell.fg, self.phase(cx, y, area, ct));
                            cell.set_fg(fg);
                            if bold {
                                cell.modifier.insert(Modifier::BOLD);
                            }
                            cell
                        }
                    };
                }
                x += span;
            }
        }
        true
    }

    /// How many cells the target glyph at `x` occupies. Continuation cells of a
    /// wide glyph carry an empty symbol, which is how they are recognised
    /// without measuring character widths.
    fn glyph_span(&self, x: u16, y: u16, right: u16) -> u16 {
        let mut span = 1;
        while x + span < right && self.target[(x + span, y)].symbol().is_empty() {
            span += 1;
        }
        span
    }

    /// How far the wave front has passed this cell: negative ahead of it, zero
    /// at the crest, positive behind.
    fn phase(&self, x: u16, y: u16, area: ratatui::layout::Rect, t: f32) -> f32 {
        front(t) - diagonal(x, y, area)
    }

    /// The point in the sweep this cell is drawn at.
    ///
    /// Quantised to the cell's own slot, so its whole appearance -- character,
    /// brightness, hue, whether it has locked -- holds still between slots and
    /// the terminal has nothing to redraw. Staggered by a per-cell phase so the
    /// slots do not all land on the same frame, which would just move the
    /// full-screen repaint rather than remove it.
    ///
    /// Monotonic, so a locked cell stays locked, and pinned to exactly 1.0 at
    /// the end so the last frame is the content and not a slot short of it.
    fn cell_time(&self, x: u16, y: u16, t: f32) -> f32 {
        if t >= 1.0 {
            return 1.0;
        }
        let frames = DURATION.as_secs_f32() / FRAME_INTERVAL.as_secs_f32();
        let phase = (hash(x, y, SALT_PHASE) % CELL_UPDATE) as f32;
        let slot = ((t * frames + phase) / CELL_UPDATE as f32).floor() * CELL_UPDATE as f32;
        ((slot - phase) / frames).clamp(0.0, 1.0)
    }

    /// Whether this cell has stopped churning and shows its real character.
    ///
    /// `resolve` runs 0 at the leading edge of the band to 1 at the trailing
    /// edge, and each cell locks at its own point along it. Monotonic by
    /// construction -- `resolve` only grows with `t` and the threshold is fixed
    /// per cell -- so a character that has locked never falls back to noise.
    fn locked(&self, x: u16, y: u16, area: ratatui::layout::Rect, t: f32) -> bool {
        let resolve = ((self.phase(x, y, area, t) + BAND) / (2.0 * BAND)).clamp(0.0, 1.0);
        resolve >= LOCK_FLOOR + unit(hash(x, y, SALT_LOCK)) * LOCK_SPREAD
    }

    /// How close the crest is, 1 directly on it and 0 once the cell is a full
    /// band away in either direction. This bounds where the wave churns; how a
    /// cell is lit is `light`.
    fn crest(&self, x: u16, y: u16, area: ratatui::layout::Rect, t: f32) -> f32 {
        1.0 - (self.phase(x, y, area, t).abs() / BAND).min(1.0)
    }
}

/// Where the crest is at time `t`, in diagonal units.
///
/// Runs from `-BAND` to `1 + BAND` so the sweep both starts and ends off the
/// screen: at `t = 0` every cell is ahead of the wave, which is what makes the
/// first frame a full screen of noise, and at `t = 1` every cell is behind it.
fn front(t: f32) -> f32 {
    t * (1.0 + 2.0 * BAND) - BAND
}

/// A cell's fixed brightness offset, so the field has darker and lighter
/// nuances rather than one flat wash.
fn grain(x: u16, y: u16) -> f32 {
    (unit(hash(x, y, SALT_GRAIN)) * 2.0 - 1.0) * JITTER
}

/// A noise cell's resting brightness: grain alone.
///
/// Shared with the loading field on purpose. The wave draws with it too and
/// lights the result in `light`, so when the wave takes over from the field
/// nothing jumps: the same cells are the same brightness, and only the churn
/// changes hands.
fn resting_intensity(x: u16, y: u16) -> f32 {
    grain(x, y).clamp(0.0, 1.0)
}

/// The resting shade at time `t`: the field's at the start of the sweep, full
/// colour by `CLEAR_IN`.
fn rest_mix(t: f32) -> u32 {
    let cleared = (t / CLEAR_IN).clamp(0.0, 1.0);
    REST_MIX_LIT + ((REST_MIX_CLEAR - REST_MIX_LIT) as f32 * cleared) as u32
}

/// Whether a noise cell is on the crest, within a `CREST_WIDTH` of it either
/// side. `phase` is `Intro::phase`.
fn on_crest(phase: f32) -> bool {
    phase.abs() < CREST_WIDTH
}

/// How deeply the wave shades a noise cell. Nothing ahead of the crest: the
/// shade is under the wave, not in front of it. `phase` is `Intro::phase`.
fn shadow(phase: f32) -> f32 {
    if phase <= 0.0 {
        return 0.0;
    }
    let lift = ((phase - CREST_WIDTH).max(0.0) / (BAND - CREST_WIDTH)).min(1.0);
    SHADOW_DEPTH * (1.0 - lift)
}

/// A noise cell's colour on the wave, and whether it is drawn bold: `FOAM` on
/// the crest, otherwise its resting colour, shaded if the wave is over it.
fn light(rest: Color, phase: f32) -> (Color, bool) {
    if on_crest(phase) {
        return (FOAM, true);
    }
    let shaded = crate::ui::blend(rest, BACKDROP, (shadow(phase) * 255.0) as u32, 255);
    (shaded, false)
}

/// How much of the noise wears a hue rather than white at time `t`.
fn accent_share(t: f32) -> f32 {
    ACCENT_START + (ACCENT_END - ACCENT_START) * t.clamp(0.0, 1.0)
}

/// A cell's position along the diagonal, 0 at the bottom-left corner and 1 at
/// the top-right.
///
/// Both axes are normalised before being added, so the wave crosses at a
/// consistent visual angle rather than one that flattens out on a wide terminal
/// and steepens on a narrow one.
fn diagonal(x: u16, y: u16, area: ratatui::layout::Rect) -> f32 {
    let w = (area.width.saturating_sub(1)).max(1) as f32;
    let h = (area.height.saturating_sub(1)).max(1) as f32;
    let across = (x - area.left()) as f32 / w;
    let up = (area.bottom() - 1 - y) as f32 / h;
    (across + up) / 2.0
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
const SALT_LOCK: u32 = 0x27D4_EB2F;
const SALT_PHASE: u32 = 0x1656_67B1;

/// FNV-1a over the cell's coordinates and a salt, with an avalanche step so the
/// low bits are usable for a modulo.
///
/// Deterministic on purpose: the same cell in the same frame is always the same
/// character and colour, which is what makes the whole effect testable. A real
/// PRNG would be a dependency for no gain -- nothing here needs to be
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

/// Frames between one cell of the *wave* recomputing its appearance.
///
/// The wave is limited by the terminal, not by the compositor. Composing a
/// frame costs a third of a millisecond; writing one costs whatever the
/// emulator takes to render the escape sequences for every cell that changed,
/// and at 60fps with a third of the screen changing that measured at 4.7 MB/s
/// on a 200x50 terminal. Nothing renders that.
///
/// So each cell recomputes on its own slot rather than every frame, staggered
/// by a per-cell phase, which puts a hard ceiling of one in `CELL_UPDATE` cells
/// redrawn per frame. The wave still advances every frame -- different cells
/// move on different frames -- and the stagger reads as texture, which suits
/// it. The frame rate is untouched.
const CELL_UPDATE: u32 = 4;

/// Frames between one loading cell re-rolling its character.
///
/// Every cell gets a phase offset, so a twelfth of them change on any given
/// frame rather than all of them at once. That is what keeps a field that may
/// sit there for several seconds from being a full-screen repaint sixty times
/// a second, and it shimmers rather than boiling, which is the better look
/// anyway.
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
/// The same noise the wave later washes away, at the same resting brightness,
/// so the two are one continuous surface: the screen fills with noise, the
/// noise sits and shimmers while the repos are probed, and then the wave
/// crosses it and leaves the dashboard behind. Previously this time was spent
/// looking at an empty frame.
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

    /// The field at a given frame. Split from `paint` for the same reason
    /// `Intro::compose` is: so it can be looked at rather than raced.
    fn compose(&self, buf: &mut Buffer, frame: u32) {
        let area = buf.area;
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)] = noise_cell(
                    x,
                    y,
                    churn_step(x, y, frame),
                    resting_intensity(x, y),
                    // The share and the shade the wave opens on, so the
                    // handover changes nothing but the churn. The wave clears
                    // to full colour from here of its own accord.
                    accent_share(0.0),
                    rest_mix(0.0),
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

    /// A target screen with a scattering of content, as the real one is:
    /// mostly blank, some text.
    fn target() -> Buffer {
        let mut buf = Buffer::empty(area());
        buf.set_string(2, 3, "branches", ratatui::style::Style::default());
        buf.set_string(2, 4, "* main", ratatui::style::Style::default());
        buf
    }

    fn intro() -> Intro {
        Intro::new(target())
    }

    /// Ahead of the wave the field holds still. Ratatui redraws a cell whose
    /// symbol or style changed, so a screen that churns everywhere is a
    /// full-screen repaint on every one of the sweep's 175 frames.
    #[test]
    fn the_field_ahead_of_the_wave_does_not_churn() {
        let it = intro();
        let mut a = Buffer::empty(area());
        let mut b = Buffer::empty(area());
        // Sampled after `CLEAR_IN`: the whole field is deliberately changing
        // colour before that, as it clears to full colour for the wave.
        assert!(it.compose(&mut a, 0.25));
        assert!(it.compose(&mut b, 0.27));

        // The top-right corner is the last place the wave reaches, so a
        // quarter of the way through it is far outside the band.
        let (x, y) = (39u16, 0u16);
        assert!(
            it.crest(x, y, area(), 0.25) == 0.0,
            "the corner is inside the band, so this proves nothing"
        );
        assert_eq!(a[(x, y)].symbol(), b[(x, y)].symbol(), "the field churned");
        assert_eq!(a[(x, y)].fg, b[(x, y)].fg, "the field changed colour");
    }

    /// ...while the cells the wave is over do churn, or it is a static field
    /// with a spotlight moving across it.
    #[test]
    fn the_cells_under_the_wave_churn() {
        let it = intro();
        let changed = (0..40u16)
            .filter(|x| {
                let mut a = Buffer::empty(area());
                let mut b = Buffer::empty(area());
                // Far enough apart to land on different frames: the sweep
                // is ~122 frames, so a hundredth of it is not always one.
                it.compose(&mut a, 0.50);
                it.compose(&mut b, 0.53);
                a[(*x, 10)].symbol() != b[(*x, 10)].symbol()
            })
            .count();
        assert!(changed > 0, "nothing under the wave changed between frames");
    }

    /// Why the target is captured *inside* the draw closure and not after it.
    ///
    /// `Terminal::draw` swaps buffers on its way out: the screen just painted
    /// becomes the previous buffer and `current_buffer_mut` is a freshly reset
    /// blank. Capturing after the call captured that blank, so the wave
    /// resolved into nothing -- noise, then an empty pane, then the content
    /// snapping in when the wave ended. It looked like an effect played in
    /// front of the dashboard rather than one that revealed it.
    #[test]
    fn the_screen_just_painted_is_not_the_buffer_draw_leaves_behind() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Style;

        let mut term = Terminal::new(TestBackend::new(20, 2)).unwrap();
        let mut inside = None;
        term.draw(|frame| {
            frame
                .buffer_mut()
                .set_string(0, 0, "content", Style::default());
            inside = Some(frame.buffer_mut().clone());
        })
        .unwrap();

        let row = |buf: &Buffer| -> String { (0..20u16).map(|x| buf[(x, 0)].symbol()).collect() };
        let after = term.current_buffer_mut().clone();
        assert!(
            row(&inside.expect("captured")).contains("content"),
            "the closure did not see what it painted"
        );
        assert!(
            !row(&after).contains("content"),
            "draw no longer swaps buffers -- the capture could move back out"
        );
    }

    /// The columns the banner's block occupies on a given row.
    fn banner_rows(area: Rect) -> std::ops::Range<u16> {
        let (top, _, _, height) = label_span(area).expect("a banner");
        top..top + height
    }

    /// How much of the screen changes between two consecutive frames, as a
    /// mean and a worst case over the sweep, on a realistically sized terminal.
    fn churn_per_frame() -> (usize, usize, usize) {
        let (w, h) = (200u16, 50u16);
        let rect = Rect::new(0, 0, w, h);
        let it = Intro::new(Buffer::empty(rect));
        let frames = DURATION.as_secs_f32() / FRAME_INTERVAL.as_secs_f32();

        let mut worst = 0usize;
        let mut total = 0usize;
        let samples = 40;
        for step in 0..samples {
            let t0 = step as f32 / samples as f32;
            let t1 = (t0 + 1.0 / frames).min(1.0);
            let mut a = Buffer::empty(rect);
            let mut b = Buffer::empty(rect);
            assert!(it.compose(&mut a, t0));
            assert!(it.compose(&mut b, t1));
            let changed = (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .filter(|(x, y)| {
                    a[(*x, *y)].symbol() != b[(*x, *y)].symbol() || a[(*x, *y)].fg != b[(*x, *y)].fg
                })
                .count();
            worst = worst.max(changed);
            total += changed;
        }
        (total / samples, worst, w as usize * h as usize)
    }

    /// The wave is limited by the terminal, not by the compositor.
    ///
    /// Composing a frame costs a third of a millisecond; *writing* one costs
    /// whatever the emulator takes to render escape sequences for every cell
    /// that changed. With every cell recomputed every frame that measured at a
    /// third of the screen changing per frame and the whole screen during the
    /// opening -- about 4.7 MB/s to a 200x50 terminal at 60fps, which
    /// nothing renders. It read as the wave lagging.
    ///
    /// Per-cell update slots put a ceiling on it. This test is the ceiling: the
    /// failure is invisible to every other test here, because they all look at
    /// one frame at a time and each frame is individually correct.
    #[test]
    fn only_a_fraction_of_the_screen_changes_between_frames() {
        let (mean, worst, cells) = churn_per_frame();
        assert!(
            mean * 5 < cells,
            "{mean} of {cells} cells change on an average frame ({:.0}%)",
            100.0 * mean as f64 / cells as f64
        );
        assert!(
            worst * 2 < cells,
            "{worst} of {cells} cells change on the worst frame ({:.0}%)",
            100.0 * worst as f64 / cells as f64
        );
    }

    /// The wave opens at the loading field's muted shade and clears to full
    /// colour, rather than dimming the field around it: the crest stands out by
    /// being whiter and the wave by the shade it throws, so the letters it has
    /// not reached stay clear. Opening at the field's shade keeps the handover
    /// seamless.
    #[test]
    fn the_wave_opens_at_the_fields_shade_and_clears_to_full_colour() {
        assert_eq!(
            rest_mix(0.0),
            REST_MIX_LIT,
            "the wave does not open at the field's shade"
        );
        assert_eq!(rest_mix(CLEAR_IN), REST_MIX_CLEAR, "the wave never clears");
        assert_eq!(rest_mix(1.0), REST_MIX_CLEAR, "the field dims again");
        assert_eq!(REST_MIX_CLEAR, 255, "full colour is not full");

        let midway = rest_mix(CLEAR_IN / 2.0);
        assert!(
            (REST_MIX_LIT + 1..REST_MIX_CLEAR).contains(&midway),
            "the opening is a step, not a fade: {midway}"
        );
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

    /// It hands over to the wave without a seam: the field and the wave's own
    /// first frame put every cell at the same brightness and colour, so only
    /// the churn changes hands.
    #[test]
    fn the_field_and_the_waves_first_frame_agree() {
        let mut field = Buffer::empty(area());
        Loading::new().compose(&mut field, 0);
        let mut first = Buffer::empty(area());
        assert!(intro().compose(&mut first, 0.0));

        // The banner's rows are excluded: it is the one thing that does not
        // carry over, and it going is the handover.
        let rows = banner_rows(area());
        for y in (0..20u16).filter(|y| !rows.contains(y)) {
            for x in 0..40u16 {
                assert_eq!(
                    field[(x, y)].fg,
                    first[(x, y)].fg,
                    "cell ({x},{y}) changed colour when the wave took over"
                );
            }
        }
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

    /// End to end: the first frame is a full screen of noise, the last is the
    /// content exactly, and the wave exists only in between. This is the whole
    /// effect stated as one assertion pair.
    #[test]
    fn the_wave_starts_as_noise_and_ends_as_the_content() {
        let it = intro();

        let mut first = Buffer::empty(area());
        assert!(it.compose(&mut first, 0.0));
        let blank = (0..20u16)
            .flat_map(|y| (0..40u16).map(move |x| (x, y)))
            .filter(|(x, y)| first[(*x, *y)].symbol() == " ")
            .count();
        assert_eq!(blank, 0, "{blank} cells were blank on the first frame");

        let mut last = Buffer::empty(area());
        assert!(it.compose(&mut last, 1.0));
        assert_eq!(last, target(), "the last frame is not the content exactly");
    }

    /// The content emerges from under the wave rather than riding in on it: a
    /// cell that carries content is still noise while the front is ahead of it.
    /// Without this the effect would be text sliding in, not noise resolving.
    #[test]
    fn content_stays_hidden_until_the_wave_has_passed_it() {
        let it = intro();
        let mut early = Buffer::empty(area());
        assert!(it.compose(&mut early, 0.2));
        let row: String = (0..40u16).map(|x| early[(x, 3)].symbol()).collect();
        assert!(
            !row.contains("branches"),
            "content showed before the wave reached it: {row:?}"
        );
    }

    /// The sweep is drawn at the frame rate it asks for, not the event loop's.
    /// Seven frames would read as a stutter; this is the number that makes it a
    /// wave.
    #[test]
    fn the_sweep_gets_enough_frames_to_read_as_motion() {
        let frames = DURATION.as_millis() / FRAME_INTERVAL.as_millis();
        assert!(frames >= 40, "only {frames} frames over the whole sweep");
    }

    /// The first frame is a full screen of noise: every cell is ahead of the
    /// wave, which is what "the whole screen scrambled" means.
    #[test]
    fn the_wave_starts_with_every_cell_ahead_of_it() {
        let it = intro();
        for y in 0..20u16 {
            for x in 0..40u16 {
                assert!(
                    !it.locked(x, y, area(), 0.0),
                    "cell ({x},{y}) had already locked on the first frame"
                );
            }
        }
    }

    /// ...and the last frame is exactly the content, with no noise left.
    #[test]
    fn the_wave_ends_on_the_target_exactly() {
        let it = intro();
        for y in 0..20u16 {
            for x in 0..40u16 {
                assert!(
                    it.locked(x, y, area(), 1.0),
                    "cell ({x},{y}) had not locked on the last frame"
                );
            }
        }
    }

    /// A character that has locked stays locked. The thresholds are per cell,
    /// so this is the property that keeps scattered locking from reading as
    /// flicker: the wave washes the noise away and it does not come back.
    #[test]
    fn a_settled_cell_never_returns_to_noise() {
        let it = intro();
        for (x, y) in [(0u16, 19u16), (20, 10), (39, 0), (7, 15)] {
            let mut settled_at = None;
            for step in 0..=100 {
                let t = step as f32 / 100.0;
                let settled = it.locked(x, y, area(), t);
                match (settled, settled_at) {
                    (true, None) => settled_at = Some(t),
                    (false, Some(at)) => {
                        panic!("cell ({x},{y}) settled at {at} and came back at {t}")
                    }
                    _ => {}
                }
            }
            assert!(settled_at.is_some(), "cell ({x},{y}) never settled");
        }
    }

    /// The wave runs corner to corner: the bottom-left cell is reached before
    /// the top-right one. This is the diagonal, stated as an ordering rather than
    /// as an angle.
    #[test]
    fn the_bottom_left_corner_resolves_before_the_top_right() {
        let a = diagonal(0, 19, area());
        let b = diagonal(39, 0, area());
        assert!(a < b, "the wave does not run bottom-left to top-right");
        assert!(a.abs() < f32::EPSILON, "the near corner is not the origin");
        assert!(
            (b - 1.0).abs() < f32::EPSILON,
            "the far corner is not the end"
        );
    }

    /// Both corners *off* the leading diagonal sit halfway through, which is
    /// what makes the front a diagonal line rather than a radial ripple.
    #[test]
    fn the_off_diagonal_corners_arrive_together() {
        let bottom_right = diagonal(39, 19, area());
        let top_left = diagonal(0, 0, area());
        assert!((bottom_right - top_left).abs() < f32::EPSILON);
    }

    /// Perceived brightness of an RGB colour, for comparing shades.
    fn luma(c: Color) -> f32 {
        let Color::Rgb(r, g, b) = c else {
            panic!("{c:?} is not RGB");
        };
        0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32
    }

    /// Brightest on the crest: it foams solid white and bold across its whole
    /// width, past any colour in the field, rather than fading in from the
    /// edges. Ahead of the wave and once it has gone, a cell is its own clear
    /// colour, untouched.
    #[test]
    fn a_cell_is_brightest_as_the_crest_passes_it() {
        for rest in ACCENTS.into_iter().chain([NOISE_WHITE]) {
            for phase in [0.0, CREST_WIDTH * 0.9, -CREST_WIDTH * 0.9] {
                assert_eq!(
                    light(rest, phase),
                    (FOAM, true),
                    "{rest:?} at {phase} on the crest"
                );
            }
            assert_eq!(
                light(rest, -BAND),
                (rest, false),
                "{rest:?} ahead of the wave"
            );
            assert_eq!(light(rest, BAND), (rest, false), "{rest:?} after the wave");
            assert!(luma(FOAM) > luma(rest), "{rest:?} outshines the crest");
        }
    }

    /// The letters under the wave are shadowed: darker than their clear colour
    /// just behind the crest, lifting as the content comes through, and never
    /// ahead of the crest, where the wave has not reached.
    #[test]
    fn the_letters_under_the_wave_are_shadowed() {
        let rest = colors::SAPPHIRE;
        let under = light(rest, CREST_WIDTH).0;
        let lifting = light(rest, (CREST_WIDTH + BAND) / 2.0).0;
        assert!(
            luma(under) < luma(lifting),
            "the shade does not lift behind the crest"
        );
        assert!(
            luma(lifting) < luma(rest),
            "the shade is gone before the wave is"
        );
        assert_eq!(
            light(rest, -CREST_WIDTH).0,
            rest,
            "the shade runs ahead of the wave"
        );
    }

    /// Every cell the crest crosses is drawn as foam at least once. A cell
    /// recomputes only on its own slot, so a crest narrower than the ground the
    /// front covers between slots would skip cells and break into dashes.
    #[test]
    fn every_cell_is_foam_as_the_crest_passes() {
        let it = intro();
        let frames = (DURATION.as_secs_f32() / FRAME_INTERVAL.as_secs_f32()) as u32;
        for y in 0..20u16 {
            for x in 0..40u16 {
                let foamed = (0..=frames).any(|f| {
                    let ct = it.cell_time(x, y, f as f32 / frames as f32);
                    on_crest(it.phase(x, y, area(), ct))
                });
                assert!(foamed, "the crest skipped cell ({x},{y})");
            }
        }
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

    /// A wide glyph and its continuation settle together, so the row is never
    /// caught with a narrow character sitting in a continuation cell.
    #[test]
    fn a_wide_glyph_settles_with_its_continuation() {
        let mut buf = Buffer::empty(area());
        // How ratatui stores a double-width glyph: the symbol, then an empty
        // continuation cell.
        buf[(10, 10)].set_symbol("⚬");
        buf[(11, 10)].set_symbol("");
        let it = Intro::new(buf);
        assert_eq!(
            it.glyph_span(10, 10, 40),
            2,
            "the pair was not held together"
        );
        assert_eq!(it.glyph_span(12, 10, 40), 1, "an ordinary cell was widened");
    }

    /// A resize invalidates the captured screen: it describes a layout that no
    /// longer exists, so the wave bows out rather than painting a stale one.
    #[test]
    fn a_resize_ends_the_wave_rather_than_painting_a_stale_screen() {
        let it = intro();
        let mut resized = Buffer::empty(Rect::new(0, 0, 80, 24));
        assert!(!it.paint(&mut resized));
    }

    /// The same cell in the same frame is always the same character, which is
    /// what lets any of this be tested at all.
    #[test]
    fn the_noise_is_deterministic() {
        let a = noise_cell(5, 6, 3, 0.5, 0.5, REST_MIX_CLEAR);
        let b = noise_cell(5, 6, 3, 0.5, 0.5, REST_MIX_CLEAR);
        assert_eq!(a.symbol(), b.symbol());
        assert_eq!(a.fg, b.fg);
    }

    /// ...but it churns between frames, or it is a static field of letters
    /// rather than noise.
    #[test]
    fn the_noise_churns_between_frames() {
        let changed = (0..40u16)
            .filter(|x| {
                noise_cell(*x, 4, 1, 0.5, 0.5, REST_MIX_CLEAR).symbol()
                    != noise_cell(*x, 4, 2, 0.5, 0.5, REST_MIX_CLEAR).symbol()
            })
            .count();
        assert!(
            changed > 20,
            "only {changed} of 40 cells changed between frames"
        );
    }

    /// The field grows more colourful as the wave crosses it: mostly white
    /// when it arrives, mostly Catppuccin by the time the last noise is washed
    /// away. Both ends matter -- flat white reads as a loading bar, and colour
    /// from the first frame has nowhere to go.
    #[test]
    fn the_noise_grows_more_colourful_as_the_wave_goes() {
        let share = |t: f32| {
            let coloured = (0..20u16)
                .flat_map(|y| (0..40u16).map(move |x| (x, y)))
                .filter(|(x, y)| unit(hash(*x, *y, SALT_ACCENT)) < accent_share(t))
                .count();
            coloured as f32 / (40.0 * 20.0)
        };
        let early = share(0.0);
        let late = share(1.0);
        assert!(
            early < late,
            "the field did not gain colour: {early} -> {late}"
        );
        assert!(
            (0.85..0.95).contains(&early),
            "the first frames are {:.0}% colour",
            early * 100.0
        );
        assert!(
            late > 0.95,
            "the last frames are only {:.0}% colour",
            late * 100.0
        );
    }

    /// A cell that has turned colour stays colour. The threshold rises and the
    /// per-cell value is fixed, so the field gains hues rather than flickering
    /// between white and colour as the share climbs.
    #[test]
    fn a_cell_that_gains_colour_keeps_it() {
        for (x, y) in [(3u16, 7u16), (19, 2), (31, 14)] {
            let mut coloured_at = None;
            for step in 0..=50 {
                let t = step as f32 / 50.0;
                let coloured = unit(hash(x, y, SALT_ACCENT)) < accent_share(t);
                match (coloured, coloured_at) {
                    (true, None) => coloured_at = Some(t),
                    (false, Some(at)) => panic!("({x},{y}) coloured at {at}, white again at {t}"),
                    _ => {}
                }
            }
        }
    }
}
