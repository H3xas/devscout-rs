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

`bench/corpus.lock`'s `csharp` pin (`855cf1752c94ca9498e0c45ce8d09fdc9e957dd6`) and the oracle
commit (`564739a`) are unchanged from Run 0's Environment block — verified: `refs.jsonl` still
112190 records / 66924 sites, `units.jsonl` still 56 units, both byte-identical in record count to
Run 0's numbers above. No re-baseline needed; this run's predicted-figure claims are still claims
about Run 0's own bucket sizes.

This round lands the `tier` key (W2) plus a csproj project model (W3: unit discovery, admission on
the heuristic tiers, per-unit global usings, ambiguous-narrowing by project reachability) and a set
of scored-tier resolver fixes (a call-shaped ref no longer vouched by a property/field; a guess
under an external receiver must be nominally assignable). It also refines `audit --semantic` itself
(member-aware join on `(file, startLine, member)`, an oracle-covered-universe filter applied to
graph edges as well as oracle records, `structural` narrowed to false positives only). **That last
part matters for reading the deltas below**: some movement between Run 0 and Run 1 is the
measurement rule changing under the resolver, not only the resolver itself — see Defect 8.

### Environment delta

```
Date            2026-09-03
Devscout        commit f2ac770 (crate version unchanged, 0.3.0) — d833ef3..f2ac770 on top of Run
                0's ba39a96 baseline: schema 2 (tier/member on uses-member edges), csproj project
                model + admission + per-unit global usings, ambiguous narrowing by project
                reachability, call-shape/external-receiver resolver fixes, and the audit refinement
                above
Build           cargo build --release, rustc 1.97.1 (8bab26f4f 2026-07-14), aarch64-apple-darwin
Corpus/Oracle   unchanged — see verification note above
devscout map .  5634 files, cold rebuild in 1.91s (was 1.67s): 9956 defs (unchanged), 126216 edges
                (was 130366 — the project model's admission rules drop some heuristic edges rather
                than emit them), 59 project units discovered, stats.heuristic_by_tier ext 1057 /
                guess 9758 (raw, pre-audit-universe-filter; audit's own tiers below apply the
                oracle-covered-universe cut on top, see the `edges outside universe` line)
devscout audit  text + --json, <5s combined
Host            macOS (Darwin 25.2.0), Apple M2 Max, arm64, 12 cores, 64 GiB RAM, otherwise idle —
                same workstation as Run 0
```

### Audit text output (verbatim; `root` rewritten bench-relative, nothing else changed)

```
devscout audit --semantic  root bench/corpora/csharp  oracle 112190 records / 66924 sites  units ok 56 failed 0  method units
tier        edges     tp     fp   precision   fp:no-site  fp:external  fp:wrong  structural
precise     22510  21804    706       0.969           55           52       599          12
ext          1047    850    197       0.812            0           15       182           0
guess        9579   4808   4771       0.502           48         2294      2429           0
recall (56713 in-graph member sites)  precise 0.379  precise+ext 0.394  all 0.479
  by receiver  ident 0.560  qualified 0.217  this 0.000  base 0.000  call 0.004
external sites 19153  silent-correct 18055  leaked 1098
fan-out  1: 20816  2: 3573  3: 1233  4+: 343
top fp targets   InMemoryDelayProvider 295  Retry 211  CodePrinter 180  RoutingSlipExtensions 161  BusRegistrationContext 158  IBusRegistrationContext 157  IPerformanceCounter 148  NullPerformanceCounter 148  StatsDPerformanceCounter 148  InMemoryContainerTestFixture 135  ClosureInfo 103  Instance 102  ContainerTestHarness 78  Message 78  ToCSharpPrinter 73  TextTableOptions 65  ConsumerPipeConfiguratorExtensions 55  SagaPipeConfiguratorExtensions 55  Tools 52  IIndexedSagaProperty 50
top missed       MassTransit.ConsumeContext 1705  MassTransit.SendContext 932  MassTransit.MessageContext 823  MassTransit.ISendEndpoint 734  MassTransit.BehaviorContext 684  MassTransit.Testing.IBaseTestHarness 634  MassTransit.Testing.ITestHarness 600  MassTransit.PipeContext 538  MassTransit.IPublishEndpoint 495  MassTransit.SagaConsumeContext 482  MassTransit.TransitionExtensions 449  MassTransit.IRegistrationConfigurator 423  MassTransit.IStateMachineModifier 340  MassTransit.ThenExtensions 315  MassTransit.DependencyInjectionTestingExtensions 297  MassTransit.Testing.BusTestHarness 284  MassTransit.Testing.IReceivedMessageList 278  MassTransit.IReceiveConfigurator 277  MassTransit.Testing.IPublishedMessageList 272  MassTransit.TestStateMachineExtensions 269
unknown targets  enum-member 30  class 16  struct 5  delegate 1  interface 1
partial file mismatch 65
ambiguous 137
edges outside universe (not judged) 675
```

### `--json` tier objects (verbatim)

```json
{
  "precise": {
    "edges": 22510,
    "tp": 21804,
    "fp": 706,
    "precision": 0.969,
    "fp_no_site": 55,
    "fp_external_site": 52,
    "fp_wrong_target": 599,
    "structural": 12
  },
  "ext": {
    "edges": 1047,
    "tp": 850,
    "fp": 197,
    "precision": 0.812,
    "fp_no_site": 0,
    "fp_external_site": 15,
    "fp_wrong_target": 182,
    "structural": 0
  },
  "guess": {
    "edges": 9579,
    "tp": 4808,
    "fp": 4771,
    "precision": 0.502,
    "fp_no_site": 48,
    "fp_external_site": 2294,
    "fp_wrong_target": 2429,
    "structural": 0
  }
}
```

### Recall by receiver kind

Denominator 56713 in-graph member sites (same as Run 0 — oracle unchanged).

