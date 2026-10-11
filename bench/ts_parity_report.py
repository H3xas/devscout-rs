"""Render the TypeScript reference parity results document from the scores alone.

Standard library only. Every section the pre-registration or the gate defines is generated
from two score files (the pre-registered scorer's and the current scorer's, on the same stored
arm outputs) and the pre-registered files; the only other inputs are the run date and the two
devscout commits. Rendering the same inputs again writes the same bytes, so the committed
document can be checked by rerendering it.

usage: python3 -B bench/ts_parity_report.py <bench truth dir> <scores before> <scores after> <out.md>
           <run date> <devscout-base commit> <devscout-head commit>
"""

import re
import sys
from collections import defaultdict
from pathlib import Path

import ts_parity as tp

GATED_ARM = "devscout-head"
PEER_ARMS = ("crg",)
DATE_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
CODE_SUFFIXES = (".py", ".sh")
SCORER_ENTRY = "../../ts_parity.py"
ANSWER_CLASSES = ("failed answer", "no row for any target")

# The scorer changes made after the pre-registered scorer, each disclosed with the figures
# both scorers compute on the same stored outputs.
SCORER_FIXES = (
    ("`relative()`", "a peer answer path outside the corpus root is refused instead of kept as spelled"),
    ("`load_answers()`", "a run that exits outside its arm's answer codes, or whose payload status is not an "
                         "answer, answers nothing"),
    ("`gate()`", "an impact check whose answer is missing or failed fails instead of being dropped"),
    ("`registered_answers()`", "a registered invocation missing from the record is scored as failed"),
    ("`run_record_problems()`", "a record of another arm, another corpus revision, or an unregistered or "
                                "repeated invocation is refused"),
    ("`evidence()`", "the run summary, the impact answer totals and the miss and extra classes are added; "
                     "no figure or gate check reads them"),
)

BANNER = """> **Single run.** One repetition per cell. Recall and precision here are counts over a fixed, sealed
> truth set and do not vary between repetitions of the same binaries, but setup times are one
> measurement each."""

QUESTION = """**Question.** Do `devscout refs`, `tests` and `impact` answer for TypeScript symbols, before and
after the change that makes them read the TypeScript edge kinds, and how does a TypeScript-capable
peer answer the same questions on the same corpus? The seed rule, truth, adjudication, metrics,
gate, arms, isolation rules and predictions are fixed in
[`bench/truth/typescript/preregistration.md`](../../../bench/truth/typescript/preregistration.md)."""

RERUN = """## Rerunning

The steps are in
[`methodology.md` § TypeScript reference parity](../methodology.md#typescript-reference-parity)."""


# ---- formatting ------------------------------------------------------------------------


def code(text):
    return f"`{text}`"


def files_cell(paths):
    return "<br>".join(code(path) for path in paths) if paths else "none"


def fixed(value):
    return "n/a" if value is None else f"{value:.3f}"


def recall_cell(entry):
    if entry.get("failed"):
        return f"failed, 0/{entry['truth']}"
    return f"{entry['found']}/{entry['truth']} ({fixed(entry['recall'])})"


def pooled_text(block):
    failed = any(entry.get("failed") for entry in block["perSeed"].values())
    return f"{block['found']}/{block['truth']} ({fixed(block['pooled'])})" + (", a seed failed" if failed else "")


def pooled_cell(block):
    return f"**{pooled_text(block)}**"


def precision_cell(block):
    if block is None or block.get("failed"):
        return "failed"
    if not block["rows"]:
        return "n/a (no rows)"
    return f"{fixed(block['precision'])} ({block['correct']}/{block['rows']})"


def table(header, rows):
    def cell(value):
        return str(value).replace("|", "\\|")

    lines = ["| " + " | ".join(header) + " |", "| " + " | ".join("---" for _ in header) + " |"]
    lines += ["| " + " | ".join(cell(value) for value in row) + " |" for row in rows]
    return "\n".join(lines)


def target_label(key):
    return "diff list" if key == "diff" else code(key.split(":", 1)[1])


# ---- inputs ----------------------------------------------------------------------------


def corpus_entry(lock_path, corpus_id):
    """The `key = "value"` fields of one corpus block of bench/corpus.lock."""
    fields, inside = {}, False
    for line in Path(lock_path).read_text(encoding="utf-8").splitlines():
        if line.startswith("["):
            inside = line.strip() == f"[{corpus_id}]"
            continue
        match = re.match(r'^(\w+) = "(.*)"$', line)
        if inside and match:
            fields[match.group(1)] = match.group(2)
    return fields


