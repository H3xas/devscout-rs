# Compiler-backed enrichment layer

A design, not an implementation. Nothing here exists in `src/` today: every artifact, flag, field
and constant this document introduces is marked **new** or **proposed** where it is first named,
and everything else is grep-verifiable at `1774b1b`, the commit that introduced this file; the
line numbers in its citations are that commit's, so on a later `main` search by the identifier
named next to them. The layer would let
`resolve` replace a name guess with a fact a real compiler already produced — for C# only, from a
cache, out of process, never on the agent hook path.

## Purpose and decision on record

`devscout` resolves `uses-member` references with tree-sitter syntax and a ladder of rules whose
top is honest about what it is: `src/graph.rs`'s `HeuristicTier` splits the guesses into `Ext`
(C#'s own extension-method lookup over a recorded `(member, this-type)` bucket) and `Guess` (the
scored tier, picking by name among the defs declaring a member so called). `tools/scout-semantic`
is the compiler oracle those tiers are scored against; `devscout audit --semantic <refs.jsonl>` is
the scorer.

The rule that governs this document was registered before Run 2 of the resolver-precision family
and is quoted from
[`docs/benchmarks/results/2026-09-resolver-precision.md`](../benchmarks/results/2026-09-resolver-precision.md):
**recall precise+ext at or above 0.70 would have closed the question.** Run 4 measures **0.467**.
The question therefore stays open and moves to design rather than to code. That floor is measured
against a moving baseline: the same document's "Changes after Run 4, not yet measured" section
lists five resolver and extractor fixes that are not in those figures, so the gate applies to
whatever the next syntax-lane run measures, not to 0.467.

Two further facts from that section bound what follows:

- The remaining misses are bucketed by receiver kind and by target kind there, though that
  document states it "holds no separate per-target-kind table" — the target-kind evidence is the
  attribution of the top missed target. The **largest single class is accesses on lambda parameters
  typed only by the callee's delegate parameter**, a syntactic rule for in-graph callees. That rule
  is measured first, on its own, and this layer is weighed against whatever it leaves; it is named
  here only by that shape and is not part of this design.
- The input contract is **the per-site record the oracle already emits** — `receiverText`,
  `receiver`, `target` in `tools/scout-semantic/Records.cs` — **consumed from a cache and never on
  the hook path.** Of that record this design reads `file`, `startLine`, `member`, `target`,
  `targetKind`, `shape`, `external`, `ambiguous`, `unit`, and from `UnitRecord` `name`, `status`,
  `diagnostics`. The rest is carried but never consulted — every key is stored verbatim so the
  audit can read the same rows.

