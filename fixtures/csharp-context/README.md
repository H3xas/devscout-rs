# csharp-context — build-context envelope fixture

A package-light, mostly-offline solution (`Fixture.sln`) that exercises the compiler oracle's
`--emit context` mode: three projects, each engineered to hit a different context state, plus a
document dropped for every reason `RepoPaths.Classify` names.

| Project | Declared targets | What it proves |
|---|---|---|
| `Clean.csproj` | `net9.0` | One authored document, one source-generated document (`System.Text.Json` serializer context, ships in the shared framework, no new package), one `Compile` item pointing at a file that is never created, one pointing outside this fixture's root, one under `bin/` (a devscout skip directory). |
| `Legacy.csproj` | `net472;net9.0` | Multi-targeting. Under `net472` it fails to bind `string.Contains(char, StringComparison)` (a .NET-only overload), the fixture's control for a real, non-fabricated `partial`/`binding-error` compilation. Under `net9.0` it binds cleanly. |
| `Broken.csproj` | `net9.0` | A `ProjectReference` to a `.csproj` that does not exist on disk. `MSBuildWorkspace`'s `SkipUnrecognizedProjects` still produces a non-null compilation for this project (verified empirically against the restored 4.14.0 assemblies), so this is a `partial`/`workspace-failure` control, not a `failed` one -- `failed` needs a project the solution names that never reaches the workspace at all, which `Vanished` (declared in `Fixture.sln` with no project file on disk) is the control for. |

`../csharp-context-outside/Linked.cs` lives one level above this fixture's root on purpose: it is
the fixture case for the `linked-outside-root` drop reason, and must stay outside
`fixtures/csharp-context/` for that reason to fire.

## Committed envelopes

Five snapshots, each a full `--emit context` run. CI regenerates and byte-diffs one of them
(`context.json`, the default-selection run) against the committed file, the same way
`fixtures/csharp-flowtrace/facts.json` is; the other four are checked offline from committed bytes,
by `tests/context_envelope.rs`, the same as `fixtures/csharp-context-fingerprint/`'s own files:

| File | Command (besides the shared `--root fixtures/csharp-context Fixture.sln --emit context`) | What it shows |
|---|---|---|
| `context.json` | *(no `--tfm`)* | Deterministic ordinal-least selection (`net472` over `net9.0` for `Legacy`, both spelled with a `net` prefix but `4` sorts before `9`); every drop reason but `out-of-scope`; `excluded`/`not-requested` for the unselected `Legacy` variant. |
| `context-tfm-net9.0.json` | `--tfm net9.0` | Every project has a declared `net9.0` variant, so nothing is `excluded`; `Legacy` is `complete` here (the net9.0 overload exists), coexisting in the same envelope with `Clean` and `Broken`, both still `partial` for their own reasons -- the fixture's proof that a partial sibling does not poison a complete one. |
| `context-tfm-net9.0-release.json` | `--tfm net9.0 -p:Configuration=Release` | Same identities as the file above but `configuration: "Release"`; together the pair proves two identities of one project that differ only by configuration stay distinct, with distinct fingerprints, and round-trip separately. |
| `context-tfm-net48.json` | `--tfm net48` | `net48` is declared by no project here: every record is `unsupported`/`undeclared-target`, naming both the requested and the declared targets, with zero facts under any of them. |
| `context-scoped.json` | `--scope src/Legacy` | Documents under `src/Clean` and `src/Broken` are `out-of-scope`, the one drop reason the other four files do not exercise; project identities themselves are never scope-filtered. |

Every file above was also generated twice and diffed byte-identical before being committed, and
regenerating from a different absolute checkout path reproduces the same bytes: no local path, no
username, and no restore-generated `obj/*.nuget.g.props`/`.targets` content is folded into the
envelope. `versions.sdk`/`versions.msbuild` are stamped from the SDK that generated the fixture, and
reference identities (assembly MVIDs) are hashed from that SDK's install, so the install layout
matters as much as the patch number. `context.json` and
`fixtures/csharp-compiler-facts/compiler-facts.json`, the two snapshots CI regenerates, come from a
9.0.305 install laid out the way CI's `dotnet-install` lays it out, selected through the same
`global.json` pin CI writes: a macOS `.pkg` install of the same patch ships reference assemblies
with different MVIDs. The other four files are only checked offline and keep the bytes they were
generated with.

## Known gaps

- **`net472` evaluation.** `Legacy`'s `net472` variant compiles through Roslyn's own
  out-of-process BuildHost (using `Microsoft.NETFramework.ReferenceAssemblies`), and this
  fixture's independent, in-process `ContextInventory` evaluation of that same variant succeeds on
  non-Windows machines too, so its document inventory is available: `documents.inventoryAvailable`
  reads `true` and `configuration`/`platform` read `Debug`/`AnyCPU`. That evaluation used to throw
  inside the type initializer of MSBuild's `FrameworkLocationHelper`. The cause was not missing
  .NET Framework GAC/registry resolution: it was an older copy of an MSBuild library the tool
  shipped alongside itself, which the SDK's newer MSBuild loaded in place of its own. The tool now
  ships none, so the SDK supplies every MSBuild assembly. The record stays `partial`/
  `binding-error` through its own compiler diagnostic, which comes from Roslyn, not from the
  independent inventory. See `tools/scout-semantic/README.md#build-context-envelope` for the
  general rule on a compilation whose independent inventory fails: it is never reported `complete`
  by falling back to "nothing was missing".
- The fingerprint's mutation-class matrix and the four-way multi-target/multi-configuration
  identity round trip both moved to a dedicated fixture, `fixtures/csharp-context-fingerprint/`
  (its own `README.md`), so that evidence no longer has to fit this fixture's own three engineered
  projects or its five committed snapshots.