def predictions_table(preregistration):
    """(arm, metric, prediction, falsified if) for every row of the pre-registered Predictions table."""
    section = preregistration.split("\n## Predictions\n", 1)[1].split("\n## ", 1)[0]
    rows = [line for line in section.splitlines() if line.startswith("|")][2:]
    found = []
    for line in rows:
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        found.append((cells[0].strip("`"), cells[1], cells[2], cells[3]))
    return found


def manifest_rows(bench):
    recorded = tp.read_json(bench / "manifest.json")["files"]
    rows = []
    for name, digest in sorted(recorded.items()):
        path = bench / name
        actual = tp.sha256_file(path) if path.is_file() else None
        kind = "code" if name.endswith(CODE_SUFFIXES) else "registered input"
        rows.append((name, kind, digest, "matches" if actual == digest else "differs"))
    return rows


def input_problems(bench, before, after, arms, date, base, head):
    problems = []
    if not DATE_RE.match(date):
        problems.append(f"the run date {date} is not YYYY-MM-DD")
    for label, commit in (("devscout-base", base), ("devscout-head", head)):
        if not COMMIT_RE.match(commit):
            problems.append(f"the {label} commit {commit} is not a full commit id")
    for label, scores in (("before", before), ("after", after)):
        missing = [arm for arm in arms if arm not in scores]
        if missing:
            problems.append(f"the {label} scores lack the registered arms {', '.join(missing)}")
    problems += [f"the after scores of {arm} carry no evidence" for arm in arms
                 if arm in after and "evidence" not in after[arm]]
    if SCORER_ENTRY not in tp.read_json(bench / "manifest.json")["files"]:
        problems.append(f"the manifest has no entry for the pre-registered scorer {SCORER_ENTRY}")
    problems += [f"the registered input {name} differs from its manifest hash"
                 for name, kind, _, status in manifest_rows(bench) if kind != "code" and status != "matches"]
    return problems


# ---- registered figures ----------------------------------------------------------------


def registered_figures(scores, arms, seeds):
    """Every registered figure and gate check, labelled, in a fixed order."""
    names = [seed["name"] for seed in sorted(seeds["seeds"], key=lambda seed: seed["slot"])]
    targets = sorted(tp.target_lists(seeds))
    figures = []
    for arm in arms:
        for truth in ("sealed", "raw"):
            block = scores[arm][truth]
            for task in ("refs", "tests"):
                for name in names:
                    figures.append((f"{arm} {task} recall, {truth}, {name}", recall_cell(block[task]["perSeed"][name])))
                figures.append((f"{arm} {task} recall, {truth}, pooled", pooled_text(block[task])))
            for key in targets:
                figures.append((f"{arm} impact precision depth 1, {truth}, {key}",
                                precision_cell(block["impact"].get("impact", {}).get(key))))
        for check, met in sorted(scores[arm]["gate"].items()):
            figures.append((f"{arm} gate: {check}", "met" if met else "missed"))
    return figures


# ---- predictions -----------------------------------------------------------------------


def any_row(block):
    return any(entry["found"] or entry["extra"] for entry in block["perSeed"].values())


def impact_blocks(scores, seeds):
    return [(key, scores["impact"].get("impact", {}).get(key)) for key in sorted(tp.target_lists(seeds))]


def precision_of(block):
    return None if block is None or block.get("failed") or not block["rows"] else block["precision"]


def impact_actual(sealed, seeds):
    return "; ".join(f"{target_label(key)} {precision_cell(block)}" for key, block in impact_blocks(sealed, seeds))


def precisions(sealed, seeds):
    return {key: precision_of(block) for key, block in impact_blocks(sealed, seeds)}


def outside(pooled, low, high):
    return pooled is None or pooled < low or pooled > high


