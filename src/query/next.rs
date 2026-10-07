// The follow-up verbs a resolved answer suggests, derived from the model the
// answer already computed -- no second graph walk and no second model build.
//
// Every candidate is filtered through the resolver the SUGGESTED verb itself
// uses, so an emitted argv always resolves to exactly one target. That filter
// is what makes "every suggestion runs" a property of this module rather than
// a promise the caller has to keep: a candidate the verb would answer
// ambiguously or not at all is dropped here, where its argv would otherwise be
// printed as though it were runnable.

use crate::graph;
use crate::suggest::kind_rank;

use super::coverage::TestsModel;
use super::impact::ImpactModel;
use super::impact_seed::{resolve_impact_seed, SeedResolution};
use super::index::GraphIndex;
use super::member::{resolve_member_seed, MemberSeedResolution};
use super::refs::RefsModel;
use super::symbol::{resolve_symbol, Resolution};

/// One runnable suggestion: the argv a caller would type, WITHOUT the binary
/// name. The binary is the caller's to know -- it is not always `devscout`.
pub type NextArgv = Vec<String>;

// One suggestion, the two-string shape every builder below emits.
fn argv(verb: &str, arg: &str) -> NextArgv {
    vec![verb.to_string(), arg.to_string()]
}

// Whether `seed` reaches exactly one target through the ladder `refs`, `read`
// and `tests` share: the symbol ladder, then the member fallback a bare member
// seed takes.
fn symbol_target_resolves(index: &GraphIndex, seed: &str) -> bool {
    match resolve_symbol(index, seed) {
        Resolution::Resolved(_) => true,
        Resolution::Ambiguous(_) => false,
        Resolution::NotFound => matches!(
            resolve_member_seed(index, seed),
            MemberSeedResolution::Resolved(_)
        ),
    }
}

// Whether `seed` reaches exactly one target through `impact`'s own resolver,
// which reads a file path differently from a symbol (see `resolve_impact_seed`).
fn impact_target_resolves(index: &GraphIndex, seed: &str) -> bool {
    matches!(
        resolve_impact_seed(index, seed),
        SeedResolution::Resolved { .. }
    )
}

// A suggestion is only worth printing if the verb would READ its argument as
// the seed. Every query verb skips an argument that starts with `--`, so a
// path or an id that does would be taken for a flag: the command would come
// back a usage error rather than an answer, which is exactly what the resolve
// filter exists to prevent.
fn reads_as_a_seed(argv: &NextArgv) -> bool {
    argv.iter().skip(1).all(|arg| !arg.starts_with("--"))
}

// Every list leaves through here, so the two rules that make a suggestion worth
// printing -- it runs, and it is stated once -- live at one point: emission
// order, first occurrence wins, and an argument no verb would read as a seed
// drops the candidate.
fn dedup(candidates: Vec<NextArgv>) -> Vec<NextArgv> {
    let mut out: Vec<NextArgv> = Vec::new();
    for candidate in candidates {
        if reads_as_a_seed(&candidate) && !out.contains(&candidate) {
            out.push(candidate);
        }
    }
    out
}

/// The ordered, de-duplicated union of several suggestion lists.
///
/// A bare-member answer gives each member its own list; the wrapper states
/// what all of them together amount to.
pub fn merge_suggestions(lists: Vec<Vec<NextArgv>>) -> Vec<NextArgv> {
    dedup(lists.into_iter().flatten().collect())
}

/// Suggestions after a `refs` or `read` answer.
///
/// The tests that reach the symbol, then the blast radius of the file
/// declaring it. `read` answers with `refs`' own inbound model, so one builder
/// serves both verbs.
pub fn refs_next(index: &GraphIndex, model: &RefsModel) -> Vec<NextArgv> {
    let mut candidates = Vec::new();
    if symbol_target_resolves(index, &model.id) {
        candidates.push(argv("tests", &model.id));
    }
    if let Some(site) = model.sites.first() {
        if impact_target_resolves(index, &site.file) {
            candidates.push(argv("impact", &site.file));
        }
    }
    dedup(candidates)
}

/// Suggestion after a `tests` answer: the blast radius of the file declaring
/// the symbol -- the question the test list itself cannot answer.
pub fn tests_next(index: &GraphIndex, model: &TestsModel) -> Vec<NextArgv> {
    let Some(def) = index.def(&model.symbol) else {
        return Vec::new();
    };
    if impact_target_resolves(index, &def.file) {
        dedup(vec![argv("impact", &def.file)])
    } else {
        Vec::new()
    }
}

/// Suggestion after an `impact` answer.
///
/// The references of the highest-ranked row's first top symbol, the symbol
/// whose inbound edges pulled that file into the blast radius. No rows at all,
/// or a row that kept no symbol, suggests nothing.
pub fn impact_next(index: &GraphIndex, model: &ImpactModel) -> Vec<NextArgv> {
    let Some(id) = model.rows.first().and_then(|r| r.top_symbol_id.as_deref()) else {
        return Vec::new();
    };
    if symbol_target_resolves(index, id) {
        dedup(vec![argv("refs", id)])
    } else {
        Vec::new()
    }
}

/// Suggestion after a `find` answer: read the declaration the row names.
///
/// A member row declares no def of its own, so it names the type declaring it;
/// a markup or resource name names no declaration at all and suggests nothing.
pub fn find_next(index: &GraphIndex, name: &graph::GraphName) -> Vec<NextArgv> {
    if kind_rank(&name.kind) > 1 {
        return Vec::new();
    }
    let id = if name.owner.is_empty() {
        // A type-level row carries no def id, so the declaring def is found by
        // the site the row itself names. A name index and a def table built
        // from the same graph agree on all three, so a miss here means the
        // row names something that is not a declaration.
        match index
            .graph
            .defs
            .iter()
            .find(|d| d.file == name.file && d.line == name.line && d.name == name.name)
        {
            Some(def) => def.id.as_str(),
            None => return Vec::new(),
        }
    } else {
        name.owner.as_str()
    };
    if symbol_target_resolves(index, id) {
        dedup(vec![argv("read", id)])
    } else {
        Vec::new()
    }
}
