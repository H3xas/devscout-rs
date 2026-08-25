//! Crate root. Declares the crate's top-level modules.

pub mod cli;
pub mod repo;
pub mod walk;
pub mod parse;
pub mod extract;
pub mod markup;
pub mod resolve;
pub mod tsgraph;
pub mod graph;
pub mod query;
pub mod suggest;
pub mod render;
pub mod store;
pub mod hookio;
pub mod initcmd;
pub mod manifest;
pub mod mapcmd;

// Off every code path today: the crate feeds the grammar a UTF-16 view of
// source, so tree-sitter node offsets already arrive as UTF-16 and
// `parse::utf16_index` halves them instead of translating. Kept as a
// unit-tested helper for a future caller that has to cross a UTF-8 buffer back
// into that offset convention.
pub mod offsets;
