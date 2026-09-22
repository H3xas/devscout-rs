# csharp-context-fingerprint — fingerprint mutation-class evidence

A second, dedicated fixture tree for the build-context fingerprint's own evidence: a before/after
envelope pair per mutation class, each holding the consuming source file byte-identical while
exactly one other input moves. Separate from `fixtures/csharp-context/` so this fixture's own
project layout (several independent, sibling probe projects) never touches that fixture's five
CI-diffed snapshots.

None of the JSON under `pairs/` is regenerated or diffed by CI: every pair is generated once,
locally, and committed as-is (the same "locally-generated, non-CI-regenerated" precedent this
repository already used for cross-SDK evidence). `tests/context_fingerprint_pairs.rs` checks every
pair offline, with no .NET toolchain, the same way `tests/context_envelope.rs` checks the sibling
fixture.

## Projects

| Project | Role |
|---|---|
| `Probe.csproj` / `Probe.cs` | The main probe: one authored source file, a project reference to `Shared/`, one `AdditionalFiles` item (`Gen.txt`). `Probe.cs` is never edited across any pair. |
| `Shared/Shared.csproj` / `Shared/Shared.cs` | Probe's project reference. Carries a build-only knob (`SharedExtraSymbol`) used by the dependency-fingerprint pair; never edited itself. |
| `Gen.txt` | Probe's one generator input (an `AdditionalFiles` item); its content is the class 6a pair's own mutation. |
| `sdk-pair/SdkPair.csproj` / `SdkPair.cs` | A minimal, single-target (`net8.0`) project used only for the SDK-version pair, so the same target builds cleanly under both installed SDKs. |
| `reference-content/RefProbe.csproj` / `RefProbe.cs` | A minimal project referencing a committed external DLL by `HintPath`, switchable via `-p:ExtLibPath`, for the metadata-reference-content pair. `ExtLib.v1.dll` / `ExtLib.v2.dll` are two builds of the same tiny library differing only in one constant's value. |

`Probe.csproj` disables default compile-item globbing and lists `Probe.cs` explicitly, because
several independent sibling projects share this directory tree and default SDK globbing would
otherwise sweep their source into Probe's own compilation too.

## Pairs (`pairs/`)

Every pair's "before" side is `base.json` (`Probe.csproj`, default Debug, no overrides) unless
noted. Each row names the mutation, the exact commands (run from the repository root, oracle
already built via `dotnet build tools/scout-semantic -c Release`), and what the pair proves.

1. **Metadata reference content** (`class1-metadata-reference-content-{before,after}.json`) — a
   metadata reference's own content changes, not a project reference:
   ```sh
   dotnet run --project tools/scout-semantic --no-build -c Release -- \
     fixtures/csharp-context-fingerprint/reference-content/RefProbe.csproj \
     --root fixtures/csharp-context-fingerprint/reference-content \
     -p:ExtLibPath="$PWD/fixtures/csharp-context-fingerprint/reference-content/ExtLib.v1.dll" \
     --emit context --context out/fp-class1-before.json
   # then ExtLib.v2.dll -> out/fp-class1-after.json
   ```
   `RefProbe.cs` never changes; only which DLL `ExtLib` resolves to (a real MVID/content-hash
   difference, `ContextFingerprint`'s existing metadata-reference identity) differs.
2. **Build configuration** (`class2-build-configuration-after.json`) — `-p:Configuration=Release`
   against `Probe.csproj`. `configuration` is part of compilation identity by design, so this pair's
   identity legitimately differs too (not just the fingerprint); `documents` and the source stay
   the same.
3. **Preprocessor/build symbol** (`class3-preprocessor-symbol-after.json`) —
   `-p:DefineConstants=PROBE_FLAG`.
4. **Language/compiler option** (`class4-language-option-after.json`) — `-p:LangVersion=12.0`
   (base is the SDK default, `13.0`).
5. **SDK/MSBuild/compiler version** (`class5-sdk-version-{before,after}.json`) — the same
   `sdk-pair/SdkPair.csproj` (`net8.0`, buildable under either installed SDK) run once under the
   default SDK (9.0.305) and once with a `global.json` in the working directory pinning
   `8.0.121` (`MSBuildLocator` honours it: the run's own stderr line names the different MSBuild
   path). `versions.sdk`/`versions.msbuild` differ genuinely, not just as a string swap.
6. **Generator or analyzer input**:
   - **6a, generator input** (`class6a-generator-input-after.json`) — `Gen.txt`'s content changed
     to `generator-input-version=2` for the run, then reverted; the committed tree's `Gen.txt` is
     the "before" content used by `base.json`.
   - **6b, analyzer reference** (`class6b-analyzer-reference-after.json`) —
     `-p:EnableNETAnalyzers=false`, which removes the SDK's default analyzer package reference
     from the project's `AnalyzerReferences`.
7. **Dependency compilation's own fingerprint** (`class7-dependency-fingerprint-after.json`) —
   `-p:SharedExtraSymbol=SHARED_MUTATED`, read only by `Shared.csproj`'s own conditional
   `DefineConstants`. Probe's own direct symbols, imports and references are unchanged; only
   `Shared`'s own fingerprint moves, and that alone moves Probe's fingerprint through the
   project-reference fold (`references[].fingerprint` for the `Shared` entry moves too).

`tools/scout-semantic.Tests/ContextFingerprintTests.cs` additionally proves each of
`ContextFingerprint.Compute`'s seven input groups moves the digest in isolation, as a pure-function
unit test independent of any of the above fixture runs.

## Fingerprint cost

Every `--emit context` run logs `context: fingerprint computation totaled <N>ms across <M>
compilations` to stderr (`ContextBuilder.FingerprintElapsed`, wall time spent inside
`GetOrComputeFingerprint`, cache hits excluded). Measured on this machine across the runs that
produced the pairs above: consistently 40-180ms for one to two compilations, the bulk of it the
fingerprint path's own second, independent project evaluation. Recorded here, not asserted by a
test, because the number is machine-dependent.
