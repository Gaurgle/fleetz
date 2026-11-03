# Fleetz agent instructions

Read [CLAUDE.md](CLAUDE.md) first. Its project rules, ship policy and
validation requirements apply to Claude and Codex alike. The domain
invariants live in `.claude/rules/`, one file per area with the source files
it governs in its frontmatter; Claude loads one when it touches a matching
file, and Codex reads the file for the module it is about to change (the
table in CLAUDE.md says which). The release-effort ship policy is an explicit
specialization of the default house rule, not permission for every session
to commit, push or merge.

Apply instructions by scope: shared global defaults, any project family rules,
this repository's rules, then instructions in the module being changed. Read
applicable `CLAUDE.md` and `AGENTS.md` files at each level. More specific
instructions specialize broader defaults for their scope; keep broader rules
where they do not conflict. No project or module rule expands the owner's
authorization.

For Relay work, read [the workflow](docs/agent-workflow.md). If authorized to
take over as lead, run `scripts/relay-check` and report its drift lines,
then read [the live handoff](docs/agent-handoff.md),
[the release plan](docs/release-plan.md) and the board before acting. Confirm
the previous lead stopped, reconcile live state, and honor the handoff's
confirmation requirements and stop instructions. Preserve the owner's
authorization and limits; setup or takeover alone does not authorize tickets.

The existing-test exception in the workflow's Validation contract is
deliberate and narrow. Do not replace it with a generic template's test policy.

Claude agent definitions in `.claude/agents/` describe the corresponding roles.
Codex uses its available agent tools and the same handoff contract, subject to
the active runtime's permissions and delegation limits. Reading this file
does not make an ordinary session the Relay coordinator.