| Receiver kind | Run 0 | Run 1 |
| --- | --- | --- |
| `ident` | 0.561 | 0.560 |
| `qualified` | 0.218 | 0.217 |
| `this` | 0.000 | 0.000 |
| `base` | 0.000 | 0.000 |
| `call` | 0.033 | 0.004 |
| **all** | **0.481** (precise-only 0.381) | **0.479** (precise 0.379, precise+ext 0.394) |

`conditional` (`?.`): 709 records. `bare` (unqualified invocation): 7512 records. Both still
excluded from the headline recall figure. `call` recall fell from 0.033 to 0.004 — worth watching,
though the absolute record count behind it is small; see Defect 9.

### External-receiver leak

19153 external sites (unchanged — oracle-derived, not resolver-derived): 18055 silent-correct (was
16556), 1098 leaked (was 2597) — **5.7% of external sites now leak, down from 13.6%**. As a share of
the tier that can leak, 1098 / 9579 guess-tier edges = **11.5%** (Run 0's merged proxy, leaked /
`heuristic`-edges, was 17.3% — see "Predictions vs. actual" for why the two are not the same
denominator).

### Fan-out (candidate count per site)

| 1 | 2 | 3 | 4+ |
| --- | --- | --- | --- |
| 20816 | 3573 | 1233 | 343 |

Run 0: 21590 / 4045 / 2097 / 448. Every bucket shrank — fewer sites now carry an unresolved
multi-candidate guess, consistent with the project model narrowing ambiguity by reachability.

### Top-20 FP targets, by short name

| Target | FP count |
| --- | --- |
| InMemoryDelayProvider | 295 |
| Retry | 211 |
| CodePrinter | 180 |
| RoutingSlipExtensions | 161 |
| BusRegistrationContext | 158 |
| IBusRegistrationContext | 157 |
| IPerformanceCounter | 148 |
| NullPerformanceCounter | 148 |
| StatsDPerformanceCounter | 148 |
| InMemoryContainerTestFixture | 135 |
| ClosureInfo | 103 |
| Instance | 102 |
| ContainerTestHarness | 78 |
| Message | 78 |
| ToCSharpPrinter | 73 |
| TextTableOptions | 65 |
| ConsumerPipeConfiguratorExtensions | 55 |
| SagaPipeConfiguratorExtensions | 55 |
| Tools | 52 |
| IIndexedSagaProperty | 50 |

Run 0's top two FP targets (`ResponseHandlerConnectHandle` 443, `HandlerConnectHandle` 441) are gone
from this list entirely — resolved correctly now, not just demoted in rank.

### Top-20 missed targets, by id

| Target id | Missed count |
| --- | --- |
| MassTransit.ConsumeContext | 1705 |
| MassTransit.SendContext | 932 |
| MassTransit.MessageContext | 823 |
| MassTransit.ISendEndpoint | 734 |
| MassTransit.BehaviorContext | 684 |
| MassTransit.Testing.IBaseTestHarness | 634 |
| MassTransit.Testing.ITestHarness | 600 |
| MassTransit.PipeContext | 538 |
| MassTransit.IPublishEndpoint | 495 |
| MassTransit.SagaConsumeContext | 482 |
| MassTransit.TransitionExtensions | 449 |
| MassTransit.IRegistrationConfigurator | 423 |
| MassTransit.IStateMachineModifier | 340 |
| MassTransit.ThenExtensions | 315 |
| MassTransit.DependencyInjectionTestingExtensions | 297 |
| MassTransit.Testing.BusTestHarness | 284 |
| MassTransit.Testing.IReceivedMessageList | 278 |
| MassTransit.IReceiveConfigurator | 277 |
| MassTransit.Testing.IPublishedMessageList | 272 |
| MassTransit.TestStateMachineExtensions | 269 |

Nearly the same list and nearly the same counts as Run 0 (same interface-heavy, `this`/`base`/`call`
receiver-kind miss the extractor still does not accept as a qualifier — Defect 3 below, unchanged).

### Predictions vs. actual

The `tier` key has landed, so all five of the design doc's registered predictions are directly
checkable for the first time (Run 0 could only evaluate `precise` and `recall(precise)`).

