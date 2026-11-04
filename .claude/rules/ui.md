---
paths:
  - "src/ui/**/*"
  - "src/app.rs"
  - "src/contributions.rs"
  - "src/model.rs"
---

# Terminal UI invariants

Binding wherever the session runs. Loaded when a matching file is read or
edited; a loader without path scoping reads this file before changing the
modules it names. Moved from the root `CLAUDE.md` on 2026-10-30, text
unchanged.

**`ui::panel_rects` must produce the same rectangles `render` draws.** It exists
so mouse clicks can be hit-tested against what is on screen. Shared geometry
lives in one function called by both (`detail::split_inspector`, and
`ui::contributions::split_main_area` for the strip, over `dock_bottom`; the
`notes` layout splits columns first with `detail::split_main`, stacks pane 1
over the card with `split_inspector`, takes pane 2's body under its pinned
header with `detail::notes_pane_body` and splits that body into tree and
preview with `detail::split_notes_pane`); never
reintroduce a second copy of that arithmetic. Drift here misroutes clicks silently, with nothing
failing.

**`App::visible_repos` order must match the order the repo list draws.** The
cursor walks one while the eye walks the other. A free-text query switches both
to one ranked list, otherwise both group by section; both key off
`App::has_text_query()`. Within a section the order is push recency, as repoz
lists it (`sort_by_activity`), applied after the pass dedupe below.

**One color ladder per concept.** `badges::status_color` is the single source
for what a sync state looks like; the detail pane calls it rather than keeping
its own. `ui::filter_color` maps a quick filter to the same color its badge
already uses. Two ladders always drift.

Contribution intensity is the one place with two ladders, green for GitHub and
blue for local, because the two count different things. Both are reached
through one `colors::contrib_ladder(source)`, which the cells and the Less/More
legend call, so a source cannot leave half the strip on the other palette.

Both ladders are spaced evenly in CIELAB **L***, in saturation as well as
lightness: even *luminance* is the wrong metric and leaves the middles too
dark. The greens are tuned from github.com's, not sampled. The blue tops out
lower, carrying 0.07 of luminance to green's 0.72, so at the green's top the
bluest colour sRGB has reads as paper. Even L*, rate parity and chroma are the
invariants, not equal luminance; tests pin each, and `colors.rs` has the why.

**A contribution day is a Nerd Font square icon, not a block element.** The
icon is drawn by the font to sit on the text line, so it lines up with the
`Mon`/`Wed`/`Fri` labels. A half block cannot: it fills the top or bottom half
of the cell and sits a quarter row off the label's centre either way, which is
how the strip looked for a long time. The three sizes are three icons measured
in the font (`CellSize::glyph`), each one column wide as ratatui lays it out,
which the grid's column arithmetic and `slot_at`'s click routing depend on.

**The GitHub strip draws GitHub's `contributionLevel`, not a bucket of the
count.** github.com trims outliers before scaling each year and the rule is
not reproducible: measured over three years, a year whose busiest day had 83
contributions was scaled as if the top were 47, and one with an 86-day as if
it were 10. Quartiles of the max drew a 38-commit day two shades paler than
github.com. The field is free (cost 1, nodeCount 0, with and without it). The
`recent` window's levels are never used: its `from` rotates, so they are on a
scale that changes every few minutes. Days it overwrites are put on the year
window's scale by example (`Calendar::derive_missing_levels`), and a payload
cached before the field was asked for falls back to `intensity`.

**The inspector's top border carries the resize grip.** `grips::render`
paints six cells mid-border after the panes, so anything drawn on that
border (the CI body tabs) budgets around `grips::inspector_grip_span`;
pane 2's border has no grip and its arithmetic cannot be copied as is.

**`detail::repo_row_spans` is the one repo row, and the slot digest draws
it too.** A style that keys off the selected row reaches the digest's
cursor line, whose own slots (count, authors, subject) must follow it:
FZ-66's band stopped where the pane 1 part ended until
`digest::render_pane` washed the rest of the line.

**A view-local key never shadows a global one.** `r` is refresh in every
view, `notes` included; a view that needs a new action takes a key that is
unbound everywhere (FZ-59's toggle moved from `r` to `p` on his look).

**Status vocabulary is split by kind.** Counters are numeric (`3↓ 2↑ 4!`),
categorical states are words (`not cloned`). A not-cloned repo has all counters
at zero and must never render as synced: `RepoStatus::is_synced()` excludes it
deliberately.
