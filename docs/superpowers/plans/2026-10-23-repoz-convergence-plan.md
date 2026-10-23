# repoz convergence plan

Status: **agreed 2026-10-23 15:00 CEST** ("i agree on all these. as long
as we dont touch the code of fleetz, except renaming it"). Written by the
incoming lead (`claude-fable-5-1`) against `main` at `30bc026`. Tickets
filed as #102 to #108, milestone `repoz convergence`, none dispatched. Design: [repoz-print-design.md](../../repoz-print-design.md).
The plan says what happens in which order and who does each step; the
design says how the print mode is built.

## The goal in one paragraph

One Rust binary, named `repoz`, that is today's fleetz TUI plus a `--print`
mode that writes exactly what bash repoz writes today. Bash repoz is
retired. The TUI's name changes because "repoz" is the better name; the
print mode gets no name of its own. Everything is built, tested and proven
under the current name `fleetz`; the rename is the last code step, and the
public switch (repository rename, public visibility, brew and crates.io,
release binaries) is the step after that. Nothing public changes until the
end.

## Phases

| Phase | What | Who | Gate to the next phase |
|---|---|---|---|
| 0. Setup | Accept the design, file the tickets, land the bash fixture hook (P1) | Andreas accepts and commits P1; the lead files | Tickets on the board, P1 on `fix/fleetz-lessons` |
| 1. Harness | FZ-46: fixture tree, recorded GraphQL payload, golden test runner (`#[ignore]`d) | one `fz-worker` plus reviewer | Harness merged; bash repoz output recorded |
| 2. Print mode | FZ-47 to FZ-50, serial: time modes, `-d` and target, the options, fetch failures | one `fz-worker` at a time (all touch `src/report/`) | Golden parity green for every mode and option |
| 3. Proof | FZ-51: timing against bash repoz on the real fleet, recorded in the baseline doc; Andreas uses `fleetz --print` daily for a while | `fz-worker` measures; Andreas judges | His "it replaces repoz for me" |
| 4. Release 0.8.0 | Version bump, notes, his tag; `cargo install` gives him the print mode under the fleetz name | lead prepares, Andreas tags | Tagged |
| 5. Rename | FZ-52: crate, binary, config and cache dirs, docs, scripts, workflows become `repoz`; old config read as fallback | one `fz-worker` plus reviewer; CI edits need his yes | Fresh install as `repoz`, settings carried over |
| 6. Public switch | Rename `Gaurgle/repoz` (bash) to `repoz-sh`; rename `Gaurgle/fleetz` to `repoz`; make it public; FZ-20 (crates.io, brew tap) and #3 release binaries; first public tag | Andreas, with the lead preparing text and commands | `brew install` or `cargo install repoz` works on a clean machine |

Phases 1 to 3 run under the existing Relay slots beside the queued work
(#93, #96, 0.7.0, FZ-41 to FZ-45); FZ-46 touches only new files and can
start as soon as P1 lands. Phase 5 starts only on his explicit word
("do NOT start on any name changing right now", 2026-10-23, stands until
then).

## Tickets

Filed from the design's bodies with ids from `scripts/relay-board next`
(next free id is FZ-46), Backlog, priority as noted, milestone per O8.

| Id | Title | Size | Notes |
|---|---|---|---|
| FZ-46 (#102) | Golden harness for print parity | medium | new files only; needs P1 |
| FZ-47 (#103) | Print mode: default, `-a`, `-t`, `--since`, `--before` | large | expect a split (rows and sections, then commit lines) once the worker reads the bash source |
| FZ-48 (#104) | Print mode: `-d` and `/owner/repo` | medium | |
| FZ-49 (#105) | Print mode: `-s`, `-r`, `-p`, `--author`, `--no-glyphs` | medium | |
| FZ-50 (#106) | Print mode: fetch outcomes and a stalled remote | small | |
| FZ-51 (#107) | Measure print mode against bash repoz | measurement | FZ-01 method |
| FZ-52 (#108) | Rename the crate and binary to `repoz` | medium | body below; not before phase 4 and his word |

### FZ-52: Rename the crate and binary to `repoz`

Outcome: `cargo install --path .` installs `repoz`; the TUI's header,
`--help`, `info` overlay and version line say `repoz`; config is read from
`~/.config/repoz/config.toml` (then the platform dir, then `epoz`, as the
lookup order in `config.rs` does today), with `~/.config/fleetz/` still read
as a fallback for one release so an existing install keeps its settings;
the keyed cache moves to the new directory the same way; bash repoz's
`~/.config/repoz/config` is read by print mode as the design's O5 fallback;
README, CLAUDE.md, the docs index, `scripts/relay-*`, `release.yml`,
`ci.yml` and `docs.yml` spell the new name. The GitHub repository is not
renamed by this ticket.

Acceptance: a test for each fallback (old fleetz config dir, bash repoz
config file); `cargo test` names no `fleetz` binary; `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test`; CI edits listed
in the report for his yes before the commit.

## Owner items and the answers this plan assumes

| Item | Assumed answer |
|---|---|
| O1 name | `repoz` for the whole binary; the print mode is `repoz --print` (flag name still open; `-p` is taken by PR counts). Decided 2026-10-23. |
| O2 auto-print on a pipe | no, flag only |
| O3 archived repos in print mode | follow fleetz (filtered) |
| O4 cached fallback when GitHub is down | no, bash's message and exit 1 |
| O5 bash repoz config | read as a fallback in print mode (FZ-52) |
| O6 bash repoz afterwards | kept as its own repository renamed `Gaurgle/repoz-sh` (his name, 2026-10-23), archived or not as he likes, README pointing at the Rust `repoz`; `--update` and `--pull` dropped from print mode |
| O7 publishing | phase 6, with FZ-20 and #3 |
| O8 board placement | a `repoz convergence` milestone for FZ-46 to FZ-52; phase 6 items stay in M5 |

## Risks

- **Byte parity against a bash script with escapes.** Width, colour and
  the dotted leader all have to match to the byte. Mitigation: the harness
  records bash's bytes first (FZ-46), so every later diff is concrete.
- **The redirect trap.** Renaming `Gaurgle/fleetz` to `repoz` while the old
  `repoz` still holds the name is refused by GitHub; renaming the old one
  away first works, but the old clone URL then serves the new project.
  Mitigation: phase 6 order, and the public install story ready before it.
- **Name availability.** `repoz` was free on crates.io on 2026-10-23
  (API lookup); a brew tap is his own namespace. Re-check before phase 6.
- **Scope creep into the look.** The design forbids report features and
  TUI changes; a wish found on the way goes to `docs/ideas.md`.

## Versioning

Print mode is a feature: the release after 0.7.0 that carries it is
0.8.0. The rename is a contract change (binary and config paths): its
release is at least a minor; whether it is 1.0.0 is his call and belongs
with phase 6.
