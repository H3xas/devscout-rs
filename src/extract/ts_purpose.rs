use std::collections::{HashMap, HashSet};
use std::path::Path;

use tree_sitter::Node;

use super::text::{named_children, text, truncate};

// ---------------------------------------------------------------------------
// TS/JS purpose extraction. PURPOSES ONLY:
// this section never builds a def/ref/usings candidate, and nothing here is
// reachable from `extract()`/`walk()` above (the C# graph-fragment walk).
// Entry point is `extract_ts_purpose`, called from mapcmd.rs's per-file
// dispatch for any `.ts`/`.tsx`/`.js`/`.jsx` path (parse.rs's `ts_grammar_for`
// gate) and from `run_extract_dump` above for the extract-dump harness.
// ---------------------------------------------------------------------------

// One exported top-level declaration's contribution to the purpose line, as
// a `{kind, name, bases, methods}` record. `kind` already carries the literal
// string `"default"` when this entry wraps a default export (the kind-word is
// replaced by the literal `default`), so `compose_ts_purpose` never branches
// on default-ness
// itself -- it just prints `kind` as the header word, exactly like the
// bucket-sort below groups on it.
struct TsEntry {
    kind: &'static str,
    name: String,
    bases: String,
    methods: Vec<String>,
}

// A method/property `name` field is absent (or a `computed_property_name`,
// e.g. `[Symbol.iterator]() {}`) exactly when there is nothing to report --
// no semantic resolution of computed names, same zero-guessing stance as
// `outer_type_name` above.
pub(super) fn ts_declared_name(node: Node, src: &[u8]) -> String {
    match node.child_by_field_name("name") {
        Some(n) if n.kind() != "computed_property_name" => text(n, src),
        _ => String::new(),
    }
}

fn ts_accessibility(node: Node, src: &[u8]) -> Option<String> {
    named_children(node)
        .into_iter()
        .find(|c| c.kind() == "accessibility_modifier")
        .map(|m| text(m, src))
}

// No accessibility_modifier, or an explicit public one -- an absent modifier
// defaults to public in TS, the INVERSE of C#'s implicit-internal-unless-
// public default; the two languages differ here on purpose and this follows
// TS's own default, not C#'s.
fn ts_member_is_public(node: Node, src: &[u8]) -> bool {
    match ts_accessibility(node, src) {
        None => true,
        Some(a) => a == "public",
    }
}

fn class_bases(node: Node, src: &[u8]) -> String {
    let Some(heritage) = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "class_heritage")
    else {
        return String::new();
    };
    let mut parts: Vec<String> = Vec::new();
    if let Some(extends_clause) = named_children(heritage)
        .into_iter()
        .find(|c| c.kind() == "extends_clause")
    {
        if let Some(value) = extends_clause.child_by_field_name("value") {
            parts.push(text(value, src));
        }
    }
    if let Some(implements_clause) = named_children(heritage)
        .into_iter()
        .find(|c| c.kind() == "implements_clause")
    {
        for t in named_children(implements_clause) {
            parts.push(text(t, src));
        }
    }
    parts.join(", ")
}

fn class_method_names(node: Node, src: &[u8]) -> Vec<String> {
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    named_children(body)
        .into_iter()
        .filter(|c| c.kind() == "method_definition" && ts_member_is_public(*c, src))
        .map(|c| ts_declared_name(c, src))
        .filter(|name| !name.is_empty() && name != "constructor")
        .collect()
}

fn interface_bases(node: Node, src: &[u8]) -> String {
    let Some(ext) = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "extends_type_clause")
    else {
        return String::new();
    };
    named_children(ext)
        .into_iter()
        .map(|c| text(c, src))
        .collect::<Vec<_>>()
        .join(", ")
}

fn interface_method_names(node: Node, src: &[u8]) -> Vec<String> {
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    named_children(body)
        .into_iter()
        .filter(|c| c.kind() == "method_signature")
        .map(|c| ts_declared_name(c, src))
        .filter(|n| !n.is_empty())
        .collect()
}

