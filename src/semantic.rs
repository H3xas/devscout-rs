// The resolve-time compiler-fact consumption path: the compiler-enrichment
// layer's only production module family, sibling to `graph/` and
// `resolve/`. Reached only from `resolve::assembly`'s own per-reference
// wrapper, from the load point `lane::decide` (called by
// `mapcmd::map_repo`), and from `lane::record`, which `graph::rebuild_graph`
// runs between resolution and the write -- the resolver itself does no file
// I/O, and neither does anything on the hook path, per the shape every
// module here preserves: fail open to the syntax ladder, one admitted
// artifact, no second admission path, no second identity scheme.

mod discovered;
mod freshness;
mod lane;
mod layer;
mod precedence;
mod uncertainty;

pub use discovered::project as project_discovered_sites;
pub use freshness::Freshness;
pub use lane::{decide as decide_lane, record as record_lane, LaneDecision};
pub use layer::{LookupOutcome, SemanticLayer, SemanticTarget};
pub use precedence::{apply, decide, finish, track_reference, Override, SemanticDiagnostics};
pub use uncertainty::Uncertainty;