# Each registered prediction's falsifier, read from the sealed scores: (actual, falsified).
PREDICTION_RULES = {
    ("devscout-base", "refs, tests recall"): lambda s, seeds: (
        f"refs {pooled_cell(s['refs'])}, tests {pooled_cell(s['tests'])}", any_row(s["refs"]) or any_row(s["tests"])),
    ("devscout-base", "impact precision, depth 1"): lambda s, seeds: (
        impact_actual(s, seeds),
        any(block and (block["rows"] or block["outsideUniverse"]) for _, block in impact_blocks(s, seeds))),
    ("devscout-head", "refs recall"): lambda s, seeds: (
        pooled_cell(s["refs"]), outside(s["refs"]["pooled"], 0.55, 0.75)),
    ("devscout-head", "refs gate (≥ 0.9)"): lambda s, seeds: (
        pooled_cell(s["refs"]), (s["refs"]["pooled"] or 0) >= 0.9),
    ("devscout-head", "tests recall"): lambda s, seeds: (
        pooled_cell(s["tests"]), outside(s["tests"]["pooled"], 0.2, 0.5)),
    ("devscout-head", "tests gate (≥ 0.9)"): lambda s, seeds: (
        pooled_cell(s["tests"]), (s["tests"]["pooled"] or 0) >= 0.9),
    ("devscout-head", "impact precision, depth 1"): lambda s, seeds: (
        impact_actual(s, seeds), any(value is None or value < 0.5 for value in precisions(s, seeds).values())),
    ("crg", "refs recall (callers and inheritors)"): lambda s, seeds: (
        pooled_cell(s["refs"]), (s["refs"]["pooled"] or 0) > 0.35),
    ("crg", "tests recall"): lambda s, seeds: (pooled_cell(s["tests"]), (s["tests"]["pooled"] or 0) > 0.5),
    ("crg", "impact precision, depth 1"): lambda s, seeds: (
        impact_actual(s, seeds),
        (precisions(s, seeds).get("diff") or 0) < 0.5
        or all((value or 0) >= 0.5 for key, value in precisions(s, seeds).items() if key != "diff")),
}


# ---- sections --------------------------------------------------------------------------


def gate_section(after, seeds):
    gate = after[GATED_ARM]["gate"]
    sealed = after[GATED_ARM]["sealed"]
    rows = [("pooled `refs` recall", "≥ 0.9", pooled_cell(sealed["refs"]), gate["refs recall >= 0.9"]),
            ("pooled `tests` recall", "≥ 0.9", pooled_cell(sealed["tests"]), gate["tests recall >= 0.9"])]
    for key, block in impact_blocks(sealed, seeds):
        rows.append((f"depth-1 `impact` precision, {target_label(key)}", "≥ 0.5", precision_cell(block),
                     gate[f"impact precision >= 0.5 on {key}"]))
    verdict = "met" if all(gate.values()) else "missed"
    lines = ["## Gate", "", f"**Gate verdict ({code(GATED_ARM)}): {verdict}.**", "",
             table(("Check", "Threshold", code(GATED_ARM), "Result"),
                   [(check, threshold, value, "met" if met else "**missed**") for check, threshold, value, met in rows])]
    return "\n".join(lines)


def environment_section(bench, seeds, after, arms, base, head):
    corpus = corpus_entry(bench / ".." / ".." / "corpus.lock", "typescript")
    lines = [
        f"Corpus          {corpus.get('repo', '')} {seeds['pin']} ({corpus.get('license', '')}), bench/corpus.lock [typescript]",
        f"devscout-base   built from {base}, cargo build --release --locked",
        f"devscout-head   built from {head}, cargo build --release --locked",
        f"crg             {tp.PEER_PACKAGE} {tp.PEER_VERSION} from PyPI, bench/peers/code-review-graph/requirements.txt",
        "Reps            1 per cell",
    ]
    setup_rows = []
    for arm in arms:
        for step in after[arm]["evidence"]["setup"]:
            seconds = "n/a" if step["seconds"] is None else f"{step['seconds']:.1f}"
            setup_rows.append((code(arm), code(step["verb"]), step["exit"], seconds))
    return "\n".join(["## Environment", "", "```", *lines, "```", "", "Setup cost, information only:", "",
                      table(("Arm", "Setup step", "Exit", "Wall time (s)"), setup_rows)])


