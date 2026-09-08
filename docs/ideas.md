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

The **github activity** section gates on `pushedAt`, the cutoff applied in
`assemble_view` before discovery runs.

The **local changes** section was originally exempt. The argument was that it
is about uncommitted/unpushed work rather than push recency, so narrowing
would hide dirty repos you are actively working on. That held for the repo you
dirtied this morning and failed for its opposite: `~/Repos/Rust`, `bonsai` and
`AoC-2025` were each `git init`-ed, filled with untracked files and abandoned,
so they reported dirty forever and held a row forever with it. Three permanent
rows in a section that is supposed to be a worklist.

The mistake was reading "dirty" as "being worked on". The section is right to
ignore *push* time and was wrong to ignore time altogether, so it now gates on
a local clock instead: `git::last_local_activity`, the newer of the last
commit on any ref and the newest mtime among the paths `git status
--porcelain` reports. Both halves are load-bearing. A repo with no commits has
no commit to date it by, which is exactly these three, and a repo dated by its
last commit alone reads as stale while you are still editing files in it.

Unpushed commits are exempt from the cutoff (`local_repo_is_current`). Left
untracked files are noise you chose to ignore; a commit that exists only on
this machine is one `git push` from being lost with it.

The same timestamp is kept on `RepoStatus::last_local` and drawn by `info` as
a `last touched` row, so the date on screen is the one the window judged the
repo by rather than a second measurement that could disagree. Only local-only
repos carry it: computing it for GitHub-backed repos would cost two git calls
each per sweep to restate what `pushedAt` already says. `-d` mode builds its
rows through `dirty_mode_status`, which does not compute it, so that mode
draws no such row.

Section order is **github activity, local changes, also active**. Both
`App::section_rank` and `render_repo_list` encode it and
`drawn_section_order_matches_the_order_the_cursor_walks` holds them together;
before that test, reordering one and forgetting the other misrouted clicks
with nothing failing.

Untracked directories arrive from porcelain collapsed (`blue-tui/`, not every
file beneath it), so the stat reads the directory mtime. That moves when
entries are added or removed but not when a file's contents change in place,
so editing deep inside an untracked directory without adding or removing
anything will not refresh the timestamp. Accepted deliberately: the
alternative descends `target/` and `node_modules` on exactly the repos being
cheaply evicted.

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

**Full width shipped later** (`detail::split_inspector`, which replaced
`split_right_column`). The inspector now docks under the repo list *and* the
detail pane, so the percentage is of the whole main area and the two panes
share the row above. The list is rarely tall and already scrolls, so giving
its lower rows to the inspector costs little; the inspector wants width more
than anything, and at an interior of `INSPECTOR_TWO_LANE_WIDTH` (100) columns
or more it splits into a message lane on the left and a changed-files lane on
the right, one scroll offset driving both and clamped to the taller. The
contribution strip comes off the bottom of the whole area, under the
inspector, at full width; the wide-terminal rule that docked it under the
right column alone (`worth_docking_in_column`) went with the column it
docked under.

Net effect: on a 40-row terminal the inspector grows from 10 rows to ~15, and
on a 60-row one to its 24-row ceiling, instead of staying at 10 either way.

**Interactive resize shipped too.** `<`/`>` move the vertical divider, `+`/`-`
the horizontal one, `=` resets both. Both are percentages on `App`
(`repo_list_pct`, `inspector_pct`) clamped to ranges that keep either pane from
collapsing, and `split_main` joined the inspector split as shared geometry.

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
  `split_inspector` exists to avoid.

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

---

## Hidden repos in the `$HOME` sweep

**Problem.** A clone at `~/.dotfiles` is invisible to the `$HOME` sweep, so it
reports as `not cloned` even though the GitHub row is right there. The only
cure today is naming it in `extra_dirs`, which requires knowing the key exists
and repeating it on every machine.

The skip is deliberate (`discovery::scan_recursive`, `skip_hidden`) and the
CLAUDE.md invariant documents it as a cost decision: descending into every
`~/.local` and `~/.npm` "costs far more than it finds". Both halves of that
were measured on 2026-08-08 and neither held up.

**Cost.** `find $HOME -maxdepth 4` with the `PRUNE_DIRS` list applied:

