// Compiler-discovered-site projection: after the main per-reference loop has
// walked every reference the extractor actually emitted, this runs once per
// admitted artifact (not once per reference) and adds exactly the admitted,
// same-context-confirmed sites the extractor's own walk never produced a
// reference for at all. A genuinely new population, never pooled with the
// per-reference overrides `precedence.rs` decides: the edges this projects
// carry `SemanticProvenance::SemanticDiscovered`, never
// `SemanticProvenance::Semantic`, so a reader can always tell the two apart,
// and the syntax-lane recall denominator (which counts only
// extractor-emitted references) never moves because of anything this module
// adds.

use std::collections::HashSet;

use crate::graph::{Edge, SemanticProvenance};
use crate::resolve::DefIndex;

use super::layer::{LookupOutcome, SemanticLayer, SemanticTarget};

/// Appends one enriched-lane edge for every admitted, same-context-confirmed
/// occurrence whose `(file, name_line, member)` key is absent from
/// `referenced_sites` -- the set of keys the main loop's own extractor-driven
/// references already covered, keyed the same way `SemanticLayer`'s join
/// index is. Returns how many edges were appended, for the caller's own
/// enriched-lane counters.
///
/// Iterates `layer.site_keys()` in a fixed, sorted order -- `(file, line,
/// member)` ordinal order, never the underlying `HashMap`'s own iteration
/// order -- so the edges this appends, and therefore `graph.json` itself,
/// come out in the same order on every run over the same admitted artifact.
/// An extractor-emitted reference never has this problem: the main loop
/// that produces it already walks the syntax tree in one fixed order.
pub fn project(
    layer: &SemanticLayer,
    index: &DefIndex,
    referenced_sites: &HashSet<(String, usize, String)>,
    edges: &mut Vec<Edge>,
) -> usize {
    let mut site_keys: Vec<&(String, usize, String)> = layer.site_keys().collect();
    site_keys.sort();

    let mut discovered = 0;
    for (file, line, member) in site_keys {
        if referenced_sites.contains(&(file.clone(), *line, member.clone())) {
            continue;
        }
        // No `FragRef` exists at a discovered site -- the extractor never
        // produced one -- so there is no caller-side argument count to
        // disambiguate an overload with; `None` falls back to `lookup`'s
        // own no-caller-arity rule.
        match layer.lookup(index, file, *line, member, None) {
            LookupOutcome::Confirmed(target) => {
                push_discovered(edges, file, *line, member, &target);
                discovered += 1;
            }
            // More than one confirmed target survived with no caller-side
            // arity to narrow between them. When every survivor names the
            // SAME declaring def (two same-line overloads of one member on
            // the same type, e.g. `cond ? Create(x) : Create(x, y)`), this
            // site carries two genuinely distinct facts, not one ambiguous
            // one -- project each with its own signature, the same as
            // arity narrowing already lets an extractor-emitted reference
            // do. A survivor set naming more than one declaring def is a
            // real cross-type disagreement this consumer cannot resolve on
            // its own and is left unprojected, same as before.
            LookupOutcome::ConfirmedMany(targets)
                if targets.iter().all(|t| {
                    (&t.to_def_id, &t.to_file) == (&targets[0].to_def_id, &targets[0].to_file)
                }) =>
            {
                for target in &targets {
                    push_discovered(edges, file, *line, member, target);
                    discovered += 1;
                }
            }
            LookupOutcome::ConfirmedMany(_) | LookupOutcome::Other(_) | LookupOutcome::NoFact => {}
        }
    }
    discovered
}

fn push_discovered(
    edges: &mut Vec<Edge>,
    file: &str,
    line: usize,
    member: &str,
    target: &SemanticTarget,
) {
    edges.push(Edge::uses_member_semantic(
        file.to_string(),
        line,
        target.to_def_id.clone(),
        target.to_file.clone(),
        Some(member.to_string()),
        SemanticProvenance::SemanticDiscovered,
        target.overload_signature.clone(),
    ));
}
