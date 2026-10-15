#!/usr/bin/env python3
"""Classify the registered TypeScript sites before comparing either graph."""

import argparse
import csv
import fnmatch
import json
from collections import Counter, defaultdict
from pathlib import Path

from ts_resolution import direct_edges, score as score_sites, verify_manifest

FIELDS = ("fixture", "file", "line", "specifier", "target", "form", "mode", "link", "mapped")
SUBCLASSES = ("non-literal-import", "mode-not-declared", "types-versions",
              "json-module", "refused-configuration", "pnpm-default-ignore")
FORMS = {"import", "export", "import-equals", "require", "dynamic", "dynamic-non-literal"}
MODES = {"node": "node10", "node10": "node10", "node16": "node16",
         "nodenext": "nodenext", "bundler": "bundler", "classic": "classic"}
MODULES = {"none", "commonjs", "amd", "umd", "system", "es6", "es2015",
           "es2020", "es2022", "esnext", "node16", "node18", "node20", "nodenext", "preserve"}
REFUSED = object()


def strict_json(path):
    raw = path.read_text(encoding="utf-8")
    if raw.startswith("\ufeff"):
        raise ValueError("BOM")
    return json.loads(raw, parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)))


def valid_map(value):
    if isinstance(value, (bool, int, float)):
        return False
    if isinstance(value, dict):
        keys = list(value)
        if any(key.startswith(".") for key in keys) and not all(key.startswith(".") for key in keys):
            return False
        return all(valid_map(item) for item in value.values())
    if isinstance(value, list):
        return all(valid_map(item) for item in value)
    return value is None or isinstance(value, str)


def valid_patterns(value):
    return isinstance(value, list) and all(isinstance(item, str) and item
        and not any(char in item for char in "?[]{}") for item in value)


def package(path):
    if not path.exists():
        return None, ""
    try:
        data = strict_json(path)
    except (UnicodeError, ValueError):
        return REFUSED, "document"
    if not isinstance(data, dict):
        return REFUSED, "document"
    checks = (("name", lambda v: isinstance(v, str) and bool(v)),
              ("type", lambda v: v in ("module", "commonjs")),
              ("main", lambda v: isinstance(v, str)),
              ("exports", valid_map), ("imports", valid_map),
              ("workspaces", lambda v: valid_patterns(v) or
               (isinstance(v, dict) and valid_patterns(v.get("packages")))))
    for field, check in checks:
        if field in data and not check(data[field]):
            return REFUSED, field
    return data, ""


def strip_jsonc(raw):
    output = []
    quoted = False
    escaped = False
    index = 0
    while index < len(raw):
        char = raw[index]
        next_char = raw[index + 1] if index + 1 < len(raw) else ""
        if quoted:
            output.append(char)
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                quoted = False
            index += 1
        elif char == '"':
            quoted = True
            output.append(char)
            index += 1
        elif char == "/" and next_char == "/":
            while index < len(raw) and raw[index] not in "\r\n":
                index += 1
        elif char == "/" and next_char == "*":
            end = raw.find("*/", index + 2)
            if end < 0:
                raise ValueError("unclosed comment")
            index = end + 2
        elif char == ",":
            look = index + 1
            while look < len(raw) and raw[look].isspace():
                look += 1
            if look == len(raw) or raw[look] not in "}]":
                output.append(char)
            index += 1
        else:
            output.append(char)
            index += 1
    return "".join(output)


def config(path):
    if not path.exists():
        return None, ""
    try:
        data = json.loads(strip_jsonc(path.read_text(encoding="utf-8")))
    except (UnicodeError, ValueError):
        return REFUSED, "document"
    if not isinstance(data, dict) or not isinstance(data.get("compilerOptions", {}), dict):
        return REFUSED, "document"
    options = data.get("compilerOptions", {})
    for field in ("moduleResolution", "module"):
        if field in options:
            value = options[field]
            accepted = MODES if field == "moduleResolution" else MODULES
            if not isinstance(value, str) or value.lower() not in accepted:
                return REFUSED, field
    for field in ("outDir", "rootDir"):
        if field in options and (not isinstance(options[field], str) or not options[field]):
            return REFUSED, field
    if "customConditions" in options and (not isinstance(options["customConditions"], list)
            or not all(isinstance(item, str) for item in options["customConditions"])):
        return REFUSED, "customConditions"
    if "composite" in options and not isinstance(options["composite"], bool):
        return REFUSED, "composite"
    return data, ""


