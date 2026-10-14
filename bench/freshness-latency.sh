#!/bin/sh
# What a query pays for certifying a dirty index, measured against the previous
# release in the same state.
#
# Usage: bench/freshness-latency.sh <corpus-clone> [baseline-binary]
#
#   <corpus-clone>    a clone made by bench/clone-corpus.sh (pinned, no remote)
#   [baseline-binary] the binary to compare against; when omitted, a release
#                     build of the v0.9.0 tag is made under bench/out/
#
# Protocol, both states:
#   clean -- map the clone, then measure `refs <seed> --json`
#   dirty -- append one fixed comment line to the first 1,000 `.ts` files by
#            sorted path, map, append a second line to the first 100 of them,
#            then measure the same way
#
# Each arm gets its own state directory (registry and content store) and its own
# copy of the clone, so neither can read the other's index. p95 is the
# nearest-rank 190th of the 200 measured times.
#
# The run fails (non-zero) when the dirty-state p95 is more than 50 ms above the
# baseline's, when the clean-state p95 is more than 10 ms above it, when the
# dirty state does not report `stale`, or when either arm's run fails.

set -eu

seed='createProjectGraphAsync'

if [ "$#" -lt 1 ]; then
	echo "usage: $0 <corpus-clone> [baseline-binary]" >&2
	exit 2
fi

clone="$1"
baseline_arg="${2:-}"

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
work="$repo_root/bench/out/freshness-latency"
rounds=200
warmup=20
dirty_files=1000
second_pass=100
dirty_budget_ms=50
clean_budget_ms=10

if [ ! -d "$clone" ]; then
	echo "no corpus clone at $clone" >&2
	exit 2
fi

rm -rf "$work"
mkdir -p "$work"

echo "building the branch's release binary"
(cd "$repo_root" && cargo build --release --locked >/dev/null)
branch_bin="$repo_root/target/release/devscout"

if [ -n "$baseline_arg" ]; then
	baseline_bin="$baseline_arg"
else
	echo "building the v0.9.0 release binary for the baseline arm"
	mkdir -p "$work/baseline-src"
	(cd "$repo_root" && git archive v0.9.0) | tar -x -C "$work/baseline-src"
	(cd "$work/baseline-src" && cargo build --release --locked >/dev/null)
	baseline_bin="$work/baseline-src/target/release/devscout"
fi

# The first `dirty_files` TypeScript files by sorted path, skipping the
# directories the walk skips. Written to a file so both arms touch exactly the
# same set.
list_dirty() {
	root="$1"
	out="$2"
	find "$root" -type f -name '*.ts' \
		-not -path '*/node_modules/*' -not -path '*/.git/*' -not -path '*/.scout/*' \
		-not -path '*/bin/*' -not -path '*/obj/*' -not -path '*/dist/*' \
		-not -path '*/coverage/*' -not -path '*/.next/*' -not -path '*/target/*' \
		| sed "s|^$root/||" | LC_ALL=C sort | head -n "$dirty_files" >"$out"
}

prepare_arm() {
	arm="$1"
	bin="$2"
	arm_dir="$work/$arm"
	mkdir -p "$arm_dir/state"
	cp -R "$clone" "$arm_dir/repo"
	rm -rf "$arm_dir/repo/.git/scout"
	# `cp -R` gives every copied file a newer mtime than the copied index, so a
	# plain `git status` here both settles that and lets Git write the refreshed
	# index back -- the state a working tree is normally in. Without it, the arm
	# that cannot write the index (the one whose queries pass
	# `--no-optional-locks`) would be timed against a tree nobody would ever
	# query.
	(cd "$arm_dir/repo" && git status --porcelain --untracked-files=all -- . >/dev/null)

	map_arm "$arm" "$bin"
	# The seed must resolve in this corpus, or every run measures a zero-hit
	# answer and the comparison means nothing.
	(cd "$arm_dir/repo" && HOME="$arm_dir/state" \
		SCOUT_REGISTRY="$arm_dir/state/repos.json" \
		SCOUT_CONTENT_DB="$arm_dir/state/content.db" \
		"$bin" refs "$seed" >/dev/null)
}

map_arm() {
	arm="$1"
	bin="$2"
	arm_dir="$work/$arm"
	(cd "$arm_dir/repo" && HOME="$arm_dir/state" \
		SCOUT_REGISTRY="$arm_dir/state/repos.json" \
		SCOUT_CONTENT_DB="$arm_dir/state/content.db" \
		"$bin" map . >/dev/null)
}

measure_pass() {
	arm="$1"
	bin="$2"
	state="$3"
	json="$4"
	arm_dir="$work/$arm"
	(cd "$arm_dir/repo" && hyperfine --warmup "$warmup" --runs "$rounds" \
		--export-json "$json" \
		"env HOME=$arm_dir/state SCOUT_REGISTRY=$arm_dir/state/repos.json SCOUT_CONTENT_DB=$arm_dir/state/content.db $bin refs $seed --json" \
		>/dev/null 2>&1)
	python3 -I - "$json" <<'PY'
import json, math, sys
# One measurement per run; the nearest-rank p95 is the ceil(0.95 * n)-th value.
data = json.load(open(sys.argv[1]))
times = sorted(m["time_wall_clock"]["value"] for m in data["results"][0]["measurements"])
rank = max(1, math.ceil(0.95 * len(times)))
print(f"{times[rank - 1] * 1000:.1f}")
PY
}

