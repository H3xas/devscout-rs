use std::collections::HashMap;
use std::path::Path;

use crate::graph;

use super::bus::{self, BusHopRow};
use super::dispatch;
use super::index::{def_sites, DefSite, GraphIndex};
use super::member::{self, MemberCandidate};
use super::occurrence::tag_inbound_rows;
use super::refs_member::{enum_member_refs, MemberRefs};
use super::refs_outbound::build_outbound_tables;
use super::refs_tables::{
    ambiguous_row, build_table, cap_rows, edge_loc, loc_cmp, row_tier, AmbiguousTables, InboundRow,
    InboundTables, OutboundTables, Table, TsTables, SOURCE_MAX,
};
use super::symbol::{resolve_local_symbol, resolve_symbol, Resolution};
use super::symbol_refs::symbol_refs;

/// The resolved `refs` result for one symbol.
#[derive(Debug, Clone, PartialEq)]
pub struct RefsModel {
    /// The query value.
    pub query: String,
    /// The id value.
    pub id: String,
    /// The kind value.
    pub kind: String,
    /// The sites value.
    pub sites: Vec<DefSite>,
    /// The inbound value.
    pub inbound: InboundTables,
    /// Present only under `--out`; `None` otherwise, which is what keeps
    /// `--json` and the two text renderers agreeing about whether the outbound
    /// side exists at all.
    pub outbound: Option<OutboundTables>,
    /// The ambiguous value.
    pub ambiguous: AmbiguousTables,
    /// Every `bus-hop` row naming this symbol, combined -- see `bus::BusHopRow`.
    pub bus: Table<BusHopRow>,
    /// Number of files present in the graph but absent from the manifest.
    pub manifest_gap: usize,
    /// `Some` only for an enum with member-level references; `None` for every
    /// other symbol.
    pub member_refs: Option<MemberRefs>,
}

/// The outcome of a `refs` query: resolved, ambiguous, a bare-member answer,
/// or not found.
#[derive(Debug, Clone, PartialEq)]
pub enum RefsResult {
    /// Represents `Resolved`.
    Resolved(RefsModel),
    /// Represents `Ambiguous`.
    Ambiguous(Vec<String>),
    /// A bare-member answer: one resolved-shaped model per declaring type.
    Members(Vec<RefsModel>),
    /// A member seed (`Member`, `Type.Member`, `Namespace.Type.Member`) whose
    /// name survives edge verification on more than one declaring type: one
    /// row per candidate, never the bare type list [`RefsResult::Ambiguous`]
    /// renders.
    MemberAmbiguous(Vec<MemberCandidate>),
    /// Represents `NotFound`.
    NotFound,
}

// A file's project is the first segment of its repo-relative path -- the graph
// carries no other grouping, and the paths it stores are always repo-relative
// with `/` separators. A file at the repo root has the empty project, which
// only ever matches another root file.
pub(super) fn project_of(file: &str) -> &str {
    match file.find('/') {
        Some(i) => &file[..i],
        None => "",
    }
}

// Trims exactly this ASCII set and nothing else. `str::trim` would strip the
// full Unicode White_Space property (U+00A0 included) but not U+FEFF; a fixed
// explicit set keeps trimming stable and independent of the Unicode tables, so
// a line whose first or last character is one of these is trimmed predictably.
pub(super) fn trim_source(text: &str) -> &str {
    text.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n' | '\u{0b}' | '\u{0c}'))
}

/// Per-file cache of source lines, so a widely used type does not re-read one
/// consumer file once per hit. The value is `None` for a file that could not
/// be read.
pub type LineCache = HashMap<String, Option<Vec<String>>>;

fn cached_line(root: &Path, file: &str, line: usize, cache: &mut LineCache) -> String {
    let lines = cache.entry(file.to_string()).or_insert_with(|| {
        std::fs::read_to_string(root.join(file))
            .ok()
            .map(|body| body.split('\n').map(str::to_string).collect())
    });
    let Some(lines) = lines else {
        return String::new();
    };
    if line < 1 || line > lines.len() {
        return String::new();
    }
    let mut raw = lines[line - 1].as_str();
    // A UTF-8 BOM survives `read_to_string` and is not in the trim set, so line
    // 1 of a BOM'd file would otherwise print it.
    if line == 1 {
        raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    }
    trim_source(raw).to_string()
}

