#!/bin/sh
# Measures the first-read offer against the same read with a mismatched disk digest.
# Preparation and output validation run outside the timed command.
set -eu

if [ "$#" -gt 1 ]; then
	echo "usage: $0 [binary]" >&2
	exit 2
fi
for tool in hyperfine git python3; do
	if ! command -v "$tool" >/dev/null 2>&1; then
		echo "missing tool: $tool" >&2
		exit 2
	fi
done
repo_root=$(cd "$(dirname "$0")/.." && pwd)
work="$repo_root/bench/out/hook-latency"
rounds=200
warmup=20
delta_budget_ms=5
absolute_budget_ms=30
rm -rf "$work"
mkdir -p "$work"
if [ "$#" -eq 0 ]; then
	if ! (cd "$repo_root" && cargo build --release --locked >"$work/build.log" 2>&1); then
		echo "FAIL: release build; details in bench/out/hook-latency/build.log" >&2
		exit 1
	fi
	binary="$repo_root/target/release/devscout"
else
	binary=$(python3 -I -c 'import pathlib,sys; print(pathlib.Path(sys.argv[1]).resolve())' "$1")
fi
if [ ! -x "$binary" ]; then
	echo "binary is not executable" >&2
	exit 2
fi
cat >"$work/run.py" <<'PY'
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import shlex
import shutil
import statistics
import subprocess
import sys
import uuid

work = Path(__file__).resolve().parent
root = work / "repo"
state = work / "state"
indexed = "namespace App.Widgets\n{\n    public interface IWidget\n    {\n        void Render();\n    }\n}\n"
implementation = "using App.Widgets;\n\nnamespace App.Widgets.Impl\n{\n    public class WidgetImpl : IWidget\n    {\n        public void Render() { }\n    }\n}\n"
suppressed = indexed.replace("Render", "Rendez")
source = root / "src/IWidget.cs"

def environment():
    return dict(os.environ, HOME=str(state), SCOUT_REGISTRY=str(state / "repos.json"),
                SCOUT_CONTENT_DB=str(state / "content.db"),
                GIT_AUTHOR_NAME="devscout-benchmark", GIT_COMMITTER_NAME="devscout-benchmark",
                GIT_AUTHOR_EMAIL="benchmark@example.com", GIT_COMMITTER_EMAIL="benchmark@example.com")

def run(args):
    return subprocess.check_output(args, cwd=root, env=environment(), text=True).strip()

def prepare(arm):
    body = indexed if arm == "offer" else suppressed
    if source.read_text() != body:
        source.write_text(body)
    payload = {"session_id": str(uuid.uuid4()), "tool_name": "Read",
               "tool_input": {"file_path": str(source)},
               "tool_response": {"file": {"filePath": str(source), "content": indexed,
                                          "numLines": 7, "totalLines": 7, "startLine": 1}}}
    (work / "payload.json").write_text(json.dumps(payload))

def check_output(arm, response):
    if arm == "offer":
        value = json.loads(response)
        context = value.get("hookSpecificOutput", {}).get("additionalContext", "")
        valid = "this file is indexed" in context and "unchanged since" not in context
    else:
        valid = response == ""
    if not valid:
        raise ValueError(f"{arm} output did not take its required path")

def check(arm):
    check_output(arm, (work / "response.json").read_text())
    with (work / f"{arm}.validated").open("a") as out:
        out.write("ok\n")

def samples(result, expected):
    if "measurements" in result:
        if any(m.get("exit_code", 0) != 0 for m in result["measurements"]):
            raise ValueError("failed timed invocation")
        times = [m["time_wall_clock"]["value"] for m in result["measurements"]]
    else:
        times = result["times"]
    if len(times) != expected or any(not math.isfinite(t) or t <= 0 for t in times):
        raise ValueError("incomplete or invalid measurement distribution")
    return sorted(t * 1000 for t in times)

def validated(arm, count, expected):
    if count != ["ok"] * expected:
        raise ValueError(f"{arm}: incomplete validated count ({len(count)})")

def arm_name(result):
    return result.get("name", result.get("command"))

def selfcheck():
    offer = json.dumps({"hookSpecificOutput": {"additionalContext": "this file is indexed"}})
    stub = json.dumps({"hookSpecificOutput": {"additionalContext": "unchanged since first read"}})
    check_output("offer", offer)
    check_output("suppressed", "")
    failures = [("stub in offer arm", lambda: check_output("offer", stub)),
                ("stub in suppressed arm", lambda: check_output("suppressed", stub)),
                ("offer in suppressed arm", lambda: check_output("suppressed", offer)),
                ("missing offer", lambda: check_output("offer", "")),
                ("incomplete validation", lambda: validated("offer", ["ok"], 2)),
                ("non-finite time", lambda: samples({"times": [float("nan")]}, 1)),
                ("incomplete distribution", lambda: samples({"times": [0.01]}, 2))]
    for name, action in failures:
        try:
            action()
        except ValueError:
            continue
        raise ValueError(f"self-check accepted {name}")
    legacy = samples({"times": [0.02, 0.01]}, 2)
    current = samples({"measurements": [{"time_wall_clock": {"value": t}}
                                       for t in (0.02, 0.01)]}, 2)
    if legacy != [10, 20] or current != legacy:
        raise ValueError("self-check rejected an export schema")
    if (arm_name({"name": "offer", "command": "shared command"}) != "offer"
            or arm_name({"command": "suppressed"}) != "suppressed"):
        raise ValueError("self-check lost an arm label")
    print("output and export validation self-checks: PASS")

