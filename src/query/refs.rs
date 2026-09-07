use std::collections::HashMap;
use std::path::Path;

use crate::graph;

use super::index::{def_sites, symbol_refs, DefSite, GraphIndex, SymbolRefs};
use super::refs_tables::{
    ambiguous_row, build_table, cap_rows, edge_loc, loc_cmp, row_tier, AmbiguousTables, ImportRow,
    InboundRow, InboundTables, OutboundRow, OutboundTables, Table, SOURCE_MAX,
};
use super::symbol::{resolve_symbol, Resolution};

/// One enum member that carries at least one inbound reference.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberRefEntry {
    /// The name value.
    pub name: String,
    /// The count value.
    pub count: usize,
}

/// How many of an ENUM's inbound member edges land on one of its members,
/// split by member.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberRefs {
    /// The total value.
    pub total: usize,
    /// The member count value.
    pub member_count: usize,
    /// The members value.
    pub members: Vec<MemberRefEntry>,
    /// The dropped value.
    pub dropped: usize,
}

// Maximum number of per-member rows kept in a `MemberRefs`.
const MEMBER_NAME_CAP: usize = 5;

// The edges themselves are already there (a `Toggles.EnableX` access resolves
// to the member def and `symbol_refs` unions every member's inbound into the
// enum's), but a caller reading `refs Toggles` could not tell a use of the
// TYPE from a use of a member, and the per-member split is the thing an enum
// question is usually actually about.
//
// Member order is def-table order (declaration order), never by count: a
// stable list is what makes two runs print the same line. Iterates
// `graph.defs` directly for the same reason `symbol_refs`'s own enum union
// does -- that array order is the order to preserve.
fn enum_member_refs(index: &GraphIndex, def_id: &str) -> Option<MemberRefs> {
    let prefix = format!("{def_id}.");
    let mut members: Vec<MemberRefEntry> = Vec::new();
    let mut total = 0usize;
    for d in &index.graph.defs {
        if d.kind != "enum-member" || !d.id.starts_with(&prefix) {
            continue;
        }
        let count = index.inbound.get(&d.id).map_or(0, |e| e.uses_member.len())
            + index
                .heuristic_inbound
                .get(&d.id)
                .map_or(0, |e| e.uses_member.len());
        if count == 0 {
            continue;
        }
        members.push(MemberRefEntry {
            name: d.name.clone(),
            count,
        });
        total += count;
    }
    if total == 0 {
        return None;
    }
    let member_count = members.len();
    let dropped = member_count.saturating_sub(MEMBER_NAME_CAP);
    members.truncate(MEMBER_NAME_CAP);
    Some(MemberRefs {
        total,
        member_count,
        members,
        dropped,
    })
}

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
    /// Represents `NotFound`.
    NotFound,
}

