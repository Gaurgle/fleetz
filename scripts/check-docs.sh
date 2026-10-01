#!/usr/bin/env bash
# Mechanical checks on the tracked Markdown, run by .github/workflows/docs.yml
# and runnable locally. Only questions with a right answer live here: whether a
# doc is stale or contradicts another is a judgment call, left to review.
# Usage: scripts/check-docs.sh
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
failed=0

fail() {
  echo "$1"
  failed=1
}

# House rule: no em dashes (U+2014) or en dashes (U+2013). Matched by their
# UTF-8 bytes so this file does not hold the characters it forbids.
# docs/superpowers/ is exempt: dated records of landed work, kept as written.
dashes="$(git ls-files -z -- '*.md' ':(exclude)docs/superpowers' \
  | xargs -0 grep -Hn -e $'\xe2\x80\x93' -e $'\xe2\x80\x94' || true)"
[ -z "$dashes" ] || fail "em or en dash found:"$'\n'"$dashes"

# A relative link must resolve from the file that holds it. Web links, mail
# links and same-page anchors are not checked.
while IFS= read -r file; do
  dir="$(dirname "$file")"
  while IFS= read -r target; do
    target="${target%%#*}"
    [ -n "$target" ] || continue
    [ -e "$dir/$target" ] || fail "$file: broken link to $target"
  done < <(grep -oE '\]\([^) ]+\)' "$file" \
    | sed -E 's/^\]\((.*)\)$/\1/' \
    | grep -vE '^(https?:|mailto:|#)' || true)
done < <(git ls-files -- '*.md')

# Every path the CLAUDE.md index names must exist, so the map cannot outlive
# the files it points at. An empty list means the section was renamed and this
# check is reading nothing, which must not pass quietly.
indexed="$(awk '/^## Where things are written down/ { on = 1; next }
  /^## / { on = 0 }
  on && /^\| `/' CLAUDE.md | sed -E 's/^\| `([^`]+)`.*/\1/')"
[ -n "$indexed" ] || fail "CLAUDE.md: no paths found under 'Where things are written down'"
while IFS= read -r path; do
  [ -z "$path" ] || [ -e "$path" ] || fail "CLAUDE.md: indexed path $path does not exist"
done <<< "$indexed"

exit "$failed"
