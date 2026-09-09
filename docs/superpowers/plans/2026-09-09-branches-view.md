# Branches View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a fifth history view, `branches`, listing every local and remote branch for the selected repo, and collapse the info pane's remote-branch list to one expandable row.

**Architecture:** A new `HistoryView::Branches` variant reusing pane 2's existing scroll, selection and inspector machinery. The local-branch row renderer is extracted from `render_detail_pane` so `info` and `branches` draw identical rows from one source. Branch tip hashes ride existing `for-each-ref` calls, so nothing new runs during the fleet sweep.

**Tech Stack:** Rust, ratatui 0.30, crossterm 0.29, tokio. Tests live beside the code in `#[cfg(test)] mod tests`, never in `tests/`.

**Spec:** `docs/superpowers/specs/2026-09-09-branches-view-design.md`

## Global Constraints

- **No em-dashes or en-dashes** in any output: code, comments, docs, commit messages. Use a hyphen, colon, parentheses, or rewrite.
- **Conventional Commits**, first line under 72 characters. Body is for the why.
- **No `Co-Authored-By` lines.**
- **Never edit, delete or disable a test to make code pass.** Fix the code. Adding a required struct field to an existing test fixture is a compile fix, not a weakening; changing an assertion is not.
- **Every behavior change lands with a test that would have caught the old behavior**, named for the regression it guards.
- Gates before every commit: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`. All three must be clean.
- **Commit straight to `main`.** No feature branches, no PRs (confirmed for this work by the user).
- Nothing new may run during the fleet sweep. Every git call added here must ride a call already being made.
- `ui::panel_rects` must keep producing the same rectangles `render` draws.
- Counters are numeric, categorical states are words. Silence must never stand in for a state.

---

### Task 1: Local branches carry their tip hash

**Files:**
- Modify: `src/model.rs:5-15` (`BranchStatus`)
- Modify: `src/git.rs:182-192` (`branch_statuses` format string), `src/git.rs:260-285` (`parse_branch_output`)
- Modify fixtures: `src/git.rs:1086` (`local_branch` helper), `src/model.rs:555`, `src/ui/detail.rs:2201`, `src/ui/detail.rs:2471`, `src/ui/detail.rs:2478`, `src/ui/detail.rs:2485`
- Test: `src/git.rs` `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: nothing.
- Produces: `BranchStatus.tip: String`: the branch's tip commit as a short hash, empty string when `for-each-ref` gave no field (a repo with no commits).

- [ ] **Step 1: Write the failing test**

Add to `src/git.rs` tests:

```rust
/// The failure this closes: a branch row had no way to reach its own commit,
/// so the branches view could not feed the inspector. The hash rides the
/// `for-each-ref` that was already asking for `%(HEAD)` rather than costing a
/// second subprocess.
#[test]
fn branch_output_carries_the_tip_hash() {
    let out = "*\tmain\torigin/main\t[ahead 2]\ta1b2c3d\n \tbeta\t\t\te4f5a6b\n";
    let parsed = parse_branch_output(out);
    assert_eq!(parsed[0].name, "main");
    assert_eq!(parsed[0].tip, "a1b2c3d");
    assert_eq!(parsed[1].name, "beta");
    assert_eq!(parsed[1].tip, "e4f5a6b");
}

/// A repo with no commits yields no object name. The field is empty rather
/// than the row being dropped: the branch exists and must still be listed.
#[test]
fn a_branch_with_no_tip_parses_with_an_empty_hash() {
    let parsed = parse_branch_output("*\tmain\t\t\t\n");
    assert_eq!(parsed[0].name, "main");
    assert_eq!(parsed[0].tip, "");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test branch_output_carries_the_tip_hash`
Expected: FAIL to compile, "no field `tip` on type `BranchStatus`".

- [ ] **Step 3: Add the field**

In `src/model.rs`, inside `pub struct BranchStatus`, after `is_head`:

