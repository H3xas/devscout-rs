# .NET target coverage

Every staged .NET target, project system and framework-semantics profile, with the exact
compilation context that produced its row. Pinned by
[`fixtures/csharp-target-qualification/`](../fixtures/csharp-target-qualification/) and
[`tests/dotnet_target_qualification.rs`](../tests/dotnet_target_qualification.rs), which fails
when a row's committed snapshot and this document drift apart, when an inventory row goes
missing, or when a support sentence anywhere in the repository names a target whose row is not
`passing`.

**This document qualifies the compiler-fact path, not the default binary.** The Rust indexer has
no notion of a target framework: the project model hand-scans `.csproj` with no MSBuild
evaluation and no NuGet resolution, and conditional compilation is evaluated with no symbol
predefined. A row below is a statement about the `tools/scout-semantic` compiler-fact path for
that target -- built from the pinned `fixtures/csharp-target-qualification/` tree, its own SDK
pin and its own committed snapshots -- and does not widen the language-level support claim
elsewhere in this repository.

## Legend

| State | Meaning |
| --- | --- |
| `passing` | Every obligation field is recorded and the row's evidence matched its expectation. |
| `failing` | At least one obligation field's evidence did not match its expectation. |
| `smoke-tested` | Partially executed; not yet a full pass. |
| `planned` | Not yet executed; named unlock has not landed. |
| `unavailable` | Not yet executed; blocked on an unauthorized worker, package graph or SDK band. |
| `unqualified` | In scope but not yet run against the qualification gates. |
| `excluded` | Out of scope by operator instruction. |

