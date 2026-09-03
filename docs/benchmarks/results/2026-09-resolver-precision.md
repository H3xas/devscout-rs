# Results — Resolver precision, 2026-09 (Run 0)

> **Single run, no repeats.** This is Run 0 of the two-run protocol in
> [`docs/benchmarks/methodology.md#resolver-precision`](../methodology.md#resolver-precision):
> the current resolver, against the pinned public corpus, before any resolver change the
> registered predictions below were written to motivate. Its per-tier bucket sizes are the
> denominators every prediction is a claim about. **Run 1**, after resolver changes, is
> appended below rather than overwriting this section, so a regression in one tier alongside an
> improvement in another stays visible.
>
> The `tier` key has not landed yet (W2 in the design). This run reports two buckets only,
> `precise` and `heuristic` — the latter is `ext` and `guess` combined, undifferentiated. Three
> of the five registered predictions name `ext` or `guess` alone and cannot be cleanly checked
> against a merged bucket; see "Registered predictions" and "Defects" below for how each was
> scored anyway.

## Environment

```
Date            2026-09-03
Corpus          MassTransit/MassTransit @ 855cf1752c94ca9498e0c45ce8d09fdc9e957dd6 (bench/corpus.lock, registered)
                5634 files mapped by devscout map; 9956 defs, 130366 edges (graph rebuilt in 1.67s)
Oracle          tools/scout-semantic @ devscout commit 564739a, Roslyn (Microsoft.CodeAnalysis.CSharp.Workspaces) 4.14.0,
                Microsoft.Build.Locator 1.9.1 (tools/scout-semantic/packages.lock.json)
Units           56 loaded, 0 failed; 25 WorkspaceFailed diagnostics collected during load (see Defects)
Oracle output   112190 refs.jsonl records / 66924 sites; 19153 external sites; 137 ambiguous; 0 dropped
Tool version    devscout 0.3.0 (crate version; source tree carries no git metadata, so no git describe)
Build           cargo build --release, rustc 1.97.1 (8bab26f4f 2026-07-14), aarch64-apple-darwin
SDK             dotnet 9.0.305 (msbuild 9.0.305)
Host            macOS (Darwin 25.2.0), Apple M2 Max, arm64; 12 cores, 64 GiB RAM, otherwise idle
Bench root      bench/ (throwaway; nothing installed globally)
Isolation       SCOUT_REGISTRY and SCOUT_CONTENT_DB redirected under bench/state/
Network         setup only (restore + corpus clone); offline for the oracle walk and the audit
Reps            1 (Run 0 baseline — see banner)
Deviations      1. No hyperfine for this cell — it is one long-running invocation, not a repeated
                   query. Wall time below is derived from dotnet restore's own per-project report
                   plus bench/out/semantic/MassTransit/ file mtimes, not an in-process timer.
                2. -p:TargetFrameworks=net9.0 (plural) worked on the first attempt for both
                   restore and the oracle load; the -p:TargetFramework (singular) fallback in
                   bench/README.md was not needed.
```

## Corpus pin and setup

| Language | Repository | Pin | License | Status |
| --- | --- | --- | --- | --- |
| C# | MassTransit/MassTransit | `855cf1752c94ca9498e0c45ce8d09fdc9e957dd6` | Apache-2.0 | registered — measured (already the pinned gate corpus for the extraction work) |

```sh
cargo build --release
dotnet build tools/scout-semantic -c Release
bench/clone-corpus.sh csharp bench/corpora/csharp
bench/semantic.sh bench/corpora/csharp MassTransit.sln -p:TargetFrameworks=net9.0
```

| Phase | Wall time |
| --- | --- |
| `dotnet restore MassTransit.sln -p:TargetFrameworks=net9.0` | ~5m8s (bounded by the slowest project, `MassTransit.KafkaIntegration.Tests`, reported "in 5.14 min") |
| Oracle load (`MSBuildWorkspace`) + walk + write `refs.jsonl`/`units.jsonl` | ~47s |
| `devscout map .` (5634 files, cold) | 1.67s, 9956 defs / 130366 edges |
| `devscout audit --semantic` (text) + `--json` | <5s combined |
| **Total, restore through both audit runs** | **~5m55s** |

## What this measures / does not measure

Independent of the cost/wall-time benchmark family in this same directory: it scores devscout's
`uses-member` edges against a **compiler** ground truth (`tools/scout-semantic`, a Roslyn
console tool) instead of grading retrieval on a task. Full definitions are in
[`methodology.md#resolver-precision`](../methodology.md#resolver-precision); summary:

- **Per-edge precision by tier** (`precise` / `ext` / `guess`, merged into `heuristic` until the
  `tier` key lands), **recall over in-tree member sites** (denominator: oracle records with
  `shape == "access"`, `external == false`, target known to the graph), broken down by
  `receiverKind`.
- **External-receiver leak**: sites where every oracle record is `external` — no in-tree target
  could ever be correct there — split into `silent` (no edge emitted) and `leak` (a guess-tier
  false positive emitted anyway).
- **Structural impossibility**: an edge whose target unit is unreachable from its source unit's
  project-reference closure — wrong regardless of naming, reported apart from ordinary FPs.
- **Fan-out**, top-20 FP targets by short name, top-20 missed targets by id.
- **Join rule**: an oracle record and a devscout edge are the same site when `(file, startLine)`
  match, `startLine` being the line the whole member-access node starts on (the qualifier's
  first token), not the member-name token's line.
- **Excluded from recall**: `external: true` targets (scored separately, never as a graph miss);
  `conditional` (`?.`) and `bare` (unqualified invocation) shapes, reported on their own since
  devscout's extractor accepts neither as a qualifier today; files belonging to units with
  `status != "ok"` (none this run — see Environment).

## Registered predictions (2026-09-03, before Run 0)

Copied verbatim from the design doc (§8, "Corpus runs, docs, changelog"), registered before any
number in this document was seen:

> Public predictions (MassTransit, registered before Run 0 numbers are seen): precise precision
> ≥ 0.95; ext precision ≥ 0.95; guess precision < 0.30; recall(precise) 0.45–0.65; leaked
> external sites > 50% of guess edges. Falsification: precise precision < 0.95 means a hidden
> precise-tier defect class; publish the top-20 FP list with the run.

## Run 0 — baseline (devscout 0.3.0 + audit)

### Audit text output (verbatim; `root` rewritten bench-relative per the environment-disclosure
rule in `methodology.md`, nothing else changed)

```
devscout audit --semantic  root bench/corpora/csharp  oracle 112190 records / 66924 sites  units ok 56 failed 0  method units
tier        edges     tp     fp   precision   fp:no-site  fp:external  fp:wrong  structural
precise     22894  21786   1108       0.952          530           22       556          14
heuristic   14992   5386   9606       0.359          425         4606      4575        1856
recall (56713 in-graph member sites)  precise 0.381  precise+ext 0.381  all 0.481
  by receiver  ident 0.561  qualified 0.218  this 0.000  base 0.000  call 0.033
external sites 19153  silent-correct 16556  leaked 2597
fan-out  1: 21590  2: 4045  3: 2097  4+: 448
top fp targets   ResponseHandlerConnectHandle 443  HandlerConnectHandle 441  InMemoryDelayProvider 409  OneMessageConsumer 351  Retry 213  Instance 209  MemoryBufferWriter 206  InMemoryContainerTestFixture 191  CodePrinter 181  ReadOnlyProperty 175  OptionValueCollection 174  IPerformanceCounter 165  NullPerformanceCounter 165  StatsDPerformanceCounter 165  BusRegistrationContext 162  IBusRegistrationContext 161  RoutingSlipExtensions 161  ConcurrentHashSet 139  SingleThreadedDictionary 125  ClosureInfo 101
top missed       MassTransit.ConsumeContext 1631  MassTransit.SendContext 931  MassTransit.MessageContext 823  MassTransit.ISendEndpoint 734  MassTransit.BehaviorContext 678  MassTransit.Testing.IBaseTestHarness 634  MassTransit.Testing.ITestHarness 600  MassTransit.PipeContext 538  MassTransit.IPublishEndpoint 495  MassTransit.SagaConsumeContext 482  MassTransit.TransitionExtensions 449  MassTransit.IRegistrationConfigurator 423  MassTransit.IStateMachineModifier 340  MassTransit.ThenExtensions 315  MassTransit.Testing.BusTestHarness 284  MassTransit.DependencyInjectionTestingExtensions 282  MassTransit.Testing.IReceivedMessageList 278  MassTransit.IReceiveConfigurator 277  MassTransit.Testing.IPublishedMessageList 272  MassTransit.TestStateMachineExtensions 270
unknown targets  enum-member 30  class 16  struct 5  delegate 1  interface 1
partial file mismatch 65
ambiguous 137
```

### `--json` tier objects (verbatim)

```json
{
  "precise": {
    "edges": 22894,
    "tp": 21786,
    "fp": 1108,
    "precision": 0.952,
    "fp_no_site": 530,
    "fp_external_site": 22,
    "fp_wrong_target": 556,
    "structural": 14
  },
  "heuristic": {
    "edges": 14992,
    "tp": 5386,
    "fp": 9606,
    "precision": 0.359,
    "fp_no_site": 425,
    "fp_external_site": 4606,
    "fp_wrong_target": 4575,
    "structural": 1856
  }
}
```

### Recall by receiver kind

Denominator 56713 in-graph member sites (`shape == "access"`, `external == false`, target known
to the graph).

| Receiver kind | Recall |
| --- | --- |
| `ident` | 0.561 |
| `qualified` | 0.218 |
| `this` | 0.000 |
| `base` | 0.000 |
| `call` | 0.033 |
| **all** | **0.481** (precise-only 0.381, precise+ext 0.381) |

`conditional` (`?.`): 709 records. `bare` (unqualified invocation): 7512 records. Both excluded
from the headline recall figure per the methodology's documented recall bound.

### External-receiver leak

19153 external sites: 16556 silent-correct (no edge emitted), 2597 leaked (≥1 guess/heuristic
edge emitted anyway) — 13.6% of external sites leak, 17.3% of `heuristic`-tier edges (2597 /
14992).

### Fan-out (candidate count per site)

| 1 | 2 | 3 | 4+ |
| --- | --- | --- | --- |
| 21590 | 4045 | 2097 | 448 |

### Top-20 FP targets, by short name

| Target | FP count |
| --- | --- |
| ResponseHandlerConnectHandle | 443 |
| HandlerConnectHandle | 441 |
| InMemoryDelayProvider | 409 |
| OneMessageConsumer | 351 |
| Retry | 213 |
| Instance | 209 |
| MemoryBufferWriter | 206 |
| InMemoryContainerTestFixture | 191 |
| CodePrinter | 181 |
| ReadOnlyProperty | 175 |
| OptionValueCollection | 174 |
| IPerformanceCounter | 165 |
| NullPerformanceCounter | 165 |
| StatsDPerformanceCounter | 165 |
| BusRegistrationContext | 162 |
| IBusRegistrationContext | 161 |
| RoutingSlipExtensions | 161 |
| ConcurrentHashSet | 139 |
| SingleThreadedDictionary | 125 |
| ClosureInfo | 101 |

### Top-20 missed targets, by id

| Target id | Missed count |
| --- | --- |
| MassTransit.ConsumeContext | 1631 |
| MassTransit.SendContext | 931 |
| MassTransit.MessageContext | 823 |
| MassTransit.ISendEndpoint | 734 |
| MassTransit.BehaviorContext | 678 |
| MassTransit.Testing.IBaseTestHarness | 634 |
| MassTransit.Testing.ITestHarness | 600 |
| MassTransit.PipeContext | 538 |
| MassTransit.IPublishEndpoint | 495 |
| MassTransit.SagaConsumeContext | 482 |
| MassTransit.TransitionExtensions | 449 |
| MassTransit.IRegistrationConfigurator | 423 |
| MassTransit.IStateMachineModifier | 340 |
| MassTransit.ThenExtensions | 315 |
| MassTransit.Testing.BusTestHarness | 284 |
| MassTransit.DependencyInjectionTestingExtensions | 282 |
| MassTransit.Testing.IReceivedMessageList | 278 |
| MassTransit.IReceiveConfigurator | 277 |
| MassTransit.Testing.IPublishedMessageList | 272 |
| MassTransit.TestStateMachineExtensions | 270 |

### Predictions vs. actual

| Prediction | Actual | Verdict |
| --- | --- | --- |
| precise precision ≥ 0.95 | 0.952 | **met**, 0.002 above the line — thin margin, see Defects |
| ext precision ≥ 0.95 | not separable this run | **not evaluable** — `ext` is merged into `heuristic` until the `tier` key lands |
| guess precision < 0.30 | not separable this run; merged `heuristic` precision 0.359 | **not evaluable** as worded; the merged proxy is above the threshold, driven up by ext-tier edges mixed in |
| recall(precise) 0.45–0.65 | 0.381 | **miss**, below the band |
| leaked external sites > 50% of guess edges | not separable this run; leaked/`heuristic`-edges = 17.3% (2597 / 14992) | **not evaluable** as worded; the merged proxy falls far short of 50% |

Falsification clause: precise precision (0.952) did not cross below 0.95, so it is not
falsified on its own terms — but see Defects below for why the margin and the tier's own FP
subclasses (`fp_wrong_target` 556, `structural` 14) argue against reading this as a clean pass.
The top-20 FP list above is published regardless, per the clause's instruction.

## Defects this run found in its own method

1. **Three of five predictions are not evaluable pre-tier-split.** `ext` and `guess` predictions
   were written assuming the `tier` key (W2) would exist by Run 0; it has not landed, so the
   audit reports one `heuristic` bucket mixing both. The `heuristic`-precision and
   leaked/`heuristic`-edges figures above are the closest available proxies, not the metrics the
   predictions actually name — Run 1 (once tiers split) is the first point these can be checked
   as registered.
2. **The `precise` tier's own FP subclasses cut against treating 0.952 as a clean pass.** Of its
   1108 false positives, 556 are `fp_wrong_target` and 14 are flagged `structural` — a
   structurally impossible edge in the tier meant to be the audit's floor is unexpected and
   worth a defect ticket independent of the aggregate number clearing 0.95.
3. **`this`/`base` receiver kinds recall 0.000, `call` recalls 0.033.** Expected from the
   extractor's documented qualifier list (`src/extract.rs:697-720` per the design doc: `this`,
   `base`, and invocation-result qualifiers are not accepted as `uses-member` qualifiers today),
   but it means recall(all) 0.481 is propped up almost entirely by `ident` (0.561) and
   `qualified` (0.218) sites — the headline number hides a near-total miss on three of five
   receiver kinds.