def report(path, rounds, warmup, delta_bound, absolute_bound):
    results = json.loads(Path(path).read_text())["results"]
    if len(results) != 2 or {arm_name(r) for r in results} != {"offer", "suppressed"}:
        raise ValueError("expected exactly the offer and suppressed distributions")
    p95 = {}
    for result in results:
        arm = arm_name(result)
        times = samples(result, rounds)
        count = (work / f"{arm}.validated").read_text().splitlines()
        validated(arm, count, rounds + warmup)
        if any(code != 0 for code in result.get("exit_codes", [])):
            raise ValueError(f"{arm}: failed timed invocation")
        p95[arm] = times[math.ceil(0.95 * len(times)) - 1]
        median = statistics.median(times)
        print(f"{arm}: min {times[0]:.3f}, p50 {median:.3f}, p95 {p95[arm]:.3f}, "
              f"max {times[-1]:.3f}, mean {statistics.mean(times):.3f} ms; "
              f"validated {len(count)}/{rounds + warmup}")
    delta = p95["offer"] - p95["suppressed"]
    delta_ok = delta <= delta_bound
    absolute_ok = p95["offer"] <= absolute_bound
    print(f"{'PASS' if delta_ok else 'FAIL'}: p95 delta {delta:+.3f} ms <= {delta_bound:g} ms")
    print(f"{'PASS' if absolute_ok else 'FAIL'}: offer p95 {p95['offer']:.3f} ms <= {absolute_bound:g} ms")
    return 0 if delta_ok and absolute_ok else 1

def describe(binary, repository):
    print(f"platform: {platform.system()} {platform.machine()}")
    if platform.system() == "Darwin":
        print("macOS: " + subprocess.check_output(["sw_vers", "-productVersion"], text=True).strip())
        print("CPU: " + subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"], text=True).strip())
    else:
        print(f"CPU: {platform.processor() or 'unavailable'}")
    print(f"logical CPUs: {os.cpu_count()}")
    print(f"load averages (start): {os.getloadavg()}")
    for tool, flag in (("hyperfine", "--version"), ("git", "--version"), ("rustc", "--version")):
        print(subprocess.check_output([tool, flag], text=True).strip())
    print(f"git executable: {shutil.which('git')}")
    try:
        label = str(Path(binary).relative_to(repository))
    except ValueError:
        label = "<external>/" + Path(binary).name
    print(f"binary: {label}")
    print(f"binary sha256: {hashlib.sha256(Path(binary).read_bytes()).hexdigest()}")

try:
    action = sys.argv[1]
    if action == "setup":
        selfcheck()
        binary = sys.argv[2]
        state.mkdir()
        source.parent.mkdir(parents=True)
        source.write_text(indexed)
        (source.parent / "WidgetImpl.cs").write_text(implementation)
        run(["git", "init", "-q", "-b", "main", "."])
        run(["git", "add", "src"])
        tree = run(["git", "write-tree"])
        head = run(["git", "commit-tree", tree, "-m", "fixture"])
        run(["git", "update-ref", "refs/heads/main", head])
        run([binary, "init", "--no-hooks", "--no-map"])
        run([binary, "map", "."])
        describe(binary, sys.argv[3])
    elif action == "prepare":
        prepare(sys.argv[2])
    elif action == "check":
        check(sys.argv[2])
    elif action == "quote":
        print(shlex.join(sys.argv[2:]))
    elif action == "report":
        print(f"load averages (end): {os.getloadavg()}")
        sys.exit(report(sys.argv[2], int(sys.argv[3]), int(sys.argv[4]),
                        float(sys.argv[5]), float(sys.argv[6])))
    else:
        raise ValueError("unknown action")
except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
    if isinstance(error, subprocess.CalledProcessError):
        detail = f"subprocess exited {error.returncode}"
    elif isinstance(error, OSError):
        detail = error.strerror
    else:
        detail = str(error)
    print(f"FAIL: {action}: {detail}", file=sys.stderr)
    sys.exit(1)
PY

python3 -I "$work/run.py" setup "$binary" "$repo_root"
command=$(python3 -I "$work/run.py" quote env "HOME=$work/state" \
	"SCOUT_REGISTRY=$work/state/repos.json" "SCOUT_CONTENT_DB=$work/state/content.db" \
	"$binary" -C "$work/repo" hook read)
payload=$(python3 -I "$work/run.py" quote "$work/payload.json")
response=$(python3 -I "$work/run.py" quote "$work/response.json")
command="$command < $payload > $response"
helper=$(python3 -I "$work/run.py" quote python3 -I "$work/run.py")
for arm in offer suppressed; do
	python3 -I "$work/run.py" prepare "$arm"
	sh -c "$command"
	python3 -I "$work/run.py" check "$arm"
done

measure() {
	pass="$1"
	: >"$work/offer.validated"
	: >"$work/suppressed.validated"
	hyperfine --shell=default --warmup "$warmup" --runs "$rounds" \
		--export-json "$work/$pass.json" \
		--prepare "$helper prepare offer" --conclude "$helper check offer" -n offer "$command" \
		--prepare "$helper prepare suppressed" --conclude "$helper check suppressed" -n suppressed "$command"
}
echo "settling the page cache (distribution discarded)"
if ! measure settle; then
	echo "FAIL: settle invocation or output validation" >&2
	exit 1
fi
echo "measuring first-read offers and suppressed offers"
if ! measure measured; then
	echo "FAIL: measured invocation or output validation" >&2
	exit 1
fi
python3 -I "$work/run.py" report "$work/measured.json" "$rounds" "$warmup" \
	"$delta_budget_ms" "$absolute_budget_ms"