def config_chain(path):
    seen = set()
    options = {}
    for _ in range(4):
        if path in seen or not path.exists():
            break
        seen.add(path)
        data, error = config(path)
        if data is REFUSED:
            return REFUSED, seen
        if data is None:
            break
        options = {**data.get("compilerOptions", {}), **options}
        base = data.get("extends")
        if not isinstance(base, str) or not base.startswith("."):
            break
        path = (path.parent / (base if base.endswith(".json") else base + ".json")).resolve()
    return options, seen


def mode_for(root, file):
    directory = (root / file).parent
    while directory == root or root in directory.parents:
        candidate = directory / "tsconfig.json"
        if candidate.exists():
            options, _ = config_chain(candidate)
            if options is not REFUSED and ("moduleResolution" in options or "module" in options):
                return declared_mode(options)
        if directory == root:
            break
        directory = directory.parent
    for name in ("tsconfig.json", "tsconfig.base.json"):
        options, _ = config_chain(root / name)
        if options is not REFUSED and options and ("moduleResolution" in options or "module" in options):
            return declared_mode(options)
    return None


def declared_mode(options):
    if "moduleResolution" in options:
        return MODES[options["moduleResolution"].lower()]
    return {"node16": "node16", "node18": "node16", "node20": "node16",
            "nodenext": "nodenext", "preserve": "bundler"}.get(options.get("module", "").lower())


def nearest_package(root, file):
    directory = (root / file).parent
    while directory == root or root in directory.parents:
        candidate = directory / "package.json"
        if candidate.exists():
            return candidate
        if directory == root:
            break
        directory = directory.parent
    return None


def pnpm_patterns(path):
    if not path.exists():
        return None, ""
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError:
        raise
    except UnicodeError:
        return REFUSED, "document"
    found = False
    in_packages = False
    patterns = []
    for line in lines:
        stripped = line.strip()
        if not stripped or stripped.startswith("#") or stripped == "---" and not found:
            continue
        if stripped == "---" or stripped == "..." or "\t" in line[:len(line) - len(line.lstrip())]:
            return REFUSED, "document"
        if line.startswith("packages:"):
            if found:
                return REFUSED, "packages"
            found = True
            in_packages = True
            remainder = line[len("packages:"):].strip()
            if remainder:
                if not remainder.startswith("[") or not remainder.endswith("]"):
                    return REFUSED, "packages"
                pieces = remainder[1:-1].split(",")
                patterns = [piece.strip().strip("\"'") for piece in pieces if piece.strip()]
                if any(piece.strip().startswith(("&", "*", "|", ">", "!<")) for piece in pieces):
                    return REFUSED, "packages"
                in_packages = False
        elif in_packages and (line[0].isspace() or line.startswith("- ")):
            if not stripped.startswith("- "):
                return REFUSED, "packages"
            value = stripped[2:].split(" #", 1)[0].strip()
            if not value or value.startswith(("&", "*", "|", ">", "!<")):
                return REFUSED, "packages"
            patterns.append(value.strip("\"'"))
        elif not line[0].isspace() and not line.startswith("- "):
            in_packages = False
        elif not in_packages:
            continue
        else:
            return REFUSED, "document"
    if not found:
        return [], ""
    if not valid_patterns(patterns):
        return REFUSED, "packages"
    return patterns, ""


