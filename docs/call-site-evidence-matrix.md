# Call-site evidence capability matrix

What each shipped native answer surface, the persisted graph, and the optional `flowtrace-facts`
sidecar say about one C# invocation: caller identity, the invocation's own source range and
occurrence, the callee's declaration/candidate identity, resolution strength, source
revision/content identity, and supported control/await information. Every cell below names a
concrete witness (the command, the fixture and the exact key that carries it) or an explicit gap
(the absent field and where the absence is recorded).

Every witness is a real run against `fixtures/call-site-evidence/Witness.cs` (native surfaces) or
the committed `fixtures/csharp-flowtrace/facts.json` (optional surface). This document's own bytes
are produced by `tests/call_site_evidence_matrix.rs::docs_matrix_matches_the_committed_snapshot_or_is_regenerated_with_bless`,
which runs every audited command fresh and asserts the load-bearing fact behind each
witness/gap cell against that live output before writing the sentence naming it -- a hand edit
to this file, or a behavior change that would silently invalidate a claim below, fails `cargo
test` and must be regenerated with `BLESS=1 cargo test --test call_site_evidence_matrix`, the
same generate-then-byte-diff shape `semantic-audit` already applies to
`fixtures/csharp-flowtrace/facts.json`. Two cells -- the `flowtrace-facts` row of "Invocation
source range and occurrence" and of "Resolution strength" -- describe flowtrace-cli's own
consumer-side behavior and are cited from its source rather than re-run here; both say so
where they appear.

Audited native surfaces: the `--json` answers of `refs`, `read`, `impact`, `tests` and `find`, and
the persisted graph's own `edges` array (`.scout/graph/graph.json`). Audited optional surface:
the `flowtrace-facts` sidecar (`fixtures/csharp-flowtrace/facts.json`), produced by
`tools/scout-semantic` and consumed by flowtrace-cli.

## Caller identity

| Surface | Witness / gap |
| --- | --- |
| `refs --json` | Witness: every inbound/outbound row's `file` key names the calling file. `refs Record --json` -> `members[0].inbound["uses-member"].rows[*].file == "Witness.cs"`. No caller *symbol* (which method the call sits in) is carried -- a gap: only the file and line are recorded, never an enclosing-def id. |
| `read --json` | Same as `refs` (reuses `refs`' inbound machinery); the declaration span's own `file`/`startLine`/`endLine` additionally names the *callee's* declaring file, a different fact from caller identity. |
| `impact --json` | Witness: a `hit` answer's `rows[*].file` names an affected file (see the repository's own worked example, `docs/answer-contract.md`). Gap: `impact` reports file-level blast radius, not a per-invocation caller; no `fromLines` entry names which specific call inside that file. On this fixture, `impact Ledger --json` is itself a real `zero-hit`: its only callers share `Ledger`'s own file, which `impact`'s walk always excludes from the affected set. |
| `tests --json` | Witness: `rows[*].file` names the covering test file; `rows[*].lines` names the referencing lines inside it (`tests Ledger --json` on this fixture returns 0 rows -- `Witness.cs` declares no test attribute, a true negative, not a gap). |
| `find --json` | Gap, structural: `find` has no `--json` flag parser, so `--json` joins the query text as a literal token (`src/cli/dispatch.rs`); it answers no structured, machine-readable invocation evidence either way. `find Record --json` on this fixture prints non-JSON text to stdout (`Witness.cs:6: class Ledger; Record | class Caller; RepeatedCallsDifferentLines, TwoCallsOneLine, OverloadAmbiguity, Recurse, AwaitedSequence, ParallelLaunchJoin`) -- a fuzzy name/purpose match, never a JSON object, so it carries no call-site evidence in the vocabulary this matrix audits. |
| persisted graph edges | Witness: every edge object's `from_file`/`from_line` name the calling site. `.scout/graph/graph.json`'s `edges[*]` for this fixture. Same caller-symbol gap as `refs`: an edge names no enclosing def. |
| `flowtrace-facts` | Witness: a `consume`/`publish`/`method_call`-kind fact's `file`/`line`/`consumer`-or-`class` names the caller and its file. `method_call` (`class`, `method`, `field`, `calledMethod`) was a declared-but-unpopulated `FactSchema.cs` slot before this work; `FactsWalker.cs::EmitMethodCalls` now fills it for a body calling a member on a constructor-injected field, from either a block or an expression body, e.g. `DeliveryScheduledConsumer.NotifyLost`'s `_repository.Find(...)` (`fixtures/csharp-flowtrace/facts.json`, pinned by `tests/flowtrace_facts.rs::semantic_resolution_shows_where_regexes_stop`). Gap, still: no caller *method-argument* identity (which parameter value reached the call), out of scope here. |

