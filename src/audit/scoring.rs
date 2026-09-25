// Scoring primitives -- pure, no I/O, shared by `score.rs`.

use std::collections::{HashMap, HashSet};

use super::model::{DefRow, EdgeRow, OracleRef, Tier, Unit};

/// Whether `e` -- a `Tier::SemanticDiscovered` edge -- must be scored
/// neither TP nor FP because the oracle has no vocabulary to judge it by at
/// all: its originating compiler occurrence carries `shape == "identifier"`
/// (a bare field/property/event read the oracle's own walker records no
/// case for). `false` for every other tier, unconditionally -- this
/// function is the one place that reads `discovered_shapes` at all, so a
/// caller never has to remember to gate the lookup itself.
pub fn is_unjudged_discovered_edge(
    e: &EdgeRow,
    discovered_shapes: &HashMap<(String, usize, String), String>,
) -> bool {
    if e.tier != Tier::SemanticDiscovered {
        return false;
    }
    let Some(member) = &e.member else {
        return false;
    };
    discovered_shapes
        .get(&(e.from_file.clone(), e.from_line, member.clone()))
        .is_some_and(|shape| shape == "identifier")
}

/// The target-match rule: an oracle record matches an
/// edge's `to` either literally, or -- for an enum-member record -- when the
/// edge names the bare enum type (`to` == the record's target with its last
/// `.member` segment dropped). The second arm is the "two-spelling" case:
/// `resolve.rs` writes an enum-member edge as `Ns.Enum.Member` when that
/// exact def exists and as bare `Ns.Enum` otherwise (fact table, graph.rs
/// :1121-1138), and either spelling is a correct answer to the same oracle
/// record.
pub fn target_matches(r: &OracleRef, edge_to: &str) -> bool {
    let Some(t) = &r.target else { return false };
    if t == edge_to {
        return true;
    }
    if r.target_kind.as_deref() == Some("enum-member") {
        if let Some((prefix, _)) = t.rsplit_once('.') {
            if prefix == edge_to {
                return true;
            }
        }
    }
    false
}

/// The match rule's second clause (member-join refinement): whether an
/// oracle record's own `member` agrees with an edge's. An edge with no
/// `member` at all -- a schema-1 graph, or the rare reference the extractor
/// recorded no member name for (`EdgeRow`'s doc comment) -- is unconstrained
/// here, so a caller combining this with `target_matches` for a no-member
/// edge gets EXACTLY `target_matches`'s own verdict: the legacy path stays
/// byte-identical. When the edge does name a member, a record sharing its
/// `(file, startLine)` but naming a DIFFERENT member is not evidence for it
/// -- the fixture's `tests/App.Tests/WorkerTests.cs` is the motivating
/// case, `Order.Load("x").Validate()`: one line, two member references
/// (`Load` on the `Order` qualifier, `Validate` on the chain's tail), and
/// only the `member`-matching record may vouch for either edge.
pub fn member_matches(edge_member: &Option<String>, record_member: &str) -> bool {
    match edge_member {
        Some(m) => m == record_member,
        None => true,
    }
}

/// Whether an oracle record's target is a def devscout's own graph knows
/// about -- the recall-D eligibility rule's third clause. Same two-spelling
/// allowance as `target_matches`: an enum-member record is "known" if either
/// its exact id or its bare enum id is a graph def.
pub fn target_known(defs_by_id: &HashMap<&str, &DefRow>, r: &OracleRef) -> bool {
    let Some(t) = &r.target else { return false };
    if defs_by_id.contains_key(t.as_str()) {
        return true;
    }
    if r.target_kind.as_deref() == Some("enum-member") {
        if let Some((prefix, _)) = t.rsplit_once('.') {
            if defs_by_id.contains_key(prefix) {
                return true;
            }
        }
    }
    false
}

/// An id's short name -- its last `.`- or `+`-separated segment (`+` splits a
/// nested type, e.g. `Ns.Outer+Inner` -> `Inner`) -- for the "top fp targets"
/// table, which reports by short name rather than the full id, so a row reads
/// `FilterConfig`, not `Fixture.Domain.FilterConfig`.
pub fn short_name(id: &str) -> &str {
    id.rsplit(['.', '+']).next().unwrap_or(id)
}

/// `unit(file)` -- first-match file->unit-name map built from every unit's
/// `files` list.
pub fn file_to_unit(units: &[Unit]) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for u in units {
        for f in &u.files {
            m.entry(f.clone()).or_insert_with(|| u.name.clone());
        }
    }
    m
}

/// `reach(unit)` for every unit: the transitive closure of `refs` plus the
/// unit itself -- a unit always reaches itself. A
/// `refs` entry naming a unit this set has never heard of (a project
/// reference the oracle could not resolve to one of its own units) is simply
/// a dead end -- it counts as reached but contributes no further edges.
pub fn reach(units: &[Unit]) -> HashMap<String, HashSet<String>> {
    let by_name: HashMap<&str, &Unit> = units.iter().map(|u| (u.name.as_str(), u)).collect();
    let mut result = HashMap::new();
    for u in units {
        let mut seen: HashSet<String> = HashSet::new();
        let mut stack = vec![u.name.clone()];
        while let Some(n) = stack.pop() {
            if seen.contains(&n) {
                continue;
            }
            seen.insert(n.clone());
            if let Some(unit) = by_name.get(n.as_str()) {
                for r in &unit.refs {
                    if !seen.contains(r) {
                        stack.push(r.clone());
                    }
                }
            }
        }
        result.insert(u.name.clone(), seen);
    }
    result
}

/// The structural check for one edge: `None` when it cannot be checked (the
/// edge is excluded from both `checked` and `impossible`), else `Some(true)`
/// when the edge is structurally impossible.
///
/// `"units"` method: impossible when `to_file`'s unit is
/// not in `from_file`'s unit's `reach` set. Either file failing to resolve to
/// a unit at all (not listed in any unit's `files`) makes the edge
/// unchecked.
///
/// `"test-defs"` fallback: impossible when the target def is itself
/// test-attributed (`def(to).test`) and the calling file declares no
/// test-attributed def of its own. The target def must be known (found in
/// `test_by_id`) for the edge to be checked at all; an unknown target
/// (`to` not in either def source) is unchecked, not "not impossible".
pub fn is_structural(
    e: &EdgeRow,
    units_method: bool,
    file_unit: &HashMap<String, String>,
    reach_map: &HashMap<String, HashSet<String>>,
    test_by_id: &HashMap<String, bool>,
    test_files: &HashSet<String>,
) -> Option<bool> {
    if units_method {
        let u1 = file_unit.get(&e.from_file)?;
        let u2 = file_unit.get(&e.to_file)?;
        let reachable = reach_map.get(u1).is_some_and(|s| s.contains(u2));
        Some(!reachable)
    } else {
        let target_test = *test_by_id.get(&e.to)?;
        if !target_test {
            return Some(false);
        }
        Some(!test_files.contains(&e.from_file))
    }
}
