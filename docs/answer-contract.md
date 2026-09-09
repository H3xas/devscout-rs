# The `--json` answer contract

`refs`, `read`, `impact` and `tests` all answer the same way under `--json`: a single JSON
object, its first key always `schema_version`, one of its keys always `outcome`, and every hit
row inside it carrying a `why`. This document is the contract for those three things. Nothing
here documents the rest of each verb's shape -- that lives in `README.md`'s command table and in
`src/query/json.rs`'s own builder comments, which pin key order field by field.

**Forward compatibility.** A consumer should ignore any key it does not recognise. New keys are
added to these answers over time (`schema_version` and `why` are themselves two such additions);
an existing key is never renamed, removed, or given a different meaning without `schema_version`
changing.

## `schema_version`

`schema_version` is the version of the answer *shape* -- not of the graph (`graph.json` carries
its own, unrelated `GRAPH_SCHEMA_VERSION`), and not of the CLI. It is always the first key in
every object this document covers, so a consumer reading keys positionally sees it before
anything else.

Today it is always `1`. It bumps only when an existing key is renamed, removed, or given a
different meaning; adding a new key -- the ordinary way this contract grows -- never bumps it.

## `outcome`

Every answer names one of four outcomes, defined once in `src/query/outcome.rs` as the `Outcome`
enum so no caller can spell a fifth one into existence:

| `outcome` | Meaning |
| --- | --- |
| `hit` | The seed resolved (a type, a member, or a file) and the answer is the ordinary shape a caller reads. |
| `zero-hit` | The seed resolved but the answer itself is empty: `impact`'s "resolved seed, zero affected rows" case, or `refs`/`read` on a member declared by exactly one type with nothing referencing it -- `read`'s answer there is `refs`' own inbound list, so the two are byte-identical on stdout. Exit code 3; `impact` adds its zero-hit note on stderr, `refs`/`read` leave stderr empty. |
| `ambiguous` | The seed named more than one candidate (a type, or a member across more than one declaring type) and nothing was guessed between them. |
| `fallback-advised` | Nothing in the graph carries the seed at all; the caller's zero-hit note advises a text-search fallback. |

## The `ambiguous` envelope

