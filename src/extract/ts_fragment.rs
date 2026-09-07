use std::collections::{HashMap, HashSet};
use std::path::Path;

use tree_sitter::Node;

use super::text::{named_children, text, truncate};
use super::ts_fragment_types::{
    TsBinding, TsFragment, TsFragmentDef, TsImport, TsReexport, TsReexportName, TsRef,
};
use super::ts_purpose::{
    common_js_export_target, compose_hybrid_ts_purpose, is_default_export_statement,
    ts_declared_name, ts_pattern_names, ts_purpose_segments, CjsTarget,
};

// The dispatching call names, framework-specific BY NAME on purpose.
// `dispatch(x)` covers redux
// and NgRx `Store.dispatch`; `ofType(x)` is how an NgRx effect names the
// actions it reacts to. A member call ending in `.dispatch(...)` counts too,
// by the property name alone -- the receiver is never resolved, so this can
// never claim more than the call site literally spells.
const TS_DISPATCH_CALLEES: &[&str] = &["dispatch", "ofType"];

// The four node kinds a reference can be spelled as. The traversal below is
// cursor-driven and materialises a `Node` ONLY for these.
const TS_REFERENCE_NODES: &[&str] = &[
    "call_expression",
    "new_expression",
    "jsx_opening_element",
    "jsx_self_closing_element",
];

// `None` for anything that is not a `string` node, and the empty string for
// a string with no `string_fragment` child.
fn ts_string_literal(node: Option<Node>, src: &[u8]) -> Option<String> {
    let node = node?;
    if node.kind() != "string" {
        return None;
    }
    Some(
        match named_children(node)
            .into_iter()
            .find(|c| c.kind() == "string_fragment")
        {
            Some(frag) => text(frag, src),
            None => String::new(),
        },
    )
}

fn ts_line(node: Node) -> usize {
    node.start_position().row + 1
}

// A JSX tag names a component (rather than an intrinsic HTML element) exactly
// when its first character is an ASCII uppercase letter -- React's own rule,
// applied literally rather than guessed at. ASCII-only, so the classification
// of a non-ASCII first character is stable (never a component).
fn is_component_tag_name(name: &str) -> bool {
    matches!(name.chars().next(), Some(c) if c.is_ascii_uppercase())
}

