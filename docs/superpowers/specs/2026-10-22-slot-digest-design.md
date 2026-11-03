# Slot digest: what happened across the fleet since the last slot

Drafted 2026-10-22 by the coordinator from Andreas's request at 17:40
("like in repoz command, i'd like some kind of summary for the slot or the
day ... something that summarizes the collective changes over all touched
repos that slot ... activated by hovering/standing on 'github activity' /
'also active' etc in pane 1") and his "yes on all" (17:50) to the three
decisions below. Ticket FZ-45. Not to be worked on yet ("no need to work
on this right now").

## The question it answers

Pane 1 answers *which* repos moved this slot: that is what the "github
activity" section is. Nothing answers *what* moved, so today you walk the
rows one by one. repoz prints the list once and exits; fleetz holds the
digest and lets you drill.

## Decisions (his "yes on all")

1. The pane 1 section headers ("github activity", "local changes", "also
   active") become cursor stops: `j`/`k` land on them, a mouse hover or
   click lands on them.
2. Numbers are local first: no new API cost in the first version. Remote
   commit counts through GraphQL `history(since:)` are a later, measured
   step.
3. Spec and ticket now; the work queues after FZ-41 (the split inspector)
   and before FZ-42 to FZ-44.

## Design

### Where it lives

Standing on a section header makes pane 2 show the **digest for that
section and the current window**; the inspector details the digest line
under the cursor. Leaving the header returns pane 2 to the repo views for
the repo under the cursor. No new pane, no new key for opening it.

### What the digest says

```
 since 09:00 (Work 09-18)   5 repos · 14 commits · 2 PRs · 1 red CI
 fleetz             3 commits  feat(ci): the ci view heads its columns…  ✓ CI  1 PR
 bonsai-education   2 commits by A, B                                      ✗ CI
 repoz              pushed 2h ago
```

- First line: the window as repoz prints its header (`time.rs` slot
  labels), then the totals over the touched repos.
- One line per touched repo, most recent push first (the section's own
  order): name, commit count in the window, the newest subject (truncated
  to the slack), the CI verdict and the open PR count from the fields in
  hand. Authors are named when more than one touched the repo.
- A repo that is not cloned, or not fetched since the window's cutoff,
  shows `pushed 2h ago` and nothing more: fleetz knows it moved, not what
  moved.
- **Inspector** on a line: that repo's commit subjects in the window, with
  author and time; the PR titles; the CI verdict with its run. `Enter` on
  a line jumps to the repo's row in pane 1.
- **Local changes** section: its digest is the worklist, per repo the dirty
  file count, stashes and unpushed commits, which pane 1 only shows as
  counters.
- **Also active**: same shape as github activity.

### Windows

`t` on the digest cycles the window the way repoz's flags do: since the
previous slot boundary (default, `time.rs` auto mode), today, yesterday,
this week. The digest and the section agree on who is "active" because
both use the same cutoff.

### Where the numbers come from

- Commits: for a cloned repo, `git log --since=<cutoff> <default branch>`
  after the fetch the sweep already does (`git.rs`, one run per touched
  repo per sweep, cached per cutoff and head). Nothing network.
- PRs and CI: the poll's fields already on the row.
- Local changes: the sweep's `dirty`, `stash_count`, `ahead`.
- Totals: summed over the touched repos of the section.

### Interaction with other tickets

- **FZ-15 changes since seen** (#28): the digest is where "seen" would
  show (`new` marks on lines since the last visit). Separate ticket.
- **FZ-14 attention view** (#27): "what needs me"; the digest is "what
  happened". Separate.
- **Discovery invariant:** the header rows are cursor stops only; the
  `visible_repos` order and the drawn order stay one list (`CLAUDE.md`,
  "App::visible_repos order must match the order the repo list draws").
  A header stop is a row kind, not a repo.

## Rejected

- A fourth pane or an overlay for the digest: pane 2 is the detail of
  what pane 1 points at, and a header is a thing pane 1 can point at.
- Per-repo push rows in the digest (one line per push): he asked for the
  collective view; the inspector holds the detail.
- Remote commit counts in the first version: `history(since:, first:)` is
  a priced nested connection across every touched repo per poll.

## Open with Andreas

- Whether the header stop also folds the section (as a click on a header
  does in some lists) or only shows the digest. Proposal: digest only;
  folding stays a separate idea.
- The look, at the first branch build.
