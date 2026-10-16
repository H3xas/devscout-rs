# candidate-sections fixture

Invented vocabulary only: a ship's crew and a harbour office, nothing from any real codebase.
The fixture pins what `--pick` answers on `refs`, `read`, `impact` and `tests`.
`tests/cli_candidate_sections.rs` drives the built binary against it.

| File | What it declares |
| --- | --- |
| `IHold.cs` | `IHold.Stow(int)` -- the interface member. |
| `Hold.cs` | `Hold : IHold` and `Hold.Stow(int)` -- the implementation. |
| `Crane.cs` | Calls `Stow` through an `IHold` parameter. |
| `Dock.cs` | Calls `Stow` through a `Hold` parameter. |
| `Manifest.cs` | `Nautical.Crew.Manifest.Seal()`. |
| `Office.cs` | `Harbor.Office.Manifest.Seal()` -- the same type name in another namespace. |
| `Purser.cs` | Calls `Seal` on the `Nautical.Crew` manifest. |
| `Clerk.cs` | Calls `Seal` on the harbour office manifest. |
| `Logbook.cs` | A `[Fact]`-marked call to `Seal`, so one manifest has a test and the other does not. |
| `Galley.cs` | `Nautical.Crew.Galley.Simmer(int)`. |
| `FleetGalley.cs` | `Fleet.Nautical.Crew.Galley.Simmer(int)` -- an owner whose id ends with the first one's. |
| `Cook.cs` | Calls `Simmer` once through each galley. |
| `Mooring.cs` | `Nautical.Crew.Mooring.Furl()`, which nothing calls. |
| `Hawser.cs` | `Harbor.Office.Hawser.Furl()`, which nothing calls. |

## Case table

| Seed | Situation | Candidates | `--pick all` and `--pick <n>` |
| --- | --- | --- | --- |
| `Stow` | One member name on an interface and on its implementation, each with a different caller. | `Nautical.Crew.Hold.Stow`, `Nautical.Crew.IHold.Stow` | One section per declaring type, each carrying that type's own caller. |
| `Manifest` | One type name in two namespaces. | `Harbor.Office.Manifest`, `Nautical.Crew.Manifest` | One section per type. Only the harbour office manifest has a test. |
| `Simmer` | An owner whose id (`Fleet.Nautical.Crew.Galley`) ends with another candidate's id (`Nautical.Crew.Galley`). | `Fleet.Nautical.Crew.Galley.Simmer`, `Nautical.Crew.Galley.Simmer` | Each candidate answers its own owner, and neither answers the other's caller. |
| `Furl` | One member name on two types, and no caller of either. | `Harbor.Office.Hawser.Furl`, `Nautical.Crew.Mooring.Furl` | One section per type, both empty: a `zero-hit` answer as a whole, on `refs` and on `impact`. |

The candidate order is the order the refusal prints, which is name-index order for a member and
the rendered row's own order for a type. Every case here reads that order, never an assumed one.
