// What one TS file's import and re-export statements bind: the resolved
// re-export table every lookup walks, the barrel-following export lookup, and
// the `import` edges a file's statements emit. Split out of `tsgraph.rs` to keep
// that file under its size ceiling; the reference resolution that consumes the
// binding map stays there.

use std::collections::{HashMap, HashSet};

use crate::extract::TsFragment;
use crate::graph::{Def, Edge};

use super::declared::{Resolution, Resolver, Usage};
use super::disclosure::Reason;
use super::{UnresolvedImport, BARREL_HOPS};

pub(super) struct ResolvedReexport {
    spec: String,
    line: usize,
    star: bool,
    names: Vec<(String, String)>,
    namespace: Option<String>,
    to: Option<String>,
    reason: Option<Reason>,
}

/// Every file's own exports (name -> def index) and its resolved re-export
/// statements: the two tables an export lookup walks.
pub(super) struct Exports<'a> {
    pub(super) by_file: HashMap<&'a str, HashMap<String, usize>>,
    pub(super) reexports_by_file: HashMap<&'a str, Vec<ResolvedReexport>>,
}

/// Resolve every file's re-export statements once, up front, so a lookup walks
/// file paths rather than re-resolving specifiers at every hop.
pub(super) fn resolve_reexports<'a>(
    fragments: &'a [(String, TsFragment)],
    resolver: &Resolver<'_>,
) -> HashMap<&'a str, Vec<ResolvedReexport>> {
    let mut reexports_by_file: HashMap<&str, Vec<ResolvedReexport>> = HashMap::new();
    for (file, frag) in fragments {
        let rows = frag
            .reexports
            .iter()
            .map(|rx| {
                let (to, reason) =
                    match resolver.resolve(file, &rx.spec, resolver.static_usage(file)) {
                        Resolution::Resolved(to) => (Some(to), None),
                        Resolution::Unresolved(reason) => (None, Some(reason)),
                        Resolution::LegacyExternal => (None, None),
                    };
                ResolvedReexport {
                    spec: rx.spec.clone(),
                    line: rx.line,
                    star: rx.star,
                    names: rx
                        .names
                        .iter()
                        .map(|n| (n.exported.clone(), n.imported.clone()))
                        .collect(),
                    namespace: rx.namespace.clone(),
                    to,
                    reason,
                }
            })
            .collect();
        reexports_by_file.insert(file.as_str(), rows);
    }
    reexports_by_file
}

impl Exports<'_> {
    /// The def `name` names when imported from `file`, following re-export
    /// barrels. Each call is one top-level lookup with its own visited map.
    pub(super) fn lookup(&self, file: &str, name: &str) -> Option<usize> {
        let mut visited: HashMap<(String, String), usize> = HashMap::new();
        lookup_export(file, name, 0, self, &mut visited)
    }

    pub(super) fn namespace_target(&self, file: &str, name: &str) -> Option<String> {
        let mut visited = HashSet::new();
        self.namespace_target_in(file, name, 0, &mut visited)
    }

    fn namespace_target_in(
        &self,
        file: &str,
        name: &str,
        hops: usize,
        visited: &mut HashSet<String>,
    ) -> Option<String> {
        if hops >= BARREL_HOPS || !visited.insert(file.to_string()) {
            return None;
        }
        if self
            .by_file
            .get(file)
            .is_some_and(|own| own.contains_key(name))
        {
            return None;
        }
        let rows = self.reexports_by_file.get(file)?;
        for row in rows {
            if row.namespace.as_deref() == Some(name) {
                return row.to.clone();
            }
        }
        for row in rows {
            if let Some((_, imported)) = row.names.iter().find(|(exported, _)| exported == name) {
                return row
                    .to
                    .as_ref()
                    .and_then(|to| self.namespace_target_in(to, imported, hops + 1, visited));
            }
        }
        for row in rows {
            if row.star {
                if let Some(to) = row
                    .to
                    .as_ref()
                    .and_then(|to| self.namespace_target_in(to, name, hops + 1, visited))
                {
                    return Some(to);
                }
            }
        }
        None
    }
}

