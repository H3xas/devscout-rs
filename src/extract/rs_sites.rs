use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use tree_sitter::Node;

use super::rs_facts_types::{RsRole, RsSite};
use super::rs_paths::{join, path_segments, Segment};
use super::rs_scope::{block_items, body_locals, generic_names, macro_items};
use crate::parse;

// ---------------------------------------------------------------------------
// Rust reference sites: every path the edge pass tries to bind, with the
// role its position gives it and the scope it sits in.
//
// The walk covers module-level code, item signatures and every function
// body, on an explicit stack. It writes no site for a `use` declaration, a
// method name or field, a macro name, an attribute, a `macro_rules!` body, a
// literal, a primitive type or anything inside an `ERROR` region; a macro
// invocation's arguments go to the token scan instead. A site is dropped
// when its first segment is shadowed: by an item a surrounding block
// declares, by a generic parameter, or, for a single-segment value, by a
// local binding of the enclosing function. `Self` alone gives no site, and a
// `Self`-rooted path carries the impl or trait it stands for, or is dropped
// where there is neither.
// ---------------------------------------------------------------------------

/// Where a node sits: the scope, block and names around it.
#[derive(Clone, Default)]
pub(super) struct Ctx {
    pub scope: Rc<str>,
    pub block: Option<usize>,
    pub locals: Rc<HashSet<String>>,
    pub shadow: Rc<HashSet<String>>,
    pub invocation_locals: Rc<HashSet<String>>,
    pub invocation_shadow: Rc<HashSet<String>>,
    pub generics: Rc<HashSet<String>>,
    pub self_impl: Option<usize>,
    pub self_trait: Option<Rc<str>>,
    in_body: bool,
    pattern: bool,
}

/// Node ids the item and `use` walks already numbered.
pub(super) struct Numbering<'a> {
    pub blocks: &'a HashMap<usize, usize>,
    pub impls: &'a HashMap<usize, usize>,
}

const SKIPPED: &[&str] = &[
    "ERROR",
    "attribute_item",
    "inner_attribute_item",
    "macro_definition",
    "use_declaration",
    "extern_crate_declaration",
    "visibility_modifier",
    "string_literal",
    "raw_string_literal",
    "char_literal",
    "line_comment",
    "block_comment",
    "lifetime",
    "label",
    "self_parameter",
    "primitive_type",
    "metavariable",
];

const PATH_KINDS: &[&str] = &[
    "identifier",
    "scoped_identifier",
    "type_identifier",
    "scoped_type_identifier",
    "generic_type",
    "generic_type_with_turbofish",
    "generic_function",
];

/// Every reference site of one file, in source order: by the start of the
/// path, which the walk alone would not give for a generic argument list or
/// a call nested in a macro argument.
pub(super) fn collect_sites(root: Node, src: &[u8], numbering: &Numbering) -> Vec<RsSite> {
    let mut out: Vec<(usize, RsSite)> = Vec::new();
    let root_ctx = Ctx {
        shadow: Rc::new(macro_items(root, src).into_iter().collect()),
        ..Ctx::default()
    };
    let mut stack: Stack = vec![(root, None, "", root_ctx)];
    while let Some((node, field, parent, ctx)) = stack.pop() {
        let kind = node.kind();
        if SKIPPED.contains(&kind) || node.is_missing() {
            continue;
        }
        if kind == "macro_invocation" {
            super::rs_macro_scan::scan(node, src, &ctx, &mut out);
            continue;
        }
        if PATH_KINDS.contains(&kind) {
            path_site(node, parent, field, &ctx, src, &mut out, &mut stack);
            continue;
        }
        if kind == "mod_item" && ctx.in_body {
            continue;
        }
        let inner = enter(node, &ctx, src, numbering);
        push_children(node, &inner, &mut stack);
    }
    out.sort_by_key(|(start, _)| *start);
    out.into_iter().map(|(_, site)| site).collect()
}

