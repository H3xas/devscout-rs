use super::arity::generic_args_unify;
use super::index::{name_probe, DefIndex};
use super::ladder::{resolve_ref, Resolution};
use super::scope::FileContext;
use crate::graph::{Def, Edge, FragRef, HeuristicTier};
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Constructor-parameter DI resolution.
// ---------------------------------------------------------------------------

// Reverse index over EVERY def's `bases`: bare base name -> every def index
// whose bases array names it. This is the raw candidate pool a ctor-param ref's
// interface name is matched against; `resolve_ctor_param` confirms each
// candidate the same way `inherited_member_declared` confirms a base name -- by
// re-resolving it through the ladder in the CANDIDATE's own file context --
// before trusting the bare-name match, so a same-named-but-unrelated interface
// elsewhere in the corpus can never masquerade as an implementor.
pub(super) fn build_implementor_index(index: &DefIndex) -> HashMap<String, Vec<usize>> {
    let mut map: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, lists) in index.member_lists.iter().enumerate() {
        for base_name in &lists.bases {
            map.entry(base_name.clone()).or_default().push(idx);
        }
    }
    map
}

// The "namespace prefix is System./Microsoft." half of the infra-leaf
// classification. A heuristic, not a semantic check: scout parses no project
// file and does no package-reference resolution, so this reads the reference
// SITE's own usings (file-local ∪ global) rather than the unresolved type's
// true origin -- a documented imprecision.
fn is_infra_namespace(name: &str) -> bool {
    name == "System"
        || name == "Microsoft"
        || name.starts_with("System.")
        || name.starts_with("Microsoft.")
}

/// Outcome of resolving one 'ctor-param' ref, as a Rust enum -- the caller
/// turns this into the `Edge::CtorDi` variant's fields.
pub(super) enum CtorDiResolution {
    /// A single non-generic implementor of a non-generic interface.
    Plain(usize),
    /// A single non-generic implementor whose own base-list entry names the
    /// SAME closed arguments the ctor param does.
    Closed(usize),
    /// A single implementor that is itself generic and passes the
    /// interface's type argument straight through. Preferred only when no
    /// closed implementor exists.
    OpenGeneric(usize),
    /// Two or more implementors tie at the same precedence tier -- never a
    /// guess.
    Ambiguous(Vec<usize>),
    /// The interface itself does not resolve in the corpus AND the reference
    /// site's usings name a BCL/Microsoft namespace: a framework leaf, not a
    /// corpus gap.
    Infra,
    /// Every other case: the interface resolves in-corpus but nothing
    /// implements it, or it resolves to nothing and no using suggests a
    /// framework origin. Still EMITTED by the caller (never silently
    /// dropped, unlike the general ladder's `unresolved_external` count).
    Unresolved,
}