| Prediction | Actual | Verdict |
| --- | --- | --- |
| precise precision ≥ 0.95 | 0.969 (22510 edges, 706 fp) | **met** — wider margin than Run 0's 0.952, though see Defect 8: part of the FP mix shifted (`fp_no_site` 530→55, `fp_wrong_target` 556→599) because the audit's own join rule changed alongside the resolver, not from the resolver alone |
| ext precision ≥ 0.95 | 0.812 as measured (1047 edges, 197 fp) | **not met** as measured, but the shortfall is concentrated in one vendored file — excluding it, 0.978 (869 edges, 19 fp), which **would meet** the line; see Defect 10 |
| guess precision < 0.30 | 0.502 (9579 edges, 4771 fp) | **not met** — `guess` is far more precise than the ceiling the prediction feared, the inverse direction of a miss |
| recall(precise) 0.45–0.65 | 0.379 | **miss**, same band-miss as Run 0's 0.381 — essentially unmoved, 0.071 below the low end |
| leaked external sites > 50% of guess edges | 1098 / 9579 = 11.5% | **not met**, far short of 50% (Run 0's merged proxy was 17.3%, also short) |

Falsification clause: precise precision (0.969) does not cross below 0.95, so it is not falsified on
its own terms this round either — the FP-subclass caveat from Run 0's Defect 2 still argues against
reading it as an unqualified clean pass (see Defect 8's updated numbers).

## Defects this run found in its own method

Carried forward from Run 0, still applicable:

1. ~~Three of five predictions not evaluable pre-tier-split~~ — **resolved this run**: the `tier`
   key landed (W2), so all five predictions are directly checkable above.
2. **The `precise` tier's own FP subclasses still cut against an unqualified clean pass.** Of 706
   FPs, 599 are `fp_wrong_target` and 12 are `structural` (was 556 / 14 of 1108 in Run 0) — the
   `structural` count is now FP-only by construction (Defect 8), so 12 is not directly comparable to
   Run 0's 14, but `fp_wrong_target`'s share of `precise` FPs actually grew (85% vs 50% in Run 0).
3. **`this`/`base` receiver kinds still recall 0.000; `call` fell further, to 0.004** (was 0.033).
   Expected from the extractor's still-unchanged qualifier list (`this`, `base`, invocation-result
   qualifiers not accepted as `uses-member` qualifiers) — recall(all) 0.479 is still propped up
   almost entirely by `ident` (0.560) and `qualified` (0.217). See Defect 9 for the `call` drop.
4. **Two units still carry heavy compiler diagnostics while scoring `status: "ok"`** — oracle-side,
   unchanged this run (same oracle commit and units.jsonl as Run 0): `MassTransit.MartenIntegration.Tests`
   (TFM-mismatch class, 871 diagnostics) and `MassTransit.Analyzers` (2649 diagnostics). Neither
   trips `status != "ok"`.
5. **NuGetAudit restore warnings and real project-reference errors still share one text channel** in
   the oracle's `WorkspaceFailed` collection — oracle-side, unchanged.
6. **137 ambiguous sites, 65 partial file mismatches — identical to Run 0**, digit for digit. Either
   genuinely stable or a sign the resolver changes this round don't touch the code paths that
   produce these two counts; worth a closer look if Run 2 reproduces the exact same numbers again.
7. **Zero failed units, unchanged.** `--strict` would still exit 0 on this corpus.

New this run:

8. **The audit tool changed alongside the resolver, confounding a clean before/after.** Commit
   `640807a` (`fix(audit): member-aware join, oracle-covered universe, structural on FPs only`) is
   part of this round's integrated branch but landed on the resolver side of the tree, not on Run
   0's baseline (`3de5657`, off `ba39a96`, predates it). Three of its effects are visible in the
   deltas above and are measurement-rule artifacts, not resolver improvements on their own: the
   `edges outside universe` line (675 edges this run, absent from Run 0's output entirely — Run 0's
   audit did not filter graph edges to the oracle-compiled universe, only oracle records); the
   `fp_no_site` collapse in `precise` (530 → 55), largely member-aware join reclassifying same-line,
   different-member matches that used to read as "no site" into `fp_wrong_target` or `tp` correctly;
   and `structural` now counting false positives only (`TierStats.structural`'s own doc comment),
   where Run 0's figure may have included TPs at structurally-unusual sites. None of this invalidates
   the Run 0 → Run 1 comparison — the corpus, oracle, and registered predictions are all unchanged —
   but a reader should not attribute 100% of the `precise`-tier FP-mix shift to the resolver fixes
   alone.
9. **`call`-receiver recall fell from 0.033 to 0.004.** Both figures sit on a small denominator (the
   extractor's qualifier list still excludes invocation-result receivers by design, per Defect 3), so
   this may be noise from the project model narrowing a handful of previously-lucky guesses rather
   than a regression — flagged for Run 2 to confirm the direction before treating it as a trend.
10. **`ext`-tier precision is dominated by one vendored file whose namespace is chosen by
    `#if`/`#else`.** `src/MassTransit/Internals/Reflection/ExpressionCompiler.cs` accounts for 178 of
    the tier's 1047 edges (17%) and **all 178 are false positives** — every one of the tier's other
    869 edges is a true positive except 19. Recomputed directly from graph.json + the oracle
    (excluding edges whose `from_file` ends with that path, same join/match rules as `audit.rs`):
    **1047 edges / 850 tp / 197 fp / precision 0.812 including the file; 869 edges / 850 tp / 19 fp /
    precision 0.978 excluding it.** The registered `ext ≥ 0.95` prediction reads as a clear miss
    without this exclusion and a clear pass with it — the file's `#if`/`#else`-selected namespace is
    a known extractor blind spot (a preprocessor-conditional symbol the tree-sitter-based extractor
    cannot evaluate), not a representative sample of `ext`-tier behavior elsewhere in the corpus.

## Runs 2 and 3 — registered predictions (2026-09-03)

Two further runs on the same corpus pin and the same oracle output follow the extractor recall
work, so every denominator is identical to Run 1. Run 2 comes after the receiver-shape changes
(`this.`, `base.`, `?.` bindings, `await` look-through for local facts, cast, declaration-pattern
and typed `out` designations). Run 3 comes after cross-file field facts (a field declared in a
sibling partial-class file or on a base type types the receiver), one-hop call-chain tails and
element typing of a single-parameter lambda on a collection-typed receiver. Predictions, registered
before Run 2 numbers are seen:

| metric | Run 1 | Run 2 predicted | Run 3 predicted |
| --- | --- | --- | --- |
| recall `this` | 0.000 | ≥ 0.60 | ≥ 0.60 |
| recall `base` | 0.000 | ≥ 0.50 | ≥ 0.50 |
| recall `call` | 0.004 | unchanged | ≥ 0.30 |
| recall `ident` | 0.560 | unchanged | ≥ 0.60 |
| recall all | 0.479 | ≥ 0.52 | ≥ 0.55 |
| recall precise+ext | 0.394 | ≥ 0.42 | ≥ 0.47 |
| precise precision | 0.969 | ≥ 0.964 | ≥ 0.964 |
| guess precision | 0.502 | ≥ 0.48 | ≥ 0.48 |
| leaked external sites | 1098 | ≤ 1098 | ≤ 1098 |

Falsification. Precise precision below 0.964 on either run means the new receiver typing binds
wrong types, and that wave is reverted before the next one is measured. A `this` bucket still below
0.60 after Run 2 means the misses are not receiver-shape misses; they are bucketed by target kind
before the field-fact wave lands. A `call` bucket still below 0.30 after Run 3 means one
method-return hop is not where the chain misses are.

Decision rule, fixed now. If recall precise+ext is at or above 0.70 after Run 3, the question of a
compiler-backed enrichment layer is closed as not worth its dependency. Below that, the remaining
misses are bucketed by receiver kind and target kind, and an enrichment layer is designed against
the record contract the oracle already emits (`receiverText`, `receiver`, `target`), cached and
never on the hook path.

## Run 2 — after the receiver-shape wave

Same corpus pin and oracle output as Run 1 (denominators identical). Devscout is at the branch
head that adds `this`/`base`/`?.` receivers, `await` look-through, cast, pattern and typed `out`
facts, base-qualified member binding and awaited task unwrapping. Fragment cache v16 (full
reparse); graph rebuilt in 1.51s, 9956 defs, 126663 edges; `devscout map` wall time 3.6s.

### Audit text output (verbatim; `root` rewritten bench-relative, nothing else changed)

```
devscout audit --semantic  root bench/corpora/csharp  oracle 112190 records / 66924 sites  units ok 56 failed 0  method units
tier        edges     tp     fp   precision   fp:no-site  fp:external  fp:wrong  structural
precise     23117  22394    723       0.969           55           55       613          12
ext          1083    876    207       0.809            0           15       192           0
guess        9375   4771   4604       0.509           48         2183      2373           0
recall (56713 in-graph member sites)  precise 0.386  precise+ext 0.401  all 0.485
  by receiver  ident 0.565  qualified 0.216  this 0.066  base 0.248  call 0.004
external sites 19153  silent-correct 18060  leaked 1093
fan-out  1: 21260  2: 3611  3: 1210  4+: 340
top fp targets   InMemoryDelayProvider 295  Retry 211  CodePrinter 179  RoutingSlipExtensions 161  BusRegistrationContext 158  IBusRegistrationContext 157  IPerformanceCounter 148  NullPerformanceCounter 148  StatsDPerformanceCounter 148  InMemoryContainerTestFixture 135  ClosureInfo 103  Instance 102  ContainerTestHarness 78  Message 78  ToCSharpPrinter 71  TextTableOptions 65  ConsumerPipeConfiguratorExtensions 55  SagaPipeConfiguratorExtensions 55  Tools 54  IIndexedSagaProperty 50
top missed       MassTransit.ConsumeContext 1702  MassTransit.SendContext 932  MassTransit.MessageContext 823  MassTransit.ISendEndpoint 731  MassTransit.BehaviorContext 684  MassTransit.Testing.IBaseTestHarness 634  MassTransit.Testing.ITestHarness 600  MassTransit.PipeContext 538  MassTransit.IPublishEndpoint 495  MassTransit.SagaConsumeContext 482  MassTransit.TransitionExtensions 449  MassTransit.IRegistrationConfigurator 423  MassTransit.IStateMachineModifier 340  MassTransit.ThenExtensions 315  MassTransit.DependencyInjectionTestingExtensions 297  MassTransit.Testing.BusTestHarness 284  MassTransit.Testing.IReceivedMessageList 278  MassTransit.IReceiveConfigurator 277  MassTransit.Testing.IPublishedMessageList 272  MassTransit.TestStateMachineExtensions 269
unknown targets  enum-member 30  class 16  struct 5  delegate 1  interface 1
partial file mismatch 65
ambiguous 137
edges outside universe (not judged) 683
```

### `--json` tier objects (verbatim)

```json
{
  "precise": {
    "edges": 23117,
    "tp": 22394,
    "fp": 723,
    "precision": 0.969,
    "fp_no_site": 55,
    "fp_external_site": 55,
    "fp_wrong_target": 613,
    "structural": 12
  },
  "ext": {
    "edges": 1083,
    "tp": 876,
    "fp": 207,
    "precision": 0.809,
    "fp_no_site": 0,
    "fp_external_site": 15,
    "fp_wrong_target": 192,
    "structural": 0
  },
  "guess": {
    "edges": 9375,
    "tp": 4771,
    "fp": 4604,
    "precision": 0.509,
    "fp_no_site": 48,
    "fp_external_site": 2183,
    "fp_wrong_target": 2373,
    "structural": 0
  }
}
```

### Recall by receiver kind

Denominator 56713 in-graph member sites (same as Run 1 — oracle unchanged).

| Receiver kind | Run 1 | Run 2 |
| --- | --- | --- |
| `ident` | 0.560 | 0.565 |
| `qualified` | 0.217 | 0.216 |
| `this` | 0.000 | 0.066 |
| `base` | 0.000 | 0.248 |
| `call` | 0.004 | 0.004 |
| **all** | **0.479** (precise 0.379, precise+ext 0.394) | **0.485** (precise 0.386, precise+ext 0.401) |

`conditional` (`?.`): 709 records. `bare` (unqualified invocation): 7512 records. Both still
excluded from the headline recall figure.

### Predictions vs. actual

| Prediction | Actual | Verdict |
| --- | --- | --- |
| recall `this` ≥ 0.60 | 0.066 | **FAIL** |
| recall `base` ≥ 0.50 | 0.248 | **FAIL** |
| recall `call` unchanged | 0.004 | **HOLD** |
| recall `ident` unchanged | 0.565 | **HOLD** |
| recall all ≥ 0.52 | 0.485 | **FAIL** |
| recall precise+ext ≥ 0.42 | 0.401 | **FAIL** |
| precise precision ≥ 0.964 | 0.969 | **HOLD** |
| guess precision ≥ 0.48 | 0.509 | **HOLD** |
| leaked external sites ≤ 1098 | 1093 | **HOLD** |

The falsification clause fired, so every missed `this` and `base` site was bucketed before the next
wave. Two facts came out. First, the two buckets together are 528 sites, 0.93% of the 56713-site
denominator, so the registered thresholds could never have moved the aggregate by more than 0.008;
they were a poor proxy for the wave, which is recorded here as a methodology defect rather than
explained away. Second, the misses are not receiver-shape misses. All 199 missed `this` sites call
an extension method whose `this` parameter is an interface or base type the enclosing type
implements; the extension tier keys its index on the exact receiver name and never walks the
receiver's base closure. 230 of the 237 missed `base` sites name a non-public member of the base,
which the fragment's member lists do not record because they hold public members only; the eight
`base.` edges that bind to the wrong target bind to an interface base. Both classes are language
rules, not heuristics, and are fixed in the next wave under the predictions below.

## Run 2b — registered predictions (2026-09-03)

| metric | Run 2 | Run 2b predicted |
| --- | --- | --- |
| recall `this` | 0.066 | ≥ 0.80 |
| recall `base` | 0.248 | ≥ 0.85 |
| recall precise+ext | 0.401 | ≥ 0.41 |
| recall all | 0.485 | ≥ 0.49 |
| precise precision | 0.969 | ≥ 0.964 |
| ext precision | 0.809 | ≥ 0.80 |
| guess precision | 0.509 | ≥ 0.48 |
| leaked external sites | 1093 | ≤ 1098 |

Falsification. Ext-tier precision below 0.80 means walking the receiver's base closure binds
extension methods the language would not; the walk is reverted. Any guess-tier edge vouched by a
non-public member is a defect in the visibility split and reverts the split.

## Run 2b — after the hierarchy wave

Same corpus pin and oracle output as Runs 1 and 2 (denominators identical). Devscout is at the
branch head that adds non-public member lists for hierarchy-internal receivers, an interface skip
in the `base.` lookup, base-closure walks in the extension and typed-receiver tiers, and
field-type facts merged across partial files (cross-file receiver typing).

Measured on the corpus state left by Run 2: the fragment cache literal did not change between the
two builds, so the def tables this wave added were read back empty for every file. Run 2b
therefore measures the resolver rules on stale fragments; the attribution below re-measured on a
wiped state where it mattered, and every later run wipes the indexer state first. This is
recorded as a benchmark-method defect.

### Audit text output (verbatim; `root` rewritten bench-relative, nothing else changed)

```
devscout audit --semantic  root bench/corpora/csharp  oracle 112190 records / 66924 sites  units ok 56 failed 0  method units
tier        edges     tp     fp   precision   fp:no-site  fp:external  fp:wrong  structural
precise     25248  24316    932       0.963           64           55       813          12
ext          2061   1847    214       0.896            5           17       192           0
guess        9164   4687   4477       0.511           48         2155      2274           0
recall (56713 in-graph member sites)  precise 0.419  precise+ext 0.452  all 0.534
  by receiver  ident 0.624  qualified 0.216  this 0.526  base 0.248  call 0.004
external sites 19153  silent-correct 18086  leaked 1067
fan-out  1: 23275  2: 3862  3: 1332  4+: 343
top fp targets   InMemoryDelayProvider 295  Retry 211  CodePrinter 179  RoutingSlipExtensions 161  BusRegistrationContext 158  IBusRegistrationContext 157  IPerformanceCounter 148  NullPerformanceCounter 148  StatsDPerformanceCounter 148  InMemoryContainerTestFixture 107  ClosureInfo 103  Instance 102  ContainerTestHarness 78  Message 78  ToCSharpPrinter 71  TextTableOptions 65  ConsumerPipeConfiguratorExtensions 55  ISendEndpoint 55  SagaPipeConfiguratorExtensions 55  Tools 54
top missed       MassTransit.ConsumeContext 1702  MassTransit.SendContext 932  MassTransit.MessageContext 823  MassTransit.BehaviorContext 684  MassTransit.Testing.IBaseTestHarness 634  MassTransit.Testing.ITestHarness 600  MassTransit.PipeContext 538  MassTransit.IPublishEndpoint 495  MassTransit.SagaConsumeContext 482  MassTransit.TransitionExtensions 449  MassTransit.IRegistrationConfigurator 423  MassTransit.IStateMachineModifier 340  MassTransit.ThenExtensions 315  MassTransit.DependencyInjectionTestingExtensions 294  MassTransit.Testing.IReceivedMessageList 278  MassTransit.IReceiveConfigurator 275  MassTransit.Testing.IPublishedMessageList 272  MassTransit.TestStateMachineExtensions 269  MassTransit.ISendEndpoint 222  MassTransit.IProbeSite 179
unknown targets  enum-member 30  class 16  struct 5  delegate 1  interface 1
partial file mismatch 156
ambiguous 137
edges outside universe (not judged) 700
```

### `--json` tier objects (verbatim)

```json
{
  "precise": {
    "edges": 25248,
    "tp": 24316,
    "fp": 932,
    "precision": 0.963,
    "fp_no_site": 64,
    "fp_external_site": 55,
    "fp_wrong_target": 813,
    "structural": 12
  },
  "ext": {
    "edges": 2061,
    "tp": 1847,
    "fp": 214,
    "precision": 0.896,
    "fp_no_site": 5,
    "fp_external_site": 17,
    "fp_wrong_target": 192,
    "structural": 0
  },
  "guess": {
    "edges": 9164,
    "tp": 4687,
    "fp": 4477,
    "precision": 0.511,
    "fp_no_site": 48,
    "fp_external_site": 2155,
    "fp_wrong_target": 2274,
    "structural": 0
  }
}
```

### Recall by receiver kind

Denominator 56713 in-graph member sites (same as Run 1 and Run 2 — oracle unchanged).

| Receiver kind | Run 2 | Run 2b |
| --- | --- | --- |
| `ident` | 0.565 | 0.624 |
| `qualified` | 0.216 | 0.216 |
| `this` | 0.066 | 0.526 |
| `base` | 0.248 | 0.248 |
| `call` | 0.004 | 0.004 |
| **all** | **0.485** (precise 0.386, precise+ext 0.401) | **0.534** (precise 0.419, precise+ext 0.452) |

`conditional` (`?.`): 709 records. `bare` (unqualified invocation): 7512 records. Both still
excluded from the headline recall figure.

### Predictions vs. actual

| Prediction | Actual | Verdict |
| --- | --- | --- |
| recall `this` ≥ 0.80 | 0.526 | **FAIL** |
| recall `base` ≥ 0.85 | 0.248 | **FAIL** (stale fragments, see below) |
| recall precise+ext ≥ 0.41 | 0.452 | **HOLD** |
| recall all ≥ 0.49 | 0.534 | **HOLD** |
| precise precision ≥ 0.964 | 0.963 | **FAIL** |
| ext precision ≥ 0.80 | 0.896 | **HOLD** |
| guess precision ≥ 0.48 | 0.511 | **HOLD** |
| leaked external sites ≤ 1098 | 1067 | **HOLD** |

### Attribution of the precise-tier change

Every precise edge that appeared or disappeared between Run 2 and Run 2b was scored against the
oracle: 2139 new edges (1912 true, 206 wrong target, 21 at sites the oracle has no in-graph record
for) and 8 removed edges, all eight being `base.` bindings to an interface that the interface skip
correctly dropped. The 206 wrong targets fall into two classes, both resolver bugs rather than
disagreements with the compiler. 118 bind a class-typed receiver's member to an interface
declaration: the binding walk visited bases in reverse declaration order and skipped interfaces
only at depth one, so a class's first base (which C# requires to be its base class) was checked
last. 83 bind a call to a same-named instance member whose arity does not admit the call, where
the language binds an extension method: member matching was by name only, and the precise tier's
success then locked the extension tier out. Five more are a chain tail whose property name
collides with a type name. The `base` bucket did not move because the non-public member lists
were empty in the reused cache; re-measured on a wiped state the bucket reads 0.978 with precision
unchanged, leaving seven misses on nested bases. The next wave fixes the two bug classes as
language rules: declaration-order, class-first binding walks that never bind an interface for a
class receiver, and per-overload arity ranges that gate call vouching and fall through to the
extension tier.

## Run 3 — registered predictions (2026-09-03)

Run 3 measures the branch head after the arity and walk-order fixes, the chain-tail and
lambda-parameter typing, on a wiped indexer state. Two of the figures below were already seen once
in the Run 2b attribution (`base` on a wiped state, and the count of wrong targets the two bug
classes account for), so those rows are reproductions rather than predictions and are marked as
such.

| metric | Run 2b | Run 3 predicted |
| --- | --- | --- |
| recall `this` | 0.526 | ≥ 0.80 |
| recall `base` | 0.248 | ≥ 0.95 (reproduction) |
| recall `call` | 0.004 | ≥ 0.20 |
| recall `ident` | 0.624 | ≥ 0.63 |
| recall all | 0.534 | ≥ 0.55 |
| recall precise+ext | 0.452 | ≥ 0.47 |
| precise precision | 0.963 | ≥ 0.968 (reproduction of the attributed 201 removals) |
| ext precision | 0.896 | ≥ 0.85 |
| guess precision | 0.511 | ≥ 0.48 |
| leaked external sites | 1067 | ≤ 1098 |

Falsification. Precise precision below 0.968 means the two bug classes were not the whole story
and the residual wrong targets are bucketed again before anything else lands. A `call` bucket
below 0.20 means one method-return hop is not where the chain misses are. The enrichment decision
rule registered before Run 2 stands unchanged: precise+ext at or above 0.70 closes the question;
below it, the remaining misses are bucketed and the enrichment layer is designed against the
oracle's record contract.

## Run 3 — after the arity and walk-order fixes

Same corpus pin and oracle output as Runs 1, 2 and 2b (denominators identical). The indexer state
was wiped before mapping this round — full reparse, graph rebuilt in 2.13s, 9956 defs, 130649
edges. Devscout is at the branch head adding declaration-order, class-first binding walks,
per-overload method arities gating call vouching, one-hop chain tails, and lambda-parameter
element typing.

### Audit text output (verbatim; `root` rewritten bench-relative, nothing else changed)

```
devscout audit --semantic  root bench/corpora/csharp  oracle 112190 records / 66924 sites  units ok 56 failed 0  method units
tier        edges     tp     fp   precision   fp:no-site  fp:external  fp:wrong  structural
precise     25713  24983    730       0.972           64           55       611          12
ext          2194   1980    214       0.902            5           17       192           0
guess        9652   4792   4860       0.496           50         2514      2296           0
recall (56713 in-graph member sites)  precise 0.431  precise+ext 0.466  all 0.550
  by receiver  ident 0.631  qualified 0.216  this 0.535  base 0.978  call 0.128
external sites 19153  silent-correct 18042  leaked 1111
fan-out  1: 23693  2: 3902  3: 1492  4+: 368
top fp targets   InMemoryDelayProvider 295  CodePrinter 218  Retry 214  RoutingSlipExtensions 161  BusRegistrationContext 158  IBusRegistrationContext 157  IPerformanceCounter 148  NullPerformanceCounter 148  StatsDPerformanceCounter 148  InMemoryContainerTestFixture 107  ClosureInfo 103  Instance 102  ContainerTestHarness 78  Message 78  ConcurrencyLimiter 77  IConcurrencyLimiter 77  TelemetryMonitorExtensions 75  ToCSharpPrinter 73  IIndexedSagaProperty 65  IndexedSagaDictionary 65
top missed       MassTransit.ConsumeContext 1734  MassTransit.SendContext 932  MassTransit.MessageContext 823  MassTransit.BehaviorContext 684  MassTransit.Testing.IBaseTestHarness 634  MassTransit.Testing.ITestHarness 600  MassTransit.PipeContext 538  MassTransit.IPublishEndpoint 491  MassTransit.SagaConsumeContext 482  MassTransit.IRegistrationConfigurator 423  MassTransit.TransitionExtensions 418  MassTransit.IStateMachineModifier 339  MassTransit.ThenExtensions 313  MassTransit.DependencyInjectionTestingExtensions 279  MassTransit.Testing.IReceivedMessageList 278  MassTransit.IReceiveConfigurator 275  MassTransit.Testing.IPublishedMessageList 272  MassTransit.TestStateMachineExtensions 269  MassTransit.ISendEndpoint 222  MassTransit.IProbeSite 179
unknown targets  enum-member 30  class 16  struct 5  delegate 1  interface 1
partial file mismatch 155
ambiguous 137
edges outside universe (not judged) 685
```

### `--json` tier objects (verbatim)

```json
{
  "precise": {
    "edges": 25713,
    "tp": 24983,
    "fp": 730,
    "precision": 0.972,
    "fp_no_site": 64,
    "fp_external_site": 55,
    "fp_wrong_target": 611,
    "structural": 12
  },
  "ext": {
    "edges": 2194,
    "tp": 1980,
    "fp": 214,
    "precision": 0.902,
    "fp_no_site": 5,
    "fp_external_site": 17,
    "fp_wrong_target": 192,
    "structural": 0
  },
  "guess": {
    "edges": 9652,
    "tp": 4792,
    "fp": 4860,
    "precision": 0.496,
    "fp_no_site": 50,
    "fp_external_site": 2514,
    "fp_wrong_target": 2296,
    "structural": 0
  }
}
```

### Recall by receiver kind

Denominator 56713 in-graph member sites (same as Run 1, Run 2 and Run 2b — oracle unchanged).

| Receiver kind | Run 2b | Run 3 |
| --- | --- | --- |
| `ident` | 0.624 | 0.631 |
| `qualified` | 0.216 | 0.216 |
| `this` | 0.526 | 0.535 |
| `base` | 0.248 | 0.978 |
| `call` | 0.004 | 0.128 |
| **all** | **0.534** (precise 0.419, precise+ext 0.452) | **0.550** (precise 0.431, precise+ext 0.466) |

`conditional` (`?.`): 709 records. `bare` (unqualified invocation): 7512 records. Both still
excluded from the headline recall figure.

### Predictions vs. actual

| Prediction | Actual | Verdict |
| --- | --- | --- |
| recall `this` ≥ 0.80 | 0.535 | **FAIL** |
| recall `base` ≥ 0.95 | 0.978 | **HOLD** |
| recall `call` ≥ 0.20 | 0.128 | **FAIL** |
| recall `ident` ≥ 0.63 | 0.631 | **HOLD** |
| recall all ≥ 0.55 | 0.550 | **HOLD** |
| recall precise+ext ≥ 0.47 | 0.466 | **FAIL** |
| precise precision ≥ 0.968 | 0.972 | **HOLD** |
| ext precision ≥ 0.85 | 0.902 | **HOLD** |
| guess precision ≥ 0.48 | 0.496 | **HOLD** |
| leaked external sites ≤ 1098 | 1111 | **FAIL** |

Every guess-tier edge that appeared between Run 2b and Run 3 was traced: all 640 are chain-tail
references whose method-return hop found no receiver type and which then fell into the scored
tier's name-uniqueness pool, 374 of them at sites whose receiver the compiler binds to an external
type. That is a precision regression the chain-tail rule introduced, fixed in Run 4 by finishing
such a reference as external. The remaining `this` misses are 93 of 99 on a generic enclosing
type, where the extension tier compared the receiver's own type-parameter wildcards against a
non-generic `this` parameter reached through the base closure; also fixed in Run 4. Of the missed
`call` sites, 47% are bare unqualified calls (a stated non-goal) and a further third are chains
deeper than one hop. The top missed target overall, a generic context interface, splits into
accesses on untyped lambda parameters (57%) and a generic-arity collision where two declarations
share one arity-stripped id (35%).

## Run 4 — after the chain-tail and unification fixes

Same corpus pin, oracle output, and wiped-state procedure as Run 3 (graph rebuilt in 2.13s, 9956
defs, 129813 edges). Devscout is at the branch head that finishes a chain tail as external when
its hop yields no receiver type, and unifies extension type arguments against the matched base.

### Audit text output (verbatim; `root` rewritten bench-relative, nothing else changed)

```
devscout audit --semantic  root bench/corpora/csharp  oracle 112190 records / 66924 sites  units ok 56 failed 0  method units
tier        edges     tp     fp   precision   fp:no-site  fp:external  fp:wrong  structural
precise     25713  24983    730       0.972           64           55       611          12
ext          2272   2058    214       0.906            5           17       192           0
guess        8746   4479   4267       0.512           46         2078      2143           0
recall (56713 in-graph member sites)  precise 0.431  precise+ext 0.467  all 0.546
  by receiver  ident 0.628  qualified 0.216  this 0.944  base 0.978  call 0.060
external sites 19153  silent-correct 18142  leaked 1011
fan-out  1: 23685  2: 3737  3: 1383  4+: 328
top fp targets   InMemoryDelayProvider 295  Retry 211  CodePrinter 179  RoutingSlipExtensions 161  BusRegistrationContext 158  IBusRegistrationContext 157  IPerformanceCounter 148  NullPerformanceCounter 148  StatsDPerformanceCounter 148  ClosureInfo 103  Instance 102  InMemoryContainerTestFixture 83  ContainerTestHarness 78  Message 78  ToCSharpPrinter 70  TextTableOptions 65  ISendEndpoint 55  ConsumerPipeConfiguratorExtensions 53  SagaPipeConfiguratorExtensions 53  Tools 53
top missed       MassTransit.ConsumeContext 1734  MassTransit.SendContext 932  MassTransit.MessageContext 823  MassTransit.BehaviorContext 684  MassTransit.Testing.IBaseTestHarness 634  MassTransit.Testing.ITestHarness 600  MassTransit.PipeContext 538  MassTransit.IPublishEndpoint 491  MassTransit.SagaConsumeContext 482  MassTransit.TransitionExtensions 449  MassTransit.IRegistrationConfigurator 423  MassTransit.IStateMachineModifier 340  MassTransit.ThenExtensions 315  MassTransit.DependencyInjectionTestingExtensions 294  MassTransit.Testing.IReceivedMessageList 278  MassTransit.IReceiveConfigurator 275  MassTransit.Testing.IPublishedMessageList 272  MassTransit.TestStateMachineExtensions 268  MassTransit.ISendEndpoint 222  MassTransit.IProbeSite 179
unknown targets  enum-member 30  class 16  struct 5  delegate 1  interface 1
partial file mismatch 155
ambiguous 137
edges outside universe (not judged) 677
```

### `--json` tier objects (verbatim)

```json
{
  "precise": {
    "edges": 25713,
    "tp": 24983,
    "fp": 730,
    "precision": 0.972,
    "fp_no_site": 64,
    "fp_external_site": 55,
    "fp_wrong_target": 611,
    "structural": 12
  },
  "ext": {
    "edges": 2272,
    "tp": 2058,
    "fp": 214,
    "precision": 0.906,
    "fp_no_site": 5,
    "fp_external_site": 17,
    "fp_wrong_target": 192,
    "structural": 0
  },
  "guess": {
    "edges": 8746,
    "tp": 4479,
    "fp": 4267,
    "precision": 0.512,
    "fp_no_site": 46,
    "fp_external_site": 2078,
    "fp_wrong_target": 2143,
    "structural": 0
  }
}
```

### Recall by receiver kind

Denominator 56713 in-graph member sites (same as Run 1 through Run 3 — oracle unchanged).

| Receiver kind | Run 3 | Run 4 |
| --- | --- | --- |
| `ident` | 0.631 | 0.628 |
| `qualified` | 0.216 | 0.216 |
| `this` | 0.535 | 0.944 |
| `base` | 0.978 | 0.978 |
| `call` | 0.128 | 0.060 |
| **all** | **0.550** (precise 0.431, precise+ext 0.466) | **0.546** (precise 0.431, precise+ext 0.467) |

`conditional` (`?.`): 709 records. `bare` (unqualified invocation): 7512 records. Both still
excluded from the headline recall figure.

### Predictions vs. actual

Same Run 3 prediction rows, re-judged against Run 4 numbers:

| Prediction | Actual | Verdict |
| --- | --- | --- |
| recall `this` ≥ 0.80 | 0.944 | **HOLD** |
| recall `base` ≥ 0.95 | 0.978 | **HOLD** |
| recall `call` ≥ 0.20 | 0.060 | **FAIL** |
| recall `ident` ≥ 0.63 | 0.628 | **FAIL** |
| recall all ≥ 0.55 | 0.546 | **FAIL** |
| recall precise+ext ≥ 0.47 | 0.467 | **FAIL** |
| precise precision ≥ 0.968 | 0.972 | **HOLD** |
| ext precision ≥ 0.85 | 0.906 | **HOLD** |
| guess precision ≥ 0.48 | 0.512 | **HOLD** |
| leaked external sites ≤ 1098 | 1011 | **HOLD** |

Run 4 trades guess-tier volume for precision: 906 fewer guess edges (593 wrong, 313 right), leaked
external sites down from 1111 to 1011, which is below the 1098 the branch started from, and guess
precision up to 0.512. The `call` bucket falls from 0.128 to 0.060 because its earlier hits were
guesses at the right target, now silent; the all-tier figure moves by the same mechanism. The
`this` bucket reaches 0.944. Three recall predictions miss by three thousandths or less and are
recorded as misses.

## Decision on the enrichment layer (2026-09-03)

The rule registered before Run 2 stands: recall precise+ext at or above 0.70 would have closed the
question; Run 4 measures 0.467. The question therefore stays open and moves to design, not to
code. The remaining misses have been bucketed by receiver kind and target kind above; the largest
single class, accesses on lambda parameters typed only by the callee's delegate parameter, is a
syntactic rule for in-graph callees and is the next extractor step before a compiler-backed layer
is weighed against it. Whatever that weighing decides, it will be measured here, on this corpus
pin and this oracle output, with predictions registered first.

The buckets this decision rests on are the Run 4 recall-by-receiver-kind table and the Run 3
attribution of the missed `call` sites (47% bare unqualified calls, a further third chains deeper
than one hop) and of the top missed target (57% accesses on untyped lambda parameters, 35% a
generic-arity collision); this document holds no separate per-target-kind table. Applying the
registered rule's second branch: the enrichment layer is a design item of its own, tracked outside
this repository, and its input contract is the per-site record `tools/scout-semantic` already
emits — `receiverText`, `receiver`, `target` (`tools/scout-semantic/Records.cs`) — consumed from a
cache and never on the hook path. The lambda-parameter syntactic rule named above is measured
first; the compiler-backed layer is weighed against whatever that run leaves.

### Changes after Run 4, not yet measured (2026-09-05)

A whole-branch review after Run 4 found and fixed five resolver and extractor defects: partial-type
overload arities merged as a union per name; a `base.` walk on a cyclic hierarchy no longer
binding the enclosing type; a cross-file field or property type resolved in its declaring file's
`using` context; catch, query-range and untyped lambda bindings shadowing the cross-file field
fallback; and a `base.`-qualified chain head hopping through the base's method return. Each is
subtractive on wrong edges or additive on a call that previously fell through, and each carries
a unit test; none is reflected in the Run 4 figures above. The next corpus run (wiped state, as
Run 3 and Run 4 were) measures them; its predictions are registered here before it runs.

The 5%-false-positive stop condition registered for the lambda-parameter rule was not checked as
its own `ident`-bucket attribution on Runs 3 or 4; the evidence on record is indirect — precise
precision 0.972 on both runs against the 0.964 floor, and the guess-tier delta between Runs 2b and
3 attributed entirely to chain tails — so the next run carries that attribution explicitly.

### Summary across the branch

| Metric | Run 1 | Run 4 |
| --- | --- | --- |
| precise precision | 0.969 | 0.972 |
| ext precision | 0.809 | 0.906 |
| guess precision | 0.502 | 0.512 |
| leaked external sites | 1098 | 1011 |
| recall precise | 0.379 | 0.431 |
| recall precise+ext | 0.394 | 0.467 |
| recall all | 0.479 | 0.546 |
| recall `this` | 0.000 | 0.944 |
| recall `base` | 0.000 | 0.978 |
| recall `ident` | 0.560 | 0.628 |
| recall `qualified` | 0.217 | 0.216 |
| recall `call` | 0.004 | 0.060 |
