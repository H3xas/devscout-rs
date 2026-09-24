# Extension-impact qualification

The measured candidate is **not admitted to production traversal**. Its edge precision clears
0.95, but complete affected-answer precision decreases and individual answers fail the floor.
The production walker, CLI options, graph schema and evidence tiers are unchanged.

## Fixed protocol

`bench/extension_impact.py` scores a graph against the same `refs.jsonl` and `units.jsonl`
consumed by `audit --semantic`. It checks precise/ext/guess TP, FP and edge counts, plus the
recall denominator, against an independently generated native audit before replaying answers.
`examples/extension_impact.rs` invokes the actual native `build_impact_model`.

An eligible edge must be `uses-member`, `heuristic: true`, `tier: ext`, carry a nonempty member
and positive source line, be the only member edge at its `(file, line, member)` site across
all tiers, and name one definition at the target file that declares that method. Candidate
selection never consults oracle correctness. A mixed file row cannot admit its name guesses.

The replay seeds every distinct extension target file in the compiled file universe, including
targets of rejected candidates, in lexical order. It uses two hops, the existing cap of 50,
interface fan-in brake of 8, and hub indegree brake of 34. Only selected edge indexes move from
heuristic inbound to traversable inbound in an isolated index. The graph and its edge tiers
remain unchanged. An empty candidate list reproduces the baseline answer.

Whole-answer truth replaces member predictions with oracle member references while holding
non-member native context fixed. The truth set uses two hops without a row cap or numerical
widening brakes: a brake truncates an answer, but does not make an omitted dependency false.
The existing path-based infrastructure boundary applies in both arms. The oracle's membership
set is not ranked. Production predictions retain all their default brakes.

The scoring unit is `(seed file, returned file)`, counted once per answer. Every displayed row,
including heuristic suggestions, participates in the all-shown score. Asserted affected rows
are also scored separately. Newly shown and displaced rows, micro/macro/worst precision,
per-answer counts, recall, and cap/brake records remain in the generated artifacts. Precision
with no predictions is undefined. Recall denominators are oracle reference records for the
edge table and oracle-reachable seed/file pairs for the answer table; they are not interchangeable.

This is **conditional compiler-reference reachability**, not independent truth for type,
inheritance, dispatch, or dependency-injection edges, exact overload binding, imported edges,
or runtime affected behaviors. Only compiled, mapped files are evaluated. Ambiguous oracle
candidates and same-line member identity retain the semantic audit's documented limitations.
The results cannot qualify unrelated corpora or unsupported boundaries.

## Measurement at 0.6.0

Engine source: `53295f96ff4601d89abd1181e30bcf83c41640e3`. Corpus: MassTransit at
`855cf1752c94ca9498e0c45ce8d09fdc9e957dd6`. Reference denominator: the **2026-09-05 integration
re-baseline**, retained as `bench/out/semantic/MassTransit-main-c2bce9f/audit.json`: precise
precision **0.989**, recall **0.529**, and **57,607** eligible oracle records. This qualification
reproduces its exact 31,090 TP / 360 FP and 30,467 recall hits. It does not substitute a new
baseline for the attributed measurement.

Measured 2026-09-12 using the retained oracle: 112,190 records, 56 loaded units, zero failed
units. The oracle contains compiler diagnostics documented in the resolver audit; compilation
health is not inferred from `status: ok`. The scored universe contains 5,427 files; 740 member
edges outside that universe are unjudged.

| Edge cohort | Edges | TP | FP | Precision | Recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| Re-baseline precise | 31,450 | 31,090 | 360 | 0.988553 | 0.528877 |
| Retained extension tier | 2,536 | 2,514 | 22 | 0.991325 | 0.043554 |
| Fixed candidate | 2,365 | 2,343 | 22 | 0.990698 | 0.040603 |
| Precise + candidate | 33,815 | 33,433 | 382 | 0.988703 | 0.569479 |

The full extension tier hits 2,509 precise misses and overlaps zero precise recall hits.
The five additional extension TPs match conditional-access oracle records outside headline
recall. Thus the **4.355 percentage-point candidate ceiling** is not an observed downstream
answer gain. The predicate rejects 170 edges without the required method spelling in the target
definition and one competing site. The selected cohort gains 2,339 precise recall misses;
none of its 22 FPs were removed by those graph-only checks. No thresholds were tuned afterward.

Whole native answers use 195 seed files and 67,470 oracle-reachable seed/file pairs:

| Answer cohort | Rows | TP | FP | Precision | Recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| Baseline, all shown | 2,473 | 2,369 | 104 | 0.957946 | 0.035112 |
| Candidate, all shown | 4,887 | 4,684 | 203 | 0.958461 | 0.069423 |
| Baseline, asserted affected | 918 | 916 | 2 | 0.997821 | 0.013576 |
| Candidate, asserted affected | 4,331 | 4,231 | 100 | 0.976911 | 0.062709 |
| Newly shown | 2,828 | 2,714 | 114 | 0.959689 | 0.040225 |
| Displaced | 414 | 399 | 15 | 0.963768 | 0.005914 |

