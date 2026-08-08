# fleetz ideas / backlog

Design notes for features not yet built. Nothing here is committed to; it is
thinking captured so it is not lost.

---

## Interactive time scope

**Status: shipped.** `t` opens the time-window overlay (`ui/timewindow.rs`),
the window is applied as a view filter, and the last choice persists via
`window_state.rs`. Kept here for the reasoning; the open questions below are
resolved or moot.

**Problem.** repoz sets the activity window with a flag (`--since 1d`, `2d`,
`1w`, `1h`, etc.). fleetz has the same flag, but it is interactive, so you
should be able to change the window live without relaunching, and see the
list re-scope instantly.

**Why it is cheap.** fleetz already fetches all GitHub repo data (with
`pushedAt` timestamps) once at startup and holds it in memory. So changing the
window is not a re-query, it is a re-filter of data already present, exactly
like the `#dirty` / `#behind` quick-filters. It can be instant, no spinner.

### What it should gate

Only the **github activity** section (repos pushed since the cutoff). The
**local changes** section is about uncommitted/unpushed work, not push
recency, so it should stay visible regardless of the window. Otherwise
narrowing would hide dirty repos you are actively working on, which is the
opposite of useful!

### How to trigger it (two candidates, likely both)

1. **Cycle key + header indicator (MVP).** A key pair steps a preset ladder,
   narrower/wider, with the current scope shown in the header (which already
   renders "since <date> <slot>"). Suggested ladder:

   ```
   today -> 1d -> 3d -> 1w -> 2w -> 1mo -> all
   ```

   Key idea: `[` narrows, `]` widens (or `<` / `>`). Instant re-filter on each
   press. This covers the large majority of real use.

2. **Filter-strip token (fast follow).** Type something like `@1w` or
   `since:2d` into the `/` strip, reusing the exact relative-date parser the
   `--since` flag already has (`cli.rs`). Gives arbitrary values and fits the
   existing combined search/filter model. The strip already parses `#tags`, so
   an `@` / `since:` token is a natural sibling.

### The one real caveat: repo_limit

`repo_limit` (default 50) caps how many repos are fetched from GitHub (the 50
most-recently-pushed). So widening to `1mo` or `all` cannot surface repos
older than that fetched set. Options, cheapest first:

- **Cap the ladder** at what is already fetched, and note in the header when
  the window would exceed the fetched data ("showing 50 most recent").
- **Re-fetch with a bigger limit** when the user widens past the fetched set
  (costs an API call + a brief spinner, but correct).

Recommendation: ship with the cap + note; add re-fetch-on-widen only if it
actually bites.

### Implementation sketch

- The cleanest shape is to hold the **full** repo set (GitHub data + matched
  local status) once, and apply the time window as a **view filter**, the same
  layer as the quick filters in `app.rs::visible_repos`. Today the cutoff is
  applied in `load_repos` at startup; move it (or duplicate it) to the view so
  it can change without re-scanning local repos.
- Reuse `cli.rs`'s relative-date parsing for both the flag and the interactive
  token, so `1d/2d/1w/1h` behave identically in both.
- Update the header string (already computed from the mode) when the scope
  changes.
- No new GitHub call for narrowing or for widening within the fetched set.

### Open questions

- Exact ladder rungs, and whether `all` belongs on it or is a separate toggle
  (it already exists as the `-a` mode).
- Key bindings: `[` / `]` vs `<` / `>`; and whether the `@`/`since:` token is
  worth it once the cycle key exists.
- How this composes with `--before` (a second bound) interactively.

---

## Settings menu

**Status: phase 1 shipped.** `,` opens `Overlay::Settings` (`ui/settings.rs`)
with five rows: refresh interval, startup view, work start, work end, and
remember-pane-sizes. Enter applies the live fields and writes every field back
to `config.toml` via `toml_edit`, preserving comments and untouched keys.

Resolutions to the open questions below: **saving is explicit** (Enter), and
apply and save are one action, since applying without saving would leave the
session disagreeing with the file it loaded from with nothing on screen to say
so. Each row shows a dim `restart` marker when its value only takes effect next
launch, which is currently the two work-hour fields: they are read once, from
the `Config` the main loop holds by reference, when the cutoff and header are
computed. Making those live means threading a mutable config through
`handle_key`, which is a bigger change than the field is worth.

Phases 2 to 4 (reload-requiring fields, hidden repos, themes) are unbuilt. The
notes below are kept for them.

**Problem.** Every tunable lives in `~/.config/fleetz/config.toml` and is read
exactly once at startup. Changing the watch interval, the repo limit, or the
starting view means quitting, editing a file, and relaunching. For a tool you
sit in all day that is the wrong loop. A modal settings overlay would fix it,
and would give later parameters (themes, hidden repos) somewhere to land
instead of accreting one-off keybindings.

**Menu, not an in-app command.** A command line inside the TUI would need its
own parser, its own discovery problem (you have to know the command exists),
and its own help. A menu is self-documenting: open it and every tunable is
visible with its current value.

### Why the interval is nearly free

`refresh_interval` is read once into app state, CLI flag winning:

```rust
// src/main.rs:206
app.refresh_interval = args.interval.unwrap_or(config.refresh_interval);
```

The watch tick only ever compares elapsed seconds against that field
(`src/main.rs:432-436`). Nothing caches it, nothing derives from it. Mutating
`app.refresh_interval` at runtime takes effect on the next tick: no restart, no
reload, no plumbing. It is the natural first setting.

