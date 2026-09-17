# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **A new offline semantic-truth harness and fixture pack** (`devscout_rs::truth`, dev/test
  infrastructure only, no new CLI verb): a versioned case manifest schema with independently
  reviewed expectations, span-based occurrence identity, a nine-row fault-control battery, a
  compatibility-profile registry with a machine-readable capability matrix, freshness/
  transformation controls, and a committed truthful red baseline recording today's known
  analyzer misses rather than repairing them. Offline and deterministic: no `dotnet` and no
  network in the fast lane. A separate scheduled workflow probes the demonstrated profiles
  against a real compiler.

## [0.6.0] - 2026-09-09

A reach release: the graph learns which implementation a registered service resolves to, and
`impact` can name files that live in another repository.

### Added

- **`import-edges <file> --repo <id>`** loads a versioned cross-repo edge export into an
  auxiliary artifact beside `graph.json`, never touching the graph schema. `impact` then names
  files reached only through an imported edge -- directly, or composed across a fileless message
  node -- each row carrying `why: "imported-edge"`, the foreign repo id, and the export's
  provenance id. Imported rows are capped and counted apart from the native ones, so an import
  can never evict a native row. An invalid export is refused with exit 1 naming the offending
  value, leaving any pre-existing artifact untouched; a successful import replaces the prior set
  wholesale. `impact --no-imports` skips it, and a repo with no import configured answers exactly
  as it did before.
- **Dispatch edges from dependency-injection registrations.** A registration whose method name
  begins `Add` or `TryAdd`, ends `Singleton`, `Scoped` or `Transient` and carries exactly two type
  arguments becomes an `implements` edge from the implementation to the service type, provided
  both resolve to exactly one in-graph definition. A member-level `implements` edge is then added
  per matching interface method and an `overrides` edge per method carrying the literal `override`
  modifier, scoped to the implementation types a registration named. Arity ties emit nothing,
  matching the resolver's never-guess rule, and every other registration spelling -- keyed and
  named included -- records nothing extra by construction. A repository with no registrations
  gains no edges.
- **`refs`, `read`, `impact` and `tests` traverse the two new edge kinds**, joining inbound and
  outbound the same way `inherits` already does, with `--no-dispatch` on each verb to answer
  without them.

### Changed

- **Graph schema 3.** `Edge::Implements` and `Edge::Overrides` with their `edges_by_kind`
  counters, `FragDef.override_methods` and `Fragment.registrations` in the fragment shape, and
  the fragment cache generation at v19 -- the first run after upgrading remaps.
- **`src/cli.rs`, `src/render.rs` and `src/graph.rs` are thin module roots** over `src/cli/`
  (one module per verb), `src/render/` (one module per verb) and `src/graph/` (one module per
  artifact layer). Every public item kept the path it had, and `graph.json` is byte-identical
  across each split on the pinned corpus.
- The comment-hygiene scanner is the vendored canonical one, its `--selfcheck` runs in CI
  alongside the scans, plan labels are a rejected comment class, and hook mode scopes itself the
  way `--scan` does.

## [0.5.0] - 2026-09-07

A code-organisation release: the three largest modules are split by concern, CI now holds
them that way, and every `--json` answer says what it is and why.

### Added

- **`ARCHITECTURE.md`.** One row per module under `src/` giving its responsibility and its
  invariants, plus a table saying where a change goes. `tools/check-architecture.sh` fails
  when a module has no row or a row names a module that no longer exists, and CI runs it.
- **Size and complexity gates.** `too_many_lines` (100) and `cognitive_complexity` (25) are
  denied; each existing offender carries one `#[allow(..., reason = "...")]` attribute.
  `tools/size-ratchet.toml` caps how many of those attributes the tree may hold and caps
  every file under `src/` at 800 lines, except the large files it names individually at
  their current length. Those numbers only shrink. `tools/check-size-and-complexity.sh`
  enforces the ratchet, a self-test proves the checker rejects what it should, and CI runs
  both alongside `cargo clippy --lib --bins --locked`.
