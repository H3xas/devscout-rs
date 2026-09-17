#!/bin/sh
# Held-out family run for the .NET target/project/framework qualification matrix.
#
# Usage: bench/dotnet-target-qualification.sh
#
# Clones the pinned held-out corpus (bench/corpus.lock's csharp-target-held-out entry),
# scores devscout's `uses-member` edges against the compiler oracle per wave-1 stratum this
# corpus actually multi-targets, checks the result against the pre-registered thresholds in
# fixtures/csharp-target-qualification/expected-held-out.json, and writes a dated results
# document under docs/benchmarks/results/. Manual only -- never wired into CI or cargo test, per
# docs/benchmarks/README.md's own rule that the PR-gated job stays bounded to the small
# hand-authored fixture tree.
#
# Requires `cargo build --release` and a dotnet SDK matching the pinned band already run first.

set -eu

corpus_id="csharp-target-held-out"
corpus_dir="bench/clones/$corpus_id"
out="bench/out/dotnet-target-held-out"
date_tag="$(date -u +%Y-%m-%d)"
results_doc="docs/benchmarks/results/$date_tag-dotnet-target-held-out.md"
thresholds="fixtures/csharp-target-qualification/expected-held-out.json"

mkdir -p "$out" bench/state

if [ ! -e "$thresholds" ]; then
	echo "missing $thresholds; pre-registered thresholds must exist before this run" >&2
	exit 1
fi

if [ ! -d "$corpus_dir" ]; then
	./bench/clone-corpus.sh "$corpus_id" "$corpus_dir"
fi

export SCOUT_REGISTRY="$PWD/bench/state/repos.json"
export SCOUT_CONTENT_DB="$PWD/bench/state/content.db"

# The three TFMs this corpus multi-targets unconditionally, across the modern and netstandard
# wave-1 strata; the Windows-conditional net471/net462 targets are out of scope for this
# non-Windows run, and are reported as not measured rather than silently skipped.
tfms="net8.0 net6.0 netstandard2.0"
project="$corpus_dir/src/Serilog/Serilog.csproj"

{
	echo "# .NET target qualification -- held-out run ($date_tag)"
	echo
	echo "Single-run banner: one repetition per stratum, per"
	echo "[docs/benchmarks/README.md](../../../docs/benchmarks/README.md)'s honesty statement."
	echo "Predictions registered before this run in"
	echo "[expected-held-out.json](../../../fixtures/csharp-target-qualification/expected-held-out.json)."
	echo
	echo "Corpus: \`serilog/serilog\` at the SHA pinned in"
	echo "[bench/corpus.lock](../../../bench/corpus.lock) (\`$corpus_id\`)."
	echo
	echo "| Stratum (TFM) | Restore | Build | Precision | Recall (all) | Recall (precise) |"
	echo "| --- | --- | --- | --- | --- | --- |"
} >"$results_doc"

for tfm in $tfms; do
	stratum_out="$out/$tfm"
	mkdir -p "$stratum_out"

	restore_status="ok"
	if ! dotnet restore "$project" -p:TargetFramework="$tfm" >"$stratum_out/restore.log" 2>&1; then
		restore_status="failed"
	fi

	build_status="ok"
	if [ "$restore_status" = "ok" ]; then
		if ! dotnet build "$project" --no-restore -p:TargetFramework="$tfm" -c Release >"$stratum_out/build.log" 2>&1; then
			build_status="failed"
		fi
	else
		build_status="skipped"
	fi

	precision="n/a"
	recall_all="n/a"
	recall_precise="n/a"

	if [ "$build_status" = "ok" ]; then
		dotnet run --project tools/scout-semantic --no-build -c Release -- \
			"$project" --root "$corpus_dir" --tfm "$tfm" \
			--out "$stratum_out/refs.jsonl" --units "$stratum_out/units.jsonl" \
			>"$stratum_out/oracle.log" 2>&1 || true

		if [ -s "$stratum_out/refs.jsonl" ]; then
			./target/release/devscout -C "$corpus_dir" map . >"$stratum_out/map.log" 2>&1 || true
			./target/release/devscout -C "$corpus_dir" audit --semantic "$PWD/$stratum_out/refs.jsonl" \
				--units "$PWD/$stratum_out/units.jsonl" --json \
				>"$stratum_out/audit.json" 2>"$stratum_out/audit.err" || true

			if [ -s "$stratum_out/audit.json" ]; then
				precision="$(python3 -c "import json;print(json.load(open('$stratum_out/audit.json'))['tiers']['precise']['precision'])" 2>/dev/null || echo n/a)"
				recall_all="$(python3 -c "import json;print(json.load(open('$stratum_out/audit.json'))['recall']['all'])" 2>/dev/null || echo n/a)"
				recall_precise="$(python3 -c "import json;print(json.load(open('$stratum_out/audit.json'))['recall']['precise'])" 2>/dev/null || echo n/a)"
			fi
		fi
	fi

	echo "| $tfm | $restore_status | $build_status | $precision | $recall_all | $recall_precise |" >>"$results_doc"
done

python3 - "$results_doc" "$thresholds" "$out" "$tfms" <<'PYEOF' >>"$results_doc"
import json
import sys

results_doc, thresholds_path, out_dir, tfms = sys.argv[1:5]
thresholds = json.load(open(thresholds_path))
strata = thresholds["strata"]

print()
print("## Threshold check")
print()
print("Registered minimums from `expected-held-out.json`, checked against the row above.")
print()
print("| Stratum (TFM) | Precision >= min | Recall (all) >= min | Result |")
print("| --- | --- | --- | --- |")

for tfm in tfms.split():
    stratum_name = next((name for name, s in strata.items() if tfm in s.get("tfms", [])), None)
    audit_path = f"{out_dir}/{tfm}/audit.json"
    try:
        audit = json.load(open(audit_path))
        precision = audit["tiers"]["precise"]["precision"]
        recall = audit["recall"]["all"]
    except Exception:
        print(f"| {tfm} | n/a | n/a | not measured |")
        continue
    if stratum_name is None:
        print(f"| {tfm} | {precision} | {recall} | no registered stratum |")
        continue
    stratum = strata[stratum_name]
    p_min = stratum["precision"]["min"]
    r_min = stratum["recall"]["min"]
    p_ok = precision is not None and precision >= p_min
    r_ok = recall is not None and recall >= r_min
    verdict = "meets threshold" if (p_ok and r_ok) else "MISS"
    print(f"| {tfm} ({stratum_name}) | {precision} >= {p_min}: {p_ok} | {recall} >= {r_min}: {r_ok} | {verdict} |")
PYEOF

{
	echo
	echo "## Where peers win"
	echo
	echo "Not measured in this run: a peer/ripgrep baseline comparison. This run scores"
	echo "devscout's own compiler-fact edges against the pinned oracle only."
	echo
	echo "## Not measured"
	echo
	echo "\`net471\`, \`net462\`: Windows-conditional TFMs in this corpus's own \`.csproj\`,"
	echo "out of scope for this non-Windows run. Recorded here rather than silently skipped."
} >>"$results_doc"

echo "held-out run complete: $results_doc"
