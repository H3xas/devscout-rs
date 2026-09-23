"""Qualify extension candidates without changing a production graph or traversal."""

import argparse
import hashlib
import json
import subprocess
from pathlib import Path

from extension_evidence import Evidence, admission_failures, candidates, summarize


def read_jsonl(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("graph", "refs", "units", "audit", "driver", "out"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--manifest", type=Path)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=False)
    graph = json.loads(args.graph.read_text())
    units = read_jsonl(args.units)
    evidence = Evidence(graph, read_jsonl(args.refs), units,
                        json.loads(args.manifest.read_text()) if args.manifest else None)
    cohort = evidence.cohort()
    audit = json.loads(args.audit.read_text())
    for name in ("precise", "ext", "guess"):
        for metric in ("edges", "tp", "fp"):
            if cohort[name][metric] != audit["tiers"].get(name, {}).get(metric, 0):
                raise ValueError("native audit mismatch: " + name + "." + metric)
    if cohort["precise"]["recall_denominator"] != audit["recall"]["denominator"]:
        raise ValueError("native audit recall denominator mismatch")
    baseline, oracle = evidence.replay_graphs()
    selected, _ = candidates(baseline)
    original_cohort = {json.dumps(graph["edges"][i], sort_keys=True) for i in cohort["candidate_numbers"]}
    replay_cohort = {json.dumps(baseline["edges"][i], sort_keys=True) for i in selected}
    if original_cohort != replay_cohort:
        raise ValueError("candidate cohort changed under the compiled-universe projection")
    seeds = sorted({e["to_file"] for e in baseline["edges"] if e.get("tier") == "ext"
                    and e["to"] in {d["id"] for d in baseline["defs"]}})
    write_json(args.out / "baseline.json", baseline)
    write_json(args.out / "oracle.json", oracle)
    spec = dict(candidates=selected, seeds=seeds)
    write_json(args.out / "spec.json", spec)
    run = subprocess.run([str(args.driver.resolve()), str(args.out.resolve())],
                         capture_output=True, text=True)
    if run.returncode:
        (args.out / "driver-error.txt").write_text(run.stderr)
        raise RuntimeError(run.stderr.strip())
    (args.out / "answers.jsonl").write_text(run.stdout)
    answers = [json.loads(line) for line in run.stdout.splitlines()]
    summary = summarize(answers)
    failures = admission_failures(cohort, summary)
    manifest = dict(inputs={name: hashlib.sha256(path.read_bytes()).hexdigest()
                           for name, path in (("graph", args.graph), ("refs", args.refs),
                                              ("units", args.units), ("audit", args.audit),
                                              ("driver", args.driver))},
                    harness={p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                             for p in (Path(__file__), Path(__file__).with_name("extension_evidence.py"))},
                    protocol=dict(hops=2, cap=50, interface_brake=8, hub_brake=34,
                                  seeds="all extension target files, sorted",
                                  truth="fixed non-member context; no cap or numerical brakes"),
                    units=dict(ok=sum(u["status"] == "ok" for u in units),
                               failed=sum(u["status"] != "ok" for u in units),
                               diagnostics=sum(u.get("diagnostics", 0) for u in units)),
                    status="no-go" if failures else "corpus-gates-pass",
                    failures=failures,
                    cohort=cohort, whole_answer=summary)
    write_json(args.out / "results.json", manifest)
    print(json.dumps(dict(status=manifest["status"], failures=failures,
                          cohort={k: v for k, v in cohort.items() if k != "candidate_numbers"},
                          whole_answer=summary["cohorts"], queries=len(answers)), indent=2))
    return 2 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