// Interior tabs collapse to one space each and the result is cut to `SOURCE_MAX`.
fn clip_source(text: &str) -> String {
    let text = text.replace('\t', " ");
    // Cutting at `SOURCE_MAX` UTF-16 code units can split a surrogate pair; the
    // lone surrogate is then emitted as U+FFFD, which is what `from_utf16_lossy`
    // produces here.
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.len() > SOURCE_MAX {
        String::from_utf16_lossy(&units[..SOURCE_MAX])
    } else {
        text
    }
}

// The one trimmed, clipped line of source a refs hit sits on, so a caller can
// judge the hit without opening the file.
pub(super) fn hit_source(root: &Path, file: &str, line: usize, cache: &mut LineCache) -> String {
    clip_source(&cached_line(root, file, line, cache))
}

// A `uses-member` edge records the member's declaring TYPE, the file and the
// line, and never the member's own name, so no bare-member answer can be read
// off an edge. This whole-token test stands in for the field that is missing:
// an occurrence with a word character on either side does not count, so `Foo`
// never answers for a line whose only occurrence is `FooEx`. A word character
// is an ASCII letter, an ASCII digit or `_`, and nothing else -- every other
// code point, non-ASCII included, is a boundary. That rule is stated as a
// literal test rather than a character class so it does not depend on any
// regex engine's Unicode handling.
//
// The scan works over UTF-8 bytes: a valid UTF-8 needle can only match at a
// character boundary (lead and continuation byte ranges are disjoint), and
// every byte outside ASCII fails the word-character test on both sides.
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

pub(super) fn line_has_token(line: &str, token: &str) -> bool {
    let hay = line.as_bytes();
    let needle = token.as_bytes();
    if needle.is_empty() || needle.len() > hay.len() {
        return false;
    }
    for at in 0..=(hay.len() - needle.len()) {
        if &hay[at..at + needle.len()] != needle {
            continue;
        }
        let after_at = at + needle.len();
        let before_ok = at == 0 || !is_word_byte(hay[at - 1]);
        let after_ok = after_at >= hay.len() || !is_word_byte(hay[after_at]);
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

// The two non-empty outcomes of `build_member_refs_models`: a plain models
// array, or an ambiguity -- more than one declaring type surviving edge-line
// verification is reported, never turned into several models.
enum MemberRefsOutcome {
    Models(Vec<RefsModel>),
    Ambiguous(Vec<MemberCandidate>),
}

// `refs <member seed>` (`Member`, `Type.Member`, `Namespace.Type.Member`): the
// name index names the declaring type(s) the seed's own qualifier admits
// (`member::qualified_member_owners`); each candidate type's inbound
// `uses-member` edges survives only if the line it starts on carries the
// member as a whole token. A type whose edges all fail verification is
// dropped only while another type still answers, so a bare name still
// disambiguates on evidence; when none answers, the declared owners stand and
// a member nothing references resolves to an empty answer, not to nothing.
//
// More than one declaring type surviving verification answers
// `Ambiguous(candidates)`, in name-index order, rather than several models --
// the house rule of never guessing between candidates. Overloads of one name
// on ONE type are one group (one entry in `owners`, several sites), never an
// ambiguity.
// One owner's inbound `uses-member` edges, kept only if the line they start
// on carries `name` as a whole token, ranked the same way a type's own
// inbound table is: precise before guessed, the declaring type's own project
// before every other, then file, then line.
fn verified_member_edges(
    index: &GraphIndex,
    edges: &[graph::Edge],
    owner: &str,
    name: &str,
    cache: &mut LineCache,
) -> Vec<(usize, bool)> {
    let refs = symbol_refs(index, owner);
    let owner_def = &index.graph.defs[index.by_id[owner]];
    let owner_project = project_of(&owner_def.file).to_string();
    let mut kept: Vec<(usize, bool)> = refs
        .inbound_uses_member
        .iter()
        .map(|&e| (e, false))
        .chain(
            refs.heuristic_inbound_uses_member
                .iter()
                .map(|&e| (e, true)),
        )
        .collect();
    kept.retain(|&(e, _)| {
        let (file, line) = edge_loc(&edges[e]);
        line_has_token(&cached_line(&index.root, file, line, cache), name)
    });
    let foreign = |e: usize| usize::from(project_of(edge_loc(&edges[e]).0) != owner_project);
    kept.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then_with(|| foreign(a.0).cmp(&foreign(b.0)))
            .then_with(|| loc_cmp(&edges[a.0], &edges[b.0]))
    });
    kept
}

