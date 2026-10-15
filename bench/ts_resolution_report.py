#!/usr/bin/env python3
"""Render one complete module resolution comparison from stored scores."""

import json
import sys
from collections import defaultdict
from pathlib import Path


def render(scores, date, base, head):
    by_mode = defaultdict(lambda: defaultdict(lambda: defaultdict(int)))
    lines = ["# TypeScript module resolution", "",
             f"Run date: {date}. Base: `{base}`. Head: `{head}`.", "",
             "Truth: TypeScript 5.9.3 with declaration files excluded.", "",
             "| Fixture | Mode | Arm | Agreement | Wrong target | Spurious | Gate |",
             "| --- | --- | --- | ---: | ---: | ---: | --- |"]
    for score in sorted(scores, key=lambda s: (s["mode"], s["fixture"], s["arm"])):
        counts = score["counts"]
        denominator = score["truth_resolvable"]
        agreement = f"{counts['agree']}/{denominator}"
        lines.append(f"| {score['fixture']} | {score['mode']} | {score['arm']} | {agreement} | "
                     f"{counts['wrong-target']} | {counts['spurious']} | "
                     f"{'met' if score['gate_met'] else 'missed'} |")
        for key, value in counts.items():
            by_mode[score["mode"]][score["arm"]][key] += value
    lines.extend(["", "## Pooled modes", "",
                  "| Mode | Arm | Agreement | Wrong target | Spurious | Gate |",
                  "| --- | --- | ---: | ---: | ---: | --- |"])
    for mode, arms in sorted(by_mode.items()):
        for arm, counts in sorted(arms.items()):
            denominator = sum(counts[key] for key in ("agree", "miss", "wrong-target"))
            met = (denominator > 0 and counts["agree"] * 100 >= denominator * 95
                   and counts["wrong-target"] == 0 and counts["spurious"] == 0)
            lines.append(f"| {mode} | {arm} | {counts['agree']}/{denominator} | "
                         f"{counts['wrong-target']} | {counts['spurious']} | "
                         f"{'met' if met else 'missed'} |")
    lines.extend(["", "## Disagreements", ""])
    for score in sorted(scores, key=lambda s: (s["fixture"], s["arm"])):
        for row in score["disagreements"]:
            actual = ", ".join(row["actual"]) or "none"
            lines.append(f"- {score['fixture']} {score['arm']} {row['file']}:{row['line']} "
                         f"`{row['specifier']}`: {row['class']}; truth `{row['truth']}`; actual `{actual}`")
    if lines[-1] == "":
        lines.append("No disagreements.")
    return "\n".join(lines) + "\n"


def main():
    if len(sys.argv) < 6:
        raise SystemExit("usage: ts_resolution_report.py <date> <base> <head> <output> <score.json>...")
    date, base, head, output, *files = sys.argv[1:]
    scores = [json.loads(Path(file).read_text()) for file in files]
    Path(output).write_text(render(scores, date, base, head))


if __name__ == "__main__":
    main()
