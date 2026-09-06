# Architecture

How `devscout` is put together: what each module owns, what it must keep true, and where a
change goes. Read this file before the code. `tools/check-architecture.sh` fails CI when a
module under `src/` has no row here or a row names a module that no longer exists, so the
table below is complete by construction.

## Pipeline

One `devscout map` run flows left to right; every query verb reads what it wrote.

```
walk ─► parse ─► extract ──┐
          │      markup ───┼─► resolve (C#) ─┐
          └── preproc      │   tsgraph (TS) ─┼─► graph ─► query ─► render ─► cli
                           └───────────────► manifest ─────────────────────┘
project ─► resolve        repo / initcmd / mapcmd / store / hookio / suggest / audit / offsets
```

- **Indexing** (`map`): `walk` lists files, `parse` and `preproc` turn C# into a syntax tree,
  `extract` and `markup` turn the tree into per-file fragments, `resolve` / `tsgraph` turn
  fragments into a graph, `graph` persists it, `manifest` persists the file-purpose index.
- **Querying** (`find`, `refs`, `read`, `impact`, `tests`): `query` builds an in-memory index
  over the persisted graph and answers; `render` formats; `cli` parses arguments and returns
  `(exit code, output)`.
- **Agent integration** (`init`, `hook`): `initcmd` installs hooks and registers the repo,
  `hookio` rewrites tool results, `store` keeps the freshness caches.
- **Measurement** (`audit --semantic`): `audit` scores `uses-member` edges against an
  oracle export.

## Modules

One row per file under `src/`. Invariants are the facts the rest of the crate relies on;
breaking one is a behaviour change even when every test still passes.

