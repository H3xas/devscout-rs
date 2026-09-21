#!/usr/bin/env python3
"""Compose per-profile capability rows for fixtures/csharp-target-qualification/.

For each wave-1 row (profile, deep-case bundle or substitution-defect control) this
script restores and builds its project(s), runs the existing tools/scout-semantic
oracle over it, and writes a five-field capability row to
fixtures/csharp-target-qualification/results/<row-id>.json. It is the only place in
the repository that shells out to `dotnet` for this tree.

Two modes:

  --check   regenerate every row into a temporary directory and diff it byte-for-byte
            against the committed file; non-zero exit on any difference or on an
            unexpected restore/build exit code. This is what makes "two regenerations
            from the same pinned inputs are byte-identical" a checked claim.
  --write   regenerate and overwrite the committed files. Used only by an operator
            deliberately refreshing the pin (for example after an SDK-pin bump).

Neither mode trusts the oracle's own exit code or unit status for the two
substitution-defect controls: their rows are computed from the raw declared-TFM
metadata and the oracle's own stderr, independent of what the oracle's status line
claims, because that status line is exactly what the defect makes untrustworthy.

Python 3 standard library only.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
TREE = REPO_ROOT / "fixtures" / "csharp-target-qualification"
RESULTS = TREE / "results"
ORACLE_PROJECT = REPO_ROOT / "tools" / "scout-semantic"

SDK_VERSION = "9.0.305"

# Profiles this SDK band and worker can execute today (see the Design's wave-1
# boundary). Each tuple: (profile id, csproj path relative to TREE, tfm, track,
# boundary case expected to bind).
MODERN_TRACK = "modern"
FRAMEWORK_TRACK = "framework-f1"

PROFILE_ROWS = [
    ("csharp73-net5.0-sdkstyle", "profiles/net5.0-sdkstyle/Profile.csproj", "net5.0", MODERN_TRACK, True),
    ("csharp73-net6.0-sdkstyle", "profiles/net6.0-sdkstyle/Profile.csproj", "net6.0", MODERN_TRACK, True),
    ("csharp73-net7.0-sdkstyle", "profiles/net7.0-sdkstyle/Profile.csproj", "net7.0", MODERN_TRACK, True),
    ("csharp73-net8.0-sdkstyle", "profiles/net8.0-sdkstyle/Profile.csproj", "net8.0", MODERN_TRACK, True),
    ("csharp73-net9.0-sdkstyle", "profiles/net9.0-sdkstyle/Profile.csproj", "net9.0", MODERN_TRACK, True),
    ("csharp73-netstandard2.0-sdkstyle", "profiles/netstandard2.0-sdkstyle/Profile.csproj", "netstandard2.0", MODERN_TRACK, False),
    ("csharp73-netstandard2.1-sdkstyle", "profiles/netstandard2.1-sdkstyle/Profile.csproj", "netstandard2.1", MODERN_TRACK, True),
    ("csharp73-netcoreapp3.1-sdkstyle", "profiles/netcoreapp3.1-sdkstyle/Profile.csproj", "netcoreapp3.1", MODERN_TRACK, True),
    ("csharp73-net40-sdkstyle", "profiles/net40-sdkstyle/Profile.csproj", "net40", FRAMEWORK_TRACK, False),
    ("csharp73-net472-sdkstyle", "profiles/net472-sdkstyle/Profile.csproj", "net472", FRAMEWORK_TRACK, False),
    ("csharp73-net48-sdkstyle", "profiles/net48-sdkstyle/Profile.csproj", "net48", FRAMEWORK_TRACK, False),
]

DEEP_ROWS = [
    ("csharp73-net8.0-deep", "deep/net8.0-deep/Deep.csproj", "net8.0", MODERN_TRACK),
    ("csharp73-net472-deep", "deep/net472-deep/Deep.csproj", "net472", FRAMEWORK_TRACK),
]

# The net472-deep bundle's incompatible-reference case is isolated in its own
# probe project because, in that direction, restore fails outright rather than
# warning; recorded as evidence on the net472-deep row without breaking its build.
DEEP_INCOMPATIBLE_PROBE = {
    "csharp73-net472-deep": "deep/net472-deep/IncompatibleRefProbe/Probe.csproj",
}

BOUNDARY_DIAGNOSTIC = "CS1501"

# A persistent MSBuild server node can hold one project's evaluated graph across separate
# `dotnet` invocations, which made a restore's own compatibility result depend on what ran
# before it in the same shell rather than on the fixture itself. Every dotnet call this
# script makes disables that reuse, so a row's exit code depends only on its own inputs.
DOTNET_ENV = {"MSBUILDDISABLENODEREUSE": "1", "DOTNET_CLI_UI_LANGUAGE": "en"}


def run(args, cwd=None):
    env = dict(os.environ)
    env.update(DOTNET_ENV)
    # Every dotnet invocation this script makes runs with the qualification tree as its
    # working directory by default, not the repository root: the .NET CLI resolves
    # global.json from the process's current directory, so a call made from anywhere else
    # never sees this tree's own SDK pin and silently falls back to whatever SDK band the
    # caller's environment happens to install.
    proc = subprocess.run(
        args, cwd=cwd or TREE, capture_output=True, text=True, check=False, env=env
    )
    return proc.returncode, strip_local_paths(proc.stdout), strip_local_paths(proc.stderr)


_OBSERVED_SDK_VERSION = None


def observed_sdk_version():
    """The SDK version the pinned tree actually resolved, queried once and reused for
    every row -- not the module's own pin constant, which only says what was intended."""
    global _OBSERVED_SDK_VERSION
    if _OBSERVED_SDK_VERSION is None:
        code, out, err = run(["dotnet", "--version"])
        _OBSERVED_SDK_VERSION = out.strip() if code == 0 and out.strip() else f"unresolved (exit {code}): {(out + err).strip()}"
    return _OBSERVED_SDK_VERSION


