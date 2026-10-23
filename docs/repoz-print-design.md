# repoz print mode design

Status: **accepted 2026-10-23 15:00 CEST** ("i agree on all these. as long
as we dont touch the code of fleetz, except renaming it"), with the name
decided: the whole binary becomes `repoz` and print mode is a flag of it
(O1). Written in an advisor session against `main` at `4b47f18`; line
numbers drift, follow the symbols. Tickets filed the same day as #102 to
#108 (FZ-46 to FZ-52), Backlog, milestone `repoz convergence`, none
dispatched. The phased order is in
[2026-10-23-repoz-convergence-plan.md](superpowers/plans/2026-10-23-repoz-convergence-plan.md).
Standing constraint from the acceptance: fleetz's existing code is not
changed beyond the wiring print mode needs (the branch in `main`,
visibility of reused functions); only FZ-52 touches it, for the name.

## Decision so far

Andreas, 2026-10-23: repoz should be "a quick 'script version' of fleetz,
rather than what it is now", using "the technology and base build in
fleetz", and "i dont want the general look of repoz to change, i think it's
really good". He confirmed the shape ("exactly this ... i think this is the
correct way to go") with the command name left for later, and asked for the
work to run under Relay.

So: the one-shot text report that bash repoz prints today is rebuilt as a
print mode of the fleetz binary, on fleetz's own discovery, git, GitHub and
cache code, and prints what bash repoz prints. **Architecture first, names
last.** Naming, repository renames and publishing are owner items (O1, O6,
O7) and no ticket touches them.

## Why

The two tools implement discovery, matching and fetching separately, in two
languages, and only one of them has tests. On 2026-10-23 a comparison found
six fleetz-fixed bugs still live in bash repoz: owner-blind matching, first
clone by filesystem order, a prefix-matching seen-path check that hid dirty
repos, failed fetches shown as `synced`, fetches that could hang, linked
worktrees missing, plus `--no-glyphs` ending `-d` and target mode at the
first row. Four of those and the glyph crash are fixed in bash on
`fix/fleetz-lessons` in `~/repos/repoz` as a stopgap; matching was left for
this work rather than built twice. `discovery::repoz_parity` exists because
discovery already drifted once.

Built on fleetz, the print mode gets for free: matching by origin and owner
(`discovery::Wanted::match_clone`, `CheckoutRank`), fetching only remotes
that moved (`stale_remotes`, the `FETCH_HEAD` baseline), fetch outcomes and
the stall limit (FZ-04, FZ-35a), the sized org query (3 points, not 26), and
a cache write that makes the next TUI launch current. Bash repoz takes about
10 s on Andreas's machine, nearly all of it `git fetch` of every matched
clone.

## Scope

In: every mode and option bash repoz has today, printed the way it prints
them, from one fleetz binary. Out: names, repository renames, publishing,
release binaries, any change to the TUI, any new report feature.

### What bash repoz prints

Read from `~/repos/repoz/repoz` on `fix/fleetz-lessons`.

| Part | Shape |
|---|---|
| Header | `repoz` in lavender bold, then the mode in overlay (`since <date>  <slot label>`, `since <date>`, `today`, `all divergence`, `local changes`, `owner/repo`, with `→ before <date>` and `(author: <name>)` appended), then a full-width divider |
| Sections | pass 1 rows first with no section header (GitHub repos cloned under the scan roots), then `also active` (pass 2, found elsewhere under `$HOME`, or `not cloned`), then `local changes` (pass 3), each of the last two under a `section_div` header |
| Row | four spaces, optional repo-type glyph, name in sapphire, path in overlay, dotted leader in surface, then the status flush right |
| Status | counters as symbols (`2↓ 1↑ 3! 1⚑ 2PR`, each its own colour), categorical states as words (`synced`, `not cloned`, an operation, `fetch failed`), an operation or a failed fetch in front with a comma |
| Commits | under a row, `└─ <hash> <subject> ···· +ins -del  author` for each branch's behind and ahead ranges, or the last commit when nothing diverges |
| Releases (`-r`) | `└─ N since vX` or `released vX` under the row |
| Summary | divider, then the counts in words (`1 in progress · 2 behind · ... · 14 not local`), then the glyph legend, then `repoz <version>` flush right |

Colours are fixed Catppuccin Mocha RGB escapes. Width comes from `stty
size </dev/tty`, then `COLUMNS`, then `tput cols`, never under 40, else 80.

## Decisions (proposed)

**D1. One crate, one binary, entered by a flag.** Print mode is a new
`report` module under `src/`, entered by `fleetz --print` until O1 names the
command. `main` branches before any terminal setup: no raw mode, no
alternate screen, no input thread. The TUI path is untouched. Decided 2026-10-23: there is no second
binary; the whole binary takes the name `repoz` in FZ-52 and print mode
stays a flag of it. Rejected: a separate crate or a library split now (two
build units to keep in step, which is the drift this removes); a separate
reimplementation in Rust (same problem, new language).

**D2. Byte parity is the contract, held by a golden test.** For the same
filesystem, remotes and GitHub payload, print mode writes the same bytes as
bash repoz, escapes included, except the version line and the differences
D3 lists. The golden test (FZ-46) is the arbiter; a difference is a bug in
one of the two unless it is listed in D3.

**D3. The deliberate differences, each a fleetz fix.**

| Difference | Bash repoz | Print mode |
|---|---|---|
| Matching | folder name only | origin and owner first, folder name only where the remote cannot decide (house invariant) |
| Several clones of one repo | first in sorted order | `CheckoutRank` |
| Which clones fetch | every matched clone | only remotes GitHub says moved since the clone's last fetch (`stale_remotes`); unknowns fetch |
| Archived repos | listed | per O3 |

Fixtures for the golden test are built so these differences do not arise
(one clone per repo, origins that agree with folder names, no archived
repos, every clone fetched); each difference gets its own Rust test instead.

**D4. Fetching.** `stale_remotes` decides, with the empty-`FETCH_HEAD` rule
(fetch outcome contract rule 7). Concurrency 8, as bash's `FETCH_CAP`. Each
fetch has the FZ-35a stall limit and records its outcome; a failed one
prints `fetch failed` in front of its counts and counts in the summary, as
bash repoz now does. No retry.

**D5. GitHub.** One poll with fleetz's own query, accepted only through
`accept_response`, so a GraphQL error never reaches the cache. A successful
poll writes the keyed cache (`cache::KeyedCache`), so a TUI launch after a
print run paints current data. `-d` sends nothing, as today. On a failed
poll, print mode behaves as bash does (`Failed to reach GitHub. Run: gh auth
login`, exit 1) unless O4 accepts a cached fallback.

**D6. Config.** Print mode reads fleetz's `config.toml` and
`REPO_CHECK_DIR`, never `~/.config/repoz/config`. The keys map one to one:
`WORK_START`, `WORK_END`, `REPO_LIMIT`, `REPO_CHECK_DIR`, `EXTRA_DIRS` and
`SHOW_GLYPHS` to `work_start`, `work_end`, `repo_limit`, `repo_dir`,
`extra_dirs` and `icons`. The `GLYPH_*` overrides and `REPOZ_SRC` have no
counterpart (O5, O6).

**D7. Flags.** `--print` accepts bash repoz's whole set. Shared with the
TUI and parsed as today: `--since`, `--before`, `-t`, `-a`, `-d`,
`--author`, `-p`. Print-only, rejected by clap without `--print`: `-s`/`--stat`,
`-r`/`--releases`, `--no-glyphs`, the `/owner/repo` positional. `-v` prints
the version. `--update` and `--pull` are not carried (O6). `-p` in print
mode means what it means in repoz (the `2PR` counts), not the TUI's
detailed PR section.

**D8. Output.** The report writes escape sequences itself, through one
palette table holding repoz's RGB values; no ratatui. Width follows bash's
rule exactly (D2 covers it). Output is built line by line and written once
at the end of each section, so a pipe sees whole lines.

**D9. Time.** Default mode uses `time.rs`'s slot logic, already shared with
the TUI. The golden test uses `-a` and absolute `--since` dates so it never
depends on the clock; the slot boundary keeps its existing tests.

## Tickets (bodies ready for the lead to file)

Order: FZ-46, FZ-47, then FZ-48, FZ-49 and FZ-50 one at a time (all three
touch `src/report/`), then FZ-51. Ids come from `scripts/relay-board next`
when filed; the numbers here assume nothing else is filed first.

**Prerequisite P1, outside fleetz's Relay:** bash repoz gains a fixture
hook. When `REPOZ_GRAPHQL_FIXTURE` names a file, its contents replace the
`gh api graphql` output, and `REPOZ_GH_LOGIN` replaces `gh api user` in
target mode. A few lines in `~/repos/repoz/repoz`, committed by Andreas;
FZ-46 cannot run against bash repoz until it lands.

### FZ-46: Golden harness for print parity

Outcome: a `report_parity` test module that builds a fixture (temp `HOME`,
scan roots, bare remotes on disk so fetches work offline, a recorded GraphQL
payload covering a user repo, an org repo, a not-cloned repo and a repo
found outside the roots), runs bash repoz against it with the P1 hook,
`COLUMNS=100` and no tty, and compares its stdout with print mode's after
dropping the version line. Skips with a notice when `repoz` is not on
`PATH`, as `repoz_parity` does. Ships with print mode stubbed: the test
exists, records the bash output, and is `#[ignore]`d with a reason until
FZ-47 makes it pass.

Acceptance: the fixture builder is shared by every later report test; the
recorded payload has the fields both queries read; normalisation removes
the version line and nothing else; `cargo fmt --check`, `cargo clippy
--all-targets -- -D warnings`, `cargo test`.

### FZ-47: Print mode: default, `-a`, `-t`, `--since`, `--before`

Outcome: `fleetz --print` with the time modes prints header, the three
sections, rows, commit lines and summary as D2 requires, using
`assemble_view_in` for discovery and D4 and D5 for fetch and poll. Un-ignores
FZ-46's test for these modes.

Acceptance: golden parity for `-a` and `--since <date>`, with and without
`--before`; a test per D3 difference showing print mode's behaviour; no
terminal state touched (a test that print mode never calls
`enable_raw_mode`, or the branch in `main` is before it); the TUI's tests
unchanged.

### FZ-48: Print mode: `-d` and `/owner/repo`

Outcome: local-only mode (no GitHub call, `local changes` header, `All
clean.` or `N dirty`) and the single-repo target (one row or `not cloned`).

Acceptance: golden parity for both, including a linked worktree and a
dirty repo whose path is a prefix of another's; `-d` sends no request
(tested at the seam); a target with one segment resolves against the
login.

### FZ-49: Print mode: `-s`, `-r`, `-p`, `--author`, `--no-glyphs`

Outcome: the options, combinable as in bash repoz.

Acceptance: golden parity for each option alone and for `-s -r -p`
together; print-only flags rejected without `--print` (clap test).

### FZ-50: Print mode: fetch outcomes and a stalled remote

Outcome: D4's failure path: a clone whose fetch fails prints `fetch
failed, ...` and is never `synced`; the summary counts it.

Acceptance: golden parity with a fixture remote that does not exist; a
stall test using the FZ-35a seam; no credential in any printed line
(contract rule 8).

### FZ-51: Measure print mode against bash repoz

Outcome: wall time of bash repoz and `fleetz --print` on Andreas's real
fleet, interleaved in one session as FZ-01 prescribes, warm and with
fetches due, recorded in `docs/performance-baseline.md`. Measurement only.

## Owner items

- **O1. The command name and the rename order.** Decided 2026-10-23: the
  Rust TUI itself is named `repoz` ("it's just a better name"); print mode
  is `repoz --print` and needs no name of its own. Rename order (plan
  phase 6): `Gaurgle/repoz` to `repoz-sh` first, then `Gaurgle/fleetz` to
  `repoz`, after the public install story exists. Background: GitHub keeps a redirect from a renamed repository's old name only until a
  new repository takes that name, so renaming `Gaurgle/repoz` away and then
  `Gaurgle/fleetz` to `repoz` breaks every `git clone .../repoz` that meant
  the bash script. `rz` collides with lrzsz's ZMODEM receive command.
- **O2. Print automatically when stdout is not a terminal?** `fleetz | less`
  draws a TUI into a pipe today. Proposed default: no, `--print` only,
  until the name is chosen.
- **O3. Archived repositories in print mode.** fleetz filters them on the
  server (#69); bash repoz lists them. Proposed: follow fleetz.
- **O4. Cached fallback when GitHub is unreachable.** Proposed default: no
  (D5 keeps bash's message and exit code). Yes would print the cached
  dataset with an `as of` note in the header, a new line in repoz's look.
- **O5. The `~/.config/repoz/config` file.** Migrate its keys into
  `config.toml` by hand at switchover, or have print mode read it as a
  fallback.
- **O6. Bash repoz after the switch, and `--update`.** Decided 2026-10-23:
  it keeps its own repository as `Gaurgle/repoz-sh`, README pointing at
  the Rust `repoz`; archived or not is his later call. `--update` has no
  meaning for a cargo-installed binary.
- **O7. Publishing.** bash repoz is public and installs with a copy; fleetz
  is private and installs with `cargo install`. Replacing repoz in public
  needs fleetz public and release binaries (#3), which is release work.
- **O8. Board placement.** Decided 2026-10-23: milestone `repoz
  convergence` (GitHub milestone 7) for FZ-46 to FZ-52; the public switch
  (O7) stays M5 work. O2 to O5 take the proposed defaults.

## Rejected

- **Porting each fleetz fix into bash.** Every fix twice, the bash copy
  untested, and matching by owner is the largest fix and the hardest in
  bash. The stopgap fixes on `fix/fleetz-lessons` are the exception: small,
  local and live in daily use.
- **Reusing the TUI's widgets for the report.** The look is repoz's, not
  the dashboard's, and ratatui renders to a buffer for a terminal, not to a
  pipe.
- **Renaming first.** A rename with no code behind it moves the public name
  onto a private project and leaves nothing to install.