- **Member seeds on `refs`, `read`, `impact` and `tests`.** A bare member name,
  `Type.Member` or `Namespace.Type.Member` resolves as a seed once the type-resolution
  ladder has found nothing, so no answer a type seed used to give changes. A member carried
  by more than one type lists one candidate row per declaring type with its file and line,
  never a bare type list, and `--pick N` narrows to the nth. When the did-you-mean pass
  holds an exact-name match it is answered instead of advising a text search.
- **`outcome` on every `--json` answer.** One of `hit`, `zero-hit`, `ambiguous` or
  `fallback-advised`, from a closed vocabulary, so a caller can count dead ends without
  parsing prose.
- **`schema_version` and per-row `why` on every `--json` answer.** `schema_version` is the
  first key of the object; every hit row carries a `why` naming the rule or tier that
  produced it, drawn from a closed vocabulary and derived from the edge the row came from.
  [`docs/answer-contract.md`](docs/answer-contract.md) documents the shape per verb with a
  worked example, and says that consumers should ignore unknown keys.
- **Query telemetry behind `SCOUT_TELEMETRY=1`.** Each answered `find`/`refs`/`read`/`impact`/
  `tests` invocation appends one JSON line to `scout/log/queries.jsonl` under the artifact
  directory: timestamp, record schema version, verb, seed, outcome, elapsed milliseconds,
  result bytes and candidate count. A usage error or a seed with no resolved repository or
  graph logs nothing; without the variable nothing is created; an unwritable log changes
  neither exit code nor output. Telemetry is opt-in: export `SCOUT_TELEMETRY=1` in the shell
  that runs the query verbs. The agent hooks never run those verbs, so `devscout init` does
  not set the variable for them.

### Changed

- **`src/resolve.rs` is a thin module root over `src/resolve/`** — the def index, arity
  admission, member checks, file scope, receiver typing, the ladder, edge construction and
  graph assembly, with the unit tests in per-topic files under `src/resolve/tests/`.
- **`src/extract.rs` is a thin module root over `src/extract/`** — split by construct
  family: types, type definitions, members, references, receivers, lambdas, qualifiers, the
  extraction walk, the dump and JSON plumbing, and the TypeScript-family extraction.
- **`src/query.rs` is a thin module root over `src/query/`** — one module per verb (`find`,
  `refs`, `read`, `impact`, and `coverage` for the tests-reaching-a-symbol verb) over the
  shared substrate they sit on: the graph index, symbol resolution, ranking, hub-file
  classification and the ordered collections.
- Every public item kept the path it had before the three splits, and `graph.json` is
  byte-identical before and after each of them on the pinned corpus and on both audited
  fixtures.
- **An ambiguous member seed answers with candidate rows.** It previously rendered exactly
  like an ambiguous type name and ignored `--json`; it now lists the member candidates and
  emits JSON like every other answer.

## [0.4.0] - 2026-09-06

### Added

- **`uses-member` edges carry `tier` and `member`.** `tier` names which heuristic tier
  emitted the edge (`ext` for extension-method lookup, `guess` for the scored name match);
  `member` names the member the reference reads or calls, on precise and heuristic edges
  alike. Only `tier` (and `heuristic`) are omitted on a precise edge; `member` is written
  there too, which is why a precise edge's bytes change under this schema bump as well.
- **`--no-guess` on `refs`, `read`, `impact`, and `tests`.** Admits only the `ext` tier's
  edges into the query surface, dropping every scored-tier guess; compact output marks the
  two tiers `x` and `h`.
- **`stats.heuristic_by_tier` splits `heuristic_edge_count` by tier.** Always written, with
  `ext` and `guess` counts that sum to `heuristic_edge_count`.
- **A lightweight `.csproj` project model.** Hand-scans `ProjectReference`,
  `Microsoft.NET.Test.Sdk`, and `IsTestProject` out of `.csproj` and `Directory.Build.props`
  files, with no MSBuild evaluation. Discovered projects are persisted as `units` in
  `graph.json` and mirrored in a `project-units.json` sidecar; editing a `.csproj` is now
  itself a rebuild trigger.