def invocations_section(bench, after, arms):
    templates = tp.read_templates(bench / "templates.tsv")
    template_rows = [(code(row["arm"]), code(row["task"]), row["target"], code(" ".join(row["argv"])))
                     for row in templates]
    exit_rows = []
    for arm in arms:
        evidence = after[arm]["evidence"]
        for task in sorted(evidence["exits"]):
            exits = ", ".join(f"{status} ×{count}" for status, count in sorted(evidence["exits"][task].items()))
            statuses = ", ".join(f"{status} ×{count}" for status, count in
                                 sorted(evidence["statuses"].get(task, {}).items())) or "n/a"
            exit_rows.append((code(arm), code(task), sum(evidence["exits"][task].values()), exits, statuses))
    return "\n".join([
        "## Arms and invocations", "",
        "The runner ([`bench/ts-parity.sh`](../../../bench/ts-parity.sh)) expands the hashed templates in "
        "[`bench/truth/typescript/templates.tsv`](../../../bench/truth/typescript/templates.tsv):", "",
        table(("Arm", "Task", "Target", "Template"), template_rows), "",
        "Every stored invocation, by exit code and payload status:", "",
        table(("Arm", "Task", "Invocations", "Exit codes", "Payload statuses"), exit_rows)])


def seeds_section(seeds):
    seed_rows = [(seed["slot"], f"{code(seed['name'])} ({seed['keyword']})", code(seed["file"]))
                 for seed in sorted(seeds["seeds"], key=lambda seed: seed["slot"])]
    target_rows = [(target_label(key), files_cell(files)) for key, files in sorted(tp.target_lists(seeds).items())]
    return "\n".join(["## Seeds", "", table(("Slot", "Seed", "Declared in"), seed_rows), "",
                      table(("Impact target", "Files"), target_rows)])


def recall_section(task, after, arms, seeds):
    names = [seed["name"] for seed in sorted(seeds["seeds"], key=lambda seed: seed["slot"])]
    parts = [f"## `{task}` recall"]
    for truth, label in (("sealed", "Against the sealed truth:"), ("raw", "Against the raw ripgrep truth:")):
        rows = []
        for name in names:
            entry = after[arms[0]][truth][task]["perSeed"][name]
            rows.append([code(name), entry["truth"]] + [recall_cell(after[arm][truth][task]["perSeed"][name])
                                                        for arm in arms])
        rows.append(["**pooled**", after[arms[0]][truth][task]["truth"]] +
                    [pooled_cell(after[arm][truth][task]) for arm in arms])
        parts += ["", label, "", table(["Seed", "Truth files"] + [code(arm) for arm in arms], rows)]
    if task == "refs":
        widened = [arm for arm in arms if "refsWithFileImporters" in after[arm]["sealed"]]
        rows = []
        for arm in widened:
            for truth in ("sealed", "raw"):
                rows.append((code(arm), truth, pooled_cell(after[arm][truth]["refsWithFileImporters"])))
        if rows:
            parts += ["", "Refs widened by file importers (`importers_of` on the same target), information only:",
                      "", table(("Arm", "Truth", "Pooled recall"), rows)]
    return "\n".join(parts)


def impact_section(after, arms, seeds):
    parts = ["## `impact` precision at depth 1"]
    keys = sorted(tp.target_lists(seeds))
    for task, truths, depth, note in (("impact", ("sealed", "raw"), "depth 1", ""),
                                      ("impact-default", ("sealed",), "default depth", ", information only")):
        label = depth + note
        for truth in truths:
            rows = []
            for key in keys:
                truth_count = after[arms[0]][truth]["impact"][task][key]["truth"] if \
                    key in after[arms[0]][truth]["impact"].get(task, {}) else "n/a"
                rows.append([target_label(key), truth_count] +
                            [precision_cell(after[arm][truth]["impact"].get(task, {}).get(key)) for arm in arms])
            parts += ["", f"Precision (correct/rows), {label}, against the {truth} truth:", "",
                      table(["Target", "Truth files"] + [code(arm) for arm in arms], rows)]
        total_rows, outside_rows, recall_rows = [], [], []
        for arm in arms:
            for key in keys:
                totals = after[arm]["evidence"]["totals"].get(task, {}).get(key)
                block = after[arm]["sealed"]["impact"].get(task, {}).get(key)
                if totals:
                    total_rows.append((code(arm), target_label(key), totals["calls"], totals["unit"],
                                       totals["reported"], totals["returned"], totals["omitted"],
                                       "yes" if totals["truncated"] else "no"))
                if block is not None:
                    outside_rows.append((code(arm), target_label(key), files_cell(block["outsideUniverse"])))
                    recall_rows.append((code(arm), target_label(key),
                                        f"{block['correct']}/{block['truth']} ({fixed(block['recall'])})"))
        parts += ["", f"Answer totals, {label}: what each tool reports, returns and leaves out, summed over "
                      "the answer's calls:", "",
                  table(("Arm", "Target", "Calls", "Unit", "Reported", "Returned", "Omitted", "Truncated"),
                        total_rows),
                  "", f"Rows outside the `.ts`/`.tsx` universe, {label}, listed apart and not counted:", "",
                  table(("Arm", "Target", "Files"), outside_rows),
                  "", f"Recall against the sealed truth, {depth}, information only:", "",
                  table(("Arm", "Target", "Recall (correct/truth)"), recall_rows)]
    return "\n".join(parts)