### The overlay machinery already exists

`Overlay::{None, Help, Branches, TimeWindow}` (`src/app.rs:133`) is dispatched
modally in `handle_key` (`src/main.rs:555`), and `ui/timewindow.rs` is already
a one-purpose settings dialog: focusable fields, `h`/`l` to move, `j`/`k` to
adjust, `0-9` to type, Enter to apply, Esc to cancel. A settings overlay is
that same shape with a scrollable field list. Add `Overlay::Settings`, one arm
in `handle_key`, one `ui/settings.rs`.

Key: `,` is unbound and is the conventional preferences key. (`S` is also free;
`s` is taken by branches.)

### Persistence is the real decision

`Config` derives `Deserialize` only and is never written back. Two options:

1. **Write into `config.toml` with `toml_edit`.** One source of truth, and it
   preserves hand-written comments and formatting. Costs one dependency (small,
   well maintained, same author as `toml`).
2. **A machine-local overrides file**, mirroring the existing `WindowState`
   precedent (`~/.cache/fleetz/window.toml`). No new dependency, but two
   sources of truth: set 30s in the menu, later hand-edit `config.toml` to 90,
   and the invisible override still wins. That is a bug report waiting to
   happen.

Leaning toward `toml_edit`.

**Precedence rule to settle first.** `--interval` currently beats config.
Proposed: the menu edits the session value immediately, an explicit save writes
`config.toml`, and CLI flags are authoritative only at startup.

### Cost per setting

| Setting | Cost | Notes |
|---|---|---|
| `refresh_interval` | trivial | live, next tick |
| `view` (detail/compact) | trivial | `config.startup_view` exists, `v` toggles |
| `work_start` / `work_end` | trivial | display only |
| `repo_limit` | small | used at fetch time, so applying needs an implicit `R` reload |
| `repo_dir` / `extra_dirs` | small | same, plus rediscovery |
| hide repos | medium, and it is a feature not a setting | see below |
| themes | large | see below |

**Hidden repos.** No exclusion concept exists today. Needs a `hidden = [...]`
config key filtered in `App::visible_repos()` (`src/app.rs:539`), a key to hide
the selected repo from the list, and an unhide list inside the settings menu.
Without the unhide screen a hidden repo is unrecoverable from inside the TUI,
which makes the feature a trap.

**Themes.** `ui/colors.rs` is 14 `pub const` items referenced directly from all
nine `ui/*` modules plus `app.rs`. Runtime themes mean a `Theme` struct and
either threading it through every render fn or a `OnceLock` global. Mechanical
but wide. It deserves its own change, with the picker wired up afterwards.

### Suggested phasing

1. `Overlay::Settings` + `ui/settings.rs` + `,`, live-only fields (interval,
   view, work hours), with write-back. Self-contained.
2. Reload-requiring fields (`repo_limit`, `repo_dir`, `extra_dirs`), applied
   through the existing reload path.
3. Hidden repos as a real feature (config key, filter, hide key, unhide list).
4. Themes as a standalone `colors` refactor, then expose the picker.

### Open questions

- `toml_edit` write-back vs the dependency-free overrides file.
- Does saving need to be explicit (Enter/`s` writes config) or should every
  change persist immediately, with Esc as the only way to revert?
- Should the menu show which values came from CLI flags this session, so an
  overridden value does not look like it is being ignored?
- Bundled theme set vs user-defined palettes in `config.toml`.

---

## The double check mark

**Status: shipped, but not by the route below.** The fix went the other way
from this section's recommendation: rather than changing the sync glyph, CI gave
up the checkmark. CI is now a ringed disk `◉` in every state, colored green
passed / red failed / orange pending (blinking under watch mode), and sync keeps
`✓`. The detail pane's sync line also dropped it, so the ringed disk belongs to CI
alone (`model.rs::CiStatus::symbol`, `ui/badges.rs::ci_display`).

This resolves the collision at the source: the two are now different *shapes*,
not just different tokens, so no combination of states can produce two identical
badges.

**The tradeoff this section warned about is now real.** Under "Rejected" below,
"CI as filled/hollow circles" was turned down because it makes pass versus fail
a color-only distinction, which is bad for colorblind users and low-color
terminals. That is exactly what shipped. The mitigation is that the detail pane
still spells the state out ("◉ CI passed" / "◉ CI failed") in both the panel
title and the body line, so the information is recoverable one keypress away,
just not in the list.

Cheapest escape hatches if it bites: give failed its own shape (`◉`, `⬤`, or
back to `✗`) while passed and pending stay disks, or add a config key choosing
between a shape-distinct and a color-only CI set. Neither is built.

**Original problem.** A clean repo with green CI rendered two identical badges:
`✓ ✓`.
They are two unrelated facts that happen to share a glyph *and* a color, so
the row reads as a rendering bug rather than as information.

- The first `✓` is **git sync**: `RepoStatus::short_status()`
  (`src/model.rs:243`) builds `3↓ 2↑ 4!` from behind/ahead/dirty, and falls
  back to a bare `✓` only when all three are zero.
- The second `✓` is **CI**: `CiStatus::Passed.symbol()` (`src/model.rs:52`),
  alongside `✗` failed and `○` pending.

