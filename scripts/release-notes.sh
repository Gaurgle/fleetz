#!/usr/bin/env bash
# Print a "What's Changed" body for a release, built from commits rather than
# PRs: this repo commits straight to main, so GitHub's own note generator
# would list nothing. Usage: scripts/release-notes.sh <tag> [previous-tag]
set -euo pipefail

tag="$1"
prev="${2:-$(git describe --tags --abbrev=0 "$tag^" 2>/dev/null || true)}"
range="${prev:+$prev..}$tag"
repo="$(gh repo view --json nameWithOwner -q .nameWithOwner)"

# Coordination records are committed straight to main and would swamp the notes.
coordination_only() {
  local path seen=0
  while IFS= read -r path; do
    seen=1
    case "$path" in
      docs/agent-handoff.md|docs/agent-log.md|docs/agent-workflow.md|docs/release-plan.md) ;;
      docs/superpowers/*|.claude/*) ;;
      *) return 1 ;;
    esac
  done < <(git diff-tree --no-commit-id --name-only -r "$1")
  [ "$seen" -eq 1 ]
}

# Each commit as "<type>\t<subject without prefix>\t<hash>".
commits="$(git log --no-merges --format='%s%x09%h' "$range" \
  | while IFS= read -r line; do
      coordination_only "${line##*$'\t'}" || printf '%s\n' "$line"
    done \
  | awk -F'\t' '{
      type = "other"; subj = $1
      if (match($1, /^[a-z]+(\([^)]*\))?!?:[ ]*/)) {
        type = $1; sub(/[(:!].*/, "", type)
        subj = substr($1, RLENGTH + 1)
      }
      printf "%s\t%s\t%s\n", type, subj, $2
    }')"

section() {
  local title="$1"; shift
  local lines="" group
  for type in "$@"; do
    # Re-terminated per group: command substitution eats the trailing newline,
    # so a section listing several types ran the last bullet of one group into
    # the first of the next -- "...))* run the gates" on one line.
    group="$(printf '%s\n' "$commits" | awk -F'\t' -v t="$type" -v r="$repo" \
      '$1 == t { printf "* %s ([%s](https://github.com/%s/commit/%s))\n", $2, $3, r, $3 }')"
    [ -n "$group" ] && lines+="$group"$'\n'
  done
  [ -n "$lines" ] || return 0
  printf '### %s\n%s\n' "$title" "$lines"
}

echo "## What's Changed"
echo
section "Features" feat
section "Fixes" fix
section "Refactoring" refactor
section "Docs" docs
section "Tests" test
section "Housekeeping" chore cleanup build ci
section "Other" other

if [ -n "$prev" ]; then
  echo "**Full Changelog**: https://github.com/$repo/compare/$prev...$tag"
fi
