# Benchmarks

Everything this project claims about its own speed, cost, or accuracy lives here, with the
command that produced it. Nothing else in `docs/` grows past reference minimum — benchmarks
are the one sanctioned place where new claims accumulate, and they carry their own discipline
in exchange for that room.

| Document | What it holds |
| --- | --- |
| [methodology.md](methodology.md) | Task taxonomy, metrics, environment disclosure, exact rerun steps |
| [peers.md](peers.md) | The other tools an agent could install instead, and where each one leads |
| [agentic.md](agentic.md) | The model-in-the-loop protocol: lanes, effort, arms, disclosure |
| [extension-impact.md](extension-impact.md) | The extension-impact qualification harness: candidate-edge and complete-answer precision, measured apart |
| [Results](#results) | Dated result documents. One file per run. Never edited in place. |
| [methodology.md § Resolver precision](methodology.md#resolver-precision) | Per-edge precision/recall for `uses-member` edges against a compiler oracle ([`tools/scout-semantic`](../../tools/scout-semantic/README.md)), scored by `devscout audit --semantic` and run via [`bench/semantic.sh`](../../bench/semantic.sh) |
| [methodology.md § Rust edges](methodology.md#rust-edges) | Precision and recall of Rust call edges and test pairs against `rust-analyzer lsif` on this repository, run via [`bench/rust_edges.py`](../../bench/rust_edges.py) |

The harness itself is in [`bench/`](../../bench/README.md).

## The honesty statement

This is the part that governs the rest, so it is stated before any number exists.

**Gaps are published in both directions.** Every results document is required to name the
arm that leads each band × metric cell, including the cells where a peer or a plain
`ripgrep` baseline beats this tool. A results document with no "where peers win" section is
an incomplete results document, not a favourable one.

**No best-everywhere claim will be made here.** This index answers name-level and
reachability questions from a prebuilt graph. Questions that reduce to finding one
distinctive string are answered well, cheaply, and without any index at all by a skilled
agent holding `rg` — and a benchmark that hides that is measuring a strawman. Where the
index does not earn its cost, the results say so in those words.

**The baseline is skilled, not staged.** Order-of-magnitude wins in this space are usually
wins over a baseline that was told to read the repository or given no search strategy. The
baseline here gets ripgrep, listing, read, an explicit competent playbook, an equal budget,
and published transcripts. If this tool cannot beat that, it has no result worth publishing.

**Methodology and results are separate files, and results are dated.** Editing a method
never silently redates a number. A rerun gets a new file in `results/`; it does not overwrite
an old one.

**Pinned corpora, or no number.** Every figure is measured against a corpus pinned by SHA and
named in [`bench/corpus.lock`](../../bench/corpus.lock). Results measured on corpora that
cannot be published are not published — not in summary, not in ratio form, not as a range.

**Predictions are registered before the run and misses are published as misses.** The
falsification condition for each claim is written down first. A run that falsifies its own
claim is published with the same prominence as one that does not.

**Single-run figures carry a banner.** One repetition per cell cannot separate a small effect
from noise. Until a multi-run protocol closes it out, every single-run table says so at the
top of the document rather than in a footnote.

## Status

The first public-corpus run is in [`results/2026-08.md`](results/2026-08.md): scripted, no model
in the loop, against the pinned MassTransit corpus. It reports this tool losing four of eight
tasks to a skilled `rg` baseline and never breaking even on setup cost, because that is what it
measured. The agentic (model-in-the-loop) round is now appended to that same file — preliminary, one run
per cell, with its integrity caveats leading.

The same eight scripted tasks were rerun on the 0.7.0 release tree, beside a resolver-precision
audit of that tree, in [`results/2026-09-25-release-0.7.0.md`](results/2026-09-25-release-0.7.0.md),
with predictions registered first. The 0.8.0 release tree has the same treatment in
[`results/2026-09-25-release-0.8.0.md`](results/2026-09-25-release-0.8.0.md), which adds the reach
of the `bus-hop` edges and the opt-in enriched lane, and so has the 0.9.0 release tree in
[`results/2026-10-12-release-0.9.0.md`](results/2026-10-12-release-0.9.0.md).

The first TypeScript run is in
[`results/2026-10-11-typescript-reference-parity.md`](results/2026-10-11-typescript-reference-parity.md),
on the pinned nx corpus against one TypeScript-capable peer. Its registered recall gate was
missed, as its pre-registration predicted: imports written by package name stay unresolved.
The [module-resolution comparison](results/2026-10-15-typescript-module-resolution-4.md)
measures the declared modes against TypeScript 5.9.3 on pinned public fixtures and lists every
excluded site. The [reference-parity rerun](results/2026-10-15-typescript-reference-parity-3.md)
on the same sealed nx seeds records the package-name improvement and a met recall gate.

## Scorecard

**Scorecard** (devscout 0.9.0 vs the `rg` baseline, MassTransit corpus; the scripted rows' full
numbers, per-cell commands and grades are in
[`docs/benchmarks/results/2026-10-12-release-0.9.0.md`](results/2026-10-12-release-0.9.0.md),
the agentic row and its preliminary-run caveats in
[`docs/benchmarks/results/2026-08.md`](results/2026-08.md)):

| Kind | devscout | rg | Verdict |
| --- | --- | --- | --- |
| Locate | 2/2 correct | 2/2 correct, faster | Tie — use rg |
| References | 1/2 correct + 1 partial (needs `--all`) | 2/2 correct | rg on a text-built truth set; devscout's extra files are real references it cannot credit |
| Impact | 2/2 partial, better precision, 1 call | 2/2 partial, 4-call chain | devscout wins |
| End-to-end retrieval | 1/2 correct | 2/2 correct | rg wins |
| Agentic, Opus (preliminary, measured on 0.2.0) | 4/4 correct, median 180k tokens | 3/4 correct + 1 partial, median 199k tokens | No correctness edge; ~25k-token saving only |

The four scripted rows were measured on 0.9.0's final tree, one run per cell, and every
devscout answer is 0.8.0's (itself byte-identical to 0.7.0's) plus the next verb it now suggests.
0.2.0 read devscout 5 correct / 3 partial against rg's 6 / 2; 0.7.0, 0.8.0 and 0.9.0 read 4 / 4,
and the whole difference is one references task, where
devscout also names three files that call `IJobService` members through a property typed with
it, which a truth set built by text search cannot hold. The agentic row is the preliminary 0.2.0
round and has not been re-measured.

Gaps are published in both directions. `devscout` answers name-level and reachability questions
from a prebuilt graph; questions that reduce to finding one distinctive string are answered
well and cheaply by a skilled agent holding `rg`, with no index at all. Where a peer or the
plain `rg` baseline leads, the results say so. The dated results in
[`docs/benchmarks/results/2026-08.md`](results/2026-08.md) now include the first
agentic (model-in-the-loop) round — preliminary, one run per cell, its integrity caveats leading.

## Per-release notes

Releases 0.3.0 to 0.6.0
were not measured on the scorecard: 0.3.0 changes `find` output ordering and reference resolution
(exact generic arity); 0.4.0 changes resolver output again (heuristic tiers and recall), measured
in
[`docs/benchmarks/results/2026-09-resolver-precision.md`](results/2026-09-resolver-precision.md);
0.5.0 changes no resolver output and produces a byte-identical `graph.json` on the pinned corpus;
0.6.0 raises the graph schema to 3 and adds the `implements` and `overrides` edges, so its
`graph.json` is not byte-identical to 0.5.0's, and a repository whose code registers nothing
through dependency injection gains no edges and answers as it did.
Release 0.7.0 changes resolver output again: the precise tier refuses `uses-member` edges C# name
lookup cannot produce, and a member qualifier's written type-argument count decides which
same-named type it binds. Its final tree is measured in
[`docs/benchmarks/results/2026-09-25-release-0.7.0.md`](results/2026-09-25-release-0.7.0.md):
precise precision 0.993 (0.989 before the refusals), no precise false positive against an
external target or across an impossible project reference, and recall 0.528 precise, 0.571
precise+ext, 0.659 over all tiers. 0.7.0's `graph.json` is not byte-identical to 0.6.0's.

Release 0.7.0 also adds an
[extension-impact qualification harness](extension-impact.md). It measures
precision for candidate edges and complete answers separately without changing production
traversal. These measurements do not replace the task scorecard above.

Release 0.8.0 changes resolver output by default: a recognized publish or dispatch call gains a
`bus-hop` edge, a new edge kind, to each matching handler declaration in the indexed repository,
and `refs`, `read`, `impact` and `tests` traverse those candidate routes unless `--no-bus` is
given; a repository with no publish site gains no edges. The compiler-fact enrichment is opt-in:
`map` consumes a compiler-facts artifact only after one has been admitted, and without one, or
with `map --no-semantic`, the default graph is unchanged. Its final tree is measured in
[`docs/benchmarks/results/2026-09-25-release-0.8.0.md`](results/2026-09-25-release-0.8.0.md).
The default lane scores exactly as 0.7.0 does (precise precision 0.993; recall 0.528 precise,
0.571 precise+ext, 0.659 over all tiers), and with `--no-bus` every measured `refs` and `impact`
answer is byte-identical to 0.7.0's. On the pinned corpus `map` emits 23,687 `bus-hop` edges over
171 messages, 97.4% of them on one shared test message that reaches 79 handlers; these measure
reach, and route precision and recall are unmeasured. The opt-in enriched lane, on an artifact
in which 53 of the 54 loaded compilations are complete, reaches recall 0.920 precise and 0.921
precise+ext at 0.997 precision over its precise and semantic tiers together, scored against the
same oracle it was produced from. Offline, the restore needed `-p:NuGetAudit=false` for that: with the package
audit on, its unreachable-feed warning marked every compilation partial and the lane confirmed
nothing. The producer no longer does this: a NuGet audit warning (`NU1900` to `NU1905`, matched
against the project's own assets file) stays in the compilation's diagnostics but no longer
demotes it, so an audit-on offline restore now gives complete compilations. The rule is in
[`tools/scout-semantic/README.md`](../../tools/scout-semantic/README.md).

Release 0.9.0 changes resolver output again: nine precise-tier fixes, one for each mechanism of
the [wrong-target attribution](results/2026-09-24-precise-wrong-target-attribution.md), bind
what C# binds or refuse the edge. Its final tree is measured in
[`docs/benchmarks/results/2026-10-12-release-0.9.0.md`](results/2026-10-12-release-0.9.0.md).
115 of the 170 precise wrong-target edges leave and none is added: precise precision 0.997
(31,103 of 31,202), recall 0.529 precise, 0.575 precise+ext, 0.663 over all tiers. The 55 left are
54 calls whose argument types are unknown or do not tell the overloads apart, and one lambda
whose value type would be needed. 0.9.0's `graph.json` is not byte-identical to 0.8.0's; its
`bus-hop` edges are 0.8.0's, edge for edge. The opt-in enriched lane keeps 0.8.0's figures
(recall 0.920 precise, 0.921 precise+ext, 0.997 precision over its precise and semantic tiers
together) and reaches them over an offline restore with the package audit on as well as off; on
this corpus it reports `enriched-partial` and names its one partial compilation, and an artifact
with no complete compilation is refused with that reason. Every scripted scorecard answer is
0.8.0's plus the next verb it suggests, so no grade moved; `find` takes about a third longer,
because it checks the command it suggests before printing it.

A separate scripted-lane run measured **tool calls issued per task**: the index arm used fewer
calls in all four query kinds, largest on references (5.0 vs 11.8 per lane, ~2.4x) — single-run
proxy, details under "Tool-call proxy" in
[`docs/benchmarks/results/2026-08.md`](results/2026-08.md).

## Results

Dated result documents, one file per run, never edited in place:

| Result | What it measured |
| --- | --- |
| [2026-08.md](results/2026-08.md) | The first public-corpus scripted round and its rerun, plus the preliminary agentic round and the tool-call proxy |
| [2026-09-resolver-precision.md](results/2026-09-resolver-precision.md) | Resolver precision against the compiler oracle, from the baseline run onward |
| [2026-09-engine-packaging-cost.md](results/2026-09-engine-packaging-cost.md) | Optional engine packaging and cost on pinned fixtures |
| [2026-09-17-dotnet-target-held-out.md](results/2026-09-17-dotnet-target-held-out.md) | .NET target qualification, held-out run (superseded by the next row) |
| [2026-09-21-dotnet-target-held-out.md](results/2026-09-21-dotnet-target-held-out.md) | .NET target qualification, held-out run |
| [2026-09-24-precise-wrong-target-attribution.md](results/2026-09-24-precise-wrong-target-attribution.md) | Precise-tier wrong-target false positives attributed to the resolver mechanism behind each |
| [2026-09-25-release-0.7.0.md](results/2026-09-25-release-0.7.0.md) | The 0.7.0 release tree: resolver-precision audit and the scripted scorecard |
| [2026-09-25-release-0.8.0.md](results/2026-09-25-release-0.8.0.md) | The 0.8.0 release tree: default lane, `bus-hop` reach and the opt-in enriched lane |
| [2026-09-27-rust-edges.md](results/2026-09-27-rust-edges.md) | Rust call edges and test pairs against `rust-analyzer lsif` |
| [2026-10-03-rust-targets.md](results/2026-10-03-rust-targets.md) | Rust nested targets, `cfg_attr` tests and path modules against `rust-analyzer lsif` |
| [2026-10-11-typescript-reference-parity.md](results/2026-10-11-typescript-reference-parity.md) | TypeScript `refs`, `tests` and `impact` on the pinned nx corpus against one TypeScript-capable peer |
| [2026-10-15-typescript-module-resolution.md](results/2026-10-15-typescript-module-resolution.md) | First registered comparison of TypeScript module modes, with its missed nodenext gate |
| [2026-10-15-typescript-module-resolution-2.md](results/2026-10-15-typescript-module-resolution-2.md) | Second registered comparison over recorded import sites and listed exclusions |
| [2026-10-15-typescript-module-resolution-3.md](results/2026-10-15-typescript-module-resolution-3.md) | New head arm on the second registration after correcting file and directory candidates |
| [2026-10-15-typescript-module-resolution-4.md](results/2026-10-15-typescript-module-resolution-4.md) | New head arm after applying ESM rules to package-map targets |
| [2026-10-15-typescript-reference-parity.md](results/2026-10-15-typescript-reference-parity.md) | Sealed nx recall rerun after workspace package resolution |
| [2026-10-15-typescript-reference-parity-2.md](results/2026-10-15-typescript-reference-parity-2.md) | Sealed nx recall rerun at the corrected head |
| [2026-10-15-typescript-reference-parity-3.md](results/2026-10-15-typescript-reference-parity-3.md) | Sealed nx recall rerun after the package-map target fix |
| [2026-10-14-freshness-latency.md](results/2026-10-14-freshness-latency.md) | What a query pays for certifying a dirty index, against the previous release at the same state |
| [2026-10-14-hook-latency.md](results/2026-10-14-hook-latency.md) | First-read offer latency against an identical read with the offer suppressed |
| [2026-10-12-release-0.9.0.md](results/2026-10-12-release-0.9.0.md) | The 0.9.0 release tree: default lane with the precise-tier fixes attributed, `bus-hop` reach, the opt-in enriched lane in three passes and the scripted scorecard |
