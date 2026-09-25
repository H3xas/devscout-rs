#!/bin/sh
# Held-out family run for the .NET target/project/framework qualification matrix.
#
# Usage: bench/dotnet-target-qualification.sh
#
# Clones the pinned held-out corpus (bench/corpus.lock's csharp-target-held-out entry),
# scores devscout's `uses-member` edges against the compiler oracle per TFM the corpus's own
# declared TargetFrameworks actually offer, checks the result against the pre-registered
# thresholds in fixtures/csharp-target-qualification/expected-held-out.json, and writes a dated
# results document under docs/benchmarks/results/. Manual only -- never wired into CI or cargo
# test, per docs/benchmarks/README.md's own rule that the PR-gated job stays bounded to the small
# hand-authored fixture tree.
#
# Every stratum registered in expected-held-out.json gets a row, whether or not the corpus
# declares any of its TFMs: a stratum with no measurable TFM is reported "not exercised" with
# the real reason, not silently dropped from a hand-picked TFM list.
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

project="$corpus_dir/src/Serilog/Serilog.csproj"

# Every TFM any registered stratum names, not a hand-picked subset -- a stratum whose TFMs the
# corpus never declares still gets a row below, rather than silently vanishing from the report.
all_tfms="$(python3 -c "
import json
strata = json.load(open('$thresholds'))['strata']
tfms = []
for s in strata.values():
    for t in s['tfms']:
        if t not in tfms:
            tfms.append(t)
print(' '.join(tfms))
")"

# The corpus's own declared TargetFrameworks, queried at run time rather than assumed -- what
# lets a not-yet-multi-targeted stratum be reported honestly instead of guessed at authoring
# time. Evaluated on this (non-Windows) worker, so a Windows-conditional TFM in the corpus's own
# .csproj evaluates empty and is correctly absent here.
declared_tfms="$(dotnet msbuild "$project" -getProperty:TargetFrameworks | tr ';' ' ')"
# shellcheck disable=SC2086
declared_tfms="$(echo $declared_tfms)"

{
	echo "# .NET target qualification -- held-out run ($date_tag)"
	echo
	echo "Single-run banner: one repetition per stratum, per"
	echo "[docs/benchmarks/README.md](../../../docs/benchmarks/README.md)'s honesty statement."
	echo "Predictions registered before this run in"
	echo "[expected-held-out.json](../../../fixtures/csharp-target-qualification/expected-held-out.json)."
	echo "Precision/recall uncertainty below is a single-run Wilson 95% interval computed from"
	echo "each row's own tp/fp/denominator counts, not from repeated runs."
	echo
	echo "Corpus: \`serilog/serilog\` at the SHA pinned in"
	echo "[bench/corpus.lock](../../../bench/corpus.lock) (\`$corpus_id\`)."
	echo
	echo "Corpus's own declared TargetFrameworks on this (non-Windows) worker: \`$declared_tfms\`."
	echo
	echo "| TFM | Measured | Restore | Build | Oracle/audit | Precision (95% CI) | Recall all (95% CI) | Recall precise | Denominator | Unsupported coverage |"
	echo "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"
} >"$results_doc"

not_measured_reasons=""

for tfm in $all_tfms; do
	case " $declared_tfms " in
	*" $tfm "*) ;;
	*)
		not_measured_reasons="$not_measured_reasons
