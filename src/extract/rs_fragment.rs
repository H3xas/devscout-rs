use std::collections::HashMap;

#[cfg(test)]
use std::cell::Cell;

use tree_sitter::Node;

use super::rs_attrs::{self, PathFact};
use super::rs_facts_types::{RsImpl, RsModPath};
use super::rs_fragment_types::{RsFileExtraction, RsFragment, RsItem};
use super::rs_paths::{join, path_segments, type_path_node};
use super::text::truncate;
use crate::parse;

// ---------------------------------------------------------------------------
// Rust definitions with spans, off one UTF-16 parse.
//
// The walk visits only the contexts whose items have a path of their own: the
// file root, inline module bodies, `impl` bodies, `trait` bodies and `extern`
// block bodies. Function bodies, initializers and token trees (macro
// invocations and `macro_rules!` bodies) are never entered, so an item there
// is not recorded: it has no path a reader could name it by.
//
// An item's path is the enclosing inline-module names, then an owner segment
// (the self type's head inside an `impl`, the trait's name inside a `trait`,
// nothing for an extern block), then its name. The trait of
// `impl Trait for Type` is left out, so two trait impls that both define
// `fmt` repeat a path; a repeat takes a `-N` suffix in source order, which
// keeps every path unique within its file and free of any generic or trait
// text that would need quoting on a command line.
// ---------------------------------------------------------------------------

/// Extracts one Rust file's items and purpose, with no disk access.
///
/// A file the grammar returns no tree for still gets an empty fragment,
/// because a graph source with no cached fragment would force a rebuild on
/// every map.
pub fn extract_rs_file(source: &str) -> RsFileExtraction {
    let units = parse::utf16_units(source);
    let Some(tree) = parse::parse_rust(&units) else {
        return RsFileExtraction {
            purpose: None,
            fragment: RsFragment::default(),
        };
    };
    let src = parse::utf16_bytes(&units);
    let root = tree.root_node();
    let mut found = Vec::new();
    let mut impls = Impls::default();
    walk_items(root, &src, &mut found, &mut impls);
    let purpose = compose_purpose(root, &src, &found);
    let uses = super::rs_uses::collect_uses(root, &src);
    let numbering = super::rs_sites::Numbering {
        blocks: &uses.block_ids,
        impls: &impls.ids,
    };
    let sites = super::rs_sites::collect_sites(root, &src, &numbering);
    let mut fragment = assemble(found, impls.records);
    fragment.uses = uses.uses;
    fragment.blocks = uses.blocks;
    fragment.sites = sites;
    fragment.opaque_scopes = super::rs_scope::macro_scopes(root, &src);
    RsFileExtraction { purpose, fragment }
}

// The fragment's items and the declaration facts marked on them, once
// duplicate paths are suffixed.
fn assemble(found: Vec<Found>, mut impls: Vec<RsImpl>) -> RsFragment {
    let marks: Vec<(Mark, Option<usize>, PathFact)> = found
        .iter()
        .map(|f| (f.mark, f.impl_index, f.mod_path.clone()))
        .collect();
    let items = dedupe(found);
    let mut fragment = RsFragment {
        items,
        ..RsFragment::default()
    };
    for (item, (mark, impl_index, mod_path)) in fragment.items.iter().zip(marks) {
        if let Some(path) = mod_path {
            fragment.mod_paths.push(RsModPath {
                item: item.path.clone(),
                path,
            });
        }
        match mark {
            Mark::ModFile => fragment.mod_files.push(item.path.clone()),
            Mark::Ctor => fragment.ctors.push(item.path.clone()),
            Mark::Test => fragment.tests.push(item.path.clone()),
            Mark::None => {}
        }
        if let Some(i) = impl_index {
            impls[i].items.push(item.path.clone());
        }
    }
    fragment.impls = impls;
    fragment
}

// The `impl` records the item walk writes, and the node id of each.
#[derive(Default)]
struct Impls {
    records: Vec<RsImpl>,
    ids: HashMap<usize, usize>,
}

// One item as the walk finds it, before duplicate paths are suffixed.
struct Found {
    name: String,
    kind: &'static str,
    path: String,
    line: usize,
    name_line: usize,
    end_line: usize,
    file_level: bool,
    mark: Mark,
    impl_index: Option<usize>,
    // A `mod x;` declaration's path fact; `None` keeps the default rule.
    mod_path: PathFact,
}

