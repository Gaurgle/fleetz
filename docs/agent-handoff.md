# Fleetz coordinator handoff

Prepared 2026-10-01. Read this file, applicable repository instructions,
[agent-workflow.md](agent-workflow.md), [release-plan.md](release-plan.md), and
[first-wave-tickets.md](first-wave-tickets.md) before acting.

## Authority and routing

Andreas owns priorities, acceptance, commits, merges, pushes and release.
Astra is the preferred lead. Fable 5.1 may take over coordination when Astra
is unavailable or its usage needs preserving. This is an authorized manual
handoff; there is no automatic coordinator switch or background service.

- Lead: `gpt-6-astra` or `claude-fable-5-1` (Fable effort `high`).
- Codex workhorse: `gpt-6.1-sol`, effort `medium`.
- Substantive Claude worker or independent reviewer: `claude-opus-5-5`,
  effort `medium`.
- Truly small work: Sonnet or `gpt-6-luna`, effort `medium`; record the
  resolved model instead of silently substituting.

Start with one implementation worker and a separate reviewer. Do not recursively
expand delegation. Use architectural review at bounded checkpoints, not routine
board updates. Do not automatically switch to API billing or paid usage credits.

## Current state

Setup writes succeeded for all 23 issues on the private
[Fleetz release Project](https://github.com/users/Gaurgle/projects/1).
Status, priority, Worker and Ticket reviewer fields were filled. All workers
and reviewers are Unassigned. Milestones M0 through M5 exist. Native
blocking links are #16 blocked by #15 and #17 blocked by #16.

- [Release table](https://github.com/users/Gaurgle/projects/1/views/1)
- [Release board](https://github.com/users/Gaurgle/projects/1/views/2)

Both views show Title, Status, Priority, Milestone, Worker and Ticket reviewer.
The final project read exposed 21 items; #29 and #30 had not appeared in the
index despite successful additions and field writes. Recheck all 23 items
and their fields after indexing catches up. Do not duplicate successful writes.
The API verified board grouping by Status. Visual layout remains unverified
because browser automation failed to start. Final item indexing and visual
inspection are the remaining setup checks. GitHub is authoritative for current
status and assignment.

No product implementation, commit, push, merge or release started in this
setup turn. Existing issues remain open.

Uncommitted planning changes include `docs/agent-workflow.md`,
`docs/release-plan.md` and this file. Preserve them. Capture `git status --short`
and the actual base SHA before dispatch. Andreas commits and pushes himself.

## Next implementation pilot

The first pilot is [FZ-02 / #15](https://github.com/Gaurgle/fleetz/issues/15):
preserve newer GitHub state during a local refresh. Follow its acceptance
contract in the issue and `docs/first-wave-tickets.md`.

1. Prepare branch `fix/fz-02-preserve-github-state` in an isolated worktree
   from the agreed planning base; record its absolute path and base SHA.
   Planning documents not yet committed need deliberate inclusion in the
   worker checkout. Do not commit them on Andreas's behalf.
2. Dispatch Sol with the ticket, rules, allowed files and exact base.
3. Use a separate reviewer session against the exact diff and base.
4. Run the workflow's required checks and report evidence. Andreas reviews
   and commits accepted changes. Verify integration separately before he
   merges and pushes.

Do not start implementation simply because an issue is Ready. This setup
turn authorizes board completion and a handoff only. Confirm the current
session's implementation instruction before dispatching the pilot.

## Start Fable manually

From this repository, run:

```sh
claude --model claude-fable-5-1
```

Then give it this prompt:

```text
Read docs/agent-handoff.md and follow docs/agent-workflow.md.
Take over as acting Fleetz coordinator. Inspect the live board and working
tree, preserve uncommitted planning changes, and report the next authorized
step before dispatch. Use the recorded workforce and separate review.
```

Use normal permissions with no bypass. Claude subscription Max authentication
was verified through macOS keychain outside the sandbox, with no active API-key,
auth-token or third-party provider overrides. No model request was run during
board setup. Recheck status and override presence before starting Claude;
never print credentials, change billing or assume subscription headroom.
Fable availability and remaining allowance require a successful current
subscription request. Stop if unavailable or limited; ask Andreas for routing.