def strip_local_paths(text):
    """Removes the authoring machine's absolute filesystem paths from diagnostic
    text before it can reach a committed snapshot; a raw NuGet/MSBuild diagnostic
    routinely embeds the invoking machine's home directory and working path."""
    text = text.replace(str(REPO_ROOT), "<repo>")
    text = re.sub(r"/[A-Za-z0-9_./-]*/(\.nuget|\.dotnet)/", r"<home>/\1/", text)
    text = re.sub(r"/Users/[A-Za-z0-9_.-]+", "<home>", text)
    text = re.sub(r"/home/[A-Za-z0-9_.-]+", "<home>", text)
    return text


def declared_tfm(csproj_path):
    text = csproj_path.read_text(encoding="utf-8")
    single = re.search(r"<TargetFramework>([^<]+)</TargetFramework>", text)
    if single:
        return [single.group(1)]
    multi = re.search(r"<TargetFrameworks>([^<]+)</TargetFrameworks>", text)
    if multi:
        return multi.group(1).split(";")
    return []


def clean_build_state(csproj):
    """Removes a project's own obj/ and bin/ before restoring it. NuGet's incremental
    no-op detection can otherwise reuse an on-disk restore/build state left over from a
    previous run of this script and skip re-evaluating compatibility, which would make a
    row's recorded exit code depend on what ran before it rather than on the fixture
    itself -- exactly what byte-identical regeneration must not depend on."""
    for name in ("obj", "bin"):
        target = csproj.parent / name
        if target.exists():
            shutil.rmtree(target)


def restore_and_build(csproj_rel, no_restore):
    csproj = TREE / csproj_rel
    restore_code = 0
    restore_out = ""
    if not no_restore:
        clean_build_state(csproj)
        restore_code, restore_out, restore_err = run(["dotnet", "restore", str(csproj), "-v", "minimal"])
        restore_out = restore_out + restore_err
    # Debug, not Release: the oracle opens each project through MSBuildWorkspace with no
    # explicit Configuration override, which defaults to Debug, so an analyzer/generator
    # project reference must be built for Debug or the workspace cannot find its output DLL.
    build_code, build_out, build_err = run(
        ["dotnet", "build", str(csproj), "--no-restore", "-c", "Debug", "-v", "minimal"]
    )
    build_out = build_out + build_err
    return restore_code, restore_out, build_code, build_out


def run_oracle(csproj_rel, tfm, out_dir):
    out_dir.mkdir(parents=True, exist_ok=True)
    refs = out_dir / "refs.jsonl"
    units = out_dir / "units.jsonl"
    defs = out_dir / "defs.jsonl"
    args = [
        "dotnet", "run", "--project", str(ORACLE_PROJECT), "--no-build", "-c", "Release", "--",
        str(TREE / csproj_rel), "--root", str(TREE),
        "--out", str(refs), "--units", str(units), "--defs", str(defs),
    ]
    if tfm:
        args += ["--tfm", tfm]
    code, out, err = run(args)
    unit_records = _read_jsonl(units)
    ref_records = _read_jsonl(refs)
    def_records = _read_jsonl(defs)
    return code, out + err, unit_records, ref_records, def_records