A row may read `passing` only with all five obligation fields recorded: context acquisition,
semantic conformance, framework modeling, unsupported state (this table's `State` column) and
execution assumptions. Every measured row's execution assumption is `static-only`; no row here
is a runtime observation.

## Measured

Profile id shape: `<language-version>-<tfm>-<project-system>`. Every row here compiled from its
own exact TFM and reference context; no result substitutes another target's evidence. A target
joins this table only by executing, and leaves the Wave 2 table when it does.

| Profile | TFM | Track | State | Context acquisition | Semantic conformance |
| --- | --- | --- | --- | --- | --- |
| `csharp73-net5.0-sdkstyle` | net5.0 | modern | passing | SDK 9.0.305, sdk-implicit references | positive case bound; boundary case binds (expected) |
| `csharp73-net6.0-sdkstyle` | net6.0 | modern | passing | SDK 9.0.305, sdk-implicit references | positive case bound; boundary case binds (expected) |
| `csharp73-net7.0-sdkstyle` | net7.0 | modern | passing | SDK 9.0.305, sdk-implicit references | positive case bound; boundary case binds (expected) |
| `csharp73-net8.0-sdkstyle` | net8.0 | modern | passing | SDK 9.0.305, sdk-implicit references | positive case bound; boundary case binds (expected) |
| `csharp73-net9.0-sdkstyle` | net9.0 | modern | passing | SDK 9.0.305, sdk-implicit references | positive case bound; boundary case binds (expected) |
| `csharp73-netstandard1.0-sdkstyle` | netstandard1.0 | modern | passing | SDK 9.0.305, `NETStandard.Library`@1.6.1, 25 packages resolved | positive case bound; boundary case does not bind (expected, `CS1501`) |
| `csharp73-netstandard1.1-sdkstyle` | netstandard1.1 | modern | passing | SDK 9.0.305, `NETStandard.Library`@1.6.1, 33 packages resolved | positive case bound; boundary case does not bind (expected, `CS1501`) |
| `csharp73-netstandard1.2-sdkstyle` | netstandard1.2 | modern | passing | SDK 9.0.305, `NETStandard.Library`@1.6.1, 34 packages resolved | positive case bound; boundary case does not bind (expected, `CS1501`) |
| `csharp73-netstandard1.3-sdkstyle` | netstandard1.3 | modern | passing | SDK 9.0.305, `NETStandard.Library`@1.6.1, 62 packages resolved | positive case bound; boundary case does not bind (expected, `CS1501`) |
| `csharp73-netstandard1.4-sdkstyle` | netstandard1.4 | modern | passing | SDK 9.0.305, `NETStandard.Library`@1.6.1, 62 packages resolved | positive case bound; boundary case does not bind (expected, `CS1501`) |
| `csharp73-netstandard1.5-sdkstyle` | netstandard1.5 | modern | passing | SDK 9.0.305, `NETStandard.Library`@1.6.1, 62 packages resolved | positive case bound; boundary case does not bind (expected, `CS1501`) |
| `csharp73-netstandard1.6-sdkstyle` | netstandard1.6 | modern | passing | SDK 9.0.305, `NETStandard.Library`@1.6.1, 73 packages resolved | positive case bound; boundary case does not bind (expected, `CS1501`) |
| `csharp73-netstandard2.0-sdkstyle` | netstandard2.0 | modern | passing | SDK 9.0.305, sdk-implicit references | positive case bound; boundary case does not bind (expected, `CS1501`) |
| `csharp73-netstandard2.1-sdkstyle` | netstandard2.1 | modern | passing | SDK 9.0.305, sdk-implicit references | positive case bound; boundary case binds (expected) |
| `csharp73-netcoreapp3.1-sdkstyle` | netcoreapp3.1 | modern | passing | SDK 9.0.305, sdk-implicit references | positive case bound; boundary case binds (expected) |
| `csharp73-net40-sdkstyle` | net40 | framework-f1 | passing | SDK 9.0.305, `Microsoft.NETFramework.ReferenceAssemblies[.net40]@1.0.3` | positive case bound; boundary case does not bind (expected, `CS1501`) |
| `csharp73-net45-sdkstyle` | net45 | framework-f1 | passing | SDK 9.0.305, `Microsoft.NETFramework.ReferenceAssemblies[.net45]@1.0.3` | positive case bound; boundary case does not bind (expected, `CS1501`) |
| `csharp73-net452-sdkstyle` | net452 | framework-f1 | passing | SDK 9.0.305, `Microsoft.NETFramework.ReferenceAssemblies[.net452]@1.0.3` | positive case bound; boundary case does not bind (expected, `CS1501`) |
| `csharp73-net461-sdkstyle` | net461 | framework-f1 | passing | SDK 9.0.305, `Microsoft.NETFramework.ReferenceAssemblies[.net461]@1.0.3` | positive case bound; boundary case does not bind (expected, `CS1501`) |
| `csharp73-net472-sdkstyle` | net472 | framework-f1 | passing | SDK 9.0.305, `Microsoft.NETFramework.ReferenceAssemblies[.net472]@1.0.3` | positive case bound; boundary case does not bind (expected, `CS1501`) |
| `csharp73-net48-sdkstyle` | net48 | framework-f1 | passing | SDK 9.0.305, `Microsoft.NETFramework.ReferenceAssemblies[.net48]@1.0.3` | positive case bound; boundary case does not bind (expected, `CS1501`) |

Below .NET Standard 2.0 the SDK references the `NETStandard.Library` package rather than a
targeting pack, so those seven rows record the package version and the number of packages the
restore resolved; a different resolved graph changes the row.

Every row's `framework_modeling` is `not-claimed`: the shared cross-target case source
(`IContract`/`Service`/`Caller`, BCL-only) exercises no application-framework adapter. A
`net472` row's evidence never cites `netstandard2.1` output, and no Framework row is promoted
from `context_acquisition` alone -- `semantic_conformance` and `framework_modeling` content is
required too.

**The six `framework-f1` rows above (`net40`, `net45`, `net452`, `net461`, `net472`, `net48`)
publish as a target-API-surface result only.** They qualify reference-assembly target-API semantics from an
SDK-style project on this repository's existing non-Windows runner; they are not a claim of
full .NET Framework support. Their Track F2 obligations -- legacy non-SDK `.csproj`,
`packages.config`, Windows build tasks, classic ASP.NET, WPF/WinForms, and the 4.0 Client
Profile -- are unmet and remain `unavailable` in the Wave 2 table below, pending a qualified
Windows worker. Only the F1/F2 pair together is full Framework support.

## Measured -- deep-case bundles

Two representative profiles carry the case families named beyond the per-profile
positive/boundary pair: unrelated same-name collision, configured wrapper, incompatible
reference, generated and linked input, and unknown framework. The other nineteen measured
profiles are **not independently executed** for these five families -- recorded here, never hidden -- because
they exercise project-loading and tooling machinery that varies by project system and worker,
not by individual TFM.

| Bundle | TFM | Track | State | Case families |
| --- | --- | --- | --- | --- |
| `csharp73-net8.0-deep` | net8.0 | modern | passing | collision, wrapper, incompatible reference (NU1702, modern referencing framework), generated input, unknown framework (stays `candidate`) |
| `csharp73-net472-deep` | net472 | framework-f1 | passing | collision, wrapper, incompatible reference (NU1201, framework referencing modern), generated input, unknown framework (stays `candidate`) |

The unknown-framework case in both bundles stays `candidate`: a type's name and method-name
shape alone (`WidgetController.OnActionExecuting`) is never enough to claim a framework-adapter
binding without declaring assembly, type, member identity and signature.

## Substitution-defect controls (prior art)

Registered controls for the sidecar's documented silent-substitution behavior. Repairing the
loader is out of this tree's scope; this tree owns independently detecting substitution -- from
the loader's own stderr and the unit it kept, never from its status line -- and honestly
recording whichever behavior that detection actually observes, defective or fixed. A row here
reads `passing` only when the observed evidence positively shows the no-substitution outcome,
never merely from the absence of a substitution match; a control that cannot be made to fail
honestly under today's engine would be a defect in the control, not a row to soften.

| Control | State | What it reproduces |
| --- | --- | --- |
| `control-tfm-not-supplied` | passing | A `--tfm` value neither declared variant offers. The loader now explicitly refuses the request (declared-TFM mismatch diagnosed, zero units reported) instead of silently keeping the first variant -- the defect this control monitors was fixed upstream, outside this tree's own scope; the control still independently fails the run if silent substitution ever returns. |
| `control-reference-tfm-mismatch` | failing | A project referencing another project declared at a different TFM; a unit in this shape must never report a healthy result from that reference alone. |

## Wave 2 -- planned / unavailable

Not executed yet. Each row waits on a named unlock and stays here until it runs.

| Target | State | Named unlock |
| --- | --- | --- |
| net10.0 | planned | a second pinned SDK band able to target net10.0 |
| net403, net451, net46, net462, net47, net471, net481 | planned | reference-assembly companion package verification per TFM, same shape as the measured F1 rows |
| Framework 4.0 Client Profile | planned | its own profile: a smaller API surface than plain net40 |
| Legacy non-SDK `.csproj`, `packages.config`, Windows build tasks, classic ASP.NET Web Application/Web Site | unavailable | a qualified Windows worker with legacy tooling (Track F2); no verification job in this repository runs on Windows today |
| WPF, WinForms | unavailable | Track F2's qualified Windows worker |
| UWP, Xamarin/MAUI, Unity, native interop | unavailable | platform/workload profiles not yet scoped |

## Excluded / unqualified

| Target | State | Reason |
| --- | --- | --- |
| .NET Core 1.x | excluded | operator instruction |
| .NET Core 2.x | excluded | operator instruction |
| netcoreapp3.0 | unqualified | in scope, not yet run against the qualification gates |
| .NET Framework before 4.0 | excluded | operator instruction |
| `project.json` / `.xproj` projects | excluded | `dotnet restore` on the pinned SDK band ignores `project.json` projects outright; a clean restore cannot establish that the expected project inventory was processed |

## Held-out family

The held-out report is a dated, manually run benchmark, not part of `cargo test` or CI:
see [`bench/dotnet-target-qualification.sh`](../bench/dotnet-target-qualification.sh) and the
pre-registered thresholds in
[`fixtures/csharp-target-qualification/expected-held-out.json`](../fixtures/csharp-target-qualification/expected-held-out.json).
Results land under `docs/benchmarks/results/` once the run has executed at least once.

## Follow-ups

- **Eighteen of the twenty-one measured `passing` profiles have no held-out row.** The held-out
  run below currently exercises `net8.0`, `net6.0` and `netstandard2.0` only, from the one pinned
  held-out family available today. `net5.0`, `net7.0`, `net9.0`, `netstandard1.0` through
  `netstandard1.6`, `netstandard2.1`, `netcoreapp3.1`, `net40`, `net45`, `net452`, `net461`,
  `net472` and `net48` are published `passing` on their own recorded fixture evidence, without an
  independent held-out row. Closes when a held-out run (this family or a second one)
  covers the remaining strata, or when this asymmetry is otherwise resolved.
- **The `framework-f1`, `netstandard1` and `netcoreapp3.1` held-out strata are registered but not
  exercised.** See the held-out report below for the exact reason per stratum; the pinned family
  declares none of the `netstandard1` targets, and that stratum was registered after the last
  held-out run.
- **Three measured rows miss the registered `unsupported_coverage` floor.** The 2026-09-21
  held-out run computes `unsupported_coverage` (the 2026-09-17 run never computed this metric)
  and reports `MISS` against the registered `min: 1.0` for `csharp73-net6.0-sdkstyle` (net6.0,
  modern, measured 0.881), `csharp73-net8.0-sdkstyle` (net8.0, modern, measured 0.881) and
  `csharp73-netstandard2.0-sdkstyle` (netstandard2.0, netstandard, measured 0.860). Precision and
  recall meet their registered minimums for all three; only `unsupported_coverage` misses. No
  Design stop condition addresses this metric, so the three rows stay published `passing` on
  their own recorded fixture evidence -- the miss is disclosed here, not a reason to withhold
  them. Closes when a fix raises the measured coverage to the registered floor, or the floor is
  revised.

Every row above otherwise reads `passing` on its own recorded fixture evidence, except where this
section names a registered held-out obligation the measured value disagrees with; a deliberately
`failing` substitution-defect control and a visible planned/excluded/unqualified row with its own
named unlock or reason are unaffected by this section.
