# devscout

A fast code index for C# and TypeScript codebases — `map`, `find`, `refs`, `impact` from the CLI.

`devscout` walks a repository once, parses it with [tree-sitter](https://tree-sitter.github.io/)
(C#, TypeScript, TSX, JavaScript), and writes a small set of JSON artifacts next to the repo.
Every later query is answered from those artifacts, so asking "who calls this?" or "what breaks
if I touch this file?" costs a few milliseconds instead of a full-text sweep.

It is built for the case where a person or an agent needs a *name-level* answer — a definition
site, an inbound edge, a blast radius — and grep would return either nothing useful or far too
much.

## What it does

| Command | What you get |
| --- | --- |
| `devscout init [scope ...]` | Register the repo, create the artifact directory, install the agent hooks, run a first map |
| `devscout map [scope ...]` | Build or refresh the index; incremental — unchanged files are reused |
| `devscout find <query>` | Search the manifest by symbol name or by file purpose |
| `devscout refs <symbol>` | Inbound references to a symbol, grouped by edge kind (`inherits`, `uses-type`, `uses-member`, `implements`, `overrides`) |
| `devscout read <symbol>` | The symbol's declaration span and verbatim source plus the same inbound answer as `refs` |
| `devscout impact <file\|symbol>` | Blast radius: the files reachable from a seed within N hops |
| `devscout import-edges <file> --repo <id>` | Load a versioned cross-repo edge export; `impact` then reports files reached only through it (`--no-imports` to skip it) |
| `devscout compiler-facts run\|import\|status` | Optional: acquire or import a versioned compiler-derived fact artifact (see [Compiler facts](#compiler-facts)); `map` and every query never need it and never start it |
| `devscout tests <symbol>` | The test files that reach a symbol |
| `devscout <verb> <symbol> --pick N` | On any of the four verbs above, narrows a member seed with several declaring types to its nth candidate |
| `devscout stats` | Index and cache summary for the current repo |
| `devscout clear` | Drop freshness rows by age or by session |

Plumbing verbs (`parse`, `spans`, `extract-dump`, `hook`, `noop`) exist for debugging and for
the agent-hook integration. `devscout --help` lists everything.

**Languages.** C# (`.cs`) is the complete story: declarations, inheritance, type and member
usage, and preprocessor-aware extraction. TypeScript / TSX / JavaScript (`.ts`, `.tsx`, `.js`,
`.jsx`) are indexed for `find` and file purposes, and are resolved into the graph with a
narrower set of edge kinds — see [Limitations](#limitations). Import specifiers resolve through
the `paths` and `baseUrl` of the nearest `tsconfig.json` above the file (its `extends` chain
included; a nested config that declares neither falls back to the root chain), and through
re-export barrels up to eight hops deep, with the barrel the source names kept as `via` on the
edge.

## Install

From crates.io:

```sh
cargo install devscout-rs
```

The crate is named `devscout-rs`; the binary it installs is `devscout`.

From source, clone this repository and build it:

```sh
cargo build --release
# binary at ./target/release/devscout
```

Pre-built binaries for Linux, macOS, and Windows are attached to each
[tagged release](#versioning-and-releases).

## Quickstart

Any repository with C# or TypeScript in it will do. Using this repo's own fixtures as a
throwaway example:

```sh
mkdir -p /tmp/demo/src && cd /tmp/demo
cp <path-to-this-repo>/fixtures/ts-grammar/* src/
cp <path-to-this-repo>/fixtures/csharp-demo/src/* src/

devscout init
```

`init` registers the root, reports what it found, offers the agent hooks, and runs a first map:

```
devscout initialized at /private/tmp/demo/.scout (non-git root: /private/tmp/demo)
languages: 3 .cs (fully supported); 1 .js, 9 .ts, 1 .tsx (indexed and graphed, narrower edge coverage)
hooks: installed (Read, Bash); backup /tmp/devscout-home/.claude/settings.json.bak.20260824-074758-728335000
map: mapped 14 files under . (preserved 0 agent purposes, downgraded 0 changed, 14 new, 0 removed, 14 ast signatures); graph rebuilt in 0.01s (24 defs, 4 edges)
```

(macOS resolves `/tmp` to `/private/tmp`; the backup path reflects whatever `$HOME` the hooks
install ran against.)

Then query it:

```
$ devscout find Article
src/ArticleCard.tsx:1: function ArticleCard | interface ArticleCardProps
src/articleTypes.ts:1: interface ArticleItem | interface ArticleAuthor | type ArticleItemStatus | type ArticlePage

$ devscout refs IOrderRepository
Shop.Data.IOrderRepository  (interface)
def: src/IOrderRepository.cs:3
inbound:
  inherits (1):
    src/OrderRepository.cs:3  inherits  public class OrderRepository : IOrderRepository
  uses-type (0):
  uses-member (0):

$ devscout impact src/OrderRepository.cs --hops 2
impact: src/OrderRepository.cs  (file, seed files: src/OrderRepository.cs)  hops<=2
affected files: 1  shown: 1  dropped: 0
file  hops  via  top-symbols
src/OrderController.cs  1  3  OrderRepository
```

`refs`, `impact`, and `tests` also take `--json` (machine-readable) or `--compact` (one line per
hit, for piping). A zero-hit query is reported as a zero hit, not as an error — the tool never
guesses a different symbol on your behalf, and an ambiguous name prints every candidate instead
of picking one.

### Member seeds

`refs`, `read`, `impact` and `tests` all accept a bare member name (`Show`), a
`Type.Member` spelling (`Widget.Show`), or a fully-qualified
`Namespace.Type.Member` spelling (`App.Widgets.Widget.Show`) as a seed — the
type-resolution ladder runs first, so a name that resolves to a type still
answers as that type, and the member reading is only ever a fallback. A
member seed naming exactly one declaring type answers as that member;
`impact` and `tests` answer as the member's declaring type (they have no
member-shaped answer of their own), while `refs` and `read` answer with the
member's own inbound references.

A member seed carried by more than one type lists one row per candidate —
its declaring type, file, and line — rather than guessing between them or
printing a bare list of types. Pass `--pick N` (one-based) to select the nth
row from that list; an out-of-range `N` is a usage error (exit code 2).

Every `--json` answer on these four verbs leads with a top-level `schema_version` and carries a
top-level `outcome`: `hit`, `zero-hit`, `ambiguous`, or `fallback-advised` (nothing in the graph
carries the seed at all, and the zero-hit note on stderr advises a text-search fallback instead).
`zero-hit` means the seed resolved and the answer is empty: `impact`'s empty blast radius, or a
`refs`/`read` member declared by exactly one type with nothing referencing it; `tests` with no
rows still answers `hit`. Every hit row also carries a `why` naming the rule or tier that produced
it. See [`docs/answer-contract.md`](docs/answer-contract.md) for the full contract, the `why`
vocabulary, and a worked example per verb.

Re-run `devscout map .` after edits; it re-parses only what changed and leaves the graph alone
when nothing moved (`... 0 new, 0 removed ...; graph unchanged`). If the index falls behind
`HEAD`, queries print a staleness warning on stderr rather than silently answering from stale
data.

### Find ranking

`find` ranks matching files by tokens matched, then by their precise inbound-reference count.
References originating in the same file are excluded from a file's inbound count.

## Where it stores things

Inside a git repository, artifacts live under the git common directory, so they never show up as
untracked files and are shared correctly by worktrees:

```
<git-common-dir>/scout/manifest.json              file -> purpose + symbol index
<git-common-dir>/scout/index-state.json           HEAD + timestamp the index was built at
<git-common-dir>/scout/graph/graph.json           definitions, edges, and project units
<git-common-dir>/scout/graph/fragments-v19.json   per-file extraction cache (incremental map)
<git-common-dir>/scout/graph/project-units.json   csproj staleness sidecar (present only with a project model)
<git-common-dir>/scout/graph/compiler-facts-v1.json  optional versioned compiler-fact artifact (see Compiler facts below); absent unless `compiler-facts run|import` has admitted one
<git-common-dir>/scout/graph/semantic-v1.json      planned: compiler-backed enrichment cache (see docs/design/compiler-enrichment.md)
<git-common-dir>/scout/log/queries.jsonl          query-verb telemetry, one JSON line per answered invocation (opt-in; SCOUT_TELEMETRY=1)
```

Outside a git repository the same tree is written to `<root>/.scout/` instead. `devscout init`
also adds `.scout` to the repository's local exclude file so the legacy location cannot be
committed by accident.

Two stores live outside the repo:

- **Registry** — `$HOME/.claude/scout/repos.json`, the list of roots `devscout` knows about.
- **Read-freshness cache** — `<root>/.scout/cache.db` plus a content-addressed `content.db`,
  both SQLite. These are only written by the agent hooks (`devscout hook read|bash`); plain CLI
  use does not touch them.

## Environment variables

| Variable | Effect |
| --- | --- |
| `SCOUT_REGISTRY` | Path to the registry JSON. Default `$HOME/.claude/scout/repos.json`. |
| `SCOUT_CONTENT_DB` | Path to the shared content-dedup SQLite database. Default `$HOME/.claude/scout/content.db`. |
| `SCOUT_MTIME_REUSE` | `1` switches `map` from content-hash fragment reuse back to mtime-based reuse. |
| `SCOUT_DEBUG` | `1` turns on hook debug output. Equivalent to creating a `.scout/debug` file. |
| `SCOUT_TELEMETRY` | Opt-in query telemetry. Export `1` in the shell that runs `find`/`refs`/`read`/`impact`/`tests` to append one JSON line per answered invocation to `scout/log/queries.jsonl`; a usage error or a seed with no resolved repository or graph logs nothing. Unset (or any other value) writes nothing. The agent hooks never run these verbs, so `devscout init` does not set this variable for them. |
| `SCOUT_COMPILER_ENGINE` | Path to a built compiler-facts engine executable. Read only by `compiler-facts run`; no default and nothing is downloaded. Unset (or empty) refuses with one line and touches nothing. |
| `HOME` | Used to locate the registry, content database, and agent settings file. |

## Compiler facts

`devscout compiler-facts run|import|status` is entirely optional: `map` and every query verb
answer from source-level extraction alone and never spawn a compiler or touch the network. When a
compiler-derived artifact has been admitted, a future consumer can layer compiler-checked facts on
top of that same syntax-only coverage; today this verb group only acquires, validates and reports
that artifact.

- `run` launches a one-shot engine located by `SCOUT_COMPILER_ENGINE` (no default, nothing
  downloaded), captures its output under a wall-clock timeout and a byte cap, and admits it.
- `import <file>` admits a build- or CI-produced artifact through the identical admission path
  `run` uses, so a locally acquired and an externally produced artifact reach the same accept or
  refuse decision for the same bytes.
- `status` is read-only and reports `coverage: syntax-only` when no artifact has ever been
  admitted, or the admitted artifact's own coverage state otherwise.

Every check runs before a single byte is published: a mismatched engine revision, contract
version, requested profile, dependency fingerprint, compilation-context version or fingerprint, or
source-snapshot identity is refused with a stable reason and writes nothing; a killed, timed-out,
over-budget, truncated, malformed, or internally incoherent run leaves the previously admitted
artifact byte-identical. A structurally valid artifact that declares incomplete coverage is still
admitted, together with its per-unit diagnostics, and is never reported as clean or complete.
Publication is atomic — a validate-then-rename through the same same-directory temp file scheme
every other artifact in this crate already uses.

## Reading a symbol

`devscout read <symbol>` returns the indexed declaration's start and end lines,
the verbatim source in that span, and its inbound references. `<symbol>` is a
type name or a member seed (see [Member seeds](#member-seeds) — a bare name,
`Type.Member`, or `Namespace.Type.Member`); a member seed answers with its own
declaration line and inbound references, carrying no span (nothing records an
end line for a member on its own). Use `--compact` for a line-oriented summary
or `--json` for structured output. References that originate inside the
target declaration itself are excluded from inbound rows and counts, so
recursive and other self-references do not look like external callers.

On the first agent-hook read of an indexed code file, devscout offers the
nearest mapped symbol. A ranged read chooses the declaration nearest to the
requested range; a full-file read chooses the first mapped declaration. The
offer does not replace or truncate the file content, and non-code, unmapped,
stale, or already-offered reads do not produce another offer.

## Agent hooks

`devscout init` merges two `PostToolUse` entries into the agent settings file at
`$HOME/.claude/settings.json`, backing up the existing file first: `devscout hook read` and
`devscout hook bash`. They read a tool result on stdin and, when the same content has already
been read in the session, replace the payload with a one-line marker instead of repeating it.
Skip this with `devscout init --no-hooks`; the hook install is independent of the index, and a
failure there never fails `init`.

## Limitations

Known, rather than hidden:

- **TypeScript reference queries fold none of the TS edge kinds.** The graph carries `import`,
  `call`, `jsx-use` and `dispatch` edges for TS/TSX files, but `refs` and `impact` read only the
  C#-shaped `uses-type`/`uses-member` index, so those verbs answer for TypeScript defs without the
  page-to-component and caller-to-callee rows that `graph.json` holds.
- **Generic-delegate `typeParams` divergence is under review.** Type-parameter handling for
  generic delegate declarations does not yet agree with the rest of the generic ladder; the
  affected shapes are under review rather than pinned.
- **The content store follows `HOME` by default.** Set `SCOUT_CONTENT_DB` to keep it at a fixed
  path when `HOME` varies between invocations.
- **No watch mode.** `map` is fast and incremental, but you run it; nothing watches the
  filesystem for you.
- **Ordering is load-bearing but not a stability promise.** Artifact ordering is fixed and
  deterministic by design; do not rely on it staying byte-identical across minor versions.
- **Heuristic `uses-member` edges carry a `tier`: `ext` or `guess`.** `ext` is C#'s own
  extension-method lookup — an exact `(member, this-type)` bucket, matched arity, generic
  unification, and namespace visibility including enclosing namespaces, vetoed the moment an
  in-graph receiver already declares the member — whose one unverifiable case is a receiver
  outside the graph that itself declares the member. `guess` is a name match among the defs
  that declare the member, bounded by a uniqueness cap, the call-shape rule (a call is never
  vouched by a property or field), and the receiver-assignability rule (an external receiver's
  guess must be nominally assignable to it). Neither tier's edges become the premise of a
  further `impact` hop; `--no-guess` drops the `guess` tier from `refs`/`read`/`impact`/`tests`
  while keeping `ext`; compact output marks the two `x`/`h`. Precise edges carry neither
  `heuristic` nor `tier`. Receiver typing covers `this.` and `base.` qualifiers, `?.`
  bindings, a local's `await`ed initializer, cast- and pattern-designated locals, typed
  `out` parameters, and a one-hop call-chain tail. A bare unqualified call, a chain more
  than one hop deep or through `?.`, a lambda parameter neither lambda rule below types, a
  receiver typed only by inference the syntax itself does not show, a `dynamic` receiver, and
  a `using static` import stay unrecorded, so those shapes resolve through the untyped
  name-only tiers or not at all. The collection-element rule reads the receiver's shape, not
  its meaning: a call's first single-parameter lambda takes the element type of any array or
  single-type-argument generic receiver (`Task<T>`, `Lazy<T>`) the way `List<T>` gives it.
  A lambda handed straight to an in-graph method or extension method has each of its
  parameters typed positionally from that method's delegate parameter (`Action<T, ..>`,
  `Func<T, ..>`, `Predicate<T>`, `Expression<>` of those, or a declared `delegate`) when
  every overload that can take it agrees; a callee outside the graph, a generic delegate
  parameter, a callee reached through a chain or through another untyped lambda parameter, a
  named argument, and a parameter name that two lambdas in one member bind to different
  callees leave the parameter untyped.
- **A two-type-argument DI service registration records an `implements` edge.** An invocation
  whose method name begins `Add` or `TryAdd` and ends `Singleton`, `Scoped` or `Transient` and
  carries exactly two type arguments (`services.AddScoped<IContract, Widget>()`) — the keyed
  and named spellings included, since the rule is a prefix/suffix shape rather than a fixed
  name list — records a type-level `implements` edge from the implementation to the service
  type, plus a member-level `implements`/`overrides` edge per implementing/`override` member
  matched by name and arity; an ambiguous arity match (two or more candidates) emits nothing.
  This is what lets `refs`/`read` on a service interface list its implementations and `impact`
  on an implementation reach the interface's own callers through the same widened interface
  hop a base-list `inherits` edge already uses. `--no-dispatch` drops both edge kinds from
  `refs`/`read`/`impact`/`tests`, the same way `--no-guess` drops the `guess` tier — neither
  edge kind is ever itself a guess.
- **The project model reads only `.csproj` and `Directory.Build.props`.** It hand-scans
  `ProjectReference`, `Microsoft.NET.Test.Sdk`, and `IsTestProject` — no MSBuild evaluation, no
  conditions, no NuGet resolution, and no `.sln`. A file belongs to the nearest ancestor
  directory holding exactly one `.csproj`; a directory holding two or more is left unmapped.
  When a model exists, a guess never names a def in a project the reference site's project
  cannot reach, nor a def in a test project from a non-test site, and a `global using` scopes
  to the project that declared it. Without any `.csproj` files, nothing changes.
- **Graph schema 2 adds `member`, `tier`, `units`, and `stats.heuristic_by_tier`.** `member`
  is written on every `uses-member` edge, `tier` on the heuristic ones; `units` (the discovered
  `.csproj` projects) is appended last. A reserved `source` slot is set aside for a future
  semantic-provenance tag. A v1 graph.json is rebuilt automatically on the next `map`. Schema 3
  adds the `implements`/`overrides` edge kinds and their `edges_by_kind` counters; a v2
  graph.json is rebuilt the same way a v1 one is.
- **A precise `uses-member` edge binds the type that declares the member, as far as names
  and arity can tell.** The declaring type in the receiver's static chain — inherited,
  overridden and hidden members, interface members through interface, implementing-class
  and base-interface receivers, static members through bare, qualified and generic derived
  type names — is what the edge targets. Four shapes are decided by information the graph
  does not carry and bind the wrong side of an `inherits` edge: same-arity overloads split
  across base and derived by parameter type; an `internal new` member hiding a public base
  member (only `public` counts as visible to a receiver other than `this`, so an `internal
  override` is a silent miss instead); a `private new` field shadowing a public base
  property when read from outside its type; and `this.M()` in a class that both inherits a
  public `M` and explicitly implements an interface's `M`. Each is pinned as a known false
  positive in `fixtures/csharp-direction`.
- **A written type-argument count picks between same-named generic siblings.** A receiver
  written `Foo<X>` and a base written `Foo` each bind the declaration with that many type
  parameters, not whichever of `Foo` and `Foo<T>` the index met first; a name with no
  declaration at that count keeps the count-blind answer. A name whose exact count is declared
  only outside the site's imports now answers through the same global-uniqueness step a type
  reference already uses, so a few such sites resolve elsewhere or turn external instead of
  binding the wrong count. Two shapes still read as no type arguments: a receiver typed
  from a `foreach` variable or a lambda parameter, whose element type is recorded without its
  arguments, and the bare `IFoo` of `class X : IFoo, IFoo<int>`, whose base list keeps one
  entry per name and carries the generic one.
- **A fully qualified name never falls back to its bare last segment.** A dotted reference
  whose exact-qualified lookup fails matches only a def whose full path (a nested type's `+`
  read as `.`) ends with the text as written, and is external otherwise:
  `RabbitMQ.Client.ExchangeType.Fanout` does not bind an in-tree `ExchangeType`,
  `System.Text.Json.JsonSerializer.Serialize(x)` does not bind an in-tree `JsonSerializer`,
  and `expr.Member.Name` does not bind a nested type named `Member`. `Outer.Inner` still
  reaches `Outer+Inner`, `Box<string>.Slot` and `global::App.Widget` are read as the def
  paths they spell, `Derived.Item` reaches an `Item` declared inside a base of `Derived`, and
  a `using` alias at the head of a dotted name is rewritten to its target and looked up
  exactly. A text that several def paths end with stays ambiguous. The scored `guess` tier is
  unchanged, so a foreign dotted qualifier can still carry a tagged heuristic edge.
- **A static qualifier walks through nested types before its next segment reads as a member.**
  `Outer.Inner.Leaf.Value`, with or without a namespace prefix on `Outer`, binds one
  nested-type segment at a time from the shortest head that names a type, and emits a single
  precise edge to `Outer+Inner+Leaf` with `member` `Value`. The shorter windows of the same
  chain no longer emit an edge that names a nested type as if it were a member of its
  container, which is where a namespace-qualified head used to bind `Outer` with member
  `Inner`, and a nested type whose simple name repeats across containers now binds the
  container the qualifier names instead of dropping out as ambiguous. Both the walk and that
  suppression stand aside for a qualifier the extractor already typed as an instance receiver,
  whose name merely coincides with a type's. A chain with no segment after the nested type,
  such as `nameof(Outer.Inner)`, emits no `uses-member` edge at all, and a generic nested type
  is walked by its arity-less name.
- **Conditional compilation uses the no-build symbol model.** `#if`/`#elif`/`#else`/`#endif`
  are evaluated before parsing with no symbol predefined — not `DEBUG`, not `TRACE`, not a
  target-framework symbol — so `#if SYMBOL` is inactive, `#else` and `#if !SYMBOL` are active,
  and at most one arm of every group is indexed. `#define`/`#undef` inside the file are
  honored; `DefineConstants` from a `.csproj` is not read. A condition combines symbols and the
  `true`/`false` literals with `!`, `==`, `!=`, `&&`, `||` and parentheses, in that precedence
  order. A condition that does not parse is inactive, an `#elif`/`#else`/`#endif` with no group
  open is ignored, and an unclosed `#if` runs to the end of the file. Inactive lines are blanked
  in place (line numbers and offsets do not move),
  `#region`/`#pragma`/`#nullable`/`#line`/`#error`/`#warning` are left to the parser, and a
  directive-looking line inside a block comment, a verbatim string, or a raw string literal is
  not treated as a directive. The `parse` and `spans` diagnostics show the raw tree, both arms
  included.
- **Which C# constructs produce facts is catalogued, not implied.**
  [`docs/csharp-coverage.md`](docs/csharp-coverage.md) lists every construct the extractor
  meets with a syntactic verdict, an obligation (`must`, `may`, `must-not` produce a fact) and
  the measured status,
  pinned by `fixtures/csharp-syntax/` and `tests/csharp_syntax_matrix.rs`; the rows that are
  silent today are listed there as follow-ups rather than discovered by the next corpus.

`devscout` began as the Rust half of a two-implementation tool, and a number of source comments
still describe behaviour by reference to that original implementation. Those notes are history:
this crate generates and reads its own artifacts, and interoperating with anything else is
optional.

**Fixtures.** Fixture sources carry no narrative headers; case notes live in the fixture
directory's own `README.md`.

## Benchmarks

Every claim this project makes about speed, cost, or accuracy lives in
[`docs/benchmarks/`](docs/benchmarks/README.md), with the command that produced it and the
corpus SHA it ran against. The methodology, the peer tools an agent could install instead, the
agentic-lane protocol, and the dated result documents are separate files there, and the harness
is in [`bench/`](bench/README.md).

**Scorecard** (devscout 0.2.0 vs the `rg` baseline, MassTransit corpus; full numbers, per-cell
commands, and the preliminary-run caveats are in
[`docs/benchmarks/results/2026-08.md`](docs/benchmarks/results/2026-08.md)):

| Kind | devscout | rg | Verdict |
| --- | --- | --- | --- |
| Locate | 2/2 correct | 2/2 correct, ~2x faster | Tie — use rg |
| References | 2/2 correct (needs `--all`) | 2/2 correct | Tie, cost mixed |
| Impact | 2/2 partial, better precision, 1 call | 2/2 partial, 4-call chain | devscout wins |
| End-to-end retrieval | 1/2 correct | 2/2 correct | rg wins |
| Agentic, Opus (preliminary) | 4/4 correct, median 180k tokens | 3/4 correct + 1 partial, median 199k tokens | No correctness edge; ~25k-token saving only |

These numbers were measured on 0.2.0. Release 0.3.0 changes `find` output ordering and
reference resolution (exact generic arity), and has not been re-benchmarked; treat the
scorecard as 0.2.0-specific until the next round. Release 0.4.0 changes resolver output again
(heuristic tiers and recall), measured in
[`docs/benchmarks/results/2026-09-resolver-precision.md`](docs/benchmarks/results/2026-09-resolver-precision.md).
Release 0.5.0 changes no resolver output: it produces a byte-identical `graph.json` on the
pinned corpus, so those figures carry over unchanged. Release 0.6.0 raises the graph schema to
3 and adds the `implements` and `overrides` edges, so its `graph.json` is not byte-identical to
0.5.0's and the scorecard has not been re-measured against it; a repository whose code registers
nothing through dependency injection gains no edges and answers as it did.

A separate scripted-lane run measured **tool calls issued per task**: the index arm used fewer
calls in all four query kinds, largest on references (5.0 vs 11.8 per lane, ~2.4x) — single-run
proxy, details under "Tool-call proxy" in the dated results.

Gaps are published in both directions. `devscout` answers name-level and reachability questions
from a prebuilt graph; questions that reduce to finding one distinctive string are answered
well and cheaply by a skilled agent holding `rg`, with no index at all. Where a peer or the
plain `rg` baseline leads, the results say so. The dated results in
[`docs/benchmarks/results/2026-08.md`](docs/benchmarks/results/2026-08.md) now include the first
agentic (model-in-the-loop) round — preliminary, one run per cell, its integrity caveats leading.

The resolver-precision numbers come from a Roslyn oracle, [`tools/scout-semantic`](tools/scout-semantic/README.md),
that compiles a solution and records every member reference with its resolved target. The same
oracle has a second output mode, `--emit flowtrace-facts`, that writes the flow tracer's
per-repository fact set with compilation-resolved types where a text pass runs out (primary
constructors, locals, minimal-API lambdas). This cut emits eight fact kinds -- `message_class`,
`consume`, `publish`, `ctor_field`, `di_binding`, `iface_impl`, `route` and `method_span` --
under a header that names the producer, its version and the compilation; the facts carry no
provenance of their own, and a recognised site whose type does not resolve is counted on stderr
and fails the run under `--strict`. The fixture under `fixtures/csharp-flowtrace/` pins that
output byte-for-byte in CI. Roslyn stays in the sidecar -- the `devscout` binary never links it.

The plumbing verb `devscout audit --semantic <refs.jsonl> [--units F] [--defs F] [--json]
[--assert F]` scores an indexed repository's `uses-member` edges against those oracle records:
precision per tier, recall over in-graph member sites, external-receiver leaks, structurally
impossible edges, and fan-out. `--assert` reads a thresholds file and exits 1 on any
violated or missing key, reporting every one.

Two fixture solutions carry an oracle snapshot and a thresholds file of their own:
`fixtures/csharp-semantic/` holds the per-tier precision and recall numbers, and
`fixtures/csharp-direction/` holds the base-and-interface direction shapes, including the four
known false positives named under Limitations. CI regenerates each snapshot from the oracle,
diffs it against the committed one, then indexes an isolated copy of the fixture and asserts its
thresholds.

## Versioning and releases

Semantic versioning. Releases are cut by pushing a `v*` tag (`v0.1.0`, `v0.2.0`, …), which builds
and attaches binaries for Linux, macOS, and Windows. While the version is `0.x`, minor bumps may
change artifact layout — delete the artifact directory and re-run `devscout map` after upgrading.
Each release binary is keylessly signed (Sigstore/cosign) and carries a GitHub build-provenance
attestation, alongside its `.sha256` checksum; a CycloneDX SBOM covering the full dependency
graph is attached to the release too. See [RELEASING.md](RELEASING.md) for the maintainer-side
process and how to verify a downloaded binary's signature and provenance.

**Provenance.** devscout is developed alongside a private reference implementation of the same
graph contract; every release is additionally gated on behavioral parity against it, and the
committed test fixtures pin that contract byte-for-byte in this repository. You never need the
reference implementation — everything required to build, test, and verify devscout is here,
including a public conformance suite (`cargo test`, see
[CONTRIBUTING.md](CONTRIBUTING.md#reaching-release-gate-confidence-locally)) that exercises the
same command surface with fixtures that ship in this repository.

## Contributing

Bug reports, feature requests, and pull requests are welcome. See
[CONTRIBUTING.md](CONTRIBUTING.md) for how to build, test, and submit changes;
[ARCHITECTURE.md](ARCHITECTURE.md) for what each module under `src/` owns, the
invariants it holds, and where a change goes; [GOVERNANCE.md](GOVERNANCE.md) and
[MAINTAINERS.md](MAINTAINERS.md) for how the project is run;
[SECURITY.md](SECURITY.md) for private vulnerability reporting; and
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) for community expectations.

Every pull request is checked for formatting, for an architecture guide that still names each
module, and against a size and complexity ratchet: 800 lines per file, 100 lines per function
and a cognitive complexity of 25, with a fixed list of existing exceptions that only shrinks.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
this crate by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without
any additional terms or conditions.