| Module | Responsibility | Invariants |
| --- | --- | --- |
| `src/lib.rs` | Crate root: declares every module and documents it in one line. | Every module under `src/` is declared here with a one-line doc. |
| `src/main.rs` | Binary entry point. | Hands `std::env::args()` to `cli::dispatch` and does nothing else. |
| `src/audit.rs` | `audit --semantic`: scores the graph's `uses-member` edges against an oracle's reference records, per tier, and evaluates `--assert` files. | Reads the graph read-only. `--assert` collects every violated or missing key before exiting non-zero. Oracle records are joined to edges by site (file and line), target and member; a `partial file mismatch` only affects the file join of true positives, never precision. |
| `src/cli.rs` | Argument parsing, usage text and dispatch for every verb; JSON rendering of query models. | Every command returns `(code, output)`: `0` success, `1` failure, `2` usage error. `--json` and `--compact` are mutually exclusive on `refs`, `read`, `impact` and `tests`, and the conflict is reported before any other argument check. Usage text lives here and nowhere else. |
| `src/extract.rs` | C# and TypeScript-family extraction: declarations, usings, references, receiver facts, lambda slots and the file-purpose signature. | The extraction functions are pure over one source string; only the `extract-dump` plumbing verb touches the filesystem. C# source is stripped by `preproc` before extraction, so only the arm that would compile is recorded. A receiver `Fact` carries the declared type name plus its top-level type arguments, and two facts agree only when both halves do. A `LambdaSlot` records where an untyped lambda sits so the resolver can type it; extraction never resolves. |
| `src/graph.rs` | Persisted data model: `Graph`, `Def`, `Edge`, fragments, the fragment cache, staleness checks and `rebuild_graph`. | `GRAPH_SCHEMA_VERSION` is stamped into every `graph.json` written and demanded before an existing one is reused. The fragment cache filename carries the fragment schema version; bump it whenever the extractor records something old fragments lack, and delete superseded generations. Serialisation is order-preserving (`OrderedMap`), so identical input yields byte-identical `graph.json`. The `heuristic` and `tier` tags are absent on precise edges, never written as `false`. Artifacts live under the git common directory so worktrees share them. |
| `src/hookio.rs` | `hook read` and `hook bash`: rewrite a tool result on stdin when the same content was already delivered in the session. | Fails open: malformed, non-UTF-8 or incomplete input yields empty output and never an error. Only these hooks write the freshness stores; plain CLI use never touches them. |
| `src/initcmd.rs` | `init`: registers the root, creates the artifact directory, installs the agent hooks into the settings file, runs a first map. | Merges hook entries into an existing settings file after backing it up; a hook-install failure never fails `init`. `--no-hooks` skips the install and nothing else. Adds `.scout` to the local git exclude file. |
| `src/manifest.rs` | `manifest.json` read/write, `find` search, `index-state.json` and the query-time freshness warning. | `find` ANDs whitespace-split lowercase tokens over `path + purpose` and falls back to any-token matches ranked by hit count. A missing or corrupt `index-state.json` reads as absent, never as an error. The freshness warning costs one `git rev-parse HEAD` and one scoped `git status --porcelain`, the same two calls `map` pays. |
| `src/mapcmd.rs` | `map`: assembles walk, extract, resolve, graph and manifest into one incremental run. | Unchanged files are reused from the fragment cache (content hash by default, mtime under `SCOUT_MTIME_REUSE=1`). The unchanged path opens no graph file. A `.csproj` edit is detected through the project-units sidecar, not through the fragment index. |
| `src/markup.rs` | XAML, RESW and RESX extraction: `x:Class` definitions, element references and resource keys. | A markup `x:Class` def carries the same id as its code-behind `partial class`, so the two merge into one symbol with two declaring sites. |
| `src/offsets.rs` | UTF-8 byte offset to UTF-16 code-unit offset table. | Off every code path today; kept unit-tested for a caller that must cross a UTF-8 buffer back into the UTF-16 offset convention `parse` uses. |
| `src/parse.rs` | Tree-sitter parsing for C# and the TypeScript-family grammars; span collection; the `parse` and `spans` plumbing verbs. | Source is fed to the grammar as UTF-16 code units, so every node offset the crate sees is already UTF-16. Grammar crates are pinned to exact versions in `Cargo.toml`, and `docs/csharp-coverage.md` names the pin its tables were checked against. |
| `src/preproc.rs` | C# conditional-compilation pre-pass: blanks inactive `#if` / `#elif` / `#else` arms. | Evaluates with no predefined symbols. Inactive bytes become spaces and every line break stays where it was, so line and column of surviving code do not move. |
| `src/project.rs` | Hand-scanned `.csproj` and `Directory.Build.props` model: discovery, project units, reference closure, test-project detection. | Discovery never fails because nothing was found; zero projects is `Ok(None)`. The sidecar comparison is a raw byte compare against `project-units.json`. |
| `src/query.rs` | In-memory `GraphIndex` over a persisted graph and the models behind `find`, `refs`, `read`, `impact` and `tests`: symbol resolution, reverse walks, page-rank ranking, caps. | Symbol resolution never guesses: exact id, then unique exact name, then unique case-insensitive name; two or more candidates at any step is reported as ambiguous. `--no-guess` drops heuristic edges from every answer. `impact` records a file once, at its minimum hop. Table caps and hop defaults are constants in this module; `--all` lifts the `refs` caps. |
| `src/render.rs` | Text and `--compact` rendering of the query models. | Rendering reads the model only: no graph access, no re-resolution. `--json` rendering lives in `cli`, not here. |
| `src/repo.rs` | Root discovery (`.scout` ancestor, git worktree, git common dir) and the registry file. | Root discovery returns absolute, normalised paths. A missing registry reads as empty; a corrupt one is an error, so a lost registry is never mistaken for an empty one. |
| `src/resolve.rs` | Resolution of C# fragments into the graph: the def index, the qualified-name ladder, receiver typing, arity matching and the two heuristic `uses-member` tiers. | Pure over the fragment list except one `git rev-parse HEAD` for `built_at_head`; unit-testable without a parser. The ladder runs, in order: alias short-circuit, enclosing type chain, exact qualified name, alias-headed qualifier, dotted suffix, nested type qualifier, file usings, enclosing namespaces, globally unique simple name; a name that stays ambiguous is recorded as ambiguous, never picked. Partial-class duplicates land in `also_in`, never as a second def. The `Ext` tier is C#'s own extension-method lookup over a recorded `(member, this-type)` bucket; the `Guess` tier picks by name among defs declaring the member and is the only tier that can be wrong by construction. A `None` project model leaves the graph byte-identical. |
| `src/store.rs` | SQLite freshness stores: `cache.db` per root and the shared content database. | Opened with WAL and a five-second busy timeout; migrations are idempotent and a fresh database takes none of them. Populated only by the hooks; `stats` reads and `clear` prunes. |
| `src/suggest.rs` | Did-you-mean suggestions for a query that matched nothing. | Suggestions are printed and never substituted for the query or run. At most `SUGGESTION_CAP` names, nearest first. |
| `src/tsgraph.rs` | Resolution of TypeScript-family fragments: relative specifiers, `tsconfig` `paths` / `baseUrl` aliases with the `extends` chain, re-export barrels, the TS edge kinds. | A bare specifier resolves only as far as the repo's own tsconfig takes it; anything else is `external`, never a guess at a package's internals. Barrel hops are bounded and the barrel a source names is kept as `via` on the edge. |
| `src/walk.rs` | Source-tree walking, `SKIP_DIRS`, `SOURCE_EXT` and the default file-purpose heuristic. | `SOURCE_EXT` is the one list of indexed extensions; a file outside it is invisible to every verb. `SKIP_DIRS` matches a single path component exactly, at any depth. Roots handed in are absolute and normalised. |

