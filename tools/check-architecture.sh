#!/bin/sh
# ARCHITECTURE.md must carry one row per Rust module under src/, and no row
# for a module that no longer exists. CI runs this on every push.
set -eu

cd "$(dirname "$0")/.."

fail=0

for module in $(find src -name '*.rs' | sort); do
  if ! grep -qF "| \`$module\` |" ARCHITECTURE.md; then
    echo "ARCHITECTURE.md: no row for $module"; fail=1
  fi
done

for listed in $(grep -oE '^\| `src/[^`]+\.rs` \|' ARCHITECTURE.md | sed -E 's/^\| `([^`]+)` \|$/\1/'); do
  if [ ! -f "$listed" ]; then
    echo "ARCHITECTURE.md: row for $listed but no such file"; fail=1
  fi
done

if [ "$fail" -eq 0 ]; then
  echo "check-architecture: every module under src/ has a row"
fi
exit "$fail"
