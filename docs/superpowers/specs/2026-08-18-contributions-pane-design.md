# Contributions pane (GitHub-style activity heatmap)

Date: 2026-08-18
Status: approved for implementation

## Problem

fleetz answers "what is the state of my repos right now". It cannot answer
"what has my year looked like". GitHub's own contribution graph does, but
reading it means leaving the terminal for a browser, and it only ever shows
the GitHub account view: it cannot be scoped to the repos currently on screen.

We want the same picture inside fleetz, from two sources, without paying a
second GitHub request and without git subprocesses on the keystroke path.

## Decisions

Settled in discussion before design:

- **Two sources, toggled in the pane.** GitHub's own calendar is the default;
  a locally computed one from `git log` is the alternative.
- **A horizontal strip docked at the bottom of the main area**, spanning the
  full width under the repo list and the right column. Not an overlay, not a
  third `View`.
- **The local source follows the visible repo set**, so quick filters, groups,
  search and the time window all reshape it. The GitHub source stays
  viewer-wide, because a per-day calendar scoped to one repo is not something
  the API offers cheaply.
- **Adaptive grid**: wide cells when there is room, dense cells when there is
  not, and trimmed to the most recent N weeks (with an honest title) when even
  dense does not fit.
- **Focusable, with a day cursor.** A fourth focus target, reachable with `4`
  and in the Tab cycle, but only while the pane is open.
- **Present in both views, and the state persists** to `config.toml`. Default
  off, so no one gets a surprise pane on upgrade.

Decided during design, not asked:

- The local source reads `--all`, not just the default branch. Work on a
  feature branch is still work.
- The strip is a fixed height and is not resizable. Its content height is
  fixed, so a drag handle would only ever add empty space.

## Verified facts

Probed against the live API before designing, not assumed:

```
gh api graphql -f query='{ rateLimit { cost nodeCount } viewer {
  contributionsCollection { contributionCalendar {
    totalContributions weeks { contributionDays { date contributionCount } } } } } }'
```

- Reports `cost: 1, nodeCount: 0`. The field has no `first:` connection, so
  unioning it into the existing `viewer` query leaves the poll at its current
  price. This is a field, not a request, and therefore permitted by the
  "never widen the GitHub fetch without checking the cache" invariant.
- Returns exactly 53 weeks, `weeks[0].contributionDays[0].weekday == 0`
  (Sunday-first). For today, 2026-08-18, it opens on 2025-08-17.
- That start reproduces exactly as "the Sunday on or before `today - 364d`",
  verified against the probe: `2026-08-18 - 364d` is Tue 2025-08-19, whose
  preceding Sunday is 2025-08-17. 53 columns, 371 grid slots, 367 real days.
  The local source uses the same rule, so both sources share a grid.
- Only the *final* week is partial. The grid opens on a Sunday, so there is no
  leading gap; the trailing days after today are absent rather than zero.
- GitHub returns 5 distinct `color` values. We ignore them and use our own
  ladder, since the payload colors are tuned for GitHub's background.

## Architecture

### `src/contributions.rs` (new)

One pure module, no I/O, depended on by both producers and by the renderer,
depending on neither.

```rust
pub enum Source { Github, Local }

pub struct Calendar {
    start: NaiveDate,   // the Sunday on or before today - 364d
    counts: Vec<u32>,   // 371 slots, one per grid cell; slots after today are 0
    today: NaiveDate,   // where the real data stops
    source: Source,
}

impl Calendar {
    pub fn total(&self) -> u32;
    pub fn weeks(&self) -> impl Iterator<Item = [Option<u32>; 7]>;
    pub fn intensity(&self, count: u32) -> u8;   // 0..=4
    pub fn day_at(&self, week: usize, weekday: usize) -> Option<(NaiveDate, u32)>;
    pub fn month_labels(&self, cell_width: u16) -> Vec<(u16, &'static str)>;
}
```

`weeks` yields Sunday-first columns, with `None` for slots after `today`, so
the partial trailing week is handled in exactly one place and the two
producers cannot drift. There is no leading gap: the grid opens on a Sunday
by construction.

`intensity` buckets by quartiles of the calendar's own maximum, matching
GitHub's relative scaling. A quiet fleet still shows shape; a busy one is not
saturated flat.