// Collects bound identifiers out of a destructuring pattern in
// pattern-written order, never resolving the RHS of
// `export const { a, b } = X;`. Handles the object/array pattern shapes
// tree-sitter-typescript actually produces; a shape this doesn't recognise
// yields no names rather than a guess.
pub(super) fn ts_pattern_names(node: Option<Node>, src: &[u8]) -> Vec<String> {
    let Some(node) = node else {
        return Vec::new();
    };
    match node.kind() {
        "identifier" => vec![text(node, src)],
        "object_pattern" => {
            let mut names = Vec::new();
            for c in named_children(node) {
                match c.kind() {
                    "shorthand_property_identifier_pattern" => names.push(text(c, src)),
                    "pair_pattern" => {
                        names.extend(ts_pattern_names(c.child_by_field_name("value"), src))
                    }
                    "rest_pattern" => {
                        names.extend(ts_pattern_names(named_children(c).into_iter().next(), src))
                    }
                    "object_assignment_pattern" => {
                        names.extend(ts_pattern_names(c.child_by_field_name("left"), src))
                    }
                    _ => {}
                }
            }
            names
        }
        "array_pattern" => named_children(node)
            .into_iter()
            .flat_map(|c| ts_pattern_names(Some(c), src))
            .collect(),
        "rest_pattern" => ts_pattern_names(named_children(node).into_iter().next(), src),
        _ => Vec::new(),
    }
}

// `export default <expr>;` parses as an export_statement with a `value`
// field and no `declaration` field. The only shapes that resolve to a real
// name are a bare identifier (`export default Identifier;`) -- literal
// token text, no semantic resolution -- everything else (arrow function,
// anonymous function/class expression, member/call expression, object/array
// literal) is the literal name "(anonymous)".
fn ts_default_expression_entry(value_node: Node, src: &[u8]) -> TsEntry {
    let name = if value_node.kind() == "identifier" {
        text(value_node, src)
    } else {
        "(anonymous)".to_string()
    };
    TsEntry {
        kind: "default",
        name,
        bases: String::new(),
        methods: Vec::new(),
    }
}

fn ts_entries_for_declaration(decl_node: Node, is_default: bool, src: &[u8]) -> Vec<TsEntry> {
    match decl_node.kind() {
        "class_declaration" => {
            let name = ts_declared_name(decl_node, src);
            if name.is_empty() {
                Vec::new()
            } else {
                vec![TsEntry {
                    kind: if is_default { "default" } else { "class" },
                    name,
                    bases: class_bases(decl_node, src),
                    methods: class_method_names(decl_node, src),
                }]
            }
        }
        "function_declaration" => {
            let name = ts_declared_name(decl_node, src);
            if name.is_empty() {
                Vec::new()
            } else {
                vec![TsEntry {
                    kind: if is_default { "default" } else { "function" },
                    name,
                    bases: String::new(),
                    methods: Vec::new(),
                }]
            }
        }
        "interface_declaration" => {
            let name = ts_declared_name(decl_node, src);
            if name.is_empty() {
                Vec::new()
            } else {
                vec![TsEntry {
                    kind: "interface",
                    name,
                    bases: interface_bases(decl_node, src),
                    methods: interface_method_names(decl_node, src),
                }]
            }
        }
        "type_alias_declaration" => {
            let name = ts_declared_name(decl_node, src);
            if name.is_empty() {
                Vec::new()
            } else {
                vec![TsEntry {
                    kind: "type",
                    name,
                    bases: String::new(),
                    methods: Vec::new(),
                }]
            }
        }
        "enum_declaration" => {
            let name = ts_declared_name(decl_node, src);
            if name.is_empty() {
                Vec::new()
            } else {
                vec![TsEntry {
                    kind: "enum",
                    name,
                    bases: String::new(),
                    methods: Vec::new(),
                }]
            }
        }
        // `export default const x = 1;` is not legal syntax, so a
        // lexical/var declaration is never the wrapped declaration of a
        // default export -- `is_default` is not consulted here.
        "lexical_declaration" | "variable_declaration" => {
            let mut entries = Vec::new();
            for decl in named_children(decl_node) {
                if decl.kind() != "variable_declarator" {
                    continue;
                }
                for name in ts_pattern_names(decl.child_by_field_name("name"), src) {
                    entries.push(TsEntry {
                        kind: "const",
                        name,
                        bases: String::new(),
                        methods: Vec::new(),
                    });
                }
            }
            entries
        }
        _ => Vec::new(),
    }
}