def _read_jsonl(path):
    records = []
    if path.exists():
        for line in path.read_text(encoding="utf-8").splitlines():
            if line.strip():
                records.append(json.loads(line))
    return records


def compose_profile_row(profile_id, csproj_rel, tfm, track, bind_expected, out_dir, no_restore):
    restore_code, restore_out, build_code, build_out = restore_and_build(csproj_rel, no_restore)
    oracle_code, oracle_log, units, refs, defs = run_oracle(csproj_rel, tfm, out_dir)

    bind_observed = BOUNDARY_DIAGNOSTIC not in build_out
    unit = units[0] if units else None
    positive_bound = any(r.get("member") == "Describe" for r in refs)

    context_acquisition = {
        "state": "passing" if restore_code == 0 else "failing",
        "sdk": observed_sdk_version(),
        "sdk_pin": SDK_VERSION,
        "project_format": "sdk-style",
        "reference_source": "microsoft.netframework.referenceassemblies@1.0.3"
        if track == FRAMEWORK_TRACK
        else "sdk-implicit",
        "restore_exit_code": restore_code,
        "build_exit_code": build_code,
        "expected_documents": ["PositiveCase.cs", "BoundaryCase.cs"],
        "loaded_documents": sorted(unit["files"]) if unit else [],
    }
    semantic_conformance = {
        "state": "passing" if positive_bound else "failing",
        "positive_case_bound": positive_bound,
        "boundary_case_bind_expected": bind_expected,
        "boundary_case_bind_observed": bind_observed,
        "boundary_case_matches_expectation": bind_expected == bind_observed,
        "build_diagnostic_excerpt": extract_diagnostic(build_out, BOUNDARY_DIAGNOSTIC),
    }
    framework_modeling = {
        "state": "not-claimed",
        "reason": "no framework adapter is exercised by the wave-1 case source",
    }
    passing = (
        positive_bound
        and semantic_conformance["boundary_case_matches_expectation"]
        and restore_code == 0
        and unit is not None
        and unit.get("status") == "ok"
    )
    unsupported_state = {
        "state": "passing" if passing else "failing",
        "reason": "target-API surface qualified from its own compilation context"
        if passing
        else "restore, build or the boundary-case prediction disagreed with the observed evidence",
    }
    execution_assumptions = {"state": "static-only"}

    return {
        "profile_id": profile_id,
        "kind": "profile",
        "track": track,
        "tfm": tfm,
        "context_acquisition": context_acquisition,
        "semantic_conformance": semantic_conformance,
        "framework_modeling": framework_modeling,
        "unsupported_state": unsupported_state,
        "execution_assumptions": execution_assumptions,
    }


def extract_diagnostic(text, code):
    for line in text.splitlines():
        if code in line:
            return line.strip()
    return ""


DEEP_CASE_FAMILIES = {
    "collision": "unrelated same-name collision",
    "wrapper": "configured wrapper",
    "incompatible_reference": "incompatible reference",
    "generated_input": "generated and linked input",
    "unknown_framework": "unknown framework",
}


