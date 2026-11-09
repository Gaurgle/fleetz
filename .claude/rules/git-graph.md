---
paths:
  - "src/git.rs"
  - "src/git/**"
  - "src/graph.rs"
  - "src/ui/detail.rs"
  - "src/ui/detail/**"
---

# Git log and history graph invariants

Binding wherever the session runs. Loaded when a matching file is read or
edited; a loader without path scoping reads this file before changing the
modules it names. Moved from the root `CLAUDE.md` on 2026-10-30, text
unchanged.

**The compact history graph is computed, not parsed.** `git::graph` asks for
`%p` and `crate::graph` derives the topology, as lazygit does. Re-adding
`--graph` brings back filler rows carrying no commit -- which forced row and
commit indices apart everywhere -- and single-column diagonals, undrawable in
box characters. `%p` not `%P`, or a parent hash is spelled differently from the
child's own `%h` and every pipe dangles. The expanded view still reproduces
git's own drawing.

**The commit list and the graph must walk the same refs.** `git::commits` and
`git::graph` are two `git log` runs and the detail pane joins them by hash: the
churn beside a subject, and everything the inspector draws, is a graph row
looked up in the commit list. Reading HEAD in one and the union of the local
branches in the other left a side-branch commit drawn but unfindable, and the
views that indexed rather than resolved it named the row below the caret. `-n`
caps the union, not each ref, so the wider read returns the same 200 commits
for the same cost.