// The context a node's children inherit.
fn enter(node: Node, ctx: &Ctx, src: &[u8], numbering: &Numbering) -> Ctx {
    let mut inner = ctx.clone();
    match node.kind() {
        "mod_item" => {
            let name = node
                .child_by_field_name("name")
                .map(|n| strip_raw(&parse::node_text(n, src)))
                .unwrap_or_default();
            let body = node.child_by_field_name("body");
            inner = Ctx {
                scope: Rc::from(scoped(&ctx.scope, &name)),
                shadow: Rc::new(
                    body.map(|b| macro_items(b, src))
                        .unwrap_or_default()
                        .into_iter()
                        .collect(),
                ),
                ..Ctx::default()
            };
        }
        "function_item" => {
            inner.locals = Rc::new(body_locals(node, src));
            inner.generics = extended(&ctx.generics, generic_names(node, src));
            inner.in_body = true;
        }
        "closure_expression" => {
            // Inside a function the body's own walk already holds every
            // binding a closure makes; only a closure outside any function
            // walks its own subtree, once.
            if !ctx.in_body {
                inner.locals = Rc::new(body_locals(node, src));
                inner.in_body = true;
            }
        }
        "impl_item" => {
            inner.generics = extended(&ctx.generics, generic_names(node, src));
            inner.self_impl = numbering.impls.get(&node.id()).copied();
            inner.self_trait = None;
        }
        "trait_item" => {
            let name = node
                .child_by_field_name("name")
                .map(|n| strip_raw(&parse::node_text(n, src)))
                .unwrap_or_default();
            inner.generics = extended(&ctx.generics, generic_names(node, src));
            // A trait declared inside a function body is not recorded, and its
            // path would name the module-level trait of the same name.
            inner.self_trait = (!ctx.in_body).then(|| Rc::from(scoped(&ctx.scope, &name)));
            inner.self_impl = None;
        }
        "struct_item" | "enum_item" | "union_item" | "type_item" => {
            inner.generics = extended(&ctx.generics, generic_names(node, src));
        }
        "block" => {
            if let Some(&index) = numbering.blocks.get(&node.id()) {
                inner.block = Some(index);
            }
            inner.shadow = extended(&ctx.shadow, block_items(node, src));
            inner.in_body = true;
        }
        _ => {}
    }
    inner
}

fn scoped(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        format!("{scope}::{name}")
    }
}

fn strip_raw(text: &str) -> String {
    text.strip_prefix("r#").unwrap_or(text).to_string()
}

fn extended(base: &Rc<HashSet<String>>, names: Vec<String>) -> Rc<HashSet<String>> {
    if names.is_empty() {
        return Rc::clone(base);
    }
    let mut set = (**base).clone();
    set.extend(names);
    Rc::new(set)
}

// Pushes a node's named children in reverse, so they pop in source order,
// with each child's field name and whether it opens a pattern.
fn push_children<'t>(node: Node<'t>, ctx: &Ctx, stack: &mut Stack<'t>) {
    let mut children: Vec<(Node<'t>, Option<&'t str>)> = Vec::new();
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.is_named() {
                children.push((child, cursor.field_name()));
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    for (child, field) in children.into_iter().rev() {
        if field == Some("name") && node.kind() != "struct_expression" {
            continue;
        }
        let mut child_ctx = ctx.clone();
        child_ctx.pattern = child_is_pattern(node, child, field, ctx.pattern);
        stack.push((child, field, node.kind(), child_ctx));
    }
}

// Whether a child starts, continues or leaves a pattern.
fn child_is_pattern(parent: Node, child: Node, field: Option<&str>, in_pattern: bool) -> bool {
    match (parent.kind(), field) {
        ("let_declaration" | "parameter" | "for_expression" | "let_condition", Some("pattern")) => {
            true
        }
        ("match_arm", Some("pattern")) => true,
        ("match_pattern", Some("condition")) => false,
        ("closure_parameters", _) => child.kind() != "parameter",
        ("parameter" | "let_declaration" | "closure_expression", _) => false,
        _ => in_pattern,
    }
}

