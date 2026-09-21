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
execution assumptions. Every wave-1 row's execution assumption is `static-only`; this ticket
produces no runtime rows.

## Wave 1 -- measured

Profile id shape: `<language-version>-<tfm>-<project-system>`. Every row here compiled from its
own exact TFM and reference context; no result substitutes another target's evidence.

| Profile | TFM | Track | State | Context acquisition | Semantic conformance |
| --- | --- | --- | --- | --- | --- |
| `csharp73-net5.0-sdkstyle` | net5.0 | modern | passing | SDK 9.0.305, sdk-implicit references | positive case bound; boundary case binds (expected) |
| `csharp73-net6.0-sdkstyle` | net6.0 | modern | passing | SDK 9.0.305, sdk-implicit references | positive case bound; boundary case binds (expected) |
| `csharp73-net7.0-sdkstyle` | net7.0 | modern | passing | SDK 9.0.305, sdk-implicit references | positive case bound; boundary case binds (expected) |
| `csharp73-net8.0-sdkstyle` | net8.0 | modern | passing | SDK 9.0.305, sdk-implicit references | positive case bound; boundary case binds (expected) |
| `csharp73-net9.0-sdkstyle` | net9.0 | modern | passing | SDK 9.0.305, sdk-implicit references | positive case bound; boundary case binds (expected) |
| `csharp73-netstandard2.0-sdkstyle` | netstandard2.0 | modern | passing | SDK 9.0.305, sdk-implicit references | positive case bound; boundary case does not bind (expected, `CS1501`) |
| `csharp73-netstandard2.1-sdkstyle` | netstandard2.1 | modern | passing | SDK 9.0.305, sdk-implicit references | positive case bound; boundary case binds (expected) |
| `csharp73-netcoreapp3.1-sdkstyle` | netcoreapp3.1 | modern | passing | SDK 9.0.305, sdk-implicit references | positive case bound; boundary case binds (expected) |
| `csharp73-net40-sdkstyle` | net40 | framework-f1 | passing | SDK 9.0.305, `Microsoft.NETFramework.ReferenceAssemblies[.net40]@1.0.3` | positive case bound; boundary case does not bind (expected, `CS1501`) |
| `csharp73-net472-sdkstyle` | net472 | framework-f1 | passing | SDK 9.0.305, `Microsoft.NETFramework.ReferenceAssemblies[.net472]@1.0.3` | positive case bound; boundary case does not bind (expected, `CS1501`) |
| `csharp73-net48-sdkstyle` | net48 | framework-f1 | passing | SDK 9.0.305, `Microsoft.NETFramework.ReferenceAssemblies[.net48]@1.0.3` | positive case bound; boundary case does not bind (expected, `CS1501`) |

Every row's `framework_modeling` is `not-claimed`: the shared cross-target case source
(`IContract`/`Service`/`Caller`, BCL-only) exercises no application-framework adapter. A
`net472` row's evidence never cites `netstandard2.1` output, and no Framework row is promoted
from `context_acquisition` alone -- `semantic_conformance` and `framework_modeling` content is
required too.

**The three `framework-f1` rows above (`net40`, `net472`, `net48`) publish as a
target-API-surface result only.** They qualify reference-assembly target-API semantics from an
SDK-style project on this repository's existing non-Windows runner; they are not a claim of
full .NET Framework support. Their Track F2 obligations -- legacy non-SDK `.csproj`,
`packages.config`, Windows build tasks, classic ASP.NET, WPF/WinForms, and the 4.0 Client
Profile -- are unmet and remain `unavailable` in the Wave 2 table below, pending a qualified
Windows worker. Only the F1/F2 pair together is full Framework support.

## Wave 1 -- deep-case bundles

Two representative profiles carry the case families named beyond the per-profile
positive/boundary pair: unrelated same-name collision, configured wrapper, incompatible
reference, generated and linked input, and unknown framework. The other nine wave-1 profiles are
**not independently executed** for these five families -- recorded here, never hidden -- because
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
loader is out of this tree's scope; this tree owns catching and honestly recording the current,
defective behavior, so both rows below are `failing` by design -- a control that cannot be made
to fail honestly under today's engine would be a defect in the control, not a row to soften.

| Control | State | What it reproduces |
| --- | --- | --- |
| `control-tfm-not-supplied` | failing | A `--tfm` value neither declared variant offers; the loader keeps the first variant instead of failing or reporting no result. |
| `control-reference-tfm-mismatch` | failing | A project referencing another project declared at a different TFM; a unit in this shape must never report a healthy result from that reference alone. |

## Wave 2 -- planned / unavailable

Retained ownership of this ticket; not executed by this wave. Each row waits on a named unlock.

| Target | State | Named unlock |
| --- | --- | --- |
| net10.0 | planned | a second pinned SDK band able to target net10.0 |
| netstandard1.0 -- netstandard1.6 | planned | the `NETStandard.Library` 1.6.x package graph pinned and cached |
| net403, net45, net451, net452, net46, net461, net462, net47, net471, net481 | planned | reference-assembly companion package verification per TFM, same shape as the F1 three |
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

- **Eight of the eleven Wave 1 `passing` profiles have no held-out row.** The held-out run below
  currently exercises `net8.0`, `net6.0` and `netstandard2.0` only, from the one pinned held-out
  family available today. `net5.0`, `net7.0`, `net9.0`, `netstandard2.1`, `netcoreapp3.1`,
  `net40`, `net472` and `net48` are published `passing` on their own recorded fixture evidence,
  without an independent held-out row. Closes when a held-out run (this family or a second one)
  covers the remaining strata, or when this asymmetry is otherwise resolved.
- **The `framework-f1` and `netcoreapp3.1` held-out strata are registered but not exercised.**
  See the held-out report below for the exact reason per stratum.

Every row above otherwise reads `passing` on its own recorded evidence, or is a deliberately
`failing` substitution-defect control, or is a visible wave-2/excluded/unqualified row with its
own named unlock or reason.
