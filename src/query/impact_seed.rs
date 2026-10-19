// Seed resolution for the impact walk, split out of `impact.rs` to keep that
// file under this crate's per-file line budget. A pure move: every item here
// keeps the path it had through `query.rs`'s re-exports.

use super::impact::SeedKind;
use super::index::GraphIndex;
use super::member::{self, MemberCandidate, MemberSeedResolution};
use super::symbol::{resolve_local_symbol, resolve_symbol, Resolution};

/// Whether `arg` looks like a file path rather than a symbol query: a bare
/// simple/qualified name with no path separator or file extension is a symbol
/// query; anything else is a file path. Equivalent to `/\.[A-Za-z0-9]+$/` by
/// checking the LAST `.` only (if an earlier dot's suffix were all-alnum, the
/// last dot's suffix -- which contains that earlier dot -- could not be, since
/// `.` is not `[A-Za-z0-9]`). Note a fully-qualified symbol id like
/// `App.Widgets.IWidget` (trailing segment alphanumeric) DOES look like a file
/// path under this heuristic.
pub fn looks_like_file_path(arg: &str) -> bool {
    if arg.contains('/') {
        return true;
    }
    match arg.rfind('.') {
        Some(pos) => {
            let ext = &arg[pos + 1..];
            !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric())
        }
        None => false,
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Represents `SeedResolution`.
pub enum SeedResolution {
    /// The value value.
    Resolved {
        /// The resolved seed kind.
        kind: SeedKind,
        /// The resolved definition identifiers.
        ids: Vec<String>,
    },
    /// The value value.
    Ambiguous {
        /// The seed kind.
        kind: SeedKind,
        /// The candidate definition identifiers.
        ids: Vec<String>,
    },
    /// The value value.
    NotFound {
        /// The inferred seed kind.
        kind: SeedKind,
    },
    /// A member seed whose name more than one type declares.
    MemberAmbiguous(Vec<MemberCandidate>),
}

// The member ladder, shared by both branches below: a member seed answers as
// its unique declaring type; `not_found_kind` is the caller's own miss kind.
fn member_fallback(index: &GraphIndex, arg: &str, not_found_kind: SeedKind) -> SeedResolution {
    match member::resolve_member_seed(index, arg) {
        MemberSeedResolution::Resolved(id) => SeedResolution::Resolved {
            kind: SeedKind::Symbol,
            ids: vec![id],
        },
        MemberSeedResolution::Ambiguous(candidates) => SeedResolution::MemberAmbiguous(candidates),
        MemberSeedResolution::NotFound => match resolve_local_symbol(index, arg) {
            Resolution::Resolved(id) => SeedResolution::Resolved {
                kind: SeedKind::Symbol,
                ids: vec![id],
            },
            Resolution::Ambiguous(ids) => SeedResolution::Ambiguous {
                kind: SeedKind::Symbol,
                ids,
            },
            Resolution::NotFound => SeedResolution::NotFound {
                kind: not_found_kind,
            },
        },
    }
}

/// Resolve an impact-walk seed to a file's defs or a single symbol. The type
/// ladder ([`resolve_symbol`]) runs first; a member seed, tried only once it
/// has missed, answers AS its unique declaring type -- `impact` has no
/// member-shaped answer of its own. A dotted member seed looks like a file
/// path under [`looks_like_file_path`], so it is tried on a file-path MISS
/// instead: that branch never resolved a type before, so nothing changes.
pub fn resolve_impact_seed(index: &GraphIndex, arg: &str) -> SeedResolution {
    if index.by_id.contains_key(arg) {
        return SeedResolution::Resolved {
            kind: SeedKind::Symbol,
            ids: vec![arg.to_string()],
        };
    }
    if looks_like_file_path(arg) {
        return match index.by_file.get(arg) {
            Some(ids) if !ids.is_empty() => SeedResolution::Resolved {
                kind: SeedKind::File,
                ids: ids
                    .iter()
                    .map(|&i| index.graph.defs[i].id.clone())
                    .collect(),
            },
            _ => member_fallback(index, arg, SeedKind::File),
        };
    }
    match resolve_symbol(index, arg) {
        Resolution::Resolved(id) => SeedResolution::Resolved {
            kind: SeedKind::Symbol,
            ids: vec![id],
        },
        Resolution::Ambiguous(ids) => SeedResolution::Ambiguous {
            kind: SeedKind::Symbol,
            ids,
        },
        Resolution::NotFound => member_fallback(index, arg, SeedKind::Symbol),
    }
}