#[cfg(test)]
#[derive(Clone, Copy, Default)]
pub(super) struct ItemCost {
    pub items: usize,
    pub ancestry_work: usize,
    pub parent_calls: usize,
    pub path_bytes: usize,
}

#[cfg(test)]
thread_local! {
    static ITEM_COST: Cell<ItemCost> = Cell::new(ItemCost::default());
}

#[cfg(test)]
pub(super) fn reset_item_cost() {
    ITEM_COST.with(|cost| cost.set(ItemCost::default()));
}

#[cfg(test)]
pub(super) fn item_cost() -> ItemCost {
    ITEM_COST.with(Cell::get)
}

// A declaration fact the resolver needs beyond the item itself.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    None,
    // A `mod x;` declaration, whose body is another file.
    ModFile,
    // A tuple or unit struct, which is also a value.
    Ctor,
    // A function carrying `#[test]`, plainly or through `cfg_attr`.
    Test,
}

// The recorded kind for a node kind, as the Rust keyword that declares it.
fn record_kind(node_kind: &str) -> Option<&'static str> {
    Some(match node_kind {
        "mod_item" => "mod",
        "struct_item" => "struct",
        "enum_item" => "enum",
        "union_item" => "union",
        "trait_item" => "trait",
        "function_item" | "function_signature_item" => "fn",
        "const_item" => "const",
        "static_item" => "static",
        "type_item" | "associated_type" => "type",
        "macro_definition" => "macro_rules",
        _ => return None,
    })
}

// A pre-order walk over the walked contexts, kept on an explicit stack
// rather than the call stack: module nesting is unbounded in hostile input,
// and extraction runs on worker threads with small stacks. Children are
// pushed in reverse so they pop in source order. Each pending node carries
// its enclosing path and, inside an `impl` with a nameable self type, the
// index of that impl's record.
fn walk_items(root: Node, src: &[u8], out: &mut Vec<Found>, impls: &mut Impls) {
    let mut pending: Vec<Pending> = Vec::new();
    let top = Context {
        segments: Vec::new(),
        in_impl: None,
        file_level: true,
        under_path: false,
    };
    push_children(root, &top, &mut pending);
    while let Some((node, ctx)) = pending.pop() {
        let mut impl_index = None;
        let mut under_path = ctx.under_path;
        let segment = if let Some(kind) = record_kind(node.kind()) {
            let recorded = record_item(node, kind, src, &ctx, out);
            if kind == "mod" && node.child_by_field_name("body").is_some() {
                under_path |= rs_attrs::path_fact(node, src).is_some();
            }
            match recorded {
                Some(name) if kind == "mod" || kind == "trait" => Some(name),
                _ => continue,
            }
        } else if node.kind() == "impl_item" {
            impl_index = record_impl(node, src, &ctx.segments, &mut impls.records);
            if let Some(index) = impl_index {
                impls.ids.insert(node.id(), index);
            }
            Some(self_type_head(node.child_by_field_name("type"), src))
        } else if node.kind() == "foreign_mod_item" {
            None
        } else {
            continue;
        };
        // A `mod x;` declaration has no body and adds nothing.
        if let Some(body) = node.child_by_field_name("body") {
            let mut segments = ctx.segments;
            segments.extend(segment);
            let inner = Context {
                segments,
                in_impl: impl_index,
                file_level: false,
                under_path,
            };
            push_children(body, &inner, &mut pending);
        }
    }
}

// Where a pending node sits: its enclosing path, the impl record it is
// inside, whether it is at file level, and whether an enclosing inline
// module carries a `path` attribute, which makes every out-of-line child
// below it unreadable.
#[derive(Clone)]
struct Context {
    segments: Vec<String>,
    in_impl: Option<usize>,
    file_level: bool,
    under_path: bool,
}

type Pending<'t> = (Node<'t>, Context);

fn push_children<'t>(body: Node<'t>, ctx: &Context, pending: &mut Vec<Pending<'t>>) {
    let mut cursor = body.walk();
    let children: Vec<Node> = body.named_children(&mut cursor).collect();
    for child in children.into_iter().rev() {
        pending.push((child, ctx.clone()));
    }
}

// Records an `impl` block whose self type has a path; `None` otherwise.
fn record_impl(
    node: Node,
    src: &[u8],
    segments: &[String],
    impls: &mut Vec<RsImpl>,
) -> Option<usize> {
    let ty = type_path_node(node.child_by_field_name("type")?)?;
    let path = path_segments(ty, src)?;
    impls.push(RsImpl {
        scope: module_scope(segments),
        self_type: join(&path),
        items: Vec::new(),
    });
    Some(impls.len() - 1)
}