**Partly narrowed since.** A not-cloned repo used to hit the same fallback (all
three counters read zero for want of a working tree), so it rendered a green
`✓` too, and with CI metadata a `✓ ✓` for a repo that was not on disk. That
case now returns `not cloned` in `RED`. The collision this section is about --
clean repo, CI passed -- is untouched.

Both are `colors::GREEN`, so nothing separates them. Every other combination
disambiguates itself (`3↓ ✓` is obviously sync-then-CI); only the all-clear
case collides, which is also the most common row on the list.

### Change the sync glyph, not the CI glyph

`✓` / `✗` for CI is the universal idiom, and it is what GitHub's own UI shows.
Moving it would trade one confusion for another.

The sync side is the odd one out. Its vocabulary is otherwise arrows and
counts (`↓ ↑ !`), and `✓` is a fallback that fires only when there is nothing
to count. Replacing that fallback fixes the collision at its source and leaves
CI conventional. Candidates:

- `≡` for "in sync", keeping the affirmative "all good" read that the comment
  at `src/model.rs:184-187` was after.
- `·` for "nothing to report", quieter, lets CI carry the only strong glyph.
- Render nothing at all. Cleanest, but it drops the affirmation, and the
  right-aligned dot-fill would then run straight into the CI badge (or into
  empty space for repos with no CI), so the column would need a look.

### Rejected

- **Recolor one of them.** Same glyph, and pass/fail would then rest on color
  alone. Bad for colorblind users and for low-color terminals.
- **CI as filled/hollow circles** (`●` passed, `●` failed, `○` pending). A
  coherent family, but it makes pass vs fail purely a color distinction. Same
  accessibility problem.
- **Prefix the CI badge** (`CI✓`). Costs columns in a badge group that is
  deliberately tight and right-aligned flush to the pane edge.
- **A group divider** between sync and the GitHub badges. Cheap, but it adds a
  column and only weakens the collision instead of removing it.

### Notes for whoever picks this up

- One change point. `badge_spans` (`src/ui/badges.rs:119`) feeds both views, so
  compact (`ui/compact.rs:109`) and detail (via `repo_list_item`) update
  together. Its color ladder is `badges::status_color` (`src/ui/badges.rs:94`),
  now shared with the detail pane.
- The expanded CI line in the detail pane spells the state out as "passed"
  (`src/ui/detail.rs:252`, `:547`), so it is already unambiguous and should
  not change.
- Whatever is chosen, keep pass and fail distinguishable without color.
- `short_status()` has direct test coverage (`src/model.rs:297`), so the new
  glyph should be asserted there.

---

## Filter and search strip

**Status: shipped.** The dots are gone, search is fuzzy and ranked, and the
sharp edges in tag parsing are fixed. Kept here for the reasoning and for the
open questions at the end.

**Problem.** The strip drew six colored dots (dirty, behind, ahead, ci, prs,
stash) that looked like toggles but were pure output: no key and no click ever
set one, only typing a `#tag` did. There was no legend, so reading them meant
knowing the order by heart. Meanwhile the query text below already showed the
same information, in the same colors, with words attached.

Four smaller faults sat underneath:

- **Two of the six dot colors were wrong.** The comment claimed each dot used
  "the color that filter's data already uses elsewhere", but Ahead was
  `SAPPHIRE` (the repo-name color) against a `PEACH` badge, and Stashes was
  `HASH_CLR` (the commit-hash color) against an `OVERLAY` badge.
- **A bare `#` activated every filter.** Filters AND together, so six at once
  was almost always empty, and since `#` is the first keystroke of every tag,
  the list blanked and repopulated on the way to typing any of them.
- **Unknown tags failed silently.** `#drity` matched nothing, was still
  consumed from the free text, and rendered in plain yellow. No signal at all
  that the token did nothing.
- **Search was plain substring.** `name.contains(query)`, so `flz` did not find
  `fleetz`.

### What shipped

- **Dots deleted**, along with `FILTER_DEFS` and `QuickFilters::is_active`
  which existed only to feed them. The typed query is the single indicator.
- **Fuzzy search** in `src/fuzzy.rs`: a hand-rolled subsequence scorer
  (`score`, `src/fuzzy.rs:43`) with named constants for match, consecutive-run,
  word-start, gap, and trailing-remainder scoring. Hand-rolled rather than a
  matcher crate because the candidate list is tens of short names, and because
  every rule then stays stateable and directly unit-tested.
- **Ranked results.** With free text typed, `visible_repos` sorts by score
  descending and the repo list collapses its three section headers into one
  `results` header, so the top row really is the best match. Clearing the query
  restores section order. Both sides key off `App::has_text_query()`.
- **Bare `#` is inert**, and unknown tags are collected into
  `ParsedQuery::unknown_tags` and named in red in the strip.
- **Tag colors** now come from one `filter_color` fn matching the badge
  vocabulary, so ahead is PEACH and stash is OVERLAY like their badges.

### Rejected

- **Number keys for filters.** Reconsidered and rejected again: `1`/`2`/`3` are
  panel focus and `0` clears tags. This was the original reason filters live in
  the search text at all.
- **Keeping the dots but making them clickable.** `PanelRects` already
  hit-tests, so it was cheap, but it would have kept a control that needs a
  legend to read at all. The query text needs none.
- **Dropping `#tag` filters entirely.** "Show me only the dirty ones" has no
  other expression in the UI.
