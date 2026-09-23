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

use super::layer::{LookupOutcome, SemanticLayer};

/// Appends one enriched-lane edge for every admitted, same-context-confirmed
/// occurrence whose `(file, name_line, member)` key is absent from
/// `referenced_sites` -- the set of keys the main loop's own extractor-driven
/// references already covered, keyed the same way `SemanticLayer`'s join
/// index is. Returns how many edges were appended, for the caller's own
/// enriched-lane counters.
pub fn project(
    layer: &SemanticLayer,
    index: &DefIndex,
    referenced_sites: &HashSet<(String, usize, String)>,
    edges: &mut Vec<Edge>,
) -> usize {
    let mut discovered = 0;
    for (file, line, member) in layer.site_keys() {
        if referenced_sites.contains(&(file.clone(), *line, member.clone())) {
            continue;
        }
        // No `FragRef` exists at a discovered site -- the extractor never
        // produced one -- so there is no caller-side argument count to
        // disambiguate an overload with; `None` falls back to `lookup`'s
        // own ambiguous-if-unresolved rule, unchanged from before arity
        // narrowing existed.
        if let LookupOutcome::Confirmed(target) = layer.lookup(index, file, *line, member, None) {
            edges.push(Edge::uses_member_semantic(
                file.clone(),
                *line,
                target.to_def_id,
                target.to_file,
                Some(member.clone()),
                SemanticProvenance::SemanticDiscovered,
                target.overload_signature,
            ));
            discovered += 1;
        }
    }
    discovered
}