def refused_input(root, row, mode):
    source_package = nearest_package(root, row["file"])
    suffix = Path(row["file"]).suffix
    paths = []
    if source_package and ((mode in ("node16", "nodenext") and suffix in (".ts", ".tsx", ".js", ".jsx"))
            or (row["specifier"].startswith("#") and mode != "node10")
            or (not row["specifier"].startswith((".", "/", "#")) and row["link"] == ""
                and row["target"].startswith(source_package.parent.relative_to(root).as_posix() + "/"))):
        paths.append(source_package)
    if row["link"]:
        paths.append(root / row["link"] / "package.json")
        directory = (root / row["file"]).parent
        while directory == root or root in directory.parents:
            paths.append(directory / "pnpm-workspace.yaml")
            paths.append(directory / "package.json")
            if directory == root:
                break
            directory = directory.parent
    for path in paths:
        if path.name == "package.json":
            data, field = package(path)
            if data is REFUSED:
                return True
            if isinstance(data, dict) and "workspaces" in data:
                patterns = data["workspaces"]
                if isinstance(patterns, dict):
                    patterns = patterns.get("packages")
                if not valid_patterns(patterns):
                    return True
        else:
            data, field = pnpm_patterns(path)
            if data is REFUSED:
                return True
    if row["mapped"] == "1" and row["link"]:
        options, _ = config_chain(root / row["link"] / "tsconfig.json")
        if options is REFUSED:
            return True
    return False


def scope_for(root, row):
    source = (root / row["file"]).resolve()
    if root.resolve() not in source.parents or not source.is_file():
        raise ValueError(f"site source unavailable: {row['file']}:{row['line']}")
    if row["form"] == "dynamic-non-literal":
        return SUBCLASSES[0]
    mode = mode_for(root, row["file"])
    if mode not in ("node10", "node16", "nodenext", "bundler") or mode != row["mode"]:
        return SUBCLASSES[1]
    entered = root / row["link"] / "package.json" if row["link"] else None
    if not entered and row["specifier"].startswith("."):
        candidate = (root / row["file"]).parent / row["specifier"] / "package.json"
        entered = candidate if candidate.exists() else None
    if entered:
        data, _ = package(entered)
        if isinstance(data, dict) and "typesVersions" in data:
            return SUBCLASSES[2]
    if row["specifier"].split("/")[-1].endswith(".json") or row["target"].endswith(".json"):
        return SUBCLASSES[3]
    if refused_input(root, row, mode):
        return SUBCLASSES[4]
    if row["link"]:
        patterns, _ = pnpm_patterns(root / "pnpm-workspace.yaml")
        member = row["link"]
        if patterns is not REFUSED and patterns and any(fnmatch.fnmatch(member, p) for p in patterns
                if not p.startswith("!")) and any(part in ("node_modules", "bower_components", "test", "tests")
                for part in Path(member).parts):
            return SUBCLASSES[5]
    return ""


def read_sites(path, classified=False):
    rows = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        fields = line.split("\t")
        if len(fields) != len(FIELDS) + int(classified):
            raise ValueError(f"{path}:{number}: wrong field count")
        row = dict(zip(FIELDS + (("scope",) if classified else ()), fields))
        row["line"] = int(row["line"])
        if row["line"] < 1 or row["form"] not in FORMS or not row["specifier"]:
            raise ValueError(f"{path}:{number}: invalid site")
        for field in ("file", "target", "link"):
            value = row[field]
            if value and (Path(value).is_absolute() or ".." in Path(value).parts):
                raise ValueError(f"{path}:{number}: unsafe {field}")
        if row["mapped"] not in ("", "1"):
            raise ValueError(f"{path}:{number}: invalid mapped flag")
        if classified and row["scope"] not in ("", *SUBCLASSES):
            raise ValueError(f"{path}:{number}: unknown scope")
        rows.append(row)
    return rows


def write_sites(path, rows):
    with path.open("w", encoding="utf-8", newline="") as output:
        writer = csv.writer(output, delimiter="\t", lineterminator="\n")
        for row in rows:
            writer.writerow([row[field] for field in (*FIELDS, "scope")])


def checked_fixture(registration, name):
    verify_manifest(registration)
    fixtures = json.loads((registration / "fixtures.json").read_text())
    fixture = next((item for item in fixtures if item["id"] == name), None)
    if fixture is None:
        raise ValueError(f"unregistered fixture: {name}")
    return fixture


def classify(root, truth):
    rows = read_sites(truth)
    for row in rows:
        row["scope"] = scope_for(root, row)
    return rows


