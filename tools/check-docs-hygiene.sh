#!/bin/sh
# `.claude/hooks/comment-hygiene.py --scan` deliberately never reads .md --
# see its own module docstring ("scan mode never looks at prose"). That
# leaves one class of leak specific to prose docs uncaught: an internal
# roadmap tracker id (two to five letters, a hyphen, three or four digits)
# or an acceptance-check label (`AC-<number>`) pasted into a public
# results/design write-up. Extending the canonical comment-hygiene.py's own
# body to cover .md would require bumping its version/sha and re-syncing
# every other repo that vendors a byte-identical copy of it -- out of scope
# for a single-repo doc fix -- so this is a separate, narrowly-scoped grep
# gate instead, covering exactly those two patterns and nothing else (no
# private-heading or governance-vocabulary check here: those need editorial
# judgment, not a regex, and a broad prose grep on this repo's own
# pre-existing benchmark lane labels would false-positive -- the 3-4-digit
# requirement below is why those two-digit labels never match).
set -eu

cd "$(dirname "$0")/.."

fail=0

for f in $(git ls-files 'docs/*.md' 'docs/**/*.md' 2>/dev/null); do
  if grep -nE '\b([A-Z]{2,5})-[0-9]{3,4}\b' "$f" >/dev/null 2>&1; then
    grep -nE '\b([A-Z]{2,5})-[0-9]{3,4}\b' "$f" | while IFS= read -r hit; do
      echo "$f: tracker id: $hit"
    done
    fail=1
  fi
  if grep -nE '\bAC-[0-9]{1,3}\b' "$f" >/dev/null 2>&1; then
    grep -nE '\bAC-[0-9]{1,3}\b' "$f" | while IFS= read -r hit; do
      echo "$f: acceptance-check label: $hit"
    done
    fail=1
  fi
done

if [ "$fail" -eq 0 ]; then
  echo "check-docs-hygiene: no tracker ids or acceptance-check labels in tracked docs/*.md"
fi
exit "$fail"
