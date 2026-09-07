// `--json` rendering for the query models: moved out of `cli.rs` to keep
// that file under its size ratchet. `query.rs`'s model types deliberately
// derive no `Serialize` (its own module header) -- the byte shape is built
// here, by hand, key order included.
//
// This does NOT reuse `manifest::Value` (the crate's other order-preserving
// JSON value): its `Number` variant serializes floats through serde_json's
// own `Number::serialize`, which always keeps a decimal point (`1.0`,
// `100.0`) where the target JSON shape drops it for integral values (`1`,
// `100`). `score` (a `personalized_page_rank` output) is the only float
// anywhere in this output, so a tiny local ordered-value type with a
// pre-formatted-number escape hatch (`J::RawNum`) is used instead.
// String/key fields still delegate to `serde_json::to_string` for escaping
// (control chars, `"`, `\`).

use crate::graph;
use crate::query;
use crate::render;

use super::why::{why_for_uses_member, Why};

// `pub(crate)`: `audit.rs`'s `--json` rendering builds its own `J` tree with
// this same encoder rather than hand-rolling a second one.
pub(crate) enum J {
    Str(String),
    UInt(u64),
    RawNum(String),
    // Only ever built as `true`: `heuristic: true` is written and the key is
    // omitted entirely otherwise, so a `false` never reaches this encoder.
    Bool(bool),
    Arr(Vec<J>),
    Obj(Vec<(&'static str, J)>),
}

impl J {
    fn write(&self, out: &mut String) {
        match self {
            J::Str(s) => {
                out.push_str(&serde_json::to_string(s).expect("string JSON encoding cannot fail"))
            }
            J::UInt(n) => out.push_str(&n.to_string()),
            J::RawNum(s) => out.push_str(s),
            J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            J::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            J::Obj(entries) => {
                out.push('{');
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(k).expect("key JSON encoding cannot fail"));
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }

    pub(crate) fn to_json_string(&self) -> String {
        let mut s = String::new();
        self.write(&mut s);
        s
    }
}

// ECMA-262 `JSON.stringify` number formatting for a finite `f64`. Digit selection
// is serde_json's shortest-round-trip float formatter, which agrees with the
// ECMA-262 shortest-round-trip representation. Two adjustments bring it fully in
// line: the decimal point is dropped for an integral value in plain notation
// (`1`, not `1.0`), and zero is never signed (`-0` -> `"0"`); serde_json's
// `Number` keeps a `.0` and would emit `-0`. Exponential notation already matches
// (`1e+21`, `1e-7`) and passes through unchanged. `score` is always finite in
// practice; the non-finite branch coerces `NaN`/`Infinity` to `null` (as
// `JSON.stringify` does) as cheap insurance, not because personalized_page_rank
// can produce one.
pub(crate) fn js_float_string(v: f64) -> String {
    if !v.is_finite() {
        return "null".to_string();
    }
    if v == 0.0 {
        return "0".to_string();
    }
    let s = serde_json::Number::from_f64(v)
        .expect("finite, checked above")
        .to_string();
    if !s.contains('e') && s.ends_with(".0") {
        s[..s.len() - 2].to_string()
    } else {
        s
    }
}

fn j_table<R>(t: &query::Table<R>, row: impl Fn(&R) -> J) -> J {
    J::Obj(vec![
        ("total", J::UInt(t.total as u64)),
        ("dropped", J::UInt(t.dropped as u64)),
        ("rows", J::Arr(t.rows.iter().map(row).collect())),
    ])
}

// `heuristic: true` then `tier` are appended LAST on a guessed row and BOTH
// keys are ABSENT on a precise one -- they are added only in the heuristic
// branch, so a precise row's JSON carries no trace of either. `tier` sits
// immediately after `heuristic` because it refines it: a consumer reading only
// `heuristic` sees the object it saw before, and one that wants the tier finds
// it in the next slot rather than hunting the tail.
//
// A row that declares itself a guess but names no tier writes no `tier` key at
// all -- the same omit-when-empty rule every optional field here follows, and
// the same fallback the text renderer's umbrella `(heuristic)` word takes.
fn push_heuristic(
    fields: &mut Vec<(&'static str, J)>,
    heuristic: bool,
    tier: Option<graph::HeuristicTier>,
) {
    if !heuristic {
        return;
    }
    fields.push(("heuristic", J::Bool(true)));
    if let Some(tier) = tier {
        let word = match tier {
            graph::HeuristicTier::Ext => "ext",
            graph::HeuristicTier::Guess => "guess",
        };
        fields.push(("tier", J::Str(word.to_string())));
    }
}

// The static ref kind behind an inbound/outbound row, known at the call site
// building each of `refs_model_fields`'s three inbound / three outbound
// (`imports` names its own fixed word directly, in `j_import_row`) tables --
// never guessed from the row itself. Only `UsesMember` needs the row's own
// `tier` to pick its exact `why` word (see `why_for_row`); the other two name
// a fixed word regardless of whether the row was guessed, the same way
// `heuristic`/`tier` already say THAT separately.
#[derive(Debug, Clone, Copy)]
enum RowKind {
    Inherits,
    UsesType,
    UsesMember,
    Implements,
    Overrides,
}

fn why_for_row(kind: RowKind, heuristic: bool, tier: Option<graph::HeuristicTier>) -> Why {
    match kind {
        RowKind::Inherits => Why::Inherits,
        RowKind::UsesType => Why::UsesType,
        RowKind::UsesMember => why_for_uses_member(heuristic, tier),
        RowKind::Implements => Why::Implements,
        RowKind::Overrides => Why::Overrides,
    }
}

// `why` is appended absolute LAST on every row shape below, after `source`
// (present or not) -- the same append-last convention every other additive
// field in this file follows.
fn j_inbound_row(r: &query::InboundRow, kind: RowKind) -> J {
    let mut fields = vec![
        ("file", J::Str(r.file.clone())),
        ("line", J::UInt(r.line as u64)),
    ];
    push_heuristic(&mut fields, r.heuristic, r.tier);
    // `source` is appended after `heuristic` and omitted when the line could not
    // be read -- an absent key, never an empty string.
    if !r.source.is_empty() {
        fields.push(("source", J::Str(r.source.clone())));
    }
    fields.push((
        "why",
        J::Str(why_for_row(kind, r.heuristic, r.tier).as_str().to_string()),
    ));
    J::Obj(fields)
}
fn j_outbound_row(r: &query::OutboundRow, kind: RowKind) -> J {
    let mut fields = vec![
        ("file", J::Str(r.file.clone())),
        ("line", J::UInt(r.line as u64)),
        ("toFile", J::Str(r.to_file.clone())),
        ("to", J::Str(r.to.clone())),
    ];
    push_heuristic(&mut fields, r.heuristic, r.tier);
    // `source` is appended after `heuristic`, the same append-last/omit-when-empty
    // rule `j_inbound_row` follows.
    if !r.source.is_empty() {
        fields.push(("source", J::Str(r.source.clone())));
    }
    fields.push((
        "why",
        J::Str(why_for_row(kind, r.heuristic, r.tier).as_str().to_string()),
    ));
    J::Obj(fields)
}
fn j_import_row(r: &query::ImportRow) -> J {
    let mut fields = vec![
        ("file", J::Str(r.file.clone())),
        ("line", J::UInt(r.line as u64)),
        ("target", J::Str(r.target.clone())),
    ];
    if !r.source.is_empty() {
        fields.push(("source", J::Str(r.source.clone())));
    }
    fields.push(("why", J::Str(Why::Imports.as_str().to_string())));
    J::Obj(fields)
}
fn j_ambiguous_row(r: &query::AmbiguousRow) -> J {
    J::Obj(vec![
        ("file", J::Str(r.file.clone())),
        ("line", J::UInt(r.line as u64)),
        ("origin", J::Str(r.origin.clone())),
        ("raw", J::Str(r.raw.clone())),
        ("candidateCount", J::UInt(r.candidate_count as u64)),
    ])
}

// The resolved `refs` JSON shape (`build_refs_model`'s resolved return):
// `{status, query, id, kind, sites, inbound, [outbound], ambiguous,
// manifestGap, [memberRefs], outcome}`, in that key order. `outbound` sits
// between `inbound` and `ambiguous` only under `--out`; the key is either in
// that slot or absent entirely. `outcome` is always `"hit"` here and always
// last: every path that reaches this builder already resolved.
pub(crate) fn refs_model_to_json(model: &query::RefsModel) -> String {
    refs_model_j(model, query::Outcome::Hit).to_json_string()
}

// The resolved `read` JSON shape: exactly the refs shape with ONE key
// inserted -- `"span"` sits between `kind` and `sites`, carrying `{file,
// startLine, endLine, source}`. The key is ABSENT when no span is on record
// (a def whose end line was never extracted), the same honest-absence rule
// `outbound` follows; a caller cannot mistake a start-only answer for a
// span. `outcome` rides along inside `refs_model_fields`'s own tail and stays
// last regardless of where `span` is spliced in.
pub(crate) fn read_model_to_json(model: &query::ReadModel) -> String {
    let mut fields = refs_model_fields(&model.refs, query::Outcome::Hit);
    // `split_off(5)` lifts everything after the first five keys
    // (schema_version/status/query/id/kind) so `span` can take their place in
    // line.
    let tail = fields.split_off(5);
    if let Some(sp) = &model.span {
        fields.push((
            "span",
            J::Obj(vec![
                ("file", J::Str(sp.file.clone())),
                ("startLine", J::UInt(sp.start_line as u64)),
                ("endLine", J::UInt(sp.end_line as u64)),
                ("source", J::Str(sp.source.clone())),
                ("why", J::Str(Why::Declaration.as_str().to_string())),
            ]),
        ));
    }
    fields.extend(tail);
    J::Obj(fields).to_json_string()
}

// The bare-member `refs` JSON: `{status:'members', query, members, outcome}`,
// where each member is `refs_model_j`'s object unchanged -- the bare-member
// answer reshapes nothing, it only says how many declaring types answered. The
// wrapper's own `outcome` is appended last so a caller reading only the top
// level still finds it there, and the SAME word is stamped on every member so
// the two levels cannot disagree about whether the answer is empty.
pub(crate) fn member_refs_to_json(
    query_str: &str,
    models: &[query::RefsModel],
    outcome: query::Outcome,
) -> String {
    J::Obj(vec![
        ("schema_version", J::UInt(query::SCHEMA_VERSION)),
        ("status", J::Str("members".to_string())),
        ("query", J::Str(query_str.to_string())),
        (
            "members",
            J::Arr(models.iter().map(|m| refs_model_j(m, outcome)).collect()),
        ),
        ("outcome", J::Str(outcome.as_str().to_string())),
    ])
    .to_json_string()
}

fn refs_model_j(model: &query::RefsModel, outcome: query::Outcome) -> J {
    J::Obj(refs_model_fields(model, outcome))
}

// `implements`/`overrides` join an `inbound`/`outbound` JSON object only when
// their own total is non-zero -- present-only-when-applicable, the same rule
// `memberRefs` follows -- so a symbol untouched by dispatch edges keeps the
// exact `--json` bytes it had before this pair existed.
fn dispatch_table_fields<R>(
    implements: &query::Table<R>,
    overrides: &query::Table<R>,
    row: impl Fn(&R, RowKind) -> J + Copy,
) -> Vec<(&'static str, J)> {
    let mut fields = Vec::new();
    if implements.total != 0 {
        fields.push((
            "implements",
            j_table(implements, |r| row(r, RowKind::Implements)),
        ));
    }
    if overrides.total != 0 {
        fields.push((
            "overrides",
            j_table(overrides, |r| row(r, RowKind::Overrides)),
        ));
    }
    fields
}

// The three inbound tables plus the two dispatch ones, each row tagged with
// the static kind its own table names (see `RowKind`/`why_for_row`).
fn j_inbound_tables(t: &query::InboundTables) -> J {
    J::Obj(
        vec![
            (
                "inherits",
                j_table(&t.inherits, |r| j_inbound_row(r, RowKind::Inherits)),
            ),
            (
                "uses-type",
                j_table(&t.uses_type, |r| j_inbound_row(r, RowKind::UsesType)),
            ),
            (
                "uses-member",
                j_table(&t.uses_member, |r| j_inbound_row(r, RowKind::UsesMember)),
            ),
        ]
        .into_iter()
        .chain(dispatch_table_fields(
            &t.implements,
            &t.overrides,
            j_inbound_row,
        ))
        .collect::<Vec<_>>(),
    )
}

// The outbound tables, the same per-kind tagging `j_inbound_tables`
// applies -- `imports` names its own fixed word directly (see `j_import_row`)
// and stays last, after the two dispatch tables.
fn j_outbound_tables(t: &query::OutboundTables) -> J {
    J::Obj(
        vec![
            (
                "inherits",
                j_table(&t.inherits, |r| j_outbound_row(r, RowKind::Inherits)),
            ),
            (
                "uses-type",
                j_table(&t.uses_type, |r| j_outbound_row(r, RowKind::UsesType)),
            ),
            (
                "uses-member",
                j_table(&t.uses_member, |r| j_outbound_row(r, RowKind::UsesMember)),
            ),
        ]
        .into_iter()
        .chain(dispatch_table_fields(
            &t.implements,
            &t.overrides,
            j_outbound_row,
        ))
        .chain(std::iter::once((
            "imports",
            j_table(&t.imports, j_import_row),
        )))
        .collect::<Vec<_>>(),
    )
}

fn refs_model_fields(model: &query::RefsModel, outcome: query::Outcome) -> Vec<(&'static str, J)> {
    let mut fields = vec![
        ("schema_version", J::UInt(query::SCHEMA_VERSION)),
        ("status", J::Str("resolved".to_string())),
        ("query", J::Str(model.query.clone())),
        ("id", J::Str(model.id.clone())),
        ("kind", J::Str(model.kind.clone())),
        (
            "sites",
            J::Arr(
                model
                    .sites
                    .iter()
                    .map(|s| {
                        J::Obj(vec![
                            ("file", J::Str(s.file.clone())),
                            ("line", J::UInt(s.line as u64)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("inbound", j_inbound_tables(&model.inbound)),
    ];
    if let Some(ob) = &model.outbound {
        fields.push(("outbound", j_outbound_tables(ob)));
    }
    fields.push((
        "ambiguous",
        J::Obj(vec![
            (
                "inbound",
                j_table(&model.ambiguous.inbound, j_ambiguous_row),
            ),
            (
                "outbound",
                j_table(&model.ambiguous.outbound, j_ambiguous_row),
            ),
        ]),
    ));
    fields.push(("manifestGap", J::UInt(model.manifest_gap as u64)));
    // Appended LAST, after `manifestGap`, and only for an enum with member-level
    // references; an absent key keeps every other symbol's `--json` bytes
    // unchanged.
    if let Some(m) = &model.member_refs {
        fields.push((
            "memberRefs",
            J::Obj(vec![
                ("total", J::UInt(m.total as u64)),
                ("memberCount", J::UInt(m.member_count as u64)),
                (
                    "members",
                    J::Arr(
                        m.members
                            .iter()
                            .map(|e| {
                                J::Obj(vec![
                                    ("name", J::Str(e.name.clone())),
                                    ("count", J::UInt(e.count as u64)),
                                ])
                            })
                            .collect(),
                    ),
                ),
                ("dropped", J::UInt(m.dropped as u64)),
            ]),
        ));
    }
    // Appended absolute LAST, after `memberRefs`: every path building this
    // model already resolved, so the word is `hit` unless the caller knows the
    // answer is empty -- additive, since no key here changes value or moves.
    fields.push(("outcome", J::Str(outcome.as_str().to_string())));
    fields
}

fn j_impact_row(r: &query::ImpactRow) -> J {
    let mut fields = vec![
        ("file", J::Str(r.file.clone())),
        ("hop", J::UInt(r.hop as u64)),
        ("viaCount", J::UInt(r.via_count as u64)),
        ("ambiguousCount", J::UInt(r.ambiguous_count as u64)),
        (
            "topSymbols",
            J::Arr(r.top_symbols.iter().map(|s| J::Str(s.clone())).collect()),
        ),
        ("topSymbolsMore", J::UInt(r.top_symbols_more as u64)),
        ("score", J::RawNum(js_float_string(r.score))),
    ];
    // `heuristicCount` then `heuristic` then `tier`, all appended after `score`
    // and all present only on a heuristic-only row (JS assigns them inside the
    // same `if`). The last two go through the shared `push_heuristic`, so their
    // order relative to each other is stated once for all four row shapes --
    // which lands `tier` BEFORE `ifaceVia` here, in the slot right after the
    // flag it refines.
    if r.heuristic {
        fields.push(("heuristicCount", J::UInt(r.heuristic_count as u64)));
    }
    push_heuristic(&mut fields, r.heuristic, r.tier);
    // Appended LAST, present only on a row the interface hop actually reached.
    if !r.iface_via.is_empty() {
        fields.push((
            "ifaceVia",
            J::Arr(r.iface_via.iter().map(|s| J::Str(s.clone())).collect()),
        ));
    }
    // Appended after `ifaceVia`, present only when at least one edge kind could
    // attribute a line to this row. Key order inside the object is the walk's own
    // kind declaration order, fixed in `from_lines_of`, never a map iteration.
    if !r.from_lines.is_empty() {
        fields.push((
            "fromLines",
            J::Obj(
                r.from_lines
                    .iter()
                    .map(|(kind, line)| (*kind, J::UInt(*line as u64)))
                    .collect(),
            ),
        ));
    }
    // Appended LAST and only on a hub file, so every other row keeps the key
    // order it had.
    if r.infra {
        fields.push(("class", J::Str("infra".to_string())));
    }
    // Appended absolute LAST, after `class`: the one rule or tier that best
    // explains why this file was reached, always present.
    fields.push(("why", J::Str(r.why.as_str().to_string())));
    J::Obj(fields)
}

// Every field of the resolved `impact` JSON shape (`build_impact_model`'s
// resolved return) EXCEPT `outcome`: `{schema_version, query, status, kind,
// seedFiles, hops, totalAffected, rows, dropped, manifestGap,
// heuristicAffected, testsAffected, [braked]}`, in that key order. Shared by
// the plain and imports-aware builders below so the fields both answers
// carry can never drift apart between the two.
fn impact_model_fields(query_str: &str, model: &query::ImpactModel) -> Vec<(&'static str, J)> {
    vec![
        ("schema_version", J::UInt(query::SCHEMA_VERSION)),
        ("query", J::Str(query_str.to_string())),
        ("status", J::Str("resolved".to_string())),
        (
            "kind",
            J::Str(render::seed_kind_str(model.kind).to_string()),
        ),
        (
            "seedFiles",
            J::Arr(model.seed_files.iter().map(|f| J::Str(f.clone())).collect()),
        ),
        ("hops", J::UInt(model.hops as u64)),
        ("totalAffected", J::UInt(model.total_affected as u64)),
        (
            "rows",
            J::Arr(model.rows.iter().map(j_impact_row).collect()),
        ),
        ("dropped", J::UInt(model.dropped as u64)),
        ("manifestGap", J::UInt(model.manifest_gap as u64)),
        // Appended LAST after `manifestGap` -- always present, unlike the
        // per-row flags.
        (
            "heuristicAffected",
            J::UInt(model.heuristic_affected as u64),
        ),
        // Test-coverage stage, appended after it -- also always present.
        ("testsAffected", J::UInt(model.tests_affected as u64)),
    ]
    .into_iter()
    // Appended LAST and only when the brake actually fired, so every answer it
    // never touched keeps the exact key order it had before. The file entries
    // ride in the SAME array, after every interface entry, rather than in a
    // second top-level key: a consumer already reading `braked` sees both brakes
    // without a schema change.
    .chain(
        if model.braked.is_empty() && model.braked_files.is_empty() {
            None
        } else {
            Some((
                "braked",
                J::Arr(
                    model
                        .braked
                        .iter()
                        .map(|b| {
                            J::Obj(vec![
                                ("iface", J::Str(b.iface.clone())),
                                ("fanin", J::UInt(b.fanin as u64)),
                            ])
                        })
                        .chain(model.braked_files.iter().map(|b| {
                            J::Obj(vec![
                                ("file", J::Str(b.file.clone())),
                                ("indegree", J::UInt(b.indegree as u64)),
                            ])
                        }))
                        .collect(),
                ),
            ))
        },
    )
    .collect()
}

// `outcome` is `"zero-hit"` when the resolved seed's blast radius is empty
// AND (for the imports-aware caller) the import named nothing either -- the
// same condition the exit-code-3 zero-hit signal keys off -- and `"hit"`
// otherwise.
fn impact_outcome(model: &query::ImpactModel, imported_hit: bool) -> query::Outcome {
    if model.rows.is_empty() && !imported_hit {
        query::Outcome::ZeroHit
    } else {
        query::Outcome::Hit
    }
}

/// The resolved `impact` JSON shape, with the query key first, in
/// [`impact_model_fields`]'s key order, `outcome` last.
pub(crate) fn impact_model_to_json(query_str: &str, model: &query::ImpactModel) -> String {
    let mut fields = impact_model_fields(query_str, model);
    let outcome = impact_outcome(model, false);
    fields.push(("outcome", J::Str(outcome.as_str().to_string())));
    J::Obj(fields).to_json_string()
}

// An imported row's JSON shape: `{file, hop, repo, importedKind, why}`, `why`
// last like every other hit row this module builds.
fn j_imported_row(r: &query::ImportedRow) -> J {
    J::Obj(vec![
        ("file", J::Str(r.file.clone())),
        ("hop", J::UInt(r.hop as u64)),
        ("repo", J::Str(r.repo.clone())),
        ("importedKind", J::Str(r.imported_kind.clone())),
        ("why", J::Str(r.why.as_str().to_string())),
    ])
}

/// The resolved `impact` JSON shape with an import configured: every key
/// [`impact_model_to_json`] writes, then `importedAffected`, `importedDropped`,
/// `importedRows` and the export's `provenance` block, all appended before
/// `outcome` -- additive only, so a consumer already reading the plain shape
/// sees exactly what it saw before, plus these four keys. `outcome` reports
/// `"hit"` when either the native model or the imported section found
/// something, so an import can turn an otherwise-empty answer into a hit.
pub(crate) fn impact_model_to_json_with_imports(
    query_str: &str,
    model: &query::ImpactModel,
    imported: &query::ImportedSection,
    provenance: &graph::Provenance,
) -> String {
    let mut fields = impact_model_fields(query_str, model);
    fields.push(("importedAffected", J::UInt(imported.affected as u64)));
    fields.push(("importedDropped", J::UInt(imported.dropped as u64)));
    fields.push((
        "importedRows",
        J::Arr(imported.rows.iter().map(j_imported_row).collect()),
    ));
    fields.push((
        "provenance",
        J::Obj(vec![
            ("id", J::Str(provenance.id.clone())),
            ("producer", J::Str(provenance.producer.clone())),
            ("formatVersion", J::UInt(provenance.format_version)),
        ]),
    ));
    let outcome = impact_outcome(model, !imported.rows.is_empty());
    fields.push(("outcome", J::Str(outcome.as_str().to_string())));
    J::Obj(fields).to_json_string()
}

// The resolved `tests` JSON shape (`build_tests_model`'s resolved return):
// `{status, query, symbol, defFiles, rows, testFileCount, refCount,
// heuristicFileCount, heuristicRefCount, outcome}`, in that key order, with
// the heuristic pair before `outcome`, always last. Each row carries
// `via: "project"` as its own last key, appended after `heuristic`/`tier`,
// ONLY when the row's vouch is the project model -- an attribute-vouched row
// emits no `via` key at all, so today's bytes for every graph without a
// project model are unchanged. `outcome` is always `"hit"`: reaching this
// builder means the seed already resolved.
pub(crate) fn tests_model_to_json(model: &query::TestsModel) -> String {
    J::Obj(vec![
        ("schema_version", J::UInt(query::SCHEMA_VERSION)),
        ("status", J::Str("resolved".to_string())),
        ("query", J::Str(model.query.clone())),
        ("symbol", J::Str(model.symbol.clone())),
        (
            "defFiles",
            J::Arr(model.def_files.iter().map(|f| J::Str(f.clone())).collect()),
        ),
        (
            "rows",
            J::Arr(
                model
                    .rows
                    .iter()
                    .map(|r| {
                        let mut fields = vec![
                            ("file", J::Str(r.file.clone())),
                            (
                                "testDefs",
                                J::Arr(r.test_defs.iter().map(|d| J::Str(d.clone())).collect()),
                            ),
                            (
                                "lines",
                                J::Arr(r.lines.iter().map(|l| J::UInt(*l as u64)).collect()),
                            ),
                            ("refCount", J::UInt(r.ref_count as u64)),
                        ];
                        push_heuristic(&mut fields, r.heuristic, r.tier);
                        // `via` is appended LAST, after `heuristic`/`tier`, and only
                        // when the row's vouch is the project model: an
                        // attribute-vouched row keeps today's exact bytes.
                        let why = if r.via == query::TestVia::Project {
                            fields.push(("via", J::Str("project".to_string())));
                            Why::TestProject
                        } else {
                            Why::TestAttribute
                        };
                        // Appended absolute LAST, after `via` when present: which of
                        // the two ways `tests` reaches a file earned this row.
                        fields.push(("why", J::Str(why.as_str().to_string())));
                        J::Obj(fields)
                    })
                    .collect(),
            ),
        ),
        ("testFileCount", J::UInt(model.test_file_count as u64)),
        ("refCount", J::UInt(model.ref_count as u64)),
        (
            "heuristicFileCount",
            J::UInt(model.heuristic_file_count as u64),
        ),
        (
            "heuristicRefCount",
            J::UInt(model.heuristic_ref_count as u64),
        ),
        ("outcome", J::Str(query::Outcome::Hit.as_str().to_string())),
    ])
    .to_json_string()
}
