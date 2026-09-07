#!/bin/sh
# Proves tools/check-size-and-complexity.sh actually catches what it claims
# to catch, by running it against several deliberately mutated copies of the
# tree. The 101-line-function half of the gate (too_many_lines is `deny` at
# threshold 100 in clippy.toml) is exercised by clippy itself, not here --
# this script only covers the ratchet counter and the file-size ceilings.
set -eu

cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cp -R src "$work/src"
mkdir -p "$work/tools"
cp tools/size-ratchet.toml "$work/tools/size-ratchet.toml"
cp tools/check-size-and-complexity.sh "$work/tools/check-size-and-complexity.sh"

run_checker() {
  (cd "$work" && sh tools/check-size-and-complexity.sh)
}

fail=0

# A new file over the flat line limit, outside the allowlist, must fail the
# checker (case A).
awk 'BEGIN { for (i = 0; i < 801; i++) print "// filler line " i }' > "$work/src/oversize_case_a.rs"
if run_checker >/dev/null 2>&1; then
  echo "check-size-and-complexity-selftest: case A did not fail (an 801-line non-allowlisted file was accepted)"
  fail=1
else
  echo "check-size-and-complexity-selftest: case A ok (oversized non-allowlisted file rejected)"
fi
rm -f "$work/src/oversize_case_a.rs"

# Pushing the too_many_lines exemption count past the ratchet's ceiling must
# fail the checker, independent of any single function's length (case B).
{
  echo ""
  echo "#[allow(clippy::too_many_lines, reason = \"selftest marker: pushes the exemption count over the ratchet ceiling\")]"
  echo "fn selftest_case_b_marker() {}"
} >> "$work/src/lib.rs"
if run_checker >/dev/null 2>&1; then
  echo "check-size-and-complexity-selftest: case B did not fail (an extra too_many_lines allow was accepted)"
  fail=1
else
  echo "check-size-and-complexity-selftest: case B ok (exemption count over ceiling rejected)"
fi

# Restore lib.rs before the clean-copy run below.
cp src/lib.rs "$work/src/lib.rs"

# The unmodified copy must pass cleanly (case C).
if run_checker >/dev/null 2>&1; then
  echo "check-size-and-complexity-selftest: case C ok (unmodified tree accepted)"
else
  echo "check-size-and-complexity-selftest: case C did not pass (the unmodified tree was rejected)"
  fail=1
fi

# A `//` comment that merely names the lint is not an exemption and must not
# be counted toward the ratchet ceiling -- the unmodified tree already sits
# exactly at the too_many_lines ceiling, so counting comment text as an
# attribute would tip it over (case D).
{
  echo ""
  echo "// This function is getting close to the clippy::too_many_lines limit;"
  echo "// splitting it clean of a clippy::too_many_lines allow is the goal."
  echo "fn selftest_case_d_marker() {}"
} >> "$work/src/lib.rs"
if run_checker >/dev/null 2>&1; then
  echo "check-size-and-complexity-selftest: case D ok (a comment naming the lint was not counted as an exemption)"
else
  echo "check-size-and-complexity-selftest: case D did not pass (a comment naming clippy::too_many_lines was miscounted as an exemption)"
  fail=1
fi

if [ "$fail" -eq 0 ]; then
  echo "check-size-and-complexity-selftest: all cases behaved as expected"
fi
exit "$fail"