4. **Two units carry heavy compiler diagnostics while still scoring `status: "ok"`.**
   `MassTransit.MartenIntegration.Tests` (net8.0) references three net9.0 projects it cannot
   legally reference (871 diagnostics, all from the same TFM-mismatch class logged during load);
   `MassTransit.Analyzers` (netstandard2.0) carries 2649 diagnostics. Neither trips `status !=
   "ok"` (`GetCompilationAsync()` still returns a compilation), so both units' files stay in the
   scored universe, and cross-project member calls out of them likely resolve to `external` or
   miss rather than their real in-tree target — a quieter failure mode than an outright load
   failure, and one `--strict` would not catch either, since it only checks for failed projects.
5. **NuGetAudit restore warnings and real project-reference errors share one text channel.**
   The oracle's `WorkspaceFailed` collection (25 diagnostics this run) mixes NuGet
   vulnerability-advisory noise (e.g. `Package 'MessagePack' 3.1.4 has a known ... severity
   vulnerability`) with the one real defect above (the MartenIntegration.Tests TFM mismatch) in
   the same `failure: Msbuild failed when processing the file ...` format — indistinguishable at
   a glance in `run.log`; a reader has to open the log and read the message text to separate
   audit noise from an actual load defect.
6. **137 ambiguous sites, 65 partial file mismatches.** Both small relative to 112190 records
   (0.12% and 0.06%), reported here as the first baseline reading — worth tracking for drift
   across future runs rather than a defect in themselves.
7. **Zero failed units.** All 56 projects returned a compilation; `--strict` would have exited 0
   this run. The load-failure fail-open path (§10 risk 1 in the design doc) was not exercised by
   this corpus.

## Run 1 — after resolver changes

Not yet run. Append here only if `bench/corpus.lock`'s `csharp` pin and the oracle commit
(`564739a`) are both unchanged from this document's Environment block — otherwise Run 0 must be
re-baselined first, since a predicted figure is a claim about Run 0's bucket sizes specifically.
