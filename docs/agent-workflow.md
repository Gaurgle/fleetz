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
Current routing, subject to verified availability:

| Role | Model | Reasoning / effort |
|---|---|---|
| Architecture and final checkpoint | `gpt-6-astra` | Bounded decisions and owner-facing checkpoints |
| Independent architectural review | `claude-fable-5-1` | `high`; one scoped review at an architectural checkpoint |
| Rust implementation worker | `gpt-6.1-sol` | `medium`; raise only for a documented blocker |
| Narrow scans and documentation | `gpt-6-luna` | `medium`; explicit paths and deliverable |
| Claude implementation or independent review | `claude-opus-5-5` | `medium`; default for substantive Claude work |
| Truly small Claude changes | `sonnet` | `medium`; record the resolved model from the result |

Do not silently substitute models or escalate to Fable or Astra. If the selected model is unavailable
or inadequate, report the evidence and request a routing decision. Refresh this
table when available models change rather than substituting names silently.

Native Codex children use the coordinator's agent tools. Set their model and
reasoning explicitly; a child assignment does not switch the current root
conversation's model. Claude runs through the separate installed `claude` CLI,
with its own authentication, permissions and usage. It is not a native Codex
child. No orchestration service or recursive delegation is needed for the pilot.

Pass the ticket, repository rules, base commit and relevant file paths. Do not
forward the entire planning conversation. Each task has a focused handoff and
an escalation point. Record actual usage if the client exposes it; otherwise
record elapsed time and rework without inventing token or cost estimates.

After two unsuccessful fix/validation cycles on the same blocker, report the
evidence and ask the coordinator for a decision instead of widening scope.

Fable acts as a co-architect at explicit checkpoints: repository identity,
refresh scheduling, cache contracts and release readiness. Give it the proposal,
constraints and relevant code, and request risks, alternatives and a verdict.
Astra and Andreas resolve disagreements. Do not route routine status updates,
board maintenance or every small patch through both architectural models.
Implementation and review must use separate sessions, even on the same model.

The requested Claude IDs are verified in official documentation, but account
entitlement still requires a successful model request. On Max, Fable shares the
weekly allowance and has a 50% weekly-use ceiling within the subscription;
stop at the limit rather than automatically switching to paid usage credits.
See [Fable plan usage](https://support.claude.com/en/articles/15424964-claude-fable-models-on-your-plan)
and [Opus 5.5](https://platform.claude.com/docs/en/models/opus-5-5/whats-new-opus-5-5).

## Bounded Claude review pilot

Use the existing Claude subscription, not automatic API billing. Before a model
request, run `claude auth status --text` and check that no API-key, auth-token or
third-party provider override is active. Inspect presence only, never print
credentials. A sandbox may prevent macOS keychain access and report a false
negative; verify status in the normal terminal or an approved status-only call
outside the sandbox. Do not log in, alter authentication, or add billing as a
side effect. `--bare` bypasses subscription OAuth/keychain and is unsuitable here.

Prepare `/tmp/fz-02-review-handoff.txt` using the reviewer template below, with
the ticket, base SHA, exact worktree path and relevant rules. Run from that
checkout after subscription billing is confirmed:

```sh
claude -p \
  --model claude-opus-5-5 \
  --effort medium \
  --restricted \
  --tools 'Read,Glob,Grep' \
  --allowedTools 'Read,Glob,Grep' \
  --disallowedTools 'mcp__*' \
  --strict-mcp-config \
  --permission-mode dontAsk \
  --permission-prompts none \
  --max-turns 5 \
  --output-format json \
  < /tmp/fz-02-review-handoff.txt \
  > /tmp/fz-02-claude-review.json
```

This permits file inspection only: no shell, edits, writes, MCP tools or
permission bypass. Restricted mode ignores ordinary settings, so include the
ticket rules explicitly in the handoff. The turn limit bounds the smoke test;
it is not a token or subscription usage cap. Report permission denials and
incomplete evidence. This reviewer cannot execute tests; distinguish inspected
test code and supplied results from checks it actually ran.

Record CLI versions, requested and resolved model, effort, base SHA, checkout,
elapsed time, reported usage, permission denials and verdict. Local inspection
on 2026-10-01 found Codex `0.159.2` and Claude Code `2.1.286`. The smoke test is
pending subscription billing confirmation; no model request has run yet.

References: [Claude CLI](https://code.claude.com/docs/en/cli-reference),
[Claude authentication](https://code.claude.com/docs/en/authentication), and
[Codex subagent configuration](https://learn.chatgpt.com/docs/agent-configuration/subagents).

## Claude-native workforce

When Fable coordinates, the coordinator is the main session
(`claude --agent fz-coordinator`) and Claude workers are native subagents
defined in `.claude/agents/`, not `claude -p` child processes. The roles, ticket
contract, handoff templates and limits in this document apply unchanged.

| Agent | Model | Effort | Edits | Use |
|---|---|---|---|---|
| `fz-coordinator` | `claude-fable-5-1` | `high` | Coordination docs only | Main session: readiness, dispatch, integration checks, board |
| `fz-worker` | `claude-opus-5-5` | `medium` | Assigned worktree, allowed files | Implementation, measurement, investigation |
| `fz-reviewer` | `claude-opus-5-5` | `medium` | None | Independent review of the exact diff |
| `fz-small` | `sonnet` | `medium` | Named paths only | Narrow scans, documentation, truly small changes |

- The coordinator creates the ticket branch and worktree and passes its
  absolute path. The harness's `isolation: worktree` is not used: it branches
  from the default branch under a generated name, so it cannot follow the
  ticket branch convention or an agreed base commit.
- Each worker definition lists its tools explicitly, which leaves out the agent
  and MCP tools: the hierarchy stops one level below the coordinator.
- `fz-reviewer` has a shell and runs the three checks itself, unlike the
  restricted `claude -p` pilot above. It has no edit tools.
- Worker and reviewer are separate invocations, never one agent continued.
- `.claude/` is covered by the owner's global gitignore, so the definitions are
  machine-local unless force-added. A new `agents/` directory is picked up on
  the next session start, not mid-session.

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
Documentation-only tickets need `scripts/check-docs.sh` and a content review,
not unrelated Rust runs.
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
