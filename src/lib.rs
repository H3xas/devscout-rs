//! Library support for the `devscout` code-indexing CLI.
//!
//! The parsing and extraction modules turn source files into fragments, the
//! graph and resolution modules connect those fragments, and the query and
//! rendering modules produce command results. The remaining modules locate
//! repositories, persist artifacts, and implement CLI commands and hooks.

/// Command-line argument parsing and dispatch.
pub mod cli;
/// C# and TypeScript declaration and reference extraction.
pub mod extract;
/// Persisted graph and fragment data structures.
pub mod graph;
/// Agent-hook input processing.
pub mod hookio;
/// Repository initialization commands.
pub mod initcmd;
/// Manifest persistence, search, and index-freshness checks.
pub mod manifest;
/// Repository mapping and incremental fragment-cache updates.
pub mod mapcmd;
/// XAML and XML markup declaration and reference extraction.
pub mod markup;
/// Tree-sitter parsing and source-span collection.
pub mod parse;
/// Graph queries and their result models.
pub mod query;
/// Text rendering for query results.
pub mod render;
/// Repository discovery and registry access.
pub mod repo;
/// Resolution of extracted C# fragments into a graph.
pub mod resolve;
/// SQLite-backed hook freshness and content stores.
pub mod store;
/// Symbol-name suggestions for unsuccessful queries.
pub mod suggest;
/// Resolution of extracted TypeScript fragments into a graph.
pub mod tsgraph;
/// Source-tree walking and default file-purpose generation.
pub mod walk;

// Off every code path today: the crate feeds the grammar a UTF-16 view of
// source, so tree-sitter node offsets already arrive as UTF-16 and
// `parse::utf16_index` halves them instead of translating. Kept as a
// unit-tested helper for a future caller that has to cross a UTF-8 buffer back
// into that offset convention.
/// UTF-8 to UTF-16 source-offset conversion.
pub mod offsets;
