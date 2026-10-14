#!/bin/sh
# Shows tools/check-docs-hygiene.sh really covers the tracked Markdown set it
# claims: which paths it scans, that an untracked file never changes the
# result, that each pattern family fails with a path and a line, that near
# misses pass, and that neither an unreadable tracked file nor a tree Git
# cannot enumerate can report a clean scan.
#
# Every case runs against a throwaway Git repository under a temporary
# directory, so nothing here depends on the checkout's own index. Staging
# alone defines the scan set, so the fixture needs no commit.
set -eu

cd "$(dirname "$0")/.."

live_gate="$PWD/tools/check-docs-hygiene.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM

repo="$work/repo"
mkdir -p "$repo/tools" "$repo/nested/deep" "$repo/dir with space"
cp "$live_gate" "$repo/tools/check-docs-hygiene.sh"
git -C "$repo" init -q

clean_prose='Clean prose with neither a tracker id nor an acceptance label.'
printf '%s\n' "$clean_prose" > "$repo/README.md"
printf '%s\n' "$clean_prose" > "$repo/nested/deep/notes.md"
printf '%s\n' "$clean_prose" > "$repo/dir with space/note.md"
printf 'Near misses: a two-digit suffix ZZ-12, a lowercase prefix zz-123, a single-letter prefix A-123, and an oversized acceptance number AC-12345.\n' > "$repo/near-miss.md"

# Deliberately left out of the index: a positive here must change nothing.
printf 'An untracked file naming ZZ-456.\n' > "$repo/untracked.md"

git -C "$repo" add README.md nested/deep/notes.md 'dir with space/note.md' near-miss.md

success_line='no tracker ids or acceptance-check labels in tracked Markdown'
fail=0

gate_status=0
gate_output=''

run_gate() {
  gate_status=0
  gate_output=$(sh "$1" 2>&1) || gate_status=$?
}

# Assert one case's exit status, and that only a clean scan prints the success
# line -- a failure path that still reports success is exactly what this
# self-test exists to catch.
assert() {
  label=$1
  want=$2
  if [ "$want" = pass ] && [ "$gate_status" -ne 0 ]; then
    printf 'check-docs-hygiene-selftest: %s: expected a clean scan, got exit %s\n%s\n' "$label" "$gate_status" "$gate_output"
    fail=1
    return
  fi
  if [ "$want" = fail ] && [ "$gate_status" -eq 0 ]; then
    printf 'check-docs-hygiene-selftest: %s: expected a nonzero exit, got 0\n' "$label"
    fail=1
    return
  fi
  case "$gate_output" in
    *"$success_line"*)
      if [ "$want" = fail ]; then
        printf 'check-docs-hygiene-selftest: %s: a failing scan still reported success\n' "$label"
        fail=1
        return
      fi
      ;;
    *)
      if [ "$want" = pass ]; then
        printf 'check-docs-hygiene-selftest: %s: a clean scan printed no success line\n' "$label"
        fail=1
        return
      fi
      ;;
  esac
  printf 'check-docs-hygiene-selftest: %s ok\n' "$label"
}

assert_contains() {
  label=$1
  needle=$2
  case "$gate_output" in
    *"$needle"*) ;;
    *)
      printf 'check-docs-hygiene-selftest: %s: expected output to contain %s, got:\n%s\n' "$label" "$needle" "$gate_output"
      fail=1
      ;;
  esac
}

assert_absent() {
  label=$1
  needle=$2
  case "$gate_output" in
    *"$needle"*)
      printf 'check-docs-hygiene-selftest: %s: output should not contain %s, got:\n%s\n' "$label" "$needle" "$gate_output"
      fail=1
      ;;
  esac
}

# Root, nested and whitespace paths staged cleanly, with an untracked positive
# present: the gate passes and says nothing about the untracked file.
run_gate "$repo/tools/check-docs-hygiene.sh"
assert "clean tracked set with an untracked positive" pass
assert_absent "clean tracked set with an untracked positive" 'untracked.md'

# A staged tracker id in root prose.
printf 'Root prose naming ZZ-123 in passing.\n' > "$repo/tracker-positive.md"
git -C "$repo" add tracker-positive.md
run_gate "$repo/tools/check-docs-hygiene.sh"
assert "tracker id in a tracked file" fail
assert_contains "tracker id in a tracked file" 'tracker-positive.md: tracker id: 1:'

# A staged acceptance-check label.
printf 'Prose quoting AC-12 as an acceptance label.\n' > "$repo/label-positive.md"
git -C "$repo" add label-positive.md
run_gate "$repo/tools/check-docs-hygiene.sh"
assert "acceptance label in a tracked file" fail
assert_contains "acceptance label in a tracked file" 'label-positive.md: acceptance-check label: 1:'

# A hit three lines into a path containing a space: the path must be reported
# whole, on its own line, with the right line number -- a scan that
# word-splits the path cannot produce this.
printf 'First line is clean.\nSecond line is clean too.\nThird line names ZZ-900.\n' > "$repo/dir with space/hit.md"
git -C "$repo" add 'dir with space/hit.md'
run_gate "$repo/tools/check-docs-hygiene.sh"
assert "hit inside a whitespace-bearing path" fail
assert_contains "hit inside a whitespace-bearing path" 'dir with space/hit.md: tracker id: 3:'

# A path that begins with a dash: it must reach the scan as a name, never as an
# option to grep or to the test builtin.
printf 'Leading-dash path.\nSecond line names ZZ-321.\n' > "$repo/-dash.md"
git -C "$repo" add -- '-dash.md'
run_gate "$repo/tools/check-docs-hygiene.sh"
assert "hit inside a leading-dash path" fail
assert_contains "hit inside a leading-dash path" '-dash.md: tracker id: 2:'

# Paths whose bytes Git's own listing output would otherwise escape or treat as
# a pattern: a newline, and the glob characters Git pathspecs also use. Both
# must arrive at the scan untouched and be reported with the right line.
newline_name='new
line.md'
printf 'First line is clean.\nSecond line names ZZ-777.\n' > "$repo/$newline_name"
git -C "$repo" add -- "$newline_name"
run_gate "$repo/tools/check-docs-hygiene.sh"
assert "hit inside a path containing a newline" fail
assert_contains "hit inside a path containing a newline" 'line.md: tracker id: 2:'

printf 'Glob characters in a path.\nSecond line names ZZ-888.\n' > "$repo/star*.md"
git -C "$repo" add -- 'star*.md'
run_gate "$repo/tools/check-docs-hygiene.sh"
assert "hit inside a path containing glob characters" fail
assert_contains "hit inside a path containing glob characters" 'star*.md: tracker id: 2:'

# A path the index holds but the worktree does not: unreadable tracked content
# must not pass as clean.
printf 'Staged, then removed from the worktree.\n' > "$repo/missing.md"
git -C "$repo" add missing.md
rm "$repo/missing.md"
run_gate "$repo/tools/check-docs-hygiene.sh"
assert "missing tracked file" fail
assert_contains "missing tracked file" 'tracked file is missing or unreadable: missing.md'

# The same gate in a directory tree Git cannot enumerate.
mkdir -p "$work/not-a-repo/tools"
cp "$live_gate" "$work/not-a-repo/tools/check-docs-hygiene.sh"
run_gate "$work/not-a-repo/tools/check-docs-hygiene.sh"
assert "tree Git cannot enumerate" fail
assert_contains "tree Git cannot enumerate" 'cannot enumerate tracked files'

if [ "$fail" -eq 0 ]; then
  printf 'check-docs-hygiene-selftest: all cases behaved as expected\n'
fi
exit "$fail"