// A file's project is the first segment of its repo-relative path -- the graph
// carries no other grouping, and the paths it stores are always repo-relative
// with `/` separators. A file at the repo root has the empty project, which
// only ever matches another root file.
fn project_of(file: &str) -> &str {
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
fn hit_source(root: &Path, file: &str, line: usize, cache: &mut LineCache) -> String {
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

// Every type that declares `name`, in name-index order, each with the sites it
// declares it at. Two overloads are two sites on ONE type, never two
// candidates. Markup and resource rows carry no `owner`, so nothing a markup
// file names can be mistaken for a member.
fn member_owners(index: &GraphIndex, name: &str) -> Vec<(String, Vec<DefSite>)> {
    let mut out: Vec<(String, Vec<DefSite>)> = Vec::new();
    let mut at: HashMap<&str, usize> = HashMap::new();
    for n in &index.graph.names {
        if n.name != name || n.owner.is_empty() || !index.by_id.contains_key(&n.owner) {
            continue;
        }
        let site = DefSite {
            file: n.file.clone(),
            line: n.line,
        };
        match at.get(n.owner.as_str()) {
            Some(&i) => out[i].1.push(site),
            None => {
                at.insert(n.owner.as_str(), out.len());
                out.push((n.owner.clone(), vec![site]));
            }
        }
    }
    out
}

// The two non-empty outcomes of `build_member_refs_models`: a plain models
// array, or an ambiguity -- more than one declaring type surviving edge-line
// verification is reported, never turned into several models.
enum MemberRefsOutcome {
    Models(Vec<RefsModel>),
    Ambiguous(Vec<String>),
}

// `refs <bare member>`: the name index names the declaring type(s); each of
// that type's inbound `uses-member` edges survives only if the line it starts
// on carries the member as a whole token. A type whose edges all fail
// verification contributes no model at all, and when none survives the caller
// takes the zero-hit path.
//
// More than one declaring type surviving verification answers
// `Ambiguous(owner_ids)`, in name-index order, rather than several models --
// the house rule of never guessing between candidates. Overloads of one name
// on ONE type are one group (one entry in `owners`, several sites), never an
// ambiguity.
fn build_member_refs_models(
    index: &GraphIndex,
    name: &str,
    inbound_cap: usize,
) -> Option<MemberRefsOutcome> {
    let owners = member_owners(index, name);
    if owners.is_empty() {
        return None;
    }
    let edges = &index.graph.edges;
    let mut cache: LineCache = HashMap::new();
    let mut groups: Vec<(String, Vec<DefSite>, Vec<(usize, bool)>)> = Vec::new();
    for (owner, sites) in owners {
        let refs = symbol_refs(index, &owner);
        let owner_def = &index.graph.defs[index.by_id[&owner]];
        let owner_project = project_of(&owner_def.file).to_string();
        let mut kept: Vec<(usize, bool)> = Vec::new();
        for &e in &refs.inbound_uses_member {
            kept.push((e, false));
        }
        for &e in &refs.heuristic_inbound_uses_member {
            kept.push((e, true));
        }
        kept.retain(|&(e, _)| {
            let (file, line) = edge_loc(&edges[e]);
            line_has_token(&cached_line(&index.root, file, line, &mut cache), name)
        });
        // Same ranking a type's inbound table uses: precise before guessed, the
        // declaring type's own project before every other, then file, then
        // line.
        let foreign = |e: usize| usize::from(project_of(edge_loc(&edges[e]).0) != owner_project);
        kept.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| foreign(a.0).cmp(&foreign(b.0)))
                .then_with(|| loc_cmp(&edges[a.0], &edges[b.0]))
        });
        if !kept.is_empty() {
            groups.push((owner, sites, kept));
        }
    }
    if groups.is_empty() {
        return None;
    }
    // Reported before the inbound cap is ever spent, so the answer does not
    // depend on `inbound_cap`: an ambiguity is a refusal, not a budgeted,
    // capped multi-block resolution.
    if groups.len() > 1 {
        return Some(MemberRefsOutcome::Ambiguous(
            groups.into_iter().map(|(owner, _, _)| owner).collect(),
        ));
    }

    fn empty<R>() -> Table<R> {
        Table {
            total: 0,
            dropped: 0,
            rows: Vec::new(),
        }
    }
    let mut budget = inbound_cap;
    let mut models = Vec::new();
    for (owner, sites, kept) in groups {
        let take = budget.min(kept.len());
        budget -= take;
        let mut rows = Vec::new();
        for &(e, heuristic) in &kept[..take] {
            let (file, line) = edge_loc(&edges[e]);
            let source = clip_source(&cached_line(&index.root, file, line, &mut cache));
            rows.push(InboundRow {
                file: file.to_string(),
                line,
                heuristic,
                tier: row_tier(
                    heuristic,
                    edges[e].tier() == Some(graph::HeuristicTier::Ext),
                ),
                source,
            });
        }
        let total = kept.len();
        models.push(RefsModel {
            query: name.to_string(),
            id: format!("{owner}.{name}"),
            kind: "member".to_string(),
            sites,
            inbound: InboundTables {
                inherits: empty(),
                uses_type: empty(),
                uses_member: Table {
                    total,
                    dropped: total - rows.len(),
                    rows,
                },
            },
            outbound: None,
            ambiguous: AmbiguousTables {
                inbound: empty(),
                outbound: empty(),
            },
            manifest_gap: index.flagged_files.len(),
            member_refs: None,
        });
    }
    Some(MemberRefsOutcome::Models(models))
}

