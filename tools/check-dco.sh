#!/bin/sh
# DCO range boundary for pull requests: prove that every commit a pull
# request adds on top of its target base carries a conventional
# Signed-off-by trailer.
#
# The range is computed from the two commit ids a pull request event
# supplies -- the target base tip and the submitted head -- so the check
# reads the submitted commits themselves. It never inspects the checked-out
# HEAD, because a `pull_request` run normally checks out the temporary merge
# commit GitHub builds to test the merge; that synthetic commit belongs to
# neither the base nor the head and must not be charged to the contributor.
# History the target branch already held before the pull request is excluded
# the same way, by taking the merge base as the floor.
#
# Both ids arrive as data. A branch name, a ref expression or an option-like
# token is refused before it reaches Git, so no argument is ever evaluated as
# shell code and the submitted head is never inferred from the checkout.
#
# Usage: sh tools/check-dco.sh <base-sha> <head-sha>
# Exit 0 when the submitted range is non-empty and every commit in it is
# signed off; 1 on a missing sign-off or an unusable range; 2 on a bad call.
set -eu

cd "$(dirname "$0")/.."

if [ "$#" -ne 2 ]; then
  echo "usage: check-dco.sh <base-sha> <head-sha>" >&2
  exit 2
fi

base=$1
head=$2

if [ -z "$base" ] || [ -z "$head" ]; then
  echo "check-dco: base and head must be non-empty commit ids" >&2
  exit 2
fi

# Accept only hexadecimal commit ids. An empty value, a branch name such as
# `main..dev`, an option such as `--all`, or any path/glob character is
# rejected here rather than handed to Git as a revision expression.
case "$base" in
  *[!0-9a-fA-F]*)
    echo "check-dco: base '$base' is not a hexadecimal commit id" >&2
    exit 2
    ;;
esac
case "$head" in
  *[!0-9a-fA-F]*)
    echo "check-dco: head '$head' is not a hexadecimal commit id" >&2
    exit 2
    ;;
esac

if ! base_commit=$(git rev-parse --verify --quiet "${base}^{commit}"); then
  echo "check-dco: base '$base' does not resolve to a commit in this checkout" >&2
  exit 1
fi
if ! head_commit=$(git rev-parse --verify --quiet "${head}^{commit}"); then
  echo "check-dco: head '$head' does not resolve to a commit in this checkout" >&2
  exit 1
fi

if ! merge_base=$(git merge-base "$base_commit" "$head_commit"); then
  echo "check-dco: no merge base between $base_commit and $head_commit" >&2
  exit 1
fi

submitted=$(git rev-list --count "${merge_base}..${head_commit}")
if [ "$submitted" -eq 0 ]; then
  echo "check-dco: ${merge_base}..${head_commit} adds no submitted commits" >&2
  exit 1
fi

echo "check-dco: checking $submitted submitted commit(s) in ${merge_base}..${head_commit}"
exec python3 .claude/hooks/comment-hygiene.py --require-signoff "${merge_base}..${head_commit}"