## Invocation source range and occurrence

| Surface | Witness / gap |
| --- | --- |
| `refs --json` | Witness (proven gap, closed here): before `occurrenceIndex`, two calls to `Record` on `Witness.cs:40` serialized as two byte-identical row objects (`tests/call_site_evidence.rs::two_calls_on_one_line_...` fails against the tagging code reverted). After: the pair carries `occurrenceIndex: 0` and `occurrenceIndex: 1`, additive, appended last, `schema_version` unchanged. Every other named shape (different lines, overload, recursion, awaited sequence, parallel launch) already round-trips with no collision, by line alone. |
| `read --json` | Same mechanism, same fixture, a second real collision: `read Ledger --json`'s `inbound["uses-type"].rows` carries two rows at `Witness.cs:28` (the field's declared type and `new Ledger()` on the same line), both now `occurrenceIndex: 0`/`1`. |
| `impact --json` | Gap, by design: impact rows are per-file aggregates; `fromLines` names one representative line per edge kind, never every occurrence. Occurrence-identity work requires that to stay unchanged, and it is untouched here. |
| `tests --json` | Gap: a `tests` row's `lines` array lists every referencing line but carries no per-line occurrence discriminator for two references on the same line (this fixture's own test-coverage rows are empty, so this is a structural read of the JSON shape, not a fixture-proven case). |
| `find --json` | Not applicable -- `find` carries no invocation evidence at all (see Caller identity). |
| persisted graph edges | Gap, confirmed directly: `graph.json`'s two `uses-member` edges for `Witness.cs:40` are themselves byte-identical objects (`{"from_file":"Witness.cs","from_line":40,"kind":"uses-member","member":"Record","to":"Evidence.Ledger","to_file":"Witness.cs"}` twice), distinguishable only by their position in the `edges` array. `occurrenceIndex` is computed at query time from exactly this array order and is NOT written back into the persisted graph, so this gap remains at the graph layer by design (no `GRAPH_SCHEMA_VERSION` bump, no fragment-cache generation rename). |
| `flowtrace-facts` | Gap, cited from flowtrace-cli's own source, not re-verified in this repository's own tests: `factSite()` (`lib/trace.js`, flowtrace-cli) keys a fact/provider merge on `` `${type}|${file}|${line}` ``, so two facts at the same type/file/line already merge to one site on the consumer side -- out of scope here (consumer-side projection work tracked in the flowtrace-cli project). |

## Callee declaration/candidate identity

| Surface | Witness / gap |
| --- | --- |
| `refs --json` | Witness: `refs Record --json`'s `members[0].id == "Evidence.Ledger.Record"` and `sites` names both declaring lines (`8` and `12`, the two overloads) -- declaration location is a field distinct from any inbound row's `file`/`line`. |
| `read --json` | Witness: `read Ledger --json`'s `span.file`/`span.startLine`/`span.endLine` (`Witness.cs`/`6`/`20`) is the declaration span, structurally separate from `inbound.*.rows[*].file`/`line` (the invocation sites) -- declaration location has always been a distinct field, unmodified here. |
| `impact --json` | Gap: `impact` names affected files, never a callee id. |
| `tests --json` | Witness: `symbol` names the resolved callee id (`tests Ledger --json` -> `"symbol": "Evidence.Ledger"`). |
| `find --json` | Not applicable (see Caller identity). |
| persisted graph edges | Witness: an edge's `to`/`to_file` name the callee's declaring id/file; `defs[*].id`/`file`/`line` names the declaration itself, separately. |
| `flowtrace-facts` | Witness: `consumer`/`fqn` on a `consume` fact name the acting type, not a per-call target; `method_call.calledMethod` (now emitted -- see Caller identity) names the callee member by name, not a resolved id -- weaker than `refs`' graph-id identity, by design (the sidecar asserts, it does not resolve). |

