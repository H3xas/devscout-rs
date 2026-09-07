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
| `src/query.rs` | Root of the query layer: the verb/substrate map in its header, the `mod` list and the public re-exports. | Holds no code. Every public item keeps the path it had before the split; everything else is `pub(super)` at most. |
| `src/query/coverage.rs` | The tests-reaching-a-symbol verb (named to avoid colliding with this module's own `tests`): `build_tests_model`, `TestsModel`, `TestVia`. | A file's own attributed test def outranks the project-model vouch; a symbol nothing tests answers "none", never a name-convention guess. |
| `src/query/find.rs` | The `find` verb: `find_names`, `first_decl_line_by_file`, `file_inbound_counts`, `name_tier`, `source_line`. | `file_inbound_counts` excludes heuristic edges, `imports`, `ctor-di`, `ambiguous`, and a file's references to itself. |
| `src/query/impact.rs` | The `impact` verb: `impact_walk`'s reverse k-hop traversal, the interface hop and its fan-in brake, the hub brake, `resolve_impact_seed`, `build_impact_model`. | Records a file once, at its minimum hop. A heuristic edge reaches a file and stops there -- it never widens the walk further. |
| `src/query/index.rs` | `GraphIndex` and its two-phase load (`load_graph_index`, `load_graph_index_with`), `IndexOptions`, plus the def-site helpers `def_files`/`def_sites`/`symbol_refs`. | Heuristic edges are indexed in their own adjacency, never mixed into the precise one; `--no-guess` admits only the extension tier. A corrupt manifest.json fails open. |
| `src/query/infra.rs` | The hub-file name-pattern classification `impact` widens against: `is_infra_file`, `DEFAULT_HUB_MAX_INDEGREE`. | A classification of what a file is FOR, always on; the in-degree half of the hub brake is a separate, disable-able threshold. |
| `src/query/rank.rs` | Personalized PageRank over a file-level subgraph: `personalized_page_rank`, `DEFAULT_DAMPING`, `DEFAULT_ITERATIONS`. | Ranking only, never removes a node; callers must build the `nodes` array in a fixed order for a bit-stable result. |
| `src/query/read.rs` | The `read` verb: `build_read_model`, the declaration span (`ReadSpan`). | Reuses `refs` resolution and inbound machinery wholesale with `out` off; a span past EOF or before a shrunk file's start degrades to no span, never invented text. |
| `src/query/refs.rs` | The `refs` verb: `build_refs_model`, the bare-member fallback, enum member-ref rollup, ranked inbound/outbound capping. | The member path is reached only when nothing declares the query as a type. A bare-member hit is verified by a whole-token match on its referencing line. |
| `src/query/refs_tables.rs` | The row/table shaping `refs`, `read` and `impact` all reuse: `Table`, `InboundRow`/`OutboundRow`/`ImportRow`/`AmbiguousRow`, the row caps (`DEFAULT_CAP`, `INBOUND_CAP`, `OUTBOUND_CAP`, `SOURCE_MAX`). | Location sorts use plain `str::cmp`; `row_tier` never reports a stronger tier than the edges behind a row actually prove. |
| `src/query/seq.rs` | Insertion-order-preserving scratch structures: `SeqSet`, `SeqMap`. | First insertion wins a slot; neither type is a persisted artifact shape. |
| `src/query/symbol.rs` | Symbol resolution: `resolve_symbol`'s never-guess ladder and `Resolution`. | Exact id, then unique exact name, then unique case-insensitive name, then a unique dotted-id tail; two or more candidates at any step is ambiguous, never resolved further. |
| `src/query/tests.rs` | Shared fixture builders for the query tests and the list of test modules. | Fixtures build `graph::Graph` directly, no JSON round-trip; a temp `.git`-shaped dir backs the one manifest fixture that needs disk. |
| `src/query/tests/coverage.rs` | `build_tests_model`, the attribute and project-model vouches. | Pins that a harness file in a test project earns a row with no test defs of its own. |
| `src/query/tests/find.rs` | `find_names`, `file_inbound_counts`, `first_decl_line_by_file`. | Pins which edge kinds count as inbound interest and that a file's self-references never do. |
| `src/query/tests/heuristic_ordering.rs` | Heuristic-tier row ordering shared by `refs` and `impact`. | Pins that every precise row sorts before any heuristic row and that a full cap of precise rows leaves no room for guesses. |
| `src/query/tests/impact_from_lines.rs` | The per-edge-kind referencing line on an impact row. | Pins the lowest-line-per-kind rule and that an ambiguous-only hit still names its line under `direct`. |
| `src/query/tests/impact_hub.rs` | The hub-file brake: in-degree and `is_infra_file`'s name patterns. | Pins the four `is_infra_file` shapes and that a hub reached on the last hop is never reported as braked. |
| `src/query/tests/impact_iface.rs` | The ctor-di and interface-hop widening path. | Pins that an ambiguous ctor-di edge and a same-named foreign interface never widen the walk. |
| `src/query/tests/impact_iface_brake.rs` | The broad-interface fan-in brake. | Pins that the brake narrows only the braked interface's own hop, never a narrower sibling's. |
| `src/query/tests/impact_misc.rs` | Enum-reached files, `uses-member` blast radius, PPR sanity, seed-argument parsing. | Pins that `looks_like_file_path` matches the same shape a `.`-suffixed symbol id would. |
| `src/query/tests/impact_ranking.rs` | `build_impact_model`'s hop limit, file-path seeding, PageRank-ordered ranking. | Pins that ranking is deterministic across repeated runs and never drops a row below the cap. |
| `src/query/tests/index.rs` | `load_graph_index`'s manifest join, hub in-degree, `--no-guess`, the heuristic adjacency. | Pins that a guess never enters the precise adjacency and that `--no-guess` keeps the extension tier only. |
| `src/query/tests/read.rs` | `build_read_model`'s declaration span. | Pins that a reference inside the declaration's own span is excluded from its inbound count. |
| `src/query/tests/refs_bare_member.rs` | The bare-member fallback: `line_has_token`, verified single- and multi-owner lookups. | Pins that a longer identifier sharing the query's prefix is refused and that a type answers before a same-named member. |
| `src/query/tests/refs_enum.rs` | `build_refs_model` on an enum and its members. | Pins that a member's inbound edges union into the enum's own answer and that two same-named members stay ambiguous. |
| `src/query/tests/refs_tables.rs` | `build_refs_model`'s inbound/outbound/ambiguous tables, caps and ranking. | Pins the rank order (resolved before heuristic, own project before foreign) and that `--all` lifts both caps together. |
| `src/query/tests/symbol.rs` | `resolve_symbol`'s ladder, including the enum-member tail. | Pins the exact-id/unique-name/case-insensitive/dotted-tail order and that two candidates at any step stay ambiguous. |
| `src/render.rs` | Text and `--compact` rendering of the query models. | Rendering reads the model only: no graph access, no re-resolution. `--json` rendering lives in `cli`, not here. |
| `src/repo.rs` | Root discovery (`.scout` ancestor, git worktree, git common dir) and the registry file. | Root discovery returns absolute, normalised paths. A missing registry reads as empty; a corrupt one is an error, so a lost registry is never mistaken for an empty one. |
| `src/resolve.rs` | Root of C# resolution: the ladder rules in its header, the `mod` list and the public re-exports. | Holds no code. The public surface is `resolve_graph`, `resolve_graph_with_ts`, `resolve_graph_with_model`, `DefIndex`, `ExtCandidate`, `MemberLists`, `MethodOverloadParams`; every other item is `pub(super)` at most. |
| `src/resolve/index.rs` | The def index: `DefIndex`, `MemberLists`, `ExtCandidate`, `MethodOverloadParams`, `build_def_index`, `name_probe`. | Defs keep first-insertion order; a partial class's later files land in `also_in`, never as a second def. Enum members are keyed by qualified name but excluded from the simple-name pool. |
| `src/resolve/arity.rs` | Arity admission for instance overloads and extension entries, including generic-argument unification. | An optional parameter makes an entry a range and a `params` array accepts any count; concrete type arguments must match, a wildcard unifies with an unbound method type parameter. |
| `src/resolve/members.rs` | Member declaration checks (`declares_member`, `member_shape`, `member_vouched`) and the inheritance-closure walk that finds the base declaring a member. | The base walk visits class bases in declaration order before any interface, terminates on a cycle, and never binds a class-typed receiver to an interface declaration. A scored guess never vouches through a non-public member. |
| `src/resolve/scope.rs` | Per-file scope (`FileContext`: namespace, usings, aliases), candidate scoring, and global usings per project unit. | Global usings scope to the declaring unit when a project model exists, are repo-wide without one, and fall open for a file no project owns. An empty namespace is absent, not a matchable prefix. |
| `src/resolve/receiver.rs` | Receiver typing: field and property types, property and call-return hops, delegate and lambda slots, descriptor unification, the assignability cache, candidate admission, nested-type visibility. | A lambda parameter is typed positionally from the callee's delegate parameter and stays untyped when overloads disagree. An external receiver refuses a candidate not assignable to it. A chain tail whose hop fails emits no guess. |
| `src/resolve/ladder.rs` | The ladder itself: `resolve_ref` steps 0 to 4, dotted-suffix and nested-qualifier fallbacks, project-reachability narrowing (`Admission`, `narrow_by_reachability`), `capped_candidates`. | A step that finds exactly one candidate resolves; two or more stop as ambiguous. Ambiguous lists are capped at five and sorted by id. A dotted reference the exact step misses never reaches steps 2 to 4. |
| `src/resolve/edges.rs` | Edge construction (`type_edge`, the heuristic dedup key) and constructor-injection resolution (`resolve_ctor_param`, the implementor index, infrastructure-namespace detection). | `ctor-di` binds a sole implementor, prefers a closed implementor over an open generic one, and records a tie as ambiguous. Byte-identical guesses collapse to one edge. |
| `src/resolve/assembly.rs` | `resolve_graph`, `resolve_graph_with_ts`, `resolve_graph_with_model`: the pass over every file's fragments, the `Ext` and `Guess` tiers, stats. | A ref an earlier tier answered never gets a heuristic duplicate. The scored tier refuses a member name carried by more than `SCORED_UNIQUENESS_CAP` defs and emits at most `SCORED_EMIT_CAP` edges. A `None` project model leaves the graph byte-identical. |
| `src/resolve/tests.rs` | Shared fixture builders for the resolver tests and the list of test modules. | Fragments are built by hand, never parsed; `no_git_root` keeps `built_at_head` at `None`. |
| `src/resolve/tests/ladder.rs` | Head hash, aliases, ambiguity caps, enum-member ids. | Pins the alias short-circuit and the five-entry ambiguous cap. |
| `src/resolve/tests/member_qualifiers.rs` | `uses-member` qualifier rules and declaration-expression type refs. | Pins when a bare or dotted qualifier emits and when it is dropped silently. |
| `src/resolve/tests/qualified_names.rs` | Enclosing-namespace prefixes and foreign-qualified names. | Pins that a foreign-qualified name never binds a same-named in-tree type. |
| `src/resolve/tests/dotted_suffix.rs` | Dotted-suffix matches, global aliases, nested types through a derived type, same-namespace and global-using resolution. | Pins that two path-suffix matches stay ambiguous. |
| `src/resolve/tests/receiver_tier.rs` | The static-qualifier and declared-receiver tiers. | Pins that a receiver whose type does not declare the member earns no edge. |
| `src/resolve/tests/nested_end_to_end.rs` | Multi-level nested qualifiers and instance receivers named like a type. | Pins that a namespace type never shadows a same-named nested type. |
| `src/resolve/tests/extension_tier.rs` | Extension-method admission: namespaces, ranges, vetoes by declared members. | Pins that an instance member shadows a visible extension of the same name. |
| `src/resolve/tests/extension_generics.rs` | Generic `this` parameters against concrete and wildcard receivers. | Pins exact concrete-argument matching. |
| `src/resolve/tests/scored_tier.rs` | The scored guess tier and the member-shape rule. | Pins the uniqueness and emit caps and the emitted order. |
| `src/resolve/tests/receiver_rule.rs` | External-receiver assignability and generic-sibling selection. | Pins that a generic receiver binds the generic sibling, not the first indexed def. |
| `src/resolve/tests/byte_identity.rs` | Serialization stability of the edge array with and without heuristics or a project model. | Pins the byte-identity gate the moves are held to. |
| `src/resolve/tests/project_admission.rs` | Project reachability: admission, unit-scoped global usings, narrowing. | Pins that a non-test site never names a def in a test project. |
| `src/resolve/tests/graph_invariants.rs` | Edge schema, stats, partial-class merging, dedup, counts. | Pins that only heuristic edges carry a tier and the heuristic count is appended last. |
| `src/resolve/tests/nested_step.rs` | The enclosing-type step of the ladder. | Pins that the innermost enclosing type wins and a dotted ref never enters the step. |
| `src/resolve/tests/ctor_di.rs` | Constructor-injection edges. | Pins sole-implementor binding and ambiguity on a tie. |
| `src/resolve/tests/receiver_hops.rs` | Property hops and `var` typed from a call's return. | Pins that an unrecorded property or an ambiguous type stops the hop. |
| `src/resolve/tests/base_members.rs` | `this`, `base` and inherited field receivers; local, catch and parameter shadowing. | Pins that an in-file local shadows a same-named field fact. |
| `src/resolve/tests/base_walk.rs` | Base-walk order, overload arity admission, chain tails through base methods, static qualifiers on bases. | Pins declaration-order base visiting before interfaces. |
| `src/resolve/tests/lambda_parameters.rs` | Lambda parameters typed from delegate parameters of in-graph callees. | Pins positional typing and the untyped outcome for disagreeing overloads. |
| `src/store.rs` | SQLite freshness stores: `cache.db` per root and the shared content database. | Opened with WAL and a five-second busy timeout; migrations are idempotent and a fresh database takes none of them. Populated only by the hooks; `stats` reads and `clear` prunes. |
| `src/suggest.rs` | Did-you-mean suggestions for a query that matched nothing. | Suggestions are printed and never substituted for the query or run. At most `SUGGESTION_CAP` names, nearest first. |
| `src/tsgraph.rs` | Resolution of TypeScript-family fragments: relative specifiers, `tsconfig` `paths` / `baseUrl` aliases with the `extends` chain, re-export barrels, the TS edge kinds. | A bare specifier resolves only as far as the repo's own tsconfig takes it; anything else is `external`, never a guess at a package's internals. Barrel hops are bounded and the barrel a source names is kept as `via` on the edge. |
| `src/walk.rs` | Source-tree walking, `SKIP_DIRS`, `SOURCE_EXT` and the default file-purpose heuristic. | `SOURCE_EXT` is the one list of indexed extensions; a file outside it is invisible to every verb. `SKIP_DIRS` matches a single path component exactly, at any depth. Roots handed in are absolute and normalised. |

## Where a change goes

| To add or change | Go to |
| --- | --- |
| A C# construct the index should record | `src/extract.rs` for the walker, `src/graph.rs` for the fragment shape and its cache version, a fixture under `fixtures/` with its README, the construct's row in `docs/csharp-coverage.md`. |
| A resolution rule or tier for `uses-member` | `src/resolve/ladder.rs` for a type-resolution step, `src/resolve/receiver.rs` for receiver typing, `src/resolve/assembly.rs` for a heuristic tier, with the test file of the same topic under `src/resolve/tests/`; if the edge gains a tag, `src/graph.rs` (`Edge`, schema version) and `src/audit.rs` (tier scoring); then re-run the semantic audit on the fixture and record the numbers in `docs/benchmarks/`. |
| A TypeScript edge kind or alias rule | `src/extract.rs` (TS section) and `src/tsgraph.rs`; fixture under `fixtures/ts-grammar/`; the Limitations list in `README.md`. |
| A query verb | A model and builder in the matching `src/query/*.rs` (`find.rs`, `refs.rs`/`refs_tables.rs`, `read.rs`, `impact.rs`, `coverage.rs` for tests-reaching-a-symbol), the matching test topic under `src/query/tests/`, text and compact renderers in `src/render.rs`, JSON rendering plus argument parsing and usage text in `src/cli.rs`, the command table in `README.md`, an integration test under `tests/`. |
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