### GitHub source (`src/github.rs`)

- Add to the existing `viewer` block in `build_query`:

  ```graphql
  contributionsCollection { contributionCalendar {
    totalContributions
    weeks { contributionDays { date contributionCount } }
  } }
  ```

- `Viewer` gains `#[serde(default)] contributions: Option<ContributionsCollection>`,
  the same treatment `organizations` already carries, so a payload written to
  disk by today's build still deserializes after the upgrade.
- No cache-key change. The key encodes `repo_limit` and `prs`; this touches
  neither. The calendar rides the existing TTL and refreshes on every poll at
  no extra cost.
- `fetch_repos` returns `GithubFetch { repos: Vec<GithubRepo>, calendar: Option<Calendar> }`
  instead of a bare `Vec`. Two call sites plus the `main.rs` wrapper.

### Local source (`src/git.rs` + `src/model.rs`)

The chosen scope means the aggregate changes on every keystroke in `/`.
Nothing may shell out on that path.

- During a sweep, each repo runs once:

  ```
  git log --all --since=<calendar start> --author=<identity> \
          --date=short --pretty=%ad
  ```

  parsed into a fixed `[u16; 371]` indexed by the same grid slot the
  `Calendar` uses, stored on `RepoStatus` as `daily: Option<DailyCounts>`.
  742 bytes per repo.
- Gated twice: only when the pane is open in local mode, and only when the
  existing `WorkKey` fingerprint says the repo moved since the last sweep. An
  idle fleet costs zero subprocesses.
- Aggregating the visible set is then slot-wise summation over
  `App::visible_repos` into one 371-slot buffer: no allocation per keystroke
  beyond that buffer, no subprocess, no await.
- Identity resolves per repo from `git config user.email`, so a
  work-configured clone counts correctly against a different address. The
  `contributions_author` config key overrides it with a git author regex
  (`--author` is a regex, so `a@b\|c@d` covers several identities).

The two sources will not agree, by construction. GitHub counts PRs, issues,
reviews and private repos outside the fleet; local counts commits in cloned
repos. The pane title names the active source so the gap is never mysterious.

### Layout (`src/ui/detail.rs`, `src/ui/mod.rs`)

- New shared helper `detail::split_with_contributions(main: Rect, open: bool)
  -> (Rect, Option<Rect>)`, called by both `render` and `panel_rects`, exactly
  as `split_main` and `split_right_column` already are. `PanelRects` gains
  `contributions: Option<Rect>`. No second copy of the arithmetic.
- Fixed height 10 rows: title, month labels, 7 day rows, legend. Plus the
  one-row gap the existing splits already use.
- If the main area cannot spare the strip while leaving the panes above their
  minimums, it is suppressed and `c` reports why in the status line rather
  than squeezing the dashboard into nothing.
- Applies in both `Detail` and `Compact`, since both have a main area to dock
  under.

### Rendering (`src/ui/contributions.rs`, new)

Three width tiers, chosen from the strip's own width:

| Tier | Condition | Per week | Full year needs |
|---|---|---|---|
| Wide | width >= 109 | glyph + gap (2 cols) | 4 + (53 * 2 - 1) |
| Dense | width >= 57 | glyph (1 col) | 4 + 53 |
| Trimmed | width < 57 | glyph (1 col) | the newest `width - 4` weeks |

The wide tier drops the gap after the final column, so it needs 109 rather
than 110.

The 4-column gutter holds `Mon`, `Wed`, `Fri` on their rows and blanks on the
others, as GitHub does. In the trimmed tier the title changes from
`in the last year` to `in the last N weeks`, so a truncated picture is never
presented as a full one.

One new ladder in `colors.rs`:

```rust
/// Contribution intensity, empty through busiest. Step 0 is `SURFACE` (the
/// same near-invisible tone the dividers use, so an empty day reads as
/// structure rather than data); step 4 is `GREEN`, the palette's existing
/// "ahead / synced" green, so the busiest day matches the brightest green
/// already on screen. The three between are interpolated in RGB.
pub const CONTRIB_LADDER: [Color; 5] = [
    Color::Rgb(69, 71, 90),    // SURFACE: no contributions
    Color::Rgb(93, 110, 108),
    Color::Rgb(117, 149, 125),
    Color::Rgb(141, 188, 143),
    Color::Rgb(166, 227, 161), // GREEN: busiest quartile
];
```