// The role a path gets from where it sits, or `None` for a path that is a
// binding or a name rather than a reference.
fn role_of(node: Node, parent: &str, field: Option<&str>, ctx: &Ctx) -> Option<RsRole> {
    let kind = node.kind();
    if ctx.pattern {
        let binding = kind == "identifier" && field != Some("type");
        return (!binding).then_some(RsRole::Pat);
    }
    Some(match (parent, field) {
        ("call_expression", Some("function")) => RsRole::Call,
        ("struct_expression", Some("name")) => RsRole::Lit,
        ("impl_item", Some("trait")) => RsRole::Impl,
        _ if matches!(
            kind,
            "identifier" | "scoped_identifier" | "generic_function"
        ) =>
        {
            RsRole::Value
        }
        _ => RsRole::Type,
    })
}

// A pending node: the node, its field name and its parent's kind, and the
// context it inherits.
type Stack<'t> = Vec<(Node<'t>, Option<&'t str>, &'t str, Ctx)>;

fn path_site<'t>(
    node: Node<'t>,
    parent: &str,
    field: Option<&'t str>,
    ctx: &Ctx,
    src: &[u8],
    out: &mut Vec<(usize, RsSite)>,
    stack: &mut Stack<'t>,
) {
    let Some(mut role) = role_of(node, parent, field, ctx) else {
        return;
    };
    let mut path = node;
    if node.kind() == "generic_function" {
        let in_call = parent == "call_expression" && field == Some("function");
        role = if in_call { RsRole::Call } else { RsRole::Value };
        let Some(function) = node.child_by_field_name("function") else {
            return;
        };
        push_generic_parts(node, ctx, stack);
        if function.kind() == "field_expression" {
            stack.push((function, Some("function"), "generic_function", ctx.clone()));
            return;
        }
        path = function;
    }
    push_generic_parts(path, ctx, stack);
    let Some(segments) = path_segments(path, src) else {
        return;
    };
    if let Some(site) = make_site(role, &segments, ctx, false) {
        out.push((path.start_byte(), site));
    }
}

// Queues the generic argument lists and qualified-path types inside a path
// chain, which are type positions of their own.
fn push_generic_parts<'t>(path: Node<'t>, ctx: &Ctx, stack: &mut Stack<'t>) {
    let mut type_ctx = ctx.clone();
    type_ctx.pattern = false;
    let mut current = Some(path);
    while let Some(node) = current {
        current = match node.kind() {
            "generic_type" | "generic_type_with_turbofish" | "generic_function" => {
                if let Some(args) = node.child_by_field_name("type_arguments") {
                    stack.push((args, Some("type_arguments"), node.kind(), type_ctx.clone()));
                }
                node.child_by_field_name("type")
            }
            "scoped_identifier" | "scoped_type_identifier" => node.child_by_field_name("path"),
            "bracketed_type" => {
                stack.push((node, None, "scoped_identifier", type_ctx.clone()));
                None
            }
            _ => None,
        };
    }
}

/// Builds a site from a path's segments, or `None` when the path is
/// shadowed or names no item this pass can reach.
pub(super) fn make_site(
    role: RsRole,
    segments: &[Segment],
    ctx: &Ctx,
    mac: bool,
) -> Option<RsSite> {
    let first = segments.first()?.text.as_str();
    if segments.len() == 1 && (first == "Self" || first.is_empty()) {
        return None;
    }
    if ctx.shadow.contains(first)
        || ctx.invocation_shadow.contains(first)
        || ctx.generics.contains(first)
    {
        return None;
    }
    let single_value = segments.len() == 1 && matches!(role, RsRole::Call | RsRole::Value);
    if single_value && (ctx.locals.contains(first) || ctx.invocation_locals.contains(first)) {
        return None;
    }
    let (self_impl, self_trait) = if first == "Self" {
        match (ctx.self_impl, &ctx.self_trait) {
            (Some(i), _) => (Some(i), None),
            (None, Some(t)) => (None, Some(t.to_string())),
            (None, None) => return None,
        }
    } else {
        (None, None)
    };
    let lines: Vec<usize> = segments.iter().map(|s| s.line).collect();
    let line = *lines.last()?;
    let spans = lines.iter().any(|l| *l != line);
    Some(RsSite {
        role,
        path: join(segments),
        line,
        lines: if spans { lines } else { Vec::new() },
        scope: ctx.scope.to_string(),
        block: ctx.block,
        self_impl,
        self_trait,
        mac,
    })
}
