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

## `next`

Every resolved answer -- a zero-hit one included -- carries a `next` key: the follow-up verbs worth
running, as an array of argv arrays without the binary name. It sits immediately before `outcome`,
and it is `[]` when the answer leaves nothing to suggest.

For `read InMemoryCatalogStore`, that key holds
`[["tests", "Widgetworks.Catalog.InMemoryCatalogStore"], ["impact", "InMemoryCatalogStore.cs"]]`.

| Verb | What it suggests |
| --- | --- |
| `refs`, `read` | `tests <id>`, then `impact <the file declaring it>` |
| `tests` | `impact <the file declaring the symbol>` |
| `impact` | `refs <the first top symbol of the highest-ranked row>` |
| `find` | `read <the def id of the first declaration row>` -- text only, as `find` has no `--json` |

For `find`, the suggestion comes from the first row that declares something `read` opens: a markup
name or a resource key printed ahead of the code is stepped over rather than standing in for the
declaration behind it.

The suggestions are derived from the answer already computed: no second query runs to produce them.
Each candidate is then resolved with the resolver the *suggested* verb itself uses, and a candidate
that would not resolve to exactly one target is dropped rather than printed. Any argv in `next` can
therefore be run as it stands -- it exits `0`, or `3` with `outcome: "zero-hit"` when the symbol it
names turns out to resolve to nothing. Nothing here can exit `1` or `2`.

`next` is present on `hit` and `zero-hit` answers, on the wrapper object and on every member object
of a `members` answer, on the `sections` wrapper and on every section answer under it, and it is
absent from the `ambiguous` envelope, from the `fallback-advised` answer and from `freshness`.

## The text count line

The text and `--compact` renderers of `refs`, `read`, `impact` and `tests` open with the same
numbers these JSON tables carry: `N inbound (showing M)` on `refs`/`read`, where N is the sum of the
five inbound tables' `total` and M the sum of their row counts, and `N files (showing M)` on
`impact`, where N is the answer's `rows.length + dropped` -- every file the walk reached, which is
`totalAffected + heuristicAffected`, since the line counts the heuristic rows the tables mark and
`totalAffected` alone does not -- and M the printed rows, and on `tests`, where no row cap applies
and the two numbers agree.

The line belongs to those four verbs and those two formats. `refs`, `read`, `impact` and `tests`
under `--json` state the same numbers as their own table fields and print no such line, and `find`
has neither. A resolved answer that reached nothing still opens with `0 inbound (showing 0)` or
`0 files (showing 0)`, while an `ambiguous` or `fallback-advised` answer states no count at all.

