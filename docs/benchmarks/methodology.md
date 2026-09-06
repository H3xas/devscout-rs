# Methodology

How a benchmark run against this tool is constructed, what it measures, and how to rerun it.

## Task taxonomy

Four task kinds. A task is built from a public issue that states a **symptom without naming the
code** — no file path, no stack trace, no type name in the title — and that links to a merged
fix. The merged diff is the ground truth, recorded before any arm runs, so the target cannot
drift toward what this tool returns. The corpus is pinned to a commit predating every fix.

| Kind | Asked | Truth | Grading |
| --- | --- | --- | --- |
| `locate` | "This is the symptom. Which file has to change?" | files the merged fix touched | `correct`: names the primary fix file; `partial`: names another touched file, or the right type in the wrong file |
| `references` | "Which files reference `X`?" (`X` a symbol the fix touched) | an adjudicated reference set | `correct`: ≥75% of the core set with ≤2 files outside core ∪ admissible; `partial`: ≥40% |
| `impact` | "If `F` changes, what else must be looked at?" | fix-touched files beyond `F`, unioned with the adjudicated set | `correct`: every file in the graded set appears, under a precision cap; `partial`: at least half |
| `end-to-end` | ticket text only: "produce a patch" | the merged fix diff | `correct`: same site, behaviour-equivalent; `partial`: right site, incomplete |

**`end-to-end` has two forms and they are never compared to each other.** With a model in the
loop it grades a patch. Without one — a scripted tool invocation with no model — it can only
grade retrieval, and it is then named `e2e-retrieval` and reported under that name.

**Two known holes in this taxonomy, both stated because they cut against us.** An `impact` task
whose fix touches only the seed file has an empty beyond-seed set, and every band is then
vacuously satisfied; graded truth must be checked non-empty before the run. And a reference
truth set built by a text pass structurally cannot contain a reference that exists only through
type inference — a file that holds `var db = Helper.GetDbInstance();` references that type
without naming it. That biases `references` toward text-search arms by construction, because
the baseline's own method defines the target it is graded against.

## Metrics

- **Correctness** per band above, with raw recall and precision printed beside the grade so a
  reader can regrade under a different band.
- **Cost.** With a model in the loop: the harness per-agent token counter, one definition,
  every arm. Without one: payload bytes of captured stdout, reported as the proxy it is
  (`tokens = ceil(bytes / 4)`) and never mixed into the same table as counter figures.
- **Cost ratio is paired.** For each task divide the baseline's cost by this tool's, then take
  the median of the per-task ratios. A median of ratios is not a ratio of medians; both are
  published, and a disagreement between them is part of the result.
- **Wall time**, cold and warm. Cold is the first invocation after the index is built; warm is
  the median of three immediate repeats. Neither is dropped for being unflattering.
- **Tool-call count** per task.
- **Follow-up reach.** A row reaches when it carries all three of a repo-relative path, a
  1-based line on that path, and a reason token (matched symbol, edge kind, or the source line).
  One failing row fails the task. Reach is a property of an output format, not of a task.
- **Setup cost, separate and never amortised**: install wall time, index build wall time,
  on-disk bytes, and the break-even task count. It is never folded into a per-task figure.
- **`not_attempted` is never zero.** A task an arm cannot express is excluded from that arm's
  denominators, counted in its own column, with the reason quoted from that tool's own docs.

## Environment disclosure template

Every results document opens with this block, filled in. Absolute paths are rewritten
bench-relative and the host is not named beyond its architecture.

```
Date            YYYY-MM-DD
Corpus          <repo> @ <full SHA>   (<n> indexed files at the pin)
Tool version    devscout <version> (<git describe or crate version>)
Build           cargo build --release, rustc <version>, <target triple>
Host            <arch> workstation, otherwise idle; <cores> cores, <ram>
Baseline        ripgrep <version>
Bench root      bench/ (throwaway; nothing installed globally)
Isolation       SCOUT_REGISTRY and SCOUT_CONTENT_DB redirected under bench/state/
Network         setup only; offline for every measured invocation
Reps            <n> per cell
Deviations      <anything that differed from this document, or "none">
```

## Rerunning against the pinned corpus

Pins live in [`bench/corpus.lock`](../../bench/corpus.lock). The C# corpus is
`MassTransit/MassTransit` at `855cf1752c94ca9498e0c45ce8d09fdc9e957dd6`.

