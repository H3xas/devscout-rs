#!/bin/sh
# Packaging and cost measurement for the optional compiler-facts engine:
# default-CLI size, framework-dependent and self-contained engine publish
# sizes, cold/warm compiler-facts wall time, peak engine memory, and
# artifact import/admission cost. Runs entirely against the pinned
# fixtures under fixtures/, never a corpus clone -- unlike bench/semantic.sh,
# nothing here needs bench/clone-corpus.sh.
#
# Usage: bench/compiler-facts-cost.sh
#
# Requires `cargo build --release` and `dotnet build tools/scout-semantic
# -c Release` already run. Everything below is offline once those two
# builds and the one `dotnet publish --self-contained` restore have
# completed; publish output and timing logs land under bench/out/, never
# committed. Uses SCOUT_REGISTRY under bench/state/, isolated from the real
# registry, matching every other script in this directory.

set -eu

root="$PWD"
out="bench/out/compiler-facts-cost"
mkdir -p "$out" bench/state
export SCOUT_REGISTRY="$root/bench/state/compiler-facts-cost-repos.json"

rid="${SCOUT_BENCH_RID:-}"
if [ -z "$rid" ]; then
	case "$(uname -s)-$(uname -m)" in
		Darwin-arm64) rid="osx-arm64" ;;
		Darwin-x86_64) rid="osx-x64" ;;
		Linux-x86_64) rid="linux-x64" ;;
		Linux-aarch64) rid="linux-arm64" ;;
		*) rid="linux-x64" ;;
	esac
fi

echo "== toolchain =="
{
	echo "rustc: $(rustc --version)"
	echo "cargo: $(cargo --version)"
	echo "dotnet: $(dotnet --version)"
	# Kernel name/release/machine only -- never the network hostname field
	# `uname -a` would otherwise include.
	echo "uname: $(uname -srm)"
} | tee "$out/environment.txt"

echo "== default CLI size and startup =="
cli_bin="target/release/devscout"
if [ ! -f "$cli_bin" ]; then
	echo "error: $cli_bin missing -- run cargo build --release first" >&2
	exit 1
fi
cli_bytes=$(wc -c <"$cli_bin" | tr -d ' ')
# Cold: first invocation after a fresh build (disk cache may still miss the
# just-linked binary); warm: median of three immediate repeats.
start_t0=$(date +%s.%N)
"$root/$cli_bin" --version >/dev/null
start_t1=$(date +%s.%N)
cli_cold=$(awk "BEGIN { printf \"%.4f\", $start_t1 - $start_t0 }")
w1_t0=$(date +%s.%N); "$root/$cli_bin" --version >/dev/null; w1_t1=$(date +%s.%N)
w2_t0=$(date +%s.%N); "$root/$cli_bin" --version >/dev/null; w2_t1=$(date +%s.%N)
w3_t0=$(date +%s.%N); "$root/$cli_bin" --version >/dev/null; w3_t1=$(date +%s.%N)
cli_warm1=$(awk "BEGIN { printf \"%.4f\", $w1_t1 - $w1_t0 }")
cli_warm2=$(awk "BEGIN { printf \"%.4f\", $w2_t1 - $w2_t0 }")
cli_warm3=$(awk "BEGIN { printf \"%.4f\", $w3_t1 - $w3_t0 }")
echo "default CLI: $cli_bytes bytes, --version cold ${cli_cold}s, warm ${cli_warm1}s ${cli_warm2}s ${cli_warm3}s"

echo "== engine packaging: framework-dependent =="
fdd_dir="$out/publish-fdd"
rm -rf "$fdd_dir"
dotnet publish tools/scout-semantic -c Release -o "$fdd_dir" >"$out/publish-fdd.log" 2>&1
fdd_bytes=$(find "$fdd_dir" -type f -exec wc -c {} + | tail -1 | awk '{print $1}')
echo "framework-dependent publish ($fdd_dir): $fdd_bytes bytes total"

echo "== engine packaging: self-contained ($rid) =="
sc_dir="$out/publish-sc"
rm -rf "$sc_dir"
# -p:NuGetLockFilePath redirects the lock file NuGet resolves/writes to a
# throwaway path under bench/out/: a self-contained publish for a RID the
# committed lock file has never resolved needs extra runtime-specific
# transitive packages, and restoring them here must never mutate the
# committed tools/scout-semantic/packages.lock.json as a side effect of
# running this script.
dotnet publish tools/scout-semantic -c Release -o "$sc_dir" \
	--self-contained true -r "$rid" \
	-p:NuGetLockFilePath="$root/$out/packages.sc.lock.json" >"$out/publish-sc.log" 2>&1
sc_bytes=$(find "$sc_dir" -type f -exec wc -c {} + | tail -1 | awk '{print $1}')
echo "self-contained publish ($sc_dir, $rid): $sc_bytes bytes total"

fixture="fixtures/csharp-compiler-facts/Fixture.csproj"