fn empty_table<R>() -> Table<R> {
    Table {
        total: 0,
        dropped: 0,
        rows: Vec::new(),
    }
}

// One declaring type's resolved-shaped model: `take` (bounded by the shared
// `budget`, spent across every group in call order) of its verified edges
// become inbound rows; the rest are counted in `dropped` but never shown.
#[allow(clippy::too_many_arguments)]
fn member_group_model(
    index: &GraphIndex,
    edges: &[graph::Edge],
    seed: &str,
    name: &str,
    owner: String,
    sites: Vec<DefSite>,
    kept: Vec<(usize, bool)>,
    dispatch: Vec<usize>,
    budget: &mut usize,
    cache: &mut LineCache,
) -> RefsModel {
    let take = (*budget).min(kept.len());
    *budget -= take;
    let mut rows = Vec::new();
    for &(e, heuristic) in &kept[..take] {
        let (file, line) = edge_loc(&edges[e]);
        let source = clip_source(&cached_line(&index.root, file, line, cache));
        rows.push(InboundRow {
            file: file.to_string(),
            line,
            heuristic,
            tier: row_tier(
                heuristic,
                edges[e].tier() == Some(graph::HeuristicTier::Ext),
            ),
            source,
            occurrence_index: None,
        });
    }
    tag_inbound_rows(&mut rows);
    let total = kept.len();
    let (mut implements_rows, mut overrides_rows) =
        dispatch::split_dispatch_rows(edges, &dispatch, |e| {
            let (file, line) = edge_loc(&edges[e]);
            InboundRow {
                file: file.to_string(),
                line,
                heuristic: false,
                tier: None,
                source: clip_source(&cached_line(&index.root, file, line, cache)),
                occurrence_index: None,
            }
        });
    tag_inbound_rows(&mut implements_rows);
    tag_inbound_rows(&mut overrides_rows);
    RefsModel {
        query: seed.to_string(),
        id: format!("{owner}.{name}"),
        kind: "member".to_string(),
        sites,
        inbound: InboundTables {
            inherits: empty_table(),
            uses_type: empty_table(),
            uses_member: Table {
                total,
                dropped: total - rows.len(),
                rows,
            },
            implements: dispatch::dispatch_table(implements_rows),
            overrides: dispatch::dispatch_table(overrides_rows),
            ts: TsTables::default(),
        },
        outbound: None,
        ambiguous: AmbiguousTables {
            inbound: empty_table(),
            outbound: empty_table(),
        },
        bus: bus::empty_bus_table(),
        manifest_gap: index.flagged_files.len(),
        member_refs: None,
    }
}

fn build_member_refs_models(
    index: &GraphIndex,
    seed: &str,
    inbound_cap: usize,
) -> Option<MemberRefsOutcome> {
    let (name, qualifier) = member::split_member_seed(seed);
    let owners = member::qualified_member_owners(index, name, qualifier);
    if owners.is_empty() {
        return None;
    }
    let edges = &index.graph.edges;
    let mut cache: LineCache = HashMap::new();
    let mut groups: Vec<(String, Vec<DefSite>, Vec<(usize, bool)>, Vec<usize>)> = owners
        .into_iter()
        .map(|(owner, sites)| {
            let kept = verified_member_edges(index, edges, &owner, name, &mut cache);
            let hits = dispatch::member_dispatch_edges(&symbol_refs(index, &owner), edges, name);
            (owner, sites, kept, hits)
        })
        .collect();
    let answers =
        |kept: &Vec<(usize, bool)>, hits: &Vec<usize>| !kept.is_empty() || !hits.is_empty();
    if groups.iter().any(|(_, _, kept, hits)| answers(kept, hits)) {
        groups.retain(|(_, _, kept, hits)| answers(kept, hits));
    }
    // Reported before the inbound cap is ever spent, so the answer does not
    // depend on `inbound_cap`: an ambiguity is a refusal, not a budgeted,
    // capped multi-block resolution.
    if groups.len() > 1 {
        return Some(MemberRefsOutcome::Ambiguous(
            groups
                .into_iter()
                .map(|(owner, sites, ..)| MemberCandidate {
                    owner,
                    name: name.to_string(),
                    file: sites[0].file.clone(),
                    line: sites[0].line,
                })
                .collect(),
        ));
    }

    let mut budget = inbound_cap;
    let models = groups
        .into_iter()
        .map(|(owner, sites, kept, hits)| {
            member_group_model(
                index,
                edges,
                seed,
                name,
                owner,
                sites,
                kept,
                hits,
                &mut budget,
                &mut cache,
            )
        })
        .collect();
    Some(MemberRefsOutcome::Models(models))
}