Cells and the Less/More legend both read it, so they cannot disagree. This is
a new concept (contribution intensity) and therefore earns its own ladder,
rather than borrowing `badges::status_color`, which means something else.

### Interaction (`src/app.rs`, `src/main.rs`)

| Key | Action |
|---|---|
| `c` | Toggle the pane open or closed |
| `C` | Cycle the source: github, local |
| `4` | Focus the pane (only while open) |
| Tab | Includes the pane in the cycle only while open |
| `h`/`l`, left/right | Move the day cursor by one day |
| `j`/`k`, up/down | Move the day cursor by one week |
| Home/End | Jump to the first or last day of the calendar |

`c` and `C` follow the existing lowercase/uppercase variant convention
(`r`/`R`, `f`/`F`, `g`/`G`).

`FocusedPanel` gains a `Contributions` variant. Unfocused, the title reads
`2,516 contributions in the last year (github)`. Focused, it reads
`7 contributions on Wed 12 Mar`. Clicking a cell focuses the pane and selects
that day, hit-tested through the new `PanelRects` entry.

The day cursor clamps at both ends of the calendar and never lands on a `None`
slot (the partial leading and trailing weeks).

### Config and settings

- One key: `contributions = "off" | "github" | "local"`, default `"off"`. The
  source persists with the open state, so a dashboard set up once returns that
  way.
- One optional key: `contributions_author`, a git author regex overriding the
  per-repo `user.email`.
- A settings-overlay row cycling the same three values, `Applies::Live`.
- README keybinding and config tables updated. CLAUDE.md gains a line noting
  the third shared split helper falls under the existing `panel_rects` parity
  invariant.

## Testing

Beside the code in `#[cfg(test)] mod tests`, per repo convention.

`contributions.rs`
- Calendar construction for a known `today`: 53 columns, `start` is a Sunday,
  `start == 2025-08-17` for `today == 2026-08-18` (the probed value), no
  `None` in the first column, `None` for every slot after `today`.
- `intensity` at the edges: an all-zero year, a year with one contribution,
  the maximum day, and the exact quartile boundaries.
- `month_labels` places each label on the column its month starts in, at both
  cell widths.

`github.rs`
- A fixture payload including the calendar parses into a `Calendar` whose
  total matches `totalContributions`.
- A fixture payload without the field (a cache written by the previous build)
  parses to `None` rather than failing the whole response.

`git.rs`
- `%ad` output parses into the histogram: repeated days accumulate, days with
  no commits stay zero, commits outside the window are ignored.

`ui`
- `panel_rects` returns a contributions rect identical to the one `render`
  draws, and `None` when the pane is closed or the area is too short. Extends
  the existing parity coverage.
- Tier selection at the exact boundary widths (56, 57, 108, 109), and the
  trimmed title naming the same week count the grid actually draws.

`app.rs`
- `c` toggles, `C` cycles the source, `4` and Tab reach the pane only while
  open.
- The day cursor clamps at both ends and skips absent slots.

## Rejected alternatives

- **A separate GraphQL request for the calendar.** Rejected on the measured
  cost above: the field is free inside the existing query, a request is not.
- **An overlay instead of a pane.** Cheapest to build, but it hides the
  dashboard while open, which makes it a thing you check rather than a thing
  you watch.
- **A third `View` alongside Detail and Compact.** Most room to grow, but it
  leaves the dashboard entirely, which is a different product than a
  glanceable strip.
- **Half-block packing** (two days per text row, 4 rows tall). Slicker and
  nearly square, but it breaks the one-label-per-row day gutter and depends on
  the terminal background behaving.
- **A per-repo heatmap following the cursor.** Cheap for the local source,
  impossible for the GitHub one, and it loses the whole-year picture that
  motivated the feature.
- **Recomputing the local calendar on filter changes.** Would put `git log`
  on the keystroke path. The per-repo histogram exists precisely to avoid it.

## Open questions

None. Both design-time decisions (`--all` and the fixed height) were flagged
and accepted.