- \`$tfm\`: not declared by the corpus's own TargetFrameworks on this worker (observed: \`$declared_tfms\`)."
		echo "| $tfm | no | -- | -- | -- | n/a | n/a | n/a | n/a | n/a |" >>"$results_doc"
		continue
		;;
	esac

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

	oracle_status="skipped"
	precision="n/a"
	recall_all="n/a"
	recall_precise="n/a"
	denominator="n/a"
	unsupported_coverage="n/a"

	if [ "$build_status" = "ok" ]; then
		oracle_status="ok"
		if ! dotnet run --project tools/scout-semantic --no-build -c Release -- \
			"$project" --root "$corpus_dir" --tfm "$tfm" \
			--out "$stratum_out/refs.jsonl" --units "$stratum_out/units.jsonl" \
			>"$stratum_out/oracle.log" 2>&1; then
			oracle_status="oracle failed"
		fi

		if [ "$oracle_status" = "ok" ] && [ -s "$stratum_out/refs.jsonl" ]; then
			if ! ./target/release/devscout -C "$corpus_dir" map . >"$stratum_out/map.log" 2>&1; then
				oracle_status="map failed"
			fi
		elif [ "$oracle_status" = "ok" ]; then
			oracle_status="oracle produced no refs"
		fi

		if [ "$oracle_status" = "ok" ]; then
			if ! ./target/release/devscout -C "$corpus_dir" audit --semantic "$PWD/$stratum_out/refs.jsonl" \
				--units "$PWD/$stratum_out/units.jsonl" --json \
				>"$stratum_out/audit.json" 2>"$stratum_out/audit.err"; then
				oracle_status="audit failed"
			fi
		fi

		if [ "$oracle_status" = "ok" ] && [ -s "$stratum_out/audit.json" ]; then
			result="$(python3 -c "
import json
a = json.load(open('$stratum_out/audit.json'))
precision = a['tiers']['precise']['precision']
recall_all = a['recall']['all']
recall_precise = a['recall']['precise']
denominator = a['recall']['denominator']
correct = a['silent']['correct']
leak = a['silent']['leak']
total = correct + leak
uc = round(correct / total, 3) if total > 0 else 'n/a'
print(precision, recall_all, recall_precise, denominator, uc)
" 2>/dev/null || echo "n/a n/a n/a n/a n/a")"
			set -- $result
			precision="${1:-n/a}"
			recall_all="${2:-n/a}"
			recall_precise="${3:-n/a}"
			denominator="${4:-n/a}"
			unsupported_coverage="${5:-n/a}"
		fi
	fi

	ci="$(python3 -c "
import json, math

def wilson(p, n, z=1.96):
    if n <= 0 or p in (None, 'n/a'):
        return 'n/a'
    p = float(p)
    denom = 1 + z * z / n
    center = (p + z * z / (2 * n)) / denom
    margin = (z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))) / denom
    lo = max(0.0, center - margin)
    hi = min(1.0, center + margin)
    return f'[{lo:.3f}, {hi:.3f}]'

try:
    audit = json.load(open('$stratum_out/audit.json'))
    p_tier = audit['tiers']['precise']
    p_n = p_tier['tp'] + p_tier['fp']
    p_ci = wilson('$precision', p_n)
    r_denom = audit['recall']['denominator']
    r_ci = wilson('$recall_all', r_denom) if r_denom > 0 else 'n/a'
    print(f'{p_ci} | {r_ci}')
except Exception:
    print('n/a | n/a')
" 2>/dev/null || echo "n/a | n/a")"
	precision_ci="$(echo "$ci" | cut -d'|' -f1 | xargs)"
	recall_ci="$(echo "$ci" | cut -d'|' -f2 | xargs)"

	echo "| $tfm | yes | $restore_status | $build_status | $oracle_status | $precision $precision_ci | $recall_all $recall_ci | $recall_precise | $denominator | $unsupported_coverage |" >>"$results_doc"
done

python3 - "$results_doc" "$thresholds" "$out" "$declared_tfms" <<'PYEOF' >>"$results_doc"
import json
import sys

results_doc, thresholds_path, out_dir, declared_tfms = sys.argv[1:5]
declared = set(declared_tfms.split())
thresholds = json.load(open(thresholds_path))
strata = thresholds["strata"]

print()
print("## Threshold check")
print()
print("Registered minimums from `expected-held-out.json`, checked per registered stratum. A")
print("stratum with no declared TFM on this worker is reported `not exercised`, never omitted.")
print()
print("| Stratum | TFMs measured | Precision >= min | Recall >= min | Unsupported coverage >= min | Result |")
print("| --- | --- | --- | --- | --- | --- |")

for stratum_name, stratum in strata.items():
    measured_tfms = [t for t in stratum["tfms"] if t in declared]
    if not measured_tfms:
        undeclared = ", ".join(f"`{t}`" for t in stratum["tfms"])
        print(f"| {stratum_name} | none | n/a | n/a | n/a | not exercised: none of {undeclared} is declared by the corpus on this worker |")
        continue

    p_min = stratum["precision"]["min"]
    r_min = stratum["recall"]["min"]
    uc_min = stratum.get("unsupported_coverage", {}).get("min")

    for tfm in measured_tfms:
        audit_path = f"{out_dir}/{tfm}/audit.json"
        try:
            audit = json.load(open(audit_path))
            precision = audit["tiers"]["precise"]["precision"]
            recall = audit["recall"]["all"]
            correct = audit["silent"]["correct"]
            leak = audit["silent"]["leak"]
            total = correct + leak
            uc = (correct / total) if total > 0 else None
        except Exception:
            print(f"| {stratum_name} | {tfm} | n/a | n/a | n/a | not measured: oracle/audit did not produce a usable result |")
            continue

        p_ok = precision is not None and precision >= p_min
        r_ok = recall is not None and recall >= r_min
        if uc_min is None:
            uc_ok = True
            uc_text = "not registered"
        elif uc is None:
            uc_ok = True
            uc_text = "not exercised: no silently-dropped edges observed"
        else:
            uc_ok = uc >= uc_min
            uc_text = f"{uc:.3f} >= {uc_min}: {uc_ok}"

        verdict = "meets threshold" if (p_ok and r_ok and uc_ok) else "MISS"
        print(f"| {stratum_name} | {tfm} | {precision} >= {p_min}: {p_ok} | {recall} >= {r_min}: {r_ok} | {uc_text} | {verdict} |")
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
	if [ -n "$not_measured_reasons" ]; then
		echo "TFMs registered in a stratum but not declared by this corpus on this (non-Windows)"
		echo "worker, recorded here rather than silently skipped:"
		echo "$not_measured_reasons"
	else
		echo "Every registered TFM was declared by the corpus on this worker."
	fi
} >>"$results_doc"

echo "held-out run complete: $results_doc"
