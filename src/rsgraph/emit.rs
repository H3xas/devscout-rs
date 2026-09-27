// Rust edges from bound reference sites.
//
// A call's last segment bound to a function gives `uses-member` to that
// function's own def, from the line of the segment that names it. A segment
// bound to a struct, enum, union, trait or type alias gives `uses-type`
// when it is a qualifier, a type position, a struct literal name, or a
// tuple or unit struct used as a value. `impl Trait for T` gives a
// type-level `implements` to a recorded trait. `Self` and a pattern's last
// segment give nothing. Two or more candidates give one `ambiguous` edge,
// whose origin is the kind the lowest-id edge-giving candidate would have
// had, and end the site. Edges follow file order, then site order, then
// segment order.

use super::bind::{Binder, From, Step};
use super::scope::{Ns, Target};
use crate::extract::{RsFragment, RsRole, RsSite};
use crate::graph::{Candidate, Edge, EdgesByKind};

/// What one bound segment gives.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    UsesMember,
    UsesType,
    Implements,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::UsesMember => "uses-member",
            Kind::UsesType => "uses-type",
            Kind::Implements => "implements",
        }
    }
}

/// Every Rust edge, in order, with the counts by kind.
pub(super) fn edges(
    binder: &Binder,
    fragments: &[(String, RsFragment)],
) -> (Vec<Edge>, EdgesByKind) {
    let mut out = Vec::new();
    let mut counts = EdgesByKind::default();
    for (file, (rel, fragment)) in fragments.iter().enumerate() {
        for site in &fragment.sites {
            site_edges(binder, file, rel, site, &mut out, &mut counts);
        }
    }
    (out, counts)
}

fn last_ns(role: RsRole) -> Ns {
    match role {
        RsRole::Call | RsRole::Value => Ns::Value,
        RsRole::Type | RsRole::Lit | RsRole::Impl | RsRole::Pat => Ns::Type,
    }
}

fn self_def(binder: &Binder, file: usize, site: &RsSite) -> Option<usize> {
    if let Some(k) = site.self_impl {
        return binder.t.impl_self.get(&(file, k)).copied();
    }
    let path = site.self_trait.as_ref()?;
    binder.t.def_by_path.get(&(file, path.clone())).copied()
}

fn site_edges(
    binder: &Binder,
    file: usize,
    rel: &str,
    site: &RsSite,
    out: &mut Vec<Edge>,
    counts: &mut EdgesByKind,
) {
    let Some(module) = binder.tree.node_of(file, &site.scope) else {
        return;
    };
    let from = From {
        module,
        file,
        block: site.block,
        self_def: self_def(binder, file, site),
    };
    let segs: Vec<&str> = site.path.split("::").collect();
    let lines = site.segment_lines();
    let steps = binder.walk(&from, &segs, last_ns(site.role), &mut Vec::new());
    for (i, step) in steps.iter().enumerate() {
        let last = i + 1 == segs.len();
        let line = lines.get(i).copied().unwrap_or(site.line);
        match step {
            Step::Bound(Target::Def(d)) if segs[i] != "Self" => {
                if let Some(kind) = kind_of(binder, *d, site.role, last) {
                    out.push(edge(binder, kind, rel, line, *d));
                    count(counts, kind);
                }
            }
            Step::Ambiguous(targets) => {
                if let Some(e) = ambiguous(binder, targets, site.role, last, rel, line, &segs[..=i])
                {
                    out.push(e);
                }
            }
            _ => {}
        }
    }
}

// The kind a segment bound to `def` gives, by its position and the role.
fn kind_of(binder: &Binder, def: usize, role: RsRole, last: bool) -> Option<Kind> {
    let d = &binder.t.defs[def];
    if !last {
        return d.is_type().then_some(Kind::UsesType);
    }
    match role {
        RsRole::Call if d.kind == "fn" => Some(Kind::UsesMember),
        RsRole::Call | RsRole::Value => (d.kind == "struct").then_some(Kind::UsesType),
        RsRole::Type | RsRole::Lit => d.is_type().then_some(Kind::UsesType),
        RsRole::Impl => (d.kind == "trait").then_some(Kind::Implements),
        RsRole::Pat => None,
    }
}

fn edge(binder: &Binder, kind: Kind, rel: &str, line: usize, def: usize) -> Edge {
    let d = &binder.t.defs[def];
    let to_file = d.rel.clone();
    match kind {
        Kind::UsesMember => Edge::uses_member(
            rel.to_string(),
            line,
            d.id.clone(),
            to_file,
            Some(d.name.clone()),
            None,
        ),
        Kind::UsesType => Edge::UsesType {
            from_file: rel.to_string(),
            from_line: line,
            to: d.id.clone(),
            to_file,
            heuristic: false,
        },
        Kind::Implements => Edge::Implements {
            from_file: rel.to_string(),
            from_line: line,
            to: d.id.clone(),
            to_file,
            member: None,
        },
    }
}

fn count(counts: &mut EdgesByKind, kind: Kind) {
    match kind {
        Kind::UsesMember => counts.uses_member += 1,
        Kind::UsesType => counts.uses_type += 1,
        Kind::Implements => counts.implements += 1,
    }
}

// One `ambiguous` edge for a segment with two or more candidates, or `None`
// when no candidate would have given an edge.
fn ambiguous(
    binder: &Binder,
    targets: &[Target],
    role: RsRole,
    last: bool,
    rel: &str,
    line: usize,
    written: &[&str],
) -> Option<Edge> {
    let mut defs: Vec<usize> = targets
        .iter()
        .filter_map(|t| match t {
            Target::Def(d) => Some(*d),
            Target::Module(_) => None,
        })
        .collect();
    defs.sort_by(|a, b| binder.t.defs[*a].id.cmp(&binder.t.defs[*b].id));
    defs.dedup_by(|a, b| binder.t.defs[*a].id == binder.t.defs[*b].id);
    let origin = defs.iter().find_map(|&d| kind_of(binder, d, role, last))?;
    let candidates: Vec<Candidate> = defs
        .iter()
        .take(crate::resolve::AMBIGUOUS_CAP)
        .map(|&d| Candidate {
            id: binder.t.defs[d].id.clone(),
            file: binder.t.defs[d].rel.clone(),
        })
        .collect();
    Some(Edge::Ambiguous {
        origin: origin.label().to_string(),
        from_file: rel.to_string(),
        from_line: line,
        raw: written.join("::"),
        candidates,
        candidate_count: defs.len(),
    })
}