| | dirs walked | cold | warm |
|---|---|---|---|
| skip hidden (today) | 1,238 | 0.25s | 0.02s |
| include hidden | 3,165 | 0.23s | 0.05-0.07s |

2.5x the directories for ~30ms warm, and nothing cold, where I/O dominates.
The sweep is already async and off the UI thread.

**Noise.** 15 git repos live under hidden paths in that `$HOME`. Thirteen are
vendored plugin clones (nine under `~/.tmux/plugins/`, plus `~/.oh-my-zsh`, a
`~/.claude/plugins/marketplaces/` checkout, `~/.codex/.tmp/plugins`). None of
them would surface: pass 2 keeps only directories whose basename matches a
GitHub repo name, and none of those names is one. The name filter is already
doing the work the hidden skip was added to do.

**repoz has never had the skip.** `scan_home_git_dirs` prunes by name only.
Same architecture on both sides (name-matched sweep for GitHub rows, configured
roots only for local changes), so repoz has been running the proposed behavior
for its whole life without noise. fleetz's skip is the divergence, and the
`~/.dotfiles` bug is its only observable effect.

### Options

1. **Depth-1 hidden check (recommended).** Test dot-directories that sit
   directly in `$HOME` for being a repo, but still never descend into them.
   `~/.dotfiles` costs one stat among ~60 dot-dirs. Every vendored clone above
   is at depth 2+ inside a hidden dir, so they stay invisible by construction,
   not by luck. Catches the convention people actually use without inheriting
   the plugin-tree exposure.
2. **Full repoz parity.** Drop `skip_hidden` from the `$HOME` sweep entirely.
   Simplest rule to state, removes the divergence, and `repoz_parity` gets a
   real hidden-nesting fixture instead of only the hidden-root one. Slightly
   more exposure: a vendored clone whose basename collides with a repo you own,
   where you have no real clone, would win the row.
3. **Config only.** Leave the code, document `extra_dirs = ["~/.dotfiles"]` in
   the README as the answer. Zero risk, but the tool stays wrong by default for
   the single most common hidden-clone convention there is.

### Rejected

- **Special-casing the literal name `.dotfiles`.** Fixes one spelling and
  teaches nothing. `matched_name` already handles the dotted-clone spelling
  generically; the gap is the walker, not the matcher.
- **Raising `PRUNE_DIRS` to cover plugin dirs (`.tmux`, `.oh-my-zsh`, ...).**
  An open-ended blocklist that has to grow forever, and `PRUNE_DIRS` is kept
  in step with repoz's `PRUNE_EXPR` by hand, so every addition is two edits and
  a parity risk.

### Open questions

- Does option 1 want depth 1 specifically, or "hidden dirs are checked but
  never descended into" at any depth? The latter is one condition instead of
  two, and on the measured tree it yields the same result.
- Should a match found under a hidden path be visually marked, so a row
  pointing at a vendored clone is at least legible as odd?
- Pass 2 has no dedupe (pass 1 does, first root winning). If two hidden
  candidates ever match one name, which wins is currently walk order. Worth
  settling before widening what pass 2 can see.

---

## Contribution sources beyond GitHub

**Problem.** The strip has two sources, `Source::Github` (green) and
`Source::Local` (blue). GitLab is the obvious third, and its brand orange is
free of both existing ladders. The question is whether the data behind it is
affordable, and what it would actually add.

**What the colour costs: nothing.** An orange ladder is already solved, on the
same evenly spaced L* steps as the other two and holding more chroma than the
green at every level:

```
(80, 58, 43)  (152, 93, 52)  (219, 134, 74)  (247, 196, 159)   hue 25
```

Hues then sit at 112, 212 and 25, so the widest gap is 87 degrees and the three
stay separable under red-green deficiency. `Source` reaches 6 files and 39
sites, all mechanical. None of this is the hard part.