- **Length-normalized fuzzy scoring.** Penalizing total haystack length sinks
  exactly the long hyphenated names initialisms are meant to find
  (`wireless-test-platform`). Only the trailing remainder after the last match
  is charged.

### Open questions

- Filters still only **AND**. "dirty or behind" is not expressible, and neither
  is negation. A `!` prefix or a `|` separator would both work; neither is
  obviously worth the parser.
- **The group filter is still a separate system**: its own state
  (`app.active_group`), its own keys (`g`/`G`), and rendered in the header in
  MAUVE rather than in the strip. So the strip is still not a complete picture
  of what is filtered, and there is no single clear-all. Folding it in as a
  `@group` token in the same query is the obvious unification.
- Fuzzy currently matches **repo name only**. Owner, path, and branch name are
  all plausible haystacks. Multi-field matching needs a per-field weight, which
  is a real design decision rather than a tweak.
- The `results` header is the only cue that ranked mode is on. It may want the
  match count, or the query echoed.

---

## Inspector and right-column layout

**Status: partly shipped.** Proportional sizing and the deduplicated geometry
landed; interactive resize did not.

**Problem.** The inspector was a hardcoded `INSPECTOR_HEIGHT: u16 = 10` and
docked only when the right column exceeded `INSPECTOR_HEIGHT + 6` rows. On a
tall terminal it stayed at ten rows while the space above it grew without
bound; below the threshold it vanished outright rather than shrinking. Nothing
about it was adjustable at runtime.

Worse, that geometry existed **twice**: once in `detail::render` and once in
`ui::panel_rects`, which exists specifically so mouse clicks can be hit-tested
against the same rectangles that were drawn. Its own doc comment said it "must
mirror `render` exactly". Any drift between the two copies would land clicks on
the wrong pane, silently.

### What shipped

`detail::split_right_column` (`src/ui/detail.rs:40`) is now the single source of
that geometry, called by both `render` and `panel_rects`. The height is
`INSPECTOR_PCT` (40%) of the right column, clamped to
`[INSPECTOR_MIN_HEIGHT, INSPECTOR_MAX_HEIGHT]` = `[8, 24]`, and further capped
so the detail pane always keeps `DETAIL_MIN_HEIGHT` = 8 rows. Below 16 rows
total the inspector is dropped, as before, rather than shown as a sliver.

Net effect: on a 40-row terminal the inspector grows from 10 rows to ~15, and
on a 60-row one to its 24-row ceiling, instead of staying at 10 either way.

**Interactive resize shipped too.** `<`/`>` move the vertical divider, `+`/`-`
the horizontal one, `=` resets both. Both are percentages on `App`
(`repo_list_pct`, `inspector_pct`) clamped to ranges that keep either pane from
collapsing, and `split_main` joined `split_right_column` as shared geometry.

Sizes reset to their defaults on every launch. Persisting them is opt-in via
`persist_pane_sizes` in the `,` menu, which writes both percentages back on
exit. Saving happens at teardown rather than per keypress, because resizing is a
held-key gesture that would otherwise rewrite the config a dozen times per drag.

**Mouse drag shipped too.** `PanelRects` gained the main area plus
`divider_at` / `repo_list_pct_at` / `inspector_pct_at`, and `App::dragging`
holds the grabbed divider between press and release. Two details worth keeping:
a boundary is grabbable with one column of slop either side, because the gap
itself is a single cell and a single-cell target cannot be hit reliably; and
once a drag starts, any mouse event that is not a drag ends it, since a release
outside the terminal can swallow the `Up` entirely and would otherwise wedge
every later mouse event in the drag branch.

### Not done

- **A settings row for the sizes themselves.** The menu can toggle persistence
  but not set the percentages numerically; the resize keys and the mouse are the
  only ways in.
- **A visible affordance on the borders.** Nothing marks a divider as draggable;
  you have to know.

  An earlier version of this note claimed hover tracking was unavailable. That
  was wrong, and worth correcting because it would send the next person down a
  dead end. `EnableMouseCapture` already emits `?1003h` (any-event tracking)
  alongside 1000 and 1002, and `MouseEventKind::Moved` is documented as "moved
  the mouse cursor while not pressing a mouse button". Hover events reach
  `handle_mouse_input` today and are dropped on its `_ => {}` arm.

  So the blocker is not the terminal, it is two things in fleetz. Any-event
  tracking emits an event per cell crossed, so anything hover-driven needs a
  dwell timer and a "same cell, ignore" guard rather than reacting per event.
  And the header has no geometry model: it builds spans and lets ratatui
  right-align them, with a fit ladder that drops pieces, so nothing knows which
  column `◉ watch` landed on. Hovering anything up there means giving the header
  the `PanelRects` treatment, with the same duplicate-layout-math risk that
  `split_right_column` exists to avoid.

  Worth remembering that hover is mouse-only. In a tool with this many
  keybindings, a fact reachable only by hovering is a fact keyboard users
  cannot get.

### Open questions

- Should the inspector's file cap (`INSPECTOR_FILE_CAP` = 30,
  `src/ui/detail.rs:54`) scale with the pane now that the pane scales? At 24
  rows the cap is barely reachable; at 8 it is far past the fold either way.
- Does a resizable inspector need a minimum that differs from its
  drop-it-entirely threshold, or is one constant enough?