run_once() {
	target="$1"
	shift
	t0=$(date +%s.%N)
	dotnet run --project tools/scout-semantic --no-build -c Release -- \
		"$fixture" --root fixtures/csharp-compiler-facts \
		--emit compiler-facts --compiler-facts "$target" \
		--tfm net9.0 -p Configuration=Debug -p Platform=AnyCPU \
		"$@" --no-git >"$out/run.log" 2>&1
	t1=$(date +%s.%N)
	awk "BEGIN { printf \"%.3f\", $t1 - $t0 }"
}

# One arm per capability set: "default" is the engine's own unflagged
# default (occurrences included, no --capabilities flag, matching the
# fixture's own committed snapshot's regenerate command); "restricted"
# pins the pre-occurrence capability set explicitly. Each arm reports wall
# time, peak memory, admitted artifact size and import cost as its own
# figure, not only as a delta -- the occurrence walk's added cost is what
# this split exists to isolate.
run_arm() {
	label="$1"
	shift
	echo "== compiler-facts wall time: cold and warm ($label) =="
	cold_out="$out/$label-cold.json"
	warm_out="$out/$label-warm.json"
	cold_time=$(run_once "$cold_out" "$@")
	warm_time=$(run_once "$warm_out" "$@")
	warm_time_2=$(run_once "$warm_out" "$@")
	warm_time_3=$(run_once "$warm_out" "$@")
	echo "$label: cold: ${cold_time}s; warm (3 repeats): ${warm_time}s ${warm_time_2}s ${warm_time_3}s"

	artifact_bytes=$(wc -c <"$cold_out" | tr -d ' ')
	echo "$label: admitted artifact size: ${artifact_bytes} bytes"

	echo "== peak engine memory, cold run ($label) =="
	mem_out="$out/$label-mem.json"
	if command -v /usr/bin/time >/dev/null 2>&1 && /usr/bin/time -l true >/dev/null 2>&1; then
		/usr/bin/time -l dotnet run --project tools/scout-semantic --no-build -c Release -- \
			"$fixture" --root fixtures/csharp-compiler-facts \
			--emit compiler-facts --compiler-facts "$mem_out" \
			--tfm net9.0 -p Configuration=Debug -p Platform=AnyCPU \
			"$@" --no-git >"$out/$label-mem.log" 2>"$out/$label-mem.time" || true
		peak_kb=$(awk '/maximum resident set size/ {print $1}' "$out/$label-mem.time")
		echo "$label: peak RSS (macOS /usr/bin/time -l, bytes): ${peak_kb:-unavailable}"
	elif command -v /usr/bin/time >/dev/null 2>&1 && /usr/bin/time -v true >/dev/null 2>&1; then
		/usr/bin/time -v dotnet run --project tools/scout-semantic --no-build -c Release -- \
			"$fixture" --root fixtures/csharp-compiler-facts \
			--emit compiler-facts --compiler-facts "$mem_out" \
			--tfm net9.0 -p Configuration=Debug -p Platform=AnyCPU \
			"$@" --no-git >"$out/$label-mem.log" 2>"$out/$label-mem.time" || true
		peak_kb=$(awk -F: '/Maximum resident set size/ {gsub(/ /,"",$2); print $2}' "$out/$label-mem.time")
		echo "$label: peak RSS (GNU time -v, KB): ${peak_kb:-unavailable}"
	else
		echo "$label: no /usr/bin/time with -l/-v available; peak memory not measured on this host"
	fi

	echo "== artifact import/admission cost ($label) =="
	# Rooted OUTSIDE this git checkout on purpose, against this arm's own
	# produced artifact (not necessarily the committed fixture): a
	# --no-git-produced artifact carries no sourceSnapshot.headSha, and
	# importing it into a root that IS a git checkout would fail
	# admission's source-snapshot check (this checkout's HEAD never
	# matches "none declared") and time a refusal instead of an admission
	# -- exactly the bug this script's own leftover import.log once
	# proved. A root with no git ancestor at all skips that check the
	# same way a non-git deployment already does, so this measures a
	# real, successful admission.
	import_root=$(mktemp -d "${TMPDIR:-/tmp}/devscout-compiler-facts-bench.XXXXXX")
	# `-C <dir>` is a global flag consumed only immediately after the
	# binary name (git's own convention -- see `apply_global_options`),
	# so it must precede the subcommand here, not follow it, or it is
	# read as an unrecognised argument to `init` and the command falls
	# back to acting on the caller's own cwd instead of the isolated
	# import root.
	"$root/$cli_bin" -C "$import_root" init --no-hooks --no-map >/dev/null
	import_t0=$(date +%s.%N)
	"$root/$cli_bin" -C "$import_root" compiler-facts import "$root/$cold_out" >"$out/$label-import.log" 2>&1
	import_t1=$(date +%s.%N)
	import_time=$(awk "BEGIN { printf \"%.3f\", $import_t1 - $import_t0 }")
	rm -rf "$import_root"
	echo "$label: compiler-facts import (this arm's own cold artifact, admitted outside a git checkout): ${import_time}s"
}

run_arm "default"
run_arm "restricted" --capabilities symbols,diagnostics

echo
echo "Raw figures above; docs/benchmarks/results/2026-09-engine-packaging-cost.md records the"
echo "committed, predictions-registered write-up."
