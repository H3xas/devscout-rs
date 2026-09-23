// Same-context authority: given what a same-context compiler fact confirmed
// for one reference and whatever the ladder itself already pushed for that
// same reference, decides whether the compiler's answer overrides it, and
// keeps the running counters an enriched-lane audit reads back (confirmed
// overrides, and how many of them actually displaced a different ladder
// target -- the disagreement diagnostic). Never touches `edges` itself; the
// caller (the resolver's own per-reference wrapper) owns truncation and the
// one replacement push, so this module stays a pure decision plus a counter.

use std::collections::HashSet;

use crate::graph::{
    Edge, EdgesByKind, FragRef, HeuristicByTier, HeuristicTier, SemanticProvenance,
};
use crate::resolve::DefIndex;

use super::discovered;
use super::layer::{LookupOutcome, SemanticLayer, SemanticTarget};
use crate::graph::SemanticStats;

/// A same-context compiler fact that overrides whatever the ladder already
/// produced for this reference. `displaced` names the ladder's own target
/// when it differs from `target` -- the disagreement this consumer must
/// preserve as a diagnostic rather than a graph edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    /// The compiler's own same-context answer.
    pub target: SemanticTarget,
    /// The ladder's own target, when it differs from `target`.
    pub displaced: Option<String>,
}

/// Decides confirm/override/fall-through for one reference. `ladder_pushed`
/// is the slice of edges the (untouched) ladder already pushed for this same
/// reference before this decision runs; `None` (fall-through) leaves them
/// exactly as they are.
pub fn decide(outcome: LookupOutcome, ladder_pushed: &[Edge]) -> Option<Override> {
    let LookupOutcome::Confirmed(target) = outcome else {
        return None;
    };
    let displaced = ladder_pushed.iter().find_map(|e| match e {
        Edge::UsesMember { to, .. } if *to != target.to_def_id => Some(to.clone()),
        _ => None,
    });
    Some(Override { target, displaced })
}

/// The running counters one `map` run accumulates across every reference:
/// how many same-context overrides fired, and how many of them displaced a
/// different ladder target. Folded into `graph::SemanticStats` by `finish`,
/// never into a graph edge.
#[derive(Debug, Clone, Copy, Default)]
pub struct SemanticDiagnostics {
    /// Same-context overrides that fired.
    pub confirmed: usize,
    /// How many of those displaced a different ladder target.
    pub disagreements: usize,
}

impl SemanticDiagnostics {
    /// Records one override: always a confirmation, and a disagreement too
    /// when it displaced a different ladder target.
    pub fn record(&mut self, ovr: &Override) {
        self.confirmed += 1;
        if ovr.displaced.is_some() {
            self.disagreements += 1;
        }
    }
}

/// Registers `r` at its own `(file, line, member)` key, when the layer is
/// loaded and `r` names a member -- the extractor's own attempt at this
/// site, regardless of whether it resolved, so `discovered.rs`'s later
/// projection can tell "the extractor tried and missed" apart from "the
/// extractor never looked here at all".
pub fn track_reference(
    layer: Option<&SemanticLayer>,
    sites: &mut HashSet<(String, usize, String)>,
    file: &str,
    r: &FragRef,
) {
    if layer.is_some() {
        if let Some(member) = &r.member {
            sites.insert((file.to_string(), r.line, member.clone()));
        }
    }
}

