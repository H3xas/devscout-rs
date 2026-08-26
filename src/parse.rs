// tree-sitter runtime + C# and TS/JS grammars behind one Language seam. Full
// extraction is extract.rs; this module owns parser access, the seed `parse`
// text dump, and the `spans` diagnostic subcommand.

use std::fs;
use std::process;

use tree_sitter::{Node, Parser};

const TYPE_KINDS: &[&str] = &[
    "class_declaration",
    "interface_declaration",
    "struct_declaration",
    "enum_declaration",
    "record_declaration",
];

fn new_parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
        .expect("failed to load C# grammar");
    parser
}

// ---------------------------------------------------------------------------
// Encoding seam -- every parse in this crate feeds the grammar UTF-16.
//
// This is not cosmetic. tree-sitter's bounded error-recovery search scores
// candidate repairs partly by SKIPPED BYTES of the input buffer
// (ERROR_COST_PER_SKIPPED_CHAR), so the same grammar on the same source picks
// DIFFERENT repairs depending on whether that source arrived as UTF-8 (one
// byte per ASCII char) or UTF-16 (two). Fixing the buffer encoding at UTF-16
// makes error recovery deterministic across inputs -- for example a vendored
// file whose `#if/#else` convention duplicates a member header across both arms
// and shares one body could otherwise be repaired two ways, one of which nests
// the second declaration as a local function inside the first arm's still-open
// block, collapsing two member scopes into one and cancelling every receiver
// fact in them.
//
// Feeding a UTF-16 view removes that whole divergence class at its root instead
// of compensating for individual damaged shapes. It also makes node spans
// natively UTF-16 -- the convention every artifact this crate emits already
// uses -- so span translation is a halving, not a table lookup: tree-sitter
// reports BYTE offsets into the buffer it parsed, and a UTF-16 buffer has
// exactly two bytes per code unit, astral pairs included.
/// Encodes source text as UTF-16 code units.
pub fn utf16_units(source: &str) -> Vec<u16> {
    source.encode_utf16().collect()
}

/// The same units as a little-endian byte buffer, which is what node byte
/// offsets index into and therefore what every `text()` helper slices.
pub fn utf16_bytes(units: &[u16]) -> Vec<u8> {
    units.iter().flat_map(|u| u.to_le_bytes()).collect()
}

/// Text of `node` out of a `utf16_bytes` buffer -- the UTF-16 counterpart of
/// `Node::utf8_text`, and the only way to read source text in this crate.
pub fn node_text(node: Node, src: &[u8]) -> String {
    let range = node.byte_range();
    if range.start > range.end || range.end > src.len() {
        return String::new();
    }
    let units: Vec<u16> = src[range]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

/// UTF-16 code-unit index for a byte offset (or column) reported by a tree
/// parsed out of a `utf16_bytes` buffer.
pub fn utf16_index(byte: usize) -> usize {
    byte / 2
}

// ---------------------------------------------------------------------------
// `parse` subcommand -- seed text dump.
// ---------------------------------------------------------------------------

/// Parses the C# file at `path` and prints its syntax tree.
pub fn run_parse(path: &str) {
    let source = fs::read_to_string(path).unwrap_or_else(|err| {
        eprintln!("failed to read {path}: {err}");
        process::exit(1);
    });

    let mut parser = new_parser();
    let units = utf16_units(&source);
    let tree = parser
        .parse_utf16_le(&units, None)
        .expect("parse returned no tree");
    walk(tree.root_node(), &utf16_bytes(&units), 0);
}

fn walk(node: Node, src: &[u8], depth: usize) {
    if TYPE_KINDS.contains(&node.kind()) {
        print_type_decl(node, src, depth);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, depth + 1);
    }
}

fn print_type_decl(node: Node, src: &[u8], depth: usize) {
    let indent = "  ".repeat(depth.saturating_sub(1));
    let kind = node.kind();
    let name = node
        .child_by_field_name("name")
        .map(|n| text(n, src))
        .unwrap_or_else(|| "<anonymous>".to_string());
    let bases = base_list_text(node, src);

    println!("{indent}{kind} {name} : [{bases}]");

    if kind == "enum_declaration" {
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for member in body.named_children(&mut cursor) {
                if member.kind() == "enum_member_declaration" {
                    if let Some(mname) = member.child_by_field_name("name") {
                        println!("{indent}  enum member: {}", text(mname, src));
                    }
                }
            }
        }
    }
}

