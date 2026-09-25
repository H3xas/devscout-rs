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
| `shared/BoundaryCase.cs` | The target-API boundary case: `string.Contains(string, StringComparison)` binds under net5.0+/netstandard2.1/netcoreapp3.1 and fails `CS1501` under every Framework profile here and every .NET Standard profile before 2.1 -- a compiler-verified prediction, not a hand-written expectation. |

## Profiles (`profiles/`)

Twenty-two leaf projects, one compilation identity each. Every profile links the two shared
cases; the six Framework profiles additionally reference the pinned
`Microsoft.NETFramework.ReferenceAssemblies` companion for their own TFM. The seven
`netstandard1.x` profiles declare no package: the SDK's implicit `NETStandard.Library` reference is
the reference source under test, and its resolved graph is recorded in the row. Restoring that
graph may print NuGet audit warnings for the old `System.*` packages it pulls in; they do not
change a row's exit codes, and this tree is never packed, published or run. `net10.0-sdkstyle` carries its own `global.json`
pinning SDK 10.0.302 with roll-forward disabled, because the tree's 9.0.305 pin cannot target
`net10.0`; the composition script runs that row from its own directory, under a dotnet root that
lists that exact band, and records the band the oracle registered.

| Profile directory | TFM | Track |
| --- | --- | --- |
| `net5.0-sdkstyle` .. `net9.0-sdkstyle` | net5.0 .. net9.0 | modern |
| `net10.0-sdkstyle` (own SDK pin, 10.0.302) | net10.0 | modern |
| `netstandard1.0-sdkstyle` .. `netstandard1.6-sdkstyle` | netstandard1.0 .. netstandard1.6 | modern |
| `netstandard2.0-sdkstyle`, `netstandard2.1-sdkstyle` | netstandard2.0, netstandard2.1 | modern |
| `netcoreapp3.1-sdkstyle` | netcoreapp3.1 | modern |
| `net40-sdkstyle`, `net45-sdkstyle`, `net452-sdkstyle`, `net461-sdkstyle`, `net472-sdkstyle`, `net48-sdkstyle` | net40, net45, net452, net461, net472, net48 | framework-f1 |

## Deep-case bundles (`deep/`)

Two representative profiles (`net8.0-deep`, `net472-deep`) carry the case families beyond the
per-profile positive/boundary pair. The other twenty profiles are not independently
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

Registered prior art for the sidecar's documented silent-substitution behavior. Neither row
trusts the loader's own status line: each independently detects substitution from the loader's
own stderr and the unit it kept, so a regression back to silent substitution still trips its
control. `tfm-not-supplied/` now reads `passing`, not because the defect it monitors was
softened, but because the loader's own fix landed upstream, outside this tree's own scope:
an undeclared `--tfm` is refused explicitly instead of silently substituted, which is the
explicit failed/unsupported row this control's row exists to require. `reference-tfm-mismatch/`
remains `failing`: a control that cannot be made to fail honestly under today's engine would be
a defect in the control, not a row to soften.

| Directory | What it reproduces |
| --- | --- |
| `tfm-not-supplied/` | `Control.csproj` multi-targets `net8.0;net472`; invoked with `--tfm net6.0`, a value neither variant declares. The loader now explicitly refuses the request ("requested tfm 'net6.0' is not declared", exit 3, zero units) instead of silently keeping the first variant; this control independently confirms the refusal from the loader's own exit code and diagnostic, and still fails the run if silent substitution ever returns. |
| `reference-tfm-mismatch/` | `P` (net8.0) references `Q` (net9.0) -- a declared-TFM mismatch between a project and its reference, the same defect class the pinned MassTransit corpus surfaced at a much larger scale. |