def predictions_section(preregistration, after, seeds):
    rows = []
    for arm, metric, predicted, falsifier in predictions_table(preregistration):
        rule = PREDICTION_RULES.get((arm, metric))
        if rule is None or arm not in after:
            raise SystemExit(f"no rule reads the registered prediction {arm} / {metric}")
        actual, falsified = rule(after[arm]["sealed"], seeds)
        rows.append((code(arm), metric, predicted, falsifier, actual, "**falsified**" if falsified else "held"))
    return "\n".join(["## Predictions against actual", "",
                      table(("Arm", "Metric", "Predicted", "Falsified if", "Actual", "Verdict"), rows)])


def misses_section(after, arms, seeds):
    names = [seed["name"] for seed in sorted(seeds["seeds"], key=lambda seed: seed["slot"])]
    miss_rows, extra_rows, impact_extra_rows = [], [], []
    impact_miss = defaultdict(list)
    for arm in arms:
        classes = after[arm]["evidence"]["classes"]
        for task in ("refs", "tests"):
            for name in names:
                entry = classes[task][name]
                miss_rows += [(code(arm), task, code(name), code(path), cls) for path, cls in sorted(entry["missed"].items())]
                extra_rows += [(code(arm), task, code(name), code(path), cls) for path, cls in sorted(entry["extra"].items())]
        for key in sorted(classes["impact"]):
            entry = classes["impact"][key]
            impact_extra_rows += [(code(arm), target_label(key), code(path), cls)
                                  for path, cls in sorted(entry["extra"].items())]
            for path, cls in sorted(entry["missed"].items()):
                impact_miss[(arm, key, cls)].append(path)
    order = sorted(impact_miss, key=lambda item: (arms.index(item[0]), item[1], item[2]))
    tally_rows = [(code(arm), target_label(key), cls, len(impact_miss[(arm, key, cls)])) for arm, key, cls in order]
    # Arms that miss the same files for the same class share one row, and an answer-level class
    # misses the whole truth, so its files are not repeated.
    shared = defaultdict(list)
    for arm, key, cls in order:
        shared[(key, cls, tuple(impact_miss[(arm, key, cls)]))].append(arm)
    detail_rows = [(", ".join(code(arm) for arm in arms_), target_label(key), cls,
                    f"every truth file of the target ({len(paths)})" if cls in ANSWER_CLASSES else files_cell(paths))
                   for (key, cls, paths), arms_ in sorted(shared.items())]
    return "\n".join([
        "## Misses and extras, classified", "",
        "Every class is computed by `evidence()` in [`bench/ts_parity.py`](../../../bench/ts_parity.py) from "
        "the stored answers and the corpus at the pin.", "",
        "### `refs` and `tests` misses against the sealed truth", "",
        table(("Arm", "Task", "Seed", "Missed file", "Class"), miss_rows) if miss_rows else "none", "",
        "### `refs` and `tests` extras: answered files outside the sealed truth", "",
        table(("Arm", "Task", "Seed", "Extra file", "Class"), extra_rows) if extra_rows else "none", "",
        "### Depth-1 `impact` extras: answered rows outside the sealed truth", "",
        table(("Arm", "Target", "Extra file", "Class"), impact_extra_rows) if impact_extra_rows else "none", "",
        "### Depth-1 `impact` misses against the sealed truth", "",
        table(("Arm", "Target", "Class", "Files"), tally_rows) if tally_rows else "none", "",
        "<details><summary>Every depth-1 <code>impact</code> miss, by class</summary>", "",
        table(("Arm", "Target", "Class", "Missed files"), detail_rows) if detail_rows else "none", "",
        "</details>"])