```rust
    /// The branch's tip commit, short form, from `for-each-ref`'s
    /// `%(objectname:short)`.
    ///
    /// Rides the query that was already being made, the same way `is_head`
    /// does. It is what lets the branches view hand the inspector a commit.
    /// Empty in a repo with no commits, where there is no object to name.
    pub tip: String,
```

- [ ] **Step 4: Ask git for it**

In `src/git.rs`, `branch_statuses`, replace the format line with:

```rust
            "--format=%(HEAD)\t%(refname:short)\t%(upstream:short)\t%(upstream:track)\t%(objectname:short)",
```

- [ ] **Step 5: Parse it**

In `parse_branch_output`, after the `let track = ...` line add:

```rust
            let tip = parts.get(4).unwrap_or(&"").trim().to_string();
```

and add `tip,` to the `BranchStatus { .. }` literal.

- [ ] **Step 6: Fix the six fixtures**

Add `tip: String::new()` to each `BranchStatus { .. }` literal at `src/git.rs:1087`, `src/model.rs:555`, `src/ui/detail.rs:2201`, `src/ui/detail.rs:2471`, `src/ui/detail.rs:2478`, `src/ui/detail.rs:2485`. These are compile fixes; do not change any assertion.

- [ ] **Step 7: Run the gates**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: all clean, both new tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/model.rs src/git.rs src/ui/detail.rs
git commit -m "feat: carry each local branch's tip hash

Rides the for-each-ref already asking for %(HEAD), so the branches view
can hand the inspector a commit without a second subprocess."
```

---

### Task 2: Remote branches carry tips, and the full list reaches the model

**Files:**
- Modify: `src/model.rs` (new `RemoteBranch` struct; `RepoStatus` gains `remote_branches`)
- Modify: `src/git.rs:865-905` (`remote_branches`, `parse_remote_branch_output`), `src/git.rs:916-928` (`unfetched_branches`)
- Modify: `src/main.rs:1913`, `:2107`, `:2565`, `:2660` (call sites) and the four `RepoStatus { .. }` literals that follow them
- Test: `src/git.rs` tests

**Interfaces:**
- Consumes: `BranchStatus.tip` from Task 1 (not used here, but the same field pattern).
- Produces:
  - `model::RemoteBranch { pub name: String, pub tip: String }`, `#[derive(Debug, Clone, PartialEq, Eq)]`
  - `git::remote_branches(&Path) -> Vec<RemoteBranch>`
  - `git::unfetched_branches(remote: &[RemoteBranch], local: &[BranchStatus]) -> Vec<String>` (return type unchanged: short names)
  - `RepoStatus.remote_branches: Vec<RemoteBranch>`

- [ ] **Step 1: Write the failing test**

Add to `src/git.rs` tests:

```rust
/// The failure this closes: only the *unfetched* subset survived the sweep,
/// so a branches view had no way to list the remotes that are fetched. The
/// full list was already being computed and thrown away.
#[test]
fn remote_branch_output_carries_names_and_tips() {
    let out = "refs/remotes/origin/HEAD\torigin\tdeadbee\n\
               refs/remotes/origin/main\torigin/main\ta1b2c3d\n\
               refs/remotes/origin/wip\torigin/wip\te4f5a6b\n";
    let parsed = parse_remote_branch_output(out);
    assert_eq!(parsed.len(), 2, "the HEAD symref must still be dropped");
    assert_eq!(parsed[0].name, "origin/main");
    assert_eq!(parsed[0].tip, "a1b2c3d");
    assert_eq!(parsed[1].name, "origin/wip");
}

/// `unfetched_branches` keeps both of its match rules once the input carries
/// tips: a remote is claimed when a local branch shares its short name, or
/// names it as upstream. The upstream half is the one that regressed before
/// (see `docs/ideas.md`, "The behind count and the branches that make it up").
#[test]
fn unfetched_still_matches_on_name_and_on_upstream() {
    let remote = vec![
        RemoteBranch { name: "origin/main".into(), tip: "a1b2c3d".into() },
        RemoteBranch { name: "origin/wip".into(), tip: "e4f5a6b".into() },
        RemoteBranch { name: "origin/nobody".into(), tip: "c0ffee1".into() },
    ];
    let local = vec![
        local_branch("main", None),
        local_branch("feature/who-goes-first", Some("origin/wip")),
    ];
    assert_eq!(unfetched_branches(&remote, &local), vec!["origin/nobody"]);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test remote_branch_output_carries_names_and_tips`