**What the data costs.** GitHub's calendar rides the poll that already runs, as
aliased fields with no `first:` connection, so it is free (see the "GraphQL
bills the `first:` you ask for" invariant in CLAUDE.md). Nothing else offers
that:

- **GitLab** has no contribution-calendar endpoint. The profile calendar is
  HTML only; the API gives paginated `/users/:id/events` to aggregate yourself.
  Many requests where GitHub costs zero, plus a second token, second config
  surface, second rate limit to mismanage.
- **Bitbucket** has no contribution-graph concept at all; it would have to be
  synthesised from commits across repos. Its brand blue (#0052CC) also collides
  head-on with the local ladder, so it could not wear its own colour.

**The question that decides it.** `Source::Local` already counts commits in
cloned repos *regardless of host*, so GitLab work in a cloned repo is on screen
already, in blue. The genuinely new information is the non-commit part: merge
requests, issues, reviews, and activity in repos that were never cloned. That
may be worth a whole API client and it may not, but "add GitLab support" sounds
much bigger than what it actually adds.

**Open question.** Bitbucket (Atlassian) or GitBucket (the self-hosted Scala
GitHub clone)? Not the same thing, and it changes the estimate sharply:
GitBucket implements a GitHub-compatible API, so it could plausibly reuse the
existing client behind a base-URL config key rather than needing a new one.

---

## The behind count and the branches that make it up

**Status: shipped.**

**Problem.** A repo row read `40↓` while `git pull`, run in that repo, reported
nothing to pull. Both were right. `RepoStatus::behind` is folded over every
local branch (`src/main.rs`), so the 40 was 21 on `beta` plus 19 on
`feature/who-goes-first`, and `main`, the branch actually checked out, was
level. Nothing on screen said the number was a sum, so the only reading
available was the wrong one.

The `info` view already drew the per-branch breakdown that resolves it. Three
separate things kept it from landing:

1. **It scrolled away.** Only the identity header was pinned. Stepping the
   commit cursor scrolled the branches block off the top, leaving the
   remote-only "not fetched" list as the first thing under the header, which
   reads like the complete branch story and is not.
2. **`origin/wip` was reported "not fetched" when it was fetched.**
   `unfetched_branches` matched remote refs to local branches by bare name
   only. `feature/who-goes-first` tracks `origin/wip`, no local branch is
   called `wip`, so the ref the repo was 19 commits behind on was labelled as
   one nobody had ever pulled.
3. **The summary never admitted it was a sum.** `long_status` said
   "40 behind" whatever the spread.

**What shipped.**

- `unfetched_branches` takes `&[BranchStatus]` and counts a remote as fetched
  when a local branch either shares its short name or names it as upstream.
  The upstream test compares the full `origin/x` form, so a same-named branch
  on a different remote is not swept up by it.
- The branch row prints `-> origin/wip` when the upstream differs from the
  branch's own name, and stays bare when it does not.
- The pinned region extends through the local branches. The count is chosen
  from a ladder (`[pinned_rows, header_rows]`, first that clears
  `MIN_BODY_ROWS`) rather than a single test, so a repo with many local
  branches falls back to pinning the header alone instead of losing the pin
  entirely.
- `long_status` says "40 behind on 2 branches" when more than one branch is
  behind, and stays unqualified when one is.

**Rejected: pinning the whole branches block.** The remote-only list is bounded
by what everyone else has pushed, not by what you have. On the repo that
prompted this it is eleven rows, and pinning it would have taken most of the
pane on exactly the busy repos whose history you need.

**Not done.** `short_status` still renders a bare `40↓` in the repo list. The
glyph column has no room for the qualifier, and the row is a glance rather
than an answer, but it is the surface the confusion started on.

---

## The watch pulse, and where the CI lamp pulses

**Status: shipped.**

**The watch indicator now pulses colour, not shape.** It used to alternate `◉`
and `●` in one green, on the reasoning recorded in `ui/mod.rs`: a changing
shape set it apart from the CI lamps, which hold a shape and say what they mean
by colour. In use that was backwards. The indicator is only ever read at the
edge of vision, and a one-cell shape swap is the harder of the two changes to
catch there. It is now a steady `◉` pulsing between Catppuccin `red` and
`maroon` (`colors::WATCH_ON` / `WATCH_OFF`), a half-step apart.

Two things the new shape has to answer for:

- **Red is CI-failed's colour.** What separates them is the word `watch`
  printed beside the glyph, which a CI lamp never has. The old design leaned on
  the glyph for that job; it did not have to.
- **Two hues rather than a colour and a dimmed twin.** A pulse that drops
  towards the background reads as something failing to draw. Both phases are
  fully lit, so it reads as a heartbeat.

Only the lamp pulses. The word `watch` beside it holds a steady `WATCH_ON`, on
the same terms as `spinner_group`: a label that moves with the glyph makes the
whole group flicker, which is motion where the eye is trying to read a word
rather than watch a light.

`WATCH_ON` is the same RGB as `RED` and is deliberately a separate constant.
`RED` means CI failed. One name carrying two meanings is how a palette starts
drifting, and either should be free to move without the other.

**The detail pane's CI lamp pulses too.** `ci_display` took no blink phase, so
pending blinked in the repo list and held still in the pane below it, for one
repo's one job. It now takes `dim_pending` on the same terms as `badge_spans`
(`app.watch_mode && !app.blink_on`), gated on watch mode for the same reason:
a pulse claims something is happening now, and outside watch mode the data is
as old as the last sweep.

**Rejected: pulsing settled states.** Passed and failed are results, not
activity. Only pending moves.

---

## Ragged churn columns in the commit list

**Status: shipped.**

**Problem.** The `+N -N` tail was one right-aligned blob. With `+2 -2` above
`+338 -21`, every row's `+` began in a different column, so the one field on
the row that is read by scanning down the page was the one field left ragged.

**What shipped.** The two halves get their own right-aligned columns, measured
across the list the way `hash_w` and `lane_w` above them already were
(`stat_part_width`, `stat_tail_width`). Rows with churn on one side only draw
blanks for the other, which is what holds the alignment when a delete-only
commit sits under an add-only one, and a row with no churn still reserves the
columns so its leader dots stop where the `+` column begins.

**Measured over the rows the call will draw, not the whole history.** `info`
draws six of a hundred and fifty; measuring all of them would reserve width for
churn nobody can see.

**A second bug underneath the first, and the reason it looked width-dependent.**
The row budget reserves two spaces, one before the subject and one before the
leader dots, but the second was drawn only inside the `dot_count > 0` branch. A
subject that filled its whole budget leaves no dots, so that row came out one
column short and slid its churn tail left by one. Whether a subject truncates
depends on the pane width, so the tail looked aligned at one size and ragged at
another, which is how it was reported. The separator is now drawn whenever the
row fits, dots or no dots, and `every_commit_row_reaches_the_pane_edge` states
the invariant directly: every commit row draws to exactly the pane's width.

**Rejected: reserving the space unconditionally.** `dot_count` saturates to
zero on a row already over budget -- a graph deep enough to overrun the pane --
and handing that row another column to overrun it with makes the wrap it
already causes worse.

---

## Making the view names read as headings

**Status: shipped.**

**Problem.** The tab strip and the pane's own headliner both drew the view name
in lower case at body-text size, so the thing that says what you are looking at
looked like more body text.

**The constraint.** A TUI cannot change font or point size. The terminal owns
both, and every cell is one grid unit. "Bigger" has to come from how the
letters are arranged inside that grid, which leaves case, weight, letterspacing,
double-width glyphs, and multi-row block banners.

**What shipped.** One form from one source (`view_name`): capitals, one cell per
letter, drawn identically by the tab strip and the headliner. Capitals are the
same number of cells, so `view_switcher`'s fit budget is untouched and the strip
still shows all four everywhere it did before.

**Tried and dropped: letterspacing the headliner** (`I N F O`). It read as
decoration in a pane whose every other line is dense, and once the headliner was
boxed the box was already doing the work of setting it apart. Letterspacing the
tabs as well was never on: it roughly doubles a strip that shares a one-row
border with the pane title, collapsing it to the active tab alone on panes where
all four fit today.

**Rejected: fullwidth Unicode forms (U+FF21..).** The only option that makes the
glyphs literally larger, and the only one with a font-coverage risk: a monospace
font without fullwidth Latin renders them as tofu. Against the grain of the
width discipline `the_commit_node_occupies_one_column` already sets.

**Rejected: a block-drawn banner.** Three rows of a pane that scrolls, and the
strip is a one-row border that could not take it at all, so the two would stop
agreeing about what the view is called.

**The headliner is the name over a divider that fades.** The divider starts in
the view's own colour, the same one its tab wears in the border, and blends into
`SURFACE` across the pane a cell at a time.

```
INFO
─────────────────────────────────────────────────────
branches
* main ✓
```

The fade goes through `ui::blend`, the same function the spinner walks its
palette with -- that one interpolates over time and this one over distance, but
it is the same arithmetic and there was no reason to write a second one. Flat
`SURFACE` was the first version, on the rule of thumb that structure gets the
structure colour. Wrong here: it read as a separate dim line that happened to
sit under the name rather than as the name's own underline running out.

**Three shapes were tried on the way here, in this order.** A closed box with
the rules carrying to the pane edge; then the same with the top rule and the
closing bar dropped, leaving an open bracket; then this. Each step removed
something that was drawing attention without saying anything: the top rule
repeated what the foot said, the closing bar boxed in a word nothing was
crowding, and the leading bar indented the name two columns so the one line
that heads the body lined up with no line of it.

**What it costs.** Two pinned rows instead of one, so the scrolling body loses
a line at every height. The threshold at which the branches block still clears
`MIN_BODY_ROWS` is unmoved at 20 rows -- the extra row landed inside slack that
was already there. The boxed version, at three rows, did push it to 21.
`the_branches_pin_holds_down_to_a_twenty_row_pane` states the threshold
directly, so the next row added to the pinned region is a decision rather than
a surprise.

**Note on the tests.** Seven render tests pinned the lowercase literals. They
now ask `view_name` what to expect, with `a_view_name_is_drawn_in_capitals`
pinning the rule itself and `capitalising_a_tab_does_not_change_its_width`
pinning the equal-width property the strip's fit budget depends on.

One consequence worth keeping in mind: because the strip and the headliner draw
the same string, `contains("GRAPH")` is true in every view and asserts nothing.
The tests that need to find the headliner match its boxed form,
`│ GRAPH │`, which only the headliner produces.

---

## Scroll feel: the lag, the wandering highlight, and Shift+scroll's target

**Status: shipped.** Three separate problems, all reported as "scrolling feels
wrong".

### 1. The wheel lagged, then ran on

**Cause.** The event loop painted once per event, and the event channel is
unbounded. A trackpad delivers wheel events far faster than a full repaint
takes, so a flick queued faster than it drained: the pane appeared to freeze,
then played out the whole backlog after the fingers had stopped.

**Fix.** Paint once per *burst*. The events are cheap; it is the repaint that is
not, so a pending paint is held while more input is already queued and
`needs_redraw` carries to the next pass. `should_paint` is the whole rule, and
it is a free function so it can be tested without an event loop. The hold is
bounded by `MAX_COALESCED_FRAMES`, or a source that never stops arriving would
never paint at all.

**Not done: bounding the channel.** A bounded channel would drop or block
instead, and dropping input is worse than deferring a frame.

### 2. The highlight wandered while scrolling

**Cause.** `scroll_detail` called `keep_selection_on_screen` on every notch, so
the wheel dragged the selection to the nearest visible entry. That silently
reassigned which commit the inspector was reading out: the highlight crept down
the pane on its own and the panel below it changed under you.

The original reasoning was sound as far as it went -- without it the highlight
left the pane and `j` then stepped a commit you could not see -- but the fix
was aimed at the wrong gesture.

**Fix.** Scrolling is a view gesture and now leaves the selection with the
content it is attached to, exactly as it leaves every other row. The re-entry
moved to the *step*: the first `j`/`k` after the selection has gone off screen
lands it at the edge you are looking at and does not also advance, so the
highlight appears where the eye already is before it starts moving again.
`reenter_selection` is the old function, returning whether it moved so the step
can spend itself on it.

**The graph view is deliberately unchanged.** Its cursor *is* its scroll
mechanism -- the view follows the cursor rather than the other way round -- so
scrolling moving the highlight is coherent there rather than surprising.

**Two tests were rewritten, not relaxed.** `scrolling_drags_the_selection_to_
the_nearest_visible_entry` and `scrolling_never_leaves_the_selection_off_the_
pane` pinned the old contract exactly. They now pin the new one: scrolling does
not move the selection, and one step brings it back.

### 3. Shift+scroll drove the wrong pane

Shift+J/K scrolled the *focused* pane from the detail pane, and stepped window
2's cursor from the repo list -- two different ideas on one key. It now
consistently drives the **next** pane: list → window 2 → inspector → back to
the list, via `shift_scroll_target`.

**The contributions strip is skipped rather than included as a no-op.** It is a
fixed-height horizontal timeline with nothing below the fold, so there is no
vertical scroll to aim at it; leaving it in the cycle would have made the key
do nothing at all from whichever pane sat before it. Compact has no pane after
the list, so the target is `None` there rather than a pane that is not drawn.

**The graph aims at itself.** It docks no inspector, so the first version
wrapped it round to the repo list -- which spent the gesture on changing repo.
The graph is the only view whose rows are finer than its commits (plain `j`/`k`
jumps whole commit blocks, because `set_detail_selected` puts the row cursor on
the next block's top row), so it is the only view with a line-by-line step
worth having. The shifted keys stay on the pane and walk `graph_cursor` one row
at a time.

**What moved rather than went.** Scrolling the focused detail pane itself is
still `PageUp`/`PageDown`, and the mouse wheel over a pane still scrolls that
pane. Only the shifted keys were repointed.

---

## The intro wave

**Status: shipped.**

Once per launch, the first painted screen arrives as a diagonal wave of
scrambled characters that resolves into the content. The screen starts *full*
of noise and ends sparse: most cells resolve to blank, and what is left
standing is the dashboard.

```
t=0.45                      t=0.70                      t=1.0
Dbv.cZH~DOwGM&hC]jo%jk      ......kkiQ;vRC^@P()W0
NFiiWuj/Ec!)+<XoX>7QMV        ....h~xT$im#b%;K4H*W
?_HBs|yRpcZqO~#:_c.)9N          ..+5xgYFawnk-a@9mk
N}X@[y!oaU)HL%7j/r#fM-      ..branches  OY^j@VM<H      branches
4k~kG18l$w0t@x@g[dB|Bu      ..* main      {/H>+dH      * main
  Ri1us?1Q2@osI:FMtat|                  BrSVvdl}v
    &fss#rkV}vC3lv<.Dp                    ?_tqDmL
      SGayph;t8X4G|HXA                      3C<]5
```

**The shape that makes it affordable.** It is a post-pass over a finished
buffer, not a change to how anything is drawn. `ui::render` paints once into a
captured target; every frame after that composes from that clone. None of the
render functions know it exists, and a frame costs one pass over the cell grid
rather than a full re-render -- which matters at 60fps.

**Its own timer.** `TICK_INTERVAL` is 100ms, which would give 28 frames over
the sweep and read as a stutter. It cannot simply be lowered: it *is* the
spinner rate, and `BLINK_TICKS` and `SLOW_REPAINT_TICKS` are expressed in it.
So the wave runs a one-shot 16ms task that stops when it is done: 93 frames
over a second and a half.

**The wave itself.** Each cell has a position along the diagonal, 0 at the
bottom-left corner and 1 at the top-right, with both axes normalised so the
front crosses at the same visual angle on any terminal shape. The front sweeps
from `-BAND` to `1 + BAND`, so at `t = 0` every cell is ahead of it (the full
screen of noise) and at `t = 1` every cell is behind it. A cell's brightness is
its distance from the crest, blended up a nuance ladder from `SURFACE` to a
half-step past `TEXT`, with per-cell grain so the band is textured rather than a
clean gradient.

**And it gains colour as it goes.** The share of noise wearing a Catppuccin hue
rather than white climbs across the sweep, from three fifths at the start to all
of it at the end -- so the field is mostly white when the wave
arrives and mostly colour by the time the last of it is washed away. The rising
threshold is compared against a fixed per-cell value, so a cell that has turned
colour stays colour: the field *gains* hues rather than flickering between
them.

**Characters lock one by one, not band by band.** Each cell draws its own
threshold for where in the band it stops churning and shows its real character.
A single shared threshold would flip the whole band at once and read as an edge
wiping the screen; scattered thresholds have characters settling one at a time
across the width of the wave, so the noise *resolves into* the content rather
than being wiped off it. Monotonic by construction -- the threshold is fixed per
cell and the resolve fraction only grows -- so nothing that has locked ever
falls back to noise.

**Only the wave churns.** Ratatui redraws a cell whose symbol or style changed,
so churning every cell every frame makes each of the 93 frames a full-screen
repaint: fine locally, unkind over ssh. Ahead of the wave the field holds still.
The churn is keyed off the crest rather than the lit intensity, because the
grain lifts resting cells above zero too and would have half the field
flickering.

**No `rand`.** An FNV-1a hash of the cell's coordinates and a salt answers every
question about a cell. Nothing here needs to be unpredictable, only unpatterned,
and determinism is what makes the whole effect testable -- `compose(buf, t)`
takes `t` as an argument, so frames are inspected rather than raced against a
clock.

**The bug that made it not a reveal at all.** The target was captured after
`terminal.draw` returned. `draw` swaps buffers on its way out -- the screen just
painted becomes the previous buffer and `current_buffer_mut` is a freshly reset
blank -- so the wave was handed a blank screen and faithfully resolved into
nothing. What you saw was noise, then an empty pane, then the content snapping
in when the wave ended: an effect played in front of the dashboard rather than
one that revealed it. The capture moved inside the closure, and
`the_screen_just_painted_is_not_the_buffer_draw_leaves_behind` pins the trap so
it cannot move back out.

**Two things that would corrupt the screen, handled.**

- **Wide glyphs.** A narrow character written into a wide glyph's continuation
  cell shifts the rest of its row. The noise is all single-width ASCII, and a
  wide glyph settles together with its continuation, which is found by its empty
  symbol rather than by measuring character widths.
- **A resize mid-wave** invalidates the captured screen. `paint` reports that
  rather than drawing a stale layout, and the caller ends the wave.

**The wait is spent on the same surface.** Before the first load lands there is
nothing to draw, and what was drawn was an empty frame. `Loading` fills the
screen with the same noise at the same resting brightness the wave opens on, so
the two are one continuous surface: the screen fills with noise, the noise sits
and shimmers while the repos are probed, and then the wave crosses it and leaves
the dashboard behind. A test compares the field against the wave's own first
frame cell by cell, because a seam there would be the one place the whole effect
gives itself away.

`LOADING` sits still in the middle of it, in block capitals. Everything around
it moves, and one still thing is what makes the rest read as motion rather than
as static; at banner size it also gives the noise a shape to break around. Its
block is cleared whole, padding included, so the field does not show through the
counters of the letters, and a screen too small for it gets the field without it
rather than a clipped word. The fill is asserted one column wide by ratatui's
own measurement, the same requirement the graph's commit node has.

**The field shimmers rather than boiling.** Each cell re-rolls its character
every twelfth frame, with a per-cell phase offset, so a twelfth of the screen
turns over on any given frame. A field that may sit there for several seconds
must not be a full-screen repaint sixty times a second -- and the shimmer reads
better than a boil.

**No flash on the way in.** The wave's target is captured and the first wave
frame painted over it *within the same draw*. Capturing, presenting, and then
starting the wave on the next frame would show one frame of finished dashboard
before it dissolved -- a flash on the way in to a reveal.

**Skippable, once, and optional.** Any keypress ends it on the spot. It is armed
only on the first `Loaded`, so a watch sweep does not dissolve the dashboard
into noise every refresh. `intro = true` in `config.toml` turns it off for good:
an animation met several times a day should be one key to retire.

**Toggleable from inside the app.** `,` carries an `intro animation` row that
writes `intro` to `config.toml` like every other row, marked restart-only and
honestly so: the wave plays on the first screen of a launch, so by the time the
overlay can be reached it has already been and gone.

The row is almost entirely declarative -- the overlay renders whatever is in
`SETTING_FIELDS` -- so the only things that could go wrong are a `config_key`
that does not match what `Config` reads, and a value that does not survive being
written. Both are covered, the second by an actual save and load through a temp
file, because a row that saves happily and changes nothing is the failure worth
guarding.