/// The resolver's own per-reference wrapper: after the untouched ladder has
/// run and pushed whatever it would have for `r` (the edges at
/// `edges[pre_len..]`), consults the layer and, on a same-context
/// confirmation, undoes exactly those pushes (edges and their own
/// `edges_by_kind`/heuristic bookkeeping) and replaces them with the one
/// compiler-vouched edge instead. A miss, or no layer at all, is a no-op: the
/// ladder's own edges stand exactly as pushed.
#[allow(
    clippy::too_many_arguments,
    reason = "each parameter is one of the resolver's own ledger fields, individually mutable, and the design forbids a post-pass over the finished edge vector that could fold them into one struct"
)]
pub fn apply(
    layer: Option<&SemanticLayer>,
    index: &DefIndex,
    edges: &mut Vec<Edge>,
    edges_by_kind: &mut EdgesByKind,
    heuristic_edge_count: &mut usize,
    heuristic_by_tier: &mut HeuristicByTier,
    diag: &mut SemanticDiagnostics,
    pre_len: usize,
    file: &str,
    r: &FragRef,
) {
    let (Some(layer), Some(member)) = (layer, r.member.as_deref()) else {
        return;
    };
    let outcome = layer.lookup(index, file, r.line, member, r.arg_count);
    let Some(ovr) = decide(outcome, &edges[pre_len..]) else {
        return;
    };
    diag.record(&ovr);
    // `edges_by_kind.uses_member` counts PRECISE uses-member edges only --
    // a heuristic (ext/guess) push bumps `heuristic_edge_count`/
    // `heuristic_by_tier` instead and never touches this counter at all
    // (`resolve_graph_with_model`'s own module comment: "heuristic edges are
    // counted here and nowhere else"). The truncated slice must therefore be
    // walked to find how many of its own edges were PRECISE (tier `None`)
    // before adjusting this counter -- subtracting the slice's raw length
    // unconditionally underflows whenever an override truncates one or more
    // heuristic edges, which a same-context fact confirming over an
    // ambiguous scored-tier guess does routinely.
    let mut precise_truncated = 0usize;
    for e in &edges[pre_len..] {
        match e {
            Edge::UsesMember {
                tier: Some(tier), ..
            } => {
                *heuristic_edge_count -= 1;
                match tier {
                    HeuristicTier::Ext => heuristic_by_tier.ext -= 1,
                    HeuristicTier::Guess => heuristic_by_tier.guess -= 1,
                }
            }
            Edge::UsesMember { tier: None, .. } => precise_truncated += 1,
            _ => {}
        }
    }
    edges_by_kind.uses_member -= precise_truncated;
    edges.truncate(pre_len);
    edges_by_kind.uses_member += 1;
    edges.push(Edge::uses_member_semantic(
        file.to_string(),
        r.line,
        ovr.target.to_def_id,
        ovr.target.to_file,
        Some(member.to_string()),
        SemanticProvenance::Semantic,
        ovr.target.overload_signature,
    ));
}

/// The resolver's own end-of-run wrapper: projects every compiler-discovered
/// site the main loop never referenced, then folds the run's own confirmed/
/// disagreement counters together with the discovered count into one
/// `SemanticStats`, or `None` when no layer loaded for this run at all.
pub fn finish(
    layer: Option<&SemanticLayer>,
    index: &DefIndex,
    sites: &HashSet<(String, usize, String)>,
    edges: &mut Vec<Edge>,
    diag: SemanticDiagnostics,
) -> Option<SemanticStats> {
    let layer = layer?;
    let discovered = discovered::project(layer, index, sites, edges);
    Some(SemanticStats {
        confirmed: diag.confirmed,
        disagreements: diag.disagreements,
        discovered,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::HeuristicTier;

    fn target(id: &str) -> SemanticTarget {
        SemanticTarget {
            to_def_id: id.to_string(),
            to_file: "Target.cs".to_string(),
            overload_signature: None,
        }
    }

    #[test]
    fn a_non_confirmed_outcome_never_overrides() {
        assert!(decide(LookupOutcome::NoFact, &[]).is_none());
    }

    #[test]
    fn confirming_the_same_target_the_ladder_already_bound_has_no_displaced_target() {
        let pushed = vec![Edge::uses_member(
            "Caller.cs".to_string(),
            8,
            "Ns.Widget".to_string(),
            "Widget.cs".to_string(),
            Some("Render".to_string()),
            None,
        )];
        let ovr = decide(LookupOutcome::Confirmed(target("Ns.Widget")), &pushed).unwrap();
        assert_eq!(ovr.displaced, None);
    }

    #[test]
    fn confirming_a_different_target_records_the_displaced_one() {
        let pushed = vec![Edge::uses_member(
            "Caller.cs".to_string(),
            8,
            "Ns.WrongGuess".to_string(),
            "WrongGuess.cs".to_string(),
            Some("Render".to_string()),
            Some(HeuristicTier::Guess),
        )];
        let ovr = decide(LookupOutcome::Confirmed(target("Ns.Widget")), &pushed).unwrap();
        assert_eq!(ovr.displaced.as_deref(), Some("Ns.WrongGuess"));
    }

    #[test]
    fn diagnostics_count_confirmations_and_disagreements_separately() {
        let mut diag = SemanticDiagnostics::default();
        diag.record(&Override {
            target: target("Ns.Widget"),
            displaced: None,
        });
        diag.record(&Override {
            target: target("Ns.Widget"),
            displaced: Some("Ns.WrongGuess".to_string()),
        });
        assert_eq!(diag.confirmed, 2);
        assert_eq!(diag.disagreements, 1);
    }
}
