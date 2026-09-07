// Member-seed resolution: `Member`, `Type.Member` and `Namespace.Type.Member`
// spellings, reached only once `resolve_symbol`'s type ladder has already
// missed (see `symbol.rs`) -- a name that resolves to a type never reaches
// this module. A member is never a `graph::Def`: the extractor records one as
// a `graph::GraphName` (method/property/field/event/enum-member) carrying an
// `owner` def id, so resolving one means scanning `graph.names`, not
// `by_simple_name`.

use std::collections::HashMap;

use crate::suggest::MEMBER_KINDS;

use super::index::{DefSite, GraphIndex};

/// Splits a member seed into its trailing member name and, when the seed
/// carries a `.`, the qualifier before it (`Type` for `Type.Member`,
/// `Namespace.Type` for `Namespace.Type.Member`). A seed with no `.` is a
/// bare member name and carries no qualifier.
pub(crate) fn split_member_seed(seed: &str) -> (&str, Option<&str>) {
    match seed.rfind('.') {
        Some(pos) => (&seed[pos + 1..], Some(&seed[..pos])),
        None => (seed, None),
    }
}

// Whether `owner_id` (a def id) is named by `qualifier`: an exact match, or a
// dotted-suffix match -- the same tail rule `resolve_symbol`'s own dotted step
// applies to a type id (see `symbol.rs`), so `Type.Member` and
// `Namespace.Type.Member` both reach a def however deeply its own namespace
// nests, without this module restating what "named by a dotted qualifier"
// means.
fn owner_matches_qualifier(owner_id: &str, qualifier: &str) -> bool {
    owner_id == qualifier || owner_id.ends_with(&format!(".{qualifier}"))
}

/// Every type declaring a member named `name`, in name-index order, each with
/// every site it declares it at.
///
/// The same shape `refs.rs`'s own `member_owners` used to build by hand, now
/// shared so `impact`/`tests` can resolve a member seed too. `qualifier`,
/// when present, narrows the result to owners [`owner_matches_qualifier`]
/// admits; `None` (a bare seed) admits every owner, unchanged from the
/// bare-member behavior this replaces. Restricted to [`MEMBER_KINDS`] and an
/// owner already in the index -- a markup or resource name carries no owner
/// and never reaches here.
pub fn qualified_member_owners(
    index: &GraphIndex,
    name: &str,
    qualifier: Option<&str>,
) -> Vec<(String, Vec<DefSite>)> {
    let mut out: Vec<(String, Vec<DefSite>)> = Vec::new();
    let mut at: HashMap<&str, usize> = HashMap::new();
    for n in &index.graph.names {
        if n.name != name
            || n.owner.is_empty()
            || !index.by_id.contains_key(&n.owner)
            || !MEMBER_KINDS.contains(&n.kind.as_str())
        {
            continue;
        }
        if let Some(q) = qualifier {
            if !owner_matches_qualifier(&n.owner, q) {
                continue;
            }
        }
        let site = DefSite {
            file: n.file.clone(),
            line: n.line,
        };
        match at.get(n.owner.as_str()) {
            Some(&i) => out[i].1.push(site),
            None => {
                at.insert(n.owner.as_str(), out.len());
                out.push((n.owner.clone(), vec![site]));
            }
        }
    }
    out
}

/// One row of an ambiguous member seed: the declaring type, the member name,
/// and the FIRST site that type declares it at.
///
/// Never a bare type list (a candidate carries the member's own name and
/// declaration site, not just the type's) -- what a member-ambiguous answer
/// renders instead of reusing an ambiguous type's own `{id, def site, kind}`
/// rows.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberCandidate {
    /// The def id of the declaring type.
    pub owner: String,
    /// The member's own name.
    pub name: String,
    /// The file the member is declared in.
    pub file: String,
    /// The line the member is declared at.
    pub line: usize,
}

/// Whether resolving `seed` as a member's declaring type, ignoring inbound
/// references entirely (unlike `refs.rs`'s own edge-verified fallback).
///
/// `impact`/`tests` answer AS the member's unique declaring type, so all they
/// need is that the member exists at all, never that something already
/// references it.
#[derive(Debug, Clone, PartialEq)]
pub enum MemberSeedResolution {
    /// Exactly one type declares the member; its def id.
    Resolved(String),
    /// More than one type declares the member.
    Ambiguous(Vec<MemberCandidate>),
    /// No type in the graph declares a member by this name (and qualifier,
    /// when the seed carried one).
    NotFound,
}

/// Resolve `seed` (`Member`, `Type.Member`, or `Namespace.Type.Member`) to
/// the def id of the type that uniquely declares it.
///
/// Used by `impact`/`tests`, which answer as the member's declaring type
/// rather than building a member-shaped model of their own (that shape is
/// `refs.rs`'s, reused wholesale by `read.rs`).
pub fn resolve_member_seed(index: &GraphIndex, seed: &str) -> MemberSeedResolution {
    let (name, qualifier) = split_member_seed(seed);
    let owners = qualified_member_owners(index, name, qualifier);
    match owners.len() {
        0 => MemberSeedResolution::NotFound,
        1 => MemberSeedResolution::Resolved(owners[0].0.clone()),
        _ => MemberSeedResolution::Ambiguous(member_candidates(&owners, name)),
    }
}

/// One [`MemberCandidate`] row per owner in `owners`, first site only.
pub fn member_candidates(owners: &[(String, Vec<DefSite>)], name: &str) -> Vec<MemberCandidate> {
    owners
        .iter()
        .map(|(owner, sites)| MemberCandidate {
            owner: owner.clone(),
            name: name.to_string(),
            file: sites[0].file.clone(),
            line: sites[0].line,
        })
        .collect()
}

/// The synthetic, fully-qualified seed `--pick` re-resolves against.
///
/// Names the exact owner a candidate row named, so the ladder that answered
/// the first time answers again, narrowed to one candidate -- no extra
/// resolution path only `--pick` takes.
pub fn qualified_seed(candidate: &MemberCandidate) -> String {
    format!("{}.{}", candidate.owner, candidate.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_member_seed_separates_the_trailing_name_from_a_dotted_qualifier() {
        assert_eq!(split_member_seed("Stow"), ("Stow", None));
        assert_eq!(split_member_seed("Bin.Stow"), ("Stow", Some("Bin")));
        assert_eq!(
            split_member_seed("Acme.Storage.Bin.Stow"),
            ("Stow", Some("Acme.Storage.Bin"))
        );
    }

    #[test]
    fn owner_matches_qualifier_accepts_an_exact_id_or_a_dotted_suffix() {
        assert!(owner_matches_qualifier("Acme.Storage.Bin", "Bin"));
        assert!(owner_matches_qualifier("Acme.Storage.Bin", "Storage.Bin"));
        assert!(owner_matches_qualifier(
            "Acme.Storage.Bin",
            "Acme.Storage.Bin"
        ));
        assert!(!owner_matches_qualifier("Acme.Storage.Bin", "Vault"));
        assert!(
            !owner_matches_qualifier("Acme.Storage.Bin", "torage.Bin"),
            "a suffix match starts on a dot boundary, not mid-segment"
        );
    }
}