## Resolution strength

| Surface | Witness / gap |
| --- | --- |
| `refs --json` / `read --json` | Witness: every row's `why` (`uses-member-precise`/`-ext`/`-guess`) and, on a heuristic row, `heuristic`/`tier`. Every row in this fixture is `uses-member-precise` -- no guessed edge exists in a package-free fixture with no ambiguity, so the guess/extension tiers are cited from `docs/answer-contract.md`'s own worked example, not re-demonstrated here. |
| `impact --json` | Witness: `rows[*].why`, `heuristicCount`/`heuristic`/`tier` when present. |
| `tests --json` | Witness: `rows[*].why` (`test-attribute`/`test-project`), `heuristic`/`tier` when present. |
| `find --json` | Not applicable. |
| persisted graph edges | Witness: an edge's absent `heuristic` key means precise; `heuristic: true` plus `tier` names the guess strength -- the source `why` is derived from. |
| `flowtrace-facts` | Gap, cited from flowtrace-cli's own source, not re-verified in this repository's own tests: a fact carries no resolution-strength field at all -- it is asserted by the optional producer, not scored; `docs/design/compiler-enrichment.md`'s D2 and `src/query/imported.rs`'s isolation keep it always below anything the engine resolved for itself. |

## Source revision/content identity

| Surface | Witness / gap |
| --- | --- |
| `refs --json` / `read --json` / `impact --json` / `tests --json` | Witness (proven gap, closed here): `src/manifest.rs::freshness_warning` already detected a changed-source-at-unchanged-HEAD condition, but only as a human-readable stderr line -- confirmed by reading `emit_freshness_warning` (`src/cli/answer.rs`), which wrote only to stderr. `src/freshness.rs::index_freshness_state` now exposes the same underlying facts as a `freshness` top-level key (`state`: `fresh`/`stale`/`unknown`, plus `indexedHead`/`currentHead`/`changedFiles` or `reason`), appended last, additive, `schema_version` unchanged. `tests/freshness.rs`'s JSON-side companion tests exercise `fresh`, `stale` (HEAD moved), `stale` (a modified indexed file) and `unknown` (`no-index-state`) against a real git fixture; every live answer this generator itself ran above also carries the key. |
| `find --json` | Not applicable -- `find` has no `--json` shape at all to carry `freshness` in. |
| persisted graph edges | Witness: `graph.json`'s own `built_at_head` field (`null` on this fixture -- a non-git root) is the graph's own revision anchor. |
| `flowtrace-facts` | Witness: `facts.json`'s top-level `compilation`/`generatedFrom` name the producing build; consumer-side identity mismatch detection (`lib/facts.js`'s `staleFactsWarnings`, flowtrace-cli) is already shipped and cited, not re-implemented. |

## Supported control/await information

