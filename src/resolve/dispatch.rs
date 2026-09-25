// Dispatch edges: the type-level `implements` fact a two-type-argument DI
// registration records, plus the member-level `implements`/`overrides` facts
// derived for exactly the implementation types a registration names -- see
// this crate's architecture guide for the invariants this module owns.
//
// Both passes are pure, run once per `resolve_graph`, and touch no file this
// crate has not already parsed: everything here reads `DefIndex`/
// `FileContext` the same way the rest of `resolve/` does.

use super::edges::type_probe;
use super::index::DefIndex;
use super::ladder::{resolve_ref, Resolution};
use super::members::{declares_member, first_base_declaring};
use super::scope::FileContext;
use crate::extract::RegistrationRecord;
use crate::graph::{Edge, EdgesByKind, Fragment};
use std::collections::HashMap;

/// One registration's outcome once both type arguments have been resolved
/// against the graph: the def indices of the implementation and the
/// service, used both to build the type-level `implements` edge and to
/// scope the member-level pass to exactly these types.
#[derive(Debug, Clone, Copy)]
pub(super) struct ResolvedRegistration {
    pub implementation: usize,
    pub service: usize,
}

// Resolves one registration's type argument against the ladder, in the
// registration site's own namespace/using context -- a bare or dotted type
// reference, never a generic one: the extractor records the type argument's
// raw text verbatim, and a registration naming a closed generic service or
// implementation is exactly the "container semantics" case this feature
// does not attempt.
fn resolve_registered_type(
    raw: &str,
    ns: &str,
    file_ctx: &FileContext,
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
) -> Option<usize> {
    let probe = type_probe(raw, ns, &[]);
    match resolve_ref(
        &probe,
        &file_ctx.usings,
        ns,
        index,
        &file_ctx.aliases,
        file_contexts,
    ) {
        Resolution::Resolved(idx, _) => Some(idx),
        Resolution::Ambiguous(..) | Resolution::External => None,
    }
}

/// Turns every registration fact into a type-level `implements` edge,
/// returning the edges alongside the resolved (implementation,
/// service) pairs the member-level pass (`resolve_member_dispatch`) scopes
/// itself to. A registration whose service or implementation type does not
/// resolve to exactly one in-graph def emits no edge and contributes no
/// pair -- "never a guess" applied to a DI registration exactly as it is
/// everywhere else in this resolver.
pub(super) fn resolve_registrations(
    fragments_by_file: &[(String, Fragment)],
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
) -> (Vec<Edge>, Vec<ResolvedRegistration>) {
    let mut edges = Vec::new();
    let mut resolved = Vec::new();
    for (file, frag) in fragments_by_file {
        if frag.registrations.is_empty() {
            continue;
        }
        let Some(file_ctx) = file_contexts.get(file) else {
            continue;
        };
        for reg in &frag.registrations {
            let RegistrationRecord {
                service,
                implementation,
                namespace,
                line: _,
            } = registration_as_record(reg);
            let Some(service_idx) =
                resolve_registered_type(&service, &namespace, file_ctx, index, file_contexts)
            else {
                continue;
            };
            let Some(impl_idx) = resolve_registered_type(
                &implementation,
                &namespace,
                file_ctx,
                index,
                file_contexts,
            ) else {
                continue;
            };
            let impl_def = &index.defs[impl_idx];
            let service_def = &index.defs[service_idx];
            edges.push(Edge::Implements {
                from_file: impl_def.file.clone(),
                from_line: impl_def.line,
                to: service_def.id.clone(),
                to_file: service_def.file.clone(),
                member: None,
            });
            resolved.push(ResolvedRegistration {
                implementation: impl_idx,
                service: service_idx,
            });
        }
    }
    (edges, resolved)
}

// A `graph::FragRegistration` is the on-disk shape; the resolver reads the
// in-memory `extract::RegistrationRecord` shape `mapcmd` builds `Fragment`
// from, so this converts field-for-field rather than gaining a second
// dependency on `graph.rs`'s own record type.
fn registration_as_record(reg: &crate::graph::FragRegistration) -> RegistrationRecord {
    RegistrationRecord {
        service: reg.service.clone(),
        implementation: reg.implementation.clone(),
        namespace: reg.namespace.clone(),
        line: reg.line,
    }
}