def compose_deep_row(profile_id, csproj_rel, tfm, track, out_dir, no_restore):
    restore_code, restore_out, build_code, build_out = restore_and_build(csproj_rel, no_restore)
    oracle_code, oracle_log, units, refs, defs = run_oracle(csproj_rel, tfm, out_dir)
    unit = units[0] if units else None

    positive_bound = any(r.get("member") == "Describe" for r in refs)
    collision_ok = any(
        r.get("member") == "Send" and r.get("receiver", "").endswith("WidgetGateway") for r in refs
    ) and any(
        r.get("member") == "Send" and r.get("receiver", "").endswith("MailQueue") for r in refs
    )
    generated_ok = any(r.get("member") == "Origin" for r in refs)
    # Calls through the wrapper's own declared type, not the shared IContract the positive
    # case already exercises -- an empty analyzer result (no caller, or a build that never
    # produced this ref) must read as not observed, not as an unconditional True.
    wrapper_ok = any(
        r.get("member") == "Describe" and r.get("receiver", "").endswith("LoggingContractWrapper")
        for r in refs
    )
    # The type must actually have been analyzed for the candidate verdict below to mean
    # anything; def_records only exists for types the oracle really processed.
    unknown_framework_analyzed = any(d.get("id", "").endswith("WidgetController") for d in defs)

    probe_rel = DEEP_INCOMPATIBLE_PROBE.get(profile_id)
    if probe_rel:
        # Always attempted, regardless of --no-restore: the probe's entire purpose is to
        # prove restore fails, so skipping it would silently report a false pass.
        probe_restore_code, probe_out, _, _ = restore_and_build(probe_rel, no_restore=False)
        incompatible_observed = probe_restore_code != 0
        incompatible_evidence = {
            "case": "incompatible_reference",
            "direction": "framework-referencing-modern",
            "restore_exit_code": probe_restore_code,
            "expected_restore_failure": True,
            "observed_restore_failure": incompatible_observed,
            "diagnostic_excerpt": extract_diagnostic(probe_out, "NU1201"),
        }
    else:
        incompatible_observed = "NU1702" in build_out
        incompatible_evidence = {
            "case": "incompatible_reference",
            "direction": "modern-referencing-framework",
            "build_exit_code": build_code,
            "expected_incompatibility_warning": True,
            "observed_incompatibility_warning": incompatible_observed,
            "diagnostic_excerpt": extract_diagnostic(build_out, "NU1702"),
        }

    case_families = {
        "collision": {"provenance": "independent", "observed": collision_ok},
        "wrapper": {"provenance": "independent", "observed": wrapper_ok},
        "incompatible_reference": dict(provenance="independent", observed=incompatible_observed, **incompatible_evidence),
        "generated_input": {"provenance": "independent", "observed": generated_ok},
        "unknown_framework": {
            "provenance": "independent",
            "observed": unknown_framework_analyzed,
            "framework_modeling": "candidate",
            "reason": "name and method-name shape only; no base type, interface or attribute ties it to a framework",
        },
    }

    context_acquisition = {
        "state": "passing" if restore_code == 0 else "failing",
        "sdk": observed_sdk_version(),
        "sdk_pin": SDK_VERSION,
        "project_format": "sdk-style",
        "reference_source": "microsoft.netframework.referenceassemblies@1.0.3"
        if track == FRAMEWORK_TRACK
        else "sdk-implicit",
        "restore_exit_code": restore_code,
        "build_exit_code": build_code,
        "expected_documents": ["PositiveCase.cs", "Collision.cs", "Wrapper.cs", "UnknownFramework.cs", "Generated.cs"],
        "loaded_documents": sorted(unit["files"]) if unit else [],
    }
    semantic_conformance = {
        "state": "passing" if positive_bound and collision_ok and generated_ok and wrapper_ok and unknown_framework_analyzed else "failing",
        "positive_case_bound": positive_bound,
        "case_families": case_families,
    }
    framework_modeling = {
        "state": "not-claimed",
        "reason": "no framework adapter is exercised by this deep-case bundle; the unknown-framework"
        " case stays candidate on name evidence alone (see case_families.unknown_framework)",
    }
    passing = (
        restore_code == 0
        and positive_bound
        and collision_ok
        and generated_ok
        and wrapper_ok
        and unknown_framework_analyzed
        and (incompatible_evidence.get("observed_restore_failure", True) if probe_rel
             else incompatible_evidence.get("observed_incompatibility_warning", True))
    )
    unsupported_state = {
        "state": "passing" if passing else "failing",
        "reason": "every deep-case family produced its expected evidence"
        if passing
        else "at least one deep-case family did not match its expected evidence",
    }
    execution_assumptions = {"state": "static-only"}

    return {
        "profile_id": profile_id,
        "kind": "deep",
        "track": track,
        "tfm": tfm,
        "context_acquisition": context_acquisition,
        "semantic_conformance": semantic_conformance,
        "framework_modeling": framework_modeling,
        "unsupported_state": unsupported_state,
        "execution_assumptions": execution_assumptions,
    }


