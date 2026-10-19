#!/bin/sh
# The canonical comment-hygiene hook deliberately never reads .md -- see its
# own module docstring ("scan mode never looks at prose"). That leaves one
# class of leak specific to prose docs uncaught: an internal roadmap tracker id
# (two to five letters, a hyphen, three or four digits) or an acceptance-check
# label (`AC-<number>`) pasted into a public results/design write-up. Teaching
# the hook to cover .md would require bumping its version/sha and re-syncing
# every repo that vendors a byte-identical copy of it -- out of scope for a
# single-repo doc fix -- so this stays a separate, narrowly-scoped grep gate
# covering exactly those two patterns and nothing else (no private-heading or
# governance-vocabulary check here: those need editorial judgment, not a
# regex, and a broad prose grep on this repo's own pre-existing benchmark lane
# labels would false-positive -- the 3-4-digit requirement below is why those
# two-digit labels never match).
#
# The scan set is every tracked lowercase `.md` path, at the root or any depth,
# so root prose such as README.md, ROADMAP.md and CHANGELOG.md is covered too.
# It comes from the index, so an untracked file never changes the result.
#
# Enumeration and reading both fail closed: CI consumes this exit status, so a
# tree Git cannot enumerate, a tracked file that cannot be read, and a grep
# that fails for any reason other than "no match" all exit nonzero instead of
# reporting a clean scan.
set -eu

self_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$self_dir/.."
self="$self_dir/$(basename -- "$0")"

tracker_id_pattern='\b([A-Z]{2,5})-[0-9]{3,4}\b'
acceptance_label_pattern='\bAC-[0-9]{1,3}\b'

# Print one `<path>: <label>: <line>` per match of one pattern in one file, and
# return nonzero for a match or for a scan error. grep's exit status separates
# them: 1 is "no match", anything above it is a failure the gate must not
# swallow.
report_matches() {
  file=$1
  label=$2
  pattern=$3
  status=0
  matches=$(grep -nE "$pattern" -- "$file") || status=$?
  case "$status" in
    0)
      printf '%s\n' "$matches" | while IFS= read -r hit; do
        printf '%s: %s: %s\n' "$file" "$label" "$hit"
      done
      return 1
      ;;
    1) return 0 ;;
    *)
      printf 'check-docs-hygiene: cannot scan %s (%s, grep exit %s)\n' "$file" "$label" "$status" >&2
      return 1
      ;;
  esac
}

# The scanning half, run once per index batch with every path as a positional
# parameter: a file name is therefore never word-split and never read as an
# option, whatever whitespace or leading dash it carries. The `.md` filter is a
# shell pattern rather than a Git pathspec, so it stays exactly lowercase even
# where Git is configured case-insensitive.
scan_files() {
  fail=0
  for file in "$@"; do
    case "$file" in
      *.md) ;;
      *) continue ;;
    esac
    if [ ! -f "$file" ] || [ ! -r "$file" ]; then
      printf 'check-docs-hygiene: tracked file is missing or unreadable: %s\n' "$file" >&2
      fail=1
      continue
    fi
    report_matches "$file" "tracker id" "$tracker_id_pattern" || fail=1
    report_matches "$file" "acceptance-check label" "$acceptance_label_pattern" || fail=1
  done
  return "$fail"
}

if [ "${1:-}" = "--scan-tracked" ]; then
  shift
  if scan_files "$@"; then
    exit 0
  fi
  exit 1
fi

tmp=$(mktemp -d) || {
  printf 'check-docs-hygiene: cannot create a temporary directory\n' >&2
  exit 1
}
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

fail=0
path_list="$tmp/tracked-paths"

if git ls-files -z > "$path_list"; then
  :
else
  printf 'check-docs-hygiene: cannot enumerate tracked files (is this a Git checkout?)\n' >&2
  fail=1
fi

# -0 keeps every byte of a path intact, including newlines; a batch large
# enough to overflow the argument limit is split by xargs, and any batch that
# exits nonzero makes xargs do the same.
if [ "$fail" -eq 0 ]; then
  if xargs -0 sh "$self" --scan-tracked < "$path_list"; then
    :
  else
    fail=1
  fi
fi

if [ "$fail" -eq 0 ]; then
  printf 'check-docs-hygiene: no tracker ids or acceptance-check labels in tracked Markdown\n'
fi

exit "$fail"