# The first pass over a tree that was only just copied reads cold data and
# competes with whatever the copy is still flushing, so each state is measured
# twice and only the second pass is kept.
measure() {
	arm="$1"
	bin="$2"
	state="$3"
	arm_dir="$work/$arm"
	measure_pass "$arm" "$bin" "$state" "$arm_dir/$state.settle.json" >/dev/null
	measure_pass "$arm" "$bin" "$state" "$arm_dir/$state.json"
}

freshness_state() {
	arm="$1"
	bin="$2"
	arm_dir="$work/$arm"
	(cd "$arm_dir/repo" && HOME="$arm_dir/state" \
		SCOUT_REGISTRY="$arm_dir/state/repos.json" \
		SCOUT_CONTENT_DB="$arm_dir/state/content.db" \
		"$bin" refs "$seed" --json) |
		python3 -I -c 'import json,sys; print(json.dumps(json.load(sys.stdin)["freshness"]))'
}

for arm in branch baseline; do
	case "$arm" in
	branch) bin="$branch_bin" ;;
	baseline) bin="$baseline_bin" ;;
	esac
	prepare_arm "$arm" "$bin"
done

# The copies are on disk but not yet quiet; give the filesystem time to finish
# with them before either arm is timed.
sleep 60

branch_clean=$(measure branch "$branch_bin" clean)
baseline_clean=$(measure baseline "$baseline_bin" clean)

dirty_list="$work/dirty-files.txt"
list_dirty "$work/branch/repo" "$dirty_list"
candidate_bytes=$(python3 -I - "$work/branch/repo" "$dirty_list" <<'PY'
import os, sys
root, listing = sys.argv[1], sys.argv[2]
total = 0
for rel in open(listing):
    total += os.path.getsize(os.path.join(root, rel.strip()))
print(total)
PY
)

# Writes one line to each of the first `count` files of the set, in both arms.
append_line() {
	count="$1"
	python3 -I - "$work/branch/repo" "$work/baseline/repo" "$dirty_list" "$count" <<'PY'
import sys
first, second, listing, count = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
line = "// freshness-latency probe\n"
for i, raw in enumerate(open(listing)):
    rel = raw.strip()
    if not rel:
        continue
    for root in (first, second):
        with open(f"{root}/{rel}", "a") as fh:
            fh.write(line)
    if i + 1 >= count:
        break
PY
}

# One line to every file in the set, then the map -- so those bytes are what the
# index certifies -- and only then a second line to the first 100, which is the
# change the query has to notice.
append_line "$dirty_files"
map_arm branch "$branch_bin"
map_arm baseline "$baseline_bin"
append_line "$second_pass"

branch_dirty=$(measure branch "$branch_bin" dirty)
baseline_dirty=$(measure baseline "$baseline_bin" dirty)

dirty_state=$(freshness_state branch "$branch_bin")

python3 -I - "$branch_clean" "$baseline_clean" "$branch_dirty" "$baseline_dirty" \
	"$dirty_state" "$candidate_bytes" "$clean_budget_ms" "$dirty_budget_ms" <<'PY'
import json, sys
branch_clean, baseline_clean, branch_dirty, baseline_dirty = (float(v) for v in sys.argv[1:5])
state = json.loads(sys.argv[5])
bytes_total = int(sys.argv[6])
clean_budget, dirty_budget = float(sys.argv[7]), float(sys.argv[8])

clean_delta = branch_clean - baseline_clean
dirty_delta = branch_dirty - baseline_dirty
print(f"clean  p95: branch {branch_clean:.1f} ms, baseline {baseline_clean:.1f} ms, delta {clean_delta:+.1f} ms")
print(f"dirty  p95: branch {branch_dirty:.1f} ms, baseline {baseline_dirty:.1f} ms, delta {dirty_delta:+.1f} ms")
print(f"dirty-state freshness: {json.dumps(state)}")
print(f"candidate bytes: {bytes_total} ({bytes_total / (1024 * 1024):.2f} MiB)")

failures = []
if state.get("state") != "stale":
    failures.append(f"the dirty state must answer stale, not {state.get('state')!r}")
if dirty_delta > dirty_budget:
    failures.append(f"dirty-state p95 delta {dirty_delta:.1f} ms exceeds {dirty_budget:.0f} ms")
if clean_delta > clean_budget:
    failures.append(f"clean-state p95 delta {clean_delta:.1f} ms exceeds {clean_budget:.0f} ms")
for failure in failures:
    print(f"FAIL: {failure}")
sys.exit(1 if failures else 0)
PY
