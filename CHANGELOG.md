# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