/// One inbound edge awaiting the global cap: which kind's table it belongs to,
/// which edge it is, and whether it was guessed.
struct RankedInbound {
    kind: usize,
    edge: usize,
    heuristic: bool,
}

/// One outbound edge awaiting the shared `--out` cap: which of the four kinds
/// it belongs to (0=inherits, 1=uses-type, 2=uses-member, 3=imports), which
/// edge it is, and whether it was guessed. `imports` is never a guess -- the
/// builder never marks one heuristic, by construction (see
/// `build_outbound_tables` below).
struct RankedOutbound {
    kind: usize,
    edge: usize,
    heuristic: bool,
}

// The three ref kinds name a `to_file` -- ranked same-project/foreign against
// it, exactly as an inbound edge ranks its `from_file`. An imports edge names a
// namespace string, never a file, so nothing proves it shares the def's own
// project: it never earns the same-project rank and always sorts as foreign,
// the never-guess rule applied to ranking rather than to resolution.
fn outbound_foreign(def_project: &str, edges: &[graph::Edge], r: &RankedOutbound) -> usize {
    if r.kind == 3 {
        return 1;
    }
    let to_file = match &edges[r.edge] {
        graph::Edge::Inherits { to_file, .. }
        | graph::Edge::UsesType { to_file, .. }
        | graph::Edge::UsesMember { to_file, .. } => to_file.as_str(),
        _ => unreachable!(
            "outbound ranked kinds 0-2 only ever hold inherits/uses-type/uses-member edge indices"
        ),
    };
    usize::from(project_of(to_file) != def_project)
}