fn base_list_text(node: Node, src: &[u8]) -> String {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "base_list" {
            let mut names = Vec::new();
            let mut bl_cursor = child.walk();
            for base_child in child.named_children(&mut bl_cursor) {
                if base_child.kind() != "argument_list" {
                    names.push(text(base_child, src));
                }
            }
            return names.join(", ");
        }
    }
    String::new()
}

fn text(node: Node, src: &[u8]) -> String {
    node_text(node, src)
}

// ---------------------------------------------------------------------------
// `spans` subcommand -- span diagnostics.
// ---------------------------------------------------------------------------

// Same node kinds as `TYPE_KINDS` plus enum members -- the `spans` scope of
// "every type declaration + enum-member node".
const SPAN_KINDS: &[&str] = &[
    "class_declaration",
    "interface_declaration",
    "struct_declaration",
    "record_declaration",
    "enum_declaration",
    "enum_member_declaration",
];

/// Represents `SpanRecord`.
pub struct SpanRecord {
    /// The kind value.
    pub kind: String,
    /// The name value.
    pub name: String,
    /// The start byte value.
    pub start_byte: usize,
    /// The end byte value.
    pub end_byte: usize,
    /// The start row value.
    pub start_row: usize,
    /// The start col value.
    pub start_col: usize,
    /// The end row value.
    pub end_row: usize,
    /// The end col value.
    pub end_col: usize,
}

/// Collects declaration spans from the C# file at `path` and prints them as JSON.
pub fn run_spans(path: &str) {
    let source = fs::read_to_string(path).unwrap_or_else(|err| {
        eprintln!("failed to read {path}: {err}");
        process::exit(1);
    });
    let records = collect_spans(&source);
    println!("{}", spans_json(&records));
}

/// Parses C# source and returns its namespace, type, and member declaration spans.
pub fn collect_spans(source: &str) -> Vec<SpanRecord> {
    let mut parser = new_parser();
    let units = utf16_units(source);
    let tree = parser
        .parse_utf16_le(&units, None)
        .expect("parse returned no tree");
    let mut out = Vec::new();
    walk_spans(tree.root_node(), &utf16_bytes(&units), &mut out);
    out.sort_by(|a, b| {
        a.start_byte
            .cmp(&b.start_byte)
            .then(a.end_byte.cmp(&b.end_byte))
            .then(a.kind.cmp(&b.kind))
            .then(a.name.cmp(&b.name))
    });
    out
}

