# Results — Precise-tier wrong-target attribution, 2026-09-24

Attributes every precise-tier `wrong-target` false positive on the pinned MassTransit corpus to
the resolver mechanism that produced it, before any of those mechanisms is changed. Companion to
[`2026-09-resolver-precision.md`](2026-09-resolver-precision.md), which this record does not edit
(one file per run, never edited in place).

## Environment

```
Date              2026-09-24
Corpus            MassTransit/MassTransit @ 855cf1752c94ca9498e0c45ce8d09fdc9e957dd6 (bench/corpus.lock)
                  a fresh copy per arm, with that commit as HEAD and no .git/scout;
                  no existing corpus checkout was mapped
Oracle            tools/scout-semantic output for the pinned corpus
                  refs.jsonl  sha256 f9b579f0629a0b930f37723edab102d0a185fc07971582988cc4272e82f5c3a6
                  units.jsonl sha256 3b7137cfff70996059ae07cadd6ff55e315f9fdaa29ef01ce1c1f0a4bc2c8fc6
Binary source     commit 4822530db2d37dd6678798ea30deb9ed3cd1f101; `devscout --version`
                  reads 0.6.0, the version before this release's bump
Binary build      `git archive` export of that commit, `cargo build --release --locked -j 4`,
                  isolated HOME/TMPDIR/CARGO_TARGET_DIR/SCOUT_REGISTRY/SCOUT_CONTENT_DB, offline
Toolchain         rustc 1.97.1 (8bab26f4f 2026-07-14), aarch64-apple-darwin
Binary identity   pinned by source commit, toolchain and build command, not by file digest:
                  the Mach-O LC_UUID, and with it the sha256, depends on the build
                  directory. Two clean builds of the commit above into different target
                  directories differ in 48 bytes (same size, identical strings) and produced
                  byte-identical graph.json, provenance and --fp-sites output. The two
                  measured binaries: sha256
                  ef6b5f040189b64d4f7e05152859ad3eec41fe75089002098cdd0f51359e8e95 and
                  f588a92775df05e5a687fd76a380ba3bf864901316b65c5d1244234837ae721e
Fragment cache    fragments-v21.json
Provenance gate   SCOUT_EDGE_PROVENANCE, set on one fresh corpus copy and unset on another
```

## Measured population

| Figure | Value |
| --- | --- |
| MassTransit graph.json sha256 | `f6523adde51480cf4fb9e051f529e7ab741c353063e3db1e59b018b6ee8da999` |
| graph | 5634 files, 9951 defs, 135918 edges, 43523 `uses-member` |
| precise edges / tp / fp / precision | 31240 / 31026 / 214 / 0.993 |
| precise `fp:no-site` / `fp:external` / `fp:wrong` / `structural` | 44 / 0 / 170 / 0 |
| sorted precise wrong-target `--fp-sites` rows, sha256 | `f9698caf6431061ff7bf5190cc9ccf7fd64f7cfde2241dfe7794042122dceb5b` |

The sorted-row digest is `grep '"tier":"precise"' | grep '"class":"wrong-target"' | LC_ALL=C sort |
shasum -a 256` over the `--fp-sites` output. Audit text, `--json` and `--fp-sites` output are
identical between the gate-set and gate-unset copies once the copy's own root path is replaced.

## Corpus-level provenance trust (checked before any row is attributed)

The per-site false-positive sink and the env-gated resolver provenance were proven only on two
small fixtures when they were built. This run re-proves both properties on the corpus itself:

1. **Graph identity across the gate.** The gate-set and gate-unset copies produce byte-identical
   `graph.json` (sha256 above). The instrumentation changes nothing the resolver emits.
2. **Full coverage.** The gate-set run wrote 43,523 provenance rows for the graph's 43,523
   `uses-member` edges, with no null step and no step outside the seven arms of
   `src/resolve/provenance.rs`: `typed-receiver` 24115, `scored` 9195, `qualifier-type` 5786,
   `extension` 2563, `qualifier-member` 825, `property-hop` 730, `base-member` 309.
3. **Join completeness.** Each of the 170 precise `wrong-target` rows joins to exactly one
   provenance row by `(file, line, member, bound target, tier)`.

## Method

**Pre-sort.** Four encoded checks were run against the bound and the expected target of every
row, reading `graph.json` and `fragments-v21.json`:

- *member* — the target, or a def reached from it through its recorded base names (breadth-first,
  six levels), lists the member among its methods, properties, fields or non-public methods;
