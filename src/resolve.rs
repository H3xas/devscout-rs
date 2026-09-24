// The resolution ladder, including ambiguous marking: `build_def_index`,
// `resolve_ref`, `collect_global_usings_by_unit`, `capped_candidates`,
// `resolve_graph`.
// Pure: no file I/O, no tree-sitter -- the only I/O this module performs is the
// single `git rev-parse HEAD` shell-out inside `resolve_graph`, delegated to
// `manifest::git_head`, and the emission record `provenance` writes when
// `SCOUT_EDGE_PROVENANCE` names a path, which no resolution ever reads.
// Artifact load/save and the fragments cache live in `graph.rs`.
//
// Ladder rules (see `resolve_ref`'s doc comment for the exact order):
//   0. Type ALIASES (`using Foo = Some.Ns.Bar;` and `global` counterpart)
//      short-circuit before the ladder for bare (non-dotted) references --
//      never ambiguous, never falls through.
//   1. Exact qualified name, tried at every ENCLOSING namespace prefix,
//      innermost first, only for dotted references. A dotted reference the
//      exact step misses gets two further chances and never reaches steps
//      2-4: steps 1a/1b (an alias at its head rewritten to the alias target,
//      then only a def whose full path ends with the written text, or a
//      nested def inside the inheritance closure of the type the qualifier
//      names), and then step 1.5.
//   1.5 A dotted qualifier walked through NESTED types: the shortest head
//      that names a type, then one exact `{id}+{segment}` lookup per
//      remaining segment. Skipped when the extractor vouches that the
//      qualifier is an instance. A dotted reference this step cannot answer
//      either finishes External rather than falling into steps 2-4.
//   2. File's usings (local ∪ every `global using`) + simple name, each
//      using name itself tried at every enclosing-namespace prefix.
//   3. The reference site's namespace and every ancestor of it, innermost
//      first (the ancestor-namespace rule -- a walk, like step 1).
//   4. Globally unique simple name.
//   A step that finds exactly one candidate resolves; two or more STOPS
//   there as ambiguous, never falling through looking for a tiebreaker.
//
// The enum-member asymmetry (the single most load-bearing invariant here):
// enum members ARE keyed in `qualified_name_to_def` (reachable by exact id,
// e.g. for uses-member resolution) but are EXCLUDED from `simple_name_to_defs`
// (the pool step 2/4's bare-name lookups draw from) -- see `build_def_index`.
// Losing this exclusion doesn't change any TYPE resolution that was already
// unambiguous via using/namespace/alias, but it does turn every type whose
// simple name collides with some unrelated enum's member name into a false
// ambiguous (or worse, a step-4 resolution picking the wrong one) purely
// because that enum happens to exist somewhere in the same build. The test
// `enum_member_does_not_collide_with_a_same_named_class_via_global_uniqueness`
// in `tests/ladder.rs` is a regression trap for exactly this: it fails loudly (ambiguous where
// it should resolve cleanly) if the exclusion is ever dropped.

mod arity;
mod assembly;
mod bus;
mod bus_vocab;
mod dispatch;
mod edges;
mod index;
mod ladder;
mod lambda_arity;
mod members;
mod provenance;
mod receiver;
mod scope;
pub use assembly::{resolve_graph, resolve_graph_with_model, resolve_graph_with_ts};
pub use index::{DefIndex, ExtCandidate, MemberLists, MethodOverloadParams};

#[cfg(test)]
mod tests;
