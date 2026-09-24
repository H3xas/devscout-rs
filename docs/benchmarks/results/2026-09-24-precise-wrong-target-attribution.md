# Results — Precise-tier wrong-target attribution, 2026-09-24

Attributes every precise-tier `wrong-target` false positive on the pinned MassTransit corpus to
the resolver mechanism that produced it, before any of those mechanisms is changed. Companion to
[`2026-09-resolver-precision.md`](2026-09-resolver-precision.md), which this record does not edit
(one file per run, never edited in place).

## Environment

```
Date              2026-09-24
Corpus            MassTransit/MassTransit @ 855cf1752c94ca9498e0c45ce8d09fdc9e957dd6 (bench/corpus.lock)
                  copied to an isolated scratch directory, .git/scout removed before mapping;
                  the tracked corpus checkout was never mapped.
Oracle            bench/out/semantic/MassTransit-main-c2bce9f/
                  refs.jsonl  sha256 f9b579f0629a0b930f37723edab102d0a185fc07971582988cc4272e82f5c3a6
                  units.jsonl sha256 3b7137cfff70996059ae07cadd6ff55e315f9fdaa29ef01ce1c1f0a4bc2c8fc6
Binary source     commit 4822530db2d37dd6678798ea30deb9ed3cd1f101 (release line, final tree for
                  this cycle; version string reads 0.6.0, pre-bump)
Binary build      `git archive 4822530…` export, `cargo build --release --locked -j 4`, isolated
                  HOME/TMPDIR/CARGO_TARGET_DIR/SCOUT_REGISTRY/SCOUT_CONTENT_DB
Binary sha256     f588a92775df05e5a687fd76a380ba3bf864901316b65c5d1244234837ae721e
Toolchain         rustc 1.97.1 (8bab26f4f 2026-07-14), aarch64-apple-darwin, offline
Fragment cache    fragments-v21.json
Provenance gate   SCOUT_EDGE_PROVENANCE, read on two fresh corpus copies (gate set / gate unset)
```

## Corpus-level provenance trust (checked before any row is attributed)

The per-site false-positive sink and the env-gated resolver provenance were proven only on two
small fixtures when they were built. This run re-proves both properties on the corpus itself,
at the head this record measures:

1. **Graph identity across the gate.** Two fresh corpus copies, one mapped with
   `SCOUT_EDGE_PROVENANCE` set and one without, produce byte-identical `graph.json`:
   sha256 `f6523adde51480cf4fb9e051f529e7ab741c353063e3db1e59b018b6ee8da999` on both. The
   instrumentation changes nothing the resolver emits.
2. **Full coverage.** The gate-set run wrote 43,523 provenance rows for the graph's 43,523
   `uses-member` edges — an exact match, zero null steps, and every row's step is one of the seven
   documented ladder arms (`base-member`, `qualifier-member`, `qualifier-type`, `typed-receiver`,
   `property-hop`, `extension`, `scored`).
3. **Join completeness.** All 170 precise `wrong-target` rows from `--fp-sites` join to exactly
   one provenance row apiece, by `(file, line, member, bound target)`.

All three checks passed; no stop condition fired.

## Reconciliation to the release-line reference

| Figure | Reference | Measured here |
| --- | --- | --- |
| MassTransit graph sha256 | `f6523adde51480cf4fb9e051f529e7ab741c353063e3db1e59b018b6ee8da999` | identical |
| precise edges / precision | 31240 / 0.993 | identical |
| precise `fp:no-site` / `fp:external` / `fp:wrong` / `structural` | 44 / 0 / 170 / 0 | identical |
| sorted precise wrong-target row digest | `f9698caf6431061ff7bf5190cc9ccf7fd64f7cfde2241dfe7794042122dceb5b` | identical |

Every figure matches the pinned reference exactly; the population is measured, not assumed, at
170 — inside the registered [120, 240] band.

## Method

Each of the 170 rows carries the tier/arm/step the provenance gate recorded, the target the
resolver bound, and the target the oracle expected. A row's verdict is reached first by four
encoded checks run against both the bound and the expected target: does the target declare the
member (directly, or through its in-graph base closure), does its own generic-parameter count
admit the call's explicit type-argument count, does a project reference reach its file, and is
its namespace brought into scope by a `using` (including the enclosing-namespace search a
nested `using` directive gets) or by ancestry of the referencing file's own namespace. A target
failing any check cannot be what C# would bind; a target passing all four is at least admissible.

118 of the 170 rows were non-decisive by those checks alone (both the bound and the expected
target passed every check) or their expected id was outside the checked graph, and were
hand-adjudicated against the corpus source, `graph.json` and `fragments-v21.json` — the true
overload C# selects at that exact call site, worked out from the argument shapes actually
written there. Every row was adjudicated on its own call site; none inherited a verdict from
a cluster of same-named rows, and hand adjudication did catch call sites inside an otherwise
uniform cluster that needed a different read (see the Method note below). The remaining 52 rows
were decisive by the encoded checks alone; 13 of those were independently re-derived from source
as an audit of the checks themselves, which caught and corrected 7 rows the checks had marked
`oracle-wrong` — every one of the seven turned out, on the actual call site, to be a case the
encoded checks under-modelled (an enclosing-namespace `using` search, and a same-arity-but-
wrong-declaring-type case), not a genuine oracle mislabel.

