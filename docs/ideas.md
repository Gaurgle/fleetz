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

**Problem.** A clean repo with green CI renders two identical badges: `✓ ✓`.
They are two unrelated facts that happen to share a glyph *and* a color, so
the row reads as a rendering bug rather than as information.

- The first `✓` is **git sync**: `RepoStatus::short_status()`
  (`src/model.rs:188`) builds `3↓ 2↑ 4!` from behind/ahead/dirty, and falls
  back to a bare `✓` only when all three are zero.
- The second `✓` is **CI**: `CiStatus::Passed.symbol()` (`src/model.rs:51`),
  alongside `✗` failed and `○` pending.

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

- One change point. `badge_spans` (`src/ui/badges.rs:91`) feeds both views, so
  compact (`ui/compact.rs:79`) and detail (`ui/detail.rs:79`, `:98`, `:119`)
  update together.
- The expanded CI line in the detail pane spells the state out as "passed"
  (`src/ui/detail.rs:252`, `:547`), so it is already unambiguous and should
  not change.
- Whatever is chosen, keep pass and fail distinguishable without color.
- `short_status()` has direct test coverage (`src/model.rs:297`), so the new
  glyph should be asserted there.

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