Expected: FAIL to compile, "cannot find struct `RemoteBranch`".

- [ ] **Step 3: Add the struct**

In `src/model.rs`, beside `BranchStatus`:

```rust
/// A remote-tracking branch: its short name (`origin/feature-x`) and the tip
/// it points at.
///
/// The tip rides the `for-each-ref` that was already listing these, the same
/// way `BranchStatus::tip` does. Note that a remote branch fleetz calls "not
/// fetched" still has a local object behind it: the phrase means no local
/// *branch* claims it, not that the ref is absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBranch {
    pub name: String,
    pub tip: String,
}
```

- [ ] **Step 4: Return it from git**

In `src/git.rs`, change `remote_branches` to return `Vec<RemoteBranch>`, and its format line to:

```rust
            "--format=%(refname)\t%(refname:short)\t%(objectname:short)",
```

Rewrite `parse_remote_branch_output` to return `Vec<RemoteBranch>`:

```rust
fn parse_remote_branch_output(stdout: &str) -> Vec<RemoteBranch> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let full = parts.next()?;
            let short = parts.next()?.trim();
            // The symref has to be matched on its FULL refname:
            // `refs/remotes/origin/HEAD` shortens to bare `origin`, so an
            // `ends_with("/HEAD")` test against the short form never fires.
            if full.ends_with("/HEAD") || short.is_empty() {
                return None;
            }
            Some(RemoteBranch {
                name: short.to_string(),
                tip: parts.next().unwrap_or_default().trim().to_string(),
            })
        })
        .collect()
}
```

- [ ] **Step 5: Update `unfetched_branches`**

```rust
pub fn unfetched_branches(remote: &[RemoteBranch], local: &[BranchStatus]) -> Vec<String> {
    remote
        .iter()
        .filter(|r| {
            let short = r.name.split_once('/').map_or(r.name.as_str(), |(_, n)| n);
            !local
                .iter()
                .any(|l| l.name == short || l.upstream.as_deref() == Some(r.name.as_str()))
        })
        .map(|r| r.name.clone())
        .collect()
}
```

Keep the existing doc comment above it verbatim: it records why the upstream half of the test exists.

- [ ] **Step 6: Carry the list into the model**

In `src/model.rs`, `RepoStatus`, beside `remote_only_branches`:

```rust
    /// Every remote-tracking branch, fetched or not.
    ///
    /// Free: `git::remote_branches` was already run during the sweep and its
    /// result reduced to `remote_only_branches` and discarded. The branches
    /// view lists all of them, so the whole list is kept instead.
    pub remote_branches: Vec<RemoteBranch>,
```

At each of the four call sites (`src/main.rs:1913`, `:2107`, `:2565`, `:2660`), the local binding already holds the full list. Add `remote_branches: remote,` to the `RepoStatus { .. }` literal that follows, placing it *after* the existing `let remote_only_branches = git::unfetched_branches(&remote, &branches);` line so the borrow still precedes the move. Add `remote_branches: vec![]` to the two fixture literals at `src/app.rs:2258` and `src/main.rs:2484`.

- [ ] **Step 7: Run the gates**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: all clean.

- [ ] **Step 8: Commit**