- Percent-of-column or absolute rows as the persisted unit? Percent survives a
  terminal resize better; rows are what a user dragging a divider is choosing.

---

## Header noise

**Status: shipped.** The header used to move constantly whether or not
anything was happening. Three changes, and one idea worth keeping.

**The idea: a relative time display is an animation.** "swept 14s ago" can only
stay true by repainting, so it is a moving part whose entire job is keeping
itself honest. It churned away at an empty terminal through a lunch break to
say what one timestamp says at a glance. The header now shows `swept 11:42`,
written once per sweep and then still. The date is added when the sweep was not
today, because fleetz sits in a tmux pane across midnight and a bare clock time
reads as today.

This applies wherever a relative age appears. It is *not* applied to commit ages
in the lists, deliberately: for a commit you care about elapsed time rather than
wall-clock, and nobody wants to subtract dates to see how old something is.

**Automatic refreshes are silent.** `start_sweep` takes a `manual` flag; only a
sweep the user asked for (`r`, `F`) announces itself with the spinner. A watch
timer firing on an interval must not repaint a colored indicator over a
dashboard being read. As a bonus a silent sweep buys no repaints either, since
there is no spinner to animate.

**What still moves is deliberate.** The pending-CI blink and the watch pulse
(`◉`/`●`). Both mean something; neither exists to keep itself accurate.

**Key hints have a placement rule now:** a key sits next to the thing it
changes, and everything else lives in the footer. So `[t]` annotates the window
label it edits (and is dropped with that label, since a bare `[t]` next to
nothing explains nothing), while `[v]` and `[?]` moved down to join the other
global actions. `[v]` had been rendering in *both* places.

The alternative, every hint in the footer, was measured and rejected: the full
hint row is 97 columns and would reach 119, which with the summary on the left
needs about 154 columns before it stops degrading to bare keys. Consistency
there would have cost the descriptions on everything.

Fixing the pulse exposed a latent bug worth recording: `SLOW_REPAINT_TICKS` was
10 against a `BLINK_TICKS` of 5, so between any two repaints the blink flipped
exactly twice and was always drawn in the same phase. The pending-CI blink had
therefore never once been visible. Any *even* multiple of the blink period has
this property, so the two constants are now tied together rather than picked
independently.

### Open

- Overdue colouring only triggers under watch mode, since without a promised
  cadence there is nothing to be late against. A manually driven dashboard that
  has not been refreshed all day says nothing about it.
- A hover popup on the watch indicator could carry the whole watch picture
  (last sweep, interval, last GitHub poll, next due). See the mouse notes in the
  inspector section for why that is more work than it looks.

---

## Owner and identity in the detail pane

**Status: shipped.** `owner { __typename, login }` is fetched, `RepoStatus`
carries `owner`, and `identity_spans` renders `owner/name` flush right on the
status line. Resolutions to the open questions below: kept in the **body**
(not the title); the remote URL line **stays** for now since it is the only
place a self-hosted host is visible; narrow panes **drop the whole group**
rather than truncating it; a repo with neither an API match nor a parseable
remote renders **nothing**.

**Problem.** Pane 2 tells you almost nothing about *whose* repo you are looking
at. The name appears once in the panel title, and the only ownership signal is
the remote URL rendered in flat `OVERLAY` grey under the status line:

```
 [2] fleetz ✓ ─────────────────────────────
 ● synced   ~/repos/fleetz
   git@github.com:Gaurgle/fleetz.git
```

You have to read a git URL to learn the owner, and nothing distinguishes a
personal repo from one owned by an org. Wanted: repo name, owner, and the org
name when org-owned, in color and bold, right-aligned on the same line as
`synced   ~/path`.

### The blocker: the owner login is never fetched

`RepoKind` (`src/model.rs:172`) knows *whether* a repo is org-owned, private,
or public, but not *who* owns it. The GraphQL query asks only for the owner's
type:

```graphql
owner { __typename }    // src/github.rs:158
```

So "Organization" vs "User" is known and the login is thrown away. Two ways to
get the name, and they are complementary rather than competing:

1. **Add `login` to the existing selection**: `owner { __typename, login }`.
   Same request, same rate-limit cost, no extra round trip. Then carry
   `owner: Option<String>` through `GithubRepo` (`src/github.rs:8`) into
   `RepoStatus` (`src/model.rs:105`). Authoritative, and it is the smallest
   possible change to the query.
2. **Parse `remote_url`** (`git@github.com:Gaurgle/fleetz.git` -> `Gaurgle`).
   Covers local-only repos that never matched a GitHub result, and works with
   no network at all. Needs care across SSH, HTTPS, scp-style, and
   self-hosted-host forms.

Do both: prefer the API login, fall back to the parsed remote, show nothing if
neither exists. That way org repos are exact and local-only clones still get
an owner instead of a blank.

### Layout

The target line already exists at `src/ui/detail.rs:299-306`. `width` is in
scope (`:284`), so right-aligning a trailing group is the same fill trick the
repo list already uses for its dot-aligned badges
(`src/ui/detail.rs:200-213`): measure the left spans, measure the right
group, insert a spacer span of the difference.

Note that `Line::right_aligned()` is not usable here. It pushes the *whole*
line right, and this line has to stay left-anchored on `● synced   ~/path`.

```
 ● synced   ~/repos/fleetz                          Gaurgle/fleetz
 ● synced   ~/repos/some-service              Acme  Acme/some-service
```