/// One inbound edge awaiting the global cap: which kind's table it belongs to,
/// which edge it is, and whether it was guessed.
struct RankedInbound {
    kind: usize,
    edge: usize,
    heuristic: bool,
}

/// Build the `refs` model for `query`. `out` includes the outbound tables;
/// `cap`/`inbound_cap`/`outbound_cap` bound the tables; `all_out` (`--all`)
/// lifts the inbound and outbound caps.
pub fn build_refs_model(
    index: &GraphIndex,
    query: &str,
    out: bool,
    cap: usize,
    inbound_cap: usize,
    outbound_cap: usize,
    all_out: bool,
) -> RefsResult {
    build_refs_model_inner(
        index,
        query,
        out,
        cap,
        inbound_cap,
        outbound_cap,
        all_out,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
#[allow(
    clippy::too_many_lines,
    reason = "one ordered assembly of the refs model's inbound and outbound sections, sharing the same caps and options across both"
)]
pub(super) fn build_refs_model_inner(
    index: &GraphIndex,
    query: &str,
    out: bool,
    cap: usize,
    inbound_cap: usize,
    outbound_cap: usize,
    all_out: bool,
    exclude_self_inbound: bool,
) -> RefsResult {
    let id = match resolve_symbol(index, query) {
        Resolution::Resolved(id) => id,
        Resolution::Ambiguous(ids) => return RefsResult::Ambiguous(ids),
        // The member path is a FALLBACK, reached only when nothing in the graph
        // declares `query` as a type, so a name that is both a type and a
        // member still answers as the type. `out` has no member reading (a
        // type's outbound edges are the type's answer, not the member's) and is
        // ignored here.
        Resolution::NotFound => match build_member_refs_models(index, query, inbound_cap) {
            Some(MemberRefsOutcome::Models(models)) => return RefsResult::Members(models),
            Some(MemberRefsOutcome::Ambiguous(candidates)) => {
                return RefsResult::MemberAmbiguous(candidates)
            }
            None => match resolve_local_symbol(index, query) {
                Resolution::Resolved(id) => id,
                Resolution::Ambiguous(ids) => return RefsResult::Ambiguous(ids),
                Resolution::NotFound => return RefsResult::NotFound,
            },
        },
    };
    let def_idx = *index
        .by_id
        .get(&id)
        .expect("resolved id must exist in the index it was resolved from");
    let def = &index.graph.defs[def_idx];
    let refs = symbol_refs(index, &id);
    let edges = &index.graph.edges;

    // The inbound kinds share ONE cap and ONE ranking, so a widely used
    // type spends its whole budget on the most specific edges it has rather
    // than on whichever kind the walk reached first. Rank: precise before
    // heuristic, then the def's own project before every other, then file,
    // then line. Push order (kind by kind, precise then heuristic within a
    // kind) is load-bearing: the sort is stable, so two edges of different
    // kinds at the same file:line keep their push order.
    let def_project = project_of(&def.file);
    let mut ranked: Vec<RankedInbound> = Vec::new();
    let mut totals = [0usize; 9];
    let is_self_inbound = |edge: usize| {
        let (file, line) = edge_loc(&edges[edge]);
        exclude_self_inbound
            && def.end_line >= def.line
            && file == def.file
            && (def.line..=def.end_line).contains(&line)
    };
    const EMPTY: &[usize] = &[];
    for (kind, (precise, heuristic)) in [
        (
            &refs.inbound_inherits[..],
            &refs.heuristic_inbound_inherits[..],
        ),
        (
            &refs.inbound_uses_type[..],
            &refs.heuristic_inbound_uses_type[..],
        ),
        (
            &refs.inbound_uses_member[..],
            &refs.heuristic_inbound_uses_member[..],
        ),
        (&refs.inbound_implements[..], EMPTY),
        (&refs.inbound_overrides[..], EMPTY),
        (&refs.inbound_ts.call[..], EMPTY),
        (&refs.inbound_ts.jsx_use[..], EMPTY),
        (&refs.inbound_ts.dispatch[..], EMPTY),
        (&refs.inbound_ts.import[..], EMPTY),
    ]
    .into_iter()
    .enumerate()
    {
        for &e in precise {
            if is_self_inbound(e) {
                continue;
            }
            ranked.push(RankedInbound {
                kind,
                edge: e,
                heuristic: false,
            });
            totals[kind] += 1;
        }
        for &e in heuristic {
            if is_self_inbound(e) {
                continue;
            }
            ranked.push(RankedInbound {
                kind,
                edge: e,
                heuristic: true,
            });
            totals[kind] += 1;
        }
    }
    let foreign = |e: usize| usize::from(project_of(edge_loc(&edges[e]).0) != def_project);
    ranked.sort_by(|a, b| {
        a.heuristic
            .cmp(&b.heuristic)
            .then_with(|| foreign(a.edge).cmp(&foreign(b.edge)))
            .then_with(|| loc_cmp(&edges[a.edge], &edges[b.edge]))
    });
    // `all_out` (`--all`, the same flag the outbound cap-lift below reads) lifts
    // the inbound cap too, not just the outbound one: a caller reaching for
    // `--all` on a truncated inbound table is asking for exactly this.
    let (shown_inbound, _) = cap_rows(ranked, if all_out { usize::MAX } else { inbound_cap });

    let mut source_cache: LineCache = HashMap::new();
    let mut rows: [Vec<InboundRow>; 9] = Default::default();
    for r in shown_inbound {
        let (file, line) = edge_loc(&edges[r.edge]);
        let source = hit_source(&index.root, file, line, &mut source_cache);
        rows[r.kind].push(InboundRow {
            file: file.to_string(),
            line,
            heuristic: r.heuristic,
            tier: row_tier(
                r.heuristic,
                edges[r.edge].tier() == Some(graph::HeuristicTier::Ext),
            ),
            source,
            occurrence_index: None,
        });
    }
    // Tagged per kind, AFTER capping and BEFORE the `Table` wrapper is built:
    // an occurrence collision is scoped to one table of one answer, never
    // across kinds (a `uses-type` row and a `uses-member` row at the same
    // file:line are already distinct by kind and need no ordinal), and a row
    // dropped by the cap above is never assigned one, per the same
    // truncation-is-not-a-collapse rule `dropped` already states.
    for kind_rows in &mut rows {
        tag_inbound_rows(kind_rows);
    }
    let [inherits_rows, uses_type_rows, uses_member_rows, implements_rows, overrides_rows, call_rows, jsx_use_rows, dispatch_rows, import_rows] =
        rows;
    let inbound_table = |total: usize, rows: Vec<InboundRow>| Table {
        total,
        dropped: total - rows.len(),
        rows,
    };
    let inbound = InboundTables {
        inherits: inbound_table(totals[0], inherits_rows),
        uses_type: inbound_table(totals[1], uses_type_rows),
        uses_member: inbound_table(totals[2], uses_member_rows),
        implements: inbound_table(totals[3], implements_rows),
        overrides: inbound_table(totals[4], overrides_rows),
        ts: TsTables {
            call: inbound_table(totals[5], call_rows),
            jsx_use: inbound_table(totals[6], jsx_use_rows),
            dispatch: inbound_table(totals[7], dispatch_rows),
            import: inbound_table(totals[8], import_rows),
        },
    };

    let outbound = out.then(|| {
        build_outbound_tables(
            &refs,
            def_project,
            edges,
            &index.root,
            if all_out { usize::MAX } else { outbound_cap },
        )
    });
    let ambiguous = AmbiguousTables {
        inbound: build_table(refs.ambiguous_inbound, edges, cap, ambiguous_row),
        outbound: build_table(refs.ambiguous_outbound, edges, cap, ambiguous_row),
    };
    let bus = bus::symbol_bus_rows(index, &id, cap);

    let member_refs = if def.kind == "enum" {
        enum_member_refs(index, &id)
    } else {
        None
    };

    RefsResult::Resolved(RefsModel {
        query: query.to_string(),
        id: id.clone(),
        kind: def.kind.clone(),
        sites: def_sites(index, &id),
        inbound,
        outbound,
        ambiguous,
        bus,
        manifest_gap: index.flagged_files.len(),
        member_refs,
    })
}