```bash
git add src/model.rs src/git.rs src/main.rs src/app.rs
git commit -m "feat: keep the full remote branch list and its tips

remote_branches was already run every sweep and reduced to the unfetched
subset, so keeping all of it costs no git calls. Tips ride the same
for-each-ref."
```

---

### Task 3: One local-branch row renderer, shared

**Files:**
- Modify: `src/ui/detail.rs:677-757` (the local-branch loop inside `render_detail_pane`)
- Test: `src/ui/detail.rs` tests

**Interfaces:**
- Consumes: `BranchStatus` from Task 1.
- Produces: `fn branch_row_lines(b: &BranchStatus, worktrees: &[Worktree], shown: &mut HashSet<String>) -> Vec<Line<'static>>`: the branch's own row plus one indented row per worktree checked out on it. `shown` collects the worktree paths consumed, so the caller can still list orphan worktrees afterwards.

- [ ] **Step 1: Write the failing test**

Add to `src/ui/detail.rs` tests:

```rust
/// The failure this closes: `info` and `branches` would each grow their own
/// copy of this row, and two copies of a row vocabulary drift exactly the way
/// two copies of layout arithmetic do. One function, called by both.
#[test]
fn a_branch_row_says_head_upstream_and_divergence_once() {
    let b = crate::model::BranchStatus {
        name: "feature/who-goes-first".into(),
        upstream: Some("origin/wip".into()),
        behind: 19,
        ahead: 0,
        is_head: true,
        tip: "a1b2c3d".into(),
    };
    let mut shown = std::collections::HashSet::new();
    let rows = branch_row_lines(&b, &[], &mut shown);
    let text: String = rows[0].spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.starts_with("* "), "HEAD marker missing: {text:?}");
    assert!(text.contains("-> origin/wip"), "upstream missing: {text:?}");
    assert!(text.contains("19 behind"), "divergence missing: {text:?}");
}

/// The arrow is for the odd one out. Repeating `origin/x` after every `x`
/// down the whole list is noise that would bury the case worth seeing.
#[test]
fn a_branch_tracking_its_own_name_stays_bare() {
    let b = crate::model::BranchStatus {
        name: "main".into(),
        upstream: Some("origin/main".into()),
        behind: 0,
        ahead: 0,
        is_head: false,
        tip: "a1b2c3d".into(),
    };
    let mut shown = std::collections::HashSet::new();
    let rows = branch_row_lines(&b, &[], &mut shown);
    let text: String = rows[0].spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(!text.contains("->"), "arrow drawn for a matching upstream: {text:?}");
    assert!(text.contains('✓'), "level branch lost its tick: {text:?}");
}

/// An untracked branch rendering bare reads as "fine" when it means "not on
/// any remote" -- the same trap as a not-cloned repo whose counters are zero.
#[test]
fn a_branch_with_no_upstream_says_so() {
    let b = crate::model::BranchStatus {
        name: "beta".into(),
        upstream: None,
        behind: 0,
        ahead: 0,
        is_head: false,
        tip: "a1b2c3d".into(),
    };
    let mut shown = std::collections::HashSet::new();
    let rows = branch_row_lines(&b, &[], &mut shown);
    let text: String = rows[0].spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("no upstream"), "silence stood in for a state: {text:?}");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test a_branch_row_says_head_upstream_and_divergence_once`
Expected: FAIL to compile, "cannot find function `branch_row_lines`".

- [ ] **Step 3: Extract the function**

Move the body of the `for b in &repo.branches { .. }` loop in `render_detail_pane` (`src/ui/detail.rs:677-757`) into a new free function, unchanged in behaviour. Keep every existing comment: they record why the `*` marker, the conditional arrow and the `no upstream` wording exist.