pub(super) fn resolve_ctor_param(
    ref_: &FragRef,
    usings: &HashSet<String>,
    ns: &str,
    index: &DefIndex,
    aliases: &HashMap<String, String>,
    file_contexts: &HashMap<String, FileContext>,
    implementors_by_base_name: &HashMap<String, Vec<usize>>,
) -> CtorDiResolution {
    match resolve_ref(ref_, usings, ns, index, aliases, file_contexts) {
        Resolution::Ambiguous(candidate_indices, _) => {
            CtorDiResolution::Ambiguous(candidate_indices)
        }
        Resolution::External => {
            if usings.iter().any(|u| is_infra_namespace(u)) {
                CtorDiResolution::Infra
            } else {
                CtorDiResolution::Unresolved
            }
        }
        Resolution::Resolved(iface_idx, _) => {
            let base_name = &ref_.name;
            let raw_candidates = implementors_by_base_name
                .get(base_name)
                .cloned()
                .unwrap_or_default();
            let mut closed_or_plain: Vec<usize> = Vec::new();
            let mut open_generic: Vec<usize> = Vec::new();
            for cand in raw_candidates {
                if cand == iface_idx {
                    continue;
                }
                let cand_def = &index.defs[cand];
                let Some(cand_ctx) = file_contexts.get(&cand_def.file) else {
                    continue;
                };
                // Re-resolve the SAME base name through the CANDIDATE's own
                // file context -- exactly inherited_member_declared's own
                // pattern.
                let probe = name_probe(base_name.clone(), &cand_def.namespace, Vec::new());
                let base_res = resolve_ref(
                    &probe,
                    &cand_ctx.usings,
                    &cand_def.namespace,
                    index,
                    &cand_ctx.aliases,
                    file_contexts,
                );
                let Resolution::Resolved(resolved_iface, _) = base_res else {
                    continue;
                };
                if resolved_iface != iface_idx {
                    continue;
                }
                let cand_args = index.member_lists[cand]
                    .base_generic_args
                    .iter()
                    .find(|(k, _)| k == base_name)
                    .map(|(_, v)| v);
                if !generic_args_unify(cand_args, ref_.args.as_ref()) {
                    continue;
                }
                if cand_args
                    .map(|a| a.iter().any(|x| x == "*"))
                    .unwrap_or(false)
                {
                    open_generic.push(cand);
                } else {
                    closed_or_plain.push(cand);
                }
            }
            if closed_or_plain.len() == 1 {
                return if ref_.args.is_some() {
                    CtorDiResolution::Closed(closed_or_plain[0])
                } else {
                    CtorDiResolution::Plain(closed_or_plain[0])
                };
            }
            if closed_or_plain.len() > 1 {
                return CtorDiResolution::Ambiguous(closed_or_plain);
            }
            if open_generic.len() == 1 {
                return CtorDiResolution::OpenGeneric(open_generic[0]);
            }
            if open_generic.len() > 1 {
                return CtorDiResolution::Ambiguous(open_generic);
            }
            CtorDiResolution::Unresolved
        }
    }
}

// ---------------------------------------------------------------------------
// Top-level orchestration.
// ---------------------------------------------------------------------------

pub(super) fn type_edge(kind: &str, file: &str, line: usize, target: &Def) -> Edge {
    let from_file = file.to_string();
    let to = target.id.clone();
    let to_file = target.file.clone();
    match kind {
        "inherits" => Edge::Inherits {
            from_file,
            from_line: line,
            to,
            to_file,
            heuristic: false,
        },
        _ => Edge::UsesType {
            from_file,
            from_line: line,
            to,
            to_file,
            heuristic: false,
        },
    }
}

// The identity a heuristic edge is deduped on: everything its serialized form
// carries. `None` for a precise edge, which is never a dedup subject. Field
// order matches the edge's own, so two edges share a key exactly when they
// serialize to the same bytes -- which is why `tier` and `member` join the
// key the moment they join the edge: two guesses that name DIFFERENT members
// of the same target on one line are two distinct facts now, and collapsing
// them would drop one.
pub(super) fn heuristic_edge_key(e: &Edge) -> Option<String> {
    let (kind, from_file, from_line, to, to_file, tier, member) = match e {
        Edge::Inherits {
            from_file,
            from_line,
            to,
            to_file,
            heuristic: true,
        } => ("inherits", from_file, from_line, to, to_file, None, None),
        Edge::UsesType {
            from_file,
            from_line,
            to,
            to_file,
            heuristic: true,
        } => ("uses-type", from_file, from_line, to, to_file, None, None),
        Edge::UsesMember {
            from_file,
            from_line,
            to,
            to_file,
            heuristic: true,
            tier,
            member,
        } => (
            "uses-member",
            from_file,
            from_line,
            to,
            to_file,
            *tier,
            member.as_deref(),
        ),
        _ => return None,
    };
    let tier = match tier {
        Some(HeuristicTier::Ext) => "ext",
        Some(HeuristicTier::Guess) => "guess",
        None => "-",
    };
    let member = member.unwrap_or("-");
    Some(format!(
        "{kind} {from_file} {from_line} {to} {to_file} {tier} {member}"
    ))
}
