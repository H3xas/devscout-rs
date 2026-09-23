// Rendering -- text (default) and `--json` (via cli.rs's `J`).

use crate::query::json::J;

use super::model::Tier;
use super::score::AuditReport;

/// `-`  when `denom == 0` (no eligible record at all -- an undefined ratio,
/// not a zero one), else the hit rate to 3 decimals.
pub fn ratio_text(hits: usize, denom: usize) -> String {
    if denom == 0 {
        "-".to_string()
    } else {
        format!("{:.3}", hits as f64 / denom as f64)
    }
}

/// Same rule as `ratio_text`, JSON-shaped: a bare `null` (via `J::RawNum`,
/// which writes its string argument through unescaped -- `J` has no `Null`
/// variant, and this is the one place a ratio has no value to report)
/// instead of `-`.
pub fn ratio_j(hits: usize, denom: usize) -> J {
    if denom == 0 {
        J::RawNum("null".to_string())
    } else {
        J::RawNum(format!("{:.3}", hits as f64 / denom as f64))
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one ordered pass rendering every report section in the fixed order the text output promises"
)]
pub fn render_text(r: &AuditReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "devscout audit --semantic  root {}  lane {}  oracle {} records / {} sites  units ok {} failed {}  method {}",
        r.root, r.lane, r.oracle_records, r.oracle_sites, r.units_ok, r.units_failed, r.structural_method
    ));

    if !r.tiers.is_empty() {
        lines.push(format!(
            "{:<10}{:>7}{:>7}{:>7}{:>12}{:>13}{:>13}{:>10}{:>12}{:>10}",
            "tier",
            "edges",
            "tp",
            "fp",
            "precision",
            "fp:no-site",
            "fp:external",
            "fp:wrong",
            "structural",
            "unjudged"
        ));
        for (tier, ts) in &r.tiers {
            lines.push(format!(
                "{:<10}{:>7}{:>7}{:>7}{:>12.3}{:>13}{:>13}{:>10}{:>12}{:>10}",
                tier.key(),
                ts.edges,
                ts.tp,
                ts.fp,
                ts.precision(),
                ts.fp_no_site,
                ts.fp_external_site,
                ts.fp_wrong_target,
                ts.structural,
                ts.unjudged,
            ));
        }
    }

    lines.push(format!(
        "recall ({} in-graph member sites)  precise {}  precise+ext {}  all {}",
        r.recall_denominator,
        ratio_text(r.recall_precise, r.recall_denominator),
        ratio_text(r.recall_precise_ext, r.recall_denominator),
        ratio_text(r.recall_all, r.recall_denominator),
    ));
    let by_receiver = r
        .by_receiver
        .iter()
        .map(|(k, v)| match v {
            Some(x) => format!("{k} {x:.3}"),
            None => format!("{k} -"),
        })
        .collect::<Vec<_>>()
        .join("  ");
    lines.push(format!("  by receiver  {by_receiver}"));

    lines.push(format!(
        "external sites {}  silent-correct {}  leaked {}",
        r.oracle_external_sites, r.silent_correct, r.silent_leak
    ));
    // The per-tier `structural` column counts false positives only; this is
    // the whole-graph figure `--assert structural.impossible` judges, so the
    // text report carries it too.
    lines.push(format!(
        "structural  impossible {}  checked {}",
        r.structural_impossible, r.structural_checked
    ));
    lines.push(format!(
        "fan-out  1: {}  2: {}  3: {}  4+: {}",
        r.fanout[0], r.fanout[1], r.fanout[2], r.fanout[3]
    ));

    if !r.top_fp.is_empty() {
        let s = r
            .top_fp
            .iter()
            .map(|(n, c)| format!("{n} {c}"))
            .collect::<Vec<_>>()
            .join("  ");
        lines.push(format!("top fp targets   {s}"));
    }
    if !r.top_missed.is_empty() {
        let s = r
            .top_missed
            .iter()
            .map(|(id, c)| format!("{id} {c}"))
            .collect::<Vec<_>>()
            .join("  ");
        lines.push(format!("top missed       {s}"));
    }
    if !r.unknown_targets.is_empty() {
        let s = r
            .unknown_targets
            .iter()
            .map(|(k, c)| format!("{k} {c}"))
            .collect::<Vec<_>>()
            .join("  ");
        lines.push(format!("unknown targets  {s}"));
    }
    // These four are printed only when non-zero: the common case (a clean
    // run against a well-formed oracle) has all four at zero, and a text
    // report that always carried four "0" lines would bury the signal a
    // real run needs to see.
    if r.partial_file_mismatch > 0 {
        lines.push(format!("partial file mismatch {}", r.partial_file_mismatch));
    }
    if r.oracle_ambiguous > 0 {
        lines.push(format!("ambiguous {}", r.oracle_ambiguous));
    }
    if r.oracle_dropped > 0 {
        lines.push(format!("dropped (outside universe) {}", r.oracle_dropped));
    }
    if r.edges_outside_universe > 0 {
        lines.push(format!(
            "edges outside universe (not judged) {}",
            r.edges_outside_universe
        ));
    }

    lines.join("\n")
}