- *type arguments* — when the site's ref records a `typeArgCount`, it equals the target type's own
  type-parameter count;
- *project* — the site's project is the target's, or reaches it through the transitive
  `ProjectReference` closure;
- *namespace* — the target's namespace is a `using` of the site's file, or the file's own
  namespace, an ancestor of it, or a namespace nested inside it.

A bound target failing any check reads `defect`; a bound target passing all four with an expected
target failing one reads `oracle-wrong`; anything else is non-decisive. Result: 111 non-decisive,
52 `defect`, 7 `oracle-wrong`. The pre-sort outcome is kept per row in the `check` column; it
decided nothing.

**Per-site adjudication.** Every one of the 170 rows was then read at its own call site in the
corpus source, with the receiver's declared type, every candidate declaration, the ref's own facts
in `fragments-v21.json` and the emitting step from the provenance row: first which member C# binds
there, then which resolver rule produced the different target. Both the verdict and the mechanism
come from that reading; `evidence` is `source-adjudicated` on all 170 rows.

- The 52 pre-sort `defect` verdicts all held.
- None of the 7 pre-sort `oracle-wrong` verdicts held. Three (`EndpointConfiguration.cs:88`,
  `TimelineExtensions.cs:38` and `:41`) failed the type-argument check only because it compared the
  qualifier's written count with the type-parameter count of a generic interface the member is
  reached through by inheritance. Four (`ServiceBusReceiveEndpointBuilder.cs:28`, `:51`, `:60` and
  `AsyncMessageList_Specs.cs:84`) failed the namespace check only because a member reached through
  a receiver's type needs no `using` for the namespace that declares it. All seven are `defect`.

**Result: all 170 rows are `defect`. Zero rows are `oracle-wrong`.**

## Defect groups

Nine mechanisms; sizes sum to 170. Each row's `mechanism` names its group.