// The `default` keyword in `export default ...` is an unnamed token (no
// field name), so it has to be found by scanning ALL raw children (not just
// named ones) rather than `child_by_field_name`.
pub(super) fn is_default_export_statement(node: Node) -> bool {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).any(|c| c.kind() == "default");
    found
}

fn ts_esm_entries(export_stmts: &[Node], src: &[u8]) -> Vec<TsEntry> {
    let mut entries = Vec::new();
    for stmt in export_stmts {
        if let Some(decl_node) = stmt.child_by_field_name("declaration") {
            entries.extend(ts_entries_for_declaration(
                decl_node,
                is_default_export_statement(*stmt),
                src,
            ));
            continue;
        }
        if let Some(value_node) = stmt.child_by_field_name("value") {
            entries.push(ts_default_expression_entry(value_node, src));
        }
        // Anything else (`export { a, b };` with no `from` clause) has no
        // declaration and no value field, so it is skipped rather than
        // guessed at. Re-exports (`export * from './x';`) are collected
        // separately, by `ts_reexport_names`.
    }
    entries
}

pub(super) enum CjsTarget {
    Whole,
    Prop(String),
}

// `module.exports = X` / `module.exports.foo = X` / `exports.foo = X` --
// `Whole` for the first shape, `Prop(name)` for the other two, or `None`
// when `left` isn't one of these three recognised shapes.
pub(super) fn common_js_export_target(left: Option<Node>, src: &[u8]) -> Option<CjsTarget> {
    let left = left?;
    if left.kind() != "member_expression" {
        return None;
    }
    let obj = left.child_by_field_name("object")?;
    let prop = left.child_by_field_name("property")?;
    if obj.kind() == "identifier" && text(obj, src) == "module" && text(prop, src) == "exports" {
        return Some(CjsTarget::Whole);
    }
    if obj.kind() == "identifier" && text(obj, src) == "exports" {
        return Some(CjsTarget::Prop(text(prop, src)));
    }
    if obj.kind() == "member_expression" {
        let outer_obj = obj.child_by_field_name("object");
        let outer_prop = obj.child_by_field_name("property");
        if let (Some(oo), Some(op)) = (outer_obj, outer_prop) {
            if oo.kind() == "identifier" && text(oo, src) == "module" && text(op, src) == "exports"
            {
                return Some(CjsTarget::Prop(text(prop, src)));
            }
        }
    }
    None
}

struct LocalKind {
    kind: &'static str,
    bases: String,
    methods: Vec<String>,
}