Colors should reuse the vocabulary already established by
`badges::glyph_span` (`src/ui/badges.rs:26-31`) rather than inventing a second
scheme: `MAUVE` for org-owned, `PEACH` for private, `GREEN` for public. Owner
bold, repo name bold, separator dim. Rendering the matching Nerd Font
building/lock/globe glyph ahead of the owner would carry the kind visually and
costs one call to a function that already exists.

### Open questions

- **Title or body?** The panel title already right-aligns the CI badge
  (`src/ui/detail.rs:280`), so `owner/name` could live there instead, next to
  it. The body keeps the title short but duplicates the name, which is already
  in the title at `:268`. Body is what was asked for; worth eyeballing both.
- **Redundant with the remote URL line?** Once `Gaurgle/fleetz` renders
  explicitly, the `git@github.com:...` line below it carries little extra
  signal. Candidate for dropping, or for shortening to just the host so
  self-hosted remotes stay visible.
- **Narrow panes.** In compact view or a narrow terminal the right group will
  collide with a long path. Needs a drop order: shed the owner prefix first,
  then truncate the path (`crate::ui::truncate` already exists).
- Whether a repo with no GitHub match and no remote should render a dim
  placeholder or nothing at all.

---

## Custom command keybindings

The next five sections came out of a comparison with
[gh-dash](https://github.com/dlvhdr/gh-dash) (2026-08), a GitHub PR/issue
dashboard TUI. The two tools have opposite centers of gravity (gh-dash is
GitHub-search-side and knows nothing about local clones; fleetz is fleet-side
and keeps GitHub deliberately thin), but a handful of their mechanisms map
cleanly onto fleetz. Each section notes where the idea came from and what was
changed in translation.

**Problem.** fleetz hardcodes exactly three ways to act on the selected repo:
`Enter` (lazygit), `e` (`$EDITOR`), `o` (a shell). Anything else you do to a
repo daily (a build, `gh pr create`, a tmux window, a deploy script) means
leaving fleetz and typing the path yourself. Every new hardcoded key is a code
change, a help-overlay line, and a fight over the shrinking pool of free keys,
and it bakes one person's toolchain into the binary.

gh-dash's answer is user-defined keybindings in config: a key, a shell command
with template placeholders, and an optional display name for the help menu.
That shape fits fleetz's config.toml directly:

```toml
[[commands]]
key = "B"
name = "build"
run = "cargo build --release"

[[commands]]
key = "T"
name = "tmux window"
run = "tmux new-window -c {path}"
```

### What already exists

The hard part is done. `suspend_and_run` / `suspend_and_run_in_dir`
(`src/main.rs:1219`, `:1240`) leave the alternate screen, release the input
gate, run a child that owns the terminal, and restore. The three hardcoded
keys all go through them; a custom command is one more caller with the command
line built from config instead of a literal. Run in the repo directory via
`suspend_and_run_in_dir` and most commands need no placeholder at all.

Placeholders should start minimal: `{path}` (expanded working directory),
`{name}` (repo name), `{owner}` (may be empty for local-only repos). gh-dash
exposes PR fields too (`{{.PrNumber}}` etc.); fleetz has no selected-PR
concept, only a selected repo, so PR placeholders wait on the enrich-on-select
work below giving the inspector a cursor of its own.

### Collisions and the help overlay

A config key that shadows a builtin (`f`, `r`, `q`...) must be a startup
error naming both meanings, not a silent override. gh-dash lets rebinding
shadow builtins; fleetz should not, because the README documents the builtin
set as stable vocabulary and a config file that flips `q` to "deploy" is a
support incident. Unknown placeholder in the template: also an error at load,
not at keypress (gh-dash gets this right with `missingkey=error`).

The help overlay (`ui/help.rs`) gets a `custom` section listing `key name`
pairs, name falling back to the command truncated. The footer should not grow:
custom keys are the user's own and need no reminder there.

### Rejected

- **Rebinding builtin keys.** All of gh-dash's keybinding machinery hangs off
  this, and it is the wrong half to copy. The builtins are documentation-stable
  and mnemonic; the demand is "add my command", not "move fetch".
- **A command palette / leader key.** Solves key scarcity, but it is a second
  interaction grammar for a tool whose whole model is one-keypress actions.
  Revisit only if users genuinely run out of capital letters.
- **Per-context command sets** (repo-list commands vs branch-overlay commands).
  gh-dash needs this because its views have different row types. fleetz's
  actions all take a repo; one flat list is enough until the inspector grows
  its own selection.

### Open questions

- `[[commands]]` array-of-tables vs a `[commands]` map keyed by key. The array
  keeps order for the help overlay; the map makes duplicate keys a TOML-level
  error for free.
- Should a command be able to opt out of the suspend (fire-and-forget, like a
  `tmux new-window` that never takes the terminal)? A `background = true`
  flag is cheap, but silent failures of background commands need somewhere to
  land (the footer error slot?).
- Does the settings overlay eventually list these, or is config-file-only fine?
  (They are the definition of a hand-edited power feature; leaning file-only.)

---

## Enrich-on-select: real PR detail in the inspector

**Problem.** The batch GraphQL query fetches five PRs per repo with the fields
a list line needs (`src/github.rs:150`): number, title, author, age, review
decision. That is the right cost shape for fifty repos, and the invariant says
never widen it per-repo. But it leaves the inspector unable to answer the
questions you actually open a PR for: which checks failed, what the review
threads say, how big the diff is, whether it conflicts. Today the answer is
`O` (browser), a full context switch.

gh-dash resolves the same tension with two tiers: a cheap batched list query,
plus a lazy per-row `resource(url: $url) { ... on PullRequest }` fetch that
runs only when a row is selected, cached per URL, cleared on refresh. That is
fleetz's own "batch on GraphQL, target on git" invariant, extended to GitHub
itself: batch for the fleet, target for the thing under the cursor.

### Shape

- A second, narrow query in `github.rs`: one PR by URL, fetching check runs
  with names and conclusions, review threads, files changed with +/- counts,
  `mergeable`. One node, so about the same rate-limit floor as any call; it
  runs at human selection speed, not at poll speed.
- Trigger on selection dwell (say 300ms after the cursor lands, reusing the
  tick), not on every `j` keystroke through the list.
- Cache in memory keyed by PR URL plus `updatedAt`, so a PR that has not moved
  is never fetched twice in a session. `r` keeps the cache (it is keyed on
  updatedAt, so staleness self-corrects), `R` drops it with everything else.
- Needs a PR cursor: today the detail pane lists PRs but nothing selects one.
  That cursor is also what custom commands and an `O`-on-selected-PR need, so
  it pays for itself three times.

### Rejected

- **Widening the batch query.** Fifty repos times checks-and-threads is paid
  on every poll for data nobody is looking at, and it bloats the disk cache.
  The invariant exists precisely to forbid this.
- **Prefetching detail for all visible PRs on load.** Same problem with a
  delay; selection is the only honest signal of interest.
- **Shelling to `gh pr view --json` per selection.** Works, but the GraphQL
  client, auth, and error path already exist in `github.rs`; a second
  transport for the same API is a drift farm.

### Open questions

- Where does detail render: the inspector pane (natural, but it is currently
  commit-oriented) or a new overlay like branches? The pane keeps the
  three-panel model; an overlay fits "one PR, temporarily, in depth" better.
- Does the enrichment belong in the disk cache too, so reopening fleetz shows
  the last-known detail instantly? Leaning no: it is per-selection data, and
  the memory cache plus the fast query covers the real case.
- How much of gh-dash's tab carousel (overview / activity / commits / checks /
  files) is worth having? Checks and files earn their place for a dashboard;
  full comment threads may be the browser's job after all.

