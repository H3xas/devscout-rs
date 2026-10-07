// The enum-member split of a `refs` answer, split out of `refs.rs` to keep
// that file under this crate's per-file line budget. A pure move: both types
// keep the path they had through `query.rs`'s re-exports.

use super::index::GraphIndex;

/// One enum member that carries at least one inbound reference.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberRefEntry {
    /// The name value.
    pub name: String,
    /// The count value.
    pub count: usize,
}

/// How many of an ENUM's inbound member edges land on one of its members,
/// split by member.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberRefs {
    /// The total value.
    pub total: usize,
    /// The member count value.
    pub member_count: usize,
    /// The members value.
    pub members: Vec<MemberRefEntry>,
    /// The dropped value.
    pub dropped: usize,
}

// Maximum number of per-member rows kept in a `MemberRefs`.
const MEMBER_NAME_CAP: usize = 5;

// The edges themselves are already there (a `Toggles.EnableX` access resolves
// to the member def and `symbol_refs` unions every member's inbound into the
// enum's), but a caller reading `refs Toggles` could not tell a use of the
// TYPE from a use of a member, and the per-member split is the thing an enum
// question is usually actually about.
//
// Member order is def-table order (declaration order), never by count: a
// stable list is what makes two runs print the same line. Iterates
// `graph.defs` directly for the same reason `symbol_refs`'s own enum union
// does -- that array order is the order to preserve.
pub(super) fn enum_member_refs(index: &GraphIndex, def_id: &str) -> Option<MemberRefs> {
    let prefix = format!("{def_id}.");
    let mut members: Vec<MemberRefEntry> = Vec::new();
    let mut total = 0usize;
    for d in &index.graph.defs {
        if d.kind != "enum-member" || !d.id.starts_with(&prefix) {
            continue;
        }
        let count = index.inbound.get(&d.id).map_or(0, |e| e.uses_member.len())
            + index
                .heuristic_inbound
                .get(&d.id)
                .map_or(0, |e| e.uses_member.len());
        if count == 0 {
            continue;
        }
        members.push(MemberRefEntry {
            name: d.name.clone(),
            count,
        });
        total += count;
    }
    if total == 0 {
        return None;
    }
    let member_count = members.len();
    let dropped = member_count.saturating_sub(MEMBER_NAME_CAP);
    members.truncate(MEMBER_NAME_CAP);
    Some(MemberRefs {
        total,
        member_count,
        members,
        dropped,
    })
}