Fourteen points are settled below, each with the alternative it displaced. The open question is
not "what shape" but "does it clear the gate in
[Shipping gate and cost model](#shipping-gate-and-cost-model)"; if it does not, the layer is not
shipped and this document stays as the record of why.

## Record contract

### What the oracle emits today

`tools/scout-semantic/Records.cs` declares three record types, all with fixed JSON key order and
every key always written (`null` when unknown):

| Record | Fields | Emitted to |
| --- | --- | --- |
| `RefRecord` (17) | `file`, `startLine`, `line`, `shape`, `receiverKind`, `receiverText`, `receiver`, `member`, `memberKind`, `target`, `targetKind`, `targetFile`, `targetUnit`, `ext`, `external`, `ambiguous`, `unit` | `refs.jsonl` (`--out`) |
| `UnitRecord` (8) | `name`, `path`, `tfm`, `test`, `status`, `diagnostics`, `refs`, `files` | `units.jsonl` (`--units`) |
| `DefRecord` (6) | `id`, `kind`, `file`, `line`, `unit`, `test` | `defs.jsonl` (`--defs`) |

`RefRecord.CompareKeyTo` is the sort and dedup key — `(file, startLine, line, member, target,
ambiguous)` — which collapses one ambiguous site's overload candidates into a single row. Three
facts from `tools/scout-semantic/Walker.cs` decide what the consumer may infer. A site whose
`GetSymbolInfo` yields **neither a symbol nor candidates emits no record at all** — there is no
"the compiler looked and found nothing" row. When the symbol is null and candidates exist, **one
record per candidate** is emitted with `ambiguous: true`. `External` is
`symbol.DeclaringSyntaxReferences.Length == 0`, and `Ext` marks a reduced extension method,
un-reduced to its `ReducedFrom` definition so `target` names the declaring static class.

**D1 (settled).** The consumer is a **resolve-time per-site override**, not an extract-time fact.
Rejected: writing oracle facts into the per-file fragment cache, which is content-cached per file
and must never depend on an external tool having run.

**D2 (settled).** Stated as the per-reference rule the placement implements: the layer is consulted
**for a reference that produced no precise edge**, at the moment the heuristic tiers would
otherwise run. A precise edge produced later in the same loop at the same join key does **not**
retract an already-pushed semantic edge — a repeated fact on one line, which the resolver already
tolerates (`src/resolve.rs:3230`'s dedup pass leaves precise edges alone on purpose). Precise edges
are never overridden: precise precision is 0.972 on Run 4, and a disagreement is an audit finding
to investigate, not an edge to rewrite. Rejected: letting the compiler win everywhere, hiding
extractor bugs behind the oracle.

**D3 (settled).** Three classes of record, and only three.

| Class | Condition | Effect at the site |
| --- | --- | --- |
| Positive | `ambiguous == false`, not `external`, `target` known to the graph by `target_known`'s rule, and the record's `unit` has `status == "ok"` — **any `shape`** | emit one `uses-member` edge; skip the site's `ext`/`guess` tiers |
| Negative | `ambiguous == false`, `external == true`, `shape == "access"`, and the record's `unit` has `status == "ok"` **and** `diagnostics == 0` in the cache's units | skip the site's `ext`/`guess` tiers, add nothing |
| Inert | everything else | no effect; the syntax tiers run exactly as today |

The two active classes are disjoint, and evaluation order is fixed: `external` records go to the
negative rule, and only a non-external record can be a positive. Inert covers `ambiguous == true`
records; **every** record from a unit whose `status` is not `"ok"`, positive or negative alike,
because a failed compilation's bindings are not trusted at all; `external` records from a unit
reporting any `diagnostics`; `conditional` and `bare` **negative** records (that rule is
`access`-only); a record whose target the graph does not know; one whose `file` is not a fragment
key; a cross-unit target conflict at one join key (below); and — the important one — **a site with
no record at all.**

**The positive rule carries no shape clause, deliberately.** `access`, `conditional` and `bare`
positives all emit: a bare unqualified call the extractor recorded a reference for is exactly a
site the syntax tiers cannot type. The negative rule stays `access`-only because that is the
audit's own leak rule. So `conditional` and `bare` positives show in the `semantic` tier row but in
**no** recall figure, whose denominator is recall-D and `access`-only (`src/audit.rs:879-881`).

`target_known` (`src/audit.rs:431`) is the existing rule this borrows: an exact def id, or, for a
record with `targetKind == "enum-member"`, the bare enum id its `target` is prefixed by. Which of
the two the edge's `to` carries is fixed: **the exact `target` whenever that id is a graph def;
otherwise, and only for `targetKind == "enum-member"`, the bare enum id; when both exist the exact
id wins.** The edge is then
`{from_file: file, from_line: startLine, to: <that id>, to_file: <that def's `Def.file`>, member,
source: "semantic"}`: the `heuristic` key is omitted (its value is false) and no `tier` key is
written. `to_file` always comes from the
graph's own def index, never from the record's `targetFile`: for a partial type the index carries
**one `Def` per id**, whose `file` is the first declaration and whose other declaring files sit in
`also_in`, and `src/audit.rs`'s `EdgeRow` doc comment spells out why the join is on def id alone —
so a partial class does not split into two answers.

**Path spelling, because the join is an exact string match.** A record's `file` is root-relative
and `/`-separated (`Walker.cs`'s `Relative` against `--root`); the graph's `from_file` is
`repo::rel_path`'s spelling (`src/repo.rs:143`), also root-relative and `/`-joined. They are
compared as strings, with no normalisation and **case-sensitively on every platform**; a record
whose `file` is not a fragment key is inert and counted in `recordsIgnored`, one of the
**proposed** `stats.semantic` counters defined in [Cache](#cache). That has teeth: the
oracle strips the root with a comparison ordinal only on Linux and case-insensitive elsewhere
(`RepoPaths`'s `PathCmp`, `tools/scout-semantic/Walker.cs:23-26`), so a casing divergence makes the
record inert rather than mismatched. The `--root` the **proposed** `semantic run`
([Sidecar invocation](#sidecar-invocation)) passes is therefore always **the repository root `map`
uses**, not the solution's directory.

**Duplicates and conflicts at one join key.** Two non-ambiguous positive records can share
`(file, startLine, member)` with different `target`s. Two cases, split by `unit`: from the **same
`unit`** it is two receivers of different types reading a same-named member on one line, and the
rule is **one semantic edge per target**; from **different `unit`s** it is one source line bound
differently by two compilations — whether the records came from two solutions or from one that
walks the file under two projects — and the join key is **inert**, because the layer cannot tell
which compilation the reader means and guessing is the thing it exists to stop.

Two tuples, not one. **Lookup and the tier skip use the 3-tuple join key** `(file, startLine,
member)` — what the resolver asks the layer about, and what "the layer has acted here" means.
**Edge dedup uses the 4-tuple** `(file, startLine, member, target)`, so one join key carrying two
same-unit targets yields two edges and neither is emitted twice; that 4-tuple is deliberately not
the oracle's `CompareKeyTo`, which carries `line` too and would emit twice for a chain split across
lines. A second syntax reference at a join key the layer has already claimed pushes nothing and
still skips the tiers. A positive and a negative at one join key: the positives apply, the tiers
stay skipped. A join key whose records are all `ambiguous`: no effect.

**Absence is never a signal.** `tools/scout-semantic/README.md` states the reason plainly: the
target solution must be restored first, because "an unrestored project loads without its metadata
references, which silently turns real symbols into unresolved candidates". A missing record is
therefore indistinguishable from a misconfigured run, so the negative rule is built on a *present*
`external: true` record from a unit reporting `status: "ok"` and zero `diagnostics` — never on
silence. Restore state is not the only reason for silence either: `Accept`
(`tools/scout-semantic/Walker.cs:304-313`) keeps only methods, properties, fields and events,
dropping constructors, static constructors, destructors, local functions and anonymous functions.

## Cache

**D4 (settled).** One **new** file, `semantic-v1.json`, beside the fragments cache in the directory
`graph_dir` resolves (`src/graph.rs:101`): `<git-common-dir>/scout/graph/` inside a git repository,
`<root>/.scout/graph/` outside one. The version literal lives in the filename exactly as
`fragments-v16.json` does, and a superseded generation rolls over through `SUPERSEDED_CACHE_FILES`
(`src/graph.rs:157`), so the rename is the invalidation mechanism and no reader carries
version-compat logic. Nothing rolls over today: a future `semantic-v2.json` writer is what adds
`"semantic-v1.json"` to that list. Rejected: one cache file per solution, needing its own merge
rule on the read path.

Proposed shape. **This block is the normative key order, verbatim:**

```
{ "schema": 1,
  "oracle": { "tool": "scout-semantic", "version": "<informational>", "recordsSchema": 1 },
  "hashedAt": "oracle" | "import",
  "projectModel": "<sha256 hex of the fresh project-units serialization>" | null,
  "solutions": ["<root-relative solution paths in run order>"],
  "units": [ UnitRecord... ],
  "files": { "<root-relative path>": { "sha256": "<hex of file bytes>", "records": [ RefRecord... ] } } }
```

`files` keys sort **ordinal (byte-wise) ascending**; records keep the oracle's own order; **every
`RefRecord` is stored verbatim, all 17 keys**, so rows extracted from the cache parse
byte-identically to a real `refs.jsonl`. The writer is `serde_json::to_vec` over a struct in
exactly that field order — compact, as `graph.json` is written (`atomic_write_json`,
`src/graph.rs:226`) — so two writers over the same inputs produce the same bytes, which is what
makes a digest of them a safe rebuild trigger. Two header fields the tool does not supply:

- **`oracle.recordsSchema` is a literal the consumer owns.** No version or schema constant exists
  in `tools/scout-semantic/*.cs`; only package versions are pinned in
  `tools/scout-semantic/scout-semantic.csproj`. The proposed `SEMANTIC_RECORDS_SCHEMA: u32 = 1`
  lives beside `GRAPH_SCHEMA_VERSION` (`src/graph.rs:837`), stamped only after the writer has
  checked that every record carries exactly the 17 `RefRecord` keys; an unknown or missing key
  makes the import refuse with exit 1.
- **`oracle.version` is informational**, from a proposed additive `--version` flag on the tool
  (its assembly informational version and the Roslyn package version). The probe is best-effort:
  `Program.Parse` throws on any unknown option (`tools/scout-semantic/Program.cs:147-150`), so a
  tool built before the flag exits non-zero and the writer stores `null`. No gate reads the field.

**D5 (settled): per-file validity, whole-layer gates.** `map` recomputes each walked C# file's
sha256 and applies its records only when the stored hash matches; a mismatch drops **that file's
records only** — the site falls through to the syntax tiers — and counts as stale. The hash is over
the file's **raw bytes as stored on disk** (no decoding, no newline normalisation, a byte-order
mark included), lowercase hex. These are bytes `map` already reads: under content-hash reuse
`cache_key_for` (`src/mapcmd.rs:361`) does `fs::read` on every graph file and hashes it with no
decode in between, so the layer costs no extra read — it just retains the full digest beside the
8-byte `hashkey::cache_key`. A file whose bytes do not decode cleanly hashes like any other and
keeps its records.

**Under `SCOUT_MTIME_REUSE=1` the layer is disabled.** That branch (`src/mapcmd.rs:369`) calls only
`fs::metadata`, so no bytes are read and nothing can validate a record. `map` applies no records,
writes no `stats.semantic`, and the rebuild trigger treats the run as wanting no cache. It prints
`semantic: cache ignored (mtime reuse)` **only when `semantic-v1.json` exists** — `load` stats the
path first and returns `None` silently when there is no file, so a repository that never ran the
oracle never sees the line. **One rebuild per mode switch, never per run**: after an enriched build
the first mtime run finds the sidecar with nothing wanted, rebuilds once and deletes it, and every
further mtime run is steady state; switching back to hash reuse with the cache still present
rebuilds once more, then is steady again.

Four conditions instead disable the layer entirely; the build is then byte-identical to one with
no cache at all:

| Gate | Condition |
| --- | --- |
| schema | `schema != 1` |
| records schema | `oracle.recordsSchema != SEMANTIC_RECORDS_SCHEMA` |
| project model | `projectModel` is not the sha256 of this run's fresh `serde_json::to_vec(&graph_units(model))`, or exactly one side is null |
| parse | the file is not valid JSON of this shape |

The hashed bytes are the **fresh serialization**, not the file on disk: `project::sidecar_differs`
(`src/project.rs:425`) byte-compares `project-units.json` against
`serde_json::to_vec(&graph_units(m))` computed on the spot (`src/project.rs:433`), so hashing that
same serialization keeps one definition of "the project model changed". The gate matters because a
`.csproj` is not a `SOURCE_EXT`: editing one moves no walked file's mtime, so `index_is_stale`
alone can never see it (`src/graph.rs:123`).

**The accepted unsoundness, stated rather than hidden.** A record in a file whose hash still
matches can name a target that still exists but is no longer what the compiler would bind after an
edit *elsewhere* — a new base-class overload, a changed field type in another file — and nothing
detects it until the next oracle run. `target_known` catches an outright removal and the
project-model gate catches every change to the project graph; cross-file drift inside an unchanged
project is the residue, and the price of a cache that does not re-run a compiler on every `map`.

**D6 (settled): where the file hashes come from.** A **new**, additive oracle output
`--files <files.jsonl>`, one `{"file": "<root-relative>", "sha256": "<hex>"}` per walked document,
sorted by file; `RefRecord` is unchanged. It hashes the same on-disk bytes `map` does, reading the
file itself rather than Roslyn's already-decoded in-memory text. **One row per path**: a file
linked into two projects is walked once per project (`tools/scout-semantic/Program.cs:259-280`) and
hashes identically both times, so rows dedup on path — two projects disagreeing is about *records*,
not hashes, and is the cross-unit conflict rule above. The proposed `import` accepts a missing
`--files` by hashing the working tree and marking the header `"hashedAt": "import"`; the documented
race is that an edit between the run and the import goes unnoticed until that file changes again.
Both it and `run` stamp `projectModel` by running the same `.csproj` discovery `map_repo` runs
(`src/project.rs`) and hashing the fresh `serde_json::to_vec(&graph_units(model))` then, so the
gate compares an import-time hash against a map-time one. Rejected: keying validity on mtimes —
the weaker signal, and in the one mode devscout offers it there are no bytes to key on at all.

**D7 (settled): the rebuild trigger.** `Stats` (`src/graph.rs:739`) gains an appended-last,
omit-when-`None` field `semantic: Option<SemanticStats>` (both **new**) carrying
`{ cacheId, filesCurrent, filesStale, recordsApplied, recordsSilenced, recordsIgnored }`, where
`cacheId` is the sha256 hex of the `semantic-v1.json` bytes read. It is written **only** when the
layer was present and passed every gate, so a build without the layer writes bytes identical to
today's. The omit-when-`None` precedent on that struct is `ts` (`src/graph.rs:771`) and only `ts`:
`heuristic_edge_count` (`:758`), `test_def_count` (`:763`) and `heuristic_by_tier` (`:780`) are
always serialized, and `semantic` must not be, or every graph without the layer would grow a key.
The counters, defined so two implementations agree:

| Counter | Definition |
| --- | --- |
| `filesCurrent` | walked C# files in this run's scope that have a cache entry whose hash matches |
| `filesStale` | walked files in scope whose cache entry's hash differs |
| `recordsApplied` | positive records that produced an edge |
| `recordsSilenced` | negative records that suppressed the heuristic tiers at a site |
| `recordsIgnored` | records in current files inert for any reason — ambiguous; unknown target; path not a fragment key; a shape-gated (`conditional`/`bare`) negative; a status-gated record from a non-`ok` unit; a diagnostics-gated `external`; a cross-unit conflict |

A walked file with no cache entry, and a cache entry for a file outside this run's walk, count in
**neither** file column; records inside a stale file are not counted at all, because the file is
the unit of staleness. Same rule for a **scoped `map`**: `devscout map src` consults the layer only
for the files that run walks.

A trigger is needed because `rebuild_graph` (`src/graph.rs:1655`) returns
`RebuildOutcome::NotRebuilt` when `!changed && graph.json exists && graph_schema_is_current(root)`,
and importing a cache changes no walked file and no project model. It does **not** read
`stats.semantic` back: `Graph`'s field order is `schema_version`, `built_at_head`, `defs`, `edges`,
`stats`, so `stats` sits past both full arrays — out of reach of the 64-byte head read
`graph_schema_is_current` (`src/graph.rs:1608`) uses, and `read_graph` would open the artifact this
fast path exists to skip. Settled mechanism: a **new** sidecar
`<graph_dir>/semantic-applied.json` holding `{"cacheId": "<sha256 hex>"}`, written by
`rebuild_graph` when the layer was applied and **deleted** when it was not — the
`project-units.json` convention, whose doc comment (`src/graph.rs:123`) spells out why a file left
behind would be indistinguishable from "no model". The third OR term at `src/mapcmd.rs:704` is a
proposed `semantic_sidecar_differs(root, wanted)` with the same four-way match
`project::sidecar_differs`
(`src/project.rs:425`) uses: no sidecar and nothing wanted (no cache, or the **new**
`map --no-semantic`) is
`false`; a sidecar with nothing wanted, or a cache wanted with no sidecar, is `true`; two ids
compare unequal. `stats.semantic.cacheId` stays as the reported field, but the **sidecar is
authoritative for the trigger** — should the two disagree, the cost is one extra rebuild, the safe
direction. The case people hit, stated explicitly: **`map --no-semantic` after an enriched build
finds the sidecar, forces a rebuild, and that rebuild deletes it.**

The hash is a full sha256 hex, not `hashkey::cache_key` (`src/mapcmd.rs:144`), which is documented
as an opaque 8-byte reuse key never exposed as a content hash. The house full-digest helper is
`digest_hex`, `pub fn` inside the private `mod sha256` (`src/hookio.rs:176`) over the `sha2` crate
`Cargo.toml` pins; the implementation lifts it to `pub(crate)` beside `hashkey` rather than growing
a second digest routine.

## Pipeline entry and edge tagging

**D1, made precise.** The override runs **inside the per-reference loop of
`resolve_graph_with_model`** (`src/resolve.rs:2103`) — not as a post-pass over the finished edge
vector, because `graph.json`'s edges are unsorted and their byte order *is* insertion order. For a
`uses-member` reference where the syntax ladder produced no precise edge and the heuristic tiers
are about to run — the `Some(HeuristicTier::Ext)` emission at `src/resolve.rs:2905`, the
`Some(HeuristicTier::Guess)` emission at `src/resolve.rs:3092` — the resolver first consults the
layer for the join key `(file, startLine, member)`. A **positive** record pushes the
`source: "semantic"` edge at exactly that point and skips both heuristic tiers; a **negative**
record skips both tiers and pushes nothing; **no usable record** and the tiers run as today.

Four consequences, and they are the reason for this placement. The semantic edge sits **where the
site's own heuristic edge would have sat**, so ordering is unchanged everywhere else. Nothing is
deleted after the fact, so `heuristic_edge_count` and `heuristic_by_tier` never count a replaced
edge. The heuristic dedup pass (`heuristic_edge_key`, `src/resolve.rs:2023`, applied at
`src/resolve.rs:3230`) is untouched, keying only on heuristic edges. And a record can only act at a
site the extractor recorded a reference for: **the layer never invents a site**, so it cannot
repair a shape the extractor does not record — a real bound on the reachable recall gain, measured
rather than assumed.

### How the layer reaches the resolver

The resolver does no I/O for this. `map_repo` (`src/mapcmd.rs:412`) loads and gates the cache after
the walk, through a proposed module `src/semantic.rs` whose entry point is
`load(root, &MapOptions) -> Option<SemanticLayer>` — `None` for absent, gated-out, `--no-semantic`
or mtime-reuse. `rebuild_graph` (`src/graph.rs:1655`) and `resolve_graph_with_model`
(`src/resolve.rs:2103`) each gain **one** parameter, `Option<&SemanticLayer>`; the thinner
`resolve_graph` (`src/resolve.rs:2075`) and `resolve_graph_with_ts` (`src/resolve.rs:2088`) pass
`None`. Both changed functions are `pub` with existing test callers, updated in the same commit.

Loading in `map_repo` preserves the resolver's purity — its only I/O today is the
`git rev-parse HEAD` shell-out behind `manifest::git_head` (`src/resolve.rs:3251`), and a mid-resolve
file read would make every resolver unit test filesystem-dependent. `rebuild_graph` then writes or
deletes `semantic-applied.json` from the outcome the resolver hands back in `Graph.stats.semantic`,
so the sidecar and the counters cannot disagree about whether the layer was applied.

**D8 (settled): the edge tag.** `source: Option<Provenance>` is appended after `member` on
`Edge::UsesMember` (`src/graph.rs:504`), with `skip_serializing_if = "Option::is_none"` and
`default` on read — the slot `src/graph.rs:466-468` already reserves **by that exact name and
position**. `Provenance` is a **new** one-variant enum whose `Semantic` serializes as `"semantic"`,
so the on-disk key reads `"source":"semantic"` and a second provenance later costs a variant, not a
schema argument. **No
`GRAPH_SCHEMA_VERSION` bump**: the key is omitted when empty, so a graph built without the layer is
byte-identical to today's and an old reader sees only an added key. Rejected: a new edge *kind*,
forcing every consumer of `kind == "uses-member"` to learn a second spelling. The edge is built
through a **new** second constructor,
`Edge::uses_member_semantic(from_file, from_line, to, to_file, member)`, beside `Edge::uses_member`
(`src/graph.rs:671`): no existing call site changes, and no emit site can build a semantic edge
that also carries a tier — the reasoning that made `uses_member` derive `heuristic` from `tier`
rather than take both.

**Query surface.** A semantic edge is precise-class and renders like one: the `heuristic` key is
omitted (its value is false) and no `tier` key is written, so `src/query.rs`'s `admits` closure
never sees it. It
survives `--no-guess`; it joins the precise adjacency and is therefore a legitimate premise for a
further `impact` hop (the comment at `src/query.rs:2765` explains why a *guess* is not);
`compact_marker` (`src/render.rs:94`) appends no mark, because an unmarked row is a fact and this
is one; and it joins "the per-file precise inbound-edge count `find`'s tie-break ranks by"
(`src/query.rs:226`), whose own doc comment counts `uses-member` edges "without the guess tag"
(`src/query.rs:234-240`). Those last two are a
behaviour change worth naming: with the layer on, `find` ordering and `impact` reach change,
because precise-class edges enter the adjacency the ranking runs over. Off, absent or gated out,
nothing moves.

**The graph-edge key and the query-row key differ, deliberately.** On the edge the tag is
`source`; on a query row `source` is **already taken** by the source line text (`src/query.rs:1288`
declares `pub source: String`, and `j_inbound_row` at `src/cli.rs:1262` appends it after
`heuristic`/`tier`). A JSON row therefore carries a **proposed** `provenance: "semantic"` key
after `source`, omitted elsewhere — a plain string there, not the edge's `Provenance` type. Two
names for one fact is a cost; changing what `source` means on a row agents parse is a larger one.

**D12 (settled): determinism.** Records apply in the oracle's own order; the emitted edge takes the
same ordering the resolver applies to everything else; `source` serializes last on the edge. Two
`map` runs over one tree and one cache are byte-identical, and the cache is byte-identical for the
same inputs because the oracle is sequential by design — one project, one document, one syntax node
at a time, which `tools/scout-semantic/README.md` says is exactly why.

## Audit treatment

**D9 (settled).** `tier_of` (`src/audit.rs:314`) maps an edge with `source == "semantic"` to a
**new** `Tier::Semantic`, checked **before** the `tier` string and the `heuristic` bool; `ORDER`
(`src/audit.rs:60`) becomes precise, ext, guess, heuristic, semantic — appended last, so every
existing row and JSON key keeps its position — and `Tier::key()` gains `"semantic"`.

The precise, ext, guess and heuristic rows keep their definitions and their columns —
`tier edges tp fp precision fp:no-site fp:external fp:wrong structural` — computed over whatever
edges the audited graph holds; the `semantic` row is one more row with the same columns.

### Comparability is by lane, not by row

Printing a second "syntax" recall on the enriched graph and calling it comparable with Runs 1–4
does not work. At every enriched site the heuristic tiers are **skipped**, so that graph holds
strictly fewer `ext` and `guess` edges than the syntax-only build of the same tree: every per-tier
row on it, and any "syntax" recall recomputed over it, is a figure over a *different edge set*, and
filtering semantic edges back out cannot restore guesses the resolver never emitted. The two
figures come from **two builds**, not two rows of one report:

| Lane | Build | Audit |
| --- | --- | --- |
| syntax | wiped state, no cache on disk (or `map --no-semantic`) | as today — **this** is the figure comparable with Runs 1–4 |
| enriched | the same tree plus the cache | the tier rows over the edges that graph holds, fewer heuristic edges by design, plus the `semantic` row |

A semantic edge is counted **precise-class**: `site_hit`'s tier predicate admits `Tier::Semantic`
alongside `Tier::Precise` for `recall.precise`, alongside `Tier::Precise` and `Tier::Ext` for
`recall.precise_ext`, and everywhere for `recall.all`, so a semantic hit counts in all three. The
predicate is **unconditional** — it never consults the lane; on the syntax lane there are no
semantic edges, so admitting them moves nothing, and the **proposed** `lane` key stays a report
label rather than a scoring input. The denominator does not move either: recall-D, `access`-only,
non-external, `target_known` (`src/audit.rs:879-881`) — changing it makes every run incomparable.

`--json` therefore grows two **proposed** things: a `lane` string, `"syntax"` or `"enriched"`,
written **always** as the last top-level key and set from whether the audited graph carries
`stats.semantic`; and a `tiers.semantic` object with the same columns as every other tier, written
whenever the graph carries at least one semantic edge. In text
mode `lane` is appended to the header line `render_text` prints (`src/audit.rs:1027`). No existing
key changes meaning or position, and no `recall.enriched` object is added.

**A zero-edge tier row is absent, not zero.** `tiers_ordered` (`src/audit.rs:862-864`) drops any
tier with `ts.edges == 0`, so a graph with no semantic edges has no `semantic` row and no
`tiers.semantic` key. `evaluate_assert` counts a missing path as a violation (`src/audit.rs:1345`),
which is the useful behaviour here: the enriched thresholds file asserts `tiers.semantic.edges`
with a `min` as well as the precision, so a build that silently applied nothing fails instead of
passing vacuously.

### What the `semantic` row's precision means

In CI and in both benchmark lanes the cache is imported from the **same oracle output the audit
reads**, and under that pairing the `semantic` row's precision is **1.0 by construction** — every
semantic edge came from a non-ambiguous compiler record whose target the graph knows — so any lower
value is a consumer bug. Audited against an oracle output *newer* than the cache, a semantic false
positive instead measures the cross-file staleness D5 accepts, and the report cannot tell the two
apart. That is why the lanes always pair a cache with the output it was imported from, and why the
**proposed** `semantic status` exists.

`audit` reads the oracle file the caller passes on `--semantic` and **never the cache**; pairing
them is the caller's job. `cmd_audit`'s flag set is unchanged — `--semantic` (required), `--units`,
`--defs`, `--assert`, `--json`. The enriched CI step asserts against a **second, new** file,
`fixtures/csharp-semantic/expected-enriched.json`, carrying `tiers.semantic.precision` with
`min 1.0` **and** `tiers.semantic.edges` with a `min`; the existing `expected.json` and the step
that uses it are untouched.

## Sidecar invocation

**D11 (settled).** A **new** `semantic` command group of four subcommands. `map` and the hooks
**never spawn `dotnet`.**

```
devscout semantic run [--solution <path>]... [--timeout <secs>] [--tfm <tfm>] [-p K=V]... [--restore]
devscout semantic import <refs.jsonl> --units <units.jsonl> [--files <files.jsonl>]
devscout semantic status
devscout semantic clear
```

- **`run`** discovers `*.sln`, `*.slnx` and `*.slnf` under the root when none is given, sorted —
  the three extensions `Loader.cs:52` opens as a solution; anything else it accepts, a `.csproj`
  included, is named explicitly with `--solution`. Discovery is a **new** traversal reusing only
  `src/walk.rs`'s `SKIP_DIRS`, because none of those extensions is a `SOURCE_EXT`
  (`src/walk.rs:72`) and the ordinary walk cannot find them. It requires `dotnet` on `PATH` and the
  **new** env var `SCOUT_SEMANTIC_TOOL` (path to the built tool's `.dll` or executable); without
  either it exits 2 with one line and leaves the cache untouched. It runs `dotnet restore` **only**
  under `--restore`, because restore has network and policy implications a mapping tool should not
  assume. It then runs the oracle once per solution with
  `--root <root> --out <refs> --units <units> --files <files> --strict`, concatenates the
  per-solution outputs, dedups on the oracle's own key and nothing else, and writes through
  `atomic_write_json` (`src/graph.rs:222-226`) — its unique temp name, not a fixed `.tmp`.
- **Any non-zero oracle exit aborts the whole `run`.** The oracle writes its outputs *before* it
  returns 2 (`tools/scout-semantic/Program.cs:308-346`), so a strict-failed run leaves
  usable-looking files behind that must not be merged. Exit 2 (strict failure) and 3 (zero
  projects) — the codes `Program.cs:48` documents — both abort, one failing solution of several
  aborts all, nothing is written, and the existing cache stays byte-identical.
- **The merge synthesizes nothing.** Records enter the cache exactly as emitted; there is no
  invented "conflicting" record. The conflict rule is entirely consumer-side (see
  [Record contract](#record-contract)) and covers the cross-unit case however the records arrived,
  which is what makes it testable through `import` of a hand-written file with no `dotnet` present.
  Rejected: preferring the first solution, which would make the answer depend on discovery order.
- **`--timeout` is a kill switch, not the gate.** Per solution, default 600 s; on timeout the child
  is killed and the old cache is left as it was. The 300 s figure in
  [Shipping gate](#shipping-gate-and-cost-model) is a *ship criterion* on the cold run, so a 450 s
  run succeeds here and fails the gate, by design. `SCOUT_SEMANTIC_TOOL` and `--timeout` are both
  proposed.
- **`import`** consumes a snapshot produced anywhere — including the committed
  `fixtures/csharp-semantic/oracle/*.jsonl` — and is the path every test in
  [Acceptance](#acceptance-for-the-implementation) uses, so the layer is testable with no .NET SDK.
  Exit 0 on success, 1 on malformed input: a record missing any of the 17 `RefRecord` keys, or
  carrying an unknown one.
- **`status`** prints on stdout one line per header field (`schema`, `oracle.version`, `hashedAt`,
  `projectModel` current or stale, `solutions`), then `files: current N, stale M, uncovered K`,
  then one line per unit whose `status` is not `"ok"` or whose `diagnostics` exceeds zero. Exit 0
  when current, 1 when stale or absent.
- **`clear`** removes the cache file **only**, exits 0, prints nothing when there was none;
  `semantic-applied.json` self-heals on the next `map` (sidecar present, nothing wanted, rebuild,
  deleted).
- **`map --no-semantic`** (a **new** flag) ignores the cache and writes no `stats.semantic`.
  `cmd_map` (`src/cli.rs:927-931`) filters `--refresh` out of its argument list and treats
  everything else as a scope directory, so the flag joins that filter and rides on a second
  `MapOptions` field (`src/mapcmd.rs:167`); `MapOptions::from_env` is unchanged, this being a flag
  rather than an environment switch.

**What `map` prints.** One stderr line, only when there is something to say: `semantic: applied
<recordsApplied> records from <filesCurrent> files (<filesStale> stale)` when the layer was
applied, `semantic: cache ignored (<reason>)` — one of `schema`, `records schema`, `project model`,
`parse`, `mtime reuse` — when a gate refused it, nothing when no cache exists.
`MapReport::summary_line` (`src/mapcmd.rs:237`), which is what `map` writes to stdout, is
unchanged.

### Failure modes

Every row is the layer failing **open**: the graph is still built, and the worst outcome is today's
graph.

| Condition | What `map` does | What the user sees |
| --- | --- | --- |
| No cache file | Builds with the syntax tiers only; writes no `stats.semantic` | Nothing; graph.json byte-identical to a no-layer build |
| `dotnet` missing (`semantic run`) | Not involved — `map` never spawns it | `run` exits 2, one line; existing cache byte-identical |
| `SCOUT_SEMANTIC_TOOL` unset (`semantic run`) | Not involved | `run` exits 2, one line; existing cache byte-identical |
| Unrestored solution | Applies whatever records the run produced | Records are `ambiguous` or absent, so inert by D3; `status` shows the unit's `diagnostics` |
| Partial solution load under `--strict` | Not involved | Oracle exits 2, cache untouched, `run` reports it |
| A unit with `status != "ok"` in a non-strict import | Every record from that unit is inert, positive or negative | They count in `recordsIgnored`; `status` lists the unit |
| `run` timeout | Not involved | Child killed, old cache intact, one line naming the elapsed limit |
| Unwritable graph directory | `map` fails as it already does for any artifact it cannot write | The existing write error |
| `oracle.recordsSchema` mismatch | Whole-layer gate fails; layer treated as absent | One stderr line; graph identical to a no-layer build |
| Project model changed since the run | Whole-layer gate fails; layer treated as absent | One stderr line; `status` exits 1 |
| One file edited since the run | Drops that file's records only; counts it stale | `stats.semantic.filesStale` ≥ 1; that file resolves by syntax |
| Another worktree on another branch, same git common dir | Applies only records whose per-file hash matches that tree | Files that differ fall through; the shared cache is safe by construction |
| Corrupt or truncated cache JSON | Treated as absent | One stderr line; `map` still succeeds |
| Two units disagree on one site | Stores both records verbatim | The join key is inert by D3; the records count in `recordsIgnored` |
| `SCOUT_MTIME_REUSE=1` | No file bytes are read, so the layer is disabled | `semantic: cache ignored (mtime reuse)` only if a cache file exists, else silence; the sidecar rule sees "nothing wanted", so the mode switch costs one rebuild |
| Scoped `map` (`map src`) | Consults the layer only for files that run walks | Out-of-scope entries are neither current nor stale and are counted in neither |
| Every cached file stale | Layer present, applies nothing | `stats.semantic` written with `recordsApplied: 0`; the enriched assert fails on `tiers.semantic.edges` |
| A solution outside the root | The oracle emits no record for its documents | `Relative` returns null outside `--root`, so those files simply never appear in the cache |
| A repository with only `.slnx` / `.slnf` | Not involved | `run` discovers them like a `.sln` |
| `map` reads while `run` renames | Reads either the old cache or the new one, never a partial file | Self-healing: the sidecar id will not match the new cache, so the next `map` rebuilds |

## Shipping gate and cost model

**D10 (settled).** The layer ships only if, on the public corpus and **after the
delegate-parameter lambda rule has landed and been measured**, all four hold:

| Gate | Threshold |
| --- | --- |
| Recall gain | enriched recall precise+ext ≥ syntax precise+ext **+ 0.10 absolute** |
| Recall floor | enriched recall precise+ext ≥ **0.70** — the rule already on record |
| Precision hold | precise precision within **0.005** of the syntax-only figure |
| Cost | cold oracle run ≤ **300 s** wall and ≤ **2×** `dotnet build -c Release` of the same solution, warm restore, same machine, in the same sitting as the oracle run |

Predictions are registered in the results document **before** the run, as
[`docs/benchmarks/methodology.md`](../benchmarks/methodology.md) already requires of every family
there, and the enriched lane is reported beside the syntax lane, never netted into it. If the gate
fails, the layer is not shipped and this document is the record of the attempt. Rejected: shipping
on the fixture alone — it cannot discriminate. `recall.all` is already 1.0, and of 66 records in
total only 36 are `access` and in-tree, with recall-D narrowing further by `target_known`: a
denominator well under half the file, over six toy projects, so every gate row is already satisfied
or unmeasurable.

### Cost, measured and unmeasured

Measured on the committed fixture, one developer machine, .NET SDK 9.0.305:

| Measurement | Value |
| --- | --- |
| Oracle cold run, fixture solution | 3.76 s wall (3.21 s user); second run 3.18 s |
| Output | 66 refs, 6 units, 0 failed units, 0 workspace diagnostics |
| Reproducibility | byte-identical run to run, and to the committed snapshot |
| Restore | tool 1.0 s with a warm package cache; fixture 1.1 s |

On the pinned public corpus the cold oracle run is **not yet measured**, against the 300 s cap. The
fixture is six small projects: it establishes that the run is reproducible and that restore is not
the dominant cost, and nothing at all about a real solution. The cap exists so the layer cannot
quietly become a build-time dependency — a re-run is always a cold run by D14, so 300 s is 300 s
every time the cache is refreshed.

## Acceptance for the implementation

**D13 (settled).** Named tests, all runnable **without `dotnet`**: they consume the committed
snapshot `fixtures/csharp-semantic/oracle/refs.jsonl` (66 records) and `units.jsonl` (6 units)
through `import`, plus hand-written cache fixtures. `tests/semantic_audit.rs` is the precedent — it
scores the fixture from the committed snapshot with no SDK present.

| Test | What it proves |
| --- | --- |
| `semantic_cache_round_trips_the_fixture_records_byte_identically` | serde round-trip of the cache over all 66 records, byte-identical output |
| `imported_cache_replaces_guess_and_ext_edges_with_semantic_edges` | every site that had a `guess`/`ext` edge and a positive record carries exactly one `source: "semantic"` edge and no heuristic edge |
| `precise_edges_are_unchanged_with_the_layer_on` | the precise rows are byte-for-byte what they were |
| `a_negative_record_silences_a_leaked_guess` | a negative record removes the site's `guess` edge and `silent.leak` falls from 1 to 0 on the hand-written pair |
| `an_ambiguous_record_changes_nothing` | inert class, D3 |
| `a_bare_positive_record_emits_and_a_bare_negative_does_not` | the shape asymmetry, D3 |
| `a_stale_file_hash_falls_through_to_the_syntax_tiers` | per-file validity, D5 |
| `a_stale_project_model_disables_the_whole_layer` | whole-layer gate, D5 |
| `a_missing_cache_builds_a_byte_identical_graph` | omit-when-absent, D7/D8 |
| `no_semantic_flag_builds_a_byte_identical_graph` | `map --no-semantic` |
| `importing_a_cache_forces_the_next_map_to_rebuild` | the `NotRebuilt` short-circuit does not fire |
| `removing_the_cache_forces_the_next_map_to_rebuild` | the "sidecar present, nothing wanted" half of D7 |
| `audit_reports_a_semantic_row_at_precision_one` | the `semantic` row, D9 |
| `audit_on_the_syntax_lane_is_unchanged_by_the_cache_on_disk` | a cache present but `--no-semantic` gives byte-identical audit output |
| `audit_on_the_enriched_lane_counts_semantic_edges_as_precise_class` | a semantic hit counts in `recall.precise`, `recall.precise_ext` and `recall.all` |
| `semantic_run_without_a_tool_exits_two_and_leaves_the_cache` | D11's exit-2 path, cache byte-identical |
| `a_semantic_edge_is_a_legitimate_impact_premise` | the walk continues through it, unlike a guess (precedent `tests/cli_no_guess.rs`) |
| `find_ranking_counts_a_semantic_edge_as_precise_inbound` | precise-class ranking (precedent `tests/cli_find_ranking.rs`) |
| `schema_gate_disables_the_layer` | whole-layer gate, D5 |
| `records_schema_gate_disables_the_layer` | whole-layer gate, D5 |
| `a_corrupt_cache_is_treated_as_absent` | one stderr line, `map` still succeeds |
| `an_external_record_from_a_unit_with_diagnostics_is_inert` | the negative rule's unit clause |
| `a_record_whose_target_the_graph_does_not_know_is_inert` | `target_known`'s clause |
| `one_semantic_edge_per_join_key_and_target` | the `(file, startLine, member, target)` seen-set |
| `an_enum_member_target_binds_to_the_exact_id_when_it_exists` | the exact-id-wins rule |
| `conflicting_targets_from_different_units_are_inert` | the cross-unit conflict rule, through `import` |
| `import_refuses_a_record_with_a_missing_key` | the 17-key validation, exit 1 |
| `status_exits_one_when_stale_and_zero_when_current` | `semantic status` |
| `clear_removes_the_cache_and_the_next_map_rebuilds` | `clear` plus the sidecar's self-heal |

Note on the leak test: the committed fixture asserts `"silent.leak": {"max": 0}` in
`fixtures/csharp-semantic/expected.json` and passes it today, so there is no leaking site in the
snapshot to silence. The test builds one, from a **proposed** hand-written trio at
`fixtures/csharp-semantic/enrichment/{cache.json,refs.jsonl,units.jsonl}` — also where every other
hand-written cache above lives — so no test needs the committed snapshot to change.

**CI.** The semantic job (`.github/workflows/ci.yml`) gains one step after the existing
snapshot-diff and audit steps: run `semantic import` on the oracle output the job just produced —
with **no `--files`** — rebuild the isolated fixture copy's graph, and audit it against the **new**
`fixtures/csharp-semantic/expected-enriched.json`. Importing without `--files` is sound because of
how the job builds that copy: `rsync -a --exclude bin --exclude obj` reproduces the same files
under the same root-relative layout the oracle walked, so hashing the tree at import time yields
the hashes the oracle would have written. Cache and audited output are the same run's, which is
what makes the `semantic` row's `min 1.0` a real assertion. Every existing step is untouched — the
oracle step, the `diff -u` against the committed snapshot, and the `--assert` against
`expected.json`, which remains the syntax lane.

**Documentation the implementation must update:**

- `README.md` — "Where it stores things" (the `semantic-v1.json` line loses "planned", and
  `semantic-applied.json` joins it), "Environment variables" (`SCOUT_SEMANTIC_TOOL`), and the
  Limitations bullets on tiers and on the reserved `source` slot.
- `CHANGELOG.md`, and `docs/benchmarks/methodology.md` — where the enriched lane is defined as a
  **separate lane**, never mixed into the syntax runs.

## Non-goals and neighbouring work

**D14 (settled).** Not in scope, each for a stated reason:

- **Implementing any of this.** This document settles shape, not code.
- **Roslyn in the binary, or anything on the hook path.** The oracle stays out of process, kept out
  of the crate by `Cargo.toml`'s `exclude`; the hooks never spawn it.
- **Changing `RefRecord`.** `--files` and `--version` are additive outputs; the 17-key record is
  the contract and does not move.
- **TypeScript.** A type-aware sidecar for TS is a separate `ROADMAP.md` item on a different stack,
  sharing nothing with this but the word "sidecar".
- **The tool's second output mode for another consumer's fact schema.** Tracked on its own.
- **The delegate-parameter lambda rule.** Measured first and separately; the two never ship in one
  measurement.
- **Re-scoring published runs.** Runs 0–4 stand as measured; the enriched lane is new columns.
- **Shipping or downloading the oracle.** The user builds it, or does not use the layer.
- **Incremental compilation inside the oracle.** A re-run is always a cold run; incrementality
  lives entirely in per-file cache validity, where a hash decides it rather than a build system's
  own idea of what changed.

## Amendment (2026-09-17): the admitted-artifact concern is superseded

This section amends the document above rather than editing it silently. Everything above this
line stands as originally written and still describes the per-site resolver-override cache this
document proposes (`semantic-v1.json`, `SCOUT_SEMANTIC_TOOL`, `devscout semantic run|import|status`)
-- none of which has shipped.

A separate, now-implemented concern is a versioned, atomically admitted artifact carrying a
one-shot engine run's own symbol identities, diagnostics and compilation-context health, produced
either by a local engine run or by a build/CI-produced import, and admitted through one Rust
path. That artifact is `compiler-facts-v1.json`, its own file beside `graph.json`, admitted by
`devscout compiler-facts run|import|status` -- see [`README.md`](../../README.md#compiler-facts).
It supersedes this document's proposed cache shape for the admitted-artifact concern specifically:
a resolver-override cache still needs everything `D4` above proposes (a per-site override keyed to
resolver behaviour), but the shape of "the versioned file an engine run produces and this crate
admits" is now the shipped `compiler-facts-v1.json` contract, not a fresh `semantic-v1.json` design.
Any future implementation of the resolver-override cache this document proposes should read the
admitted `compiler-facts-v1.json` artifact as its own compiler-fact source rather than defining a
second admission path.

Nothing in `D4` through `D14` above is retracted; this amendment narrows only which artifact
"the versioned file beside the fragments cache" now names for the admission concern, and leaves
the resolver-override cache proposal itself open and unimplemented.