// The number of (source overload, target overload) PAIRS of `member` that
// share an arity range -- the one arity-based uniqueness test both
// `resolve_member_implements` and `resolve_member_overrides` use. Exactly 1
// is the only resolved outcome: 0 means neither side's own facts back a
// pairing at all, and 2 or more means two or more candidates tie on EITHER
// side (two overloads on the implementation, or two on the interface/base),
// "ambiguous... emits nothing" applied to a signature match instead of a
// name lookup.
fn matching_overload_count(index: &DefIndex, source: usize, target: usize, member: &str) -> usize {
    let Some(source_ranges) = index.member_lists[source].method_arities.get(member) else {
        return 0;
    };
    let Some(target_ranges) = index.member_lists[target].method_arities.get(member) else {
        return 0;
    };
    source_ranges
        .iter()
        .map(|r| target_ranges.iter().filter(|t| *t == r).count())
        .sum()
}

// Member-level `implements` edges for one resolved registration: every
// method the registered SERVICE interface declares that
// the IMPLEMENTATION's own arity facts uniquely back.
fn resolve_member_implements(index: &DefIndex, reg: ResolvedRegistration) -> (Vec<Edge>, usize) {
    let mut edges = Vec::new();
    let mut count = 0;
    let service_def = &index.defs[reg.service];
    for member in service_def.methods.clone() {
        if matching_overload_count(index, reg.implementation, reg.service, &member) != 1 {
            continue;
        }
        let impl_def = &index.defs[reg.implementation];
        edges.push(Edge::Implements {
            from_file: impl_def.file.clone(),
            from_line: impl_def.line,
            to: service_def.id.clone(),
            to_file: service_def.file.clone(),
            member: Some(member),
        });
        count += 1;
    }
    (edges, count)
}

// Member-level `overrides` edges for one implementation type: every
// `override`-marked method paired with the nearest
// in-graph base member of the same name -- found the same way
// `base_member_declared`'s own base walk is, class bases before interfaces,
// depth-first -- that the arity facts uniquely back.
fn resolve_member_overrides(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    impl_idx: usize,
) -> (Vec<Edge>, usize) {
    let mut edges = Vec::new();
    let mut count = 0;
    for member in index.member_lists[impl_idx].override_methods.clone() {
        let Some(base_idx) =
            first_base_declaring(index, file_contexts, impl_idx, false, |idx, i| {
                declares_member(idx, i, Some(&member), None)
            })
        else {
            continue;
        };
        if matching_overload_count(index, impl_idx, base_idx, &member) != 1 {
            continue;
        }
        let impl_def = &index.defs[impl_idx];
        let base_def = &index.defs[base_idx];
        edges.push(Edge::Overrides {
            from_file: impl_def.file.clone(),
            from_line: impl_def.line,
            to: base_def.id.clone(),
            to_file: base_def.file.clone(),
            member: Some(member),
        });
        count += 1;
    }
    (edges, count)
}

/// Member-level `implements`/`overrides` edges, scoped to exactly
/// the implementation types `resolve_registrations` resolved: a repository
/// with no registration facts resolves this to nothing at all, which is
/// what keeps its `graph.json` byte-identical apart from the schema
/// version. `overrides` is computed once per implementation type, not once
/// per registration, so a type registered under two different service
/// interfaces does not double its own override edges.
pub(super) fn resolve_member_dispatch(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    registrations: &[ResolvedRegistration],
) -> (Vec<Edge>, usize, usize) {
    let mut edges = Vec::new();
    let mut implements_count = 0;
    let mut overrides_count = 0;
    let mut seen_impls: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for &reg in registrations {
        let (member_edges, member_count) = resolve_member_implements(index, reg);
        edges.extend(member_edges);
        implements_count += member_count;

        if seen_impls.insert(reg.implementation) {
            let (override_edges, override_count) =
                resolve_member_overrides(index, file_contexts, reg.implementation);
            edges.extend(override_edges);
            overrides_count += override_count;
        }
    }
    (edges, implements_count, overrides_count)
}

/// Runs both passes and folds their result into the graph under assembly:
/// every two-type-argument DI registration becomes a type-level `implements`
/// edge, and exactly the implementation types a registration resolved get
/// their own member-level `implements`/`overrides` edges. A repository with
/// no registration facts runs both passes over an empty list and adds
/// nothing, which is what keeps its `graph.json` byte-identical apart from
/// the schema version.
pub(super) fn append_dispatch_edges(
    fragments_by_file: &[(String, Fragment)],
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    edges: &mut Vec<Edge>,
    edges_by_kind: &mut EdgesByKind,
) {
    let (registration_edges, resolved) =
        resolve_registrations(fragments_by_file, index, file_contexts);
    edges_by_kind.implements += registration_edges.len();
    edges.extend(registration_edges);
    let (member_edges, implements, overrides) =
        resolve_member_dispatch(index, file_contexts, &resolved);
    edges_by_kind.implements += implements;
    edges_by_kind.overrides += overrides;
    edges.extend(member_edges);
}
