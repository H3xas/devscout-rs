# member-seeds fixture

Invented vocabulary (a ship's crew, nothing from any real codebase) pinning the four
seed situations a member seed can put `refs`/`read`/`impact`/`tests` in, one per
`outcome` value. `tests/cli_member_seeds.rs` drives the built binary against it.

| File | What it declares |
| --- | --- |
| `Galley.cs` | `Galley.Stow(int)` and `Galley.Ladle()`. |
| `Larder.cs` | `Larder.Stow(int)` -- the same name as `Galley.Stow`, on an unrelated type. |
| `Anchor.cs` | `Anchor.Weigh()`, declared but never called from anywhere in this fixture. |
| `Quartermaster.cs` | Calls `Galley.Stow`, `Larder.Stow` and `Galley.Ladle` once each. |

## Case table

| Seed | Situation | `outcome` |
| --- | --- | --- |
| `Ladle` | A unique bare member name -- only `Galley` declares it. | `hit` |
| `Stow` | A member name carried by two types (`Galley` and `Larder`), both referenced. | `ambiguous` |
| `Anchor.Weigh` | A `Type.Member` spelling, naming a member nothing else in the fixture calls. | `zero-hit` (on `impact`: the seed resolves, its blast radius is empty) |
| `Boatswain` | A name the graph does not hold at all. | `fallback-advised` |
