// The resolve-time compiler-fact consumption path: the compiler-enrichment
// design's only production module family, sibling to `graph/` and
// `resolve/`. Reached only from `resolve::assembly`'s own per-reference
// wrapper and `mapcmd::map_repo`'s existing load point -- the resolver
// itself does no file I/O, and neither does anything on the hook path, per
// the retained design shape every module here preserves: fail open to the
// syntax ladder, one admitted artifact, no second admission path, no second
// identity scheme.

mod discovered;
mod freshness;
mod layer;
mod precedence;
mod uncertainty;

pub use discovered::project as project_discovered_sites;
pub use freshness::Freshness;
pub use layer::{LookupOutcome, SemanticLayer, SemanticTarget};
pub use precedence::{apply, decide, finish, track_reference, Override, SemanticDiagnostics};
pub use uncertainty::Uncertainty;
