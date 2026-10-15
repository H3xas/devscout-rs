#!/usr/bin/env python3
"""Render a registered comparison with excluded sites and synthetic results."""

import argparse
import json
from pathlib import Path

from ts_resolution_report import render as render_table


def render(scores, synthetics, date, base, head):
    lines = render_table(scores, date, base, head).splitlines()
    lines[3:3] = ["First registration: `2026-10-15-typescript-module-resolution.md`. "
                  "This run scores the sites recorded by the extractor and lists excluded sites.", ""]
    lines.extend(["", "## Outside the scoring set", "",
                  "| Fixture | Arm | Sub-class | Sites |",
                  "| --- | --- | --- | ---: |"])
    for result in sorted(scores, key=lambda item: (item["fixture"], item["arm"])):
        outside = result["outside_envelope"]
        for subclass, count in sorted(outside["counts"].items()):
            lines.append(f"| {result['fixture']} | {result['arm']} | {subclass} | {count} |")
    lines.append("")
    for result in sorted(scores, key=lambda item: (item["fixture"], item["arm"])):
        for site in sorted(result["outside_envelope"]["sites"],
                           key=lambda item: (item["file"], item["line"], item["specifier"])):
            actual = ", ".join(site["actual"]) or "none"
            lines.append(f"- {result['fixture']} {result['arm']} {site['file']}:{site['line']} "
                         f"`{site['specifier']}`: {site['sub_class']}; "
                         f"truth `{site['truth']}`; actual `{actual}`")
    lines.extend(["", "## Synthetic cases", ""])
    for result in sorted(synthetics, key=lambda item: item["arm"]):
        lines.append(f"- {result['arm']}: {result['matching']}/{result['rows']} matching")
        for site in result["mismatches"]:
            actual = ", ".join(site["actual"]) or "none"
            lines.append(f"  - {site['case']} {site['file']}:{site['line']} "
                         f"`{site['specifier']}`: expected `{site['expected']}`; actual `{actual}`")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("date")
    parser.add_argument("base")
    parser.add_argument("head")
    parser.add_argument("output", type=Path)
    parser.add_argument("scores", nargs="+", type=Path)
    parser.add_argument("--synthetic", nargs="+", required=True, type=Path)
    args = parser.parse_args()
    if args.output.exists():
        raise ValueError(f"output already exists: {args.output}")
    scores = [json.loads(path.read_text()) for path in args.scores]
    synthetics = [json.loads(path.read_text()) for path in args.synthetic]
    args.output.write_text(render(scores, synthetics, args.date, args.base, args.head))


if __name__ == "__main__":
    main()