// The four outbound kinds share ONE cap and ONE ranking under `--out`,
// mirroring `build_refs_model`'s own inbound block below: resolved before
// heuristic, then the def's own project before every other (imports always
// foreign, per `outbound_foreign` above), then file, then line. Each shown hit
// carries the same trimmed source line an inbound hit does, read at the SAME
// `from_line` an inbound row reads -- for an outbound edge that is a line in
// the def's own file, the site actually making the reference, not the caller's.
fn build_outbound_tables(
    refs: &SymbolRefs,
    def_project: &str,
    edges: &[graph::Edge],
    root: &Path,
    outbound_cap: usize,
) -> OutboundTables {
    let mut ranked: Vec<RankedOutbound> = Vec::new();
    for &e in &refs.outbound_inherits {
        ranked.push(RankedOutbound {
            kind: 0,
            edge: e,
            heuristic: false,
        });
    }
    for &e in &refs.heuristic_outbound_inherits {
        ranked.push(RankedOutbound {
            kind: 0,
            edge: e,
            heuristic: true,
        });
    }
    for &e in &refs.outbound_uses_type {
        ranked.push(RankedOutbound {
            kind: 1,
            edge: e,
            heuristic: false,
        });
    }
    for &e in &refs.heuristic_outbound_uses_type {
        ranked.push(RankedOutbound {
            kind: 1,
            edge: e,
            heuristic: true,
        });
    }
    for &e in &refs.outbound_uses_member {
        ranked.push(RankedOutbound {
            kind: 2,
            edge: e,
            heuristic: false,
        });
    }
    for &e in &refs.heuristic_outbound_uses_member {
        ranked.push(RankedOutbound {
            kind: 2,
            edge: e,
            heuristic: true,
        });
    }
    for &e in &refs.outbound_imports {
        ranked.push(RankedOutbound {
            kind: 3,
            edge: e,
            heuristic: false,
        });
    }
    let mut totals = [0usize; 4];
    for r in &ranked {
        totals[r.kind] += 1;
    }
    ranked.sort_by(|a, b| {
        a.heuristic
            .cmp(&b.heuristic)
            .then_with(|| {
                outbound_foreign(def_project, edges, a).cmp(&outbound_foreign(
                    def_project,
                    edges,
                    b,
                ))
            })
            .then_with(|| loc_cmp(&edges[a.edge], &edges[b.edge]))
    });
    let (shown, _) = cap_rows(ranked, outbound_cap);

    let mut source_cache: LineCache = HashMap::new();
    let mut inherits = Vec::new();
    let mut uses_type = Vec::new();
    let mut uses_member = Vec::new();
    let mut imports = Vec::new();
    for r in shown {
        let (file, line) = edge_loc(&edges[r.edge]);
        let source = hit_source(root, file, line, &mut source_cache);
        if r.kind == 3 {
            let graph::Edge::Imports { target, .. } = &edges[r.edge] else {
                unreachable!("outbound kind 3 only ever holds imports edge indices");
            };
            imports.push(ImportRow {
                file: file.to_string(),
                line,
                target: target.clone(),
                source,
            });
            continue;
        }
        let (to_file, to) = match &edges[r.edge] {
            graph::Edge::Inherits { to_file, to, .. }
            | graph::Edge::UsesType { to_file, to, .. }
            | graph::Edge::UsesMember { to_file, to, .. } => (to_file.clone(), to.clone()),
            _ => unreachable!("outbound ranked kinds 0-2 only ever hold inherits/uses-type/uses-member edge indices"),
        };
        let row = OutboundRow {
            file: file.to_string(),
            line,
            to_file,
            to,
            heuristic: r.heuristic,
            tier: row_tier(
                r.heuristic,
                edges[r.edge].tier() == Some(graph::HeuristicTier::Ext),
            ),
            source,
        };
        match r.kind {
            0 => inherits.push(row),
            1 => uses_type.push(row),
            _ => uses_member.push(row),
        }
    }

    OutboundTables {
        inherits: Table {
            total: totals[0],
            dropped: totals[0] - inherits.len(),
            rows: inherits,
        },
        uses_type: Table {
            total: totals[1],
            dropped: totals[1] - uses_type.len(),
            rows: uses_type,
        },
        uses_member: Table {
            total: totals[2],
            dropped: totals[2] - uses_member.len(),
            rows: uses_member,
        },
        imports: Table {
            total: totals[3],
            dropped: totals[3] - imports.len(),
            rows: imports,
        },
    }
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
        Resolution::NotFound => {
            return match build_member_refs_models(index, query, inbound_cap) {
                Some(MemberRefsOutcome::Models(models)) => RefsResult::Members(models),
                Some(MemberRefsOutcome::Ambiguous(ids)) => RefsResult::Ambiguous(ids),
                None => RefsResult::NotFound,
            };
        }
    };
    let def_idx = *index
        .by_id
        .get(&id)
        .expect("resolved id must exist in the index it was resolved from");
    let def = &index.graph.defs[def_idx];
    let refs = symbol_refs(index, &id);
    let edges = &index.graph.edges;

    // The three inbound kinds share ONE cap and ONE ranking, so a widely used
    // type spends its whole budget on the most specific edges it has rather
    // than on whichever kind the walk reached first. Rank: precise before
    // heuristic, then the def's own project before every other, then file,
    // then line. Push order (kind by kind, precise then heuristic within a
    // kind) is load-bearing: the sort is stable, so two edges of different
    // kinds at the same file:line keep their push order.
    let def_project = project_of(&def.file);
    let mut ranked: Vec<RankedInbound> = Vec::new();
    let mut totals = [0usize; 3];
    let is_self_inbound = |edge: usize| {
        let (file, line) = edge_loc(&edges[edge]);
        exclude_self_inbound
            && def.end_line >= def.line
            && file == def.file
            && (def.line..=def.end_line).contains(&line)
    };
    for (kind, (precise, heuristic)) in [
        (&refs.inbound_inherits, &refs.heuristic_inbound_inherits),
        (&refs.inbound_uses_type, &refs.heuristic_inbound_uses_type),
        (
            &refs.inbound_uses_member,
            &refs.heuristic_inbound_uses_member,
        ),
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
    let mut rows: [Vec<InboundRow>; 3] = [Vec::new(), Vec::new(), Vec::new()];
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
        });
    }
    let [inherits_rows, uses_type_rows, uses_member_rows] = rows;
    let inbound_table = |total: usize, rows: Vec<InboundRow>| Table {
        total,
        dropped: total - rows.len(),
        rows,
    };
    let inbound = InboundTables {
        inherits: inbound_table(totals[0], inherits_rows),
        uses_type: inbound_table(totals[1], uses_type_rows),
        uses_member: inbound_table(totals[2], uses_member_rows),
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
        manifest_gap: index.flagged_files.len(),
        member_refs,
    })
}