// `visited` is one top-level lookup's own map, fresh at every call site
// (never shared across bindings or refs): a `(file, name)` pair revisited
// mid-walk means a cyclic `export *` pair sent the walk back to a state it is
// already computing, so it fails closed to `None` right there rather than
// recursing again. That guard also keeps a single `export *` fan-out linear in
// files times names. What the map REMEMBERS is the shallowest hop count the
// pair was walked from: a pair first met deep in the walk, where the cap cut
// its own re-exports short, is walked again when a shorter path reaches it
// with more hops to spend -- and never again from the same depth or deeper.
fn lookup_export(
    file: &str,
    name: &str,
    hops: usize,
    exports: &Exports,
    visited: &mut HashMap<(String, String), usize>,
) -> Option<usize> {
    let key = (file.to_string(), name.to_string());
    if visited.get(&key).is_some_and(|&seen| seen <= hops) {
        return None;
    }
    visited.insert(key, hops);
    // Own declarations are checked before the hop cap is enforced, so a
    // chain of exactly `BARREL_HOPS` barrels followed by a declaring file
    // still resolves -- the cap bounds how many re-export hops are
    // FOLLOWED, not how many files may be visited in total.
    if let Some(own) = exports.by_file.get(file).and_then(|m| m.get(name)) {
        return Some(*own);
    }
    if hops >= BARREL_HOPS {
        return None;
    }
    if exports.reexports_by_file.get(file).is_some_and(|rows| {
        rows.iter()
            .any(|row| row.namespace.as_deref() == Some(name))
    }) {
        return None;
    }
    for rx in exports
        .reexports_by_file
        .get(file)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        let Some(to) = &rx.to else { continue };
        if rx.star {
            if let Some(hit) = lookup_export(to, name, hops + 1, exports, visited) {
                return Some(hit);
            }
            continue;
        }
        // Declaration order is the tie-break: the first re-export naming
        // this export wins, matching the first-occurrence rule the def
        // map itself uses.
        for (exported, imported) in &rx.names {
            if exported != name {
                continue;
            }
            if let Some(hit) = lookup_export(to, imported, hops + 1, exports, visited) {
                return Some(hit);
            }
        }
    }
    None
}

/// What one file's import and re-export statements produce: its `import`
/// edges in statement order, how many specifiers stayed external, and the
/// local-name binding map its references resolve through.
pub(super) struct FileImports {
    pub(super) edges: Vec<Edge>,
    pub(super) external: usize,
    pub(super) bindings: HashMap<String, (String, String)>,
    pub(super) unresolved: Vec<UnresolvedImport>,
}

/// Emit `file`'s `import` edges: one per resolved import statement, one more
/// per distinct declaring file a barrel import reaches (`via`), and one per
/// resolved re-export statement.
pub(super) fn file_imports(
    file: &str,
    frag: &TsFragment,
    resolver: &Resolver<'_>,
    defs: &[Def],
    exports: &Exports,
) -> FileImports {
    let mut out = FileImports {
        edges: Vec::new(),
        external: 0,
        bindings: HashMap::new(),
        unresolved: Vec::new(),
    };
    for imp in &frag.imports {
        let usage = if imp.require {
            Usage::Require
        } else {
            resolver.static_usage(file)
        };
        let to = match resolver.resolve(file, &imp.spec, usage) {
            Resolution::Resolved(to) => to,
            Resolution::Unresolved(reason) => {
                out.external += 1;
                out.unresolved
                    .push(unresolved(file, imp.line, &imp.spec, reason));
                continue;
            }
            Resolution::LegacyExternal => {
                out.external += 1;
                continue;
            }
        };
        out.edges.push(Edge::Import {
            from_file: file.to_string(),
            from_line: imp.line,
            target: imp.spec.clone(),
            to_file: to.clone(),
            via: None,
            bound_defs: Vec::new(),
        });
        // First binding of a local name wins, the same first-occurrence
        // rule the def map uses: a file that binds one name twice is
        // already broken TypeScript, and picking the later one would make
        // the answer depend on statement order for no gain.
        for b in &imp.bindings {
            out.bindings
                .entry(b.local.clone())
                .or_insert((to.clone(), b.imported.clone()));
        }
        // A barrel is a routing table, not a dependency: `import
        // { useThing } from '../hooks'` names index.ts but DEPENDS on the
        // module index.ts re-exports it from. Without this edge, `impact`
        // on the declaring file misses every consumer that came through
        // the barrel. The edge to the named file stays (it is what the
        // source literally says); this one carries `via` so a reader can
        // tell the two apart, and one line contributes at most one edge per
        // distinct declaring file however many names it pulls through.
        // Each bound definition rides the edge whose `to_file` declares it,
        // so a name the barrel only routes never lands on the barrel's edge.
        let statement = out.edges.len() - 1;
        let mut via_edge: HashMap<String, usize> = HashMap::new();
        for b in &imp.bindings {
            if b.imported == "*" {
                if let Some(target) = exports.lookup(&to, "export=") {
                    bind(&mut out.edges[statement], &defs[target].id);
                }
                continue;
            }
            let Some(target) = exports.lookup(&to, &b.imported) else {
                continue;
            };
            let target_file = defs[target].file.clone();
            let slot = if target_file == to {
                statement
            } else if let Some(&slot) = via_edge.get(&target_file) {
                slot
            } else {
                via_edge.insert(target_file.clone(), out.edges.len());
                out.edges.push(Edge::Import {
                    from_file: file.to_string(),
                    from_line: imp.line,
                    target: imp.spec.clone(),
                    to_file: target_file,
                    via: Some(to.clone()),
                    bound_defs: Vec::new(),
                });
                out.edges.len() - 1
            };
            bind(&mut out.edges[slot], &defs[target].id);
        }
    }
    append_dynamic_imports(file, frag, resolver, &mut out);
    append_reexports(file, exports, defs, &mut out);
    out
}