def peers_section(after, arms):
    cells = after["wherePeersWin"]
    rows = [(cell["metric"], ", ".join(code(arm) for arm in cell["leaders"]) or "none (no truth)",
             *[fixed(cell["values"].get(arm)) for arm in arms]) for cell in cells]
    peer_rows = [(cell["metric"], ", ".join(code(arm) for arm in cell["leaders"]),
                  *[fixed(cell["values"].get(arm)) for arm in arms])
                 for cell in cells if any(arm in cell["leaders"] for arm in PEER_ARMS)]
    return "\n".join([
        "## Where peers win", "",
        "Generated from the scorer's `wherePeersWin` output: every metric cell, the arm or arms leading it, "
        "and each arm's value.", "",
        table(["Metric cell", "Leading arm(s)"] + [code(arm) for arm in arms], rows), "",
        "Cells a peer leads or ties:", "",
        table(["Metric cell", "Leading arm(s)"] + [code(arm) for arm in arms], peer_rows) if peer_rows else "none"])


def manifest_section(bench, seeds):
    corpus = corpus_entry(bench / ".." / ".." / "corpus.lock", "typescript")
    rows = [(code(name), kind, code(digest[:16]), status) for name, kind, digest, status in manifest_rows(bench)]
    pin_status = "matches" if corpus.get("sha") == seeds["pin"] else "differs"
    return "\n".join([
        "## Manifest check", "",
        "Every file `bench/truth/typescript/manifest.json` hashed at pre-registration, checked against the "
        "file as rendered. A code entry may change by a disclosed scorer fix; a registered input may not.", "",
        table(("File", "Kind", "Manifest sha256", "Status"), rows), "",
        table(("Pin", "Registered", "Status"),
              [("`bench/corpus.lock` `[typescript]` `sha`", code(seeds["pin"]), pin_status)])])


def disclosure_section(bench, before, after, arms, seeds):
    first = dict(registered_figures(before, arms, seeds))
    second = registered_figures(after, arms, seeds)
    rows = [(label, first.get(label, "absent"), value, "equal" if first.get(label) == value else "**differs**")
            for label, value in second]
    equal = sum(1 for row in rows if row[3] == "equal")
    return "\n".join([
        "## Scorer-fix disclosure", "",
        f"Scored before: `bench/ts_parity.py` as pre-registered (its manifest sha256 "
        f"{code(tp.read_json(bench / 'manifest.json')['files'][SCORER_ENTRY][:16])}). "
        f"Scored after: the committed scorer (sha256 {code(tp.sha256_file(Path(tp.__file__))[:16])}). "
        "Both read the same stored arm outputs with the same arguments.", "",
        table(("Fix", "Change"), SCORER_FIXES), "",
        f"**Registered figures and gate checks equal before and after: {equal} of {len(rows)}.**", "",
        "<details><summary>Every registered figure and gate check, before and after</summary>", "",
        table(("Figure", "Before", "After", "Status"), rows), "", "</details>"])


def render(bench_dir, before, after, date, base, head):
    bench = Path(bench_dir)
    seeds = tp.read_json(bench / "seeds.json")
    arms = []
    for row in tp.read_templates(bench / "templates.tsv"):
        if row["arm"] not in arms:
            arms.append(row["arm"])
    problems = input_problems(bench, before, after, arms, date, base, head)
    if problems:
        raise SystemExit("refused: " + "; ".join(problems))
    preregistration = (bench / "preregistration.md").read_text(encoding="utf-8")
    sections = [
        f"# TypeScript reference parity — {date}", BANNER, QUESTION,
        gate_section(after, seeds),
        environment_section(bench, seeds, after, arms, base, head),
        invocations_section(bench, after, arms),
        seeds_section(seeds),
        recall_section("refs", after, arms, seeds),
        recall_section("tests", after, arms, seeds),
        impact_section(after, arms, seeds),
        predictions_section(preregistration, after, seeds),
        misses_section(after, arms, seeds),
        peers_section(after, arms),
        manifest_section(bench, seeds),
        disclosure_section(bench, before, after, arms, seeds),
        RERUN,
    ]
    return "\n\n".join(sections) + "\n"


def main(argv):
    if len(argv) != 8:
        print(__doc__.split("usage: ", 1)[1], file=sys.stderr)
        return 2
    bench_dir, before_path, after_path, out, date, base, head = argv[1:]
    text = render(bench_dir, tp.read_json(before_path), tp.read_json(after_path), date, base, head)
    Path(out).write_text(text, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