- **`tests` lists harness files that live in test projects.** When a project model exists, a
  file in a test project that reaches the symbol is listed even without attribute-marked test
  methods, suffixed `(test project)` in text and carrying `"via":"project"` (appended last) in
  JSON, so a harness reference stays distinguishable from a discovered test; `impact`'s
  `testsAffected` counts both.
- **`this.` and `base.` member accesses resolve to the enclosing type and its bases.**
  `this.Name` types the reference as the enclosing type, including its own type arguments
  when the type is generic, and emits no reference outside any type; `base.Name` starts the
  member lookup at the enclosing type's in-graph bases, in declaration order, walking each
  base's own inheritance chain, and never considers the enclosing type itself — no in-graph
  base declaring the member resolves as an ordinary external receiver, not a guess.
- **`?.` bindings emit the same reference a plain member access would.** `a?.B` resolves
  exactly as `a.B` does, at the conditional-access expression's own line.
- **Local type facts see through `await`, casts, patterns, and typed `out` parameters.**
  `var x = await Repo.LoadAsync();`, `var x = (T)e;`, an `is`/switch pattern designation, and
  `out T x` all record a type fact for the introduced name; `out var x` still records none.
- **A local assigned from an awaited call resolves through one `Task<T>`/`ValueTask<T>`
  unwrap.** `var o = await Repo.LoadAsync();` types `o` from the unwrapped return type
  rather than the raw `Task<...>`, and a `Task<Task<T>>` return is unwrapped only once.
- **One-hop call-chain tails are typed.** A member access on the result of `a.B()` carries
  the inner call as its receiver and resolves through one method-return hop; a qualifier
  that is itself a chain (`.D` on `a.B().C()`) or a null-conditional chain (`a?.B()?.C`)
  emits no typed reference.
- **A collection receiver types its first lambda argument's sole parameter.** A
  single-parameter lambda passed as the first argument to a call on an array- or
  single-type-argument-generic-typed identifier gets that parameter typed as the element
  type; two-argument generics, multi-parameter lambdas, and later arguments get none. The
  rule reads the receiver's shape only, so a single-type-argument wrapper such as `Task<T>`
  or `Lazy<T>` types the parameter as its type argument too.
- **Field types cross files.** A bare-identifier receiver with no in-file type fact is typed
  from the enclosing type's own field and property declarations, merged across every file a
  partial type spans, then from the same tables on each in-graph base in declaration order;
  an in-file local or parameter of the same name always wins over this fallback.
- **Method arities are recorded per overload.** Each declared method records the
  parameter-count range every overload accepts, an unbounded `params` overload left
  open-ended and optional parameters lowering the minimum.
- **An untyped lambda parameter is typed from the callee's delegate parameter.** Every
  method records its parameter types per overload (`methodParams`, type parameters as `*`,
  an extension method's receiver as `this <type>`; a `delegate` declaration records its own
  parameters under `Invoke`). A lambda passed straight to `ident.M(...)`, `this.M(...)` or a
  bare `M(...)` whose parameter carries no annotation and earns no collection element fact
  records the call it sits in; when that callee is an in-graph method (or extension) whose
  parameter at the lambda's position is `Action<..>`, `Func<..>`, `Predicate<T>`,
  `Expression<>` of one of those, or an in-graph `delegate`, each lambda parameter is typed
  positionally from that delegate and its member accesses resolve as typed-receiver edges.
  Overloads that can take the lambda must agree on the type, a `*` yields nothing, and the
  callee's receiver is read one hop deep only (an in-file fact, a static class name, or the
  enclosing type), and a parameter name that two lambdas in one member bind to different
  callees records no slot. The fragment cache moves to v18.
