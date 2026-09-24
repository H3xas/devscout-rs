// `AuditReport` -- `score`'s pure output, and the pass that builds it. Every
// count is a `usize`; ratios are computed at render time
// (`render::ratio_text`/`render::ratio_j`) from a hit count and a
// denominator, both kept, rather than as a pre-divided `f64` -- there is
// exactly one place (each renderer) that has to decide how a zero
// denominator prints, instead of that decision being baked into the data.

use std::collections::{HashMap, HashSet};

use super::fp_sites::FpSite;
use super::model::{DefRow, EdgeRow, Inputs, OracleRef, Tier};
use super::report::TierStats;
use super::scoring::{
    file_to_unit, is_structural, is_unjudged_discovered_edge, member_matches, reach, short_name,
    target_known, target_matches,
};

pub struct AuditReport {
    pub root: String,
    /// `"syntax"` or `"enriched"` -- see `Inputs::lane`'s own doc comment.
    pub lane: &'static str,
    pub oracle_records: usize,
    pub oracle_sites: usize,
    pub oracle_external_sites: usize,
    pub oracle_ambiguous: usize,
    pub oracle_dropped: usize,
    pub units_ok: usize,
    pub units_failed: usize,
    pub structural_method: &'static str,
    /// Only tiers with `edges > 0`, in `Tier::ORDER`.
    pub tiers: Vec<(Tier, TierStats)>,
    pub recall_denominator: usize,
    pub recall_precise: usize,
    pub recall_precise_ext: usize,
    pub recall_all: usize,
    /// Fixed order: ident, qualified, this, base, call -- the order the text
    /// and JSON renderers both print. `None` when that receiver kind has no
    /// D-eligible record at all (printed `-` in text, `null` in JSON),
    /// `Some(hits as a fraction of that bucket's denominator)` otherwise.
    pub by_receiver: Vec<(&'static str, Option<f64>)>,
    pub recall_conditional: usize,
    pub recall_bare: usize,
    pub silent_correct: usize,
    pub silent_leak: usize,
    pub structural_impossible: usize,
    pub structural_checked: usize,
    /// Sites-with-N-edges histogram, buckets `[1, 2, 3, "4+"]`.
    pub fanout: [usize; 4],
    /// `(targetKind, distinct-target count)`, sorted by count desc then kind
    /// asc.
    pub unknown_targets: Vec<(String, usize)>,
    /// `(short name, count)`, sorted by count desc then name asc, top 20.
    pub top_fp: Vec<(String, usize)>,
    /// `(target id, count)`, sorted by count desc then id asc, top 20.
    pub top_missed: Vec<(String, usize)>,
    pub partial_file_mismatch: usize,
    /// `--units` edges only (always 0 with no `--units`): a `uses-member`
    /// edge whose `from_file` fell outside `universe` -- a project the
    /// oracle's `.sln` never compiled at all, ok or failed, so there is no
    /// ground truth for this edge one way or the other. Dropped before
    /// every other signal below (tiers, recall, structural, fan-out), the
    /// same as an out-of-universe oracle record, and counted on its own:
    /// unlike `fp_no_site` (a site the oracle DID walk and simply saw no
    /// reference on this exact line), this edge was never in the oracle's
    /// judged universe to begin with, so it is neither a true nor a false
    /// positive.
    pub edges_outside_universe: usize,
    /// One row per false positive, in scoring order. Built only when
    /// `Inputs.collect_fp_sites` is set, written only by `--fp-sites`;
    /// nothing the report prints reads it.
    pub fp_sites: Vec<FpSite>,
}

/// Appends one row when `collect` is set; a no-op otherwise. Kept as its own
/// function, not an inline `if`, so the opt-in branch does not add to
/// `score`'s own cognitive-complexity budget -- the loop it is called from
/// already carries the tier/class decision.
fn push_fp_site(
    fp_rows: &mut Vec<FpSite>,
    collect: bool,
    e: &EdgeRow,
    class: &'static str,
    evidence: &[&OracleRef],
    structural: bool,
) {
    if collect {
        fp_rows.push(FpSite::new(e, class, evidence, structural));
    }
}

/// Scores `inputs` into a full `AuditReport`. Pure: every branch below reads
/// only `inputs` and locally built indexes over it.
#[allow(
    clippy::too_many_lines,
    reason = "one ordered scoring pass building every report tier from the same inputs; the tiers must stay in the order the report promises"
)]
pub fn score(inputs: Inputs) -> AuditReport {
    let root = inputs.root.display().to_string();
    let collect_fp_sites = inputs.collect_fp_sites;

    // "units" method (below) applies exactly when `--units` produced at
    // least one unit -- computed once, up front, since both the edge-universe
    // filter just below and the structural-check setup further down switch
    // on it.
    let units_method = !inputs.units.is_empty();

    // Universe filter: an oracle record whose file is outside `universe` is
    // dropped and counted, never scored.
    let mut oracle_dropped = 0usize;
    let records: Vec<&OracleRef> = inputs
        .records
        .iter()
        .filter(|r| {
            let keep = inputs.universe.contains(&r.file);
            if !keep {
                oracle_dropped += 1;
            }
            keep
        })
        .collect();

    // Universe filter, edge side: with `--units`, a `uses-member` edge whose
    // `from_file` is outside `universe` (a project the oracle's `.sln` never
    // compiled, ok or failed) is dropped before every signal below -- it is
    // neither a true nor a false positive, just unjudgeable, the same
    // reasoning as the record-side filter just above. With no `--units`
    // (`units_method` false), `universe` carries no per-file compile status
    // to filter by (see `load::build_universe`), so nothing is dropped here
    // -- this stays a no-op, matching every edge's pre-refinement fate.
    let mut edges_outside_universe = 0usize;
    let edges: Vec<&EdgeRow> = inputs
        .edges
        .iter()
        .filter(|e| {
            let keep = !units_method || inputs.universe.contains(&e.from_file);
            if !keep {
                edges_outside_universe += 1;
            }
            keep
        })
        .collect();

    let graph_defs_by_id: HashMap<&str, &DefRow> = inputs
        .graph_defs
        .iter()
        .map(|d| (d.id.as_str(), d))
        .collect();

    // Site indexes: every kept record, and every kept uses-member edge,
    // grouped by `(file, startLine)`/`(from_file, from_line)`.
    let mut by_site: HashMap<(String, usize), Vec<&OracleRef>> = HashMap::new();
    for r in &records {
        by_site
            .entry((r.file.clone(), r.start_line))
            .or_default()
            .push(r);
    }
    let mut edges_by_site: HashMap<(String, usize), Vec<&EdgeRow>> = HashMap::new();
    for e in edges.iter().copied() {
        edges_by_site
            .entry((e.from_file.clone(), e.from_line))
            .or_default()
            .push(e);
    }

    let oracle_sites = by_site.len();
    let oracle_ambiguous = records.iter().filter(|r| r.ambiguous).count();

    // Silent-correct / leak: sites where every `shape == "access"` record is
    // external.
    let mut access_by_site: HashMap<(String, usize), Vec<&OracleRef>> = HashMap::new();
    for r in records.iter().filter(|r| r.shape == "access") {
        access_by_site
            .entry((r.file.clone(), r.start_line))
            .or_default()
            .push(r);
    }
    let mut silent_correct = 0usize;
    let mut silent_leak = 0usize;
    for (site, recs) in &access_by_site {
        if recs.iter().all(|r| r.external) {
            if edges_by_site.contains_key(site) {
                silent_leak += 1;
            } else {
                silent_correct += 1;
            }
        }
    }
    let oracle_external_sites = silent_correct + silent_leak;

    // Structural check setup: "units" method when `--units` produced at
    // least one unit, else the "test-defs" fallback.
    let structural_method: &'static str = if units_method { "units" } else { "test-defs" };
    let file_unit = file_to_unit(&inputs.units);
    let reach_map = reach(&inputs.units);
    // `test_by_id`/`test_files`: graph defs first, oracle `--defs` entries
    // layered on top (a later insert overwrites an earlier one for the same
    // id) -- an oracle def, when given, is the more authoritative "is this a
    // test-attributed def" signal (see `DefRow`'s doc comment).
    //
    // `test_files` is derived from the MERGED map afterwards rather than
    // accumulated during the merge: a set only ever grows, so an oracle row
    // saying `test:false` for a def the graph called a test def could never
    // take its file back out again, and the file would stay marked a test file
    // on the strength of a verdict the merge had already overruled. Deriving
    // it at the end means the layering wins for the file exactly as it wins
    // for the def.
    let mut test_by_id: HashMap<String, bool> = HashMap::new();
    let mut file_by_id: HashMap<String, String> = HashMap::new();
    for d in inputs.graph_defs.iter().chain(inputs.oracle_defs.iter()) {
        test_by_id.insert(d.id.clone(), d.test);
        file_by_id.insert(d.id.clone(), d.file.clone());
    }
    // A file is a test file when ANY def the merged view still calls a test
    // def is declared in it.
    let test_files: HashSet<String> = test_by_id
        .iter()
        .filter(|&(_, &t)| t)
        .filter_map(|(id, _)| file_by_id.get(id).cloned())
        .collect();
    let mut structural_impossible = 0usize;
    let mut structural_checked = 0usize;
    let edge_structural: Vec<bool> = edges
        .iter()
        .copied()
        .map(|e| {
            match is_structural(
                e,
                units_method,
                &file_unit,
                &reach_map,
                &test_by_id,
                &test_files,
            ) {
                Some(flag) => {
                    structural_checked += 1;
                    if flag {
                        structural_impossible += 1;
                    }
                    flag
                }
                None => false,
            }
        })
        .collect();

    // Per-tier TP/FP classification, fan-out and top-fp-targets tallies.
    let mut tiers: HashMap<Tier, TierStats> = HashMap::new();
    let mut fp_targets: HashMap<String, usize> = HashMap::new();
    let mut fanout_sites: HashMap<(String, usize), usize> = HashMap::new();
    for e in edges.iter().copied() {
        *fanout_sites
            .entry((e.from_file.clone(), e.from_line))
            .or_insert(0) += 1;
    }
    let mut partial_file_mismatch = 0usize;
    let mut fp_rows: Vec<FpSite> = Vec::new();

    for (i, e) in edges.iter().copied().enumerate() {
        let stats = tiers.entry(e.tier).or_default();
        stats.edges += 1;

        // Unjudged, `SemanticDiscovered` only: a discovered edge whose own
        // occurrence shape is `"identifier"` has no oracle vocabulary that
        // could ever match it (the oracle's walker records no case for a
        // bare field/property/event read at all), so scoring it as a false
        // positive would be scoring the oracle's own blind spot, not this
        // edge's correctness. Reported separately (`TierStats::unjudged`,
        // excluded from `tp`/`fp` and from `precision`'s denominator) rather
        // than silently dropped, so the population is still counted in
        // `edges` and visible in the report.
        if is_unjudged_discovered_edge(e, &inputs.discovered_shapes) {
            stats.unjudged += 1;
            continue;
        }

        let site_key = (e.from_file.clone(), e.from_line);
        // Whether this edge landed a true positive -- tracked separately
        // from the FP/TP branch below so the structural tally after it can
        // count a structurally-impossible edge only when it is ALSO a false
        // positive (`TierStats.structural`'s doc comment).
        let is_tp = match by_site.get(&site_key) {
            None => {
                stats.fp += 1;
                stats.fp_no_site += 1;
                *fp_targets.entry(short_name(&e.to).to_string()).or_insert(0) += 1;
                push_fp_site(
                    &mut fp_rows,
                    collect_fp_sites,
                    e,
                    "no-site",
                    &[],
                    edge_structural[i],
                );
                false
            }
            Some(recs) => {
                let tp_record = recs.iter().find(|r| {
                    !r.external && target_matches(r, &e.to) && member_matches(&e.member, &r.member)
                });
                if let Some(r) = tp_record {
                    stats.tp += 1;
                    if let Some(tf) = &r.target_file {
                        if tf != &e.to_file {
                            partial_file_mismatch += 1;
                        }
                    }
                    true
                } else {
                    // Splitting this false positive into "the site really is
                    // an external API and the edge leaked" vs "the site has an
                    // in-tree answer and the edge picked the wrong one", in
                    // two steps:
                    //
                    //  1. Scope to the records naming the edge's OWN member.
                    //     A different-member record sharing this source line
                    //     (a fluent chain's outer call, a lambda parameter
                    //     access) is not evidence about the reference this
                    //     edge represents -- the fixture's
                    //     `entity.Property(e => e.Name)` case
                    //     (`AppDbContext.cs`), where an unrelated `e.Name`
                    //     record on the same line used to make a guessed
                    //     `Property(...)` edge look like a wrong-target miss
                    //     instead of the external-API leak it is. If any
                    //     scoped record is in-tree the edge got a real answer
                    //     wrong (`fp_wrong_target`); if every scoped record is
                    //     external it is a leak (`fp_external_site`). A
                    //     no-member edge (legacy path) is unconstrained, so
                    //     its scoped set is every record at the site.
                    //
                    //  2. When the scoped set is EMPTY -- the edge names a
                    //     member no record at this site names at all -- there
                    //     is no same-member evidence to read, so fall back to
                    //     the whole site: any in-tree record there makes it
                    //     `fp_wrong_target`, and a site whose every record is
                    //     external makes it `fp_external_site`. Falling back
                    //     rather than letting a vacuous "all records are
                    //     external" over an empty set call every such edge a
                    //     leak, which is how an invented member at a
                    //     thoroughly in-tree site used to be counted.
                    let scoped: Vec<&&OracleRef> = recs
                        .iter()
                        .filter(|r| member_matches(&e.member, &r.member))
                        .collect();
                    stats.fp += 1;
                    let evidence: Vec<&OracleRef> = if scoped.is_empty() {
                        recs.clone()
                    } else {
                        scoped.iter().map(|r| **r).collect()
                    };
                    let external_site = evidence.iter().all(|r| r.external);
                    let class = if external_site {
                        stats.fp_external_site += 1;
                        "external"
                    } else {
                        stats.fp_wrong_target += 1;
                        "wrong-target"
                    };
                    *fp_targets.entry(short_name(&e.to).to_string()).or_insert(0) += 1;
                    push_fp_site(
                        &mut fp_rows,
                        collect_fp_sites,
                        e,
                        class,
                        &evidence,
                        edge_structural[i],
                    );
                    false
                }
            }
        };
        stats.structural += usize::from(!is_tp && edge_structural[i]);
    }
    let tiers_ordered: Vec<(Tier, TierStats)> = Tier::ORDER
        .into_iter()
        .filter_map(|t| tiers.get(&t).filter(|ts| ts.edges > 0).map(|ts| (t, *ts)))
        .collect();

    // Recall D: shape == "access", non-external, target known to the graph.
    let site_hit = |r: &OracleRef, tier_ok: &dyn Fn(Tier) -> bool| -> bool {
        edges_by_site
            .get(&(r.file.clone(), r.start_line))
            .is_some_and(|es| {
                es.iter().any(|e| {
                    tier_ok(e.tier)
                        && target_matches(r, &e.to)
                        && member_matches(&e.member, &r.member)
                })
            })
    };
    let d_records: Vec<&&OracleRef> = records
        .iter()
        .filter(|r| r.shape == "access" && !r.external && target_known(&graph_defs_by_id, r))
        .collect();
    let recall_denominator = d_records.len();
    // A `Semantic` edge (a per-reference override of an already-emitted
    // reference) is precise-class recall by construction: it either confirms
    // or corrects what the ladder would have answered at that exact site, so
    // it joins both `recall_precise` and `recall_precise_ext` alongside the
    // syntax-lane tiers they already count. `SemanticDiscovered` joins
    // neither -- a discovered site is a structurally different population
    // (never extractor-emitted, so never comparable to a syntax-lane figure)
    // and is reported only in `recall_all`, its own `TierStats` row, and a
    // dedicated gap report; folding it in here would make the shipping
    // gate's enriched-vs-syntax precise+ext comparison no longer
    // apples-to-apples.
    let recall_precise = d_records
        .iter()
        .filter(|r| site_hit(r, &|t| t == Tier::Precise || t == Tier::Semantic))
        .count();
    let recall_precise_ext = d_records
        .iter()
        .filter(|r| {
            site_hit(r, &|t| {
                t == Tier::Precise || t == Tier::Ext || t == Tier::Semantic
            })
        })
        .count();
    let recall_all = d_records.iter().filter(|r| site_hit(r, &|_| true)).count();

    let mut top_missed: HashMap<String, usize> = HashMap::new();
    for r in &d_records {
        if !site_hit(r, &|_| true) {
            // `target_known` guarantees `target` is `Some`.
            *top_missed.entry(r.target.clone().unwrap()).or_insert(0) += 1;
        }
    }

    let by_receiver: Vec<(&'static str, Option<f64>)> =
        ["ident", "qualified", "this", "base", "call"]
            .into_iter()
            .map(|k| {
                let subset: Vec<_> = d_records.iter().filter(|r| r.receiver_kind == k).collect();
                if subset.is_empty() {
                    (k, None)
                } else {
                    let hits = subset.iter().filter(|r| site_hit(r, &|_| true)).count();
                    (k, Some(hits as f64 / subset.len() as f64))
                }
            })
            .collect();

    let recall_conditional = records
        .iter()
        .filter(|r| r.shape == "conditional" && !r.external && target_known(&graph_defs_by_id, r))
        .count();
    let recall_bare = records
        .iter()
        .filter(|r| r.shape == "bare" && !r.external && target_known(&graph_defs_by_id, r))
        .count();

    // Unknown targets: in-solution (per the record's own `external == false`)
    // but not a def devscout's graph knows about, grouped by targetKind.
    let mut unknown_by_kind: HashMap<String, HashSet<String>> = HashMap::new();
    for r in &records {
        if r.external {
            continue;
        }
        let Some(t) = &r.target else { continue };
        if target_known(&graph_defs_by_id, r) {
            continue;
        }
        let kind = r
            .target_kind
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        unknown_by_kind.entry(kind).or_default().insert(t.clone());
    }
    let mut unknown_targets: Vec<(String, usize)> = unknown_by_kind
        .into_iter()
        .map(|(k, set)| (k, set.len()))
        .collect();
    unknown_targets.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let mut fanout = [0usize; 4];
    for n in fanout_sites.values() {
        match n {
            1 => fanout[0] += 1,
            2 => fanout[1] += 1,
            3 => fanout[2] += 1,
            _ => fanout[3] += 1,
        }
    }

    let mut top_fp: Vec<(String, usize)> = fp_targets.into_iter().collect();
    top_fp.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    top_fp.truncate(20);
    let mut top_missed: Vec<(String, usize)> = top_missed.into_iter().collect();
    top_missed.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    top_missed.truncate(20);

    let units_ok = inputs.units.iter().filter(|u| u.status == "ok").count();
    let units_failed = inputs.units.len() - units_ok;

    AuditReport {
        root,
        lane: inputs.lane,
        oracle_records: records.len(),
        oracle_sites,
        oracle_external_sites,
        oracle_ambiguous,
        oracle_dropped,
        units_ok,
        units_failed,
        structural_method,
        tiers: tiers_ordered,
        recall_denominator,
        recall_precise,
        recall_precise_ext,
        recall_all,
        by_receiver,
        recall_conditional,
        recall_bare,
        silent_correct,
        silent_leak,
        structural_impossible,
        structural_checked,
        fanout,
        unknown_targets,
        top_fp,
        top_missed,
        partial_file_mismatch,
        edges_outside_universe,
        fp_sites: fp_rows,
    }
}
