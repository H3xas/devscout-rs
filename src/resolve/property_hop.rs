use super::arity::resolve_receiver_type;
use super::index::{name_probe, DefIndex};
use super::ladder::{resolve_ref, Resolution};
use super::members::declares_here_for_ref;
use super::scope::{FileContext, FileUsings};
use crate::graph::FragRef;
use std::collections::HashMap;

// A two-segment receiver whose head has a known type can bind through that
// type's recorded property declaration. The property fact's argument list
// selects a same-named generic sibling; an absent list retains the receiver
// helper's existing arity-blind fallback. The second type must declare the
// member itself, with the same call and delegate gate as a typed receiver.
// This hop deliberately does not walk the second type's bases.
pub(super) fn property_hop_target(
    r: &FragRef,
    usings: &FileUsings,
    ns: &str,
    index: &DefIndex,
    aliases: &HashMap<String, String>,
    file_contexts: &HashMap<String, FileContext>,
    source_file: &str,
) -> Option<usize> {
    let owner = r.receiver_property_owner.as_ref()?;
    let probe = name_probe(owner.clone(), ns, r.outer_types.clone());
    let Resolution::Resolved(owner_idx, _) =
        resolve_ref(&probe, usings, ns, index, aliases, file_contexts)
    else {
        return None;
    };
    let fact = index.member_lists[owner_idx].property_types.get(&r.name)?;
    let hop = name_probe(fact.type_name.clone(), ns, r.outer_types.clone());
    let Resolution::Resolved(target_idx, _) = resolve_receiver_type(
        hop,
        fact.args.as_ref().map(Vec::len),
        usings,
        ns,
        index,
        aliases,
        file_contexts,
    ) else {
        return None;
    };
    declares_here_for_ref(index, file_contexts, target_idx, r, false, source_file)
        .then_some(target_idx)
}