---

## Richer PR rows: the free fields

**Problem.** A PR line renders number, title, author, age, and review status
(`src/ui/detail.rs:753`). Missing, and cheap: whether it is a draft, whether
it conflicts with its base, and how big it is. A draft currently looks
identical to a ready PR, which misreads exactly the way the double check mark
used to.

The invariant already prices this: adding fields to the existing GraphQL query
is free, adding a request is not. gh-dash's list rows carry `isDraft`,
`mergeable`, `mergeStateStatus`, `isInMergeQueue`, `additions`, `deletions`,
and comment counts on the same search call; the equivalent fields slot into
the `pullRequests(...)` selection at `src/github.rs:150` at zero extra
rate-limit cost.

### Worth taking, in order

1. **`isDraft`.** Render the whole line dim (gh-dash's choice, and the right
   one: a draft is a de-emphasized state, not another badge). Possibly a
   `draft` word where review status would go, since review decisions on drafts
   are moot anyway.
2. **`additions` / `deletions`.** A `+120 -34` tail, abbreviated past 1k
   (`+31.4k`), in the GREEN/RED the diff vocabulary already owns. Answers "is
   this a five-minute review" from the list.
3. **`mergeable` / `mergeStateStatus`.** A conflict marker on the line. This is
   fleet-relevant signal: a PR that went stale into conflict is the thing a
   dashboard should surface without being asked.
4. **`comments.totalCount`**: probably not. It is activity, not state, and the
   line is a fixed budget (`title_budget` is already doing truncation math).

### The cache wrinkle

The disk cache stores the raw response JSON and its path varies by query shape
(`cache.rs::cache_path_varies_by_query_shape`). New fields must deserialize
as `Option`/default so a pre-upgrade cache file read within its TTL does not
error; they render as absent for at most `cache_ttl` seconds and then are
simply there. No migration, but the serde defaults are load-bearing and need
the test that proves an old-shape JSON still parses.

### Rejected

- **A separate PR-detail query for these.** They are list-line fields; paying
  a request for them inverts the cost model. The detail query (previous
  section) is for data too heavy to batch, and these are not.
- **New columns in the repo list itself.** The repo line's badge group is
  repo-state vocabulary; per-PR facts belong on PR lines in the detail pane.

### Open questions

- Does the `prs` count badge on the repo line keep counting drafts? Arguments
  both ways; gh-dash sidesteps by being query-driven. A dim `2+1d` reads well
  but is a new counter format against the "counters are numeric" rule.
- Is `mergeStateStatus` (which folds in CI and review) better than bare
  `mergeable` (conflicts only), or does it double-render state the CI lamp
  already shows?

---

## CI completion notification

**Problem.** Watch mode already tightens the GitHub poll to 15 seconds while a
CI run is in flight and immediately after a push, so fleetz knows within
seconds when a run finishes. All of that awareness currently ends at a color
change in a pane you have to be looking at. The one moment a dashboard should
tap you on the shoulder (the run you just pushed finished, and it failed) is
the moment you have switched away to work.