def compose_tfm_not_supplied_control(out_dir, no_restore):
    csproj_rel = "controls/tfm-not-supplied/Control.csproj"
    declared_tfms = declared_tfm(TREE / csproj_rel)
    restore_code, restore_out, build_code, build_out = restore_and_build(csproj_rel, no_restore)
    # Requests a TFM neither variant declares -- the exact value that makes the
    # existing loader fall back to keeping the first variant instead of failing.
    oracle_code, oracle_log, units, refs, _defs = run_oracle(csproj_rel, "net6.0", out_dir)
    unit = units[0] if units else None
    kept_first_variant = "keeping net8.0" in oracle_log
    substitution_occurred = kept_first_variant and unit is not None and unit.get("tfm") != "net6.0"

    # Proves the two variants stay distinct compilations, not by reading the fixture's own
    # source text, but by requesting each declared variant explicitly and checking that the
    # oracle returns that exact variant back -- unlike the unmatched request above, which it
    # does not.
    variant_evidence = {}
    for declared in declared_tfms:
        _c, _l, v_units, _r, _d = run_oracle(csproj_rel, declared, out_dir / f"variant-{declared}")
        v_unit = v_units[0] if v_units else None
        variant_evidence[declared] = {
            "requested": declared,
            "returned_tfm": v_unit.get("tfm") if v_unit else None,
            "matches_request": v_unit is not None and v_unit.get("tfm") == declared,
        }
    variants_stay_distinct = (
        len(declared_tfms) >= 2
        and all(e["matches_request"] for e in variant_evidence.values())
        and len({e["returned_tfm"] for e in variant_evidence.values()}) == len(declared_tfms)
    )

    return {
        "profile_id": "control-tfm-not-supplied",
        "kind": "control",
        "track": "control",
        "tfm": "net6.0 (requested; not declared by either variant)",
        "context_acquisition": {
            "state": "passing" if restore_code == 0 else "failing",
            "sdk": observed_sdk_version(),
            "sdk_pin": SDK_VERSION,
            "declared_tfms": declared_tfms,
            "requested_tfm": "net6.0",
            "restore_exit_code": restore_code,
            "build_exit_code": build_code,
        },
        "semantic_conformance": {
            "state": "failing",
            "oracle_reported_status": unit.get("status") if unit else None,
            "oracle_reported_tfm": unit.get("tfm") if unit else None,
            "substitution_occurred": substitution_occurred,
            "variant_evidence": variant_evidence,
            "variants_stay_distinct_compilations": variants_stay_distinct,
            "note": "the oracle's own unit status is not trusted for this row; substitution is"
            " independently confirmed from its stderr variant-selection line and the tfm it kept,"
            " and the two declared variants are proven distinct by explicitly requesting each one"
            " and observing it return, not by reading the fixture's declared TFMs as text",
        },
        "framework_modeling": {"state": "not-claimed", "reason": "not applicable to a substitution control"},
        "unsupported_state": {
            "state": "failing" if substitution_occurred else "passing",
            "reason": "the loader kept the first declared variant for an unmatched --tfm instead of"
            " failing or reporting no result, reproducing the documented substitution defect"
            if substitution_occurred
            else "no substitution observed on this run",
        },
        "execution_assumptions": {"state": "static-only"},
    }