// The enclosing path is the scope itself for module-level items: an impl or
// trait never encloses another impl.
fn module_scope(segments: &[String]) -> String {
    segments.join("::")
}

// Records one item under `segments` and returns its name, or `None` when it
// has no nameable identity.
fn record_item(
    node: Node,
    kind: &'static str,
    src: &[u8],
    ctx: &Context,
    out: &mut Vec<Found>,
) -> Option<String> {
    let name_node = node.child_by_field_name("name")?;
    let name = item_name(name_node, src)?;
    let mark = mark_of(node, kind, src);
    let mod_path = match mark {
        Mark::ModFile if ctx.under_path => Some(None),
        Mark::ModFile => rs_attrs::path_fact(node, src),
        _ => None,
    };
    let mut path = ctx.segments.to_vec();
    path.push(name.clone());
    let path = path.join("::");
    #[cfg(test)]
    ITEM_COST.with(|cost| {
        let mut value = cost.get();
        value.items += 1;
        value.path_bytes += path.len();
        cost.set(value);
    });
    out.push(Found {
        name: name.clone(),
        kind,
        path,
        line: span_start_line(node),
        name_line: name_node.start_position().row + 1,
        end_line: end_line(node),
        file_level: ctx.file_level,
        mark,
        impl_index: ctx.in_impl,
        mod_path,
    });
    Some(name)
}

fn mark_of(node: Node, kind: &str, src: &[u8]) -> Mark {
    match kind {
        "mod" if node.child_by_field_name("body").is_none() => Mark::ModFile,
        "struct" => match node.child_by_field_name("body") {
            None => Mark::Ctor,
            Some(body) if body.kind() == "ordered_field_declaration_list" => Mark::Ctor,
            Some(_) => Mark::None,
        },
        "fn" if rs_attrs::is_test(node, src) => Mark::Test,
        _ => Mark::None,
    }
}

// The name an item is recorded under, or `None` when it has no nameable
// identity: a missing name in broken code, a macro metavariable, or `_`.
fn item_name(name_node: Node, src: &[u8]) -> Option<String> {
    if name_node.is_missing() || name_node.kind() == "metavariable" {
        return None;
    }
    let name = strip_raw(&parse::node_text(name_node, src));
    (!name.is_empty() && name != "_").then_some(name)
}

fn strip_raw(text: &str) -> String {
    text.strip_prefix("r#").unwrap_or(text).to_string()
}

// The last identifier of an `impl`'s self type once generic arguments,
// references, raw pointers, `dyn` and path qualifiers are removed. A type
// with no single name (tuple, array, slice, function pointer, unit, never, a
// `+`-bounded trait object) heads as `_`.
fn self_type_head(node: Option<Node>, src: &[u8]) -> String {
    let head = node.map(|n| type_head(n, src)).unwrap_or_default();
    if head.is_empty() {
        "_".to_string()
    } else {
        head
    }
}

fn type_head(mut node: Node, src: &[u8]) -> String {
    loop {
        let field = match node.kind() {
            "type_identifier" | "identifier" | "primitive_type" => {
                return strip_raw(&parse::node_text(node, src));
            }
            "generic_type" | "reference_type" | "pointer_type" => "type",
            "scoped_type_identifier" | "scoped_identifier" => "name",
            "dynamic_type" => "trait",
            _ => return String::new(),
        };
        match node.child_by_field_name(field) {
            Some(next) => node = next,
            None => return String::new(),
        }
    }
}

// The first line of an item's span: the first outer doc comment or outer
// attribute directly above it. A plain comment among them is passed over but
// never starts the span; an inner doc comment, an inner attribute, any other
// node or the start of the body ends the backward walk.
fn span_start_line(node: Node) -> usize {
    let mut start = node.start_position().row;
    let mut current = node.prev_sibling();
    while let Some(prev) = current {
        match prev.kind() {
            "attribute_item" => start = prev.start_position().row,
            "line_comment" | "block_comment" => {
                if prev.child_by_field_name("outer").is_some() {
                    start = prev.start_position().row;
                } else if prev.child_by_field_name("inner").is_some() {
                    break;
                }
            }
            _ => break,
        }
        current = prev.prev_sibling();
    }
    start + 1
}