- **TypeScript aliases follow the nearest `tsconfig.json`.** A bare specifier resolves through
  the `paths`/`baseUrl` chain of the closest ancestor `tsconfig.json` of the importing file,
  so an app-level `@/*` inside a workspace resolves instead of falling out external; the
  repo-root `tsconfig.json`/`tsconfig.base.json` chain stays the fallback for files under no
  nested config or under one that declares neither option. A nested `paths` is a
  whole-property override, TypeScript's own rule: it also turns the root's aliases off for
  the files beneath it, so a specifier those files resolved through the root before now
  resolves only if the nested `paths` names it.
- **Chained TypeScript barrels are followed.** A name pulled through up to eight nested
  `index.ts` re-exports (`export * from`, `export { X } from`) resolves to its declaring
  file; a visited set makes a re-export cycle terminate. The barrel-followed `import` edge
  still carries `via`, and a `jsx-use` or `call` edge bound through a two-hop barrel now
  lands on the declaring file instead of resolving to nothing. `refs`/`impact` still fold
  none of the TS edge kinds (README, Limitations). Pinned by `fixtures/ts-resolution/`.
- **The Roslyn oracle emits the flow tracer's fact set.** `tools/scout-semantic --emit
  flowtrace-facts` writes one provider document per solution (`schemaVersion`, `producer`,
  `version`, `repo`, `kind: backend`, compilation identity, git identity, then `facts`) in the
  flow tracer's published fact schema: `message_class`, `consume`, `publish`, `ctor_field`,
  `di_binding`, `iface_impl`, `route` and `method_span`, with message, parameter and
  handler-lambda types resolved by the compilation rather than read off the text -- a primary
  constructor's consumer, a publish of a local variable, and a minimal-API lambda's parameters
  all become facts. Output is sorted and byte-reproducible; every fact is checked against the
  embedded required-field table before it is written, and `--strict` fails the run on any
  site the walk recognised but could not resolve. A package-free fixture solution under
  `fixtures/csharp-flowtrace/` pins the output as a committed snapshot that CI diffs, and
  `tests/flowtrace_facts.rs` pins its shape without a .NET toolchain.
- **A C# construct catalogue with a pinned fixture.** `docs/csharp-coverage.md` enumerates
  every C# construct the extractor meets, with its grammar node kinds, a syntactic verdict,
  an obligation (`must` / `may` / `must-not` produce a fact), the status measured at this
  release, and the fixture that shows it; `fixtures/csharp-syntax/` holds one compilable file
  per construct group, and `tests/csharp_syntax_matrix.rs` fails when a fixture stops parsing
  clean under the pinned grammar, when a producing `must` row stops producing, or when the
  fixture set and the catalogue drift apart. The grammar's three known gaps (C# 14
  `extension` blocks, `allows ref struct`, list-pattern slice designations) are recorded with
  the crate version.

### Changed

- **Graph schema bumped to 2.** `tier`, `member`, and `units` are the new keys; a v1
  graph.json is rebuilt automatically on the next `map` rather than read as-is.
- **`global using` scopes to the project that declared it when a project model exists.**
  Without a model every `global using` is still repo-wide, unchanged.
- **Tier (f) admits an extension class from an enclosing namespace, not just an imported
  one.** `App.Ext` is now visible from `App.Ext.Deep` with no `using` at all, matching C#'s
  own namespace-visibility rule.
- **A same-named ambiguity is narrowed by project reachability.** When a project model
  exists and the ladder finds two same-named defs, candidates the reference site's project
  cannot reach are removed before the precise tiers judge the result: one survivor resolves
  precisely, none behaves like an external name, two or more stay ambiguous with the shorter
  list. Without a model nothing changes.
- **Fragment cache moves to v16.** The next `map` after upgrading reparses every file once;
  the superseded v15 cache files are removed.
- **Typed-receiver lookups walk the receiver's base closure, class bases before any
  interface, in declaration order.** A class-typed receiver never binds to an interface
  declaration at any depth; the extension tier tries every in-graph base, and its raw base
  names, as a lookup key when the receiver's own type misses, in no promised order.
- **Non-public base members are visible to `this.` and `base.` lookups only.** The scored
  tier keeps vouching only through publicly declared members, so a guess never resolves
  through a private one.

### Fixed

- **A call no longer vouches through a property or field.** `entity.Property(x => x.Id)` has
  no overload-resolution path to a property or field of that name, so the scored tier no
  longer lets one stand in as evidence for a call shape it cannot answer.
- **A scored guess under an external receiver must be nominally assignable to it.** A
  candidate the receiver's type could never actually be is now a disproved guess rather than
  a weak one.
- **A scored guess never crosses into a project the reference site cannot reach, or into a
  test project from non-test code.** When a project model exists, both are now structural
  refusals rather than name-only guesses.
- **A `base.` call no longer binds to an interface base.** The base walk skips interface
  bases entirely — an interface declares a contract, not a target.
- **A call whose argument count no overload admits no longer binds to the same-named
  instance member.** It falls through to the extension tier instead, exactly as it would
  for a member the receiver does not declare at all.
- **A receiver typed by a call whose method-return hop fails no longer enters the guess
  pool.** When the hop yields no in-graph receiver type — for a chain tail, or for a local
  assigned from a call or an awaited call — the reference is finished as external instead of
  falling into the extension tier with an unknown receiver or the scored tier's
  name-uniqueness pool.
- **Extension-method generic unification checks the matched base's own type arguments.**
  When the extension tier reaches its lookup key through the receiver's base closure,
  unification runs against that base's declared type arguments rather than the receiver's
  own, so an extension declared on an implemented interface binds for a generic enclosing
  type.
- **Overloads a partial type declares across files all admit their calls.** Method arities
  merge as a union per name across every file the type spans, so `this.Run(1)` with `Run()`
  in one file and `Run(int)` in another binds instead of falling through.
- **A `base.` lookup on a cyclic hierarchy never binds the enclosing type.** The
  base-closure walk starts with its own type already marked as seen.
- **A field or property type declared in another file resolves in that file's context.**
  A base or sibling-partial field typed `Alpha` resolves `Alpha` with the declaring file's
  `using`s and namespace, not the reading file's, so a same-named type visible only from
  the reading file no longer takes the edge.
- **Every catch, query-range and lambda binding shadows the cross-file field fallback.** A
  `catch (T e)` designation records a type fact; a query range variable and a lambda
  parameter the element rule does not type take the name untyped, so a same-named field on
  the enclosing type or a base can no longer type them.
- **A `base.`-qualified chain head hops through the base's method return.**
  `base.Make().Validate()` types the tail from the first in-graph base declaring `Make`,
  never from the enclosing type's own same-named member.
- **The scored tier no longer re-admits an extension the extension tier declined on arity.**
  An extension of the receiver's exact type is still a valid guess when only its namespace
  was not imported at the site, but a call whose argument count the extension cannot take
  has no binding under any import and is refused there too.
- **A receiver written `Foo<X>` binds the generic `Foo<T>`, not whichever of `Foo` and `Foo<T>`
  was indexed first.** The two share one id, so a declared receiver type now resolves with the
  argument count it was written with and each base-list entry with the count its generic-argument
  record carries; the precise tier, the base walks it shares with the extension veto, and the
  veto itself read the sibling the language names. A name with no def at that count anywhere
  keeps the arity-blind answer; a name shared by fewer than two defs costs one lookup as before.
- **A static qualifier walks through nested types before its next segment is read as a
  member.** `Outer.Inner.Leaf.Value`, with or without a namespace prefix on `Outer`, binds
  the qualifier one nested-type segment at a time from the shortest head that names a type
  and emits a single precise edge to `Outer+Inner+Leaf` with `member` `Value`. The shorter
  windows of the same chain no longer emit an edge that names a nested type as if it were a
  member of its container, which is where a namespace-qualified head used to produce a
  precise edge to `Outer` with member `Inner`, and a nested type whose simple name repeats
  across containers now binds to the container the qualifier names instead of dropping out
  as ambiguous.
- **A fully qualified name never falls back to its bare last segment.** A dotted reference
  whose exact-qualified lookup fails matches only a def whose full path (a nested type's `+`
  read as `.`) ends with the text as written, and is external otherwise:
  `RabbitMQ.Client.ExchangeType.Fanout` no longer binds to an in-tree `ExchangeType`,
  `System.Text.Json.JsonSerializer.Serialize(x)` no longer binds to an in-tree
  `JsonSerializer`, and `expr.Member.Name` no longer binds to a nested type named `Member`.
  `Outer.Inner` still reaches `Outer+Inner`, `Box<string>.Slot` and `global::App.Widget` are
  read as the def paths they spell, `Derived.Item` reaches an `Item` declared inside a base of
  `Derived`, and a `using` alias at the head of a dotted name is rewritten to its target and
  looked up exactly.
- **A qualified or generic static qualifier binds the base that declares the member.**
  `Ns.Derived.Create()` and `Derived<int>.Create()` used to bind `Derived` on type certainty
  alone; the named type, then its in-graph base closure, is asked which def declares the
  member first, and the certainty answer is kept only when neither does.
- **An interface-typed receiver binds the base interface that declares the member.**
  `IExtended : IContract`, `ext.Fulfil()` resolves precisely to `IContract` instead of
  emitting nothing; a class-typed receiver still never binds an interface declaration at any
  depth. Pinned by a second fixture solution, `fixtures/csharp-direction`, with its own
  committed oracle snapshot, `expected.json`, and CI oracle diff, whose precise tier scores
  every inheritance-direction shape the compiler decides along an `inherits` edge and names
  its four remaining false positives (see the README's limitations).
- **Declarations and references inside an inactive preprocessor arm are no longer indexed.**
  The C# extractor now evaluates `#if`/`#elif`/`#else`/`#endif` before parsing, with the
  no-build symbol model: no symbol is predefined (`DEBUG` and `TRACE` included), `#define`
  and `#undef` inside the file are honored, and every other symbol is false, so at most one
  arm of every group reaches the parser. A file whose namespace or a member header is
  chosen by a symbol previously yielded every type under a doubled namespace and every ref
  from both arms; it now yields each once, under the arm the compiler keeps. Inactive lines
  are blanked in place, so line numbers and byte offsets of the surviving text are
  unchanged. `#region`, `#pragma`, `#nullable`, and `#line` are left alone, and a
  directive-looking line inside a block comment, a verbatim string, or a raw string literal
  is not a directive. The fragment cache generation moves to v17 so every file reparses
  once on the next `map`.

