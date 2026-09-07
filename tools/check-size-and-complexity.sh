#!/bin/sh
# Enforces the ratchet in tools/size-ratchet.toml: a flat line ceiling for
# every file under src/ not named there, a per-file ceiling for each file
# that is, and a cap on how many too_many_lines / cognitive_complexity
# clippy exemptions the tree may carry. CI runs this on every push.
set -eu

cd "$(dirname "$0")/.."

ratchet="tools/size-ratchet.toml"
flat_limit=800
fail=0

get_allow_ceiling() {
  grep -E "^$1 = [0-9]+\$" "$ratchet" | sed -E "s/^$1 = ([0-9]+)\$/\1/"
}

too_many_lines_ceiling=$(get_allow_ceiling too_many_lines)
cognitive_complexity_ceiling=$(get_allow_ceiling cognitive_complexity)

too_many_lines_count=$(grep -rho 'clippy::too_many_lines' src --include='*.rs' | wc -l | tr -d ' ')
cognitive_complexity_count=$(grep -rho 'clippy::cognitive_complexity' src --include='*.rs' | wc -l | tr -d ' ')

if [ "$too_many_lines_count" -gt "$too_many_lines_ceiling" ]; then
  echo "check-size-and-complexity: #[allow(clippy::too_many_lines)] count $too_many_lines_count exceeds ratchet ceiling $too_many_lines_ceiling"
  fail=1
fi

if [ "$cognitive_complexity_count" -gt "$cognitive_complexity_ceiling" ]; then
  echo "check-size-and-complexity: #[allow(clippy::cognitive_complexity)] count $cognitive_complexity_count exceeds ratchet ceiling $cognitive_complexity_ceiling"
  fail=1
fi

# Allowlisted files: each is capped at its own recorded ceiling, and the
# ratchet must not name a file that no longer exists.
allowlisted_files=$(grep -E '^"src/[^"]+" = [0-9]+$' "$ratchet" | sed -E 's/^"([^"]+)" = ([0-9]+)$/\1 \2/')

listed_paths=""
old_ifs=$IFS
IFS='
'
for row in $allowlisted_files; do
  file=$(echo "$row" | cut -d' ' -f1)
  ceiling=$(echo "$row" | cut -d' ' -f2)
  listed_paths="$listed_paths $file"

  if [ ! -f "$file" ]; then
    echo "check-size-and-complexity: ratchet names $file but no such file exists"
    fail=1
    continue
  fi

  lines=$(wc -l < "$file" | tr -d ' ')
  if [ "$lines" -gt "$ceiling" ]; then
    echo "check-size-and-complexity: $file has $lines lines, exceeds its ratchet ceiling of $ceiling"
    fail=1
  fi
done
IFS=$old_ifs

# Every other file under src/ is capped by the flat limit.
for file in $(find src -name '*.rs' | sort); do
  case " $listed_paths " in
    *" $file "*) continue ;;
  esac

  lines=$(wc -l < "$file" | tr -d ' ')
  if [ "$lines" -gt "$flat_limit" ]; then
    echo "check-size-and-complexity: $file has $lines lines, exceeds the flat limit of $flat_limit (add it to tools/size-ratchet.toml only if splitting it further is not the answer)"
    fail=1
  fi
done

if [ "$fail" -eq 0 ]; then
  echo "check-size-and-complexity: file sizes and clippy exemption counts are within the ratchet"
fi
exit "$fail"