gh-dash binds `w` to `gh pr checks --watch` plus a desktop notification when
it completes. The mechanism is worth taking; the architecture is not: fleetz
already runs the poll that detects the transition, so spawning a watcher
process per PR would duplicate it.

### Shape

The poll diff already computes state transitions per repo. A notification
fires when a repo's `CiStatus` goes `Pending -> Passed` or `Pending -> Failed`
during watch mode. That is the entire trigger; no new polling, no child
processes.

Delivery, in escalating order of dependency weight:

1. **Terminal bell (`\x07`).** Zero dependencies, tmux users get their pane
   flagged, and it works over SSH. Nearly free to ship first.
2. **OSC 9 escape sequence.** Real notification text on terminals that support
   it (iTerm2, kitty, wezterm), still no dependency, silently inert elsewhere.
3. **A desktop-notification crate** (`notify-rust` or similar). What gh-dash's
   `beeep` does. Real notifications everywhere, but it is a new dependency
   (house rules: ask first) and behaves differently over SSH.

Config: `notify = "off" | "bell" | "desktop"`, default off. A dashboard that
starts making noise on upgrade is rude; this is opt-in.

### Rejected

- **Per-PR watcher processes** (`gh pr checks --watch` per PR). Duplicates the
  poll fleetz already runs, one process per PR, and the results arrive on a
  channel nobody owns.
- **Notifying on every transition for every repo.** Fifty repos of org
  activity would make the feature its own DoS. Scope below.
- **Polling faster instead.** Latency is not the problem; attention is. The
  15s adaptive floor is already tight enough.

### Open questions

- Scope: all watched repos, or only repos whose last push came from this
  machine (the poll already detects local pushes to force an immediate
  re-poll, so "recently pushed locally" is computable)? Leaning the latter as
  the default; a run you triggered is a promise made to you, a colleague's
  run is not.
- Failures only, or completions too? Failure-only is the calmer default;
  pass notifications earn their keep exactly when you are waiting to merge.
- Does the bell tier fire on the same conditions as desktop, or is bell cheap
  enough to fire on more?

---

## Snooze, not hide

**Problem.** The settings-menu section plans hidden repos as phase 3 and
immediately names the trap: a `hidden = [...]` list makes a hidden repo
unrecoverable from inside the TUI without also building an unhide screen. The
deeper problem is that "hide forever" is rarely what the itch is. The real
case is "this repo is noise right now": an archived experiment that sits in
the list, a fork you touched once. Permanent hiding of a repo that later gets
a push is exactly how a dashboard starts lying.

gh-dash has a pattern worth stealing from its notifications view: "done" is
stored locally as `id -> updated_at`, and an item marked done automatically
resurfaces the moment it has newer activity, with entries pruned after 90
days. Hiding is temporary by construction, so no unhide UI is needed for the
common case.

### Translation to repos

A snoozed repo stores its `pushed_at` at snooze time. It is filtered from
`App::visible_repos` while its current `pushed_at` still equals the stored
one, and reappears (with a one-time marker, say a NEW-style badge for one
sweep) the moment the remote moves. Local activity should also wake it:
if the working tree goes dirty, you are in that repo, and a dashboard hiding
a repo you are actively editing fails its one job.

Storage follows the `window_state.rs` precedent: a machine-local state file
(`~/.cache/fleetz/`), not config.toml. Snooze is session-state-shaped, like
the remembered time window; putting it in config would make the config file
lie about being the input you wrote.

Keys: `h` (hide/snooze) is free. Unsnooze-all can live on the settings
overlay or as `H`; because snooze self-expires, the unhide screen stops being
a blocker and becomes a nicety (a dim `3 snoozed` count in the footer or
header keeps the state from being invisible).

### What remains of permanent hiding

Probably a config-file-only `hidden = [...]` for the true never-show cases
(archived repos you keep cloned). No in-TUI key sets it, so nothing needs an
in-TUI undo: you edit the file both ways. The interactive surface is snooze
alone, which cannot strand anything.

### Rejected

- **Hidden list alone (the original phase 3).** The trap the settings section
  already documents: needs an unhide screen just to be safe to ship, and it
  goes stale silently as hidden repos accumulate activity.
- **Time-based snooze ("hide for a week").** Simpler to explain than
  activity-based, but wrong on both edges: the repo comes back unchanged (why
  is it back?) or stays hidden through a push (the lie again). Activity is the
  honest clock. Could be added later as `h` with a count prefix if wanted.
- **Snoozing sections or groups.** The group filter (`g`) already covers
  wholesale scoping; snooze is per-repo noise control.

### Open questions

- Does local dirtiness wake a snoozed repo, or only remote `pushed_at`
  movement? (Argued yes above, but it means snooze state consults local
  status, which couples the filter to sweep timing.)
- Should a snoozed-but-woken repo carry a marker until acknowledged, or is
  reappearing enough? A marker needs an acknowledge interaction, which is new
  vocabulary.
- Prune horizon for stale snooze entries (gh-dash uses 90 days), and whether
  pruning matters at all at fleet sizes of tens rather than thousands.
- Does `#snoozed` become a tag so the strip can show them on demand, instead
  of a dedicated unhide list anywhere?