| Surface | Witness / gap |
| --- | --- |
| `refs --json` / `read --json` / `impact --json` / `tests --json` | Gap, confirmed by `tests/call_site_evidence.rs::no_answer_this_fixture_produces_ever_claims_an_unmodeled_control_or_dispatch_fact` (re-run independently by this document's own generator, above): none of `branchPoint`/`paramSource`/`exceptionMap`/`callOrder`/`awaitOrder`/`sequence`/`dispatchTarget` ever appears, on any of the four audited verbs, for this fixture's awaited-sequence or parallel-launch-join witnesses. An `await` or a parallel launch is recorded only as an ordinary `uses-member` call site, indistinguishable in shape from a synchronous one -- native devscout models no control-flow or await-ordering fact at all, a structural non-goal, not an oversight. |
| `find --json` | Not applicable. |
| persisted graph edges | Gap: `Edge`'s kinds (`src/graph/edge.rs`) carry no control/await tag. |
| `flowtrace-facts` | Gap: none of the 9 fact kinds this repository's fixture emits (`consume, ctor_field, di_binding, iface_impl, message_class, method_call, method_span, publish, route`) carries an await or branch fact -- `method_call` included: it names a call, never an ordering or a branch. `branch_point`/`param_source`/`exception_map` stay the sidecar's documented non-goals; confirmed absent by direct inspection of the committed snapshot's fact-kind set. |

## Supported-shape table: which fixture shapes each mode establishes

| Fixture shape | Native (`devscout`, no .NET SDK) | Optional (`flowtrace-facts`, dotnet-gated) |
| --- | --- | --- |
| A call on a plain local/field (not ctor-injected) | Yes -- every `uses-member` edge, regardless of how the receiver got there. | No -- `method_call` is scoped to a constructor-injected field only, per its documented shape. |
| A call on a constructor-injected field, block-bodied method | Yes, same as above (no special case). | Yes -- `method_call`. |
| A call on a constructor-injected field, expression-bodied method (`=>`) | Yes, same as above (no special case). | Yes -- `method_call` reads either a block or an expression body; a proven gap closed here (`EmitMethodCalls` used to return early on any expression-bodied method), witnessed by `ParcelsController.HasRecord`. |
| A constructor-injected field assigned in a different file of a partial class | Yes -- extraction has no such restriction. | No, by construction: `EmitMethodCalls`/`InjectedFields` only read a constructor declared in the SAME `SemanticModel`'s syntax tree as the calling method; a Roslyn `SemanticModel` cannot resolve symbols in another file's tree without a second model lookup this sidecar does not add. Recorded as a known, accepted miss, not silently swallowed. |
| Two calls to one target on one line | Yes -- `occurrenceIndex`. | Not proven here: `factSite()` (flowtrace-cli) merges same-line facts of the same type before this question is even reachable; out of scope (flowtrace-cli's own consumer-side projection work). |
| Every native-surface acceptance case here, with the .NET SDK absent | Yes -- confirmed by running the toolchain-free suite (`cargo test`, no `dotnet` on `PATH` needed) against every case above. | Not applicable -- the optional surface's own tests read a committed snapshot (`tests/flowtrace_facts.rs`), never invoke `dotnet` either, but the snapshot itself is produced by a separate, dotnet-gated regeneration (`semantic-audit` in `ci.yml`). |

## Report: supported/expected call sites, incorrect targets, unknown relations, per mode

Raw denominators, never a single blended figure across native and optional modes.

| Mode | Case | Expected sites | Found | Incorrectly asserted targets | Unknown relations |
| --- | --- | --- | --- | --- | --- |
| Native | Repeated calls, different lines | 2 | 2 | 0 | 0 |
| Native | Two calls, one line | 2 | 2 (distinguished by `occurrenceIndex`) | 0 | 0 |
| Native | Overload ambiguity | 2 | 2 (which overload bound: unknown, recorded as a gap, never asserted) | 0 | 1 (overload identity) |
| Native | Recursion (`this.`-qualified) | 1 | 1 | 0 | 0 |
| Native | Recursion (unqualified, not shipped) | 1 | 0 | 0 | 1 (no ref extracted at all -- see `fixtures/call-site-evidence/EXPECTED.md`) |
| Native | Awaited sequence | 2 | 2 | 0 | 1 (await order) |
| Native | Parallel launch/join | 2 | 2 | 0 | 1 (launch/join relation) |
| Optional (`flowtrace-facts`) | `method_call` (this repository's own fixture) | 6 (ctor-injected-field calls found by direct inspection: 2 in `DeliveryScheduledConsumer`, 3 in `ParcelsController`, 1 in `GetParcelHandler`) | 6 | 0 | 0 |

**Outcome is mixed:** occurrence identity, revision/content identity reaching `--json`, and
`method_call` (block- and expression-bodied alike) are the additive-change branch -- each closes a
real, narrow gap this fixture's own results proved, with no schema-version bump. The other two
named revision-identity cases (enrichment-identity mismatch, absent provider) are the
documented-mapping branch: existing flowtrace-cli behavior already satisfies them, cited above,
not re-implemented. The optional sidecar's committed snapshot carries 152 facts total across
9 kinds (`consume, ctor_field, di_binding, iface_impl, message_class, method_call, method_span, publish, route`). No cell in this report blends a native-mode count with an
optional-mode count, or an "already worked" cell with a "we built this" cell, into one number.
