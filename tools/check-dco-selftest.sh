#!/bin/sh
# Proves tools/check-dco.sh and the checker it calls behave the way the DCO
# gate claims, by running the real wrapper against isolated temporary Git
# histories. Each scenario builds the smallest history that shows one
# outcome, then asserts the wrapper's exit status. Nothing is stubbed: the
# temporary tree holds its own copy of the wrapper and of the checker, so the
# wrapper's own merge-base range selection and the checker's trailer parsing
# are the real code under test.
set -eu

cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

mkdir -p "$work/.claude/hooks" "$work/tools"
cp .claude/hooks/comment-hygiene.py "$work/.claude/hooks/comment-hygiene.py"
cp tools/check-dco.sh "$work/tools/check-dco.sh"

git -C "$work" init -q
git -C "$work" config user.name "Selftest Author"
git -C "$work" config user.email "selftest@example.com"
git -C "$work" config commit.gpgsign false

fail=0

run_wrapper() {
  ( cd "$work" && sh tools/check-dco.sh "$1" "$2" )
}

check_pass() {
  label=$1
  base=$2
  head=$3
  if run_wrapper "$base" "$head" >/dev/null 2>&1; then
    echo "check-dco-selftest: ok: $label"
  else
    echo "check-dco-selftest: FAIL: $label (the range was rejected, expected it to pass)" >&2
    fail=1
  fi
}

check_fail() {
  label=$1
  base=$2
  head=$3
  if run_wrapper "$base" "$head" >/dev/null 2>&1; then
    echo "check-dco-selftest: FAIL: $label (the range passed, expected it to be rejected)" >&2
    fail=1
  else
    echo "check-dco-selftest: ok: $label"
  fi
}

sha() {
  git -C "$work" rev-parse HEAD
}

# The target base tip: an unsigned commit already on the target branch, so
# the first scenarios also show that pre-existing target history stays
# outside the checked range.
git -C "$work" commit -q --allow-empty -m "Baseline change with no sign-off"
base=$(sha)

git -C "$work" checkout -q -b pr-signed "$base"
git -C "$work" commit -q --allow-empty -m "Add a signed change" \
  -m "Signed-off-by: Selftest Author <selftest@example.com>"
signed=$(sha)

git -C "$work" checkout -q -b pr-unsigned "$base"
git -C "$work" commit -q --allow-empty -m "Add an unsigned change"
unsigned=$(sha)

git -C "$work" checkout -q -b pr-malformed "$base"
git -C "$work" commit -q --allow-empty -m "Add a change with a lookalike trailer" \
  -m "Signed-off-by: nobody"
malformed=$(sha)

git -C "$work" checkout -q -b pr-note "$base"
git -C "$work" commit -q --allow-empty -m "Add a change whose sign-off is not the footer" \
  -m "Signed-off-by: Selftest Author <selftest@example.com>" \
  -m "A trailing paragraph after the trailer."
note=$(sha)

git -C "$work" checkout -q -b pr-chain "$base"
git -C "$work" commit -q --allow-empty -m "Unsigned change first"
git -C "$work" commit -q --allow-empty -m "Signed change second" \
  -m "Signed-off-by: Selftest Author <selftest@example.com>"
chain_tip=$(sha)

git -C "$work" checkout -q -b pr-both "$base"
git -C "$work" commit -q --allow-empty -m "First signed change" \
  -m "Signed-off-by: Selftest Author <selftest@example.com>"
git -C "$work" commit -q --allow-empty -m "Second signed change" \
  -m "Signed-off-by: Selftest Author <selftest@example.com>"
both_tip=$(sha)

# A target that moved on after the pull request forked, plus the temporary
# merge commit GitHub builds for a `pull_request` run. The checkout ends on
# that synthetic merge so the last assertion runs with HEAD pointing at it.
git -C "$work" checkout -q -b target-advanced "$base"
git -C "$work" commit -q --allow-empty -m "Target branch moves ahead with no sign-off"
target_tip=$(sha)

git -C "$work" checkout -q -b pr-merged "$base"
git -C "$work" commit -q --allow-empty -m "Add a signed change on the pull request" \
  -m "Signed-off-by: Selftest Author <selftest@example.com>"
merged_head=$(sha)

git -C "$work" checkout -q target-advanced
git -C "$work" merge -q --no-ff -m "Merge pull request #1 from fork/pr-merged" pr-merged

check_pass "a signed head with an unsigned target base" "$base" "$signed"
check_fail "an unsigned head commit" "$base" "$unsigned"
check_fail "a lookalike trailer on the head commit" "$base" "$malformed"
check_fail "a sign-off that is not the final paragraph" "$base" "$note"
check_fail "an unsigned commit hidden behind a signed tip" "$base" "$chain_tip"
check_pass "two signed commits" "$base" "$both_tip"
check_pass "unsigned target history and the synthetic merge excluded" "$target_tip" "$merged_head"

check_fail "a head id that is not in the checkout" "$base" "0123456789abcdef0123456789abcdef01234567"
check_fail "a range that selects no commits" "$signed" "$signed"
check_fail "a branch-name argument is refused before Git runs" "main..dev" "$signed"
check_fail "an option-like argument is refused before Git runs" "--all" "$signed"
check_fail "an empty base id is refused before Git runs" "" "$signed"

if ( cd "$work" && sh tools/check-dco.sh "$base" ) >/dev/null 2>&1; then
  echo "check-dco-selftest: FAIL: a one-argument call was accepted" >&2
  fail=1
else
  echo "check-dco-selftest: ok: a one-argument call is refused"
fi

if ( cd "$work" && python3 .claude/hooks/comment-hygiene.py --require-signoff "$signed..$signed" ) >/dev/null 2>&1; then
  echo "check-dco-selftest: FAIL: the checker accepted an empty range" >&2
  fail=1
else
  echo "check-dco-selftest: ok: the checker refuses an empty range"
fi

if [ "$fail" -eq 0 ]; then
  echo "check-dco-selftest: all cases behaved as expected"
fi
exit "$fail"