All-shown precision improves by just 0.052 percentage points; asserted affected precision
falls by 2.091 points. Candidate all-shown answers below 0.95 grow from 21 to 30; 17 asserted
answer cohorts fail the floor. Macro all-shown precision is 0.956362 to 0.959528, but the worst
candidate all-shown answer is 0.0 and the worst asserted answer is 0.42. Four seed files return
no shown rows in each arm and are reported as undefined, not perfect precision.

For a concrete affected-answer failure, seeding `DeserializeVariableExtensions.cs` yields
21 supported / 29 unsupported asserted files (precision 0.42). Seeding
`ICollectionNameFormatter.cs` changes an asserted answer from 50 TP / 0 FP to 40 TP / 10 FP.
These examples are observations of the fixed cohort, not names to exclude in a tuned rule.

**Recommendation: no admission.** More correct files are shown, but edge precision alone does
not establish safe affected answers. Individual answer floors fail, and asserted precision
regresses substantially. Multi-hop behavior correctness and a production provenance/suppressor
contract remain unqualified. A stronger predicate requires independent evidence and a new
qualification decision; renaming the tier is not a remedy.

## Measurement at 2026-09-23

Engine source: `4df6069d079bc5a6efa77a498c7df2c6678905a4` (the implementation base on the 0.7.0
release line; two resolver revisions landed on this line after 0.6.0 and move the precise and
extension edge tiers). Corpus: MassTransit at `855cf1752c94ca9498e0c45ce8d09fdc9e957dd6`,
unchanged. Reference denominator: the same **2026-09-05 integration re-baseline**, retained as
`bench/out/semantic/MassTransit-main-c2bce9f/audit.json`: precise precision **0.989**, recall
**0.529**, and **57,607** eligible oracle records. The truth instrument is held fixed; only the
engine moved.

Measured 2026-09-23 using the retained oracle: 112,190 records, 56 loaded units, zero failed
units. The scored universe contains 5,427 files; 733 member edges outside that universe are
unjudged.

| Edge cohort | Edges | TP | FP | Precision | Recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| Precise | 31,240 | 31,026 | 214 | 0.993150 | 0.527783 |
| Retained extension tier | 2,543 | 2,521 | 22 | 0.991349 | 0.043675 |
| Fixed candidate | 2,372 | 2,350 | 22 | 0.990725 | 0.040724 |
| Precise + candidate | 33,612 | 33,376 | 236 | 0.992979 | 0.568507 |

Whole native answers use the same 195 seed files and 67,470 oracle-reachable seed/file pairs:

| Answer cohort | Rows | TP | FP | Precision | Recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| Baseline, all shown | 2,476 | 2,374 | 102 | 0.958805 | 0.035186 |
| Candidate, all shown | 4,871 | 4,701 | 170 | 0.965100 | 0.069675 |
| Baseline, asserted affected | 917 | 917 | 0 | 1.000000 | 0.013591 |
| Candidate, asserted affected | 4,315 | 4,248 | 67 | 0.984473 | 0.062961 |
| Newly shown | 2,810 | 2,727 | 83 | 0.970463 | 0.040418 |
| Displaced | 415 | 400 | 15 | 0.963855 | 0.005929 |

All-shown precision improves by 0.630 percentage points; asserted affected precision falls by
1.553 points, from a perfect baseline to 0.984473. Candidate all-shown answers below 0.95 rise
from 20 to 24; 8 asserted answer cohorts fail the floor. Macro all-shown precision is 0.956775
to 0.965218, but the worst candidate all-shown answer is 0.0. Four seed files return no shown
rows in each arm and are reported as undefined, not perfect precision.

**No-go holds on this engine.** The replay above returns exit 2: candidate edge precision clears
0.95, but individual all-shown answers still fail the floor and asserted affected precision still
regresses, so the recommendation is unchanged. A second, private corpus was also measured at
this commit against its own retained oracle; every one of its numerical gates held. Neither
result changes the outcome: **no admission**, still pending a stronger, independently qualified
predicate.

## Reproduce

Keep corpus artifacts in ignored output directories. Build the current source, regenerate the
native audit from the same graph/oracle inputs, and pass those paths explicitly:

```sh
cargo build --locked --bin devscout --example extension_impact
./target/debug/devscout -C "$corpus" audit --semantic "$refs" --units "$units" --json > "$audit"
python3 -B bench/extension_impact.py --graph "$graph" --manifest "$manifest" \
  --refs "$refs" --units "$units" --audit "$audit" \
  --driver target/debug/examples/extension_impact --out "$new_output_directory"
```

The output directory must be new. It retains projected inputs, the exact cohort, all answer
rows, input/harness hashes, and results. Exit 2 is a completed **no-go measurement**, exit 0
means this corpus passed the numerical gates (not release approval), and execution/input
errors fail separately. The MassTransit command above is expected to return 2.

```sh
python3 -B -m unittest discover -s bench -p test_extension_evidence.py
cargo test --locked --example extension_impact
```

The controls cover mixed sites, non-promotable guesses, member-aware joins, recall overlap,
compiled universe, cap displacement, failure hidden by high aggregate precision, two extension
hops, baseline suppression, and oracle-brake truncation. Synthetic cases validate the harness;
they do not establish corpus safety.
