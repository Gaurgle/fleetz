# Maximize and restore a pane (FZ-13m)

**Status: dropped.** Andreas on 2026-10-17, about 17:30: "i just dont know
if this feature is something ive requested?" and then "we dont need a focus
mode". It was never his request: it entered the plan on 2026-10-01 from the
ideas backlog ("pane maximize" in M3's scope line) and the FZ-13 split on
#26 put it in wave 4. Kept as a record of what was considered; no ticket.

Design note, 2026-10-17, by the coordinator (`claude-fable-5-1`) for Andreas's
acceptance before a ticket is written. Parent: FZ-13 (#26), wave 4. His
answer of 2026-10-17 to "what restore returns to": "the last saved config
would be restored, if possible." The key and the panes were left to the
coordinator to propose.

## Problem

Four panes share the screen in the detail view (repo list, repo pane,
inspector, contribution strip), two in the compact view (list, strip). The
only way to give one pane the room is to fold the others (`3`, `4`) and drag
or step the dividers (`<`, `>`, `+`, `-`), then undo all of it by hand (`=`
resets the sizes but not the folds). Reading a long commit in the inspector,
or a wide `graph` view, means a dance.

## Proposal

**One key, `z`, toggles the focused pane between its normal place and the
whole main area.** `z` is tmux's zoom, which Andreas uses all day in the same
terminal; it is unbound in fleetz; lazygit's screen modes (`+`, `_`) are
taken here by the inspector resize. Pressing `z` again restores. So does
focusing another pane (`1` to `4`, `Tab`, `Shift-Tab`, a click outside the
maximized pane), because a focus change is a request to see that pane.

**Which panes.** The repo list, the repo pane and the inspector. Not the
strip: its height is fixed by design (`CONTRIB_HEIGHT`, the seven day rows
and the labels), so "the whole area" would be mostly blank; its own width
already scrolls three years. `z` on the focused strip does nothing. In the
compact view the list is the only pane, and `z` gives it the strip's rows
(the strip is hidden, not folded; it comes back on restore).

**What maximize draws.** The maximized pane fills the main area between the
header and the footer, with its own border and title, in the focus colour.
The other panes and the strip are not drawn. The header and footer stay,
and the footer's pane segment (FZ-13n, #81) shows `[z] restore`, lit green
while maximized, under the "a key with an on state lights up" rule. The
pane's title carries nothing extra: the layout says it.

**What restore returns to.** Exactly the layout from before `z`: the two
dividers (`repo_list_pct`, `inspector_pct`) and the two fold flags are not
touched by maximize, so restoring is forgetting that a pane is maximized.
With `persist_pane_sizes` on that layout is the saved one, which is his
"last saved config"; with it off it is the launch default plus whatever he
nudged this session, which is "if possible". Maximize itself is never
persisted: a launch is always normal.

**While maximized.**

- `j`/`k`, `PgUp`/`PgDn`, scrolling and the pane's own keys (`[`/`]`, `B`,
  `J`/`K`) work as they do now. `J`/`K` drive the next pane's state; the
  effect is visible on restore.
- `<`, `>`, `+`, `-`, `=` and the fold keys (`3`, `4` on the focused pane)
  do nothing: there is no divider to move and no second pane to fold. `3`
  or `4` from the maximized list or repo pane focuses that pane, which
  restores first.
- The mouse: clicks land on the maximized pane only (`panel_rects` reports
  only it); there are no drag dividers; the wheel scrolls it.
- A view switch (`v`) restores first: the other view has its own layout.
- `r`, `R`, `w`, `f`, `F`, search, overlays, the settings popup: unchanged.
- A `graph` view that docks no inspector: `z` on the repo pane still
  works; the inspector cannot be focused there today, so no case arises.

**Fold and maximize.** `z` on a folded inspector unfolds it for the
duration; restore refolds it (the fold flag is kept, the drawing ignores it
while maximized).

## Constraints the ticket must meet

- `ui::panel_rects` decides the maximized layout and `render` draws from it:
  the first domain invariant. The maximized pane's rect is the main area;
  every other field is `None` (the struct's `repo_list` is a `Rect` today
  and becomes `Option<Rect>`, or the maximized pane is reported in a new
  field; the ticket chooses, with the hit-test tests updated). No second
  copy of the geometry.
- Compact and detail both go through the same `maximized:
  Option<FocusedPanel>` on `App`; `toggle_maximize` is a pure state change
  with tests for every rule above, like `toggle_fold`.
- `App::visible_repos` and the cursor are untouched; maximize is drawing
  only.
- Existing tests: the layout tests run with `maximized = None` and should
  pass as written. `the_fold_tag_is_where_the_title_draws_it`,
  `panel_rects_reports_the_strip_as_it_is_drawn` and the drag tests are the
  ones to watch; any assertion that must change is Andreas's decision.
- Help overlay row, README key row, and the footer segment hint.

## Rejected

- **A full-screen overlay** for the maximized pane (as `docs/ideas.md`
  rejected for the help): it would duplicate the pane's drawing code and
  escape `panel_rects`.
- **Maximizing the strip.** Fixed height; nothing to gain.
- **Remembering the maximized state across launches.** A dashboard should
  open as a dashboard; the saved layout is the dividers and, if FZ-13o's
  follow-up lands, the folds.
- **`m` or `Space`.** `m` has no prior meaning for him; `Space` marks in the
  branches overlay.

## Size and order

One ticket, M. After #81 (FZ-13n) merges or is dropped, since the footer
segment is where the hint lives; otherwise the hint goes to the global row
and the ticket says so. Files: `src/app.rs`, `src/ui/mod.rs`, `src/main.rs`
(the key), `src/ui/help.rs`, `src/ui/footer.rs`, `README.md`.

## Questions for Andreas

1. `z` as the key, and focus-change-restores: yes or no?
2. The strip excluded: yes or no?
3. Resize and fold keys inert while maximized (rather than applied
   invisibly): yes or no?
