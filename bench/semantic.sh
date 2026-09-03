#!/bin/sh
# Score devscout's `uses-member` edges against the scout-semantic compiler oracle,
# for one corpus solution.
#
# Usage: bench/semantic.sh <corpus-dir> <solution.sln> [msbuild -p:... args]
#   e.g. bench/semantic.sh bench/corpora/csharp MassTransit.sln -p:TargetFrameworks=net9.0
#
# Restores <corpus-dir>/<solution.sln>, runs tools/scout-semantic over it for the oracle's
# refs.jsonl/units.jsonl, indexes the corpus with devscout, then runs `audit --semantic`
# twice (text and --json). Any trailing [-p:... args] are passed through to both the
# restore and the oracle, so a multi-target solution can be pinned the same way on both.
#
# Uses SCOUT_REGISTRY/SCOUT_CONTENT_DB under bench/state/, isolated from the real registry
# and content store, matching every other script in bench/. Requires `cargo build --release`
# and the corpus already present (see bench/clone-corpus.sh) run first.
#
# refs.jsonl is derived from a corpus that may not be publishable; like every other corpus
# artifact under bench/out/, it is never committed.

set -eu

corpus="${1:-}"
sln="${2:-}"

if [ -z "$corpus" ] || [ -z "$sln" ]; then
	echo "usage: $0 <corpus-dir> <solution.sln> [msbuild -p:... args]" >&2
	exit 2
fi
shift 2

name="$(basename "$sln" .sln)"
out="bench/out/semantic/$name"
mkdir -p "$out" bench/state

export SCOUT_REGISTRY="$PWD/bench/state/repos.json"
export SCOUT_CONTENT_DB="$PWD/bench/state/content.db"

dotnet restore "$corpus/$sln" "$@"

dotnet run --project tools/scout-semantic -c Release -- \
	"$corpus/$sln" --root "$corpus" \
	--out "$out/refs.jsonl" --units "$out/units.jsonl" "$@"

./target/release/devscout -C "$corpus" map .

./target/release/devscout -C "$corpus" audit --semantic "$PWD/$out/refs.jsonl" --units "$PWD/$out/units.jsonl" \
	>"$out/audit.txt"
./target/release/devscout -C "$corpus" audit --semantic "$PWD/$out/refs.jsonl" --units "$PWD/$out/units.jsonl" --json \
	>"$out/audit.json"

echo "$name audited: $out/{refs.jsonl,units.jsonl,audit.txt,audit.json}"