**Result: all 170 rows are `defect`. Zero rows are `oracle-wrong`.** No row reads "unclear"; every
row's mechanism traces to a concrete gap in one resolver function, stated below.

**Method note, cluster caution confirmed.** The `AddConsumer`/`AddSaga`/`AddSagaStateMachine`/
`AddExecuteActivity`/`AddActivity` sites split into two call shapes that look identical by member
name and receiver interface but resolve through different mechanisms depending on the exact
argument list written at each site — some by the call's own explicit type-argument count, others
by an argument's exact parameter-delegate shape. Reading the cluster by its name alone would have
missed that split.

## Defect groups

Every `defect` row's mechanism is a concrete, evidenced gap in exactly one resolver function.
Sizes sum to 170.

| Group | Sites | Mechanism | Owner | Fact vs. rule | Recall risk |
| --- | --- | --- | --- | --- | --- |
| bare-name-cross-project | 4 | A bare simple-name candidate search matches a same-named type in a project the referencing file's project never reaches, ahead of the nested type actually in scope there. | typed-receiver / qualifier-type candidate lookup | rule-only (the project-reference graph is already read elsewhere in the resolver) | low |
| base-walk-not-nearest | 46 | When a receiver's own declared type does not itself declare a member, the base-closure walk returns the first declaring base its traversal reaches, not the nearest (most-derived) one — so a member re-declared with a narrower type by an intermediate interface or class loses to a farther ancestor's declaration of the same name. | typed-receiver base-closure walk | rule-only, likely (traversal-order fix); may need a `hides`/redeclaration fact for the general case | medium |
| explicit-interface-impl-invisible | 1 | A member implemented only as an explicit interface implementation is not reachable through the implementing type itself in C#; the member-declaration check does not model that inaccessibility and treats the type as declaring it anyway. | typed-receiver member-declaration check | new fact (explicit-interface-implementation marker) | low |
| bare-name-shadowing | 8 | A bare identifier that names both an inherited member and a type (or whose narrowed type a pattern match established) is resolved as the type/unnarrowed declaration, when C#'s own lookup or flow-typing rules would prefer the member or the narrowed type. | typed-receiver / qualifier-type name lookup | new fact (flow-sensitive narrowing; member-vs-type shadowing) | low |
| declares-member-shape-blind | 111 | The member-declaration check admits a same-named candidate by name and value-argument COUNT alone. It never compares argument TYPES against the candidate's declared parameter types, and never compares the call's own explicit type-argument count against the candidate's own generic-parameter count — so an instance member that merely shares a name and argument count with the actual call binds ahead of the extension method, sibling overload, or differently-typed base member that the argument shapes actually select. | member-declaration / arity check | new fact (per-argument type/shape, explicit type-argument count) | high |

## Recommended sequence

Narrowest and lowest-risk first, largest and highest-risk last, following the precedent that a
change touching many sites at once is proven on its narrowest slice before it is widened:

1. **bare-name-cross-project** (4) — a project-reachability filter reuses data the resolver
   already reads.
2. **base-walk-not-nearest** (46) — a traversal-order change with a moderate, boundable surface.
3. **explicit-interface-impl-invisible** (1) — one new fact, narrowly scoped.
4. **bare-name-shadowing** (8) — narrowest by site count, but the two call shapes it covers
   (member/type shadowing, pattern-match narrowing) are also the most novel; may be judged not
   worth a dedicated extractor fact for its current yield.
5. **declares-member-shape-blind** (111) — the largest group and the only one that touches the
   central member-declaration check every tier reads. Recommended last, and recommended to be
   split into narrower sub-mechanisms (explicit type-argument-count checking first, since it is
   the more mechanical of the two; argument-type/shape checking second) rather than implemented
   as one change, for the same reason six narrower rules outperformed one broad one on this
   resolver before.

## Reproduction

```
git archive 4822530db2d37dd6678798ea30deb9ed3cd1f101 | tar -x -C <export>
cd <export> && cargo build --release --locked -j 4   # binary sha256 above
cp -a bench/corpora/csharp <scratch>/mt && rm -rf <scratch>/mt/.git/scout
cd <scratch>/mt && SCOUT_EDGE_PROVENANCE=<path> <bin> map .
<bin> audit --semantic bench/out/semantic/MassTransit-main-c2bce9f/refs.jsonl \
  --units bench/out/semantic/MassTransit-main-c2bce9f/units.jsonl \
  --fp-sites <path> --json
```

The per-site rows behind the group table, one JSON object per line (`file`, `line`, `member`,
`bound`, `boundFile`, `expected`, `expectedFile`, `step`, `verdict`, `mechanism`, `evidence`),
are committed alongside this record.