```rust
/// One local branch's rows: the branch itself, then any worktrees checked out
/// on it, indented beneath.
///
/// Shared by `info` and `branches` so the two cannot drift into drawing the
/// same branch differently -- the failure `detail::split_inspector` exists to
/// prevent, one level up in the same file.
///
/// `shown` collects the worktree paths this consumed, so the caller can still
/// list the orphans afterwards.
fn branch_row_lines(
    b: &crate::model::BranchStatus,
    worktrees: &[crate::model::Worktree],
    shown: &mut std::collections::HashSet<String>,
) -> Vec<Line<'static>> {
    // ... the moved body, pushing into a local `Vec<Line>` and returning it ...
}
```

Replace the loop in `render_detail_pane` with:

```rust
        for b in &repo.branches {
            lines.extend(branch_row_lines(b, &repo.worktrees, &mut shown_worktrees));
        }
```

Note `shown_worktrees` is currently a `HashSet<&String>`; change it to `HashSet<String>` and adjust the orphan-worktree filter at `src/ui/detail.rs:959` accordingly (`!shown_worktrees.contains(&wt.path)`).

- [ ] **Step 4: Run the tests**

Run: `cargo test`
Expected: the three new tests pass, and **every existing info-pane render test still passes unchanged**. If any existing assertion fails, the extraction changed behaviour: fix the extraction, never the assertion.

- [ ] **Step 5: Run the gates and commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add src/ui/detail.rs
git commit -m "refactor: extract the local branch row renderer

info and branches must draw the same branch identically; two copies of a
row vocabulary drift the way two copies of layout arithmetic do."
```

---

### Task 4: The branches view

**Files:**
- Modify: `src/app.rs:37-67` (`HistoryView`), `src/app.rs:1516` (`selected_commit_count`), `src/app.rs:1595` (`detail_nth_hash`)
- Modify: `src/ui/detail.rs` (`render_detail_pane`: a `Branches` arm)
- Test: `src/ui/detail.rs` tests, `src/app.rs` tests

**Interfaces:**
- Consumes: `branch_row_lines` (Task 3), `BranchStatus.tip` (Task 1), `RepoStatus.remote_branches` + `RemoteBranch` (Task 2).
- Produces: `HistoryView::Branches` with `label() == "branches"` and `index() == 4`; the view's selectable entries are local branches first, then remote branches, in list order.

- [ ] **Step 1: Write the failing test**

Add to `src/ui/detail.rs` tests. Reuse the existing whole-screen render helper in that module for a realistic frame.

```rust
/// The branches view lists every remote, not only the unfetched ones: that
/// distinction is a marker on a row, not a reason to omit the row.
#[test]
fn the_branches_view_lists_local_and_remote_sections() {
    let out = branches_screen();
    assert!(out.contains("local branches"), "no local section:\n{out}");
    assert!(out.contains("remotes"), "no remotes section:\n{out}");
    assert!(out.contains("origin/main"), "a fetched remote was dropped:\n{out}");
    assert!(out.contains("origin/nobody"), "an unfetched remote was dropped:\n{out}");
    assert!(out.contains("not fetched"), "the unfetched marker is missing:\n{out}");
}

/// A remote nothing local claims is marked; one a local branch claims is not.
/// Silence must not stand in for either state.
#[test]
fn only_unclaimed_remotes_are_marked_not_fetched() {
    let out = branches_screen();
    let marked: Vec<&str> = out
        .lines()
        .filter(|l| l.contains("not fetched"))
        .collect();
    assert!(
        marked.iter().all(|l| l.contains("origin/nobody")),
        "a claimed remote was marked not fetched: {marked:?}"
    );
}
```

Add the fixture helper beside them:

```rust
/// A repo with one HEAD branch, one tracking branch, and three remotes of
/// which one is unclaimed, rendered in the branches view.
fn branches_screen() -> String {
    // Build via the module's existing whole-screen render helper, setting:
    //   app.history_view = HistoryView::Branches;
    //   repo.branches      = [main -> origin/main (level), beta (no upstream)]
    //   repo.remote_branches = [origin/main, origin/beta, origin/nobody]
    //   repo.remote_only_branches = ["origin/nobody"]
    // Return the flattened screen text.
    todo!("use the existing screen helper in this module")
}
```

**Note to the implementer:** replace that `todo!` with the module's actual render-to-string helper before running anything. It is the only placeholder in this plan and exists because the helper's exact name differs between `ui/mod.rs` (`screen`) and `ui/detail.rs`; check which is in scope and use it.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test the_branches_view_lists_local_and_remote_sections`
Expected: FAIL to compile, "no variant named `Branches`".

