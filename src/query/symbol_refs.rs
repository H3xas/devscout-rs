// The immediate inbound/outbound edges of one resolved symbol, read from a
// built [`GraphIndex`]: split out of `index.rs`, which holds the index itself,
// to keep that file under this crate's per-file line budget.

use super::index::{def_files, GraphIndex, InboundEntry};

/// The immediate inbound/outbound edges of one resolved symbol, holding
/// `graph.edges` indices rather than cloned edges (the same convention
/// [`GraphIndex`] uses).
#[derive(Debug, Clone, Default)]
pub struct SymbolRefs {
    /// The inbound inherits value.
    pub inbound_inherits: Vec<usize>,
    /// The inbound uses type value.
    pub inbound_uses_type: Vec<usize>,
    /// The inbound uses member value.
    pub inbound_uses_member: Vec<usize>,
    /// `implements` edges naming this symbol as their target -- never
    /// heuristic, so there is no `heuristic_inbound_implements` counterpart.
    pub inbound_implements: Vec<usize>,
    /// `overrides` edges naming this symbol as their target, same rule.
    pub inbound_overrides: Vec<usize>,
    /// The outbound inherits value.
    pub outbound_inherits: Vec<usize>,
    /// The outbound uses type value.
    pub outbound_uses_type: Vec<usize>,
    /// The outbound uses member value.
    pub outbound_uses_member: Vec<usize>,
    /// `implements` edges originating in one of this symbol's own declaring
    /// files.
    pub outbound_implements: Vec<usize>,
    /// `overrides` edges originating in one of this symbol's own declaring
    /// files.
    pub outbound_overrides: Vec<usize>,
    /// The outbound imports value.
    pub outbound_imports: Vec<usize>,
    /// The same two tables again over the HEURISTIC adjacency, built by the
    /// identical rules (enum members union into the enum's inbound, partial
    /// classes union outbound over every declaring file) so a heuristic row is
    /// never present or absent for a reason a precise row would not have been.
    pub heuristic_inbound_inherits: Vec<usize>,
    /// The heuristic inbound uses type value.
    pub heuristic_inbound_uses_type: Vec<usize>,
    /// The heuristic inbound uses member value.
    pub heuristic_inbound_uses_member: Vec<usize>,
    /// The heuristic outbound inherits value.
    pub heuristic_outbound_inherits: Vec<usize>,
    /// The heuristic outbound uses type value.
    pub heuristic_outbound_uses_type: Vec<usize>,
    /// The heuristic outbound uses member value.
    pub heuristic_outbound_uses_member: Vec<usize>,
    /// The ambiguous inbound value.
    pub ambiguous_inbound: Vec<usize>,
    /// The ambiguous outbound value.
    pub ambiguous_outbound: Vec<usize>,
}

/// Immediate (1-hop) inbound/outbound for one resolved symbol. Partial-class
/// defs union outbound over every declaring file. An enum query unions every
/// member's inbound `uses-member` edges in, iterating `index.graph.defs`
/// directly (its array order) rather than the unordered `by_id` map.
pub fn symbol_refs(index: &GraphIndex, def_id: &str) -> SymbolRefs {
    let empty = InboundEntry::default();
    let base = index.inbound.get(def_id).unwrap_or(&empty);
    let inbound_inherits = base.inherits.clone();
    let inbound_uses_type = base.uses_type.clone();
    let mut inbound_uses_member = base.uses_member.clone();
    // Neither kind ever fires on an enum (C# forbids an enum implementing an
    // interface or overriding anything), so unlike `inbound_uses_member`
    // below, neither needs the enum-member union.
    let inbound_implements = base.implements.clone();
    let inbound_overrides = base.overrides.clone();

    if let Some(&i) = index.by_id.get(def_id) {
        if index.graph.defs[i].kind == "enum" {
            let prefix = format!("{def_id}.");
            for d in &index.graph.defs {
                if d.kind != "enum-member" || !d.id.starts_with(&prefix) {
                    continue;
                }
                if let Some(m) = index.inbound.get(&d.id) {
                    inbound_uses_member.extend(m.uses_member.iter().copied());
                }
            }
        }
    }

    let mut outbound_inherits = Vec::new();
    let mut outbound_uses_type = Vec::new();
    let mut outbound_uses_member = Vec::new();
    let mut outbound_implements = Vec::new();
    let mut outbound_overrides = Vec::new();
    let mut outbound_imports = Vec::new();
    for file in def_files(index, def_id) {
        if let Some(o) = index.outbound_by_file.get(&file) {
            outbound_inherits.extend(o.inherits.iter().copied());
            outbound_uses_type.extend(o.uses_type.iter().copied());
            outbound_uses_member.extend(o.uses_member.iter().copied());
            outbound_implements.extend(o.implements.iter().copied());
            outbound_overrides.extend(o.overrides.iter().copied());
            outbound_imports.extend(o.imports.iter().copied());
        }
    }

    // The heuristic halves, same rules, separate adjacency.
    let heuristic_base = index.heuristic_inbound.get(def_id).unwrap_or(&empty);
    let heuristic_inbound_inherits = heuristic_base.inherits.clone();
    let heuristic_inbound_uses_type = heuristic_base.uses_type.clone();
    let mut heuristic_inbound_uses_member = heuristic_base.uses_member.clone();

    if let Some(&i) = index.by_id.get(def_id) {
        if index.graph.defs[i].kind == "enum" {
            let prefix = format!("{def_id}.");
            for d in &index.graph.defs {
                if d.kind != "enum-member" || !d.id.starts_with(&prefix) {
                    continue;
                }
                if let Some(m) = index.heuristic_inbound.get(&d.id) {
                    heuristic_inbound_uses_member.extend(m.uses_member.iter().copied());
                }
            }
        }
    }

    let mut heuristic_outbound_inherits = Vec::new();
    let mut heuristic_outbound_uses_type = Vec::new();
    let mut heuristic_outbound_uses_member = Vec::new();
    for file in def_files(index, def_id) {
        if let Some(o) = index.heuristic_outbound_by_file.get(&file) {
            heuristic_outbound_inherits.extend(o.inherits.iter().copied());
            heuristic_outbound_uses_type.extend(o.uses_type.iter().copied());
            heuristic_outbound_uses_member.extend(o.uses_member.iter().copied());
        }
    }

    let ambiguous_inbound = index
        .ambiguous_by_candidate
        .get(def_id)
        .cloned()
        .unwrap_or_default();
    let ambiguous_outbound: Vec<usize> = def_files(index, def_id)
        .iter()
        .flat_map(|f| index.ambiguous_by_file.get(f).cloned().unwrap_or_default())
        .collect();

    SymbolRefs {
        inbound_inherits,
        inbound_uses_type,
        inbound_uses_member,
        inbound_implements,
        inbound_overrides,
        outbound_inherits,
        outbound_uses_type,
        outbound_uses_member,
        outbound_implements,
        outbound_overrides,
        outbound_imports,
        heuristic_inbound_inherits,
        heuristic_inbound_uses_type,
        heuristic_inbound_uses_member,
        heuristic_outbound_inherits,
        heuristic_outbound_uses_type,
        heuristic_outbound_uses_member,
        ambiguous_inbound,
        ambiguous_outbound,
    }
}