// A CommonJS export-by-name mirrors the referenced top-level local
// declaration if one exists by that name, else a bare `const Name` -- this is
// the lookup table for that mirroring, built once per file from the plain
// (non-exported, since CommonJS files carry no `export` keyword at all)
// top-level declarations.
fn ts_local_declaration_kinds(program_node: Node, src: &[u8]) -> HashMap<String, LocalKind> {
    let mut kinds = HashMap::new();
    for c in named_children(program_node) {
        match c.kind() {
            "function_declaration" => {
                let name = ts_declared_name(c, src);
                if !name.is_empty() {
                    kinds.insert(
                        name,
                        LocalKind {
                            kind: "function",
                            bases: String::new(),
                            methods: Vec::new(),
                        },
                    );
                }
            }
            "class_declaration" => {
                let name = ts_declared_name(c, src);
                if !name.is_empty() {
                    kinds.insert(
                        name,
                        LocalKind {
                            kind: "class",
                            bases: class_bases(c, src),
                            methods: class_method_names(c, src),
                        },
                    );
                }
            }
            "lexical_declaration" | "variable_declaration" => {
                for decl in named_children(c) {
                    if decl.kind() != "variable_declarator" {
                        continue;
                    }
                    if let Some(name_node) = decl.child_by_field_name("name") {
                        if name_node.kind() == "identifier" {
                            kinds.insert(
                                text(name_node, src),
                                LocalKind {
                                    kind: "const",
                                    bases: String::new(),
                                    methods: Vec::new(),
                                },
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }
    kinds
}

fn property_export_entry(name: String, local_kinds: &HashMap<String, LocalKind>) -> TsEntry {
    match local_kinds.get(&name) {
        Some(local) => TsEntry {
            kind: local.kind,
            name,
            bases: local.bases.clone(),
            methods: local.methods.clone(),
        },
        None => TsEntry {
            kind: "const",
            name,
            bases: String::new(),
            methods: Vec::new(),
        },
    }
}

fn object_literal_export_entries(
    obj_node: Node,
    local_kinds: &HashMap<String, LocalKind>,
    src: &[u8],
) -> Vec<TsEntry> {
    let mut entries = Vec::new();
    for c in named_children(obj_node) {
        match c.kind() {
            "shorthand_property_identifier" => {
                entries.push(property_export_entry(text(c, src), local_kinds))
            }
            "pair" => {
                let Some(key) = c.child_by_field_name("key") else {
                    continue;
                };
                let name = if key.kind() == "string" {
                    named_children(key).into_iter().next().map(|n| text(n, src))
                } else {
                    Some(text(key, src))
                };
                if let Some(name) = name {
                    entries.push(property_export_entry(name, local_kinds));
                }
            }
            _ => {}
        }
    }
    entries
}

fn ts_cjs_entries(program_node: Node, src: &[u8]) -> Vec<TsEntry> {
    let local_kinds = ts_local_declaration_kinds(program_node, src);
    let mut entries = Vec::new();
    for stmt in named_children(program_node) {
        if stmt.kind() != "expression_statement" {
            continue;
        }
        let Some(assign) = named_children(stmt)
            .into_iter()
            .find(|n| n.kind() == "assignment_expression")
        else {
            continue;
        };
        let Some(target) = common_js_export_target(assign.child_by_field_name("left"), src) else {
            continue;
        };
        let right = assign.child_by_field_name("right");
        match target {
            CjsTarget::Whole => match right {
                Some(r) if r.kind() == "object" => {
                    entries.extend(object_literal_export_entries(r, &local_kinds, src))
                }
                Some(r) => entries.push(ts_default_expression_entry(r, src)),
                // An assignment_expression with no `right` field is not
                // producible by valid JS syntax -- never guess, skip.
                None => {}
            },
            CjsTarget::Prop(name) => entries.push(property_export_entry(name, &local_kinds)),
        }
    }
    entries
}

// A file with at least one ESM export_statement is treated as ESM (its
// CommonJS-shaped statements, if any, are never scanned); a file with zero
// export_statements falls to the CommonJS path: a file with no `export`
// keyword at all is treated as CommonJS.
fn ts_export_entries(program_node: Node, src: &[u8]) -> Vec<TsEntry> {
    let export_stmts: Vec<Node> = named_children(program_node)
        .into_iter()
        .filter(|c| c.kind() == "export_statement")
        .collect();
    if export_stmts.is_empty() {
        ts_cjs_entries(program_node, src)
    } else {
        ts_esm_entries(&export_stmts, src)
    }
}

// Re-export ("barrel") bucket: `export { A, B } from 'm'`,
// `export * from 'm'`, `export * as NS from 'm'`. Collected as plain name
// tokens, in source order across every re-export statement in the file,
// deduped by a single set, like every other bucket, just scoped to this
// bucket rather than the file-wide seen_methods set.
// `export { A as B } from 'm'` contributes the EXPORTED (alias) name B, not
// the local name A -- what this barrel's own consumers actually import. A
// bare `export * from 'm'` contributes the literal token "*"; the same dedupe
// collapses it to one occurrence regardless of how many bare `export *`
// statements the file has. Local `export { A };` (no `from` clause -- no
// `source` field) is skipped by ts_esm_entries above: it has neither a
// `declaration` nor a `value` field.
fn push_reexport_name(name: String, names: &mut Vec<String>, seen: &mut HashSet<String>) {
    if seen.insert(name.clone()) {
        names.push(name);
    }
}

fn ts_reexport_names(program_node: Node, src: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for stmt in named_children(program_node) {
        if stmt.kind() != "export_statement" {
            continue;
        }
        if stmt.child_by_field_name("source").is_none() {
            continue; // not a re-export (local `export { A };`, or a declaration/default export)
        }
        if let Some(ns_export) = named_children(stmt)
            .into_iter()
            .find(|c| c.kind() == "namespace_export")
        {
            if let Some(id) = named_children(ns_export)
                .into_iter()
                .find(|c| c.kind() == "identifier")
            {
                push_reexport_name(text(id, src), &mut names, &mut seen);
            }
            continue;
        }
        if let Some(clause) = named_children(stmt)
            .into_iter()
            .find(|c| c.kind() == "export_clause")
        {
            for spec in named_children(clause) {
                if spec.kind() != "export_specifier" {
                    continue;
                }
                let alias = spec.child_by_field_name("alias").map(|n| text(n, src));
                let name = spec.child_by_field_name("name").map(|n| text(n, src));
                if let Some(chosen) = alias.or(name) {
                    push_reexport_name(chosen, &mut names, &mut seen);
                }
            }
            continue;
        }
        // bare `export * from 'm';` -- no export_clause, no namespace_export
        push_reexport_name("*".to_string(), &mut names, &mut seen);
    }
    names
}

fn reexports_bucket_segment(program_node: Node, src: &[u8]) -> Option<String> {
    let names = ts_reexport_names(program_node, src);
    if names.is_empty() {
        None
    } else {
        Some(format!("reexports {}", names.join(", ")))
    }
}

const TS_BUCKET_ORDER: &[&str] = &[
    "class",
    "function",
    "const",
    "interface",
    "type",
    "enum",
    "default",
];

// Raw (pre-truncation) purpose segments, joined with " | " -- the SAME
// join `compose_ts_purpose` truncates, exposed separately so the hybrid
// comment-prefix composition (`extract_ts_purpose_with_heuristic` below) can
// apply the SAME global `truncate()` to `<comment> — <segments>` as a whole,
// rather than truncating twice. Returns `None` exactly when there is nothing
// to report at all -- no exported declarations AND no re-export statements --
// a barrel-only file contributes its reexports bucket.
pub(super) fn ts_purpose_segments(program_node: Node, src: &[u8]) -> Option<String> {
    let entries = ts_export_entries(program_node, src);
    let mut sorted: Vec<&TsEntry> = Vec::with_capacity(entries.len());
    for kind in TS_BUCKET_ORDER {
        sorted.extend(entries.iter().filter(|e| e.kind == *kind));
    }
    let reexports_segment = reexports_bucket_segment(program_node, src);
    if sorted.is_empty() && reexports_segment.is_none() {
        return None;
    }
    // Same single shared Set, in bucket-emission order, as C#'s
    // compose_signature -- inherited as-is, known consequence and all (a
    // class implementing an interface earlier in bucket order usually
    // empties the interface's own member list, since the class already
    // claimed those names; see the OrderService.ts fixture).
    let mut seen_methods: HashSet<String> = HashSet::new();
    let mut segments: Vec<String> = Vec::new();
    for e in sorted {
        let mut methods = Vec::new();
        for m in &e.methods {
            if seen_methods.contains(m) {
                continue;
            }
            seen_methods.insert(m.clone());
            methods.push(m.clone());
        }
        let header = if e.bases.is_empty() {
            format!("{} {}", e.kind, e.name)
        } else {
            format!("{} {} : {}", e.kind, e.name, e.bases)
        };
        segments.push(if methods.is_empty() {
            header
        } else {
            format!("{header}; {}", methods.join(", "))
        });
    }
    // Reexports is the final bucket, after default -- appended
    // once, never bucket-sorted alongside the TsEntry-derived segments above
    // since it is a single pre-joined string, not a per-entry segment.
    if let Some(seg) = reexports_segment {
        segments.push(seg);
    }
    Some(segments.join(" | "))
}

fn compose_ts_purpose(program_node: Node, src: &[u8]) -> Option<String> {
    ts_purpose_segments(program_node, src).map(|s| truncate(&s))
}

/// Compose a leading-comment hybrid purpose prefix. `comment_text` is
/// `walk::default_purpose_detailed`'s output (already ≤100 chars); the
/// SAME global `truncate()` every other purpose bucket uses applies to the
/// combined string over the RAW (pre-truncation) segment join, so the AST
/// tail fills the remainder under the 200-char cap instead of compounding
/// two independent truncations.
pub fn compose_hybrid_ts_purpose(comment_text: &str, raw_segments: &str) -> String {
    truncate(&format!("{comment_text} — {raw_segments}"))
}

/// Extract a one-line purpose signature from TS/JS source, or `None` when the
/// file exports nothing or fails to parse (in which case the caller falls back
/// to the heuristic purpose). `grammar` comes from `parse::ts_grammar_for`.
/// PURE -- no heuristic or disk access; see `extract_ts_purpose_with_heuristic`
/// below for the hybrid-aware entry point that `mapcmd.rs` and
/// `run_extract_dump` actually use.
pub fn extract_ts_purpose(source: &str, grammar: crate::parse::TsGrammar) -> Option<String> {
    let units = crate::parse::utf16_units(source);
    let tree = crate::parse::parse_ts_js(&units, grammar)?;
    compose_ts_purpose(tree.root_node(), &crate::parse::utf16_bytes(&units))
}

/// Hybrid-aware entry point used by `mapcmd.rs`'s per-file dispatch. Returns
/// `None` exactly when `extract_ts_purpose` would (zero-export or parse
/// failure) -- a zero-export file has no purpose to prefix. When there IS a
/// purpose, `root`/`rel` are used ONLY to ask `walk::default_purpose_detailed`
/// whether the heuristic's match for this file came from the comment-marker
/// branch; if so, the comment text is prefixed via `compose_hybrid_ts_purpose`
/// before the final truncate, else the plain (unprefixed) purpose is returned
/// exactly as `extract_ts_purpose` would produce it.
pub fn extract_ts_purpose_with_heuristic(
    root: &Path,
    rel: &str,
    source: &str,
    grammar: crate::parse::TsGrammar,
) -> Option<String> {
    let units = crate::parse::utf16_units(source);
    let tree = crate::parse::parse_ts_js(&units, grammar)?;
    let raw = ts_purpose_segments(tree.root_node(), &crate::parse::utf16_bytes(&units))?;
    let detail = crate::walk::default_purpose_detailed(root, rel);
    Some(if detail.is_comment {
        compose_hybrid_ts_purpose(&detail.text, &raw)
    } else {
        truncate(&raw)
    })
}