### Benchmarks

- **Resolver-precision benchmark against a compiler oracle.** `tools/scout-semantic` (a C# console
  project, not part of the crate build) emits one record per member reference from a compiled
  solution; the plumbing verb `devscout audit --semantic <refs.jsonl>` scores `uses-member`
  edges per tier for precision, recall, external-receiver leaks and cross-project impossibility,
  with `--assert` thresholds for CI. `fixtures/csharp-semantic` pins the defect shapes; baseline
  figures on the pinned MassTransit corpus are in `docs/benchmarks/results/2026-09-resolver-precision.md`.
- **Recall and precision on the pinned MassTransit corpus, Run 1 through Run 4.** `this.`
  receivers climb from 0.000 to 0.944 recall and `base.` receivers from 0.000 to 0.978;
  overall recall rises 0.479 → 0.546 (precise-only 0.379 → 0.431, precise+ext
  0.394 → 0.467). Extension-tier precision rises 0.809 → 0.906 on more than double the
  edges (1083 → 2272); precise precision holds near-flat at 0.969 → 0.972 and guess
  precision at 0.502 → 0.512; leaked external sites fall 1098 → 1011. Run 4 misses four of
  its registered recall predictions (`call` 0.060 against ≥ 0.20, `ident` 0.628 against
  ≥ 0.63, all 0.546 against ≥ 0.55, precise+ext 0.467 against ≥ 0.47). Full per-tier
  figures, every prediction with its verdict, and the open enrichment decision are in
  `docs/benchmarks/results/2026-09-resolver-precision.md`.