fn append_dynamic_imports(
    file: &str,
    frag: &TsFragment,
    resolver: &Resolver<'_>,
    out: &mut FileImports,
) {
    for imp in &frag.dynamic_imports {
        match resolver.resolve(file, &imp.spec, Usage::Import) {
            Resolution::Resolved(to) => out.edges.push(Edge::Import {
                from_file: file.to_string(),
                from_line: imp.line,
                target: imp.spec.clone(),
                to_file: to,
                via: None,
                bound_defs: Vec::new(),
            }),
            Resolution::Unresolved(reason) => {
                out.external += 1;
                out.unresolved
                    .push(unresolved(file, imp.line, &imp.spec, reason));
            }
            Resolution::LegacyExternal => out.external += 1,
        }
    }
}

fn append_reexports(file: &str, exports: &Exports<'_>, defs: &[Def], out: &mut FileImports) {
    for rx in exports
        .reexports_by_file
        .get(file)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        match &rx.to {
            None => {
                out.external += 1;
                if let Some(reason) = rx.reason {
                    out.unresolved
                        .push(unresolved(file, rx.line, &rx.spec, reason));
                }
            }
            Some(to) => {
                // A named re-export names the symbol it hands on, so it binds
                // like an import -- but only a name `to` itself declares: one
                // `to` merely re-exports again stays unbound rather than
                // adding an edge the statement never wrote.
                let mut edge = Edge::Import {
                    from_file: file.to_string(),
                    from_line: rx.line,
                    target: rx.spec.clone(),
                    to_file: to.clone(),
                    via: None,
                    bound_defs: Vec::new(),
                };
                if !rx.star {
                    for (_, imported) in &rx.names {
                        if let Some(target) = exports.lookup(to, imported) {
                            if defs[target].file == *to {
                                bind(&mut edge, &defs[target].id);
                            }
                        }
                    }
                }
                out.edges.push(edge);
            }
        }
    }
}

fn unresolved(file: &str, line: usize, spec: &str, reason: Reason) -> UnresolvedImport {
    UnresolvedImport {
        from_file: file.to_string(),
        from_line: line,
        target: spec.to_string(),
        reason: reason.as_str().to_string(),
    }
}

// Append `id` to an import edge's bound definitions unless it is already
// there: two bindings of one export (`import { A, A as B }`) bind it once.
fn bind(edge: &mut Edge, id: &str) {
    if let Edge::Import { bound_defs, .. } = edge {
        if !bound_defs.iter().any(|d| d == id) {
            bound_defs.push(id.to_string());
        }
    }
}

#[cfg(test)]
mod tests;
