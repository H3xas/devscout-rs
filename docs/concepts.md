# Concepts

The ideas the rest of the documentation assumes. Terms in bold are defined in the
[Glossary](#glossary).

## The index

`devscout` walks a repository once, parses it with [tree-sitter](https://tree-sitter.github.io/)
(C#, TypeScript, TSX, JavaScript, Rust), and writes a small set of JSON artifacts next to the
repo. Every later query is answered from those artifacts instead of a full-text sweep.

It is built for the case where a person or an agent needs a *name-level* answer — a definition
site, an inbound edge, a blast radius — and grep would return either nothing useful or far too
much. Where the artifacts live is in [Storage](reference/storage.md).

## Edges

The graph holds definitions and the **edges** between them. Each edge has an **edge kind**. On
C#, `refs` groups inbound references by kind: `inherits`, `uses-type`, `uses-member`, `implements`
and `overrides`, plus `bus-hop` rows for a handler or for a type whose file publishes a message:
possible routes from a recognized publish or dispatch call to a matching handler, runtime routing
unverified.

C# (`.cs`) is the complete story: declarations, inheritance, type and member usage, and
preprocessor-aware extraction. TypeScript / TSX / JavaScript (`.ts`, `.tsx`, `.mts`, `.cts`,
`.js`, `.jsx`, `.mjs`, `.cjs`) are indexed for `find` and file purposes, and are resolved into
`import`, `call`, `jsx-use` and `dispatch` edges, which `refs`, `read`, `impact` and `tests` read as
their own tables; `tests` also counts a `.ts`/`.tsx`/`.js`/`.mjs`/`.cjs` file named `*.spec.*`,
`*.test.*` or placed below `__tests__/` as a test file.
JavaScript files answer through the same edges but that surface is not measured. Rust edges come
from path resolution. How each
language's references become edges is in [How names resolve](explanation/resolution.md).

## Tiers

A `uses-member` edge is either **precise** or heuristic. A heuristic edge carries a **tier**:
**ext** for C#'s own extension-method lookup, or **guess** for a name match among the definitions
that declare the member. Neither tier's edges become the premise of a further `impact` hop, and
`--no-guess` drops the `guess` tier. With an admitted compiler-fact artifact, edges a compiler
confirmed carry the `semantic` or `semantic-discovered` source instead; see
[Compiler facts](guides/compiler-facts.md). The rules behind each tier are in
[Heuristic tiers](explanation/resolution.md#heuristic-tiers).

## Seeds

A **seed** is what a query starts from: a symbol name or, for `impact`, a file. `refs`, `read`,
`impact` and `tests` all accept a bare member name (`Show`), a `Type.Member` spelling
(`Widget.Show`), or a fully-qualified `Namespace.Type.Member` spelling (`App.Widgets.Widget.Show`)
as a seed — the type-resolution ladder runs first, so a name that resolves to a type still answers
as that type, and the member reading is only ever a fallback. A **member seed** naming exactly one
declaring type answers as that member; `impact` and `tests` answer as the member's declaring type
(they have no member-shaped answer of their own), while `refs` and `read` answer with the member's
own inbound references.

A member seed carried by more than one type lists one row per **candidate** — its declaring type,
file, and line — rather than guessing between them or printing a bare list of types. Pass
`--pick N` (one-based) to select the nth row from that list; an out-of-range `N` is a usage error
(exit code 2).

A Rust item is named by its name, by its id (`src/shelf.rs#Widget::new`), or by a `::` path such
as `Widget::new` or `cabinet::Hinge::oil`; a path two items share within a file (two trait impls
that both define `fmt`, say) is ambiguous, and the second one's id carries a `-1` suffix
(`src/shelf.rs#Gadget::fmt-1`).

## Outcomes

Every `--json` answer on these four verbs leads with a top-level `schema_version` and carries a
top-level **outcome**: `hit`, `zero-hit`, `ambiguous`, or `fallback-advised` (nothing in the graph
carries the seed at all, and the zero-hit note on stderr advises a text-search fallback instead).
`zero-hit` means the seed resolved and the answer is empty: `impact`'s empty blast radius, or a
`refs`/`read` member declared by exactly one type with nothing referencing it; `tests` with no rows
still answers `hit`. Every hit row also carries a `why` naming the rule or tier that produced it.
See [JSON output](reference/json-output.md) for the full contract, the `why` vocabulary, and a
worked example per verb.

A zero-hit query is reported as a zero hit, not as an error — the tool never guesses a different
symbol on your behalf, and an ambiguous name prints every candidate instead of picking one.

## Freshness

Re-run `devscout map .` after edits; it re-parses only what changed and leaves the graph alone when
nothing moved. If the index falls behind `HEAD`, queries print a staleness warning on stderr rather
than silently answering from stale data. There is no watch mode: `map` is fast and incremental, but
you run it; nothing watches the filesystem for you.

**Freshness** also names two narrower records: the agent hooks' read-freshness cache, which
remembers what a session has already read (see [Agent hooks](guides/agent-hooks.md)), and the
`freshness` key of a `--json` answer (see [JSON output](reference/json-output.md#freshness)).

## Find ranking

`find` ranks matching files by tokens matched, then by their precise inbound-reference count.
References originating in the same file are excluded from a file's inbound count.

## Lanes

A graph is built in one of two **lanes**. The syntax lane uses source-level extraction alone; it is
the default and needs nothing else. The enriched lane also consumes an admitted compiler-fact
artifact. `map` prints a `lane:` line whenever an artifact is present or `--no-semantic` is given,
and records the same answer as `stats.lane` in `graph.json`; see
[The lane line](guides/compiler-facts.md#the-lane-line).

## Glossary

| Term | Meaning |
| --- | --- |
| edge | A recorded reference from one definition or file to another in the graph. |
| edge kind | What an edge records: `inherits`, `uses-type`, `uses-member`, `implements`, `overrides` and `bus-hop` on C#; `import`, `call`, `jsx-use` and `dispatch` on TypeScript. |
| tier | How a heuristic `uses-member` edge was found: `ext` or `guess`. A precise edge carries no tier. |
| precise | A `uses-member` edge with neither `heuristic` nor `tier`: the resolver bound the declaring type as far as names and arity can tell. |
| ext | The tier of an edge found by C#'s own extension-method lookup. |
| guess | The tier of an edge found by a bounded name match among the definitions that declare the member; `--no-guess` drops it. |
| candidate | One of several definitions a name could mean. An ambiguous answer lists every candidate instead of choosing one. |
| seed | The symbol or file a query starts from. |
| member seed | A seed that names a member: `Show`, `Widget.Show` or `App.Widgets.Widget.Show`. |
| outcome | The answer's kind: `hit`, `zero-hit`, `ambiguous` or `fallback-advised`. |
| freshness | Whether the index is current with `HEAD`; a query warns on stderr when it has fallen behind. |
| lane | Which graph a `map` built: syntax-only, or enriched by an admitted compiler-fact artifact. |