`refs`, `read`, `impact` and `tests` share one JSON shape for `ambiguous`, whichever of the two
candidate vocabularies below produced it: `schema_version` first, `outcome: "ambiguous"`, the
`query` string, and a `candidates` array, at exit code 1. A type seed's candidates carry `{id,
file, line, kind}` (`ambiguous_candidates_out` in `src/cli.rs`); a member seed's carry `{owner,
name, file, line}` (`member_ambiguous_out`), naming the declaring type and the member instead of
a graph id.

The two examples below are real `--json` runs, each against its own small scratch repo built to
show the shape -- the shipped `fixtures/conformance/csharp/` corpus has no name that resolves
ambiguously.

A type seed (`Widget` declared in two namespaces):

```
devscout refs Widget --json
```

```json
{
  "schema_version": 1,
  "outcome": "ambiguous",
  "query": "Widget",
  "candidates": [
    { "id": "App.Alpha.Widget", "file": "src/A.cs", "line": 3, "kind": "class" },
    { "id": "App.Beta.Widget", "file": "src/B.cs", "line": 3, "kind": "class" }
  ]
}
```

A member seed (`Run` declared on both `Widget` types above):

```
devscout refs Run --json
```

```json
{
  "schema_version": 1,
  "outcome": "ambiguous",
  "query": "Run",
  "candidates": [
    { "owner": "App.Alpha.Widget", "name": "Run", "file": "src/A.cs", "line": 5 },
    { "owner": "App.Beta.Widget", "name": "Run", "file": "src/B.cs", "line": 5 }
  ]
}
```

## `why`

Every hit row -- an inbound/outbound/import row on `refs`/`read`, a row of `impact`'s blast
radius, a row of `tests`' coverage, and `read`'s declaration span -- carries a `why`: which rule
or tier produced it. The vocabulary is defined once, in `src/query/why.rs`, as the `Why` enum, the
same closed-vocabulary discipline `Outcome` uses. `why` is never stamped on an ambiguous or
candidate-count row: those already carry their own `origin`/`candidateCount` fields describing
why they are *unresolved*, a different question from which rule produced a settled hit.

| `why` | Meaning |
| --- | --- |
| `inherits` | A base-list (`inherits`) edge -- a class or interface named in a base list. |
| `uses-type` | A `uses-type` edge -- a type used as a field/property/parameter/return type, a generic argument, and similar. |
| `uses-member-precise` | A `uses-member` edge the resolver proved outright, with no guessing. |
| `uses-member-ext` | A `uses-member` edge found through C#'s own extension-method lookup, which can see the `(member, this-type)` pair but not the receiver's real members. |
| `uses-member-guess` | A `uses-member` edge the scored-guess tier picked because exactly one type in the graph declares a member of that name (or, for a heuristic edge from a graph written before tiers were recorded, an edge that cannot prove which of the two it was). |
| `ctor-di` | A constructor-injection edge: the row was reached through a parameter whose type is injected. |
| `imports` | A C# `using` directive. |
| `declaration` | `read`'s declaration span -- the def's own source, not an edge at all. |
| `test-attribute` | A `tests` row earned because a def declared in the file carries a test-runner attribute (`[Fact]`, `[Test]`, `[TestMethod]`, and their qualified/targeted/shared-bracket forms). |
| `test-project` | A `tests` row earned because the project model places the file's unit inside a project marked `test`, with no attributed def of its own. |
| `imported-edge` | An `impact` row reached only through an imported cross-repo edge -- never one of this graph's own -- carrying the foreign `repo` id and the export's `provenance` id. The weakest evidence this vocabulary names: a foreign, producer-asserted fact never outranks anything this crate resolved for itself. |

**Vocabulary growth.** `why` and `outcome` may each gain a new word over time without a
`schema_version` bump, the same way an added key never bumps it. A consumer must treat a `why`
or `outcome` value it does not recognise as opaque -- skip it rather than fail.

Four edge kinds this crate records never reach `why`: a TS/TSX-only edge (`import`/`call`/
`jsx-use`/`dispatch`) is never admitted into the inbound/outbound adjacency any of these four
verbs read (`query::index`'s `Edge::Import | Edge::Call | Edge::JsxUse | Edge::Dispatch => {}`
arm), so a row built from one -- and a word for it -- never exists.

`why` is derived entirely from what the graph already persists on the edge that produced the row:
its kind, and, for a `uses-member` edge, its `tier` (`ext`, `guess`, or absent, meaning precise).
An `impact` row can fold more than one edge kind into a single per-file summary; when it does,
the strongest evidence wins -- an explicit `ctor-di` site over a plain resolved reference, over
the broader interface hop, over a guess, in that order.

## One worked example per verb

Each example below is a real `--json` run against a fixture in this repository
(`fixtures/conformance/csharp/`, copied into its own throwaway git repo and mapped), reformatted
for readability. `_store`/`store` are the only two callers of `InMemoryCatalogStore.CountItems`;
`CatalogStoreTests` is the only test file; `ICatalogStore` is the interface `InMemoryCatalogStore`
implements.

### `refs`

```
devscout refs CountItems --json
```

```json
{
  "schema_version": 1,
  "status": "members",
  "query": "CountItems",
  "members": [
    {
      "schema_version": 1,
      "status": "resolved",
      "query": "CountItems",
      "id": "Widgetworks.Catalog.InMemoryCatalogStore.CountItems",
      "kind": "member",
      "sites": [{ "file": "InMemoryCatalogStore.cs", "line": 9 }],
      "inbound": {
        "inherits": { "total": 0, "dropped": 0, "rows": [] },
        "uses-type": { "total": 0, "dropped": 0, "rows": [] },
        "uses-member": {
          "total": 2,
          "dropped": 0,
          "rows": [
            {
              "file": "CatalogController.cs",
              "line": 9,
              "source": "return _store.CountItems();",
              "why": "uses-member-precise"
            },
            {
              "file": "CatalogStoreTests.cs",
              "line": 12,
              "source": "Assert.Equal(0, store.CountItems());",
              "why": "uses-member-precise"
            }
          ]
        }
      },
      "ambiguous": {
        "inbound": { "total": 0, "dropped": 0, "rows": [] },
        "outbound": { "total": 0, "dropped": 0, "rows": [] }
      },
      "manifestGap": 0,
      "outcome": "hit"
    }
  ],
  "outcome": "hit"
}
```

`CountItems` is a bare member name, so `refs` answers under the `members` wrapper (one entry per
declaring type -- here, just `InMemoryCatalogStore`); the wrapper and each member both lead with
their own `schema_version`.

### `read`

```
devscout read InMemoryCatalogStore --json
```

```json
{
  "schema_version": 1,
  "status": "resolved",
  "query": "InMemoryCatalogStore",
  "id": "Widgetworks.Catalog.InMemoryCatalogStore",
  "kind": "class",
  "span": {
    "file": "InMemoryCatalogStore.cs",
    "startLine": 5,
    "endLine": 18,
    "source": "public class InMemoryCatalogStore : ICatalogStore\n{\n    private readonly List<string> _items = new();\n\n    public int CountItems()\n    {\n        return _items.Count;\n    }\n\n    public void AddItem(string name)\n    {\n        _items.Add(name);\n    }\n}",
    "why": "declaration"
  },
  "sites": [{ "file": "InMemoryCatalogStore.cs", "line": 5 }],
  "inbound": {
    "inherits": { "total": 0, "dropped": 0, "rows": [] },
    "uses-type": {
      "total": 3,
      "dropped": 0,
      "rows": [
        {
          "file": "CatalogController.cs",
          "line": 5,
          "source": "private readonly InMemoryCatalogStore _store = new InMemoryCatalogStore();",
          "why": "uses-type"
        },
        {
          "file": "CatalogController.cs",
          "line": 5,
          "source": "private readonly InMemoryCatalogStore _store = new InMemoryCatalogStore();",
          "why": "uses-type"
        },
        {
          "file": "CatalogStoreTests.cs",
          "line": 11,
          "source": "var store = new InMemoryCatalogStore();",
          "why": "uses-type"
        }
      ]
    },
    "uses-member": {
      "total": 2,
      "dropped": 0,
      "rows": [
        {
          "file": "CatalogController.cs",
          "line": 9,
          "source": "return _store.CountItems();",
          "why": "uses-member-precise"
        },
        {
          "file": "CatalogStoreTests.cs",
          "line": 12,
          "source": "Assert.Equal(0, store.CountItems());",
          "why": "uses-member-precise"
        }
      ]
    }
  },
  "ambiguous": {
    "inbound": { "total": 0, "dropped": 0, "rows": [] },
    "outbound": { "total": 0, "dropped": 0, "rows": [] }
  },
  "manifestGap": 0,
  "outcome": "hit"
}
```

The declaration span carries `"why": "declaration"` -- it quotes the def's own source, not an
edge -- while the inbound rows below it carry the ordinary edge-derived words.

### `impact`

```
devscout impact ICatalogStore --json
```

```json
{
  "schema_version": 1,
  "query": "ICatalogStore",
  "status": "resolved",
  "kind": "symbol",
  "seedFiles": ["ICatalogStore.cs"],
  "hops": 2,
  "totalAffected": 3,
  "rows": [
    {
      "file": "InMemoryCatalogStore.cs",
      "hop": 1,
      "viaCount": 1,
      "ambiguousCount": 0,
      "topSymbols": ["ICatalogStore"],
      "topSymbolsMore": 0,
      "score": 0.3195320656137759,
      "fromLines": { "direct": 5 },
      "why": "inherits"
    },
    {
      "file": "CatalogController.cs",
      "hop": 2,
      "viaCount": 3,
      "ambiguousCount": 0,
      "topSymbols": ["InMemoryCatalogStore"],
      "topSymbolsMore": 0,
      "score": 0.15227392859677333,
      "fromLines": { "direct": 5 },
      "why": "uses-type"
    },
    {
      "file": "CatalogStoreTests.cs",
      "hop": 2,
      "viaCount": 2,
      "ambiguousCount": 0,
      "topSymbols": ["InMemoryCatalogStore"],
      "topSymbolsMore": 0,
      "score": 0.15227392859677333,
      "fromLines": { "direct": 11 },
      "why": "uses-type"
    }
  ],
  "dropped": 0,
  "manifestGap": 0,
  "heuristicAffected": 0,
  "testsAffected": 1,
  "outcome": "hit"
}
```

`InMemoryCatalogStore.cs` is affected at hop 1 because it declares the base-list edge to
`ICatalogStore` itself (`why: "inherits"`); the other two files are affected at hop 2 because
they use `InMemoryCatalogStore`, the hop-1 file, as a type (`why: "uses-type"`).

### `tests`

```
devscout tests InMemoryCatalogStore --json
```

```json
{
  "schema_version": 1,
  "status": "resolved",
  "query": "InMemoryCatalogStore",
  "symbol": "Widgetworks.Catalog.InMemoryCatalogStore",
  "defFiles": ["InMemoryCatalogStore.cs"],
  "rows": [
    {
      "file": "CatalogStoreTests.cs",
      "testDefs": ["Widgetworks.Catalog.Tests.CatalogStoreTests"],
      "lines": [11, 12],
      "refCount": 2,
      "why": "test-attribute"
    }
  ],
  "testFileCount": 1,
  "refCount": 2,
  "heuristicFileCount": 0,
  "heuristicRefCount": 0,
  "outcome": "hit"
}
```

`CatalogStoreTests` earns its row because it declares `CountItems_StartsAtZero`, a def carrying
xUnit's `[Fact]` attribute -- `why: "test-attribute"`, the other of the two ways a `tests` row
is earned being `test-project` (the file's unit is in a project marked `test`, with no attributed
def of its own).