- [ ] **Step 3: Add the variant**

In `src/app.rs`:

```rust
pub enum HistoryView {
    #[default]
    Info,
    Commits,
    Reflog,
    Graph,
    Branches,
}
```

```rust
    pub const ALL: [Self; 5] = [
        Self::Info,
        Self::Commits,
        Self::Reflog,
        Self::Graph,
        Self::Branches,
    ];
```

Add `Self::Branches => 4` to `index()` and `Self::Branches => "branches"` to `label()`.

- [ ] **Step 4: Count and resolve its entries**

In `selected_commit_count`, add:

```rust
            // Local branches then remotes, in the order the view draws them.
            HistoryView::Branches => repo.branches.len() + repo.remote_branches.len(),
```

In `detail_nth_hash`, add:

```rust
            // A branch resolves to its own tip. Locals come first, so an index
            // past them names a remote.
            HistoryView::Branches => {
                let n = repo.branches.len();
                match i < n {
                    true => repo.branches.get(i).map(|b| b.tip.as_str()),
                    false => repo.remote_branches.get(i - n).map(|r| r.tip.as_str()),
                }
                .filter(|h| !h.is_empty())
            }
```

- [ ] **Step 5: Render it**

In `render_detail_pane`, add a `Branches` arm that pushes `section_label("local branches")`, then `branch_row_lines` per local branch, then `section_label("remotes")`, then one row per `repo.remote_branches` entry, marked when its name is in `repo.remote_only_branches`:

```rust
        for r in &repo.remote_branches {
            let mut spans = vec![Span::styled(
                format!("  {}", r.name),
                Style::default().fg(colors::OVERLAY),
            )];
            if repo.remote_only_branches.contains(&r.name) {
                spans.push(Span::styled(
                    " not fetched",
                    Style::default().fg(colors::SUBTEXT),
                ));
            }
            lines.push(Line::from(spans));
        }
```

Record `block_tops` for each branch row so the cursor and scroll behave as they do in the other views.

- [ ] **Step 6: Run the tests**

Run: `cargo test`
Expected: the two new tests pass. Existing tests that assert `HistoryView::COUNT` or cycle through `ALL` will now see five views; check each such failure is only a count change and fix the *test's expected count*, not its behaviour. If any asserts a specific cycle order, `branches` sits after `graph`.

- [ ] **Step 7: Run the gates and commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add src/app.rs src/ui/detail.rs
git commit -m "feat: add a branches view to pane 2