```sh
cargo build --release
bench/clone-corpus.sh csharp bench/clones/csharp   # shallow fetch at the pin, origin removed

export SCOUT_REGISTRY="$PWD/bench/state/repos.json"
export SCOUT_CONTENT_DB="$PWD/bench/state/content.db"
./target/release/devscout map bench/clones/csharp   # cold build; time and size go in the env block

./target/release/devscout refs <Symbol> --json
./target/release/devscout impact <path/to/File.cs> --hops 2 --json
```

`origin` is removed by the clone script on purpose: an arm that can fetch upstream can read the
answer instead of finding it. Re-run any query three times for the warm figure;
[`bench/README.md`](../../bench/README.md) has the timing harness.

## Resolver precision

A second, independent benchmark family: instead of grading retrieval on a task, it scores
devscout's `uses-member` edges against a **compiler** ground truth. The oracle is
[`tools/scout-semantic`](../../tools/scout-semantic/README.md), a Roslyn console tool that
walks a compiled C# solution and emits one record per member-access, conditional-access, or
bare-invocation site it finds. The scoring verb is `devscout audit --semantic <refs.jsonl>
--units <units.jsonl>`; [`bench/semantic.sh`](../../bench/semantic.sh) runs the oracle and the
audit in sequence against one corpus solution.

### Metric family

- **Per-edge precision by tier.** Every `uses-member` edge devscout emits is scored true- or
  false-positive against the oracle record at its site, broken out by the edge's resolution
  tier (`precise`, `ext`, `guess`, or the legacy `heuristic` bucket until the `tier` key
  lands).
- **Recall over in-tree member sites.** Denominator = oracle records with `shape == "access"`,
  `external == false`, and a target the graph already carries a definition for (an id in
  `defs.jsonl`, or an enum member's enum id). Reported for `precise`-only, `precise+ext`, and
  all tiers together, and broken down by `receiverKind` (`ident`, `qualified`, `this`, `base`,
  `call`).
- **External-receiver leak.** Sites where every oracle record is `external` — no in-tree
  target could ever be correct there. `silent` when devscout emits no edge at such a site,
  `leak` when it emits one anyway: a guess-tier false positive that name-collision alone could
  never have produced a true positive for.
- **Structural impossibility.** An edge whose target unit is unreachable from its source
  unit's project-reference closure (or, run without `--units`, whose target is a test-only
  definition referenced from non-test code): a guess that is wrong regardless of naming,
  reported separately from ordinary false positives.
- **Fan-out.** A histogram of candidate count per site (1 / 2 / 3 / 4+), plus the top
  false-positive targets by short name and the top missed targets by id — the shape of where
  precision and recall are actually being lost, not just the headline numbers.

### Join rule

An oracle record and a devscout edge are the same site when `(file, startLine)` match, where
`startLine` is the line the **whole member-access node starts on** — the first token of the
qualifier (`_logger` in `_logger.LogInformation(...)`), not the line of the member name token
itself. This is what devscout's own extractor records for a `uses-member` edge, so it is the
only join key that does not silently misalign a qualifier and member name split across lines.

### Exclusions

- **External targets** (`external: true` in the oracle record) never enter the recall
  denominator — the graph cannot resolve to a definition it never indexed — and are scored
  separately, as silent-correct or leaked, never as an ordinary graph miss.
- **`conditional` (`?.`) and `bare` (unqualified invocation) shapes** are reported on their
  own, outside the headline recall figure: devscout's extractor accepts neither as a
  member-access qualifier today, so they are a documented recall bound rather than noise to
  average into the main number.
- **Failed units** (`status != "ok"` in `units.jsonl`) drop their files from the scored
  universe on both sides — an oracle that could not compile a project has no ground truth for
  edges inside it, and a graph edge into or out of it cannot be judged either.

### Two-run protocol

Predictions are registered before the numbers exist, same as everywhere else in this
directory. **Run 0** is the baseline: the current resolver, against the pinned corpus and
fixture, before the resolver changes the fixture was built to motivate land. Its per-tier
bucket sizes — edge counts, site counts — are the denominators every prediction is written
against, so a predicted precision or leak rate is a claim about Run 0's buckets, not about
whatever a later run happens to produce. **Run 1** runs after the resolver changes, same
corpus pin and same fixture, and is reported beside Run 0 rather than overwriting it, so a
regression in one tier alongside an improvement in another stays visible instead of netting
out.
