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

/// Resolve an exact id or run the prior bare-name ladder over non-local defs.
///
/// A step with multiple non-local candidates includes its local candidates in
/// the ambiguity; callers try member and local fallbacks after a miss.
pub fn resolve_symbol(index: &GraphIndex, query: &str) -> Resolution {
    if index.by_id.contains_key(query) {
        return Resolution::Resolved(query.to_string());
    }
    if let Some(exact) = index.by_simple_name.get(query) {
        if let Some(result) = non_local_step(index, exact) {
            return result;
        }
    }
    let lower = query.to_lowercase();
    if let Some(ci) = index.by_lower_name.get(&lower) {
        if let Some(result) = non_local_step(index, ci) {
            return result;
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
        let tail: Vec<usize> = index
            .graph
            .defs
            .iter()
            .enumerate()
            .filter(|(_, d)| d.id.ends_with(&suffix))
            .map(|(i, _)| i)
            .collect();
        if let Some(result) = non_local_step(index, &tail) {
            return result;
        }
    }
    // A `::` path of a Rust item: `Type::item` or `module::Type::item`, a
    // tail of the path after the id's last `#`, matched with or without the
    // `-N` suffix a repeated path carries. Last on purpose: no C# or TS id
    // contains `::`, so no query that resolved before resolves differently.
    if query.contains("::") {
        let tail = format!("::{query}");
        let matches: Vec<usize> = index
            .graph
            .defs
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                d.id.rfind('#').is_some_and(|hash| {
                    let path = &d.id[hash + 1..];
                    [path, strip_repeat_suffix(path)]
                        .iter()
                        .any(|p| *p == query || p.ends_with(&tail))
                })
            })
            .map(|(i, _)| i)
            .collect();
        if let Some(result) = non_local_step(index, &matches) {
            return result;
        }
    }
    Resolution::NotFound
}

fn non_local_step(index: &GraphIndex, candidates: &[usize]) -> Option<Resolution> {
    let count = candidates
        .iter()
        .filter(|&&i| !index.graph.defs[i].local)
        .count();
    match count {
        0 => None,
        1 => Some(Resolution::Resolved(
            index.graph.defs[*candidates.iter().find(|&&i| !index.graph.defs[i].local)?]
                .id
                .clone(),
        )),
        _ => {
            let mut ids: Vec<String> = candidates
                .iter()
                .map(|&i| index.graph.defs[i].id.clone())
                .collect();
            ids.sort();
            Some(Resolution::Ambiguous(ids))
        }
    }
}

/// Resolve a file-local declaration after graph and member lookup miss.
pub fn resolve_local_symbol(index: &GraphIndex, query: &str) -> Resolution {
    if let Some(exact) = index.by_simple_name.get(query) {
        if let Some(result) = local_step(index, exact) {
            return result;
        }
    }
    if let Some(ci) = index.by_lower_name.get(&query.to_lowercase()) {
        if let Some(result) = local_step(index, ci) {
            return result;
        }
    }
    if query.contains('.') {
        let suffix = format!(".{query}");
        let matches: Vec<usize> = index
            .graph
            .defs
            .iter()
            .enumerate()
            .filter(|(_, d)| d.local && d.id.ends_with(&suffix))
            .map(|(i, _)| i)
            .collect();
        if let Some(result) = local_step(index, &matches) {
            return result;
        }
    }
    Resolution::NotFound
}

fn local_step(index: &GraphIndex, candidates: &[usize]) -> Option<Resolution> {
    let mut matches: Vec<String> = candidates
        .iter()
        .filter_map(|&i| {
            let d = &index.graph.defs[i];
            d.local.then(|| d.id.clone())
        })
        .collect();
    matches.sort();
    match matches.len() {
        0 => None,
        1 => Some(Resolution::Resolved(matches[0].clone())),
        _ => Some(Resolution::Ambiguous(matches)),
    }
}

// `Gadget::fmt-1` without its `-1`. A path segment is an identifier, which
// never holds a `-`, so a trailing `-<digits>` can only be a repeat suffix.
fn strip_repeat_suffix(path: &str) -> &str {
    match path.rsplit_once('-') {
        Some((head, digits))
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) =>
        {
            head
        }
        _ => path,
    }
}