- **A stale fragment cache silently read this branch's additive tables as empty.** A corpus
  run measured against a fragment cache built by an earlier commit on this branch dropped
  every table it had not yet cached — base closures, arities, field types, and the rest —
  understating both recall and precision; every corpus run now wipes the indexer state
  before mapping.

### Docs

- **Design for the compiler-backed enrichment layer.** `docs/design/compiler-enrichment.md`
  settles, before any code, how a cached, out-of-process consumer of the oracle's per-site
  records would enter `resolve`: the record contract, the `semantic-v1.json` cache and its
  per-file content-hash validity, the `source: "semantic"` edge tag in the slot schema 2
  reserved, how `audit --semantic` keeps the syntax-tier figures comparable, the sidecar
  invocation model, and the gate the layer must clear to ship. No behaviour changes.

## [0.3.0] - 2026-08-27

### Added

- **`read` verb.** Serves a symbol's declaration span plus its inbound callers — with real
  declaration spans for TypeScript as well as C#. The first read of an indexed file offers
  the nearest symbol, including ranged reads, which offer the symbol nearest the requested
  range. References originating inside the target's own declaration span are excluded from
  inbound results and counts.
- **`find` ranking.** Results are ranked by precise inbound reference counts. References
  originating in the same file are excluded from a file's inbound count.

