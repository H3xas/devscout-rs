# csharp-target-qualification fixtures

Qualifies the compiler-fact path (`tools/scout-semantic`) across staged .NET targets, project
systems and framework profiles. Its own `global.json` (exact SDK pin, roll-forward disabled),
`Directory.Build.props` (C# 7.3 pinned across the target axis) and `Directory.Packages.props`
(Framework reference-assembly packages) hold this tree independent of the rest of the
repository's own build. Composed and diffed by
[`tools/qualify-dotnet-targets.py`](../../tools/qualify-dotnet-targets.py); results are read by
[`tests/dotnet_target_qualification.rs`](../../tests/dotnet_target_qualification.rs) with no
`dotnet` on the test path, and published in
[`docs/dotnet-target-coverage.md`](../../docs/dotnet-target-coverage.md).

## Shared cases

| File | What it exercises |
| --- | --- |
| `shared/PositiveCase.cs` | The positive case linked into every profile: `IContract`/`Service`/`Caller`, BCL-only, no target-specific API. |
| `shared/BoundaryCase.cs` | The target-API boundary case: `string.Contains(string, StringComparison)` binds under net5.0+/netstandard2.1/netcoreapp3.1 and fails `CS1501` under net40/net472/net48/netstandard2.0 -- a compiler-verified prediction, not a hand-written expectation. |

## Profiles (`profiles/`)

Eleven leaf projects, one compilation identity each. Every profile links the two shared cases;
the three Framework profiles additionally reference the pinned
`Microsoft.NETFramework.ReferenceAssemblies` companion for their own TFM.

| Profile directory | TFM | Track |
| --- | --- | --- |
| `net5.0-sdkstyle` .. `net9.0-sdkstyle` | net5.0 .. net9.0 | modern |
| `netstandard2.0-sdkstyle`, `netstandard2.1-sdkstyle` | netstandard2.0, netstandard2.1 | modern |
| `netcoreapp3.1-sdkstyle` | netcoreapp3.1 | modern |
| `net40-sdkstyle`, `net472-sdkstyle`, `net48-sdkstyle` | net40, net472, net48 | framework-f1 |

## Deep-case bundles (`deep/`)

Two representative profiles (`net8.0-deep`, `net472-deep`) carry the case families beyond the
per-profile positive/boundary pair. The other nine wave-1 profiles are not independently
executed for these families -- recorded in the published document, never hidden.

| File | Case family | What it exercises |
| --- | --- | --- |
| `net8.0-deep/Collision.cs`, `net472-deep/Collision.cs` | unrelated same-name collision | `WidgetGateway.Send` and the unrelated `MailQueue.Send` must each resolve to their own receiver's declared member. |
| `net8.0-deep/Wrapper.cs`, `net472-deep/Wrapper.cs` | configured wrapper | `LoggingContractWrapper` implements the shared `IContract` by delegating to an inner instance, the shape a DI registration commonly wraps. |
| `IncompatibleRef/`, `net8.0-deep/Deep.csproj` | incompatible reference (modern referencing framework) | A net472-only library referenced from net8.0 restores under NuGet's asset-compatibility fallback (`NU1702`), a healthy-looking restore that is not a compatible reference. |
| `IncompatibleRefModern/`, `net472-deep/IncompatibleRefProbe/` | incompatible reference (framework referencing modern) | A net8.0-only library referenced from net472 cannot restore at all (`NU1201`); isolated in its own probe project so the failure does not take the rest of the bundle's build down with it. |
| `deep/Generated/`, `net8.0-deep/Generated.cs`, `net472-deep/Generated.cs` | generated and linked input | A minimal incremental generator (referenced as an analyzer) emits `GeneratedMarker`, which exists only as compile-time-generated input and is consumed by a hand-written caller. |
| `net8.0-deep/UnknownFramework.cs`, `net472-deep/UnknownFramework.cs` | unknown framework | `WidgetController.OnActionExecuting` reads as a framework lifecycle hook by name alone; no base type, interface or attribute ties it to one, so it must stay `candidate`, never a claimed binding. |

## Substitution-defect controls (`controls/`)

Registered prior art for the sidecar's documented silent-substitution behavior. Both rows are
deliberately `failing`: a control that cannot be made to fail honestly under today's engine
would be a defect in the control, not a row to soften.

| Directory | What it reproduces |
| --- | --- |
| `tfm-not-supplied/` | `Control.csproj` multi-targets `net8.0;net472`; invoked with `--tfm net6.0`, a value neither variant declares, the loader keeps the first variant (`net8.0`) instead of failing or reporting no result. |
| `reference-tfm-mismatch/` | `P` (net8.0) references `Q` (net9.0) -- a declared-TFM mismatch between a project and its reference, the same defect class the pinned MassTransit corpus surfaced at a much larger scale. |
