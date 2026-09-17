# csharp-context — build-context envelope fixture

A package-light, mostly-offline solution (`Fixture.sln`) that exercises the compiler oracle's
`--emit context` mode: three projects, each engineered to hit a different context state, plus a
document dropped for every reason `RepoPaths.Classify` names.

| Project | Declared targets | What it proves |
|---|---|---|
| `Clean.csproj` | `net9.0` | One authored document, one source-generated document (`System.Text.Json` serializer context, ships in the shared framework, no new package), one `Compile` item pointing at a file that is never created, one pointing outside this fixture's root, one under `bin/` (a devscout skip directory). |
| `Legacy.csproj` | `net472;net9.0` | Multi-targeting. Under `net472` it fails to bind `string.Contains(char, StringComparison)` (a .NET-only overload), the fixture's control for a real, non-fabricated `partial`/`binding-error` compilation. Under `net9.0` it binds cleanly. |
| `Broken.csproj` | `net9.0` | A `ProjectReference` to a `.csproj` that does not exist on disk. `MSBuildWorkspace`'s `SkipUnrecognizedProjects` still produces a non-null compilation for this project (verified empirically, see the implementation journal's dated correction), so this is a `partial`/`workspace-failure` control, not the `failed` state the design interview originally assumed. |

`../csharp-context-outside/Linked.cs` lives one level above this fixture's root on purpose: it is
the fixture case for the `linked-outside-root` drop reason, and must stay outside
`fixtures/csharp-context/` for that reason to fire.

## Committed envelopes

Five snapshots, each a full `--emit context` run, regenerated and diffed byte-for-byte in CI the
same way `fixtures/csharp-flowtrace/facts.json` is:

| File | Command (besides the shared `--root fixtures/csharp-context Fixture.sln --emit context`) | What it shows |
|---|---|---|
| `context.json` | *(no `--tfm`)* | Deterministic ordinal-least selection (`net472` over `net9.0` for `Legacy`, both spelled with a `net` prefix but `4` sorts before `9`); every drop reason but `out-of-scope`; `excluded`/`not-requested` for the unselected `Legacy` variant. |
| `context-tfm-net9.0.json` | `--tfm net9.0` | Every project has a declared `net9.0` variant, so nothing is `excluded`; `Legacy` is `complete` here (the net9.0 overload exists), coexisting in the same envelope with `Clean` and `Broken`, both still `partial` for their own reasons -- the fixture's proof that a partial sibling does not poison a complete one. |
| `context-tfm-net9.0-release.json` | `--tfm net9.0 -p:Configuration=Release` | Same identities as the file above but `configuration: "Release"`; together the pair proves two identities of one project that differ only by configuration stay distinct, with distinct fingerprints, and round-trip separately. |
| `context-tfm-net48.json` | `--tfm net48` | `net48` is declared by no project here: every record is `unsupported`/`undeclared-target`, naming both the requested and the declared targets, with zero facts under any of them. |
| `context-scoped.json` | `--scope src/Legacy` | Documents under `src/Clean` and `src/Broken` are `out-of-scope`, the one drop reason the other four files do not exercise; project identities themselves are never scope-filtered. |

Every file above was also generated twice and diffed byte-identical before being committed
(recorded in the implementation journal, not re-asserted by `cargo test`, which never invokes
`dotnet`).

## Known gaps in this pass

- **`net472` evaluation.** `Legacy`'s `net472` variant compiles fine through Roslyn's own
  out-of-process BuildHost (using `Microsoft.NETFramework.ReferenceAssemblies`), but this
  fixture's independent, in-process `ContextInventory` evaluation of that same variant throws
  inside MSBuild's `FrameworkLocationHelper` on this machine (and, believed but not separately
  verified, any non-Windows CI runner) -- classic .NET Framework GAC/registry resolution has no
  non-Windows equivalent. The tool catches this and degrades gracefully (a warning on stderr, an
  `expected`/`dropped` list that falls back to exactly what loaded, `configuration`/`platform`
  left `null`), so the run still completes and the compiler-diagnostic-driven `partial` state for
  `net472` is still real and still proven -- only the independent document-inventory diff for that
  one variant is unavailable here.
- **The fingerprint's mutation-class matrix (acceptance's `AC-5`) is not built in this pass.** No
  before/after fixture pair exists yet for reference, import, project-build-config, preprocessor-
  symbol, language-option, generator-input, dependency-fingerprint or SDK/MSBuild/compiler-version
  changes. The fingerprint mechanism itself is exercised (every committed envelope carries real,
  distinct SHA-1 fingerprints per identity, folding references/imports/symbols/language options/
  versions/build identity/project-reference chain -- see `ContextFingerprint.cs`), but the
  acceptance's own "changing X changes the fingerprint, the consuming file stays byte-identical"
  evidence, one pair per mutation class, is unresolved and recorded as such in the ticket's
  implementation handoff.
- **`AC-4`'s four-identity round trip is a reduced, two-axis proof**, not the full matrix: two
  distinct targets (`net472` vs `net9.0`, via `context.json` and `context-tfm-net9.0.json`) and,
  separately, two distinct configurations of one target (`net9.0` `Debug` vs `Release`, via
  `context-tfm-net9.0.json` and `context-tfm-net9.0-release.json`) -- not four identities merged
  in one assertion, because the `net472`/`Release` combination inherits the evaluation gap above
  (its `configuration` field is unreadable there too, so it adds no new distinguishing evidence).