## Where a change goes

| To add or change | Go to |
| --- | --- |
| A C# construct the index should record | `src/extract.rs` for the walker, `src/graph.rs` for the fragment shape and its cache version, a fixture under `fixtures/` with its README, the construct's row in `docs/csharp-coverage.md`. |
| A resolution rule or tier for `uses-member` | `src/resolve.rs`; if the edge gains a tag, `src/graph.rs` (`Edge`, schema version) and `src/audit.rs` (tier scoring); then re-run the semantic audit on the fixture and record the numbers in `docs/benchmarks/`. |
| A TypeScript edge kind or alias rule | `src/extract.rs` (TS section) and `src/tsgraph.rs`; fixture under `fixtures/ts-grammar/`; the Limitations list in `README.md`. |
| A query verb | A model and builder in `src/query.rs`, text and compact renderers in `src/render.rs`, JSON rendering plus argument parsing and usage text in `src/cli.rs`, the command table in `README.md`, an integration test under `tests/`. |
| A CLI flag | The verb's argument loop and usage string in `src/cli.rs`, the matching option on the query model, `README.md`. |
| A fixture | A directory under `fixtures/` with a README stating what it pins, plus the integration test under `tests/` that drives the binary against it; fixture vocabulary is invented, never copied from a real codebase. |
| A markup format | `src/markup.rs` for the facts and `src/walk.rs` for the extension. |
| A hook behaviour | `src/hookio.rs` for the rewrite, `src/store.rs` for what is remembered, `src/initcmd.rs` for how it is installed. |
| A persisted artifact or its location | `src/graph.rs` or `src/manifest.rs` for the file, `src/repo.rs` for where roots and the git common dir come from, the storage table in `README.md`. |
| An environment variable | Read it where it applies, list it in the README environment table. |
| A module | Declare it in `src/lib.rs` with a one-line doc, add its row above, keep it clean under `cargo clippy --all-targets -- -D warnings` as `CONTRIBUTING.md` asks. |

## Tests

Unit tests live beside the code they cover; integration tests under `tests/` drive the built
binary against `fixtures/`. `tests/conformance.rs` runs the public conformance suite;
`tests/semantic_audit.rs` and `tests/semantic_audit_direction.rs` score the resolver against the
committed oracle snapshots that CI also regenerates. Nothing in the suite reaches the network.
