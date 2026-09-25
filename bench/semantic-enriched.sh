#!/bin/sh
# Paired syntax-lane / enriched-lane audit for one corpus solution: runs
# bench/semantic.sh (unmodified) for the syntax lane, then admits a
# compiler-facts artifact from the SAME corpus checkout and audits the
# resulting enriched graph against the SAME oracle output -- the "same-run
# pairing" the enriched audit fixture's own expected-enriched assertions
# depend on, applied here to a real corpus instead of a small fixture.
#
# Usage: bench/semantic-enriched.sh <corpus-dir> <solution.sln> [msbuild -p:... args]
#   e.g. bench/semantic-enriched.sh bench/clones/csharp MassTransit.sln -p:TargetFrameworks=net9.0
#
# Requires `cargo build --release`, the corpus already cloned (see
# bench/clone-corpus.sh) at a pinned commit with a real git history (NOT
# `--no-git` for the corpus itself -- the compiler-facts run below needs a
# genuine `sourceSnapshot.headSha` to admit against), and the .NET SDK
# `compiler-facts import`'s default profile expects (net9.0 / Debug / AnyCPU;
# override with the same [-p:... args] this script forwards).
#
# Registers no prediction of its own -- this script only reproduces the
# commands; the gate's own four criteria and their evaluation belong in a
# dated docs/benchmarks/results/ entry, written by whoever runs this.

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
out_enriched="bench/out/semantic-enriched/$name"
out_facts="bench/out/compiler-facts"
mkdir -p "$out" "$out_enriched" "$out_facts" bench/state

export SCOUT_REGISTRY="$PWD/bench/state/repos.json"
export SCOUT_CONTENT_DB="$PWD/bench/state/content.db"

# The syntax lane, unmodified -- also produces the oracle's own
# refs.jsonl/units.jsonl the enriched-lane audit below reuses, unchanged, per
# the pairing rule.
sh "$(dirname "$0")/semantic.sh" "$corpus" "$sln" "$@"

# The SAME oracle, in compiler-facts mode, against the SAME corpus checkout --
# never a second, independent walk, and never against a different commit than
# the one the syntax lane's own refs.jsonl was captured from.
dotnet run --project tools/scout-semantic -c Release -- \
	"$corpus/$sln" --root "$corpus" \
	--emit compiler-facts --compiler-facts "$out_facts/$name-compiler-facts.json" \
	--tfm net9.0 -p Configuration=Debug -p Platform=AnyCPU "$@"

./target/release/devscout -C "$corpus" compiler-facts import "$PWD/$out_facts/$name-compiler-facts.json"
./target/release/devscout -C "$corpus" map .

./target/release/devscout -C "$corpus" audit --semantic "$PWD/$out/refs.jsonl" --units "$PWD/$out/units.jsonl" \
	>"$out_enriched/audit.txt"
./target/release/devscout -C "$corpus" audit --semantic "$PWD/$out/refs.jsonl" --units "$PWD/$out/units.jsonl" --json \
	>"$out_enriched/audit.json"

echo "$name audited (both lanes): $out/{refs.jsonl,units.jsonl,audit.txt,audit.json}, $out_enriched/{audit.txt,audit.json}"
