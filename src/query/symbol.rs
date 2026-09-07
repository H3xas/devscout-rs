use super::index::GraphIndex;

// ============================================================================
// resolve_symbol.
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
/// Represents `Resolution`.
pub enum Resolution {
    /// Represents `Resolved`.
    Resolved(String),
    /// Represents `Ambiguous`.
    Ambiguous(Vec<String>),
    /// Represents `NotFound`.
    NotFound,
}

/// "Never guess" resolution ladder: exact id, then unique exact name, then
/// unique case-insensitive name; two-or-more candidates at any step is reported
/// as ambiguous, not resolved further.
pub fn resolve_symbol(index: &GraphIndex, query: &str) -> Resolution {
    if index.by_id.contains_key(query) {
        return Resolution::Resolved(query.to_string());
    }
    if let Some(exact) = index.by_simple_name.get(query) {
        if exact.len() == 1 {
            return Resolution::Resolved(index.graph.defs[exact[0]].id.clone());
        }
        if exact.len() > 1 {
            return Resolution::Ambiguous(
                exact
                    .iter()
                    .map(|&i| index.graph.defs[i].id.clone())
                    .collect(),
            );
        }
    }
    let lower = query.to_lowercase();
    if let Some(ci) = index.by_lower_name.get(&lower) {
        if ci.len() == 1 {
            return Resolution::Resolved(index.graph.defs[ci[0]].id.clone());
        }
        if ci.len() > 1 {
            return Resolution::Ambiguous(
                ci.iter().map(|&i| index.graph.defs[i].id.clone()).collect(),
            );
        }
    }
    // A TAIL of a def id, which is how a caller spells an enum member:
    // `Toggles.EnableX`, never the namespace-qualified
    // `App.Config.Toggles.EnableX` the graph keys it under. Reached only after
    // every step above has missed, so no query that resolved before still
    // resolves differently. Restricted to a dotted query on purpose -- every
    // def is indexed under its simple name, so `by_simple_name` above is
    // already exhaustive for an undotted one and this step could only repeat
    // it. Two or more ids ending in the same tail is an ambiguity, reported as
    // one rather than guessed at. Iterates `graph.defs` directly (its array
    // order) rather than the unordered `by_id` map.
    if query.contains('.') {
        let suffix = format!(".{query}");
        let tail: Vec<String> = index
            .graph
            .defs
            .iter()
            .filter(|d| d.id.ends_with(&suffix))
            .map(|d| d.id.clone())
            .collect();
        if tail.len() == 1 {
            return Resolution::Resolved(tail.into_iter().next().expect("one tail match"));
        }
        if tail.len() > 1 {
            return Resolution::Ambiguous(tail);
        }
    }
    Resolution::NotFound
}
