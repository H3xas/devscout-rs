# Compiler facts

`devscout compiler-facts run|import|status` is entirely optional: `map` and every query verb
answer from source-level extraction alone and never spawn a compiler or touch the network. When a
compiler-derived artifact has been admitted, the resolver consumes it at the next `map`; `map
--no-semantic` skips an admitted artifact even when one is present and always rebuilds, producing
the exact syntax-only graph a build with no artifact admitted would, apart from its `stats.lane`
record (see [The lane line](#the-lane-line) below).

This guide covers acquiring or importing an artifact, what admission checks, how `map` consumes
it, and how to read the lane it reports. The design history is in the
[compiler-enrichment design record](../design/compiler-enrichment.md); the engine itself is
described in [`tools/scout-semantic/README.md`](../../tools/scout-semantic/README.md).

## Acquire or import an artifact

- `run` launches a one-shot engine located by `SCOUT_COMPILER_ENGINE` (no default, nothing
  downloaded), captures its output under a wall-clock timeout and a byte cap, and admits it.
- `import <file>` admits a build- or CI-produced artifact through the identical admission path
  `run` uses, so a locally acquired and an externally produced artifact reach the same accept or
  refuse decision for the same bytes.
- `status` is read-only and reports `coverage: syntax-only` when no artifact has ever been
  admitted, or the admitted artifact's own coverage state otherwise.

The flags each sub-command takes are listed in the [CLI reference](../reference/cli.md#compiler-facts).

## The coverage line

`run`, `import` and `status` print the same coverage line. It reads `coverage: complete` only when
the artifact lists no incomplete unit and every counted compilation in its build context is
`complete`. Counted compilations are those whose state is `complete`, `partial` or `failed`
(`unsupported` and `excluded` ones carry no facts by design). Otherwise the line names the
incomplete ones with their reasons, in ascending order:

```text
admitted compiler facts (engine 2), coverage: partial (3 of 6 compilations: workspace-failure 3)
admitted compiler facts (engine 2), coverage: incomplete (1 unit affected; 1 of 2 compilations: binding-error 1)
```

Admission and exit codes do not change: an artifact whose compilations are all partial is still
admitted, and an occurrence from a compilation that is not complete still confirms nothing.

### Offline restores and NuGet audit warnings

A restore done offline with NuGet's vulnerability audit on logs an unreachable-feed warning
(`NU1900`) for each project. Audit warnings no longer demote a compilation: the producer keeps
them in the compilation's workspace diagnostics and still reads the compilation `complete` (the
rule is in [`tools/scout-semantic/README.md`](../../tools/scout-semantic/README.md)). Artifacts
produced before this change keep their `partial` states until they are produced again, and the
coverage line and the lane report them as partial.

## Admission checks

Every check runs before a single byte is published: a mismatched engine revision, contract
version, requested profile, dependency fingerprint, compilation-context version or fingerprint, or
source-snapshot identity is refused with a stable reason and writes nothing; a killed, timed-out,
over-budget, truncated, malformed, or internally incoherent run leaves the previously admitted
artifact byte-identical. A structurally valid artifact that declares incomplete coverage is still
admitted, together with its per-unit diagnostics, and is never reported as clean or complete.
Publication is atomic — a validate-then-rename through the same same-directory temp file scheme
every other artifact in this crate already uses.

## Consuming an admitted artifact

At `map` time, a same-context compiler fact determines the answer at its own occurrence,
including where it contradicts a syntax-derived edge; the displaced syntax answer is preserved as
a disagreement diagnostic (`stats.semantic.disagreements` in `graph.json`), never as a graph
edge. A compiler-verified occurrence the syntax extractor never referenced at all is projected as
its own, separately provenanced population (`source: "semantic-discovered"`, never pooled with
the per-reference overrides).

Freshness is whole-artifact: any change to the checkout's git head or working-tree cleanliness
since the artifact was captured invalidates every fact it carries, even for a consuming file that
is itself byte-identical. So does a changed MSBuild import (`Directory.Build.props`,
`Directory.Packages.props`, a `.targets` file) the artifact's own compilation context named at
capture time, re-checked offline against its recorded hash -- the case a git-only check misses
when the import lives outside the mapped repository.

`devscout audit --semantic` reports both an enriched graph's `semantic` and `semantic-discovered`
tiers alongside the syntax tiers, and a `lane` key (`"syntax"`, `"enriched"` or
`"enriched-partial"`) naming which kind of graph was audited (see [The lane line](#the-lane-line)
below). This mechanism is registered against a ship/no-ship gate on a public corpus; see
[`docs/benchmarks/results/2026-09-resolver-precision.md`](../benchmarks/results/2026-09-resolver-precision.md)'s
"Run 7" section for the measured result — the layer is additive and always available on request,
but is not currently recommended enabled by default on the strength of that measurement. How
enrichment changes what the `precise` tier holds is explained in
[How names resolve](../explanation/resolution.md#compiler-facts-and-the-precise-tier).

## The lane line

Whenever an entry sits at the compiler-facts path or `--no-semantic` is given, `map` prints one
more line after its summary and writes the same answer into `graph.json` as `stats.lane`, the
last key of `stats`:

| Line | `stats.lane` |
| --- | --- |
| `lane: enriched` | `{"state":"applied","compilations":{"total":M,"partial":0}}` |
| `lane: enriched, partial (K of M compilations partial: <tally>)` | `{"state":"partial","reason":"compilations-incomplete","detail":"K of M compilations partial: <tally>","compilations":{"total":M,"partial":K}}` |
| `lane: syntax (enriched lane not applied: no-complete-compilation: <detail>)` | `{"state":"not-applied","reason":"no-complete-compilation","detail":"<detail>","compilations":{...}}` |
| `lane: syntax (enriched lane not applied: nothing-confirmed[: <detail>])` | `{"state":"not-applied","reason":"nothing-confirmed","detail":"<detail>","compilations":{...}}`, `detail` only when a compilation is partial |
| `lane: syntax (enriched lane not applied: stale-artifact: <token>)` | `{"state":"not-applied","reason":"stale-artifact","detail":"<token>"}` |
| `lane: syntax (enriched lane not applied: imports-moved: <token>)` | `{"state":"not-applied","reason":"imports-moved","detail":"<token>"}` |
| `lane: syntax (enriched lane not applied: artifact-unreadable)` | `{"state":"not-applied","reason":"artifact-unreadable"}` |
| `lane: syntax (enriched lane not applied: no-semantic)` | `{"state":"not-applied","reason":"no-semantic"}` |

`detail` is the freshness token, verbatim. `stale-artifact` carries one of
`source-snapshot-dirty-at-admission`, `source-snapshot-missing`, `working-tree-dirty`,
`source-head-mismatch` or `source-head-unavailable`; `imports-moved` carries
`import-file-changed:<identity>` or `import-file-missing:<identity>`, naming the import that moved.
`artifact-unreadable` covers any entry at the artifact path that does not read as an admitted
artifact, and `map` still exits 0 with the syntax graph. A stale or moved-import artifact writes no
`stats.semantic`, so `audit --semantic` reports `lane: syntax` for that graph. With no artifact on
disk and no `--no-semantic`, `map`'s output and `graph.json` carry no lane line or key at all.

A fresh artifact is then judged by its own compilations, counted as in the coverage line above.
When none of them is complete (`no-complete-compilation`), the layer is dropped before resolution
as a stale one is. When some are complete but the run confirmed and discovered nothing
(`nothing-confirmed`), the lane did not apply either. Both write no `stats.semantic`, and the
graph's edges equal the syntax-only graph's. When facts were used but some counted compilation is
not complete, the lane is `partial`: its `stats.semantic` counters stand, and `detail` names the
incomplete compilations by reason. Every record decided from the compilations carries
`compilations` (`total`, the counted compilations, and `partial`, those not complete); the four
reasons above are decided first and never carry it.

## The lane in `audit --semantic`

`audit --semantic` reads the same record. Its `lane` is `enriched-partial` when `stats.lane.state`
is `partial`, `enriched` when `stats.semantic` is present otherwise, and `syntax` otherwise, so a
CI check for `"enriched"` fails on a partial run. The text header adds the reason and detail for a
partial or not-applied record. The `--json` report appends `lane_detail` (the `stats.lane` record)
and `compilations` (its counts) when the graph has a lane record, so `--assert` can pin
`compilations.partial`, for example `{"compilations.partial": {"max": 0}}`. A graph written before
the lane record existed audits as it did, and an assertion on `compilations.partial` reports it
missing. Exit codes are unchanged. The rest of `audit` is described in the
[benchmark methodology](../benchmarks/methodology.md#resolver-precision).

## Where the artifact lives

The admitted artifact is `compiler-facts-v<N>.json` beside `graph.json`; see
[Storage](../reference/storage.md). `SCOUT_COMPILER_ENGINE` is described in
[Environment variables](../reference/environment.md).