Nothing in the JSON changed to make that possible -- the totals were always here, and the text
output now states them rather than leaving a capped table looking complete. The
[CLI reference](cli.md#output-formats) describes the line and the `next:` trailer under it.

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

## The `sections` envelope

`--pick all` answers every candidate of an ambiguous seed, each in its own section, on all four
verbs. The `--json` shape is `schema_version` first, `status: "sections"`, the `query` string as
typed, a `sections` array, `next`, `outcome`, and `freshness` once at the end -- the key order the
`members` wrapper already uses, so a consumer learns one arrangement.

Each `sections` entry carries `pick` (its one-based number, matching `--pick N`), `candidate` (the
refusal's own candidate object at that position, in the refusal's own order) and `answer` (that
candidate's `--json` answer, byte for byte what `--pick N --json` prints, minus the `freshness`
key the wrapper carries once). The sections are never merged, deduplicated or reordered, so a row
always belongs to exactly one candidate.

`next` is the union of the sections' own `next` lists, by the same merge the `members` wrapper
applies to its members. `outcome` is `hit` when at least one section is, and `zero-hit` otherwise,
with exit code 0 or 3 to match. `schema_version` stays 1 and the `outcome` set keeps its four
values: `sections` is a `status`, not a fifth outcome.

```json
{
  "schema_version": 1,
  "status": "sections",
  "query": "Run",
  "sections": [
    {
      "pick": 1,
      "candidate": { "owner": "App.Alpha.Widget", "name": "Run", "file": "src/A.cs", "line": 5 },
      "answer": { "schema_version": 1, "status": "members", "query": "App.Alpha.Widget.Run", "members": [], "next": [], "outcome": "hit" }
    },
    {
      "pick": 2,
      "candidate": { "owner": "App.Beta.Widget", "name": "Run", "file": "src/B.cs", "line": 5 },
      "answer": { "schema_version": 1, "status": "members", "query": "App.Beta.Widget.Run", "members": [], "next": [], "outcome": "hit" }
    }
  ],
  "next": [],
  "outcome": "hit",
  "freshness": { "state": "fresh" }
}
```

Without `--pick`, the same seed still refuses under the `ambiguous` envelope above. `--pick N`
alone answers candidate `N` and prints that candidate's own shape, with no wrapper.

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
| `bus-hop` | A recognized publish or dispatch site's message reaches a registered consumer/handler across project and assembly boundaries. A possible route, runtime routing unverified -- see [`bus-hop`](#bus-hop) below. |
| `call` | A TS/TSX/JS `call` edge: a call or `new` naming the definition. |
| `jsx-use` | A TS/TSX/JS `jsx-use` edge: a JSX tag naming the component. |
| `dispatch` | A TS/TSX/JS `dispatch` edge: an action creator handed to a dispatching call (`dispatch(...)`, `ofType(...)`). |
| `import` | A TS/TSX/JS `import` edge whose bindings name the definition: a named, default or type-only import, or a named re-export. |
| `test-path` | A `tests` row earned because the file has one of the eight TypeScript/JavaScript suffixes (`.ts`, `.tsx`, `.mts`, `.cts`, `.js`, `.jsx`, `.mjs`, `.cjs`) and is either named `*.spec.*` or `*.test.*`, or below a directory segment exactly `__tests__`, with neither an attributed def nor a test project behind it. |

**Vocabulary growth.** `why` and `outcome` may each gain a new word over time without a
`schema_version` bump, the same way an added key never bumps it. A consumer must treat a `why`
or `outcome` value it does not recognise as opaque -- skip it rather than fail.

**TypeScript-family tables.** `refs` and `read` answer the four TS/TSX/JS edge kinds in tables of
their own, keyed by the edge-kind name: `call`, `jsx-use`, `dispatch` and `import`, in that order,
appended to `inbound` after `overrides` and, under `--out`, to `outbound` after `imports`. A table
joins the object -- and the default and `--compact` renderers print it -- only when its total is
non-zero, the rule `implements`/`overrides` already follow, so an answer with no TS edge keeps the
exact bytes it had before these tables existed. They share the one inbound cap and ranking (and,
outbound, the one outbound cap) with every other kind. An inbound `import` row is an import
statement or named re-export binding the queried definition; an outbound `import` row is one bound
definition of one statement, so a statement binding two names is two rows, each naming its own
`to`. The kinds are read by edge kind, not by the file's language, so a JavaScript caller answers
too; that surface is not measured.

**`bound_defs`.** In `graph.json`, a TS `import` edge carries `bound_defs` after `via`, present
only when non-empty: the ids of the definitions its named, default and type-only bindings (or a
named re-export's names) resolve to, in binding order with duplicates dropped. Each id is declared
in that edge's own `to_file` -- a name reached through a barrel sits on the `via` edge to its
declaring file -- so the field never adds an edge. A namespace, whole-module `require`,
side-effect or `export *` statement binds no name and yields no `import` row; its namespace member
calls still answer as `call` rows. A `graph.json` written before the field existed loads and
answers with no `import` table until the next `map` rewrites it.

**Unresolved TypeScript imports.** `graph.json` schema 3 appends `unresolved_imports` only when a
declared-mode scope has imports without an indexed implementation target. A row has `from_file`,
`from_line`, `target` (the source specifier) and one closed `reason`; see the
[reason vocabulary](typescript-modules.md#unresolved-imports). `stats.ts` appends
`unresolved_import_reasons` (reason to count), `unsupported_mode_scopes` (config and mode),
`refused_config` (file and failing field), and `non_literal_dynamic_import_count` when those values
are non-empty. An absent key means there are no records of that kind. These graph additions do not
change its schema version or the query-answer `schema_version`.

**Test files by name.** `tests` counts a file ending in `.ts`, `.tsx`, `.mts`, `.cts`,
`.js`, `.jsx`, `.mjs` or `.cjs` when its name matches the `test-path` rule above
and neither the attribute nor the project vouch reaches it. Such a row
writes `via: "path"` where a project-vouched row writes `via: "project"`, and the default
renderer marks it ` (test path)`. `impact`'s `testsAffected` reads the same definition.

None of these additions changes `schema_version`; it stays `1`.

`why` is derived entirely from what the graph already persists on the edge that produced the row:
its kind, and, for a `uses-member` edge, its `tier` (`ext`, `guess`, or absent, meaning precise).
An `impact` row can fold more than one edge kind into a single per-file summary; when it does,
the strongest evidence wins -- an explicit `ctor-di` site over a plain resolved reference, over
the broader interface hop, over a guess, in that order. The four TS kinds are resolved references;
within that slot the lowest referencing line wins, so a TS row usually reports `import`.

A `bus-hop` joins that same fold, one step past a guess: `ctor-di`, then a resolved reference, then
an ambiguous one, then the interface hop, then a guess, then a `bus-hop` -- never displacing any of
the five, since every one of them is at least a confirmed edge the resolver merely guessed the
target of, where a `bus-hop` is a candidate the analyzer found in source with runtime routing still
unverified. A file whose only path back to the seed crosses a `bus-hop` keeps that possible-route
marker through later ordinary-reference hops; one that also has an independent non-bus path drops
it and reports that path's stronger `why` instead (`impact`'s `busOnly` row key -- see
[`bus-hop`](#bus-hop)).

## `freshness`

`refs`, `read`, `impact` and `tests` (`find` has no `--json` shape) append a top-level `freshness`
object as their true last key, always after `outcome`. `occurrenceIndex` is a *row* key nested
inside `inbound`/`outbound` several levels down, never a top-level sibling of `freshness` -- the
two never share an adjacency to describe. `freshness` is always present under `--json`, on every
outcome, not only a resolved hit: it is a property of the index at query time, not of the specific
query.

`freshness.state` is one of three words:

| `state` | Meaning | Other keys |
| --- | --- | --- |
| `"fresh"` | The worktree asking published this index, at the working tree's current HEAD, and no indexed byte differs from what the index recorded. | none |
| `"stale"` | The index is behind the working tree. | `indexedHead`, `currentHead` (both full-length, unlike the truncated stderr warning), `changedFiles` (indexed files and configuration inputs whose bytes differ from the indexed bytes, plus added and deleted relevant files) |
| `"unknown"` | Freshness could not be established either way. | `reason`, one of the words below |

`freshness.reason` is one of five words:

| `reason` | Meaning |
| --- | --- |
| `"no-index-state"` | No `index-state.json`, or one this build cannot certify: an index built by 0.9.0, an interrupted one, or a corrupt one. The stderr line distinguishes a missing record, which stays silent, from an uncertifiable one, which asks for a re-run. |
| `"git-unavailable"` | `git rev-parse HEAD` did not resolve, or the `git status` read failed or produced output this build cannot parse. |
| `"other-worktree"` | The index was published by a different worktree of this repository. Run `devscout map` here to publish this worktree's own index. |
| `"validation-budget"` | The change set exceeded one of the validation caps (5,000 `git status` entries, 2,048 candidate paths, or 16 MiB of candidate bytes), so freshness could not be verified inside the budget. |
| `"index-changed"` | The record or one of the two artifacts changed while the query ran, or a `map` is running or was interrupted. |

`changedFiles` counts files by their bytes, never by size or modification time, and includes
configuration inputs that model discovery reads: an edited `.csproj`, `Directory.Build.props` or
`Cargo.toml`, and a new file the walk would index.

Both stderr and `--json` come from one validation per invocation, so they describe one reading of
one moment. Every verdict but a genuinely fresh index and two silent cases has its own stderr
line; stderr stays silent for a missing record and for unavailable Git. The stale line keeps the
wording earlier releases used.

This exposes, to a programmatic consumer, exactly the conditions the stderr lines describe for a
human, computed from the same validation and never able to disagree with it. No native precise
tier is upgraded or downgraded by this key -- it says whether the index is current, not whether
any individual row is trustworthy.

## `occurrenceIndex`

Two rows of one inbound or outbound table on `refs`/`read` can otherwise serialize to
byte-identical JSON objects -- for example, two calls to the same target on the same line. When
that happens, each of the colliding rows carries `occurrenceIndex`: a 0-based integer, assigned in
the table's existing stable emission order, appended as the row's last key (after `why`) and
omitted (`Option::is_none`) on every row that has no such collision. It is present **only** when
two or more rows of the *same table* of the *same answer* would otherwise be indistinguishable on
every other field they carry.

`occurrenceIndex` is scoped to one table of one answer against one graph snapshot. It is **not** a
cross-query, cross-table or cross-snapshot identity: the same call site can carry a different
`occurrenceIndex` on a later run against a changed snapshot, or none at all once a sibling that
used to collide with it is edited away. A consumer that needs a stable identity across runs must
not read this field as one.

A row truncated away by an existing cap (`INBOUND_CAP`/`OUTBOUND_CAP`/`--all` lifts both) is
counted in the table's own `dropped`, exactly as before this field existed, and is never assigned
or implied an `occurrenceIndex` -- truncation stays truncation, never a silent merge.

This key is additive under the forward-compatibility rule above: it is new, appended last, and
`schema_version` does not move.

## `bus-hop`

`refs`/`read`'s `bus` table is a separate, always-built table (`RefsModel.bus`, after `memberRefs`
and before `outcome` on the top-level JSON object) combining every inbound (this symbol is the
handler) and outbound (this symbol's own file publishes) `bus-hop` row for the resolved symbol.
Neither `freshness`'s true-last-key position nor `occurrenceIndex`'s row-collision rule changes:
both are about different tables and a different scope than this one. An array message is
identified by its element's id followed by its array suffix (`[]` for one array layer): a single
message and an array of that same message are distinct identities on both sides of a hop. An
array of arrays is not yet told apart from a single further layer -- a named gap.

Every bus-hop row is a **possible route with runtime routing unverified** -- a candidate type
relationship the analyzer found in source, never a confirmed delivery. This is stated three ways:

- **Text.** Every rendered bus-hop line (`refs`/`read`) ends with the words `(possible route,
  runtime routing unverified)`.
- **`--json`.** The row's own object carries an additive `possibleRoute` key (its true last key,
  after `handlers`): `{"unverified": true, "verify": {"publisher": "<file>:<line>", "message":
  "<id>", "handler": "<id>", "handlerFile": "<file>"}, "missingEvidence": [...]}`. `verify` names
  the exact identity a human would re-check by hand; `missingEvidence` is a fixed list of the
  evidence classes this analyzer never establishes at all (receiver registration at runtime,
  endpoint/routing topology, host co-location) -- never a search this analyzer ran, only the
  classes it cannot see from source.
- **`--compact`.** The row collapses to its existing terse marker (`file:line` plus the direction
  letter) with a trailing `?`, and the block's own header names the path back to the full row
  (`bus-hop (? = possible route, rerun without --compact for the full row)`).

`impact`'s blast radius carries the same uncertainty downstream, in both directions: a seeded
handler reaches its publishers (the reverse hop `refs`/`read` also show), and a seeded publish site
-- or a symbol whose declaration contains one -- reaches its candidate handlers in turn, at every
later hop within the requested depth and the shared brake, the same as every other edge kind.
`--no-bus` reproduces the answer neither direction would have contributed, byte for byte.

A row whose only path back to the seed crosses a bus hop, at any hop depth, discloses it the same
three ways a `refs`/`read` bus-hop row does:

- **Text.** The row's line ends with the words `(possible route, runtime routing unverified` plus,
  since an `impact` row carries no message/handler fields of its own otherwise, the identity to
  re-check: `: message=<id> handler=<id> handlerFile=<file>)`.
- **`--json`.** The row carries an additive `busOnly: true` key (`--json`, after `why`) plus a
  `possibleRoute` object shaped exactly like a `refs`/`read` bus-hop row's own (`unverified`,
  `verify`, `missingEvidence`), fed from the specific hop the row's marker traces back to -- the
  lowest by (publisher file, line, message, handler) when more than one hop could have supplied it.
  `busOnly` is kept alongside `possibleRoute`, never replaced by it.
- **`--compact`.** The row carries a trailing `?` marker and a pointer to rerun without `--compact`
  for the full row, the same shape `refs`' own compact bus block uses.

A row also reached by an independent non-bus path -- at that hop or an earlier one -- carries
neither: its stronger evidence (the `why` a non-bus edge kind supplies) stands on its own, the same
way an explicit `ctor-di` site already outranks a plain reference, and it discloses no bus-hop
identity at all.

`tests` carries the same disclosure for its OWN bus-hop half: a test file that PUBLISHES to the
seeded handler, over a bus hop, is a possible route the same three ways -- its own `bus-hop` table
(text and `--compact` blocks, and a `--json` `bus-hop` key mirroring `refs`' own) reuses the
identical row shape and wording `refs`/`read` already use. These rows are counted in neither
`testFileCount`/`refCount` nor their heuristic twins: a possible route must never inflate precise
test coverage.

Nothing here adds a graph field, a schema field, a `why` word or an evidence word: this is
query-time rendering over facts the graph already carries (see [`why`](#why) for `bus-hop`'s own
weakest-evidence place in the fold-when-more-than-one-kind-fired rule).

## One worked example per verb

Each example below is a real `--json` run against a fixture in this repository
(`fixtures/conformance/csharp/`, copied into its own throwaway git repo and mapped), reformatted
for readability. `_store`/`store` are the only two callers of `InMemoryCatalogStore.CountItems`;
`CatalogStoreTests` is the only test file; `ICatalogStore` is the interface `InMemoryCatalogStore`
implements. These four examples predate the `freshness` key and are kept focused on the
pre-existing shape, so its top-level object is elided from each -- see the `## freshness` section
above for its own shape and the three states it can carry; every example below still gains it as
its true last key on every real run.

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
      "next": [["tests", "Widgetworks.Catalog.InMemoryCatalogStore.CountItems"], ["impact", "InMemoryCatalogStore.cs"]],
      "outcome": "hit"
    }
  ],
  "next": [["tests", "Widgetworks.Catalog.InMemoryCatalogStore.CountItems"], ["impact", "InMemoryCatalogStore.cs"]],
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
  "next": [["tests", "Widgetworks.Catalog.InMemoryCatalogStore"], ["impact", "InMemoryCatalogStore.cs"]],
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
  "next": [["refs", "Widgetworks.Catalog.ICatalogStore"]],
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
  "next": [["impact", "InMemoryCatalogStore.cs"]],
  "outcome": "hit"
}
```

`CatalogStoreTests` earns its row because it declares `CountItems_StartsAtZero`, a def carrying
xUnit's `[Fact]` attribute -- `why: "test-attribute"`. The other ways a `tests` row is earned are
`test-project` (the file's unit is in a project marked `test`, with no attributed def of its own)
and `test-path` (a TypeScript/JavaScript file with one of the eight listed suffixes
named as a test, with neither of the other two).