// The line of an item's last character: a node that ends right after a line
// break reports the next row at column 0, which is not part of the item.
fn end_line(node: Node) -> usize {
    let end = node.end_position();
    if end.column == 0 && end.row > node.start_position().row {
        end.row
    } else {
        end.row + 1
    }
}

// Suffixes every repeated path with `-1`, `-2`, ... in walk order, which is
// source order. The count map only numbers repeats; it never orders output.
fn dedupe(found: Vec<Found>) -> Vec<RsItem> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    found
        .into_iter()
        .map(|f| {
            let repeats = seen.entry(f.path.clone()).or_insert(0);
            let path = if *repeats == 0 {
                f.path
            } else {
                format!("{}-{}", f.path, repeats)
            };
            *repeats += 1;
            RsItem {
                name: f.name,
                kind: f.kind.to_string(),
                path,
                line: f.line,
                name_line: f.name_line,
                end_line: f.end_line,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Purpose: `<prefix> — <segments>`, either part alone, or `None`. The prefix
// is the file's leading comment (an inner doc's first non-empty line, or a
// plain comment's first line); the segments name the file-level items,
// grouped by kind.
// ---------------------------------------------------------------------------

const SEGMENT_ORDER: &[&str] = &[
    "struct",
    "enum",
    "union",
    "trait",
    "type",
    "fn",
    "const",
    "static",
    "macro_rules",
    "mod",
];

fn compose_purpose(root: Node, src: &[u8], found: &[Found]) -> Option<String> {
    let prefix = purpose_prefix(root, src);
    let segments = purpose_segments(found);
    match (prefix, segments.is_empty()) {
        (Some(p), false) => Some(truncate(&format!("{p} — {}", segments.join(" | ")))),
        (Some(p), true) => Some(truncate(&p)),
        (None, false) => Some(truncate(&segments.join(" | "))),
        (None, true) => None,
    }
}

fn purpose_segments(found: &[Found]) -> Vec<String> {
    SEGMENT_ORDER
        .iter()
        .flat_map(|kind| {
            found
                .iter()
                .filter(move |f| f.file_level && f.kind == *kind)
                .map(|f| format!("{} {}", f.kind, f.name))
        })
        .collect()
}

fn is_comment(node: Node) -> bool {
    matches!(node.kind(), "line_comment" | "block_comment")
}

fn is_inner_doc(node: Node) -> bool {
    is_comment(node) && node.child_by_field_name("inner").is_some()
}

fn purpose_prefix(root: Node, src: &[u8]) -> Option<String> {
    let mut cursor = root.walk();
    let children: Vec<Node> = root
        .named_children(&mut cursor)
        .skip_while(|n| matches!(n.kind(), "shebang" | "inner_attribute_item"))
        .collect();
    let first = *children.first()?;
    if is_inner_doc(first) {
        if first.kind() == "block_comment" {
            return first_doc_line(&parse::node_text(first, src));
        }
        // Consecutive `//!` lines are one doc, as rustdoc reads them.
        return children
            .iter()
            .take_while(|n| n.kind() == "line_comment" && is_inner_doc(**n))
            .find_map(|n| first_doc_line(&parse::node_text(*n, src)));
    }
    if is_comment(first) && first.child_by_field_name("outer").is_none() {
        return plain_comment_line(&parse::node_text(first, src));
    }
    None
}

// The first non-empty line of an inner doc comment's text, with its marker,
// a closing `*/` and one leading `*` removed.
fn first_doc_line(text: &str) -> Option<String> {
    let body = text
        .strip_prefix("//!")
        .or_else(|| text.strip_prefix("/*!"))
        .unwrap_or(text);
    let body = body.strip_suffix("*/").unwrap_or(body);
    body.lines()
        .map(|l| {
            let l = l.trim();
            l.strip_prefix('*').unwrap_or(l).trim().to_string()
        })
        .find(|l| !l.is_empty())
}

// A plain comment's first line with its opening marker (and a closing `*/`
// on that same line) removed; an empty first line gives no prefix.
fn plain_comment_line(text: &str) -> Option<String> {
    let line = text.lines().next().unwrap_or("");
    let line = line
        .strip_prefix("//")
        .or_else(|| line.strip_prefix("/*"))
        .unwrap_or(line);
    let line = line.trim_end().strip_suffix("*/").unwrap_or(line).trim();
    (!line.is_empty()).then(|| line.to_string())
}