fn ts_import_bindings(stmt: Node, src: &[u8]) -> Vec<TsBinding> {
    let Some(clause) = named_children(stmt)
        .into_iter()
        .find(|c| c.kind() == "import_clause")
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for c in named_children(clause) {
        match c.kind() {
            "identifier" => out.push(TsBinding {
                local: text(c, src),
                imported: "default".to_string(),
            }),
            "namespace_import" => {
                if let Some(id) = named_children(c)
                    .into_iter()
                    .find(|n| n.kind() == "identifier")
                {
                    out.push(TsBinding {
                        local: text(id, src),
                        imported: "*".to_string(),
                    });
                }
            }
            "named_imports" => {
                for spec in named_children(c) {
                    if spec.kind() != "import_specifier" {
                        continue;
                    }
                    let Some(name) = spec.child_by_field_name("name") else {
                        continue;
                    };
                    let alias = spec.child_by_field_name("alias");
                    out.push(TsBinding {
                        local: text(alias.unwrap_or(name), src),
                        imported: text(name, src),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

// A bare `export * from 'm'` carries no names and is marked `star`. `export *
// as NS from 'm'` binds a namespace OBJECT under NS -- resolving `NS.member`
// through it would be a second hop past the one level the resolver follows,
// so it contributes its import edge and no name mapping (neither `star` nor
// any `names` entry).
fn ts_reexport_entry(stmt: Node, src: &[u8]) -> Option<TsReexport> {
    let spec = ts_string_literal(stmt.child_by_field_name("source"), src)?;
    let line = ts_line(stmt);
    if named_children(stmt)
        .iter()
        .any(|c| c.kind() == "namespace_export")
    {
        return Some(TsReexport {
            spec,
            line,
            star: false,
            names: Vec::new(),
        });
    }
    let Some(clause) = named_children(stmt)
        .into_iter()
        .find(|c| c.kind() == "export_clause")
    else {
        return Some(TsReexport {
            spec,
            line,
            star: true,
            names: Vec::new(),
        });
    };
    let mut names = Vec::new();
    for s in named_children(clause) {
        if s.kind() != "export_specifier" {
            continue;
        }
        let Some(name) = s.child_by_field_name("name") else {
            continue;
        };
        let alias = s.child_by_field_name("alias");
        names.push(TsReexportName {
            exported: text(alias.unwrap_or(name), src),
            imported: text(name, src),
        });
    }
    Some(TsReexport {
        spec,
        line,
        star: false,
        names,
    })
}

// The kind vocabulary is `TS_BUCKET_ORDER` so a TS def row reads
// the same way a TS purpose segment does. `default` is not among them: a
// default export keeps the kind of what it declares, and the file records the
// local name it exported by default separately.
fn ts_decl_kind(node: Node) -> Option<&'static str> {
    match node.kind() {
        "class_declaration" | "abstract_class_declaration" => Some("class"),
        "function_declaration" | "generator_function_declaration" => Some("function"),
        "interface_declaration" => Some("interface"),
        "type_alias_declaration" => Some("type"),
        "enum_declaration" => Some("enum"),
        "lexical_declaration" | "variable_declaration" => Some("const"),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct TsDecl {
    kind: &'static str,
    line: usize,
    end_line: usize,
}

// Every top-level declaration with the line its NAME is declared on, whether
// exported or not: `export { A }` names a local declaration that was written
// without the keyword, and a CommonJS file exports by name only.
fn ts_top_level_decls(program_node: Node, src: &[u8]) -> HashMap<String, TsDecl> {
    let mut decls: HashMap<String, TsDecl> = HashMap::new();
    fn add(
        decls: &mut HashMap<String, TsDecl>,
        name: String,
        kind: &'static str,
        line: usize,
        end_line: usize,
    ) {
        if name.is_empty() {
            return;
        }
        decls.entry(name).or_insert(TsDecl {
            kind,
            line,
            end_line,
        });
    }
    fn from_declaration(node: Node, src: &[u8], decls: &mut HashMap<String, TsDecl>) {
        let Some(kind) = ts_decl_kind(node) else {
            return;
        };
        if kind == "const" {
            for d in named_children(node) {
                if d.kind() != "variable_declarator" {
                    continue;
                }
                let name_node = d.child_by_field_name("name");
                let line = ts_line(name_node.unwrap_or(d));
                for n in ts_pattern_names(name_node, src) {
                    add(decls, n, "const", line, d.end_position().row + 1);
                }
            }
            return;
        }
        add(
            decls,
            ts_declared_name(node, src),
            kind,
            ts_line(node),
            node.end_position().row + 1,
        );
    }
    for c in named_children(program_node) {
        if c.kind() == "export_statement" {
            if let Some(decl) = c.child_by_field_name("declaration") {
                from_declaration(decl, src, &mut decls);
            }
            continue;
        }
        from_declaration(c, src, &mut decls);
    }
    decls
}

fn ts_default_export_name(program_node: Node, src: &[u8]) -> Option<String> {
    for stmt in named_children(program_node) {
        if stmt.kind() != "export_statement" || !is_default_export_statement(stmt) {
            continue;
        }
        if let Some(decl) = stmt.child_by_field_name("declaration") {
            let name = ts_declared_name(decl, src);
            if !name.is_empty() {
                return Some(name);
            }
            continue;
        }
        if let Some(value) = stmt.child_by_field_name("value") {
            if value.kind() == "identifier" {
                return Some(text(value, src));
            }
        }
    }
    None
}

// The names this file makes importable, each mapped to the top-level
// declaration it names. A name with no matching declaration in this file
// contributes nothing: there is no line to point a caller at, and inventing
// one is a guess.
#[allow(
    clippy::cognitive_complexity,
    reason = "one flat dispatch over every export shape a top-level declaration can take; the shapes only make sense enumerated together"
)]
fn ts_exported_names(
    program_node: Node,
    decls: &HashMap<String, TsDecl>,
    src: &[u8],
) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    macro_rules! add {
        ($name:expr) => {{
            let name: String = $name;
            if !name.is_empty() && decls.contains_key(&name) && seen.insert(name.clone()) {
                names.push(name);
            }
        }};
    }
    let export_stmts: Vec<Node> = named_children(program_node)
        .into_iter()
        .filter(|c| c.kind() == "export_statement")
        .collect();
    if !export_stmts.is_empty() {
        for stmt in export_stmts {
            if stmt.child_by_field_name("source").is_some() {
                continue; // re-export, handled separately
            }
            if let Some(decl) = stmt.child_by_field_name("declaration") {
                if ts_decl_kind(decl) == Some("const") {
                    for d in named_children(decl) {
                        if d.kind() != "variable_declarator" {
                            continue;
                        }
                        for n in ts_pattern_names(d.child_by_field_name("name"), src) {
                            add!(n);
                        }
                    }
                } else {
                    add!(ts_declared_name(decl, src));
                }
                continue;
            }
            if let Some(clause) = named_children(stmt)
                .into_iter()
                .find(|c| c.kind() == "export_clause")
            {
                for s in named_children(clause) {
                    if s.kind() != "export_specifier" {
                        continue;
                    }
                    // A local `export { A as B }` publishes B, but the
                    // DECLARATION it points at is A -- only a name this file
                    // actually declares earns a def, so the alias is recorded
                    // against A's own line.
                    if let Some(name) = s.child_by_field_name("name") {
                        add!(text(name, src));
                    }
                }
            }
        }
        return names;
    }
    // CommonJS -- no `export` keyword anywhere in the file.
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
            CjsTarget::Prop(prop) => {
                add!(prop);
            }
            CjsTarget::Whole => match right {
                Some(r) if r.kind() == "object" => {
                    for c in named_children(r) {
                        if c.kind() == "shorthand_property_identifier" {
                            add!(text(c, src));
                        } else if c.kind() == "pair" {
                            if let Some(value) = c.child_by_field_name("value") {
                                if value.kind() == "identifier" {
                                    add!(text(value, src));
                                }
                            }
                        }
                    }
                }
                Some(r) if r.kind() == "identifier" => {
                    add!(text(r, src));
                }
                _ => {}
            },
        }
    }
    names
}

// `const x = require('m')` / `const { a, b } = require('m')` -- the CommonJS
// counterpart of an import clause, recorded in the same shape. A whole-module
// binding is `"*"` (the same marker a `* as ns` import uses) because that is
// what `require` returns; a destructured one names each property directly.
fn ts_require_import(decl_node: Node, src: &[u8]) -> Vec<TsImport> {
    let mut out = Vec::new();
    for d in named_children(decl_node) {
        if d.kind() != "variable_declarator" {
            continue;
        }
        let Some(value) = d.child_by_field_name("value") else {
            continue;
        };
        if value.kind() != "call_expression" {
            continue;
        }
        let Some(fnode) = value.child_by_field_name("function") else {
            continue;
        };
        if fnode.kind() != "identifier" || text(fnode, src) != "require" {
            continue;
        }
        let args = value.child_by_field_name("arguments");
        let first = args.and_then(|a| named_children(a).into_iter().next());
        let Some(spec) = ts_string_literal(first, src) else {
            continue;
        };
        let name_node = d.child_by_field_name("name");
        let bindings = match name_node {
            Some(n) if n.kind() == "identifier" => {
                vec![TsBinding {
                    local: text(n, src),
                    imported: "*".to_string(),
                }]
            }
            _ => ts_pattern_names(name_node, src)
                .into_iter()
                .map(|n| TsBinding {
                    local: n.clone(),
                    imported: n,
                })
                .collect(),
        };
        out.push(TsImport {
            spec,
            line: ts_line(d),
            bindings,
        });
    }
    out
}

#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "one flat dispatch over every reference shape a TS node can carry; splitting it would separate branches that share the same consumed-set bookkeeping"
)]
fn ts_record_ref(
    node: Node,
    src: &[u8],
    refs: &mut Vec<TsRef>,
    consumed: &mut HashSet<usize>,
    known: &HashSet<String>,
) {
    match node.kind() {
        "call_expression" => {
            let fnode = node.child_by_field_name("function");
            let callee = match fnode {
                Some(f) if f.kind() == "identifier" => Some(text(f, src)),
                Some(f) if f.kind() == "member_expression" => {
                    f.child_by_field_name("property").map(|p| text(p, src))
                }
                _ => None,
            };
            let dispatching = callee
                .as_deref()
                .is_some_and(|c| TS_DISPATCH_CALLEES.contains(&c));
            if dispatching {
                // `dispatch(loadThings())` names loadThings, not the inner
                // call's own callee: the argument IS the action creator or
                // thunk, and marking it consumed is what stops the walk from
                // ALSO recording it as a plain call. `dispatch(clearCart)` (an
                // already-built action object) names it directly.
                let args = node
                    .child_by_field_name("arguments")
                    .map(named_children)
                    .unwrap_or_default();
                for arg in args {
                    if arg.kind() == "identifier" {
                        let name = text(arg, src);
                        if known.contains(&name) {
                            refs.push(TsRef {
                                kind: "dispatch".to_string(),
                                name,
                                member: None,
                                line: ts_line(arg),
                            });
                        }
                        consumed.insert(arg.id());
                    } else if arg.kind() == "call_expression" {
                        if let Some(inner) = arg.child_by_field_name("function") {
                            if inner.kind() == "identifier" {
                                let name = text(inner, src);
                                if known.contains(&name) {
                                    refs.push(TsRef {
                                        kind: "dispatch".to_string(),
                                        name,
                                        member: None,
                                        line: ts_line(inner),
                                    });
                                }
                                consumed.insert(arg.id());
                            }
                        }
                    }
                }
            } else if !consumed.contains(&node.id()) {
                match fnode {
                    Some(f) if f.kind() == "identifier" => {
                        let name = text(f, src);
                        if known.contains(&name) {
                            refs.push(TsRef {
                                kind: "call".to_string(),
                                name,
                                member: None,
                                line: ts_line(f),
                            });
                        }
                    }
                    Some(f) if f.kind() == "member_expression" => {
                        let obj = f.child_by_field_name("object");
                        let prop = f.child_by_field_name("property");
                        // The QUALIFIER only, never the chain's tail method --
                        // the same line the C# side draws: `uses-member`
                        // resolves to a type, never to a chained call's
                        // method def.
                        if let (Some(obj), Some(prop)) = (obj, prop) {
                            let name = text(obj, src);
                            if obj.kind() == "identifier" && known.contains(&name) {
                                refs.push(TsRef {
                                    kind: "call".to_string(),
                                    name,
                                    member: Some(text(prop, src)),
                                    line: ts_line(obj),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        "new_expression" => {
            if let Some(ctor) = node.child_by_field_name("constructor") {
                let name = text(ctor, src);
                if ctor.kind() == "identifier" && known.contains(&name) {
                    refs.push(TsRef {
                        kind: "call".to_string(),
                        name,
                        member: None,
                        line: ts_line(ctor),
                    });
                }
            }
        }
        "jsx_opening_element" | "jsx_self_closing_element" => {
            let name_node = node.child_by_field_name("name");
            match name_node {
                Some(n) if n.kind() == "identifier" => {
                    let name = text(n, src);
                    if is_component_tag_name(&name) && known.contains(&name) {
                        refs.push(TsRef {
                            kind: "jsx-use".to_string(),
                            name,
                            member: None,
                            line: ts_line(n),
                        });
                    }
                }
                Some(n) if n.kind() == "member_expression" || n.kind() == "nested_identifier" => {
                    let obj = n.child_by_field_name("object");
                    let prop = n.child_by_field_name("property");
                    if let (Some(obj), Some(prop)) = (obj, prop) {
                        let name = text(obj, src);
                        if obj.kind() == "identifier"
                            && is_component_tag_name(&name)
                            && known.contains(&name)
                        {
                            refs.push(TsRef {
                                kind: "jsx-use".to_string(),
                                name,
                                member: Some(text(prop, src)),
                                line: ts_line(obj),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

// Pre-order, the same order the recursive form visits in: a dispatching call
// is always seen before the argument it consumes, which is what lets the
// consumed set suppress a duplicate plain-call ref for that argument.
// Cursor-driven, materialising a `Node` only for the four reference kinds.
fn ts_ref_walk(program_node: Node, src: &[u8], refs: &mut Vec<TsRef>, known: &HashSet<String>) {
    let mut consumed: HashSet<usize> = HashSet::new();
    let mut cursor = program_node.walk();
    loop {
        if TS_REFERENCE_NODES.contains(&cursor.node().kind()) {
            ts_record_ref(cursor.node(), src, refs, &mut consumed, known);
        }
        if cursor.goto_first_child() {
            continue;
        }
        let mut advanced = false;
        while cursor.depth() > 0 {
            if cursor.goto_next_sibling() {
                advanced = true;
                break;
            }
            if !cursor.goto_parent() {
                break;
            }
        }
        if !advanced && !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Extract the reference fragment (imports, exports, refs) from a parsed TS/JS
/// program node.
pub fn extract_ts_fragment(program_node: Node, src: &[u8]) -> TsFragment {
    let decls = ts_top_level_decls(program_node, src);
    let mut imports: Vec<TsImport> = Vec::new();
    let mut reexports: Vec<TsReexport> = Vec::new();
    for stmt in named_children(program_node) {
        if stmt.kind() == "import_statement" {
            let Some(spec) = ts_string_literal(stmt.child_by_field_name("source"), src) else {
                continue;
            };
            imports.push(TsImport {
                spec,
                line: ts_line(stmt),
                bindings: ts_import_bindings(stmt, src),
            });
        } else if stmt.kind() == "export_statement" && stmt.child_by_field_name("source").is_some()
        {
            if let Some(entry) = ts_reexport_entry(stmt, src) {
                reexports.push(entry);
            }
        } else if stmt.kind() == "lexical_declaration" || stmt.kind() == "variable_declaration" {
            imports.extend(ts_require_import(stmt, src));
        }
    }
    let defs: Vec<TsFragmentDef> = ts_exported_names(program_node, &decls, src)
        .into_iter()
        .map(|name| {
            let d = &decls[&name];
            TsFragmentDef {
                name,
                kind: d.kind.to_string(),
                line: d.line,
                end_line: d.end_line,
            }
        })
        .collect();
    // The only local names a cross-file resolver could ever land on: something
    // this file imported, or something this file itself exports. A reference
    // to anything else is unresolvable BY THE RESOLVER'S OWN RULE, so
    // recording it would put a fact in the fragment cache that exists only to
    // be discarded -- and it changes no edge the resolver would have emitted.
    let mut known: HashSet<String> = defs.iter().map(|d| d.name.clone()).collect();
    for imp in &imports {
        for b in &imp.bindings {
            known.insert(b.local.clone());
        }
    }
    let mut refs: Vec<TsRef> = Vec::new();
    ts_ref_walk(program_node, src, &mut refs, &known);
    // Appended LAST and only when the file has one, the house rule for every
    // added fact -- a file with no default export keeps the shorter shape.
    let default = ts_default_export_name(program_node, src).filter(|d| decls.contains_key(d));
    TsFragment {
        ts: 1,
        defs,
        imports,
        reexports,
        refs,
        default,
    }
}

/// One TS/JS file's whole contribution, off ONE parse: its purpose (when the
/// file exports anything) and its reference fragment (always), from the same
/// tree. A parse failure yields `None`, leaving the file out of BOTH outputs.
pub struct TsFileExtraction {
    /// The purpose value.
    pub purpose: Option<String>,
    /// The fragment value.
    pub fragment: TsFragment,
}

/// Parses TypeScript-family source into its optional purpose and graph fragment.
///
/// Returns `None` when the selected grammar cannot parse the source.
pub fn extract_ts_file(
    root: &Path,
    rel: &str,
    source: &str,
    grammar: crate::parse::TsGrammar,
) -> Option<TsFileExtraction> {
    let units = crate::parse::utf16_units(source);
    let tree = crate::parse::parse_ts_js(&units, grammar)?;
    let src = crate::parse::utf16_bytes(&units);
    let root_node = tree.root_node();
    let purpose = ts_purpose_segments(root_node, &src).map(|raw| {
        let detail = crate::walk::default_purpose_detailed(root, rel);
        if detail.is_comment {
            compose_hybrid_ts_purpose(&detail.text, &raw)
        } else {
            truncate(&raw)
        }
    });
    Some(TsFileExtraction {
        purpose,
        fragment: extract_ts_fragment(root_node, &src),
    })
}