fn walk_spans(node: Node, src: &[u8], out: &mut Vec<SpanRecord>) {
    if SPAN_KINDS.contains(&node.kind()) {
        if let Some(name_node) = node.child_by_field_name("name") {
            let start_point = node.start_position();
            let end_point = node.end_position();
            let (start_byte, start_col) = (
                utf16_index(node.start_byte()),
                utf16_index(start_point.column),
            );
            let (end_byte, end_col) = (utf16_index(node.end_byte()), utf16_index(end_point.column));
            out.push(SpanRecord {
                kind: node.kind().to_string(),
                name: text(name_node, src),
                start_byte,
                end_byte,
                start_row: start_point.row,
                start_col,
                end_row: end_point.row,
                end_col,
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk_spans(child, src, out);
    }
}

// Hand-rolled 2-space-indented JSON so the output format is fixed and does not
// depend on a serializer's formatting choices.
/// Serializes span records as a JSON array.
pub fn spans_json(records: &[SpanRecord]) -> String {
    if records.is_empty() {
        return "[]".to_string();
    }
    let mut out = String::from("[\n");
    let last = records.len() - 1;
    for (i, r) in records.iter().enumerate() {
        out.push_str("  {\n");
        out.push_str(&format!("    \"kind\": {},\n", json_string(&r.kind)));
        out.push_str(&format!("    \"name\": {},\n", json_string(&r.name)));
        out.push_str(&format!("    \"start_byte\": {},\n", r.start_byte));
        out.push_str(&format!("    \"end_byte\": {},\n", r.end_byte));
        out.push_str(&format!("    \"start_row\": {},\n", r.start_row));
        out.push_str(&format!("    \"start_col\": {},\n", r.start_col));
        out.push_str(&format!("    \"end_row\": {},\n", r.end_row));
        out.push_str(&format!("    \"end_col\": {}\n", r.end_col));
        out.push_str(if i == last { "  }\n" } else { "  },\n" });
    }
    out.push(']');
    out
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------
// TS/JS grammar seam. Second grammar set alongside C#, PURPOSES ONLY. No graph
// fragment ever comes out of this seam; see extract.rs's `extract_ts_purpose`
// and mapcmd.rs's per-file dispatch for where that boundary is enforced. The
// per-extension grammar pin is deliberate: `.ts` gets the dedicated typescript
// grammar (the tsx grammar misparses a legal `.ts` cast-syntax expression),
// `.tsx` gets the dedicated tsx grammar (plain typescript can't parse real
// JSX), `.js`/`.jsx` both get the dedicated javascript grammar (kept off
// tsx-as-superset so TS-only syntax landing in a `.js` file still surfaces as
// an ERROR node).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Represents `TsGrammar`.
pub enum TsGrammar {
    /// Represents `Typescript`.
    Typescript,
    /// Represents `Tsx`.
    Tsx,
    /// Represents `Javascript`.
    Javascript,
}

/// Maps a file extension to its grammar. Based on the last `.` in `rel`, not
/// path-aware: a dotted directory segment on a file with no extension of its
/// own would misfire (a known, accepted edge case).
pub fn ts_grammar_for(rel: &str) -> Option<TsGrammar> {
    let dot = rel.rfind('.')?;
    match &rel[dot..] {
        ".ts" => Some(TsGrammar::Typescript),
        ".tsx" => Some(TsGrammar::Tsx),
        ".js" | ".jsx" => Some(TsGrammar::Javascript),
        _ => None,
    }
}

/// Whether `rel` is a TS/JS file -- `ts_grammar_for(rel).is_some()`.
pub fn is_ts_js(rel: &str) -> bool {
    ts_grammar_for(rel).is_some()
}

fn new_ts_parser(grammar: TsGrammar) -> Parser {
    let mut parser = Parser::new();
    let language = match grammar {
        TsGrammar::Typescript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        TsGrammar::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        TsGrammar::Javascript => tree_sitter_javascript::LANGUAGE.into(),
    };
    parser
        .set_language(&language)
        .expect("failed to load TS/JS grammar");
    parser
}

/// One parse of `source` under `grammar`. `Parser::parse` returns `None` only
/// on cancellation/malformed-input-size edge cases, never on ordinary syntax
/// errors (those surface as ERROR nodes inside a still-present tree).
pub fn parse_ts_js(units: &[u16], grammar: TsGrammar) -> Option<tree_sitter::Tree> {
    new_ts_parser(grammar).parse_utf16_le(units, None)
}

#[cfg(test)]
mod ts_grammar_tests {
    use super::*;

    #[test]
    fn extension_maps_to_the_rt1_pinned_grammar() {
        assert_eq!(ts_grammar_for("a/b.ts"), Some(TsGrammar::Typescript));
        assert_eq!(ts_grammar_for("a/b.tsx"), Some(TsGrammar::Tsx));
        assert_eq!(ts_grammar_for("a/b.js"), Some(TsGrammar::Javascript));
        assert_eq!(ts_grammar_for("a/b.jsx"), Some(TsGrammar::Javascript));
        assert_eq!(ts_grammar_for("a/b.cs"), None);
        assert_eq!(ts_grammar_for("a/b"), None);
    }

    #[test]
    fn is_ts_js_true_only_for_the_four_extensions() {
        assert!(is_ts_js("x.ts"));
        assert!(is_ts_js("x.tsx"));
        assert!(is_ts_js("x.js"));
        assert!(is_ts_js("x.jsx"));
        assert!(!is_ts_js("x.cs"));
        assert!(!is_ts_js("x.json"));
    }

    #[test]
    fn each_grammar_loads_and_parses_without_panicking() {
        assert!(parse_ts_js(
            &utf16_units("const x: number = 1;\n"),
            TsGrammar::Typescript
        )
        .is_some());
        assert!(parse_ts_js(&utf16_units("const x = <div>hi</div>;\n"), TsGrammar::Tsx).is_some());
        assert!(parse_ts_js(
            &utf16_units("function f() { return 1; }\n"),
            TsGrammar::Javascript
        )
        .is_some());
    }
}