def score(registration, fixture_name, arm, classified, graph):
    fixture = checked_fixture(registration, fixture_name)
    rows = read_sites(classified, True)
    if any(row["fixture"] != fixture_name for row in rows):
        raise ValueError("classified truth contains a different fixture")
    graph_data = json.loads(graph.read_text())
    inside = [{"fixture": row["fixture"], "file": row["file"], "line": row["line"],
               "specifier": row["specifier"], "truth": row["target"]}
              for row in rows if not row["scope"]]
    result = score_sites(inside, graph_data)
    if result["truth_resolvable"] < fixture["minimum_truth_resolvable"]:
        raise ValueError("fixture does not meet its registered size")
    edges = direct_edges(graph_data)
    outside = [dict(file=row["file"], line=row["line"], specifier=row["specifier"],
                    form=row["form"], sub_class=row["scope"], truth=row["target"],
                    actual=sorted(edges.get((row["file"], row["line"], row["specifier"]), set())))
               for row in rows if row["scope"]]
    result["outside_envelope"] = {"counts": dict(Counter(row["sub_class"] for row in outside)),
                                  "sites": outside}
    return {"fixture": fixture_name, "mode": fixture["mode"], "arm": arm, **result}


def count(files):
    fixtures = []
    pooled = defaultdict(int)
    for file in files:
        rows = read_sites(file, True)
        if not rows:
            raise ValueError(f"empty classified truth: {file}")
        names = {row["fixture"] for row in rows}
        if len(names) != 1:
            raise ValueError(f"mixed fixtures: {file}")
        inside = [row for row in rows if not row["scope"]]
        mode = inside[0]["mode"] if inside else ""
        resolved = sum(bool(row["target"]) for row in inside)
        pooled[mode] += resolved
        fixtures.append({"fixture": next(iter(names)), "mode": mode, "sites": len(rows),
                         "truth_resolvable": resolved,
                         "outside_envelope": dict(Counter(row["scope"] for row in rows if row["scope"]))})
    return {"fixtures": fixtures, "pooled_truth_resolvable": dict(pooled)}


def synthetic(registration, arm, cases_root):
    verify_manifest(registration)
    expected = list(csv.DictReader((Path("fixtures/module-resolution") / "expected.tsv").open(), delimiter="\t"))
    mismatches = []
    for row in expected:
        graph = json.loads((cases_root / row["case"] / "graph.json").read_text())
        edges = direct_edges(graph).get((row["file"], int(row["line"]), row["specifier"]), set())
        wanted = row["expected"]
        if wanted.startswith("unresolved:"):
            reason = wanted.partition(":")[2]
            unresolved = graph.get("unresolved_imports", [])
            matched = not edges and any(item["from_file"] == row["file"]
                and item["from_line"] == int(row["line"]) and item["target"] == row["specifier"]
                and item["reason"] == reason for item in unresolved)
        else:
            matched = edges == {wanted}
        if not matched:
            mismatches.append({"case": row["case"], "file": row["file"], "line": int(row["line"]),
                               "specifier": row["specifier"], "expected": wanted, "actual": sorted(edges)})
    return {"arm": arm, "rows": len(expected), "matching": len(expected) - len(mismatches),
            "mismatches": mismatches}


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ("verify", "count", "classify", "score", "synthetic"):
        sub.add_parser(name)
    args, rest = parser.parse_known_args()
    if args.command == "verify":
        folder = Path(rest[0])
        print(json.dumps({"ok": True, "registered": sorted(verify_manifest(folder)["sha256"])}))
    elif args.command == "classify":
        root, truth, output = map(Path, rest)
        write_sites(output, classify(root, truth))
    elif args.command == "count":
        print(json.dumps(count([Path(item) for item in rest]), indent=2, sort_keys=True))
    elif args.command == "score":
        registration, fixture, arm, classified, graph, output = rest
        Path(output).write_text(json.dumps(score(Path(registration), fixture, arm, Path(classified),
                                                Path(graph)), indent=2) + "\n")
    elif args.command == "synthetic":
        registration, arm, cases_root, output = rest
        Path(output).write_text(json.dumps(synthetic(Path(registration), arm, Path(cases_root)),
                                                indent=2) + "\n")


if __name__ == "__main__":
    main()
