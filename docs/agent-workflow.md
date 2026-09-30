# Agent workflow for the release effort

This is a project-specific working agreement for the tickets in
[release-plan.md](release-plan.md). It does not authorize implementation,
external publication, dependencies, CI changes, commits or pushes by itself.
The [Fleetz public release Project](https://github.com/users/Gaurgle/projects/1)
is authoritative for live status, priority, worker and reviewer assignment.

## Roles and limits

- Andreas owns priorities, interaction acceptance, commits, merges, pushes and release.
- Coordinator owns architecture, ticket readiness, assignments and integration.
- Worker implements one assigned ticket and supplies evidence.
- Reviewer independently evaluates that ticket's exact diff and criteria.

Start with one implementation worker and one reviewer. After the FZ-02 pilot,
allow at most two simultaneous implementation workers with disjoint ownership.
Do not create another management layer or recursively delegate by default.
Claude and Codex use the same ticket contract; cross-provider dispatch may be
manual. A ticket remains usable without a custom orchestration service.

## Model and context budget

The coordinator explicitly selects a model available in the running client.
Use a capable workhorse for Rust and state/concurrency changes, a lighter model
for bounded searches and documentation, and Astra for unresolved architecture,
high-risk review and milestone decisions. Avoid hardcoded obsolete model names.

Pass the ticket, repository rules, base commit and relevant file paths. Do not
forward the entire planning conversation. Each task has a focused handoff and
an escalation point. Record actual usage if the client exposes it; otherwise
record elapsed time and rework without inventing token or cost estimates.

After two unsuccessful fix/validation cycles on the same blocker, report the
evidence and ask the coordinator for a decision instead of widening scope.

## Isolation and integration

1. Record `git status --short` and the agreed `main` base commit before dispatch.
   Preserve existing user changes. Create one feature branch and worktree per
   implementation ticket, for example `fix/fz-02-preserve-github-state`.
2. Each worker gets one checkout and a declared file scope. Do not edit another
   agent's worktree. Coordination files are written only by the coordinator.
3. Include the planning documents in the worker checkout if they are not yet
   committed; they do not automatically appear in a worktree from an older base.
4. Shared event/model contracts are decided before parallel implementation.
   Worktrees prevent file interference, not incompatible designs.
5. A worker leaves reviewed changes uncommitted on its feature branch and
   reports the branch, worktree path, base, changed files and untracked files.
   Do not rely on `git diff` alone to
   transport new files. Do not commit as a shortcut for handing off work.
6. Review the exact diff, including added files. If changes follow review,
   re-review the changed portion and affected acceptance criteria.
7. Andreas commits accepted work on its feature branch. Coordinator verifies
   integration against current `main` in an isolated checkout, reports conflicts
   and runs checks on the combined result. Andreas performs the final merge and
   push. If `main` moves, revalidate the changed integration result before merging.
   Worktree checks alone do not establish integration safety.
8. Retain the worktree until the result is integrated or explicitly discarded.
   Do not remove files/worktrees without authorization.

PRs are optional. The release Project and planning tickets are published. New
external publication remains a separate explicit action unless already
authorized for the current task.

## Validation contract

For code changes, reproduce the problem with a meaningful regression test when
applicable, implement the fix, then run:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Use targeted tests during iteration. Never weaken, delete or disable an
existing test to make the implementation pass. If a contract appears wrong,
stop that change and explain it. Adding tests is allowed within ticket scope.
Documentation-only tickets need link/content review, not unrelated Rust runs.
Performance claims need comparable measurements, not just passing tests.

The baseline from the 2026-09-30 review was 643 tests passed and 1 ignored;
this is historical evidence, not a substitute for validation at the new base.

## Worker handoff template

```text
Ticket: <ID and full ticket body or path>
Role: implementation / measurement / investigation
Model: <explicit model and reasoning setting>
Base commit: <SHA>
Branch: <ticket feature branch>
Worktree: <absolute path>
Allowed files: <paths; read other files as needed>
Dependencies accepted: <IDs or none>

Read applicable repository instructions and docs/agent-workflow.md.
Deliver exactly this ticket's outcome. Keep unrelated improvements out.
Verify the reported issue before fixing it; report contradictory evidence.
Do not change dependencies, CI, persistence schemas or external API contracts
without the required approval. Escalate major refactors or ownership changes.
Do not commit, merge, push, publish, install globally or modify another checkout.

Return:
- Outcome and acceptance criteria satisfied or unresolved.
- Changed and untracked files, base SHA, branch and worktree path.
- Exact checks run and results, including failures or skipped checks.
- Reproduction evidence and relevant before/after measurements.
- Risks, follow-ups and decisions needed, briefly.
```

## Reviewer handoff template

```text
Ticket: <ID and acceptance criteria>
Base commit: <SHA>
Branch: <ticket feature branch>
Worktree: <absolute path>
Worker report: <concise report>

Review independently. Treat the report as a claim to verify.
Read the actual diff and untracked additions. Check scope, contracts, failure
paths, state ownership and regression coverage. For async changes, trace both
completion orders. Run relevant tests and check the supplied evidence.
Do not edit production code or relax tests during review.

Return one verdict: accepted / changes requested / blocked.
For each finding give severity, file and symbol, triggering scenario and
required correction. Separate blockers from optional follow-ups. State which
checks you independently ran and which evidence you only inspected.
```

## Owner-facing progress

Coordinator reports at handoffs or meaningful blockers:

```text
FZ-02: Review. Worker reproduced the race and implemented field-safe merging.
Validation: targeted regression and full checks passed in the worktree.
Next: independent review, then integration checks.
Decision needed: none.
```

Use actual evidence, not optimistic percentages. Keep the board current when
an assignment, blocker or review verdict changes. Ready to integrate is still
unfinished; Done includes the owner's commit, integration into main and
combined verification.