def compose_reference_tfm_mismatch_control(out_dir, no_restore):
    p_rel = "controls/reference-tfm-mismatch/P/P.csproj"
    q_rel = "controls/reference-tfm-mismatch/Q/Q.csproj"
    p_tfm = declared_tfm(TREE / p_rel)
    q_tfm = declared_tfm(TREE / q_rel)
    mismatch = p_tfm != q_tfm

    restore_code, restore_out, build_code, build_out = restore_and_build(p_rel, no_restore)
    # Unlike the earlier version of this control, the unit is actually observed: dotnet
    # restore/build reject this reference outright (NU1201), but the oracle's own project
    # loading is a separate path (MSBuildWorkspace evaluation, not the SDK's restore/build
    # targets) and can still report a status for P independent of whether the SDK build
    # succeeded -- which is exactly what must be checked, not assumed.
    oracle_code, oracle_log, units, refs, _defs = run_oracle(p_rel, p_tfm[0] if p_tfm else None, out_dir)
    p_unit = next((u for u in units if u.get("name") == "P"), None)
    reports_healthy_unit = p_unit is not None and p_unit.get("status") == "ok"
    diagnostic = extract_diagnostic(restore_out, "NU1201") or extract_diagnostic(build_out, "NU1201")

    return {
        "profile_id": "control-reference-tfm-mismatch",
        "kind": "control",
        "track": "control",
        "tfm": p_tfm[0] if p_tfm else "",
        "context_acquisition": {
            "state": "failing",
            "sdk": observed_sdk_version(),
            "sdk_pin": SDK_VERSION,
            "p_declared_tfm": p_tfm,
            "q_declared_tfm": q_tfm,
            "restore_exit_code": restore_code,
            "build_exit_code": build_code,
        },
        "semantic_conformance": {
            "state": "failing",
            "reference_tfm_mismatch": mismatch,
            "oracle_produced_unit_for_p": p_unit is not None,
            "oracle_reported_unit_status": p_unit.get("status") if p_unit else None,
            "oracle_reported_unit_diagnostics": p_unit.get("diagnostics") if p_unit else None,
            "note": "observed from an actual restore/build/oracle run over P's own project, not"
            " assumed from declared-TFM XML alone: dotnet restore/build reject the mismatched"
            " reference outright (NU1201), while the oracle's independent project-loading path"
            " still reports P's unit as a healthy 'ok' regardless of that rejection",
            "diagnostic_excerpt": diagnostic,
        },
        "framework_modeling": {"state": "not-claimed", "reason": "not applicable to a substitution control"},
        "unsupported_state": {
            "state": "failing" if (mismatch and reports_healthy_unit) else "passing",
            "reason": "P declares a different TargetFramework than the project it references, and"
            " the oracle nonetheless reports a healthy unit for it -- a unit in this shape must"
            " never report a healthy result from that reference alone"
            if (mismatch and reports_healthy_unit)
            else "no mismatch observed on this run, or the oracle correctly reported the unit unhealthy",
        },
        "execution_assumptions": {"state": "static-only"},
    }


def canonical_json(row):
    return json.dumps(row, indent=2, sort_keys=True) + "\n"


def clean_tree_build_state():
    """Removes every obj/ and bin/ under the qualification tree once, before any row runs.
    A per-project clean is not enough on its own: a referenced project (for example the
    substitution control's own Q, or a deep-case bundle's Generator) can carry stale state
    from an earlier row's build even when the referencing project's own directory is clean."""
    for name in ("obj", "bin"):
        for target in TREE.rglob(name):
            if target.is_dir():
                shutil.rmtree(target, ignore_errors=True)


def compose_all(no_restore):
    if not no_restore:
        clean_tree_build_state()
    rows = {}
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        for profile_id, csproj_rel, tfm, track, bind_expected in PROFILE_ROWS:
            rows[profile_id] = compose_profile_row(
                profile_id, csproj_rel, tfm, track, bind_expected, tmp_path / profile_id, no_restore
            )
        for profile_id, csproj_rel, tfm, track in DEEP_ROWS:
            rows[profile_id] = compose_deep_row(profile_id, csproj_rel, tfm, track, tmp_path / profile_id, no_restore)
        rows["control-tfm-not-supplied"] = compose_tfm_not_supplied_control(
            tmp_path / "control-tfm-not-supplied", no_restore
        )
        rows["control-reference-tfm-mismatch"] = compose_reference_tfm_mismatch_control(
            tmp_path / "control-reference-tfm-mismatch", no_restore
        )
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true", help="regenerate and diff against committed rows")
    mode.add_argument("--write", action="store_true", help="regenerate and overwrite committed rows")
    parser.add_argument("--no-restore", action="store_true", help="skip dotnet restore (already restored)")
    args = parser.parse_args()

    rows = compose_all(args.no_restore)

    RESULTS.mkdir(parents=True, exist_ok=True)
    if args.write:
        for profile_id, row in rows.items():
            (RESULTS / f"{profile_id}.json").write_text(canonical_json(row), encoding="utf-8")
        print(f"wrote {len(rows)} row(s) to {RESULTS}")
        return 0

    mismatches = []
    for profile_id, row in rows.items():
        committed_path = RESULTS / f"{profile_id}.json"
        regenerated = canonical_json(row)
        if not committed_path.exists():
            mismatches.append(f"{profile_id}: no committed snapshot")
            continue
        committed = committed_path.read_text(encoding="utf-8")
        if committed != regenerated:
            mismatches.append(f"{profile_id}: regenerated output differs from the committed snapshot")

    if mismatches:
        print("qualify-dotnet-targets --check: drift detected", file=sys.stderr)
        for m in mismatches:
            print(f"  {m}", file=sys.stderr)
        return 1

    print(f"qualify-dotnet-targets --check: {len(rows)} row(s) match their committed snapshot")
    return 0


if __name__ == "__main__":
    sys.exit(main())