#[allow(
    clippy::too_many_lines,
    reason = "one ordered pass assembling every report field into the JSON shape render_text mirrors"
)]
pub fn render_json(r: &AuditReport) -> String {
    let mut tiers_fields: Vec<(&'static str, J)> = Vec::new();
    for tier in Tier::ORDER {
        if let Some((_, ts)) = r.tiers.iter().find(|(t, _)| *t == tier) {
            tiers_fields.push((
                tier.key(),
                J::Obj(vec![
                    ("edges", J::UInt(ts.edges as u64)),
                    ("tp", J::UInt(ts.tp as u64)),
                    ("fp", J::UInt(ts.fp as u64)),
                    ("precision", J::RawNum(format!("{:.3}", ts.precision()))),
                    ("fp_no_site", J::UInt(ts.fp_no_site as u64)),
                    ("fp_external_site", J::UInt(ts.fp_external_site as u64)),
                    ("fp_wrong_target", J::UInt(ts.fp_wrong_target as u64)),
                    ("structural", J::UInt(ts.structural as u64)),
                    ("unjudged", J::UInt(ts.unjudged as u64)),
                ]),
            ));
        }
    }

    let by_receiver_j: Vec<(&'static str, J)> = r
        .by_receiver
        .iter()
        .map(|(k, v)| {
            (
                *k,
                match v {
                    Some(x) => J::RawNum(format!("{x:.3}")),
                    None => J::RawNum("null".to_string()),
                },
            )
        })
        .collect();

    J::Obj(vec![
        ("status", J::Str("ok".to_string())),
        ("root", J::Str(r.root.clone())),
        ("lane", J::Str(r.lane.to_string())),
        (
            "oracle",
            J::Obj(vec![
                ("records", J::UInt(r.oracle_records as u64)),
                ("sites", J::UInt(r.oracle_sites as u64)),
                ("external_sites", J::UInt(r.oracle_external_sites as u64)),
                ("ambiguous", J::UInt(r.oracle_ambiguous as u64)),
                ("dropped", J::UInt(r.oracle_dropped as u64)),
            ]),
        ),
        (
            "units",
            J::Obj(vec![
                ("ok", J::UInt(r.units_ok as u64)),
                ("failed", J::UInt(r.units_failed as u64)),
                ("method", J::Str(r.structural_method.to_string())),
            ]),
        ),
        ("tiers", J::Obj(tiers_fields)),
        (
            "recall",
            J::Obj(vec![
                ("denominator", J::UInt(r.recall_denominator as u64)),
                ("precise", ratio_j(r.recall_precise, r.recall_denominator)),
                (
                    "precise_ext",
                    ratio_j(r.recall_precise_ext, r.recall_denominator),
                ),
                ("all", ratio_j(r.recall_all, r.recall_denominator)),
                ("by_receiver", J::Obj(by_receiver_j)),
                ("conditional", J::UInt(r.recall_conditional as u64)),
                ("bare", J::UInt(r.recall_bare as u64)),
            ]),
        ),
        (
            "silent",
            J::Obj(vec![
                ("correct", J::UInt(r.silent_correct as u64)),
                ("leak", J::UInt(r.silent_leak as u64)),
            ]),
        ),
        (
            "structural",
            J::Obj(vec![
                ("impossible", J::UInt(r.structural_impossible as u64)),
                ("checked", J::UInt(r.structural_checked as u64)),
                ("method", J::Str(r.structural_method.to_string())),
            ]),
        ),
        (
            "fanout",
            J::Obj(vec![
                ("1", J::UInt(r.fanout[0] as u64)),
                ("2", J::UInt(r.fanout[1] as u64)),
                ("3", J::UInt(r.fanout[2] as u64)),
                ("4+", J::UInt(r.fanout[3] as u64)),
            ]),
        ),
        (
            "unknown_targets",
            J::Arr(
                r.unknown_targets
                    .iter()
                    .map(|(k, c)| {
                        J::Obj(vec![
                            ("kind", J::Str(k.clone())),
                            ("count", J::UInt(*c as u64)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "top_fp",
            J::Arr(
                r.top_fp
                    .iter()
                    .map(|(n, c)| {
                        J::Obj(vec![
                            ("name", J::Str(n.clone())),
                            ("count", J::UInt(*c as u64)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "top_missed",
            J::Arr(
                r.top_missed
                    .iter()
                    .map(|(id, c)| {
                        J::Obj(vec![
                            ("id", J::Str(id.clone())),
                            ("count", J::UInt(*c as u64)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "partial_file_mismatch",
            J::UInt(r.partial_file_mismatch as u64),
        ),
        (
            "edges_outside_universe",
            J::UInt(r.edges_outside_universe as u64),
        ),
    ])
    .to_json_string()
}
