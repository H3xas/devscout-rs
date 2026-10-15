#!/usr/bin/env python3
"""Score declared module edges against TypeScript's pinned resolver output."""

import argparse
import hashlib
import json
from pathlib import Path


def verify_manifest(folder):
    manifest = json.loads((folder / "manifest.json").read_text())
    for name, expected in manifest["sha256"].items():
        actual = hashlib.sha256((folder / name).read_bytes()).hexdigest()
        if actual != expected:
            raise ValueError(f"changed registered input: {name}")
    return manifest


def read_truth(path):
    rows = []
    for line in path.read_text().splitlines():
        fields = line.split("\t")
        if len(fields) != 5:
            raise ValueError(f"truth row has {len(fields)} fields: {line}")
        fixture, file, line_number, specifier, target = fields
        rows.append({"fixture": fixture, "file": file, "line": int(line_number),
                     "specifier": specifier, "truth": target})
    return rows


def direct_edges(graph):
    found = {}
    for edge in graph["edges"]:
        if edge["kind"] != "import" or edge.get("via") is not None:
            continue
        key = (edge["from_file"], edge["from_line"], edge["target"])
        found.setdefault(key, set()).add(edge["to_file"])
    return found


def score(truth, graph):
    edges = direct_edges(graph)
    counts = {key: 0 for key in ("agree", "agree-unresolved", "miss", "wrong-target", "spurious")}
    disagreements = []
    for row in truth:
        key = (row["file"], row["line"], row["specifier"])
        targets = edges.get(key, set())
        wanted = row["truth"]
        if wanted and targets == {wanted}:
            label = "agree"
        elif wanted and not targets:
            label = "miss"
        elif wanted:
            label = "wrong-target"
        elif targets:
            label = "spurious"
        else:
            label = "agree-unresolved"
        counts[label] += 1
        if label not in ("agree", "agree-unresolved"):
            disagreements.append({**row, "class": label, "actual": sorted(targets)})
    denominator = counts["agree"] + counts["miss"] + counts["wrong-target"]
    met = (denominator > 0 and counts["agree"] * 100 >= denominator * 95
           and counts["wrong-target"] == 0 and counts["spurious"] == 0)
    return {"counts": counts, "truth_resolvable": denominator, "gate_met": met,
            "disagreements": disagreements}


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    verify = sub.add_parser("verify")
    verify.add_argument("registration", type=Path)
    scoring = sub.add_parser("score")
    scoring.add_argument("registration", type=Path)
    scoring.add_argument("fixture")
    scoring.add_argument("arm")
    scoring.add_argument("truth", type=Path)
    scoring.add_argument("graph", type=Path)
    scoring.add_argument("output", type=Path)
    args = parser.parse_args()
    manifest = verify_manifest(args.registration)
    if args.command == "verify":
        print(json.dumps({"ok": True, "registered": sorted(manifest["sha256"])}))
        return
    fixtures = json.loads((args.registration / "fixtures.json").read_text())
    fixture = next((f for f in fixtures if f["id"] == args.fixture), None)
    if fixture is None:
        raise ValueError(f"unregistered fixture: {args.fixture}")
    truth = read_truth(args.truth)
    if any(row["fixture"] != args.fixture for row in truth):
        raise ValueError("truth contains a different fixture")
    graph = json.loads(args.graph.read_text())
    result = score(truth, graph)
    if result["truth_resolvable"] < fixture["minimum_truth_resolvable"]:
        raise ValueError("fixture does not meet its registered size")
    args.output.write_text(json.dumps({"fixture": args.fixture, "mode": fixture["mode"],
                                        "arm": args.arm, **result}, indent=2) + "\n")


if __name__ == "__main__":
    main()
