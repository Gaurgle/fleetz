# fleetz ideas / backlog

Design notes for features not yet built. Nothing here is committed to; it is
thinking captured so it is not lost.

---

## Interactive time scope

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