| Group | Sites | Mechanism | Owner in `src/resolve/` | Class | Recall risk |
| --- | --- | --- | --- | --- | --- |
| `argument-type-unchecked` | 74 | A same-named overload is admitted on member name and value-argument count; the written arguments' types are never compared with its parameter types, so an overload that rejects them (a `string` for `StopContext`, `TimeSpan` for `DateTime`, `IPipeSpecification<ConsumeContext>` for `IPipeSpecification<ConsumeContext<T>>`) binds ahead of the extension, base or sibling-interface overload that accepts them. 15 of these are `Apply` overloads declared in two sibling interfaces, which no hiding rule can separate. | `members.rs` `declares_member` with `arity.rs` `method_arity_admits`, as called by `declares_here_for_ref` and the `typed_receiver_base_member` walk predicate | new fact: the static type of each argument where it is known | high |
| `type-argument-count-unchecked` | 43 | A call's explicit type-argument count (`AddConsumer<TConsumer, TDefinition>()`) is never compared with the candidate method's own generic-parameter count, so the interface method (one type parameter for `AddConsumer` and `AddFuture`, two for `AddSagaStateMachine`) binds ahead of the extension that takes the two or three written. | `members.rs` `declares_member` with `arity.rs` `method_arity_admits` | new fact: the call's method type-argument count and each overload's generic-parameter count | low |
| `hidden-member-reached-first` | 24 | When the receiver's type does not declare the member, the base walk returns the first declaring def in depth-first declaration order; a more-derived `new Topology` reached through a later base hides the one it returned. All 24 are the `Topology` property. | `members.rs` `first_base_declaring` / `declares_in_base_closure`, via `typed_receiver_base_member` | rule-only: drop any declaring def that is itself a base of another declaring def | medium |
| `explicit-implementation-as-member` | 6 | Explicit interface implementations are recorded as ordinary members: an explicit property's type wins the first-declaration slot over the public property of the same name, an explicit property supplies a receiver fact for a bare name that should name a static class, and explicit methods are admitted by the `this.` lookup. | consumers `members.rs` `declares_member_any_visibility` and `typed_receiver_precise_target`, and the property hop in `assembly.rs` `resolve_graph_with_model`; the facts come from `raw_property_types`, `raw_non_public_method_names` (`src/extract/members.rs`) and `collect_declared_member_facts` (`src/extract/receivers.rs`) | new fact: mark or omit explicit implementations | low |
| `inherited-member-read-as-type` | 6 | A bare qualifier that names an inherited property (`RoutingSlip`, `Filter`) is resolved as the same-named type imported by a `using` or declared as the enclosing type; C# finds the member first. | `assembly.rs` `resolve_graph_with_model` type-qualifier arm, admitted by `ladder.rs` `type_qualifier_arm_admits`; the member facts it needs are read by `receiver.rs` `bare_receiver_field_or_property_type` | rule-only | low |
| `using-before-enclosing-namespace` | 6 | The ladder answers a bare type name from the file's `using` directives before the enclosing namespace, so `TestInstance` imported from `TestFramework.Sagas` wins over the `TestInstance` declared in the site's own namespace. | `ladder.rs` `resolve_ref` (step 2 before step 3) | rule-only for these sites; the general order interleaves each namespace level's types with that level's `using` directives, which needs each directive's level | medium |
| `qualified-receiver-type-truncated` | 5 | A receiver declared with a qualified type (`Outbox.OutboxSendEndpoint`, `ConfigurationHostSettings.ConfigurationBatchSettings`) is recorded by its last segment, which the ladder then resolves to a different same-named type. For 4 of the 5 that type sits in a project the site's project cannot reference, answered by the graph-wide-unique step. | consumer: the typed-receiver probe in `assembly.rs` `resolve_graph_with_model` through `arity.rs` `resolve_receiver_type`; the fact comes from `base_type_identifier` (`src/extract/refs.rs`) | new fact: the receiver type's written qualifier | low |
| `lambda-applicability-unchecked` | 5 | A lambda argument is checked against a candidate overload only by the parameter count of the delegate it would convert to, and 3 of the 5 come from the property hop, which does not apply even that check. At 4 sites (`harness.Published/Sent/Consumed.SelectAsync(_ => true)`, `messageList.Any(m => m switch { … })`) the bound type declares two overloads that pass it: `SelectAsync`/`Any(Action<…Filter>, …)`, which the value body (`true`, a switch expression, neither a statement expression) cannot convert to, and `SelectAsync<T>`/`Any<T>(FilterDelegate<I…Message<T>>, …)`, which the one-parameter, `bool`-valued lambda fits by count and by return type; C# rejects that one only because `T` occurs solely in the delegate's parameter type, from which an implicitly typed lambda gives type inference nothing to fix it. A check of the body's return shape alone leaves these 4 sites bound to the same wrong target. At the fifth (`configurator.ClassMap(_ => classMapConfigurator)`) the interface's `ClassMap(Func<IServiceProvider, BsonClassMap<TSaga>>)` expects a value and gets one, a parameter declared `Action<BsonClassMap<TSaga>>`; only the body's type compared with the delegate's return type rejects it. | `lambda_arity.rs` `lambda_arity_admits`, whose per-overload predicate has to carry every check; the property hop in `assembly.rs` `resolve_graph_with_model` calls `declares_member` without it and walks no base, so at its 3 sites a correct check removes the wrong edge without producing the right one | new facts: whether a lambda is implicitly typed and whether its body yields a value (4 sites); each overload's own method type parameters and where they occur in its parameter types, which the recorded `*` does not tell apart from the declaring type's (4 sites; the per-overload generic-parameter fact `type-argument-count-unchecked` also needs); the static type of the body's value (1 site; the argument-type fact `argument-type-unchecked` needs) | medium |
| `property-hop-arity-blind` | 1 | The property hop resolves a property's declared type (`IReceivedMessageList<TMessage>`, type arguments recorded) without its type-argument count and lands on the non-generic sibling, which is not in the receiver's hierarchy; it then checks the member with `declares_member` alone. | `assembly.rs` `resolve_graph_with_model` property hop; `arity.rs` `resolve_receiver_type` and `members.rs` `declares_here_for_ref` are what the typed-receiver tier uses for the same two steps | rule-only | low |

Emitting steps by group: `argument-type-unchecked` 67 `typed-receiver`, 6 `qualifier-type`, 1
`property-hop`; `explicit-implementation-as-member` 3 `typed-receiver`, 2 `property-hop`, 1
`qualifier-type`; `inherited-member-read-as-type` 6 `qualifier-type`;
`lambda-applicability-unchecked` 3 `property-hop`, 2 `typed-receiver`; `property-hop-arity-blind` 1
`property-hop`; every other group is all `typed-receiver`.

## Recommended sequence

Rule-only changes first, then the extractor facts, with the central member-declaration check
last, following the precedent that a change touching many sites is proven on its narrowest slice
before it is widened:

1. **property-hop-arity-blind** (1, rule-only, low) — the property hop takes the arity-aware
   receiver resolution and the full declaration gate the typed-receiver tier already uses.
2. **inherited-member-read-as-type** (6, rule-only, low) — the facts are already read for bare
   receivers; only the order of the type-qualifier arm changes.
3. **hidden-member-reached-first** (24, rule-only, medium) — the largest rule-only group; every
   site is a property, so the rule can start with properties and fields before it touches methods,
   where only a same-signature redeclaration hides.
4. **using-before-enclosing-namespace** (6, rule-only, medium) — small, but it reorders every type
   resolution, so it is measured graph-wide before anything else lands on top of it.
5. **explicit-implementation-as-member** (6) and **qualified-receiver-type-truncated** (5) — new
   extractor facts, each low risk; they can share one fragment-cache generation bump.
6. **type-argument-count-unchecked** (43, new fact, low) — mechanical once both counts are recorded,
   and it only refuses a candidate when both are known. The per-overload generic-parameter fact it
   records, kept as the list of the method's own type parameters, is what item 7 reads.
7. **lambda-applicability-unchecked** (5, new facts, medium) — after 6, because four of its sites
   need each overload's own method type parameters as well as whether the lambda's body yields a
   value; the three property-hop sites also need item 1's declaration check in the hop, and a base
   walk in the hop to bind the right target rather than none. Refusing an overload whose method
   type parameter cannot be inferred is the riskier half, since it drops a whole overload: it
   applies only when the call writes no type arguments and every occurrence of that type parameter
   sits in the parameter types of the delegate an implicitly typed lambda converts to. The fifth
   site needs the static type of the body's value, the fact item 8 introduces for arguments, and
   lands with it.
8. **argument-type-unchecked** (74, new fact, high) — the only group that touches the member-
   declaration check for arbitrary argument shapes. Split it by how the argument's type is known:
   10 sites are decided by an argument whose type its syntax alone gives (a string literal, an
   interpolated string, `typeof`); the rest need a declared local, field or parameter type, a
   member's return type, or generic instantiation against the receiver's type arguments. The check
   has to run inside the base walk's predicate, so that a rejected `Apply` on one sibling interface
   lets the walk continue to the other.

## Reproduction

```
git archive 4822530db2d37dd6678798ea30deb9ed3cd1f101 | tar -x -C "$EXPORT"
cd "$EXPORT"
CARGO_TARGET_DIR="$TARGET" cargo build --release --locked -j 4
bench/clone-corpus.sh csharp "$CORPUS"               # MassTransit at 855cf17…, HEAD = the pin
dotnet build tools/scout-semantic -c Release         # oracle, as bench/semantic.sh runs it
dotnet restore "$CORPUS/MassTransit.sln" -p:TargetFrameworks=net9.0
dotnet run --project tools/scout-semantic -c Release -- "$CORPUS/MassTransit.sln" --root "$CORPUS" \
  --out "$ORACLE/refs.jsonl" --units "$ORACLE/units.jsonl" -p:TargetFrameworks=net9.0
shasum -a 256 "$ORACLE/refs.jsonl" "$ORACLE/units.jsonl"   # compare with the digests above
cd "$CORPUS"
SCOUT_EDGE_PROVENANCE="$OUT/provenance.jsonl" "$TARGET/release/devscout" map .
"$TARGET/release/devscout" audit --semantic "$ORACLE/refs.jsonl" --units "$ORACLE/units.jsonl" \
  --fp-sites "$OUT/fp-sites.jsonl" --json
```

`$EXPORT`, `$TARGET`, `$ORACLE` and `$OUT` are fresh directories outside the corpus, and `$CORPUS`
is a path outside it that does not exist yet (`bench/clone-corpus.sh` refuses an existing destination);
every path passed to `audit` is absolute, so it resolves from inside the corpus copy. Run with an
isolated `HOME`, `SCOUT_REGISTRY` and `SCOUT_CONTENT_DB`; a second arm on another fresh copy with
`SCOUT_EDGE_PROVENANCE` unset gives the graph-identity check.

The per-site rows behind the group table, one JSON object per line (`file`, `line`, `member`,
`bound`, `boundFile`, `expected`, `expectedFile`, `step`, `check`, `verdict`, `mechanism`,
`evidence`), sorted by `(file, line, member)`, are committed as
[`bench/precise_wrong_target_rows.jsonl`](../../../bench/precise_wrong_target_rows.jsonl).