### Changed

- **`init` language census reports three honest tiers**: C# fully supported;
  TS/TSX/JS indexed and graphed with narrower edge coverage; other counted extensions
  present, not indexed. The census no longer understates TypeScript.
- **The content-database default resolves at runtime** from the home directory, matching
  the registry path's resolution, instead of a compile-time manifest path. The
  `SCOUT_CONTENT_DB` override is unchanged.
- Crate-wide rustfmt adoption and a full public-API rustdoc pass
  (`missing_docs = "warn"` at zero warnings). CI enforces formatting from this release on.

### Fixed

- **Bare names no longer bind to nested types the reference site cannot name.** A bare,
  undotted type name reaches a nested type only when the reference site sits inside the
  nesting chain or inherits the enclosing type; other bare references fall through as
  unresolved or external instead of emitting a false precise edge that `impact` then
  widens through (for example, a bare `Claim` under `using System.Security.Claims`
  binding to an unrelated nested test class).
- **Generic arity is matched exactly during resolution.** A type reference resolves only
  to definitions with a matching type-parameter count; arity-overloaded siblings stay
  distinct, and mismatched references fall through as unresolved or external instead of
  binding a wrong same-named definition. The fragment cache generation is bumped, so the
  first run after upgrading re-extracts.

### Benchmarks

- The published scorecard was measured on 0.2.0 and is not re-measured in this release.
  `find` output ordering and reference resolution changed in 0.3.0, so those numbers
  describe 0.2.0 until the next benchmark round.

## [0.2.0] - 2026-08-25

Both entries are fixes for defects the first benchmark round found in this tool and
recorded against itself (`docs/benchmarks/results/2026-08.md`, "Defects this run found
in its own method"). Both change observable output, hence the minor bump.

### Changed

- **`find`: every manifest-pool row now carries a line.** Rows were `path: purpose`,
  with no line, so a follow-up could not open the hit at a position and the row failed
  the benchmark's follow-up-reach rule. They are now `path:line: purpose`, where `line`
  is that file's own first declaration in the name index, falling back to line 1 for a
  file the index carries no declared symbol for at all. The declaration block above it
  is unchanged.

### Fixed

- **`refs --all` now lifts the inbound cap too, not just the outbound one.** `--all`
  lifted `OUTBOUND_CAP` only, so a `refs` answer truncated at `INBOUND_CAP = 30` had no
  flag that could recover it and silently returned fewer referring files than the graph
  held. `--all` now lifts both caps. Output without `--all` is unchanged.

## [0.1.0]

Initial public release: `map`, `find`, `refs`, `impact`, `init`, `stats`, `clear` over a
C# and TypeScript/JavaScript index.
