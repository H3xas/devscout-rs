// Purposes + def/ref candidates + enum-member defs, onto native tree-sitter
// behind the grammar seam in parse.rs. This is the AST-purpose and def/ref
// extractor; see offsets.rs for the span translation it depends on.
//
// Scope (purpose composition + graph-fragment extraction, run per file):
//   - purpose: a compact one-line signature of a file's namespace-level
//     types (class/interface/struct/record/enum), truncated to 200 UTF-16
//     code units.
//   - defs: namespace-level + nested types (id = dotted namespace, "+"
//     joined for nested-type chains) and enum members (id =
//     "<EnumFQN>.<Member>"), each with its public method names.
//   - usings: using directives, either {alias, target, global} (alias
//     form) or {text, global} (plain/static/global form).
//   - refs: uses-type / inherits / uses-member / imports candidates with
//     line + enclosing-namespace context.
//
// Def/ref extraction NEVER emits a byte offset or a column -- only the
// node's start row + 1 (a 1-based line number). The offset table exists to
// translate UTF-8-byte offsets/columns to UTF-16 code units; rows are
// unaffected by that translation, so nothing here needs OffsetTable -- an
// absence, not a gap.
//
// Split by concern, C# side first: `text` (tree-sitter/text primitives
// shared with the TypeScript-family extraction below), `types` (the C# data
// model -- DefRecord/RefRecord/UsingRecord/Extraction/NameRecord/Fact/
// LambdaSlot/PublishRecord), `refs` (building RefRecord candidates plus the
// type/generic descriptor primitives that feed them), `members` (raw
// per-member fact extraction: methods/properties/fields/bases/extension
// methods/test methods), `type_defs` (assembling DefRecord/NameRecord/
// UsingRecord instances from those raw facts), `bus` (message-bus
// publish-site facts and the nested base type-argument fact a generic
// consumer base needs), `delegate_args` (the parameter count a
// lambda-literal or local-function call argument brings to its call),
// `receivers` (the local/field fact tables a member-access qualifier
// resolves against), `lambdas` (untyped lambda-parameter slot typing),
// `qualifiers` (`Scope` and member-access qualifier resolution), `walk` (the
// top-level recursive-descent AST walk and its `extract` entry point),
// `dump` (the `extract-dump` subcommand).
// `json` holds the hand-rolled JSON serialisation for both the C# and the
// TS-family fragment shapes. `ts_purpose`, `ts_fragment_types` and
// `ts_fragment` hold the TypeScript-family purpose and reference-fact
// extraction.
mod bus;
mod bus_vocab;
mod delegate_args;
mod dump;
mod json;
mod lambdas;
mod members;
mod qualifiers;
mod receivers;
mod refs;
mod text;
mod ts_fragment;
mod ts_fragment_types;
mod ts_purpose;
mod type_defs;
mod types;
mod walk;

pub use dump::run_extract_dump;
pub use json::{extraction_to_json, ts_extraction_to_json};
pub use ts_fragment::{extract_ts_file, extract_ts_fragment, TsFileExtraction};
pub use ts_fragment_types::{
    TsBinding, TsFragment, TsFragmentDef, TsImport, TsReexport, TsReexportName, TsRef,
};
pub use ts_purpose::{
    compose_hybrid_ts_purpose, extract_ts_purpose, extract_ts_purpose_with_heuristic,
};
pub use types::{
    DefRecord, EnclosingCallFact, ExtensionMethod, Extraction, Fact, HandlerRegistrationRecord,
    LambdaSlot, NameRecord, PublishRecord, RefRecord, RegistrationRecord, UsingRecord,
};
pub use walk::extract;

#[cfg(test)]
mod tests;