Local branches and remotes in one list, reusing the pane's scroll,
selection and inspector rather than a new pane. Read-only: delete stays
in the b overlay."
```

---

### Task 5: The inspector says when a tip is unreachable

**Files:**
- Modify: `src/ui/detail.rs` (`render_inspector`)
- Test: `src/ui/detail.rs` tests

**Interfaces:**
- Consumes: Task 4's `detail_nth_hash` `Branches` arm.
- Produces: no new API.

- [ ] **Step 1: Write the failing test**

```rust
/// The failure this closes: `git::commits` and `git::graph` walk the union of
/// *local* branches capped at 200, so a remote-only branch's tip resolves to
/// nothing and the inspector drew an empty pane. Categorical states are words:
/// silence must not stand in for one.
#[test]
fn an_unreachable_branch_tip_says_so_instead_of_drawing_nothing() {
    let out = branches_screen_with_selection(/* index of a remote-only row */);
    assert!(
        out.contains("not on any local branch"),
        "the inspector went blank on an unreachable tip:\n{out}"
    );
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test an_unreachable_branch_tip_says_so_instead_of_drawing_nothing`
Expected: FAIL, the string is absent.

- [ ] **Step 3: Implement**

In `render_inspector`, where it currently returns early or draws nothing because `inspected_commit()` is `None`, add: when `app.history_view == HistoryView::Branches` and the selection resolves to a non-empty hash that is not in `repo.graph`, draw the hash and the line `not on any local branch` in `colors::OVERLAY`.

- [ ] **Step 4: Run the gates and commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add src/ui/detail.rs
git commit -m "fix: say when a branch tip is not on any local branch

The commit list walks the union of local branches, so a remote-only tip
resolves to nothing. An empty inspector read as 'no detail' rather than
'not reachable from here'."
```

---

### Task 6: Collapse the info pane's remote list

**Files:**
- Modify: `src/app.rs` (new `expand_remote_branches: bool` field, default `false`)
- Modify: `src/main.rs` (`B` key in `handle_normal_input`, beside the existing `KeyCode::Char('b')` arm)
- Modify: `src/ui/detail.rs` (the remote-only block in `render_detail_pane`)
- Test: `src/ui/detail.rs` tests

**Interfaces:**
- Consumes: Task 3's extraction (the local branches above it are untouched).
- Produces: `App.expand_remote_branches: bool`.

- [ ] **Step 1: Write the failing test**

```rust
/// The reported bug: 23 remote branches sat between the pinned local branches
/// and the commit list, so `recent commits` started below the fold and the
/// selection scrolled the branches away the moment the cursor moved.
#[test]
fn info_keeps_the_commit_list_on_the_first_screen_with_many_remotes() {
    let out = info_screen_with_remote_branches(23);
    assert!(
        out.contains("23 remote branches not fetched"),
        "the list did not collapse:\n{out}"
    );
    assert!(
        out.contains("recent commits"),
        "the commit list is still below the fold:\n{out}"
    );
}

/// Collapsed is not a dead end: the names are still reachable in place.
#[test]
fn expanding_the_summary_draws_every_remote_name() {
    let out = info_screen_expanded_with_remote_branches(23);
    assert!(!out.contains("23 remote branches not fetched"), "still collapsed:\n{out}");
    assert!(out.contains("origin/branch-0"), "names missing when expanded:\n{out}");
    assert!(out.contains("origin/branch-22"), "names truncated when expanded:\n{out}");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test info_keeps_the_commit_list_on_the_first_screen_with_many_remotes`
Expected: FAIL, the summary string is absent.

- [ ] **Step 3: Add the flag**

In `src/app.rs`, on `App`:

```rust
    /// Whether `info` draws the remote-branch names or just their count.
    ///
    /// App-level and session-lived rather than per repo: it is a preference
    /// about how you want `info` to read, not a fact about one repo. Resets
    /// on launch.
    pub expand_remote_branches: bool,
```

Initialise it `false` in `App::new`.

- [ ] **Step 4: Draw the summary**

Replace the remote-only block in `render_detail_pane` with:

```rust
        // Collapsed to a count by default. A remote-only branch name is not
        // identifying the way a PR number or a changed path is -- three of
        // twenty-three feature branches a colleague pushed tell you nothing
        // the count does not, while costing three rows of the commit list.
        // `B` expands it; `branches` lists them all with their state.
        if !repo.remote_only_branches.is_empty() {
            if app.expand_remote_branches {
                for name in &repo.remote_only_branches {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {name}"), Style::default().fg(colors::OVERLAY)),
                        Span::styled(" not fetched", Style::default().fg(colors::SUBTEXT)),
                    ]));
                }
            } else {
                lines.push(Line::from(Span::styled(
                    format!(
                        "  {} remote branch{} not fetched",
                        repo.remote_only_branches.len(),
                        if repo.remote_only_branches.len() == 1 { "" } else { "es" }
                    ),
                    Style::default().fg(colors::OVERLAY),
                )));
            }
        }
```

- [ ] **Step 5: Bind the key**

In `src/main.rs`, beside the existing `KeyCode::Char('b') | KeyCode::Char('s')` arm:

```rust
        // Expand or collapse the remote branch summary in `info`. The full
        // list with its state lives in the `branches` view; this is for a
        // quick look without leaving the glance.
        KeyCode::Char('B') => {
            app.expand_remote_branches = !app.expand_remote_branches;
            app.needs_redraw = true;
        }
```

- [ ] **Step 6: Run the gates and commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add src/app.rs src/main.rs src/ui/detail.rs
git commit -m "fix: collapse info's remote branch list to a count

Twenty-plus not-fetched rows sat between the pinned branches and the
commit list, so the selection scrolled them away as soon as the cursor
moved. B expands them in place; branches lists them all."
```

---

### Task 7: Documentation

**Files:**
- Modify: `README.md:122` (the `[` / `]` row), `README.md` keybinding table (add `B`)
- Modify: `src/ui/help.rs` (the `[`/`]` line, and a `B` entry)
- Modify: `docs/ideas.md` (a new `## Branches view` section)

- [ ] **Step 1: README**

Update the `[` / `]` row to name five views: `info, commits, reflog, graph, branches`. Add a row: `` | `B` | `info`: expand or collapse the remote branch summary | ``.

- [ ] **Step 2: Help overlay**

Update the `[`/`]` description string to list five views, and add a `("B", "info: expand the remote branch summary")` entry.

- [ ] **Step 3: ideas.md**

Add a `## Branches view` section with **Status: shipped**, recording: why a fifth view rather than a new pane or an overlay (lazygit's left column is one repo, fleetz's is the fleet); why read-only for now (the `b` overlay already deletes, and whether you want to delete from a reference list is a question the built view answers better); why the data additions are free (`remote_branches` was already run and discarded, tips ride existing `for-each-ref` calls); why `info` collapses to a bare count rather than the house sample-plus-count pattern (a branch name is not identifying the way a PR number is); and the known limitation that a remote-only tip is not in the local-branch union that `git::commits` and `git::graph` walk, with the note that widening them would have to be done to both together.

- [ ] **Step 4: Commit**

```bash
git add README.md src/ui/help.rs docs/ideas.md
git commit -m "docs: record the branches view and the B toggle"
```

---

## Self-Review

**Spec coverage.** Every spec section maps to a task: the fifth view (Task 4), read-only (no task mutates), nothing new in the sweep (Tasks 1 and 2 both ride existing calls), the three data additions (Tasks 1 and 2), the shared local-row renderer (Task 3), the remotes section listing all remotes (Task 4), the inspector arm (Task 4) and its limitation (Task 5), the info collapse and `B` (Task 6), docs (Task 7). All seven of the spec's listed tests appear across Tasks 1-6.

**Placeholders.** One deliberate `todo!` in Task 4 Step 1, flagged inline with instructions: the render-to-string helper's name differs between `ui/mod.rs` and `ui/detail.rs` and must be checked in scope. Task 5's test names a helper (`branches_screen_with_selection`) the implementer writes from Task 4's `branches_screen`; same for Task 6's two `info_screen_*` helpers.

**Type consistency.** `BranchStatus.tip: String` (Task 1) is read in Task 4's `detail_nth_hash`. `RemoteBranch { name, tip }` (Task 2) is constructed in Task 2's test, stored on `RepoStatus.remote_branches`, and read in Task 4's renderer and `detail_nth_hash`. `unfetched_branches` keeps returning `Vec<String>` so `remote_only_branches` is unchanged, which Tasks 4 and 6 both rely on. `branch_row_lines` has one signature, defined in Task 3 and called in Tasks 3 and 4.
