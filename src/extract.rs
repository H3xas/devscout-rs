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

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

const MAX_PURPOSE: usize = 200;

// (grammar node kind, TYPE_KINDS label) pairs, iterated as an ordered slice
// to preserve declaration order.
const TYPE_KINDS: &[(&str, &str)] = &[
    ("class_declaration", "class"),
    ("interface_declaration", "interface"),
    ("struct_declaration", "struct"),
    ("record_declaration", "record"),
    ("enum_declaration", "enum"),
];

const NAMESPACE_NODES: &[&str] = &["namespace_declaration", "file_scoped_namespace_declaration"];

fn type_kind_label(kind: &str) -> Option<&'static str> {
    TYPE_KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, label)| *label)
}

fn new_parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
        .expect("failed to load C# grammar");
    parser
}

fn named_children<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn text(node: Node, src: &[u8]) -> String {
    crate::parse::node_text(node, src)
}

fn declared_name(node: Node, src: &[u8]) -> String {
    node.child_by_field_name("name")
        .map(|n| text(n, src))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Purpose/signature composition.
// ---------------------------------------------------------------------------

fn namespace_level_types<'a>(root: Node<'a>) -> Vec<Node<'a>> {
    let mut types = Vec::new();
    collect_types(root, &mut types);
    types
}

fn collect_types<'a>(node: Node<'a>, types: &mut Vec<Node<'a>>) {
    for child in named_children(node) {
        if NAMESPACE_NODES.contains(&child.kind()) {
            let body = child.child_by_field_name("body").unwrap_or(child);
            collect_types(body, types);
            continue;
        }
        if type_kind_label(child.kind()).is_some() {
            types.push(child);
        }
    }
}

fn base_list_text(node: Node, src: &[u8]) -> String {
    let Some(bases) = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "base_list")
    else {
        return String::new();
    };
    let raw = text(bases, src);
    let after_colon = match raw.strip_prefix(':') {
        Some(rest) => rest.trim_start(),
        None => raw.as_str(),
    };
    collapse_whitespace(after_colon).trim().to_string()
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            out.push(ch);
            in_ws = false;
        }
    }
    out
}

fn is_public(node: Node, src: &[u8]) -> bool {
    named_children(node)
        .into_iter()
        .any(|c| c.kind() == "modifier" && text(c, src) == "public")
}

fn strip_async_suffix(name: &str) -> String {
    name.strip_suffix("Async").unwrap_or(name).to_string()
}

// Purpose-signature method names: trailing "Async" stripped, deduped by the
// caller across the whole file. NOT the same helper as raw_method_names --
// a graph def needs the real, unabridged name (see that function's own
// comment).
fn public_method_names(node: Node, src: &[u8], kind: &str) -> Vec<String> {
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    named_children(body)
        .into_iter()
        .filter(|c| c.kind() == "method_declaration" && (kind == "interface" || is_public(*c, src)))
        .map(|c| strip_async_suffix(&declared_name(c, src)))
        .filter(|n| !n.is_empty())
        .collect()
}

// One namespace-level type's raw ingredients for a purpose segment --
// (line, kind, name, bases, unfiled public method names). Line comes along
// so `extract()`'s type-declaration-recovery path (below) can merge
// recovered types' parts into this list at the right sorted position
// before formatting; the ordinary (non-recovery) path never inspects it.
type SegmentParts = (usize, &'static str, String, String, Vec<String>);

fn type_segment_parts(node: Node, src: &[u8]) -> Option<SegmentParts> {
    let kind = type_kind_label(node.kind())?;
    let name = declared_name(node, src);
    if name.is_empty() {
        return None;
    }
    let bases = base_list_text(node, src);
    let methods = public_method_names(node, src, kind);
    Some((node.start_position().row + 1, kind, name, bases, methods))
}

// Formats an already line-ordered list of segment parts into one purpose
// string: "kind name[ : bases][; m1, m2]" joined with " | ", methods
// deduped globally across the whole list in order (a method name credited
// to an earlier type in the list is never repeated on a later one -- port
// of astPurposes' own cross-type dedup), then truncated to MAX_PURPOSE
// UTF-16 code units.
fn format_segments(parts: Vec<SegmentParts>) -> Option<String> {
    let mut seen_methods: HashSet<String> = HashSet::new();
    let mut segments: Vec<String> = Vec::new();
    for (_line, kind, name, bases, raw_methods) in parts {
        let mut methods = Vec::new();
        for method in raw_methods {
            if seen_methods.contains(&method) {
                continue;
            }
            seen_methods.insert(method.clone());
            methods.push(method);
        }
        let header = if bases.is_empty() {
            format!("{kind} {name}")
        } else {
            format!("{kind} {name} : {bases}")
        };
        let segment = if methods.is_empty() {
            header
        } else {
            format!("{header}; {}", methods.join(", "))
        };
        segments.push(segment);
    }
    if segments.is_empty() {
        None
    } else {
        Some(truncate(&segments.join(" | ")))
    }
}

// NB: no `compose_signature(types, src)` wrapper -- extract()'s only
// caller needs to interleave a second (recovered) parts list before
// formatting, so it calls type_segment_parts()/format_segments() directly.

// The 200-unit limit counts UTF-16 code units, not bytes or chars -- matched
// here via `encode_utf16` rather than `str::len`/char indexing, so a purpose
// string with non-ASCII identifiers truncates at a code-unit boundary.
fn truncate(s: &str) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    if units.len() > MAX_PURPOSE {
        let mut truncated = String::from_utf16_lossy(&units[..MAX_PURPOSE - 3]);
        truncated.push_str("...");
        truncated
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------
// Graph fragment extraction and its helpers. Scope: defs =
// namespace-level+nested types with public method names plus enum members;
// refs = using directives, base-list types,
// object-creation types, field/property/parameter/return types, generic
// type arguments, and member-access qualifier.member candidates. No
// call-graph resolution.
// ---------------------------------------------------------------------------

/// Represents `DefRecord`.
pub struct DefRecord {
    /// The id value.
    pub id: String,
    /// The name value.
    pub name: String,
    /// The namespace value.
    pub namespace: String,
    /// The kind value.
    pub kind: String,
    /// The line value.
    pub line: usize,
    /// The methods value.
    pub methods: Vec<String>,
    /// Declared property names, source order, deduped
    /// (indexers excluded by construction: `indexer_declaration` is a
    /// different grammar node). NO accessibility filter, deliberately
    /// asymmetric with `methods`: static/const/readonly/expression-bodied all
    /// count and only indexers are excluded, so a private member is still a
    /// member of the type. Empty for every
    /// enum-member def.
    pub properties: Vec<String>,
    /// Declared field names, source order, deduped, every
    /// declarator of every `field_declaration` ("private int a, b;"
    /// contributes both). `event_field_declaration` is a distinct node type
    /// and is NOT a field for this purpose. Same no-accessibility-filter
    /// rule as `properties`.
    pub fields: Vec<String>,
    /// (method name, declared return type NAME) pairs in
    /// FIRST-declaration source order, parallel to `methods` (same
    /// `is_recorded_method` predicate, so the two can never drift). The
    /// first declaration of a name claims the slot outright: a later
    /// overload with a different return type is ignored, and a first
    /// declaration whose return type yields no fact (void, var, a
    /// predefined type) BLOCKS the name rather than letting a later
    /// overload stand in for it. A Vec of pairs, not a map: the serialized
    /// key order is significant (see graph.rs's FragDef).
    pub method_returns: Vec<(String, String)>,
    /// The extension methods this type declares: every method
    /// whose FIRST parameter carries the `this` modifier, in source order,
    /// deduped by (name, thisType, arityMin, arityMax). Appended LAST, after
    /// `method_returns`, and omitted at serialization when empty. Unlike
    /// `methods` there is NO accessibility filter (`internal static class
    /// FooExtensions` is the shape this feature exists for) and no `static`
    /// check on either the method or its class -- C# already disallows a
    /// `this` parameter anywhere else, so the parameter modifier IS the
    /// discriminator.
    pub extension_methods: Vec<ExtensionMethod>,
    /// The DIRECT base-type identifiers this
    /// declaration lists, in source order, deduped. Same base_list traversal
    /// `record_base_list` walks for its `inherits` refs, reduced to a base
    /// IDENTIFIER because the resolver re-RESOLVES these names through the
    /// ordinary ladder rather than matching them; the resulting closure is
    /// what lets the extension tier see an inherited instance member and
    /// decline. Appended after `extension_methods` and omitted when empty.
    pub bases: Vec<String>,
    /// The declaring type's OWN type-parameter names (`class
    /// MongoRepository<T>` records `["T"]`), empty for every non-generic
    /// declaration. This is the ctor-DI resolver's "is this def itself an
    /// open-generic implementation" signal. Appended after `bases`, omitted
    /// when empty.
    pub type_params: Vec<String>,
    /// Per base name that carried a type-argument list, that
    /// list's generic-arg descriptors relative to `type_params` (a `"*"`
    /// wildcard marks a position that is a pass-through of the declaring
    /// type's own parameter). A `Vec` of pairs, not a map, for the same
    /// reason `method_returns` is: the serialized key order is significant.
    /// Appended after `type_params`, omitted when empty; a base
    /// with no type-argument list at all contributes no entry.
    pub base_generic_args: Vec<(String, Vec<String>)>,
    /// The methods this type declares that a test
    /// framework would DISCOVER as tests, in source order, deduped. Appended
    /// LAST, after `base_generic_args`, and omitted at serialization when
    /// empty. The attribute IS the fact, which is why this is the one member
    /// fact with no accessibility filter at all and why no file, folder or
    /// type-name convention is read anywhere.
    pub test_methods: Vec<String>,
    /// (property name, declared type fact) pairs for exactly the
    /// properties `properties` records, in the same source order and under the
    /// same dedup. A property whose declared type yields no fact (a predefined
    /// type) has no entry, so the two lists are parallel but not equal in
    /// length. A `Vec` of pairs, not a map, for the same reason
    /// `method_returns` is one: the serialized key order is significant.
    /// Appended LAST, after `test_methods`.
    pub property_types: Vec<(String, Fact)>,
    /// 1-based last line of the complete declaration node.
    pub end_line: usize,
}

/// One `extensionMethods` entry. Field order (`name`, `thisType`, `arityMin`,
/// `arityMax`, `thisArgs`) is significant: it fixes the serialized field
/// order.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionMethod {
    /// The name value.
    pub name: String,
    /// The this type value.
    pub this_type: String,
    /// Non-this parameters a caller cannot
    /// leave out: no default value AND no `params` modifier.
    pub arity_min: usize,
    /// Total non-this parameter count, or -1 when the trailing parameter is a
    /// `params` array (unbounded above). Signed precisely so -1 can be the
    /// sentinel written as a JSON number.
    pub arity_max: i64,
    /// The this-parameter type's TOP-LEVEL
    /// type-argument descriptors, present only when that type is generic.
    /// A position naming one of the method's or the enclosing class's own type
    /// parameters is recorded as "*" (wildcard).
    pub this_args: Option<Vec<String>>,
}

/// Represents `RefRecord`.
pub struct RefRecord {
    /// The kind value.
    pub kind: String,
    /// The name value.
    pub name: String,
    /// The qualified value.
    pub qualified: Option<String>,
    /// The member value.
    pub member: Option<String>,
    /// The line value.
    pub line: usize,
    /// `None` only for 'imports' refs -- a using directive is not
    /// namespace-scoped, so `ns` is deliberately `null`. Every other ref
    /// carries `Some(ns)`, where `ns`
    /// may itself be the empty string at file scope.
    pub namespace: Option<String>,
    /// The type arg count value.
    pub type_arg_count: Option<usize>,
    /// `true` when a uses-member qualifier carried a
    /// type-argument list anywhere (`Cache<T>.x`, `Ns.Cache<T>.x`): syntax
    /// only a TYPE can carry, so the resolver treats it as type-certainty
    /// for non-enum member emission. Always `false` for every other ref
    /// kind.
    pub generic: bool,
    /// The declared type NAME of the receiver a uses-member
    /// qualifier names, when the enclosing scope holds exactly one fact for
    /// it (a declared local, a `var x = new T()` local, a parameter, a
    /// field, or a primary-constructor parameter). Set ONLY for a BARE,
    /// non-generic qualifier -- see the `member_access_expression` arm's
    /// guard, which is what makes the chain-tail hazard structurally
    /// impossible. Always `None` for every other ref kind.
    pub receiver_type: Option<String>,
    /// The argument count of the call this
    /// member access is the CALLEE of (`argument_list` named-child count),
    /// recorded only when the access is the `function` field of an
    /// `invocation_expression`. A property read (`x.P`) carries `None`, which
    /// is what keeps it out of the arity-matched extension tier entirely.
    /// Appended AFTER `receiver_type`. Always `None` for every other ref kind.
    pub arg_count: Option<usize>,
    /// The DECLARED receiver type's top-level
    /// type-argument descriptors, present only when that type is generic. A
    /// position naming a type parameter of the enclosing method or class is
    /// recorded as "*" (wildcard): nothing at the fact site knows its binding.
    /// Travels with `receiver_type` -- the fact carries both or neither.
    /// Appended LAST, after `arg_count`.
    pub receiver_args: Option<Vec<String>>,
    /// The walk's own type_stack verbatim: the enclosing type simple
    /// names, OUTERMOST first, the same order type_id joins with "+" to build
    /// a nested def id. Empty at namespace level, and empty is exactly what an
    /// absent key deserializes to. Appended LAST, after `receiver_args`.
    pub outer_types: Vec<String>,
    /// Generic-arg descriptors for a 'ctor-param' ref's
    /// parameter type (same descriptor shape as `receiver_args`: a `"*"`
    /// wildcard for a pass-through of the enclosing type's own type
    /// parameter), present only when that type is generic. `None` for every
    /// other ref kind. Appended LAST of all, after `outer_types` -- 'ctor-param'
    /// is a ref kind no other caller touches, so putting it after that
    /// "last of all" field costs no other ref kind a single byte.
    pub args: Option<Vec<String>>,
    /// The type whose PROPERTY the qualifier's last segment is, for
    /// a two-segment chain whose head the enclosing scope has a fact for
    /// ("a.Settings" in `a.Settings.Reload()`). Never travels with
    /// `receiver_type`: that one is bare-qualifier-only and this one is
    /// dotted-qualifier-only, which is what keeps the chain-tail hazard
    /// structurally impossible -- the resolver reaches the tail's type through
    /// the head type's RECORDED property types, never by inheriting the head's
    /// own. Appended after `args`.
    pub receiver_property_owner: Option<String>,
    /// The type whose METHOD a `var x = Q.M(...)` initializer
    /// called, and that method's name. Always set as a pair, and never
    /// alongside `receiver_type`: the local's type is whatever `M` returns, a
    /// lookup only the resolver can do. Appended LAST of all.
    pub receiver_call_owner: Option<String>,
    /// The receiver call member value.
    pub receiver_call_member: Option<String>,
}

/// Represents `UsingRecord`.
pub enum UsingRecord {
    /// The value value.
    Alias {
        /// The alias name.
        alias: String,
        /// The aliased target.
        target: String,
        /// Whether the directive is global.
        global: bool,
    },
    /// The value value.
    Plain {
        /// The imported namespace text.
        text: String,
        /// Whether the directive is global.
        global: bool,
    },
}

/// Represents `Extraction`.
pub struct Extraction {
    /// The purpose value.
    pub purpose: Option<String>,
    /// The defs value.
    pub defs: Vec<DefRecord>,
    /// The usings value.
    pub usings: Vec<UsingRecord>,
    /// The refs value.
    pub refs: Vec<RefRecord>,
    /// Every member the file's types declare, appended LAST after
    /// `refs` in both the fragment and the `extract-dump` shape.
    pub names: Vec<NameRecord>,
}

/// One declared name and the line its own NAME TOKEN sits on. Deliberately
/// not the declaration node's start row: a member's span begins
/// at its attribute list, so an attributed member would point a reader at the
/// `[` line instead of the line carrying the name they searched for.
///
/// Serialized field order (`name`, `kind`, `line`, `owner`) is significant;
/// `owner` is omitted when empty, which is how a markup or resource
/// key -- built by `markup.rs`, owned by no C# type -- serializes with three
/// fields.
#[derive(Debug, Clone, PartialEq)]
pub struct NameRecord {
    /// The name value.
    pub name: String,
    /// The kind value.
    pub kind: String,
    /// The line value.
    pub line: usize,
    /// The owner value.
    pub owner: String,
}

// Unwraps nullable/array/alias-qualified wrappers,
// pulls the base identifier out of a generic_name (discarding its type
// arguments -- those are recorded separately when the walk reaches the
// type_argument_list node), and takes qualified_name's full dotted text
// verbatim. Anything else (predefined_type, implicit_type/`var`, ...) is
// not a user-defined type reference.
fn outer_type_name(node: Node, src: &[u8]) -> Option<String> {
    match node.kind() {
        "nullable_type" | "array_type" => match node.child_by_field_name("type") {
            Some(inner) => outer_type_name(inner, src),
            None => None,
        },
        "generic_name" => named_children(node)
            .into_iter()
            .find(|c| c.kind() == "identifier")
            .map(|id| text(id, src)),
        "qualified_name" => {
            let tail = node.child_by_field_name("name")?;
            let full = text(node, src);
            let tail_text = text(tail, src);
            let normalized_tail = outer_type_name(tail, src)?;
            full.strip_suffix(&tail_text)
                .map(|prefix| format!("{prefix}{normalized_tail}"))
        }
        "alias_qualified_name" => match node.child_by_field_name("name") {
            Some(n) => outer_type_name(n, src),
            None => None,
        },
        "identifier" => Some(text(node, src)),
        _ => None,
    }
}

// The base IDENTIFIER of a type node, for the local type facts. Deliberately
// NOT outer_type_name: that one
// returns a qualified name's FULL dotted text because the resolution ladder
// tries an exact-FQN match first. A stage-2 fact is a NAME only -- generic
// arguments stripped, a qualified name reduced to its last segment, and
// predefined types (string/int/void/...) plus `var` (implicit_type) yielding
// None, which every caller reads as "no fact" rather than as a type called
// "string". An empty result is folded to None as well: every caller guards
// with a presence check, for which "" and absent are the same answer.
//
// `keep_predefined` flips ONLY the predefined-type case, for the
// one fact family that wants "string" as an answer rather than as a refusal:
// an extension method's this-parameter type. `this string s` is legal and
// common C#, and its thisType is a name a receiver has to match exactly, not a
// def anyone resolves. Every local-fact caller passes `false` and therefore
// keeps its exact prior output.
fn base_type_identifier(node: Option<Node>, src: &[u8], keep_predefined: bool) -> Option<String> {
    let node = node?;
    let name = match node.kind() {
        "nullable_type" | "array_type" => {
            base_type_identifier(node.child_by_field_name("type"), src, keep_predefined)
        }
        "generic_name" => named_children(node)
            .into_iter()
            .find(|c| c.kind() == "identifier")
            .map(|id| text(id, src)),
        "qualified_name" | "alias_qualified_name" => {
            base_type_identifier(node.child_by_field_name("name"), src, keep_predefined)
        }
        "identifier" => Some(text(node, src)),
        "predefined_type" => {
            if keep_predefined {
                Some(text(node, src))
            } else {
                None
            }
        }
        _ => None,
    };
    name.filter(|n| !n.is_empty())
}

// The generic_name a type node bottoms out at, following the SAME unwrapping
// path
// base_type_identifier takes (nullable and array wrappers, qualified/alias
// tails), or None when the type is not generic at its top level. `Ns.Box<T>`
// parses as a qualified_name whose `name` field IS the generic_name, which is
// why the qualified case recurses rather than stopping.
fn top_level_generic_name(node: Option<Node>) -> Option<Node> {
    let node = node?;
    match node.kind() {
        "nullable_type" | "array_type" => top_level_generic_name(node.child_by_field_name("type")),
        "qualified_name" | "alias_qualified_name" => {
            top_level_generic_name(node.child_by_field_name("name"))
        }
        "generic_name" => Some(node),
        _ => None,
    }
}

fn type_argument_arity(node: Option<Node>, src: &[u8]) -> Option<usize> {
    let generic = top_level_generic_name(node)?;
    let list = named_children(generic)
        .into_iter()
        .find(|c| c.kind() == "type_argument_list")?;
    Some(text(list, src).bytes().filter(|b| *b == b',').count() + 1)
}

// The TOP-LEVEL generic argument descriptors of a type node, or None when it
// carries no
// type-argument list (`Box<>`, an unbound generic, counts as none). One
// descriptor per argument, in source order: the argument's base identifier
// (predefined types KEPT, so `IDictionary<string, object>` records
// ["string", "object"]), except a position naming one of `type_params` -- the
// enclosing method's or type's own type parameters -- which records "*".
//
// A single argument that yields NO base identifier at all (a tuple type, a
// pointer) drops the WHOLE list: the match rule compares positions by index,
// so a partial list would silently shift every argument after the hole.
fn generic_arg_descriptors(
    node: Option<Node>,
    src: &[u8],
    type_params: &HashSet<String>,
) -> Option<Vec<String>> {
    let generic = top_level_generic_name(node)?;
    let list = named_children(generic)
        .into_iter()
        .find(|c| c.kind() == "type_argument_list")?;
    let args = named_children(list);
    if args.is_empty() {
        return None;
    }
    let mut descriptors = Vec::with_capacity(args.len());
    for arg in args {
        let base = base_type_identifier(Some(arg), src, true)?;
        descriptors.push(if type_params.contains(&base) {
            "*".to_string()
        } else {
            base
        });
    }
    Some(descriptors)
}

// The type-parameter names a declaration introduces (`class Box<T>`,
// `void Then<TSaga, TData>(...)`). Empty for every non-generic declaration.
fn type_parameter_names(node: Node, src: &[u8]) -> HashSet<String> {
    let mut names = HashSet::new();
    let Some(list) = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "type_parameter_list")
    else {
        return names;
    };
    for p in named_children(list) {
        if p.kind() != "type_parameter" {
            continue;
        }
        let name = match p.child_by_field_name("name") {
            Some(n) => text(n, src),
            None => text(p, src),
        };
        if !name.is_empty() {
            names.insert(name);
        }
    }
    names
}

// Same traversal as type_parameter_names, but order-preserving
// (first-occurrence order, deduped) rather than a HashSet: this is the one
// caller that serializes the list itself (DefRecord.type_params) rather than
// only testing membership, and a HashSet's iteration order is undefined --
// This must emit the type parameters in source order for the extract-dump
// bytes to be stable.
fn type_parameter_names_ordered(node: Node, src: &[u8]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let Some(list) = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "type_parameter_list")
    else {
        return names;
    };
    for p in named_children(list) {
        if p.kind() != "type_parameter" {
            continue;
        }
        let name = match p.child_by_field_name("name") {
            Some(n) => text(n, src),
            None => text(p, src),
        };
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

// A member-access qualifier is a uses-member candidate when it is a plain
// name ('identifier' or 'qualified_name'), a 'generic_name' ("Cache<T>.x" --
// type args dropped for resolution, the same normalization outer_type_name
// applies in type positions), OR a member_access_expression chain that
// itself bottoms out at a plain name -- "Some.Namespace.MyEnum" in
// "Some.Namespace.MyEnum.Member" parses as nested member_access_expression
// (not qualified_name) since it's in expression position, so the chain is
// flattened recursively into its full dotted text. walk() still recurses
// into every level of the chain regardless (each level is its own
// member_access_expression node), so a shorter window within the same chain
// also gets its own (separately resolved) candidate -- the resolution ladder
// (resolve.rs) silently drops whichever one doesn't clear its emission
// tiers. A chain that never bottoms out at a plain name (a call, an
// object-creation expression, an indexer, ...) simply yields no candidate at
// that position, per the "never guess" rule.
//
// Returns (text, generic) rather than bare text: a type-argument list
// anywhere in the qualifier is syntax only a
// TYPE can carry (locals, fields, and properties cannot), so the resolver
// uses `generic` as a type-certainty signal for non-enum member emission.
fn member_qualifier_info(node: Option<Node>, src: &[u8]) -> Option<(String, bool)> {
    let node = node?;
    match node.kind() {
        "identifier" | "qualified_name" => Some((text(node, src), false)),
        "generic_name" => named_children(node)
            .into_iter()
            .find(|c| c.kind() == "identifier")
            .map(|id| (text(id, src), true)),
        "member_access_expression" => {
            let inner = member_qualifier_info(node.child_by_field_name("expression"), src);
            let name_node = node.child_by_field_name("name")?;
            let (inner_text, inner_generic) = inner?;
            if name_node.kind() == "generic_name" {
                return named_children(name_node)
                    .into_iter()
                    .find(|c| c.kind() == "identifier")
                    .map(|id| (format!("{inner_text}.{}", text(id, src)), true));
            }
            let name = text(name_node, src);
            if name.is_empty() {
                return None;
            }
            Some((format!("{inner_text}.{name}"), inner_generic))
        }
        _ => None,
    }
}

// The argument count of the CALL this member access is the callee of, or
// `None` when it is
// not a callee at all.
//
// The parent test is deliberately narrow on both halves. The parent must be an
// `invocation_expression`, and `node` must be its `function` field -- an access
// sitting in the parent's ARGUMENT list ("Send(x.Payload)") has an
// invocation_expression parent too, and inheriting that call's argument count
// would be exactly the kind of borrowed fact stage 2's chain-tail hazard is
// about. Everything else -- a property read, an element access, a member access
// used as a value -- yields `None`, and an absent argCount is what keeps a
// non-call out of the arity-matched extension tier entirely.
//
// Because the test reads `node`'s OWN parent, every ref site gets its own
// answer for free: a flattened chain window and a promoted qualifier both
// ask the question of their own position in the tree and can never inherit a
// neighbour's count. Node identity is compared here by byte-range: two
// distinct nodes of one tree cannot share both a start and an end byte AND a
// kind at the same tree position.
fn invocation_arg_count(node: Node) -> Option<usize> {
    let parent = node.parent()?;
    if parent.kind() != "invocation_expression" {
        return None;
    }
    let function = parent.child_by_field_name("function")?;
    if function.id() != node.id() {
        return None;
    }
    let args = parent.child_by_field_name("arguments")?;
    if args.kind() != "argument_list" {
        return None;
    }
    Some(named_children(args).len())
}

fn push_ref(
    refs: &mut Vec<RefRecord>,
    kind: &str,
    name: String,
    line: usize,
    ns: Option<String>,
    qualified: Option<String>,
    type_stack: &[String],
) {
    refs.push(RefRecord {
        kind: kind.to_string(),
        name,
        qualified,
        member: None,
        line,
        namespace: ns,
        type_arg_count: None,
        generic: false,
        receiver_type: None,
        arg_count: None,
        receiver_args: None,
        outer_types: type_stack.to_vec(),
        args: None,
        receiver_property_owner: None,
        receiver_call_owner: None,
        receiver_call_member: None,
    });
}

// One 'ctor-param' ref per constructor parameter (see the
// "constructor_declaration" walk arm). Kept separate from push_ref, like
// push_member_ref is, rather than overloading it with a slot every other
// caller would pass as None.
fn push_ctor_param_ref(
    refs: &mut Vec<RefRecord>,
    name: String,
    line: usize,
    ns: String,
    args: Option<Vec<String>>,
    type_stack: &[String],
) {
    refs.push(RefRecord {
        kind: "ctor-param".to_string(),
        name,
        qualified: None,
        member: None,
        line,
        namespace: Some(ns),
        type_arg_count: Some(args.as_ref().map_or(0, Vec::len)),
        generic: false,
        receiver_type: None,
        arg_count: None,
        receiver_args: None,
        outer_types: type_stack.to_vec(),
        args,
        receiver_property_owner: None,
        receiver_call_owner: None,
        receiver_call_member: None,
    });
}

// Member access needs the member name alongside the qualifier, which
// push_ref has no slot for -- kept separate rather than overloading it with
// an extra argument every other caller would have to pass as None.
//
// `receiver_type` is appended AFTER `generic`, and only ever
// set for a BARE qualifier -- see the member_access_expression arm for the
// guard. Keeping the guard at the call site (rather than here) is what makes
// the chain-tail hazard structurally impossible: a flattened tail ("x.Foo"
// as the qualifier of ".Bar") is dotted, so it can never be handed a
// receiver fact it did not earn.
//
// `arg_count` is appended AFTER
// `receiver_type` and computed the same way -- at the call site, from the ref's
// OWN node -- for the same reason.
//
// `receiver_args` is appended LAST and travels
// with the receiver FACT -- both halves come out of the same `Fact`, so a
// receiverArgs can never land on a ref that has no receiver.
//
// `outer_types` is appended LAST of all, after `receiver_args` -- see
// push_ref for what it carries.
fn push_member_ref(
    refs: &mut Vec<RefRecord>,
    qualifier_text: &str,
    member: String,
    line: usize,
    ns: String,
    generic: bool,
    receiver: Option<Fact>,
    arg_count: Option<usize>,
    type_stack: &[String],
    property_owner: Option<String>,
) {
    // A call fact records the CALLEE it depends on and never a receiver type:
    // the two are mutually exclusive on one ref, which is what lets every
    // reader tell a recorded type from a lookup the resolver still owes.
    let (receiver_type, receiver_args, receiver_call_owner, receiver_call_member) = match receiver {
        Some(Fact {
            type_name,
            call: Some(member),
            ..
        }) => (None, None, Some(type_name), Some(member)),
        Some(Fact {
            type_name,
            args,
            call: None,
        }) => (Some(type_name), args, None, None),
        None => (None, None, None, None),
    };
    match qualifier_text.rfind('.') {
        Some(dot) => refs.push(RefRecord {
            kind: "uses-member".to_string(),
            name: qualifier_text[dot + 1..].to_string(),
            qualified: Some(qualifier_text.to_string()),
            member: Some(member),
            line,
            namespace: Some(ns),
            type_arg_count: None,
            generic,
            receiver_type,
            arg_count,
            receiver_args,
            outer_types: type_stack.to_vec(),
            args: None,
            receiver_property_owner: property_owner.clone(),
            receiver_call_owner: receiver_call_owner.clone(),
            receiver_call_member: receiver_call_member.clone(),
        }),
        None => refs.push(RefRecord {
            kind: "uses-member".to_string(),
            name: qualifier_text.to_string(),
            qualified: None,
            member: Some(member),
            line,
            namespace: Some(ns),
            type_arg_count: None,
            generic,
            receiver_type,
            arg_count,
            receiver_args,
            outer_types: type_stack.to_vec(),
            args: None,
            receiver_property_owner: property_owner,
            receiver_call_owner,
            receiver_call_member,
        }),
    }
}

// Line comes from the type node itself, not the enclosing declaration -- a
// declaration's own span starts at its attribute list when it has one,
// which would otherwise point a reader at the attribute line instead of
// the line the type reference is actually on.
fn record_single_type(
    node: Option<Node>,
    kind: &str,
    ns: &str,
    type_stack: &[String],
    src: &[u8],
    refs: &mut Vec<RefRecord>,
) {
    let Some(node) = node else {
        return;
    };
    if node.kind() == "tuple_type" {
        for el in named_children(node) {
            if el.kind() != "tuple_element" {
                continue;
            }
            record_single_type(
                el.child_by_field_name("type"),
                kind,
                ns,
                type_stack,
                src,
                refs,
            );
        }
        return;
    }
    let Some(raw) = outer_type_name(node, src) else {
        return;
    };
    let line = node.start_position().row + 1;
    let arity = Some(type_argument_arity(Some(node), src).unwrap_or(0));
    match raw.rfind('.') {
        Some(dot) => push_ref(
            refs,
            kind,
            raw[dot + 1..].to_string(),
            line,
            Some(ns.to_string()),
            Some(raw.clone()),
            type_stack,
        ),
        None => push_ref(
            refs,
            kind,
            raw.clone(),
            line,
            Some(ns.to_string()),
            None,
            type_stack,
        ),
    }
    if let Some(last) = refs.last_mut() {
        last.type_arg_count = arity;
    }
}

fn record_base_list(
    node: Node,
    ns: &str,
    type_stack: &[String],
    src: &[u8],
    refs: &mut Vec<RefRecord>,
) {
    let Some(bl) = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "base_list")
    else {
        return;
    };
    for child in named_children(bl) {
        if child.kind() == "argument_list" {
            continue;
        }
        if child.kind() == "primary_constructor_base_type" {
            record_single_type(
                child.child_by_field_name("type"),
                "inherits",
                ns,
                type_stack,
                src,
                refs,
            );
            continue;
        }
        record_single_type(Some(child), "inherits", ns, type_stack, src, refs);
    }
}

// Deliberately NOT public_method_names: that one strips a trailing "Async"
// and dedupes across the whole file for purpose-signature compactness. A
// graph def needs the real name, unabridged.
fn raw_method_names(node: Node, src: &[u8], kind: &str) -> Vec<String> {
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    named_children(body)
        .into_iter()
        .filter(|c| is_recorded_method(*c, src, kind))
        .map(|c| declared_name(c, src))
        .filter(|n| !n.is_empty())
        .collect()
}

// The one predicate `methods` and `method_returns` share, so the two lists
// can never drift apart: method_returns is a map PARALLEL to methods, not a
// second, wider survey of the type.
fn is_recorded_method(node: Node, src: &[u8], kind: &str) -> bool {
    node.kind() == "method_declaration" && (kind == "interface" || is_public(node, src))
}

// Declared property names, source order, deduped. Indexers are
// a different grammar node (indexer_declaration) so they are excluded by
// construction; expression-bodied properties are property_declaration like
// any other, so they are included. No accessibility filter -- see
// DefRecord::properties.
fn raw_property_names(node: Node, src: &[u8]) -> Vec<String> {
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut names = Vec::new();
    for c in named_children(body) {
        if c.kind() != "property_declaration" {
            continue;
        }
        let name = declared_name(c, src);
        if name.is_empty() || !seen.insert(name.clone()) {
            continue;
        }
        names.push(name);
    }
    names
}

// (name, fact) pairs for exactly the properties `raw_property_names`
// records, in the same source order and under the same dedup -- a property
// whose declared type yields no fact simply has no entry. The fact is the SAME
// shape a receiver fact carries, which is what lets resolution treat a property
// hop exactly like a field- or local-typed one.
fn raw_property_types(
    node: Node,
    src: &[u8],
    type_params: &HashSet<String>,
) -> Vec<(String, Fact)> {
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut pairs = Vec::new();
    for c in named_children(body) {
        if c.kind() != "property_declaration" {
            continue;
        }
        let name = declared_name(c, src);
        if name.is_empty() || !seen.insert(name.clone()) {
            continue;
        }
        if let Some(fact) = type_fact(c.child_by_field_name("type"), src, type_params) {
            pairs.push((name, fact));
        }
    }
    pairs
}

// Declared field names, source order, deduped -- every
// declarator of every field_declaration ("private int a, b;" contributes
// both). event_field_declaration is a distinct node type and is NOT a field
// for this purpose.
fn raw_field_names(node: Node, src: &[u8]) -> Vec<String> {
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut names = Vec::new();
    for c in named_children(body) {
        if c.kind() != "field_declaration" {
            continue;
        }
        let Some(vd) = named_children(c)
            .into_iter()
            .find(|k| k.kind() == "variable_declaration")
        else {
            continue;
        };
        for decl in named_children(vd) {
            if decl.kind() != "variable_declarator" {
                continue;
            }
            let Some(name) = decl.child_by_field_name("name").map(|n| text(n, src)) else {
                continue;
            };
            if name.is_empty() || !seen.insert(name.clone()) {
                continue;
            }
            names.push(name);
        }
    }
    names
}

// (name, returnTypeName) pairs in first-declaration order, for
// exactly the methods `raw_method_names` records. FIRST declaration of a name
// claims the slot outright: a later overload with a different return type is
// ignored, and a first declaration whose return type yields no fact (void,
// var, a predefined type) BLOCKS the name rather than letting a later
// overload's return type stand in for it -- picking a non-first overload
// would be a guess.
fn raw_method_returns(node: Node, src: &[u8], kind: &str) -> Vec<(String, String)> {
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut pairs = Vec::new();
    for c in named_children(body) {
        if !is_recorded_method(c, src, kind) {
            continue;
        }
        let name = declared_name(c, src);
        if name.is_empty() || !seen.insert(name.clone()) {
            continue;
        }
        if let Some(returns) = base_type_identifier(c.child_by_field_name("returns"), src, false) {
            pairs.push((name, returns));
        }
    }
    pairs
}

// The two halves of an extension method's acceptable argument COUNT, read off
// the parameter list as
// written:
//   arity_min -- non-this parameters a caller cannot leave out: no default
//     value AND no `params` modifier.
//   arity_max -- the total non-this parameter count, or -1 (unbounded) when the
//     trailing parameter is a `params` array.
// A single exact arity would under-match every optional-parameter and
// `params` call site and -- worse -- make two classes look like ONE candidate
// when only one of them could actually bind the call: the false-uniqueness
// shape.
//
// The loop reads ALL children, not named_children, because tree-sitter-c-sharp
// does NOT wrap a `params` parameter in a `parameter` node: it emits a bare
// anonymous `params` token followed by that parameter's type and name nodes as
// direct children of the parameter_list. A named-children count therefore reads
// `(this T t, params X[] xs)` as THREE parameters and a `parameter`-node count
// reads it as one; the token itself is the only reliable signal, and since C#
// requires `params` to be last, seeing it at all means unbounded.
fn extension_arity_range(parameters: Node, src: &[u8]) -> (usize, i64) {
    let mut total: i64 = 0;
    let mut min: usize = 0;
    let mut unbounded = false;
    let mut seen_this = false;
    let mut cursor = parameters.walk();
    for c in parameters.children(&mut cursor) {
        if c.kind() == "params" {
            unbounded = true;
            continue;
        }
        if c.kind() != "parameter" {
            continue;
        }
        // The first `parameter` node IS the this-parameter (the caller has
        // already verified its `this` modifier), and a flattened `params` group
        // can never occupy that slot.
        if !seen_this {
            seen_this = true;
            continue;
        }
        total += 1;
        let is_params = named_children(c)
            .iter()
            .any(|m| m.kind() == "modifier" && text(*m, src) == "params");
        if is_params {
            unbounded = true;
        } else if !has_default_value(c) {
            min += 1;
        }
    }
    (min, if unbounded { -1 } else { total })
}

// A default value is an `=` token among the parameter's own children.
fn has_default_value(parameter: Node) -> bool {
    (0..parameter.child_count() as u32).any(|i| parameter.child(i).map(|c| c.kind()) == Some("="))
}

// Extension methods this type declares: every method whose FIRST
// parameter carries the `this` modifier, as
// {name, thisType, arityMin, arityMax, thisArgs?} in source order, deduped by
// the (name, thisType, arityMin, arityMax) QUADRUPLE -- the key the resolver's
// bucket lookup and range filter are built from. Overloads that differ only in
// their later parameters ("Render(this Widget w)" /
// "Render(this Widget w, int d)") are two entries with two ranges, and a call
// binds to whichever one its argument count actually falls inside.
//
// `this_args` (the generic amendment) is present only when the this-parameter
// type is generic, and records that type's TOP-LEVEL type arguments with this
// method's and this class's own type parameters written as "*" -- a wildcard,
// because an extension declared over `EventPipelineBinder<TSaga, TData>`
// genuinely accepts any binding, while one declared over
// `IDictionary<string, object>` accepts exactly that one.
//
// Two deliberate NON-filters, both the same argument. The enclosing class is
// not checked for `static`, and neither is the method: C# already disallows a
// `this` parameter anywhere but a static method of a static non-generic class,
// so the parameter modifier IS the discriminator and the parser's output is the
// truth -- a filter could only ever throw away a fact, never add one. And
// unlike `methods` (public members only), there is NO accessibility filter:
// `internal static class FooExtensions` is the single most common shape this
// feature exists for, and an extension method is usable wherever it is VISIBLE
// -- which the resolver bounds by the using/namespace admission rule, not by a
// modifier read off the declaration.
//
// `this` on a non-first parameter is not an extension method (and does not
// compile); the first-parameter-only read is what excludes it. A `parameter`
// node can carry MORE than one modifier (`this ref T x`), so EVERY named
// `modifier` child is scanned.
fn raw_extension_methods(node: Node, src: &[u8]) -> Vec<ExtensionMethod> {
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    let class_type_params = type_parameter_names(node, src);
    let mut seen: HashSet<String> = HashSet::new();
    let mut entries = Vec::new();
    for c in named_children(body) {
        if c.kind() != "method_declaration" {
            continue;
        }
        let Some(parameters) = c.child_by_field_name("parameters") else {
            continue;
        };
        let params = named_children(parameters);
        let Some(first) = params.first().copied() else {
            continue;
        };
        if first.kind() != "parameter" {
            continue;
        }
        if !named_children(first)
            .iter()
            .any(|m| m.kind() == "modifier" && text(*m, src) == "this")
        {
            continue;
        }
        let name = declared_name(c, src);
        // keep_predefined: `this string s` records "string". Same array/generic/
        // qualified collapsing as every other type fact, so `this Widget[] a`
        // records "Widget" -- matching what a `Widget[]` local's receiver fact
        // records, which is the whole point of the pair being compared by name.
        let type_node = first.child_by_field_name("type");
        let Some(this_type) = base_type_identifier(type_node, src, true) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let (arity_min, arity_max) = extension_arity_range(parameters, src);
        if !seen.insert(format!("{name} {this_type} {arity_min} {arity_max}")) {
            continue;
        }
        let mut type_params = class_type_params.clone();
        type_params.extend(type_parameter_names(c, src));
        let this_args = generic_arg_descriptors(type_node, src, &type_params);
        entries.push(ExtensionMethod {
            name,
            this_type,
            arity_min,
            arity_max,
            this_args,
        });
    }
    entries
}

// The DIRECT base-type names this declaration lists, in source order and
// deduped -- the same base_list
// traversal `record_base_list` walks for its `inherits` refs, reduced to a base
// IDENTIFIER (generic arguments stripped, a qualified name cut to its last
// segment) because these names are RESOLVED, not matched: the resolver hands
// each one back through the ordinary ladder as a bare name, exactly like a
// stage-2 receiver fact.
//
// Recorded for every kind record_type_def handles, not just the four that emit
// `inherits` refs: an enum's `: byte` and a delegate's absent base list both
// reduce to nothing on their own (a predefined type yields no identifier), so
// the extra generality costs no bytes and needs no per-kind branch.
fn raw_base_names(node: Node, src: &[u8]) -> Vec<String> {
    let Some(bl) = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "base_list")
    else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    for child in named_children(bl) {
        if child.kind() == "argument_list" {
            continue;
        }
        let type_node = if child.kind() == "primary_constructor_base_type" {
            child.child_by_field_name("type")
        } else {
            Some(child)
        };
        if let Some(name) = base_type_identifier(type_node, src, false) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

// The generic-argument descriptors
// raw_base_names throws away, keyed by the same bare base identifier,
// first-declaration wins (mirroring raw_base_names's own first-wins dedup).
// `type_params` here is the DECLARING type's own type parameters, so
// `class MongoRepository<T> : IRepository<T>` records `[("IRepository",
// ["*"])]` -- a wildcard pass-through, the ctor-DI resolver's signal that
// this is an OPEN-generic implementation -- while `class SpecificRepo :
// IRepository<User>` records `[("IRepository", ["User"])]`, a closed one. A
// base with no type-argument list at all contributes no entry.
fn raw_base_generic_args(
    node: Node,
    src: &[u8],
    type_params: &HashSet<String>,
) -> Vec<(String, Vec<String>)> {
    let Some(bl) = named_children(node)
        .into_iter()
        .find(|c| c.kind() == "base_list")
    else {
        return Vec::new();
    };
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for child in named_children(bl) {
        if child.kind() == "argument_list" {
            continue;
        }
        let type_node = if child.kind() == "primary_constructor_base_type" {
            child.child_by_field_name("type")
        } else {
            Some(child)
        };
        let Some(name) = base_type_identifier(type_node, src, false) else {
            continue;
        };
        if out.iter().any(|(k, _)| k == &name) {
            continue;
        }
        if let Some(args) = generic_arg_descriptors(type_node, src, type_params) {
            out.push((name, args));
        }
    }
    out
}

// The attribute short names that MARK a method as a test
// -- xUnit's Fact/Theory, NUnit's Test/TestCase/TestCaseSource (Theory is
// shared with xUnit), MSTest's TestMethod/DataTestMethod -- split in two
// because only MSTest's pair is GATED: `[TestMethod]` inside a class that does
// not carry `[TestClass]` is not a discovered test, while xUnit has no
// class-level attribute at all and NUnit's `[TestFixture]` is optional.
// Data-source attributes (InlineData, MemberData, ClassData, DataRow,
// DynamicData), lifecycle hooks (SetUp, TearDown, OneTimeSetUp,
// TestInitialize, ...) and the class-level containers themselves are absent
// from both sets, so none of them can ever mark a method.
const DIRECT_TEST_ATTRIBUTES: &[&str] = &["Fact", "Theory", "Test", "TestCase", "TestCaseSource"];
const MSTEST_TEST_ATTRIBUTES: &[&str] = &["TestMethod", "DataTestMethod"];
const MSTEST_CLASS_ATTRIBUTE: &str = "TestClass";
const TEST_METHOD_KINDS: &[&str] = &["class", "struct", "record"];
const ATTRIBUTE_SUFFIX: &str = "Attribute";

// Every attribute name written on one declaration, normalized to the spellings
// the sets above are keyed by. An `attribute_list` is a direct named child of
// the declaration it decorates (type or method) and holds one `attribute` child
// per comma-separated entry inside a single bracket pair, optionally preceded
// by an `attribute_target_specifier` (`[method: Fact]`) -- a distinct node type,
// so filtering on `attribute` skips it without a special case. The attribute's
// `name` field is an `identifier` or a `qualified_name`; only the segment after
// the last dot names the type, and C# lets a usage site drop the `Attribute`
// suffix, so both spellings of the same name are offered and either one
// matching is a match.
fn attribute_names(node: Node, src: &[u8]) -> HashSet<String> {
    let mut names = HashSet::new();
    for list in named_children(node) {
        if list.kind() != "attribute_list" {
            continue;
        }
        for attr in named_children(list) {
            if attr.kind() != "attribute" {
                continue;
            }
            let Some(name_node) = attr.child_by_field_name("name") else {
                continue;
            };
            let text = text(name_node, src);
            if text.is_empty() {
                continue;
            }
            let last = match text.rfind('.') {
                Some(i) => &text[i + 1..],
                None => &text[..],
            };
            if last.is_empty() {
                continue;
            }
            names.insert(last.to_string());
            if last.len() > ATTRIBUTE_SUFFIX.len() && last.ends_with(ATTRIBUTE_SUFFIX) {
                names.insert(last[..last.len() - ATTRIBUTE_SUFFIX.len()].to_string());
            }
        }
    }
    names
}

// class/struct/record only. An interface body cannot host a discovered test (no
// runner instantiates one) and an enum body has no methods at all, so the kind
// gate costs one lookup and keeps both out by construction rather than relying
// on their bodies happening to be empty of matches.
//
// A local function inside a method body is not a method_declaration at type
// body level, so the flat named-children scan every other member fact uses
// excludes it for free; a nested type computes its own list on its own visit
// and never inherits an enclosing `[TestClass]`.
fn raw_test_methods(node: Node, src: &[u8], kind: &str) -> Vec<String> {
    if !TEST_METHOD_KINDS.contains(&kind) {
        return Vec::new();
    }
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    let mstest = attribute_names(node, src).contains(MSTEST_CLASS_ATTRIBUTE);
    let mut seen: HashSet<String> = HashSet::new();
    let mut names = Vec::new();
    for c in named_children(body) {
        if c.kind() != "method_declaration" {
            continue;
        }
        let attrs = attribute_names(c, src);
        let marked = attrs.iter().any(|a| {
            DIRECT_TEST_ATTRIBUTES.contains(&a.as_str())
                || (mstest && MSTEST_TEST_ATTRIBUTES.contains(&a.as_str()))
        });
        if !marked {
            continue;
        }
        let name = declared_name(c, src);
        if name.is_empty() || !seen.insert(name.clone()) {
            continue;
        }
        names.push(name);
    }
    names
}

// A flat "namespace.name" id collides two ways a real C# id never does:
// nested types (is `Outer` a namespace segment or a type?) and an unrelated
// namespace-level type sharing a dotted path with someone else's nested
// type. Nested types get the CLR's own answer: joined with `+` onto their
// enclosing type chain, never `.`.
fn type_id(name: &str, ns: &str, type_stack: &[String]) -> String {
    if !type_stack.is_empty() {
        let prefix = if ns.is_empty() {
            String::new()
        } else {
            format!("{ns}.")
        };
        format!("{prefix}{}+{name}", type_stack.join("+"))
    } else if !ns.is_empty() {
        format!("{ns}.{name}")
    } else {
        name.to_string()
    }
}

// Every member this type declares: methods regardless of
// accessibility, properties, fields and events, each with the line its own
// name sits on. No accessibility filter and no dedup -- two overloads are two
// declarations at two lines, and both are answers.
fn record_declared_members(node: Node, owner_id: &str, src: &[u8], names: &mut Vec<NameRecord>) {
    let Some(body) = node.child_by_field_name("body") else {
        return;
    };
    for c in named_children(body) {
        match c.kind() {
            "method_declaration" => push_declared_name(
                c.child_by_field_name("name"),
                "method",
                owner_id,
                src,
                names,
            ),
            "property_declaration" => push_declared_name(
                c.child_by_field_name("name"),
                "property",
                owner_id,
                src,
                names,
            ),
            "event_declaration" => {
                push_declared_name(c.child_by_field_name("name"), "event", owner_id, src, names)
            }
            "field_declaration" => push_declarators(c, "field", owner_id, src, names),
            "event_field_declaration" => push_declarators(c, "event", owner_id, src, names),
            _ => {}
        }
    }
}

fn push_declared_name(
    name_node: Option<Node>,
    kind: &str,
    owner_id: &str,
    src: &[u8],
    names: &mut Vec<NameRecord>,
) {
    let Some(name_node) = name_node else {
        return;
    };
    let name = text(name_node, src);
    if name.is_empty() {
        return;
    }
    names.push(NameRecord {
        name,
        kind: kind.to_string(),
        line: name_node.start_position().row + 1,
        owner: owner_id.to_string(),
    });
}

fn push_declarators(
    node: Node,
    kind: &str,
    owner_id: &str,
    src: &[u8],
    names: &mut Vec<NameRecord>,
) {
    let Some(vd) = named_children(node)
        .into_iter()
        .find(|k| k.kind() == "variable_declaration")
    else {
        return;
    };
    for decl in named_children(vd) {
        if decl.kind() != "variable_declarator" {
            continue;
        }
        push_declared_name(decl.child_by_field_name("name"), kind, owner_id, src, names);
    }
}

// `type_params` is the declaring type's own type-parameter set --
// empty (`&EMPTY_TYPE_PARAMS`, a shared static) for the two callers that never
// carry any in this grammar's terms, enum and delegate declarations, so this
// function never has to branch on caller identity to know what to pass.
fn record_type_def(
    node: Node,
    ns: &str,
    kind: &str,
    type_stack: &[String],
    src: &[u8],
    defs: &mut Vec<DefRecord>,
    names: &mut Vec<NameRecord>,
    type_params: &HashSet<String>,
) {
    let name = declared_name(node, src);
    if name.is_empty() {
        return;
    }
    let id = type_id(&name, ns, type_stack);
    record_declared_members(node, &id, src, names);
    // Field order is significant (graph.rs's FragDef serializes these bytes):
    // id, name, namespace, kind, line, methods, then the member-fact additions
    // appended LAST in declaration order -- properties, fields, methodReturns
    // -- then extensionMethods and (for the inheritance veto) bases, then
    // type_params and base_generic_args, then testMethods, then propertyTypes.
    // Each is omitted when empty, so a type with none of them serializes
    // exactly as it did before those additions.
    defs.push(DefRecord {
        id,
        name,
        namespace: ns.to_string(),
        kind: kind.to_string(),
        line: node.start_position().row + 1,
        methods: raw_method_names(node, src, kind),
        properties: raw_property_names(node, src),
        fields: raw_field_names(node, src),
        method_returns: raw_method_returns(node, src, kind),
        extension_methods: raw_extension_methods(node, src),
        bases: raw_base_names(node, src),
        type_params: type_parameter_names_ordered(node, src),
        base_generic_args: raw_base_generic_args(node, src, type_params),
        test_methods: raw_test_methods(node, src, kind),
        property_types: raw_property_types(node, src, type_params),
        end_line: node.end_position().row + 1,
    });
}

// Enum members are cheap to record on the same walk. Each member becomes
// its own def, id'd as "<EnumId>.<Member>" -- appending with "." even when
// EnumId itself carries a "+"-joined nested-type suffix, so a member id can
// never collide with the "+"-joined nested-type scheme.
fn record_enum_members(
    node: Node,
    ns: &str,
    type_stack: &[String],
    src: &[u8],
    defs: &mut Vec<DefRecord>,
) {
    let name = declared_name(node, src);
    if name.is_empty() {
        return;
    }
    let enum_id = type_id(&name, ns, type_stack);
    let Some(body) = node.child_by_field_name("body") else {
        return;
    };
    for member in named_children(body) {
        if member.kind() != "enum_member_declaration" {
            continue;
        }
        let member_name = declared_name(member, src);
        if member_name.is_empty() {
            continue;
        }
        defs.push(DefRecord {
            id: format!("{enum_id}.{member_name}"),
            name: member_name,
            namespace: ns.to_string(),
            kind: "enum-member".to_string(),
            line: member.start_position().row + 1,
            methods: Vec::new(),
            properties: Vec::new(),
            fields: Vec::new(),
            method_returns: Vec::new(),
            extension_methods: Vec::new(),
            bases: Vec::new(),
            type_params: Vec::new(),
            base_generic_args: Vec::new(),
            test_methods: Vec::new(),
            property_types: Vec::new(),
            end_line: member.end_position().row + 1,
        });
    }
}

fn record_using(node: Node, src: &[u8], usings: &mut Vec<UsingRecord>, refs: &mut Vec<RefRecord>) {
    // `global using ...;` is parsed with a leading unnamed `global` token --
    // it's not a field, so child_by_field_name can't see it; child(0) can.
    let is_global = node.child(0).map(|n| n.kind()) == Some("global");
    // Covers `using X.Y;`, `using static X.Y.Z;`, and `using Alias = X.Y.Z;`
    // -- in every case the last named child is the actual imported path.
    // Alias directives (exactly 2 named children: [aliasName, target]) are
    // kept distinct from plain/static imports because resolution
    // short-circuits on them instead of treating the alias name as an
    // ordinary using.
    let kids = named_children(node);
    let Some(target) = kids.last() else {
        return;
    };
    let text_val = text(*target, src).trim().to_string();
    if text_val.is_empty() {
        return;
    }
    if kids.len() == 2 {
        let alias_name = text(kids[0], src).trim().to_string();
        if !alias_name.is_empty() {
            usings.push(UsingRecord::Alias {
                alias: alias_name,
                target: text_val.clone(),
                global: is_global,
            });
        }
    } else {
        usings.push(UsingRecord::Plain {
            text: text_val.clone(),
            global: is_global,
        });
    }
    push_ref(
        refs,
        "imports",
        text_val,
        node.start_position().row + 1,
        None,
        None,
        &[],
    );
}

// ---------------------------------------------------------------------------
// Receiver facts.
//
// Two flat name->type tables per member ref: one for the enclosing TYPE
// (fields + primary-constructor parameters, which are in scope for the whole
// body) and one for the enclosing MEMBER declaration (its parameters and
// every local declared anywhere inside it). The member table shadows the type
// table: a local or parameter (innermost) wins over a field.
//
// The member table is deliberately FLAT -- a permitted simplification -- so
// two sibling blocks each declaring `x` with a different type
// collapse to one CONFLICTED entry rather than to a per-block answer. A
// conflicted entry, and an entry whose type yields no fact at all
// (`var x = SomeCall()`, `string s`), are both stored as `None`: the name is
// taken, and nothing vouches for its type, so no fact is produced. Storing
// them rather than omitting them is what keeps a local from silently falling
// through to a same-named field of a different type.
// ---------------------------------------------------------------------------

/// One receiver fact: the declared type NAME plus that type's top-level
/// type-argument descriptors when it carried any. Two facts agree only when
/// BOTH halves do, so `Box<int> a` and `Box<string> a` in sibling blocks
/// conflict exactly like two different type names would.
#[derive(Clone, PartialEq, Eq)]
pub struct Fact {
    /// The type name value.
    pub type_name: String,
    /// The args value.
    pub args: Option<Vec<String>>,
    /// When set, the name's type is whatever the method of this name returns
    /// on the type `type_name` stands for, a lookup only the resolver can do.
    /// Part of the equality the table compares, so `var x = A.Make()` and
    /// `var x = A.Build()` in sibling blocks conflict exactly like two
    /// different type names would.
    pub call: Option<String>,
}

// name -> `Some(fact)` when exactly one fact vouches for it, `None` when the
// name is taken but nothing does. Both the table and its "taken but unknown"
// entries live in one map; the `Option<Fact>` value is that empty slot.
type FactTable = HashMap<String, Option<Fact>>;

fn add_fact(table: &mut FactTable, name: Option<String>, fact: Option<Fact>) {
    let Some(name) = name.filter(|n| !n.is_empty()) else {
        return;
    };
    match table.get(&name) {
        None => {
            table.insert(name, fact);
        }
        Some(existing) => {
            if *existing != fact {
                table.insert(name, None);
            }
        }
    }
}

// The declared-type half stays `keep_predefined = false` --
// a `string s` local still vouches for nothing.
fn type_fact(type_node: Option<Node>, src: &[u8], type_params: &HashSet<String>) -> Option<Fact> {
    let type_name = base_type_identifier(type_node, src, false)?;
    let args = generic_arg_descriptors(type_node, src, type_params);
    Some(Fact {
        type_name,
        args,
        call: None,
    })
}

// `var x = new T(...)` -- the ONLY shape where an initializer is consulted.
// An explicitly typed declaration is answered by its own type node, so
// `object o = new Widget()` records `object` (a predefined type: no fact),
// never `Widget`.
fn new_expression_fact(
    declarator: Node,
    src: &[u8],
    type_params: &HashSet<String>,
) -> Option<Fact> {
    let init = named_children(declarator)
        .into_iter()
        .find(|c| c.kind() == "object_creation_expression")?;
    type_fact(init.child_by_field_name("type"), src, type_params)
}

// Fields and primary-constructor parameters of one type declaration. Direct
// children only: a nested type gets its OWN table, never the enclosing
// type's, because a nested type cannot reach an outer instance field.
fn collect_type_facts(node: Node, src: &[u8], type_params: &HashSet<String>) -> FactTable {
    let mut table = FactTable::new();
    for kid in named_children(node) {
        if kid.kind() != "parameter_list" {
            continue;
        }
        for p in named_children(kid) {
            if p.kind() != "parameter" {
                continue;
            }
            add_fact(
                &mut table,
                p.child_by_field_name("name").map(|n| text(n, src)),
                type_fact(p.child_by_field_name("type"), src, type_params),
            );
        }
    }
    let Some(body) = node.child_by_field_name("body") else {
        return table;
    };
    collect_declared_member_facts(&named_children(body), src, type_params, &mut table);
    table
}

// The declared-member half of `collect_type_facts`, over a plain member slice
// -- shared with the ERROR-recovery path, where the same declarations exist as
// flat siblings because the parser never built the type node that would have
// wrapped them. A property's declared type vouches for its own name
// exactly like a field's does: one table, one conflict rule, and an indexer is
// a different grammar node so it is excluded by construction.
fn collect_declared_member_facts(
    members: &[Node],
    src: &[u8],
    type_params: &HashSet<String>,
    table: &mut FactTable,
) {
    for c in members {
        if c.kind() == "property_declaration" {
            let name = declared_name(*c, src);
            add_fact(
                table,
                Some(name),
                type_fact(c.child_by_field_name("type"), src, type_params),
            );
            continue;
        }
        if c.kind() != "field_declaration" {
            continue;
        }
        let Some(vd) = named_children(*c)
            .into_iter()
            .find(|k| k.kind() == "variable_declaration")
        else {
            continue;
        };
        let fact = type_fact(vd.child_by_field_name("type"), src, type_params);
        for decl in named_children(vd) {
            if decl.kind() != "variable_declarator" {
                continue;
            }
            add_fact(
                table,
                decl.child_by_field_name("name").map(|n| text(n, src)),
                fact.clone(),
            );
        }
    }
}

// Every `parameter` (explicitly typed lambda parameters included -- an
// implicit_parameter has no type node and therefore no fact) and every
// `variable_declaration` in the member's whole subtree, including the ones
// inside nested lambdas and local functions. One pass, one table.
//
// `class_type_params` is the enclosing type's parameter set; this member's own
// are unioned onto it, which is what turns
// `EventPipelineBinder<FutureState, TM>` into ["FutureState", "*"] when TM is
// the method's own parameter. A local function's own type parameters are
// deliberately NOT unioned in -- its locals land in this same flat table, and
// a local-function parameter used as a type argument records its literal name
// instead, which simply fails to unify. Narrower than the language, never
// wider.
fn collect_member_facts(
    node: Node,
    src: &[u8],
    class_type_params: &HashSet<String>,
    type_facts: &FactTable,
) -> FactTable {
    let mut type_params = class_type_params.clone();
    type_params.extend(type_parameter_names(node, src));
    let mut table = FactTable::new();
    // `var x = Q.M(...)` is settled in a SECOND pass over the
    // collected declarations rather than during the walk: `Q`'s own fact has to
    // be FINAL before it can be read, and the table is flat, so a sibling block
    // declaring `Q` differently cancels it to no fact at all.
    let mut deferred: Vec<DeferredCall> = Vec::new();
    // `foreach (var item in collection)` needs a second pass too:
    // the collection may be a local declared anywhere in this same flat
    // table, including one declared AFTER this foreach in source order.
    let mut deferred_foreach: Vec<DeferredForeach> = Vec::new();
    visit_member_facts(
        node,
        src,
        &type_params,
        &mut table,
        &mut deferred,
        &mut deferred_foreach,
    );
    if !deferred.is_empty() {
        let pending: HashSet<&str> = deferred.iter().map(|d| d.name.as_str()).collect();
        for d in &deferred {
            // A qualifier that is itself one of these locals is refused rather
            // than read from a half-settled table: one hop, never a chain. The
            // refusal still stores the name as TAKEN, which is what keeps it
            // from falling through to a same-named field of a different type.
            let owner = if pending.contains(d.qualifier.as_str()) {
                None
            } else {
                qualifier_type_name(&table, type_facts, &d.qualifier)
            };
            let fact = owner.map(|type_name| Fact {
                type_name,
                args: None,
                call: Some(d.member.clone()),
            });
            add_fact(&mut table, Some(d.name.clone()), fact);
        }
    }
    for d in &deferred_foreach {
        // Unlike the call-owner qualifier above, no explicit
        // "one hop, never a chain" refusal is needed for the collection: the
        // derived fact never carries a type argument (see
        // `collection_element_fact`), so reading a SIBLING foreach variable
        // that has not settled yet -- or that settled to no fact at all --
        // both read as "no single type argument", the same refusal an
        // ordinary unresolvable collection gets. Order among these entries
        // therefore cannot change the answer.
        let fact = collection_element_fact(&table, type_facts, &d.collection);
        add_fact(&mut table, Some(d.name.clone()), fact);
    }
    table
}

// One `var x = Q.M(...)` local awaiting the second pass: the local's name, and
// the two halves of the callee its type depends on.
struct DeferredCall {
    name: String,
    qualifier: String,
    member: String,
}

// One `foreach (var item in collection)` local awaiting the
// second pass: the loop variable's name, and the bare identifier of the
// collection its element type depends on.
struct DeferredForeach {
    name: String,
    collection: String,
}

fn visit_member_facts(
    n: Node,
    src: &[u8],
    type_params: &HashSet<String>,
    table: &mut FactTable,
    deferred: &mut Vec<DeferredCall>,
    deferred_foreach: &mut Vec<DeferredForeach>,
) {
    if n.kind() == "parameter" {
        add_fact(
            table,
            n.child_by_field_name("name").map(|x| text(x, src)),
            type_fact(n.child_by_field_name("type"), src, type_params),
        );
    } else if n.kind() == "variable_declaration" {
        let type_node = n.child_by_field_name("type");
        let is_var = type_node.map(|t| t.kind()) == Some("implicit_type");
        let declared = if is_var {
            None
        } else {
            type_fact(type_node, src, type_params)
        };
        for decl in named_children(n) {
            if decl.kind() != "variable_declarator" {
                continue;
            }
            let name = decl.child_by_field_name("name").map(|x| text(x, src));
            let fact = if is_var {
                new_expression_fact(decl, src, type_params)
            } else {
                declared.clone()
            };
            let call = match (is_var && fact.is_none(), name.as_deref()) {
                (true, Some(n)) if !n.is_empty() => invocation_call(decl, src),
                _ => None,
            };
            match call {
                Some((qualifier, member)) => deferred.push(DeferredCall {
                    name: name.unwrap_or_default(),
                    qualifier,
                    member,
                }),
                None => add_fact(table, name, fact),
            }
        }
    } else if n.kind() == "foreach_statement" {
        // foreach_statement carries its own `type`/`left` fields,
        // not a variable_declaration node, so it needs its own rule: an
        // explicitly typed loop variable is answered by that type node
        // exactly like any other declaration; a `var` loop variable is
        // answered by the COLLECTION's own fact, when that fact carries
        // exactly one top-level type argument (settled below, second pass);
        // anything else stays taken-but-unknown. A destructuring
        // `foreach (var (a, b) in ...)` has no single name to record and is
        // left alone entirely -- neither a fact nor a taken slot.
        let left = n.child_by_field_name("left");
        let name = left
            .filter(|l| l.kind() == "identifier")
            .map(|l| text(l, src));
        if let Some(name) = name {
            let type_node = n.child_by_field_name("type");
            let is_var = type_node
                .map(|t| t.kind() == "implicit_type")
                .unwrap_or(true);
            if !is_var {
                add_fact(table, Some(name), type_fact(type_node, src, type_params));
            } else {
                let right = n.child_by_field_name("right");
                match right.filter(|r| r.kind() == "identifier") {
                    Some(r) => deferred_foreach.push(DeferredForeach {
                        name,
                        collection: text(r, src),
                    }),
                    None => add_fact(table, Some(name), None),
                }
            }
        }
    }
    for c in named_children(n) {
        visit_member_facts(c, src, type_params, table, deferred, deferred_foreach);
    }
}

// The (qualifier, member) halves of a `var x = Q.M(...)`
// initializer, or `None` for every other shape. The qualifier must be BARE and
// non-generic for the same reason a receiver fact's is: a dotted or computed
// qualifier is not a name the ladder can put a type behind. A bare call
// (`var x = M()`) has no qualifier at all and is deliberately not covered.
fn invocation_call(declarator: Node, src: &[u8]) -> Option<(String, String)> {
    let init = named_children(declarator)
        .into_iter()
        .find(|c| c.kind() == "invocation_expression")?;
    let function = init.child_by_field_name("function")?;
    if function.kind() != "member_access_expression" {
        return None;
    }
    let (qualifier, generic) =
        member_qualifier_info(function.child_by_field_name("expression"), src)?;
    if generic || qualifier.contains('.') {
        return None;
    }
    let name_node = function.child_by_field_name("name")?;
    let member = if name_node.kind() == "generic_name" {
        named_children(name_node)
            .into_iter()
            .find(|c| c.kind() == "identifier")
            .map(|id| text(id, src))?
    } else {
        text(name_node, src)
    };
    if member.is_empty() {
        return None;
    }
    Some((qualifier, member))
}

// The type NAME a bare qualifier stands for, as far as the file can vouch: the
// fact's own type when one vouches for the name; `None` when the name is taken
// but nothing vouches for it, or when what vouches is itself a call
// fact (one hop, never a chain); and the text itself when no declaration in
// scope claims the name at all -- an unclaimed bare qualifier is a type name,
// which is the static-call shape.
fn qualifier_type_name(locals: &FactTable, type_facts: &FactTable, name: &str) -> Option<String> {
    match locals.get(name).or_else(|| type_facts.get(name)) {
        None => Some(name.to_string()),
        Some(Some(fact)) if fact.call.is_none() => Some(fact.type_name.clone()),
        Some(_) => None,
    }
}

// The collection's OWN fact, read the same two tables
// `qualifier_type_name` reads, but never falling back to the bare name as a
// type: an unclaimed identifier is nobody's collection, not a static type to
// guess with. A single top-level type argument is the only shape that
// vouches for an element type; a wildcard (the enclosing declaration's own
// type parameter) vouches for nothing here either, because nothing at this
// site knows what it is bound to.
fn collection_element_fact(locals: &FactTable, type_facts: &FactTable, name: &str) -> Option<Fact> {
    match locals.get(name).or_else(|| type_facts.get(name)) {
        Some(Some(fact)) => match fact.args.as_deref() {
            Some([arg]) if arg != "*" => Some(Fact {
                type_name: arg.clone(),
                args: None,
                call: None,
            }),
            _ => None,
        },
        _ => None,
    }
}

// Declarations that own a body a local can be declared in. A local function
// is deliberately NOT here: its locals belong to the enclosing member's flat
// table (collect_member_facts already walked into it), and giving it its own
// scope would hide the enclosing method's locals from it.
const MEMBER_SCOPE_NODES: &[&str] = &[
    "method_declaration",
    "constructor_declaration",
    "destructor_declaration",
    "operator_declaration",
    "conversion_operator_declaration",
    "property_declaration",
    "indexer_declaration",
];

// The enclosing type's field/primary-ctor table plus, inside a member
// declaration, that member's own local/param table layered over it.
struct Scope<'a> {
    /// Shared by reference with every member scope opened under the same
    /// type declaration.
    type_facts: Rc<FactTable>,
    /// The enclosing type declaration's own type-parameter names, shared the
    /// same way. A member scope unions its own on top when it builds its
    /// member table (see `collect_member_facts`).
    type_params: Rc<HashSet<String>>,
    /// `None` at type-body level: a type body is not a member body.
    node: Option<Node<'a>>,
    /// Built on FIRST USE, not on scope entry: most member declarations in a
    /// real file contain no bare-identifier member access at all, and the
    /// scan is a second traversal of the member's subtree. Laziness is
    /// invisible in the output -- the same table, only built on demand.
    member_facts: OnceCell<Option<FactTable>>,
}

impl<'a> Scope<'a> {
    fn root() -> Self {
        Scope {
            type_facts: Rc::new(FactTable::new()),
            type_params: Rc::new(HashSet::new()),
            node: None,
            member_facts: OnceCell::new(),
        }
    }

    fn for_type(node: Node<'a>, src: &[u8]) -> Self {
        let type_params = type_parameter_names(node, src);
        Scope {
            type_facts: Rc::new(collect_type_facts(node, src, &type_params)),
            type_params: Rc::new(type_params),
            node: None,
            member_facts: OnceCell::new(),
        }
    }

    fn for_member(&self, node: Node<'a>) -> Self {
        Scope {
            type_facts: Rc::clone(&self.type_facts),
            type_params: Rc::clone(&self.type_params),
            node: Some(node),
            member_facts: OnceCell::new(),
        }
    }

    // A member-table entry answers even when its
    // value is `None` -- a local whose type nothing vouches for must NOT
    // fall through to a same-named field of a different type.
    fn receiver_fact_for(&self, name: &str, src: &[u8]) -> Option<Fact> {
        let locals = self.member_facts.get_or_init(|| {
            self.node
                .map(|n| collect_member_facts(n, src, &self.type_params, &self.type_facts))
        });
        if let Some(table) = locals {
            if let Some(found) = table.get(name) {
                return found.clone();
            }
        }
        self.type_facts.get(name).cloned().flatten()
    }
}

// walk_list mutates its local `ns` mid-iteration for a FILE-SCOPED
// namespace (`namespace X;`): tree-sitter parses its declared name as the
// node's only child, and every member that follows is a SIBLING of that
// node under the same parent, not a descendant of it. So the namespace
// switch happens while iterating a sibling list, updating ns for the
// remaining siblings in that same list -- a per-node match arm can't
// express that, hence walk_list and walk staying separate functions.
fn walk_list<'a>(
    nodes: Vec<Node<'a>>,
    mut ns: String,
    type_stack: &[String],
    src: &[u8],
    out: &mut Extraction,
    scope: &Scope<'a>,
) {
    for node in nodes {
        if node.kind() == "file_scoped_namespace_declaration" {
            let name = node
                .child_by_field_name("name")
                .map(|n| text(n, src))
                .unwrap_or_default();
            ns = if ns.is_empty() {
                name
            } else {
                format!("{ns}.{name}")
            };
            continue;
        }
        walk(node, &ns, type_stack, src, out, scope);
    }
}

// type_stack (see type_id) also doubles as the "am I inside a type" signal
// for nothing else -- ref extraction runs at every depth regardless of
// nesting.
fn walk<'a>(
    node: Node<'a>,
    ns: &str,
    type_stack: &[String],
    src: &[u8],
    out: &mut Extraction,
    scope: &Scope<'a>,
) {
    // Entering a member declaration installs a fresh (lazily built)
    // local/param table over the enclosing type's field table, for this
    // subtree only.
    let member_scope;
    let scope: &Scope<'a> = if MEMBER_SCOPE_NODES.contains(&node.kind()) {
        member_scope = scope.for_member(node);
        &member_scope
    } else {
        scope
    };
    match node.kind() {
        "namespace_declaration" => {
            let name = node
                .child_by_field_name("name")
                .map(|n| text(n, src))
                .unwrap_or_default();
            let new_ns = if ns.is_empty() {
                name
            } else {
                format!("{ns}.{name}")
            };
            walk_list(named_children(node), new_ns, type_stack, src, out, scope);
        }
        "using_directive" => {
            record_using(node, src, &mut out.usings, &mut out.refs);
        }
        "class_declaration"
        | "interface_declaration"
        | "struct_declaration"
        | "record_declaration" => {
            let kind = type_kind_label(node.kind()).expect("matched TYPE_KINDS arm");
            // Computed before record_type_def (Scope::for_type
            // below recomputes its own copy for the type-facts table; cheap
            // and kept separate rather than threading one instance through
            // both call sites).
            let type_params = type_parameter_names(node, src);
            record_type_def(
                node,
                ns,
                kind,
                type_stack,
                src,
                &mut out.defs,
                &mut out.names,
                &type_params,
            );
            record_base_list(node, ns, type_stack, src, &mut out.refs);
            let name = declared_name(node, src);
            let new_stack: Vec<String> = if name.is_empty() {
                type_stack.to_vec()
            } else {
                let mut s = type_stack.to_vec();
                s.push(name);
                s
            };
            // A type declaration opens a new field/primary-ctor table and
            // closes any enclosing member scope (`node: None` -- a type body
            // is not a member body).
            let type_scope = Scope::for_type(node, src);
            walk_list(
                named_children(node),
                ns.to_string(),
                &new_stack,
                src,
                out,
                &type_scope,
            );
        }
        "enum_declaration" => {
            record_type_def(
                node,
                ns,
                "enum",
                type_stack,
                src,
                &mut out.defs,
                &mut out.names,
                &HashSet::new(),
            );
            record_enum_members(node, ns, type_stack, src, &mut out.defs);
            // No recursion into the enum body: enum member initializer
            // expressions are not walked.
        }
        "delegate_declaration" => {
            record_type_def(
                node,
                ns,
                "delegate",
                type_stack,
                src,
                &mut out.defs,
                &mut out.names,
                &HashSet::new(),
            );
            record_single_type(
                node.child_by_field_name("type"),
                "uses-type",
                ns,
                type_stack,
                src,
                &mut out.refs,
            );
            walk_list(
                named_children(node),
                ns.to_string(),
                type_stack,
                src,
                out,
                scope,
            );
        }
        "field_declaration" => {
            let vd = named_children(node)
                .into_iter()
                .find(|c| c.kind() == "variable_declaration");
            record_single_type(
                vd.and_then(|v| v.child_by_field_name("type")),
                "uses-type",
                ns,
                type_stack,
                src,
                &mut out.refs,
            );
            walk_list(
                named_children(node),
                ns.to_string(),
                type_stack,
                src,
                out,
                scope,
            );
        }
        "property_declaration" => {
            record_single_type(
                node.child_by_field_name("type"),
                "uses-type",
                ns,
                type_stack,
                src,
                &mut out.refs,
            );
            walk_list(
                named_children(node),
                ns.to_string(),
                type_stack,
                src,
                out,
                scope,
            );
        }
        "parameter" => {
            record_single_type(
                node.child_by_field_name("type"),
                "uses-type",
                ns,
                type_stack,
                src,
                &mut out.refs,
            );
            walk_list(
                named_children(node),
                ns.to_string(),
                type_stack,
                src,
                out,
                scope,
            );
        }
        // One 'ctor-param' fact per constructor parameter,
        // ALONGSIDE (never instead of) the plain 'uses-type' ref the
        // "parameter" arm above still emits as walk_list recurses into this
        // node's own parameter_list below. Uses the same type_fact helper
        // stage-2 field/local receiver facts use, so a parameter's generic
        // arguments survive as descriptors instead of being stripped to a
        // bare name the way the general 'uses-type' ladder strips them.
        // scope.type_params is already the enclosing TYPE's own parameters
        // here (MEMBER_SCOPE_NODES swapped in a member scope above with the
        // SAME type_params -- a C# constructor cannot declare type
        // parameters of its own).
        "constructor_declaration" => {
            if let Some(params) = node.child_by_field_name("parameters") {
                for p in named_children(params) {
                    if p.kind() != "parameter" {
                        continue;
                    }
                    let type_node = p.child_by_field_name("type");
                    if let Some(fact) = type_fact(type_node, src, &scope.type_params) {
                        let line = type_node.map(|t| t.start_position().row + 1).unwrap_or(0);
                        push_ctor_param_ref(
                            &mut out.refs,
                            fact.type_name,
                            line,
                            ns.to_string(),
                            fact.args,
                            type_stack,
                        );
                    }
                }
            }
            walk_list(
                named_children(node),
                ns.to_string(),
                type_stack,
                src,
                out,
                scope,
            );
        }
        // `Method(out SomeEnum x)` / `obj is SomeEnum x` inline-declaration
        // sites -- a `type` + `name` pair, same shape as `parameter`, just in
        // expression position instead of a parameter list. Same
        // record_single_type pipeline, so it goes through the normal ladder
        // including ambiguous marking, exactly like an ordinary type usage.
        "declaration_expression" => {
            record_single_type(
                node.child_by_field_name("type"),
                "uses-type",
                ns,
                type_stack,
                src,
                &mut out.refs,
            );
            walk_list(
                named_children(node),
                ns.to_string(),
                type_stack,
                src,
                out,
                scope,
            );
        }
        "method_declaration" => {
            record_single_type(
                node.child_by_field_name("returns"),
                "uses-type",
                ns,
                type_stack,
                src,
                &mut out.refs,
            );
            walk_list(
                named_children(node),
                ns.to_string(),
                type_stack,
                src,
                out,
                scope,
            );
        }
        "object_creation_expression" => {
            record_single_type(
                node.child_by_field_name("type"),
                "uses-type",
                ns,
                type_stack,
                src,
                &mut out.refs,
            );
            walk_list(
                named_children(node),
                ns.to_string(),
                type_stack,
                src,
                out,
                scope,
            );
        }
        "typeof_expression" => {
            record_single_type(
                node.child_by_field_name("type"),
                "uses-type",
                ns,
                type_stack,
                src,
                &mut out.refs,
            );
            walk_list(
                named_children(node),
                ns.to_string(),
                type_stack,
                src,
                out,
                scope,
            );
        }
        // Covers every position a member access can appear in -- cast-to-int
        // values over a work-type enum, argument lists, switch/pattern arms,
        // initializers -- because none of those container node types are
        // special-cased above, so they all reach here via the default walk.
        "member_access_expression" => {
            let expr_field = node.child_by_field_name("expression");
            // The member itself can be a generic_name too ("Foo.Bar<T>(...)"):
            // normalize to the bare method name so the resolver's method-list
            // membership check sees the name the def actually recorded.
            let member = node.child_by_field_name("name").map(|n| {
                if n.kind() == "generic_name" {
                    named_children(n)
                        .into_iter()
                        .find(|c| c.kind() == "identifier")
                        .map(|id| text(id, src))
                        .unwrap_or_default()
                } else {
                    text(n, src)
                }
            });
            let normal_qualifier = member_qualifier_info(expr_field, src);
            if let (Some((qt, generic)), Some(m)) = (&normal_qualifier, &member) {
                if !m.is_empty() {
                    // A receiver fact is asked for ONLY for a bare,
                    // non-generic qualifier: a dotted qualifier is a
                    // flattened chain window (whose head's fact it must never
                    // inherit) or a namespace path, and a type-argument list
                    // is syntax no local, parameter, or field can carry.
                    let dot_at = if *generic { None } else { qt.find('.') };
                    let bare = dot_at.is_none() && !*generic;
                    let receiver = if bare {
                        scope.receiver_fact_for(qt, src)
                    } else {
                        None
                    };
                    // The head of a TWO-segment chain, and only when
                    // the scope vouches for its type: "a.Settings" asks what
                    // `a` is, while "x.y.Settings" and a namespace path ask
                    // nothing, because a head this file cannot type is a head
                    // no property lookup can start from.
                    let property_owner = dot_at
                        .filter(|d| !qt[d + 1..].contains('.'))
                        .and_then(|d| scope.receiver_fact_for(&qt[..d], src))
                        .filter(|fact| fact.call.is_none())
                        .map(|fact| fact.type_name);
                    push_member_ref(
                        &mut out.refs,
                        qt,
                        m.clone(),
                        node.start_position().row + 1,
                        ns.to_string(),
                        *generic,
                        receiver,
                        // Asked of THIS node, so a chain window answers for its
                        // own call and never for the one wrapping it.
                        invocation_arg_count(node),
                        type_stack,
                        property_owner,
                    );
                }
            }
            walk_list(
                named_children(node),
                ns.to_string(),
                type_stack,
                src,
                out,
                scope,
            );
        }
        "type_argument_list" => {
            for arg in named_children(node) {
                record_single_type(Some(arg), "uses-type", ns, type_stack, src, &mut out.refs);
            }
            walk_list(
                named_children(node),
                ns.to_string(),
                type_stack,
                src,
                out,
                scope,
            );
        }
        _ => {
            walk_list(
                named_children(node),
                ns.to_string(),
                type_stack,
                src,
                out,
                scope,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Entry points.
// ---------------------------------------------------------------------------

/// Extract both outputs from one parse of C# `source`: the file's purpose
/// signature and its graph fragment (defs, usings, refs, names).
pub fn extract(source: &str) -> Extraction {
    let mut parser = new_parser();
    let units = crate::parse::utf16_units(source);
    let utf16 = crate::parse::utf16_bytes(&units);
    let tree = parser
        .parse_utf16_le(&units, None)
        .expect("parse returned no tree");
    let root = tree.root_node();
    let src = &utf16[..];

    let purpose_parts: Vec<SegmentParts> = namespace_level_types(root)
        .into_iter()
        .filter_map(|n| type_segment_parts(n, src))
        .collect();

    let mut out = Extraction {
        purpose: None,
        defs: Vec::new(),
        usings: Vec::new(),
        refs: Vec::new(),
        names: Vec::new(),
    };
    walk_list(
        named_children(root),
        String::new(),
        &[],
        src,
        &mut out,
        &Scope::root(),
    );

    out.purpose = format_segments(purpose_parts);

    out
}

// ---------------------------------------------------------------------------
// `extract-dump` subcommand -- canonical JSON for one file.
// ---------------------------------------------------------------------------

/// Extracts the C# file at `path` and prints the extraction as JSON.
pub fn run_extract_dump(path: &str) {
    let source = fs::read_to_string(path).unwrap_or_else(|err| {
        eprintln!("failed to read {path}: {err}");
        process::exit(1);
    });
    // The dump path dispatches on extension: a C# file yields a fragment read
    // as `{purpose, defs, usings, refs, names}`, while a TS/JS file yields a
    // `ts: 1` REFERENCE fragment carrying `defs` and `refs` but no `usings`
    // and no `names` -- and serialization drops an absent value's key, so the
    // TS dump is a three-key object. The C# dump below keeps all five keys.
    if let Some(grammar) = crate::parse::ts_grammar_for(path) {
        // Compute `root = dirname(path)`, `rel = basename(path)` and go
        // through `extract_ts_file` -- the SAME hybrid-aware worker path
        // `devscout map` uses, not the pure extractor -- so the dump exercises
        // the leading-comment prefix, off one single parse.
        let file_path = Path::new(path);
        let root = file_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let rel = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);
        match extract_ts_file(root, rel, &source, grammar) {
            Some(ts) => println!("{}", ts_extraction_to_json(&ts.purpose, &ts.fragment)),
            // A parse failure leaves the file out of both outputs, so its
            // dump is the empty fragment under a null purpose.
            None => println!("{}", ts_extraction_to_json(&None, &TsFragment::default())),
        }
        return;
    }
    let result = extract(&source);
    println!("{}", extraction_to_json(&result));
}

// Hand-rolled JSON value + pretty printer matching
// `JSON.stringify(value, null, 2)` byte-for-byte (2-space indent, no
// trailing commas, empty arrays/objects collapse to `[]`/`{}` on one line)
// -- no serde dependency, per the same "no new dependency" rule
// parse.rs's spans_json already follows. Generalized here (unlike
// spans_json's fixed six-field record) because extraction records are
// heterogeneous (optional fields, nested arrays of objects).
enum Json {
    Null,
    Bool(bool),
    Num(usize),
    /// A SIGNED number, for the one field that can be negative: an extension
    /// entry's `arityMax`, where -1 is the unbounded-`params` sentinel,
    /// written as a plain JSON number.
    Int(i64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(&'static str, Json)>),
    /// Same encoding as `Obj`, for the one record whose keys are DATA rather
    /// than a fixed schema: a def's `methodReturns`. Insertion order is
    /// significant (first-declaration source order), so this is a Vec of
    /// pairs, never a sorted map.
    Map(Vec<(String, Json)>),
}

impl Json {
    fn to_pretty_string(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }

    fn write(&self, out: &mut String, indent: usize) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => out.push_str(&n.to_string()),
            Json::Int(n) => out.push_str(&n.to_string()),
            Json::Str(s) => out.push_str(&json_string(s)),
            Json::Arr(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push_str("[\n");
                let pad = "  ".repeat(indent + 1);
                let last = items.len() - 1;
                for (i, item) in items.iter().enumerate() {
                    out.push_str(&pad);
                    item.write(out, indent + 1);
                    if i != last {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&"  ".repeat(indent));
                out.push(']');
            }
            Json::Obj(fields) => {
                write_object(out, indent, fields.iter().map(|(k, v)| (*k, v)));
            }
            Json::Map(entries) => {
                write_object(out, indent, entries.iter().map(|(k, v)| (k.as_str(), v)));
            }
        }
    }
}

fn write_object<'a>(
    out: &mut String,
    indent: usize,
    fields: impl ExactSizeIterator<Item = (&'a str, &'a Json)>,
) {
    if fields.len() == 0 {
        out.push_str("{}");
        return;
    }
    out.push_str("{\n");
    let pad = "  ".repeat(indent + 1);
    let last = fields.len() - 1;
    for (i, (k, v)) in fields.enumerate() {
        out.push_str(&pad);
        out.push_str(&json_string(k));
        out.push_str(": ");
        v.write(out, indent + 1);
        if i != last {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(&"  ".repeat(indent));
    out.push('}');
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

fn def_to_json(d: &DefRecord) -> Json {
    let mut fields: Vec<(&'static str, Json)> = vec![
        ("id", Json::Str(d.id.clone())),
        ("name", Json::Str(d.name.clone())),
        ("namespace", Json::Str(d.namespace.clone())),
        ("kind", Json::Str(d.kind.clone())),
        ("line", Json::Num(d.line)),
        (
            "methods",
            Json::Arr(d.methods.iter().map(|m| Json::Str(m.clone())).collect()),
        ),
    ];
    // Appended last, in declaration order, each only when non-empty.
    if !d.properties.is_empty() {
        fields.push((
            "properties",
            Json::Arr(d.properties.iter().map(|p| Json::Str(p.clone())).collect()),
        ));
    }
    if !d.fields.is_empty() {
        fields.push((
            "fields",
            Json::Arr(d.fields.iter().map(|f| Json::Str(f.clone())).collect()),
        ));
    }
    if !d.method_returns.is_empty() {
        fields.push((
            "methodReturns",
            Json::Map(
                d.method_returns
                    .iter()
                    .map(|(k, v)| (k.clone(), Json::Str(v.clone())))
                    .collect(),
            ),
        ));
    }
    // Appended after the properties/fields/methodReturns trio, entry keys in
    // the serialized order (name, thisType, arityMin, arityMax, thisArgs),
    // omitted when empty. `thisArgs` is present only on a generic
    // this-parameter.
    if !d.extension_methods.is_empty() {
        fields.push((
            "extensionMethods",
            Json::Arr(
                d.extension_methods
                    .iter()
                    .map(|e| {
                        let mut kv: Vec<(&'static str, Json)> = vec![
                            ("name", Json::Str(e.name.clone())),
                            ("thisType", Json::Str(e.this_type.clone())),
                            ("arityMin", Json::Num(e.arity_min)),
                            ("arityMax", Json::Int(e.arity_max)),
                        ];
                        if let Some(args) = &e.this_args {
                            kv.push((
                                "thisArgs",
                                Json::Arr(args.iter().map(|a| Json::Str(a.clone())).collect()),
                            ));
                        }
                        Json::Obj(kv)
                    })
                    .collect(),
            ),
        ));
    }
    // Appended after extensionMethods.
    if !d.bases.is_empty() {
        fields.push((
            "bases",
            Json::Arr(d.bases.iter().map(|b| Json::Str(b.clone())).collect()),
        ));
    }
    // Appended after bases, before testMethods, each only when non-empty.
    if !d.type_params.is_empty() {
        fields.push((
            "typeParams",
            Json::Arr(d.type_params.iter().map(|t| Json::Str(t.clone())).collect()),
        ));
    }
    if !d.base_generic_args.is_empty() {
        fields.push((
            "baseGenericArgs",
            Json::Map(
                d.base_generic_args
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            Json::Arr(v.iter().map(|a| Json::Str(a.clone())).collect()),
                        )
                    })
                    .collect(),
            ),
        ));
    }
    // Appended after baseGenericArgs.
    if !d.test_methods.is_empty() {
        fields.push((
            "testMethods",
            Json::Arr(
                d.test_methods
                    .iter()
                    .map(|t| Json::Str(t.clone()))
                    .collect(),
            ),
        ));
    }
    // Appended LAST, after testMethods, entry keys in source order.
    if !d.property_types.is_empty() {
        fields.push((
            "propertyTypes",
            Json::Map(
                d.property_types
                    .iter()
                    .map(|(name, fact)| (name.clone(), fact_to_json(fact)))
                    .collect(),
            ),
        ));
    }
    Json::Obj(fields)
}

// One declared type fact: `{type}`, or `{type, args}` when the declared type
// carried a top-level type-argument list. Field order is significant, and
// `args` is omitted when absent.
fn fact_to_json(fact: &Fact) -> Json {
    let mut fields: Vec<(&'static str, Json)> = vec![("type", Json::Str(fact.type_name.clone()))];
    if let Some(args) = &fact.args {
        fields.push((
            "args",
            Json::Arr(args.iter().map(|a| Json::Str(a.clone())).collect()),
        ));
    }
    Json::Obj(fields)
}

fn using_to_json(u: &UsingRecord) -> Json {
    match u {
        UsingRecord::Alias {
            alias,
            target,
            global,
        } => Json::Obj(vec![
            ("alias", Json::Str(alias.clone())),
            ("target", Json::Str(target.clone())),
            ("global", Json::Bool(*global)),
        ]),
        UsingRecord::Plain { text, global } => Json::Obj(vec![
            ("text", Json::Str(text.clone())),
            ("global", Json::Bool(*global)),
        ]),
    }
}

fn ref_to_json(r: &RefRecord) -> Json {
    let mut fields: Vec<(&'static str, Json)> = vec![
        ("kind", Json::Str(r.kind.clone())),
        ("name", Json::Str(r.name.clone())),
    ];
    if let Some(q) = &r.qualified {
        fields.push(("qualified", Json::Str(q.clone())));
    }
    if let Some(m) = &r.member {
        fields.push(("member", Json::Str(m.clone())));
    }
    fields.push(("line", Json::Num(r.line)));
    fields.push((
        "namespace",
        match &r.namespace {
            Some(ns) => Json::Str(ns.clone()),
            None => Json::Null,
        },
    ));
    if let Some(arity) = r.type_arg_count {
        fields.push(("typeArgCount", Json::Num(arity)));
    }
    // Both appended last, in that order, and only when set.
    if r.generic {
        fields.push(("generic", Json::Bool(true)));
    }
    if let Some(rt) = &r.receiver_type {
        fields.push(("receiverType", Json::Str(rt.clone())));
    }
    // Appended after receiverType, and only when the access was a callee. The
    // test is presence, not truthiness: argCount 0 is a real value.
    if let Some(ac) = r.arg_count {
        fields.push(("argCount", Json::Num(ac)));
    }
    // Appended LAST, after argCount, and only
    // when the receiver's DECLARED type was generic.
    if let Some(args) = &r.receiver_args {
        fields.push((
            "receiverArgs",
            Json::Arr(args.iter().map(|a| Json::Str(a.clone())).collect()),
        ));
    }
    // Appended LAST of all, after receiverArgs, and only when the ref sits
    // inside a type.
    if !r.outer_types.is_empty() {
        fields.push((
            "outerTypes",
            Json::Arr(r.outer_types.iter().map(|t| Json::Str(t.clone())).collect()),
        ));
    }
    // Appended LAST of all, after outerTypes, and only set for a 'ctor-param'
    // ref whose type was generic.
    if let Some(args) = &r.args {
        fields.push((
            "args",
            Json::Arr(args.iter().map(|a| Json::Str(a.clone())).collect()),
        ));
    }
    // Appended after args, and only for a two-segment chain whose
    // head the enclosing scope could type.
    if let Some(owner) = &r.receiver_property_owner {
        fields.push(("receiverPropertyOwner", Json::Str(owner.clone())));
    }
    // Appended LAST of all, and always as a pair.
    if let Some(owner) = &r.receiver_call_owner {
        fields.push(("receiverCallOwner", Json::Str(owner.clone())));
    }
    if let Some(member) = &r.receiver_call_member {
        fields.push(("receiverCallMember", Json::Str(member.clone())));
    }
    Json::Obj(fields)
}

// Field order (`name`, `kind`, `line`, `owner`) is significant, `owner`
// omitted when empty.
fn name_to_json(n: &NameRecord) -> Json {
    let mut fields: Vec<(&'static str, Json)> = vec![
        ("name", Json::Str(n.name.clone())),
        ("kind", Json::Str(n.kind.clone())),
        ("line", Json::Num(n.line)),
    ];
    if !n.owner.is_empty() {
        fields.push(("owner", Json::Str(n.owner.clone())));
    }
    Json::Obj(fields)
}

/// Serializes a C# extraction as JSON.
pub fn extraction_to_json(e: &Extraction) -> String {
    let purpose_json = match &e.purpose {
        Some(p) => Json::Str(p.clone()),
        None => Json::Null,
    };
    let root = Json::Obj(vec![
        ("purpose", purpose_json),
        ("defs", Json::Arr(e.defs.iter().map(def_to_json).collect())),
        (
            "usings",
            Json::Arr(e.usings.iter().map(using_to_json).collect()),
        ),
        ("refs", Json::Arr(e.refs.iter().map(ref_to_json).collect())),
        (
            "names",
            Json::Arr(e.names.iter().map(name_to_json).collect()),
        ),
    ]);
    root.to_pretty_string()
}

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
fn ts_declared_name(node: Node, src: &[u8]) -> String {
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
fn ts_pattern_names(node: Option<Node>, src: &[u8]) -> Vec<String> {
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
fn is_default_export_statement(node: Node) -> bool {
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

enum CjsTarget {
    Whole,
    Prop(String),
}

// `module.exports = X` / `module.exports.foo = X` / `exports.foo = X` --
// `Whole` for the first shape, `Prop(name)` for the other two, or `None`
// when `left` isn't one of these three recognised shapes.
fn common_js_export_target(left: Option<Node>, src: &[u8]) -> Option<CjsTarget> {
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
fn ts_purpose_segments(program_node: Node, src: &[u8]) -> Option<String> {
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

// ---------------------------------------------------------------------------
// TS/TSX reference facts (imports, calls, JSX uses, dispatches) and their
// helpers.
//
// Same tree the purpose composition above already parsed, one extra walk, no
// second parse. This section records only what a file SAYS: which specifiers
// it imports and under which local names, which top-level names it exports,
// and which local names it calls / renders as a JSX tag / hands to a
// dispatching call. Nothing here resolves a specifier to a file or a name to
// a declaration -- tsgraph.rs does that across files, the same split
// `extract`/`resolve_graph` already uses for C#.
//
// A TS fragment is tagged `ts: 1` (see `graph::TsFragment`) so the resolver
// split in `resolve_graph` can route it to the TS resolver instead of the C#
// one: the two fragment shapes share no field beyond `defs`, and feeding one
// to the other's resolver would resolve names across languages that have no
// relationship at all.
// ---------------------------------------------------------------------------

/// One exported top-level declaration, in the shape the fragment serializes:
/// `name`, `kind`, `line`, in that order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TsFragmentDef {
    /// The name value.
    pub name: String,
    /// The kind value.
    pub kind: String,
    /// The line value.
    pub line: usize,
    #[serde(rename = "endLine")]
    /// The end line value.
    pub end_line: usize,
}

/// One local name an import statement binds, and the export it names in the
/// source module. `imported` is `"default"` for a default clause and `"*"`
/// for a namespace clause (or a whole-module `require`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TsBinding {
    /// The local value.
    pub local: String,
    /// The imported value.
    pub imported: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Represents `TsImport`.
pub struct TsImport {
    /// The spec value.
    pub spec: String,
    /// The line value.
    pub line: usize,
    /// The bindings value.
    pub bindings: Vec<TsBinding>,
}

/// `export { A as B } from 'm'` re-exports A under the name B: `exported` is
/// what a consumer of THIS file imports, `imported` is what the source module
/// declares.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TsReexportName {
    /// The exported value.
    pub exported: String,
    /// The imported value.
    pub imported: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Represents `TsReexport`.
pub struct TsReexport {
    /// The spec value.
    pub spec: String,
    /// The line value.
    pub line: usize,
    /// The star value.
    pub star: bool,
    /// The names value.
    pub names: Vec<TsReexportName>,
}

/// One recorded reference. Field order (`kind`, `name`, optional `member`,
/// `line`) is significant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TsRef {
    /// The kind value.
    pub kind: String,
    /// The name value.
    pub name: String,
    /// Present only on a qualified reference (`ns.member(...)`,
    /// `<Ns.Thing />`), and serialized between `name` and `line` exactly
    /// where the JS object literal writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    /// The line value.
    pub line: usize,
}

/// The whole per-file fragment, in serialization order: `defs`, `imports`,
/// `reexports`, `refs`, then `default` when the file has one. The `ts: 1` tag
/// itself lives on the serde type (`graph::TsFragment`), which this converts
/// into -- exactly as `extract::Extraction` converts into `graph::Fragment`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TsFragment {
    /// The routing tag, always `1` and always FIRST -- `resolve_graph`'s
    /// door-level split reads it to send this fragment to the TS resolver
    /// instead of the C# one, and `graph::AnyFragment` reads it to tell the
    /// two cached shapes apart. Required on the read side too: a C#/markup
    /// fragment carries no `ts` key at all, which is what makes the untagged
    /// discrimination total.
    pub ts: u8,
    /// The defs value.
    pub defs: Vec<TsFragmentDef>,
    /// The imports value.
    pub imports: Vec<TsImport>,
    /// The reexports value.
    pub reexports: Vec<TsReexport>,
    /// The refs value.
    pub refs: Vec<TsRef>,
    /// Appended LAST and only when the file has one -- the house rule for
    /// every added fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

impl Default for TsFragment {
    fn default() -> Self {
        TsFragment {
            ts: 1,
            defs: Vec::new(),
            imports: Vec::new(),
            reexports: Vec::new(),
            refs: Vec::new(),
            default: None,
        }
    }
}

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

/// Render a TS/JS file's dump-extract JSON. The reader expects
/// `{purpose, defs, usings, refs, names}`, and a TS fragment carries no
/// `usings` and no `names`, so both keys are dropped -- the dump is a
/// three-key object.
pub fn ts_extraction_to_json(purpose: &Option<String>, fragment: &TsFragment) -> String {
    let purpose_json = match purpose {
        Some(p) => Json::Str(p.clone()),
        None => Json::Null,
    };
    let root = Json::Obj(vec![
        ("purpose", purpose_json),
        (
            "defs",
            Json::Arr(
                fragment
                    .defs
                    .iter()
                    .map(|d| {
                        Json::Obj(vec![
                            ("name", Json::Str(d.name.clone())),
                            ("kind", Json::Str(d.kind.clone())),
                            ("line", Json::Num(d.line)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "refs",
            Json::Arr(
                fragment
                    .refs
                    .iter()
                    .map(|r| {
                        let mut fields = vec![
                            ("kind", Json::Str(r.kind.clone())),
                            ("name", Json::Str(r.name.clone())),
                        ];
                        if let Some(m) = &r.member {
                            fields.push(("member", Json::Str(m.clone())));
                        }
                        fields.push(("line", Json::Num(r.line)));
                        Json::Obj(fields)
                    })
                    .collect(),
            ),
        ),
    ]);
    root.to_pretty_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_src(src: &str) -> Extraction {
        extract(src)
    }

    fn find_def<'a>(e: &'a Extraction, id: &str) -> Option<&'a DefRecord> {
        e.defs.iter().find(|d| d.id == id)
    }

    // --- FQN building (type_id / typeStack "+"-joining) ---------------

    #[test]
    fn namespace_level_type_fqn_is_dotted() {
        let e = extract_src("namespace Fixtures.Widgets { public class Gadget {} }");
        assert!(find_def(&e, "Fixtures.Widgets.Gadget").is_some());
    }

    #[test]
    fn top_level_type_fqn_has_no_namespace_prefix() {
        let e = extract_src("public class Gadget {}");
        let d = find_def(&e, "Gadget").expect("Gadget def present");
        assert_eq!(d.namespace, "");
    }

    #[test]
    fn nested_type_fqn_uses_plus_not_dot() {
        let e = extract_src(
            "namespace Fixtures.Widgets { public class Outer { public class Inner {} } }",
        );
        assert!(find_def(&e, "Fixtures.Widgets.Outer+Inner").is_some());
        // The "+"-joined id must never collide with a literal dotted path.
        assert!(find_def(&e, "Fixtures.Widgets.Outer.Inner").is_none());
    }

    #[test]
    fn doubly_nested_type_fqn_chains_plus_joins() {
        let e = extract_src(
            "namespace Fixtures.Widgets { public class A { public class B { public class C {} } } }",
        );
        assert!(find_def(&e, "Fixtures.Widgets.A+B+C").is_some());
    }

    #[test]
    fn enum_member_id_appends_dot_even_under_nested_type() {
        let e = extract_src(
            "namespace Fixtures.Gadgets { public class Controller { public enum State { Off, On } } }",
        );
        assert!(find_def(&e, "Fixtures.Gadgets.Controller+State").is_some());
        assert!(find_def(&e, "Fixtures.Gadgets.Controller+State.Off").is_some());
        assert!(find_def(&e, "Fixtures.Gadgets.Controller+State.On").is_some());
    }

    #[test]
    fn file_scoped_namespace_siblings_get_the_namespace() {
        let e = extract_src("namespace Fixtures.Gadgets;\npublic class Registry {}\npublic enum State { Off, On }\n");
        assert!(find_def(&e, "Fixtures.Gadgets.Registry").is_some());
        assert!(find_def(&e, "Fixtures.Gadgets.State").is_some());
        assert!(find_def(&e, "Fixtures.Gadgets.State.Off").is_some());
    }

    #[test]
    fn nested_regular_namespace_dots_accumulate() {
        let e = extract_src("namespace A { namespace B { public class Widget {} } }");
        assert!(find_def(&e, "A.B.Widget").is_some());
    }

    // --- generic arity (type_argument_list -> one uses-type ref per arg) --

    #[test]
    fn type_def_end_line_is_the_closing_brace_line_of_the_whole_declaration() {
        let e = extract_src("namespace N;\npublic class Widget\n{\n    int n;\n}\n");
        let d = find_def(&e, "N.Widget").unwrap();
        assert_eq!((d.line, d.end_line), (2, 5));
    }

    #[test]
    fn enum_member_end_line_is_its_own_single_line() {
        let e = extract_src("public enum State\n{\n    Off,\n    On,\n}\n");
        assert_eq!(
            (
                find_def(&e, "State.Off").unwrap().line,
                find_def(&e, "State.Off").unwrap().end_line
            ),
            (3, 3)
        );
        assert_eq!(
            (
                find_def(&e, "State.On").unwrap().line,
                find_def(&e, "State.On").unwrap().end_line
            ),
            (4, 4)
        );
    }

    #[test]
    fn nested_type_span_stays_within_its_own_node() {
        let e = extract_src("public class Outer\n{\n    public class Inner { }\n}\n");
        assert_eq!(
            (
                find_def(&e, "Outer").unwrap().line,
                find_def(&e, "Outer").unwrap().end_line
            ),
            (1, 4)
        );
        assert_eq!(
            (
                find_def(&e, "Outer+Inner").unwrap().line,
                find_def(&e, "Outer+Inner").unwrap().end_line
            ),
            (3, 3)
        );
    }

    #[test]
    fn generic_name_records_base_and_each_type_argument() {
        let e = extract_src(
            "using System.Collections.Generic;\nnamespace Fixtures.Generics { public class Store { public Dictionary<Key, Value> Items { get; set; } } public class Key {} public class Value {} }",
        );
        let uses_type_names: Vec<&str> = e
            .refs
            .iter()
            .filter(|r| r.kind == "uses-type")
            .map(|r| r.name.as_str())
            .collect();
        // Dictionary itself (generic_name's base identifier) plus both type
        // arguments (arity 2) -- three distinct uses-type refs total from
        // one property type.
        assert!(uses_type_names.contains(&"Dictionary"));
        assert!(uses_type_names.contains(&"Key"));
        assert!(uses_type_names.contains(&"Value"));
    }

    #[test]
    fn nested_generic_type_arguments_are_all_recorded() {
        let e = extract_src(
            "using System.Collections.Generic;\nnamespace Fixtures.Generics { public class Store { public List<Dictionary<string, Gadget>> Items { get; set; } } public class Gadget {} }",
        );
        let uses_type_names: Vec<&str> = e
            .refs
            .iter()
            .filter(|r| r.kind == "uses-type")
            .map(|r| r.name.as_str())
            .collect();
        assert!(uses_type_names.contains(&"List"));
        assert!(uses_type_names.contains(&"Dictionary"));
        assert!(uses_type_names.contains(&"Gadget"));
        // "string" is a predefined_type -- never a candidate.
        assert!(!uses_type_names.contains(&"string"));
    }

    // --- alias table (using directive parsing) -------------------------

    #[test]
    fn using_alias_directive_captures_alias_and_target() {
        let e = extract_src("using Widgets = Fixtures.Widgets.Catalog;\n");
        assert_eq!(e.usings.len(), 1);
        match &e.usings[0] {
            UsingRecord::Alias {
                alias,
                target,
                global,
            } => {
                assert_eq!(alias, "Widgets");
                assert_eq!(target, "Fixtures.Widgets.Catalog");
                assert!(!global);
            }
            UsingRecord::Plain { .. } => panic!("expected alias form"),
        }
    }

    #[test]
    fn plain_using_directive_has_no_alias() {
        let e = extract_src("using System.Collections.Generic;\n");
        match &e.usings[0] {
            UsingRecord::Plain { text, global } => {
                assert_eq!(text, "System.Collections.Generic");
                assert!(!global);
            }
            UsingRecord::Alias { .. } => panic!("expected plain form"),
        }
    }

    #[test]
    fn global_using_directive_sets_global_flag() {
        let e = extract_src("global using System;\n");
        match &e.usings[0] {
            UsingRecord::Plain { text, global } => {
                assert_eq!(text, "System");
                assert!(*global);
            }
            UsingRecord::Alias { .. } => panic!("expected plain form"),
        }
    }

    #[test]
    fn static_using_directive_is_plain_form_not_flagged() {
        // `using static` is not distinguished from a plain `using` -- both are
        // 1-named-child directives.
        let e = extract_src("using static System.Math;\n");
        match &e.usings[0] {
            UsingRecord::Plain { text, global } => {
                assert_eq!(text, "System.Math");
                assert!(!global);
            }
            UsingRecord::Alias { .. } => panic!("expected plain form"),
        }
    }

    #[test]
    fn using_directive_also_pushes_an_imports_ref_with_null_namespace() {
        let e = extract_src("using System.Collections.Generic;\n");
        let import_ref = e
            .refs
            .iter()
            .find(|r| r.kind == "imports")
            .expect("imports ref present");
        assert_eq!(import_ref.name, "System.Collections.Generic");
        assert!(import_ref.namespace.is_none());
    }

    // --- member-access qualifier capture (identifier vs. deep chain) ----

    #[test]
    fn simple_identifier_qualifier_is_captured_without_dot() {
        let e = extract_src(
            "namespace Fixtures.Orders { public enum Priority { Low, High } public class Probe { public bool F() { var x = Priority.High; return true; } } }",
        );
        let m = e
            .refs
            .iter()
            .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("High"))
            .expect("member ref present");
        assert_eq!(m.name, "Priority");
        assert!(m.qualified.is_none());
    }

    #[test]
    fn deep_member_chain_captures_every_window_qualifier_flattened_at_each_level() {
        // `Fixtures.Orders.Priority.High` parses as nested
        // member_access_expression, not qualified_name, since this is an
        // expression position -- but member_qualifier_text now flattens a
        // member_access_expression chain into its full dotted text, so the
        // OUTER window ("Fixtures.Orders.Priority" -> "High") is captured
        // too, not just the innermost identifier.identifier pair. walk()
        // still recurses into every level regardless, so the middle and
        // innermost windows are ALSO captured, each as their own separate
        // candidate -- resolve.rs's ladder is what decides, per candidate,
        // whether any of them actually resolves to an enum.
        let e = extract_src(
            "namespace Fixtures.Orders { public class Probe { public void F() { var x = Fixtures.Orders.Priority.High; } } }",
        );
        let members: Vec<(&str, &str)> = e
            .refs
            .iter()
            .filter(|r| r.kind == "uses-member")
            .map(|r| (r.name.as_str(), r.member.as_deref().unwrap_or("")))
            .collect();
        assert_eq!(
            members,
            vec![
                ("Priority", "High"),
                ("Orders", "Priority"),
                ("Fixtures", "Orders")
            ]
        );
        let outer = e
            .refs
            .iter()
            .find(|r| r.member.as_deref() == Some("High"))
            .expect("outer window present");
        assert_eq!(outer.qualified.as_deref(), Some("Fixtures.Orders.Priority"));
    }

    #[test]
    fn nested_enum_dotted_qualifier_is_captured_as_a_two_part_qualified_text() {
        // "Outer.Inner.On" -- the nested-enum-via-dotted-notation shape (not
        // the "+"-joined def id, which only exists on the def/id side, never
        // in source text). Extraction only needs to capture the RAW
        // qualifier text here; whether it resolves is resolve.rs's ladder.
        let e = extract_src(
            "namespace Fixtures.Widgets { public class Outer { public enum Inner { Off, On } } public class Probe { public void F() { var x = Outer.Inner.On; } } }",
        );
        let m = e
            .refs
            .iter()
            .find(|r| r.kind == "uses-member" && r.member.as_deref() == Some("On"))
            .expect("outer window present");
        assert_eq!(m.name, "Inner");
        assert_eq!(m.qualified.as_deref(), Some("Outer.Inner"));
    }

    // --- declaration_expression (out-declarations) -> uses-type ref -------

    #[test]
    fn inline_out_declaration_emits_a_uses_type_ref_for_its_type() {
        // `Method(out SomeEnum x)` -- a declaration_expression{type, name}
        // pair in expression position, the same shape as an ordinary
        // `parameter`.
        let e = extract_src(
            "namespace Fixtures.Orders { public class Probe { public void F() { TryGet(out SomeEnum x); } } }",
        );
        let uses_type: Vec<&str> = e
            .refs
            .iter()
            .filter(|r| r.kind == "uses-type")
            .map(|r| r.name.as_str())
            .collect();
        assert!(
            uses_type.contains(&"SomeEnum"),
            "expected a uses-type ref for the out-declared type, got {uses_type:?}"
        );
    }

    #[test]
    fn inline_out_declaration_with_var_type_yields_no_candidate() {
        // `out var x` -- implicit_type, same as an ordinary `var` parameter:
        // never a user-defined type reference, per outer_type_name.
        let e = extract_src("namespace Fixtures.Orders { public class Probe { public void F() { TryGet(out var x); } } }");
        assert!(e.refs.iter().all(|r| r.kind != "uses-type"));
    }

    // --- purpose signature ------------------------------------------------

    #[test]
    fn purpose_signature_composes_kind_name_bases_and_public_methods() {
        let e = extract_src(
            "namespace Fixtures.Orders { public interface IReader { int Count { get; } } public class Order : IReader { public int Count { get; set; } public void SaveAsync() {} private void Hidden() {} } }",
        );
        assert_eq!(
            e.purpose.as_deref(),
            Some("interface IReader | class Order : IReader; Save")
        );
    }

    #[test]
    fn purpose_signature_is_none_when_no_namespace_level_types() {
        let e = extract_src("// just a comment\n");
        assert!(e.purpose.is_none());
    }

    #[test]
    fn purpose_signature_truncates_at_200_utf16_units() {
        let long_bases = (0..40)
            .map(|i| format!("IFace{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let src = format!("public class Wide : {long_bases} {{}}");
        let e = extract_src(&src);
        let p = e.purpose.expect("purpose present");
        assert_eq!(p.encode_utf16().count(), MAX_PURPOSE);
        assert!(p.ends_with("..."));
    }

    // --- inherits via primary_constructor_base_type ------------------------

    #[test]
    fn primary_constructor_base_type_records_inherits_ref() {
        let e = extract_src(
            "namespace Fixtures.Misc { public class GadgetBase { public GadgetBase(string name) {} } public record GadgetRecord(string Name) : GadgetBase(Name); }",
        );
        let inherits: Vec<&str> = e
            .refs
            .iter()
            .filter(|r| r.kind == "inherits")
            .map(|r| r.name.as_str())
            .collect();
        assert!(inherits.contains(&"GadgetBase"));
    }

    // --- tuple types --------------------------------------------------------

    #[test]
    fn tuple_return_type_records_each_element_type() {
        let e = extract_src(
            "namespace Fixtures.Misc { public class GadgetBase {} public class Gadget { public (Gadget Primary, GadgetBase Fallback) Describe() { return (this, null); } } }",
        );
        let uses_type_names: Vec<&str> = e
            .refs
            .iter()
            .filter(|r| r.kind == "uses-type")
            .map(|r| r.name.as_str())
            .collect();
        assert!(uses_type_names.contains(&"Gadget"));
        assert!(uses_type_names.contains(&"GadgetBase"));
    }

    // --- #if/#elif/#else interrupting a fluent chain -----------------
    //
    // Native tree-sitter's error recovery for these fixtures swallows the
    // directive as a small ERROR-node extra child while continuing the
    // interrupted chain as ONE uninterrupted subtree, rather than the clean
    // statement split a non-recovering parse would produce (see
    // `preproc_promoted_qualifier`'s doc comment for the full mechanism).
    // The fixtures live in fixtures/preproc/ and are `include_str!`ed
    // here, so this fast unit test and the differential check run on the
    // identical bytes -- divergence between them would mean the fixture
    // drifted from what this test asserts, not a real behavior change.
    // They sit under fixtures/ rather than beside the harness because
    // this test is part of the crate and has to compile wherever the crate
    // does.
    //
    // Both fixtures need this much surrounding chain complexity to actually
    // reach the ERROR-node recovery path in native tree-sitter -- a much
    // shorter chain (e.g. a bare two-call `.Step1() #if DEBUG .DebugStep()
    // #endif .Step2()`) was verified to take a completely different, more
    // broken error-recovery path instead, so trimming these fixtures down is
    // NOT safe without re-verifying against the expected output.

    const IF_DIRECTIVE_CHAIN: &str =
        include_str!("../fixtures/preproc/preproc_chain_interrupt_if.cs");
    const IFELSE_DIRECTIVE_CHAIN: &str =
        include_str!("../fixtures/preproc/preproc_chain_interrupt_ifelse.cs");
    const NESTED_IF_DIRECTIVE_CHAIN_CONTROL: &str =
        include_str!("../fixtures/preproc/preproc_chain_interrupt_nested_control.cs");
    const WHOLESTMT_DIRECTIVE_CONTROL: &str =
        include_str!("../fixtures/preproc/preproc_chain_wholestmt_control.cs");

    fn uses_member_refs(e: &Extraction) -> Vec<(&str, &str, usize)> {
        e.refs
            .iter()
            .filter(|r| r.kind == "uses-member")
            .map(|r| (r.name.as_str(), r.member.as_deref().unwrap_or(""), r.line))
            .collect()
    }

    #[test]
    fn preproc_if_interrupting_chain_promotes_qualifier_at_the_resumed_identifiers_own_line() {
        let e = extract_src(IF_DIRECTIVE_CHAIN);
        // Line 31 is `.WriteTo.Debug()` itself -- NOT line 7 where the
        // interrupted chain's own `new Pipeline()` starts. Native
        // tree-sitter's enclosing (unsplit) node inherits that earlier
        // start position, which is exactly the bug this line guards: using
        // the wrong node's start position after promotion.
        assert!(uses_member_refs(&e).contains(&("WriteTo", "Debug", 31)));
    }

    #[test]
    fn preproc_if_interrupting_chain_places_promoted_ref_after_the_original_chains_own_arguments() {
        let e = extract_src(IF_DIRECTIVE_CHAIN);
        let refs = uses_member_refs(&e);
        let debug_pos = refs
            .iter()
            .position(|r| *r == ("WriteTo", "Debug", 31))
            .expect("promoted ref present");
        let interval_day_pos = refs
            .iter()
            .position(|r| *r == ("Interval", "Day", 26))
            .expect("Interval/Day ref present");
        let minimum_level_pos = refs
            .iter()
            .position(|r| *r == ("Level", "Information", 33))
            .expect("first MinimumLevel.Override argument ref present");
        // A cleanly-split tree walks the interrupted chain's OWN call
        // arguments (Interval.Day, part of the pre-`#if` `.File(...)` call)
        // before it ever reaches the promoted ref, because that ref lives in
        // what a clean parse treats as a separate, later statement.
        // Native tree-sitter has no such split -- without the deferred
        // (post-recursion) push in the walk's member_access_expression arm,
        // the promoted ref lands far too early in this array instead.
        assert!(
            interval_day_pos < debug_pos,
            "Interval/Day ({interval_day_pos}) should precede WriteTo/Debug ({debug_pos})"
        );
        assert!(debug_pos < minimum_level_pos, "WriteTo/Debug ({debug_pos}) should precede the first MinimumLevel.Override argument ref ({minimum_level_pos})");
    }

    #[test]
    fn preproc_ifelse_interrupting_chain_promotes_qualifier_in_both_arms_but_not_across_endif() {
        let e = extract_src(IFELSE_DIRECTIVE_CHAIN);
        let refs = uses_member_refs(&e);
        // Both the `#if` arm (line 23) and the `#else` arm (line 25) are
        // opening directives and each promotes its own qualifier.
        assert!(refs.contains(&("WriteTo", "Trace", 23)));
        assert!(refs.contains(&("WriteTo", "Console", 25)));
        // `#endif` never promotes: a clean parse absorbs it as a
        // trailing token rather than splitting the statement there, so
        // there is no "MinimumLevel"/"Override" candidate on either side.
        assert!(!refs.iter().any(|(_, member, _)| *member == "Override"));
        assert!(!refs.iter().any(|(name, _, _)| *name == "MinimumLevel"));
    }

    #[test]
    fn preproc_promotion_does_not_fire_on_nested_if_in_if_chain_interrupt() {
        // A `#if` nested inside another `#if` interrupting the same kind of
        // chain already parses cleanly WITHOUT the
        // compensation (native tree-sitter happens to build a proper
        // preproc_if node here too) -- this guards against the
        // compensation over-firing on this shape and introducing a spurious
        // ref.
        let e = extract_src(NESTED_IF_DIRECTIVE_CHAIN_CONTROL);
        let refs = uses_member_refs(&e);
        assert!(refs.contains(&("WriteTo", "Trace", 25)));
        assert!(!refs
            .iter()
            .any(|(name, member, _)| *name == "WriteTo" && *member == "Debug"));
    }

    #[test]
    fn preproc_promotion_does_not_fire_on_whole_statement_if_guard() {
        // An ordinary statement-level `#if DEBUG { ... }` (not interrupting
        // an expression) already parses cleanly -- guards against the
        // compensation over-firing there too. Both
        // calls resolve as ordinary `registry.Attach(...)` member accesses,
        // untouched by the directive between them.
        let e = extract_src(WHOLESTMT_DIRECTIVE_CONTROL);
        let refs = uses_member_refs(&e);
        assert_eq!(
            refs,
            vec![("registry", "Attach", 15), ("registry", "Attach", 17)]
        );
    }

    // --- TS/JS purposes (acceptance cases) -------
    //
    // Every fixture under fixtures/ts-grammar/ against its expected-output
    // string, byte-exact. These are the acceptance cases for TS/JS purpose
    // extraction: a mismatch means the extractor changed behaviour, so fix
    // the extractor rather than the fixture or the expected string.

    use crate::parse::TsGrammar;

    fn ts_purpose(src: &str, grammar: TsGrammar) -> Option<String> {
        extract_ts_purpose(src, grammar)
    }

    const ARTICLE_CARD_TSX: &str = include_str!("../fixtures/ts-grammar/ArticleCard.tsx");
    const USE_WIDGET_DATA_TS: &str = include_str!("../fixtures/ts-grammar/useWidgetData.ts");
    const ORDER_SERVICE_TS: &str = include_str!("../fixtures/ts-grammar/OrderService.ts");
    const WIDGETS_SLICE_TS: &str = include_str!("../fixtures/ts-grammar/widgetsSlice.ts");
    const STRING_UTILS_TS: &str = include_str!("../fixtures/ts-grammar/stringUtils.ts");
    const ARTICLE_TYPES_TS: &str = include_str!("../fixtures/ts-grammar/articleTypes.ts");
    const FORMAT_CURRENCY_TS: &str = include_str!("../fixtures/ts-grammar/formatCurrency.ts");
    const NOTIFICATION_DIGEST_TS: &str =
        include_str!("../fixtures/ts-grammar/notificationDigest.ts");
    const PATH_HELPERS_JS: &str = include_str!("../fixtures/ts-grammar/pathHelpers.js");
    // New fixtures.
    const REEXPORT_BARREL_TS: &str = include_str!("../fixtures/ts-grammar/reexportBarrel.ts");
    const WIDGET_PANEL_WITH_NOTE_TS: &str =
        include_str!("../fixtures/ts-grammar/widgetPanelWithNote.ts");

    #[test]
    fn ts_grammar_fixture_article_card_tsx() {
        assert_eq!(
            ts_purpose(ARTICLE_CARD_TSX, TsGrammar::Tsx).as_deref(),
            Some("function ArticleCard | interface ArticleCardProps")
        );
    }

    #[test]
    fn ts_grammar_fixture_use_widget_data_ts() {
        assert_eq!(
            ts_purpose(USE_WIDGET_DATA_TS, TsGrammar::Typescript).as_deref(),
            Some("function useWidgetData")
        );
    }

    #[test]
    fn ts_grammar_fixture_order_service_ts() {
        assert_eq!(
            ts_purpose(ORDER_SERVICE_TS, TsGrammar::Typescript).as_deref(),
            Some("class OrderService : IOrderService; getOrder, cancelOrder | interface IOrderService")
        );
    }

    #[test]
    fn ts_grammar_fixture_widgets_slice_ts() {
        assert_eq!(
            ts_purpose(WIDGETS_SLICE_TS, TsGrammar::Typescript).as_deref(),
            Some("const widgetsSlice | const setLoading | const setItems | default (anonymous)")
        );
    }

    #[test]
    fn ts_grammar_fixture_string_utils_ts() {
        assert_eq!(
            ts_purpose(STRING_UTILS_TS, TsGrammar::Typescript).as_deref(),
            Some("function truncate | function slugify | const DEFAULT_MAX_LENGTH")
        );
    }

    #[test]
    fn ts_grammar_fixture_article_types_ts() {
        assert_eq!(
            ts_purpose(ARTICLE_TYPES_TS, TsGrammar::Typescript).as_deref(),
            Some("interface ArticleItem | interface ArticleAuthor | type ArticleItemStatus | type ArticlePage")
        );
    }

    #[test]
    fn ts_grammar_fixture_format_currency_ts() {
        assert_eq!(
            ts_purpose(FORMAT_CURRENCY_TS, TsGrammar::Typescript).as_deref(),
            Some("default (anonymous)")
        );
    }

    #[test]
    fn ts_grammar_fixture_notification_digest_ts() {
        assert_eq!(
            ts_purpose(NOTIFICATION_DIGEST_TS, TsGrammar::Typescript).as_deref(),
            Some("class NotificationDigestBuilder; addEvent, build | function buildDailyDigest")
        );
    }

    #[test]
    fn ts_grammar_fixture_path_helpers_js() {
        assert_eq!(
            ts_purpose(PATH_HELPERS_JS, TsGrammar::Javascript).as_deref(),
            Some("function joinSafe | function isAbsolute")
        );
    }

    // --- reexportBarrel.ts fixture + edge cases -----------

    #[test]
    fn ts_grammar_fixture_reexport_barrel_ts() {
        assert_eq!(
            ts_purpose(REEXPORT_BARREL_TS, TsGrammar::Typescript).as_deref(),
            Some("reexports WidgetCard, WidgetList, Dialog, *, WidgetTypes")
        );
    }

    #[test]
    fn reexport_bucket_mixes_with_a_real_export_and_lands_last() {
        let src = "export function real() {}\nexport { A, B } from './m';\n";
        assert_eq!(
            ts_purpose(src, TsGrammar::Typescript).as_deref(),
            Some("function real | reexports A, B")
        );
    }

    #[test]
    fn local_export_clause_without_from_is_still_skipped() {
        let src = "const A = 1;\nexport { A };\n";
        assert_eq!(ts_purpose(src, TsGrammar::Typescript), None);
    }

    #[test]
    fn bare_star_reexports_dedupe_to_one_token_regardless_of_statement_count() {
        let src = "export * from './a';\nexport * from './b';\n";
        assert_eq!(
            ts_purpose(src, TsGrammar::Typescript).as_deref(),
            Some("reexports *")
        );
    }

    // --- Leading-comment hybrid: widgetPanelWithNote.ts fixture (pure vs hybrid) ---
    //
    // The pure entry point (`extract_ts_purpose`, exercised via `ts_purpose`
    // above) never sees the heuristic -- it must stay AST-only even for a
    // fixture whose first line IS a comment. `extract_ts_purpose_with_heuristic`
    // (mapcmd.rs's real per-file dispatch and run_extract_dump's dump path)
    // is what actually applies the comment prefix -- tested separately below
    // via a real temp-dir root/rel pair.

    #[test]
    fn ts_grammar_fixture_widget_panel_with_note_pure_extractor_stays_ast_only() {
        assert_eq!(
            ts_purpose(WIDGET_PANEL_WITH_NOTE_TS, TsGrammar::Typescript).as_deref(),
            Some("function WidgetPanel | interface WidgetPanelProps")
        );
    }

    fn scratch_root(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "scout-extract-hybrid-test-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn extract_ts_purpose_with_heuristic_prefixes_a_leading_comment() {
        let root = scratch_root("comment");
        fs::write(root.join("widget.ts"), WIDGET_PANEL_WITH_NOTE_TS).unwrap();
        let purpose = extract_ts_purpose_with_heuristic(
            &root,
            "widget.ts",
            WIDGET_PANEL_WITH_NOTE_TS,
            TsGrammar::Typescript,
        );
        assert_eq!(
            purpose.as_deref(),
            Some("Renders the widget summary panel and its retry action — function WidgetPanel | interface WidgetPanelProps")
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn extract_ts_purpose_with_heuristic_leaves_a_code_first_line_unprefixed() {
        let root = scratch_root("code-first");
        let src = "export function plain() { return 1; }\n";
        fs::write(root.join("plain.ts"), src).unwrap();
        let purpose =
            extract_ts_purpose_with_heuristic(&root, "plain.ts", src, TsGrammar::Typescript);
        assert_eq!(purpose.as_deref(), Some("function plain"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn extract_ts_purpose_with_heuristic_returns_none_for_comment_plus_zero_export() {
        // Zero-export files have no purpose downstream -- no purpose at all
        // comes out of this entry point for them, matching extract_ts_purpose's
        // own None contract.
        let root = scratch_root("comment-zero-export");
        let src = "// just notes, nothing exported\nfunction internalOnly() { return 1; }\n";
        fs::write(root.join("notesOnly.ts"), src).unwrap();
        let purpose =
            extract_ts_purpose_with_heuristic(&root, "notesOnly.ts", src, TsGrammar::Typescript);
        assert!(purpose.is_none());
        fs::remove_dir_all(&root).ok();
    }

    // --- degrade paths -------------------------------------------------

    #[test]
    fn broken_syntax_ts_degrades_to_none_not_a_panic() {
        // Genuinely malformed: no recognisable top-level export survives
        // error recovery, so compose_ts_purpose yields no entries -> None,
        // same signal that routes mapcmd.rs's per-file dispatch to the
        // heuristic purpose (broken-syntax TS degrades to the heuristic).
        let purpose = ts_purpose(
            "export class {{{ not valid typescript at all @@@",
            TsGrammar::Typescript,
        );
        assert!(purpose.is_none());
    }

    #[test]
    fn zero_export_file_yields_none_heuristic_fallback() {
        // The zero-export shape follows the established C# convention (no
        // namespace-level types -> None -> heuristic fallback) rather than
        // inventing new behaviour.
        assert!(ts_purpose(
            "function internalOnly() { return 1; }\nconst alsoInternal = 2;\n",
            TsGrammar::Typescript
        )
        .is_none());
        assert!(ts_purpose("// just a comment, no code\n", TsGrammar::Typescript).is_none());
    }

    // --- Duplicate-header preproc shape (the UTF-16 parse seam's motivating
    // corpus case) ------------------------------------------------------
    //
    // `#if X <header> { ... #else <header> { ... #endif <shared tail> }`
    // duplicates a member's header AND its opening brace across both arms
    // while sharing one closing brace, so the text is unbalanced once the
    // directives are treated as inert. Both engines' roots come back
    // has_error() on it; what they must agree on is the repair, and they do
    // only because both are handed the same UTF-16 view of the source (see
    // parse::utf16_units). Parsing UTF-8 here instead made native recovery
    // read the second arm's header as a local function nested inside the
    // first arm's still-open block -- one member scope instead of two, so
    // every receiver fact in them cancelled as a redeclaration conflict, and
    // every type declared after the shape got swallowed as misread
    // statements.
    //
    // These assertions are the expected output for these exact strings, not a
    // description of any Rust-side compensation: there is none left to
    // describe.

    // Two arms, one shared tail, one nested struct and one nested enum
    // after it. At this scale the shared repair keeps the outer type and
    // its "+"-nested members intact.
    const DUPLICATE_HEADER_HOST_SRC: &str = "
namespace Fixtures.Preproc
{
    public static class WidgetCompiler
    {
        private static readonly int[] _pool = new int[8];

#if WIDGET_V2
        private static int[] ComputeOffsets(IWidgetProvider items)
        {
            var count = items.WidgetCount;
#else
        private static int[] ComputeOffsets(IReadOnlyList<Widget> items)
        {
            var count = items.Count;
#endif
            if (count == 0)
                return _pool;
            return _pool;
        }

        private struct SlotInfo
        {
            public object Payload;
        }

        [System.Flags]
        private enum SlotStatus : byte
        {
            Empty = 0,
            Filled = 1,
        }
    }
}
";

    #[test]
    fn duplicate_header_keeps_nested_type_defs_at_their_real_scope() {
        let e = extract_src(DUPLICATE_HEADER_HOST_SRC);

        assert!(find_def(&e, "Fixtures.Preproc.WidgetCompiler").is_some());

        let slot_info = find_def(&e, "Fixtures.Preproc.WidgetCompiler+SlotInfo")
            .expect("SlotInfo kept, nested under its type");
        assert_eq!(slot_info.kind, "struct");
        assert_eq!(slot_info.namespace, "Fixtures.Preproc");

        let slot_status =
            find_def(&e, "Fixtures.Preproc.WidgetCompiler+SlotStatus").expect("SlotStatus kept");
        assert_eq!(slot_status.kind, "enum");
        assert!(find_def(&e, "Fixtures.Preproc.WidgetCompiler+SlotStatus.Empty").is_some());
        assert!(find_def(&e, "Fixtures.Preproc.WidgetCompiler+SlotStatus.Filled").is_some());

        // Both arms' parameter types are recorded; neither arm's refs are lost.
        assert!(e
            .refs
            .iter()
            .any(|r| r.kind == "uses-type" && r.name == "IWidgetProvider"));
        assert!(e
            .refs
            .iter()
            .any(|r| r.kind == "uses-type" && r.name == "IReadOnlyList"));
        assert!(e
            .refs
            .iter()
            .any(|r| r.kind == "uses-type" && r.name == "Widget"));
    }

    #[test]
    fn defs_and_refs_stay_in_ascending_line_order_on_a_pathological_file() {
        // Document order is the artifact contract; a file whose root is
        // has_error() is where it is easiest to lose, so it is pinned there.
        let e = extract_src(DUPLICATE_HEADER_HOST_SRC);
        let def_lines: Vec<usize> = e.defs.iter().map(|d| d.line).collect();
        let mut sorted = def_lines.clone();
        sorted.sort();
        assert_eq!(
            def_lines, sorted,
            "defs must already be in ascending line order"
        );

        let ref_lines: Vec<usize> = e.refs.iter().map(|r| r.line).collect();
        let mut sorted_refs = ref_lines.clone();
        sorted_refs.sort();
        assert_eq!(
            ref_lines, sorted_refs,
            "refs must already be in ascending line order"
        );
    }

    #[test]
    fn well_formed_file_is_unaffected() {
        let e = extract_src(
            "namespace Fixtures.Widgets { public class Gadget { public int Count; } public struct Handle {} }",
        );
        assert!(find_def(&e, "Fixtures.Widgets.Gadget").is_some());
        assert!(find_def(&e, "Fixtures.Widgets.Handle").is_some());
    }

    // Same shape, plus a SECOND nested type whose own body repeats the
    // pattern. Two of them in one file is enough accumulated damage that the
    // shared repair discards BOTH wrapping types: no WidgetCompiler def, no
    // Emitter def, and everything that survives lands flat at namespace-less
    // ambient scope. This drop is the correct output -- it is the shape the
    // removed Rust-only recovery pass used to "rescue", inventing an `Emitter`
    // def that must not appear.
    const DUPLICATE_HEADER_HOST_WITH_NESTED_EMITTER_SRC: &str = "
namespace Fixtures.Preproc
{
    public static class WidgetCompiler
    {
        private static readonly int[] _pool = new int[8];

#if WIDGET_V2
        private static int[] ComputeOffsets(IWidgetProvider items)
        {
            var count = items.WidgetCount;
#else
        private static int[] ComputeOffsets(IReadOnlyList<Widget> items)
        {
            var count = items.Count;
#endif
            if (count == 0)
                return _pool;
            return _pool;
        }

        private struct SlotInfo
        {
            public object Payload;
        }

        [System.Flags]
        private enum SlotStatus : byte
        {
            Empty = 0,
            Filled = 1,
        }

        private static class Emitter
        {
#if WIDGET_V2
            public static bool TryEmit(Widget expr, IWidgetProvider items, Sink sink)
            {
                var scaled = items.WidgetCount;
#else
            public static bool TryEmit(Widget expr, IReadOnlyList<Widget> items, Sink sink)
            {
                var scaled = items.Count;
#endif
                sink.Write(scaled);
                return true;
            }
        }
    }
}
";

    #[test]
    fn stacked_duplicate_headers_drop_both_wrapping_types_like_the_reference() {
        let e = extract_src(DUPLICATE_HEADER_HOST_WITH_NESTED_EMITTER_SRC);

        assert!(e.defs.iter().all(|d| d.name != "WidgetCompiler"));
        assert!(e.defs.iter().all(|d| d.name != "Emitter"));

        let slot_info = find_def(&e, "SlotInfo").expect("SlotInfo survives, flat");
        assert_eq!(slot_info.kind, "struct");
        assert_eq!(slot_info.namespace, "");
        assert!(find_def(&e, "SlotStatus").is_some());
        assert!(find_def(&e, "SlotStatus.Empty").is_some());
        assert!(find_def(&e, "SlotStatus.Filled").is_some());

        // The refs inside the innermost body still come through, at that
        // same flat ambient scope.
        assert!(e
            .refs
            .iter()
            .any(|r| r.kind == "uses-type" && r.name == "Sink"));
        assert!(e.refs.iter().any(|r| r.kind == "uses-member"
            && r.name == "sink"
            && r.member.as_deref() == Some("Write")));
    }

    // --- Def member facts -------------------------------------------------------

    fn member_facts(e: &Extraction) -> Vec<(&str, Option<&str>)> {
        e.refs
            .iter()
            .filter(|r| r.kind == "uses-member")
            .map(|r| {
                (
                    r.member.as_deref().unwrap_or(""),
                    r.receiver_type.as_deref(),
                )
            })
            .collect()
    }

    #[test]
    fn stage2a_def_records_properties_fields_and_method_returns() {
        let e = extract_src(
            r#"
namespace App.Facts;

public class Widget
{
  private readonly ILogger _log;
  private int a, b;
  public const int Max = 3;
  public static string Prefix { get; } = "p";
  public string Name => "n";
  public int this[int i] => i;
  public event System.EventHandler Changed;

  public Task<Foo> GetAsync() => null;
  public void Nothing() { }
  public Some.Ns.Thing Qualified() => null;
  public int Num() => 1;
  private Hidden Secret() => null;
}
"#,
        );
        let d = find_def(&e, "App.Facts.Widget").expect("Widget def present");
        assert_eq!(
            d.properties,
            vec!["Prefix", "Name"],
            "source order; the indexer is not a property"
        );
        assert_eq!(
            d.fields,
            vec!["_log", "a", "b", "Max"],
            "every declarator of every field, incl. const; the event is not a field"
        );
        assert_eq!(
            d.method_returns,
            vec![("GetAsync".to_string(), "Task".to_string()), ("Qualified".to_string(), "Thing".to_string())],
            "generic args stripped to the base identifier, a qualified return reduced to its last segment, void/int omitted, FIRST-declaration order"
        );
        assert!(
            !d.methods.contains(&"Secret".to_string()),
            "methodReturns stays parallel to methods: a private method is in neither"
        );
        assert!(d.method_returns.iter().all(|(n, _)| n != "Secret"));
    }

    #[test]
    fn stage2a_type_declaring_none_of_the_new_members_records_all_three_empty() {
        let e = extract_src("namespace App.Bare { public class Empty { public void Go() { } } }");
        let d = find_def(&e, "App.Bare.Empty").expect("Empty def present");
        assert_eq!(d.methods, vec!["Go"]);
        assert!(d.properties.is_empty());
        assert!(d.fields.is_empty());
        assert!(
            d.method_returns.is_empty(),
            "empty means OMITTED at serialization -- pre-stage-2 bytes preserved"
        );
    }

    #[test]
    fn stage2a_method_returns_keeps_the_first_overload_and_never_backfills_a_void_first_declaration(
    ) {
        let e = extract_src(
            r#"
namespace App.Overloads;

public class Api
{
  public Foo Get(int id) => null;
  public Bar Get(string key) => null;
  public void Send(int id) { }
  public Receipt Send(string key) => null;
}
"#,
        );
        let d = find_def(&e, "App.Overloads.Api").expect("Api def present");
        assert_eq!(
            d.method_returns,
            vec![("Get".to_string(), "Foo".to_string())],
            "Get keeps the first overload; Send is blocked by its void first declaration"
        );
    }

    #[test]
    fn stage2a_interface_records_properties_and_returns_without_any_public_modifier() {
        let e = extract_src(
            r#"
namespace App.Contracts;

public interface IRepo
{
  string Name { get; }
  Widget Find(int id);
}
"#,
        );
        let d = find_def(&e, "App.Contracts.IRepo").expect("IRepo def present");
        assert_eq!(d.properties, vec!["Name"]);
        assert_eq!(
            d.method_returns,
            vec![("Find".to_string(), "Widget".to_string())]
        );
        assert!(d.fields.is_empty());
    }

    // --- Receiver facts ---------------------------------------

    #[test]
    fn stage2b_facts_come_from_locals_var_new_params_fields_and_primary_ctor_params() {
        let e = extract_src(
            r#"
namespace App.Receivers;

public class Host(IClock clock)
{
  private readonly ILogger _log;

  public void Run(IRepo repo)
  {
    Helper h = new Helper();
    var svc = new Service();
    repo.Save();
    h.Help();
    svc.Go();
    _log.Info();
    clock.Now();
  }
}
"#,
        );
        assert_eq!(
            member_facts(&e),
            vec![
                ("Save", Some("IRepo")),
                ("Help", Some("Helper")),
                ("Go", Some("Service")),
                ("Info", Some("ILogger")),
                ("Now", Some("IClock")),
            ]
        );
    }

    #[test]
    fn stage2b_receiver_type_is_set_only_on_refs_that_earned_a_fact() {
        let e = extract_src(
            r#"
namespace App.Shape;

public class Host
{
  public void Run(IRepo repo) { repo.Save(); }
}
"#,
        );
        let r = e
            .refs
            .iter()
            .find(|r| r.kind == "uses-member")
            .expect("member ref present");
        assert_eq!(r.name, "repo");
        assert_eq!(r.qualified, None);
        assert!(!r.generic);
        assert_eq!(r.receiver_type.as_deref(), Some("IRepo"));
    }

    #[test]
    fn stage2b_no_fact_for_var_from_call_predefined_types_or_an_implicit_lambda_parameter() {
        let e = extract_src(
            r#"
namespace App.NoFacts;

public class Host
{
  public void Run(string text, System.Collections.Generic.List<Widget> items)
  {
    var computed = Compute();
    int count = 0;
    computed.Use();
    text.Trim();
    count.ToString();
    items.ForEach(x => x.Do());
  }
}
"#,
        );
        let facts: Vec<(String, Option<&str>)> = e
            .refs
            .iter()
            .filter(|r| r.kind == "uses-member")
            .map(|r| {
                (
                    format!("{}.{}", r.name, r.member.as_deref().unwrap_or("")),
                    r.receiver_type.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            facts,
            vec![
                // var + a call initializer: resolve-time territory, never guessed here
                ("computed.Use".to_string(), None),
                // predefined types: no fact
                ("text.Trim".to_string(), None),
                ("count.ToString".to_string(), None),
                // an explicitly typed parameter DOES earn a fact even when its
                // declared type is qualified AND generic -- reduced to "List"
                ("items.ForEach".to_string(), Some("List")),
                // implicit lambda parameter: no type node, no fact
                ("x.Do".to_string(), None),
            ]
        );
    }

    #[test]
    fn stage2b_two_declarations_of_one_local_with_different_types_cancel_to_no_fact() {
        let e = extract_src(
            r#"
namespace App.Conflict;

public class Host
{
  public void Run(bool flag)
  {
    if (flag) { Widget x = new Widget(); x.Go(); }
    else { Gadget x = new Gadget(); x.Go(); }
  }
}
"#,
        );
        assert_eq!(
            member_facts(&e),
            vec![("Go", None), ("Go", None)],
            "the method-level flat table is conflicted for \"x\""
        );
    }

    #[test]
    fn stage2b_same_local_name_declared_twice_with_the_same_type_still_yields_the_fact() {
        let e = extract_src(
            r#"
namespace App.NoConflict;

public class Host
{
  public void Run(bool flag)
  {
    if (flag) { Widget x = new Widget(); x.Go(); }
    else { Widget x = new Widget(); x.Go(); }
  }
}
"#,
        );
        assert_eq!(
            member_facts(&e),
            vec![("Go", Some("Widget")), ("Go", Some("Widget"))]
        );
    }

    #[test]
    fn stage2b_a_parameter_shadows_a_same_named_field_of_a_different_type() {
        let e = extract_src(
            r#"
namespace App.Shadow;

public class Host
{
  private Widget handler;

  public void Run(Gadget handler)
  {
    handler.Go();
  }

  public void Untouched()
  {
    handler.Go();
  }
}
"#,
        );
        assert_eq!(
            member_facts(&e),
            vec![("Go", Some("Gadget")), ("Go", Some("Widget"))],
            "inside Run the parameter wins; the other method still sees the field"
        );
    }

    #[test]
    fn stage2b_a_nested_type_never_inherits_the_enclosing_types_field_table() {
        let e = extract_src(
            r#"
namespace App.Nested;

public class Outer
{
  private Widget handler;

  public class Inner
  {
    public void Run() { handler.Go(); }
  }
}
"#,
        );
        assert_eq!(
            member_facts(&e),
            vec![("Go", None)],
            "a nested type cannot reach an outer instance field"
        );
    }

    #[test]
    fn stage2b_a_flattened_chain_tail_never_inherits_the_heads_receiver_type() {
        // Stage-1 chain-tail regression class, extended for receiverType: a
        // tail must NOT inherit a fact it did not earn. Structurally
        // impossible here because the tail's qualifier is dotted.
        let e = extract_src(
            r#"
namespace App.Consumers;

public class Chain
{
  public void Run()
  {
    Widget w = new Widget();
    w.Inner.Tail();
  }
}
"#,
        );
        let head = e
            .refs
            .iter()
            .find(|r| r.member.as_deref() == Some("Inner"))
            .expect("head ref present");
        let tail = e
            .refs
            .iter()
            .find(|r| r.member.as_deref() == Some("Tail"))
            .expect("tail ref present");
        assert_eq!(
            head.receiver_type.as_deref(),
            Some("Widget"),
            "the head is the ref the local declaration vouches for"
        );
        assert_eq!(
            tail.qualified.as_deref(),
            Some("w.Inner"),
            "the tail's qualifier is the flattened chain window"
        );
        assert_eq!(
            tail.receiver_type, None,
            "the tail must NOT inherit the head's receiverType"
        );
    }

    #[test]
    fn stage2b_a_generic_qualifier_never_carries_a_receiver_fact() {
        // A type-argument list is syntax no local, parameter or field can
        // carry, so the bare-only guard refuses the lookup outright even when
        // a same-named local happens to exist.
        let e = extract_src(
            r#"
namespace App.Generic;

public class Host
{
  public void Run()
  {
    Widget Cache = new Widget();
    Cache<int>.Go();
  }
}
"#,
        );
        let r = e
            .refs
            .iter()
            .find(|r| r.member.as_deref() == Some("Go"))
            .expect("member ref present");
        assert!(r.generic);
        assert_eq!(r.receiver_type, None);
    }

    #[test]
    fn stage2b_a_local_declared_in_a_using_or_for_statement_is_a_fact() {
        let e = extract_src(
            r#"
namespace App.Statements;

public class Host
{
  public void Run()
  {
    using (Stream s = Open()) { s.Read(); }
    for (Counter c = Start(); ; ) { c.Tick(); }
  }
}
"#,
        );
        assert_eq!(
            member_facts(&e),
            vec![("Read", Some("Stream")), ("Tick", Some("Counter"))]
        );
    }

    #[test]
    fn stage2b_an_explicitly_typed_foreach_variable_is_a_fact() {
        // foreach_statement carries its own `type`/`left` fields,
        // not a variable_declaration node, so it needs its own rule rather
        // than falling out of the ordinary declaration walk for free. An
        // explicitly typed loop variable is answered by that type node
        // exactly like any other declaration.
        let e = extract_src(
            r#"
namespace App.ForEach;

public class Host
{
  public void Run(System.Collections.Generic.List<Widget> items)
  {
    foreach (Widget w in items) { w.Go(); }
  }
}
"#,
        );
        let go = e
            .refs
            .iter()
            .find(|r| r.member.as_deref() == Some("Go"))
            .expect("member ref present");
        assert_eq!(go.receiver_type.as_deref(), Some("Widget"));
    }

    #[test]
    fn stage2b_a_local_function_folds_into_the_enclosing_members_flat_table() {
        let e = extract_src(
            r#"
namespace App.LocalFn;

public class Host
{
  public void Run()
  {
    Widget w = new Widget();
    void Inner() { w.Go(); }
  }
}
"#,
        );
        let go = e
            .refs
            .iter()
            .find(|r| r.member.as_deref() == Some("Go"))
            .expect("member ref present");
        assert_eq!(
            go.receiver_type.as_deref(),
            Some("Widget"),
            "a local function sees the enclosing method's locals"
        );
    }

    #[test]
    fn stage2b_an_explicitly_typed_declaration_never_reads_its_new_initializer() {
        let e = extract_src(
            r#"
namespace App.Explicit;

public class Host
{
  public void Run()
  {
    object o = new Widget();
    o.Go();
  }
}
"#,
        );
        let go = e
            .refs
            .iter()
            .find(|r| r.member.as_deref() == Some("Go"))
            .expect("member ref present");
        assert_eq!(
            go.receiver_type, None,
            "`object` is a predefined type: no fact, and Widget is never substituted for it"
        );
    }

    #[test]
    fn stage2b_each_duplicated_header_arm_keeps_its_own_receiver_facts() {
        // The duplicate-header preproc shape (see
        // DUPLICATE_HEADER_HOST_WITH_NESTED_EMITTER_SRC). Each arm is its own
        // method_declaration, so each arm's parameter list vouches for that
        // arm's refs -- the two arms are NOT one flat scope in which `items`
        // is a conflicting redeclaration. The inner type's two arms lose the
        // parameter list along with their own header, so their
        // `items` refs get no fact; `sink` does, from the surviving one.
        let e = extract_src(DUPLICATE_HEADER_HOST_WITH_NESTED_EMITTER_SRC);
        let facts: Vec<(&str, Option<&str>)> = e
            .refs
            .iter()
            .filter(|r| r.kind == "uses-member")
            .map(|r| (r.name.as_str(), r.receiver_type.as_deref()))
            .collect();
        assert_eq!(
            facts,
            vec![
                ("items", Some("IWidgetProvider")),
                ("items", Some("IReadOnlyList")),
                ("items", None),
                ("items", None),
                ("sink", Some("Sink")),
            ]
        );
    }

    // --- Extension-method def facts ---------------------------------------------

    #[test]
    fn stage3_extension_methods_recorded_in_source_order_deduped_by_name_this_type_and_arity() {
        let e = extract_src(
            r#"
namespace App.Ext;

public static class WidgetExtensions
{
  public static Widget Copy(this Widget w) => w;
  public static void Render(this Widget w) { }
  public static void Render(this Widget w, int depth) { }
  public static void Render(this Widget w, string label) { }
  public static string Trim(this string s) => s;
  public static void Each(this System.Collections.Generic.List<Widget> items) { }
  public static void Plain(Widget w) { }
  public static void Late(Widget w, this Gadget g) { }
  internal static void Hidden(this Gadget g) { }
}
"#,
        );
        let d = find_def(&e, "App.Ext.WidgetExtensions").expect("WidgetExtensions def present");
        let pairs: Vec<(&str, &str, usize, i64)> = d
            .extension_methods
            .iter()
            .map(|x| {
                (
                    x.name.as_str(),
                    x.this_type.as_str(),
                    x.arity_min,
                    x.arity_max,
                )
            })
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("Copy", "Widget", 0, 0),
                // The two 1-parameter Render overloads collapse into ONE entry
                // -- the dedup key is the (name, thisType, arityMin, arityMax)
                // QUADRUPLE, and both name the same call shape. The 0-parameter
                // overload is a DIFFERENT entry; before the first tighten
                // amendment all three were one arity-blind entry any Render(...)
                // call could match.
                ("Render", "Widget", 0, 0),
                ("Render", "Widget", 1, 1),
                // predefined this-types are KEPT, unlike stage-2 receiver facts
                ("Trim", "string", 0, 0),
                // generic args stripped to the base identifier, qualified name
                // reduced to its last segment
                ("Each", "List", 0, 0),
                // no accessibility filter: internal extensions count
                ("Hidden", "Gadget", 0, 0),
            ]
        );
        // ...and the generic amendment records the stripped arguments alongside
        // the base identifier rather than discarding them.
        assert_eq!(
            d.extension_methods
                .iter()
                .map(|x| x.this_args.clone())
                .collect::<Vec<_>>(),
            vec![
                None,
                None,
                None,
                None,
                Some(vec!["Widget".to_string()]),
                None
            ],
            "thisArgs is present ONLY on the generic this-parameter"
        );
        assert!(
            !pairs.iter().any(|(n, ..)| *n == "Plain"),
            "a static method with no this-parameter is not an extension method"
        );
        assert!(
            !pairs.iter().any(|(n, ..)| *n == "Late"),
            "`this` on a NON-first parameter is not an extension method"
        );
        // The accessibility asymmetry is deliberate and worth pinning:
        // `methods` stays public-only, so `Hidden` is reachable ONLY through
        // the extension tier.
        assert!(!d.methods.contains(&"Hidden".to_string()));
    }

    // --- The ref-side `argCount` fact ------------------------------------------

    // (qualified-or-bare qualifier + "." + member, argCount) for every
    // uses-member ref, in extraction order.
    fn member_arg_counts(e: &Extraction) -> Vec<(String, Option<usize>)> {
        e.refs
            .iter()
            .filter(|r| r.kind == "uses-member")
            .map(|r| {
                let qualifier = r.qualified.as_deref().unwrap_or(r.name.as_str());
                (
                    format!("{qualifier}.{}", r.member.as_deref().unwrap_or("")),
                    r.arg_count,
                )
            })
            .collect()
    }

    #[test]
    fn stage3_arg_count_is_recorded_only_for_a_call_and_always_from_the_refs_own_invocation() {
        let e = extract_src(
            r#"
namespace App.Consumers;

public class Chain
{
  public void Run()
  {
    Widget w = new Widget();
    w.Inner.Tail(1, 2);
    Send(w.Payload);
    Send(w.Compute(7));
    var s = w.Slug;
  }
}
"#,
        );
        assert_eq!(
            member_arg_counts(&e),
            vec![
                // The flattened chain TAIL answers for ITS OWN call, not for
                // anything the head is part of -- the same independence stage 2
                // gave receiverType, now for the second borrowed-fact hazard.
                ("w.Inner.Tail".to_string(), Some(2)),
                // The chain HEAD is the qualifier of ".Tail", never a callee.
                ("w.Inner".to_string(), None),
                // A member access sitting in someone else's ARGUMENT list has an
                // invocation_expression above it too. It must NOT inherit that
                // call's count -- the guard is "this node IS the function
                // field", not "an invocation is somewhere overhead".
                ("w.Payload".to_string(), None),
                ("w.Compute".to_string(), Some(1)),
                // An ordinary property read: no argCount, which is what keeps it
                // out of the extension tier entirely.
                ("w.Slug".to_string(), None),
            ]
        );
    }

    #[test]
    fn stage3_a_kg1_promoted_qualifier_computes_arg_count_from_its_own_invocation_parent() {
        // The chain-promotion compensation supplies only the QUALIFIER text,
        // from a DIFFERENT node than the one being walked. Its argCount must
        // still come from the walked node's own invocation parent: the promoted
        // `WriteTo.Debug()` call is a zero-argument call, and the surrounding
        // `.File(...)` call's seven arguments, which this subtree still
        // contains, must not leak into it.
        let e = extract_src(IF_DIRECTIVE_CHAIN);
        let promoted = e
            .refs
            .iter()
            .find(|r| r.name == "WriteTo" && r.member.as_deref() == Some("Debug"))
            .expect("promoted ref present");
        assert_eq!(promoted.arg_count, Some(0), "`.Debug()` takes no arguments; the wrapping `.File(...)` call's count must not leak in");
        // And every ordinary ref in the same chain answers for itself. `Interval
        // .Day` (line 26) sits INSIDE the seven-argument `.File(...)` list and
        // is a plain value read, so it records no argCount at all.
        assert_eq!(
            e.refs
                .iter()
                .filter(|r| r.kind == "uses-member")
                .map(|r| (
                    r.name.as_str(),
                    r.member.as_deref().unwrap_or(""),
                    r.arg_count
                ))
                .collect::<Vec<_>>(),
            vec![
                ("e", "Level", None),
                ("Level", "Error", None),
                ("Interval", "Day", None),
                ("WriteTo", "Debug", Some(0)),
                ("Level", "Information", None),
                ("Level", "Information", None),
            ]
        );
    }

    #[test]
    fn stage3_a_type_declaring_no_extension_methods_records_an_empty_list() {
        let e = extract_src("namespace App.Plain { public static class Utils { public static void Go(Widget w) { } } }");
        let d = find_def(&e, "App.Plain.Utils").expect("Utils def present");
        assert_eq!(d.methods, vec!["Go"]);
        assert!(
            d.extension_methods.is_empty(),
            "empty means OMITTED at serialization -- pre-stage-3 bytes preserved"
        );
    }

    #[test]
    fn stage3_this_type_unwraps_nullable_and_array_the_same_way_a_receiver_fact_does() {
        let e = extract_src(
            r#"
namespace App.Ext;

public static class Shapes
{
  public static void Each(this Widget[] items) { }
  public static void Maybe(this Widget? w) { }
  public static void Deep(this App.Other.Gadget g) { }
}
"#,
        );
        let d = find_def(&e, "App.Ext.Shapes").expect("Shapes def present");
        let pairs: Vec<(&str, &str)> = d
            .extension_methods
            .iter()
            .map(|x| (x.name.as_str(), x.this_type.as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![("Each", "Widget"), ("Maybe", "Widget"), ("Deep", "Gadget")]
        );
    }

    #[test]
    fn stage3_a_this_parameter_carrying_a_second_modifier_is_still_an_extension_method() {
        // `this ref T` has TWO modifier children -- the scan is `.some(...)`,
        // not "the first modifier is `this`".
        let e = extract_src("namespace App.Ext { public static class R { public static void Bump(this ref Counter c) { } } }");
        let d = find_def(&e, "App.Ext.R").expect("R def present");
        let pairs: Vec<(&str, &str)> = d
            .extension_methods
            .iter()
            .map(|x| (x.name.as_str(), x.this_type.as_str()))
            .collect();
        assert_eq!(pairs, vec![("Bump", "Counter")]);
    }

    #[test]
    fn stage3_base_type_identifier_keeps_predefined_only_for_the_this_parameter_caller() {
        // Same source, two fact families: `Trim`'s this-type records "string"
        // while the method's own predefined RETURN type still yields no
        // methodReturns fact -- the keep_predefined flag is per-call-site.
        let e = extract_src("namespace App.Ext { public static class S { public static string Trim(this string s) => s; } }");
        let d = find_def(&e, "App.Ext.S").expect("S def present");
        assert_eq!(
            d.extension_methods
                .iter()
                .map(|x| x.this_type.as_str())
                .collect::<Vec<_>>(),
            vec!["string"]
        );
        assert!(
            d.method_returns.is_empty(),
            "every stage-2 call site passes keep_predefined=false and is unchanged"
        );
    }

    // The `extract-dump` JSON is a byte-exact surface, so its KEY ORDER is
    // significant in its own right.
    fn def_json_keys(d: &DefRecord) -> Vec<&'static str> {
        match def_to_json(d) {
            Json::Obj(fields) => fields.into_iter().map(|(k, _)| k).collect(),
            _ => panic!("def_to_json must produce an object"),
        }
    }

    #[test]
    fn stage3_extension_methods_serialize_last_with_the_arity_range_and_this_args() {
        let e = extract_src(
            "namespace App.Ext { public static class W { public static Widget Copy(this Widget w) => w; public static string Trim(this string s, int n) => s; public static void Each(this List<Widget> l, params int[] xs) { } } }",
        );
        let d = find_def(&e, "App.Ext.W").expect("W def present");
        assert_eq!(
            def_json_keys(d),
            vec![
                "id",
                "name",
                "namespace",
                "kind",
                "line",
                "methods",
                "methodReturns",
                "extensionMethods"
            ],
            "extensionMethods lands AFTER the stage-2 additions, not among them"
        );
        let Json::Obj(fields) = def_to_json(d) else {
            panic!("object")
        };
        let (_, ext) = fields
            .into_iter()
            .find(|(k, _)| *k == "extensionMethods")
            .expect("extensionMethods present");
        let Json::Arr(entries) = ext else {
            panic!("extensionMethods is an array")
        };
        assert_eq!(entries.len(), 3);
        for entry in &entries[..2] {
            let Json::Obj(kv) = entry else {
                panic!("each entry is an object")
            };
            assert_eq!(
                kv.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
                vec!["name", "thisType", "arityMin", "arityMax"],
                "entry key order is significant too"
            );
        }
        // The two arity halves are NUMBERS, not strings: a byte-diff would
        // catch a quoted one. arityMax is
        // SIGNED: -1 is the unbounded-`params` sentinel.
        let Json::Obj(kv) = &entries[1] else {
            panic!("object")
        };
        match kv.iter().find(|(k, _)| *k == "arityMin").map(|(_, v)| v) {
            Some(Json::Num(n)) => {
                assert_eq!(*n, 1, "Trim(this string s, int n) requires one argument")
            }
            _ => panic!("arityMin must serialize as a JSON number"),
        }
        match kv.iter().find(|(k, _)| *k == "arityMax").map(|(_, v)| v) {
            Some(Json::Int(n)) => assert_eq!(*n, 1),
            _ => panic!("arityMax must serialize as a JSON number"),
        }
        // The generic entry carries thisArgs, LAST, and its params tail makes
        // arityMax the -1 sentinel.
        let Json::Obj(kv) = &entries[2] else {
            panic!("object")
        };
        assert_eq!(
            kv.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            vec!["name", "thisType", "arityMin", "arityMax", "thisArgs"],
            "thisArgs lands after arityMax, and only on a generic this-parameter"
        );
        assert!(
            matches!(
                kv.iter().find(|(k, _)| *k == "arityMax").map(|(_, v)| v),
                Some(Json::Int(-1))
            ),
            "a params tail serializes arityMax as the number -1"
        );
    }

    #[test]
    fn stage3_bases_serializes_after_extension_methods_and_is_absent_when_empty() {
        // `Ns.BaseWidget<int>` carries a type-argument list, so this def now
        // ALSO gets a baseGenericArgs entry -- bases is no
        // longer the last key when a base is itself generic, which is the
        // point this fixture was chosen to cover: both keys' relative order
        // still holds (bases before baseGenericArgs, both before the absent
        // testMethods).
        let e = extract_src(
            "namespace App.Other { public class Widget : Ns.BaseWidget<int>, IWidget { } }",
        );
        let d = find_def(&e, "App.Other.Widget").expect("Widget def present");
        assert_eq!(
            def_json_keys(d),
            vec![
                "id",
                "name",
                "namespace",
                "kind",
                "line",
                "methods",
                "bases",
                "baseGenericArgs"
            ],
            "bases lands after methods, baseGenericArgs immediately after bases"
        );
        assert_eq!(
            d.bases,
            vec!["BaseWidget".to_string(), "IWidget".to_string()],
            "base IDENTIFIERS: generic arguments stripped, a qualified name cut to its last segment"
        );
        assert_eq!(
            d.base_generic_args,
            vec![("BaseWidget".to_string(), vec!["int".to_string()])],
            "BaseWidget<int> is non-generic Widget's closed base -- int is a predefined type, KEPT (this base pass keeps predefined types the same way the this-parameter thisArgs facts do); IWidget carries no type-argument list at all, so it contributes no entry"
        );

        let plain = extract_src("namespace App.Other { public class Bare { } }");
        let pd = find_def(&plain, "App.Other.Bare").expect("Bare def present");
        assert_eq!(
            def_json_keys(pd),
            vec!["id", "name", "namespace", "kind", "line", "methods"]
        );
    }

    #[test]
    fn stage3_extension_methods_key_is_absent_when_the_type_declares_none() {
        let e = extract_src("namespace App.Plain { public static class Utils { public static void Go(Widget w) { } } }");
        let d = find_def(&e, "App.Plain.Utils").expect("Utils def present");
        assert_eq!(
            def_json_keys(d),
            vec!["id", "name", "namespace", "kind", "line", "methods"],
            "pre-stage-3 bytes preserved"
        );
    }
    // --- testMethods -----------------------------------------------------------

    #[test]
    fn test_coverage_xunit_fact_and_theory_land_in_test_methods_serialized_last_after_bases() {
        let e = extract_src(
            "namespace App.Tests;\n\npublic class WidgetTests : TestBase\n{\n  [Fact]\n  public void ComputesTotal() { }\n\n  [Theory]\n  [InlineData(1)]\n  public void RejectsEmptyCart(int n) { }\n\n  public void Helper() { }\n}\n",
        );
        let d = find_def(&e, "App.Tests.WidgetTests").expect("WidgetTests def present");
        assert_eq!(
            d.test_methods,
            vec!["ComputesTotal", "RejectsEmptyCart"],
            "source order; the unattributed helper is a method but not a test"
        );
        assert_eq!(
            def_json_keys(d),
            vec![
                "id",
                "name",
                "namespace",
                "kind",
                "line",
                "methods",
                "bases",
                "testMethods"
            ],
            "testMethods lands LAST -- after bases, which was the final fact before this stage"
        );
    }

    #[test]
    fn test_coverage_suffixed_qualified_targeted_and_shared_bracket_attribute_forms_all_match() {
        let e = extract_src(
            "namespace App.Tests;\n\npublic class SpellingTests\n{\n  [FactAttribute]\n  public void Suffixed() { }\n\n  [Xunit.Fact]\n  public void Qualified() { }\n\n  [method: Fact]\n  public void Targeted() { }\n\n  [Fact, Trait(\"speed\", \"fast\")]\n  public void SharesABracket() { }\n\n  [method: Xunit.FactAttribute]\n  public void EveryFormAtOnce() { }\n}\n",
        );
        let d = find_def(&e, "App.Tests.SpellingTests").expect("SpellingTests def present");
        assert_eq!(
            d.test_methods,
            vec!["Suffixed", "Qualified", "Targeted", "SharesABracket", "EveryFormAtOnce"],
            "the last dotted segment is compared with and without the Attribute suffix, per attribute node, not per bracket pair"
        );
    }

    #[test]
    fn test_coverage_nunit_attributes_match_without_a_test_fixture_on_the_class() {
        let e = extract_src(
            "namespace App.Tests;\n\npublic class OrderServiceTests\n{\n  [Test]\n  public void Renders() { }\n\n  [TestCase(1, 2)]\n  public void Adds(int a, int b) { }\n\n  [TestCaseSource(nameof(Cases))]\n  public void Divides(int a) { }\n}\n",
        );
        let d = find_def(&e, "App.Tests.OrderServiceTests").expect("OrderServiceTests def present");
        assert_eq!(
            d.test_methods,
            vec!["Renders", "Adds", "Divides"],
            "NUnit makes the class attribute optional, so requiring one would drop real tests"
        );
    }

    #[test]
    fn test_coverage_lifecycle_data_source_and_class_container_attributes_never_mark_a_method() {
        let e = extract_src(
            "namespace App.Tests;\n\npublic class NotTests\n{\n  [SetUp]\n  public void Prepare() { }\n\n  [TearDown]\n  public void Cleanup() { }\n\n  [OneTimeSetUp]\n  public void Once() { }\n\n  [OneTimeTearDown]\n  public void Finally() { }\n\n  [TestInitialize]\n  public void Init() { }\n\n  [TestCleanup]\n  public void Done() { }\n\n  [InlineData(1)]\n  public void OnlyInline(int n) { }\n\n  [MemberData(nameof(Cases))]\n  public void OnlyMember(int n) { }\n\n  [ClassData(typeof(Cases))]\n  public void OnlyClassData(int n) { }\n\n  [DataRow(1)]\n  public void OnlyRow(int n) { }\n\n  [DynamicData(nameof(Cases))]\n  public void OnlyDynamic(int n) { }\n\n  [TestFixture]\n  public void FixtureOnAMethod() { }\n\n  [TestClass]\n  public void ClassMarkerOnAMethod() { }\n}\n",
        );
        let d = find_def(&e, "App.Tests.NotTests").expect("NotTests def present");
        assert!(d.test_methods.is_empty());
        assert_eq!(
            def_json_keys(d),
            vec!["id", "name", "namespace", "kind", "line", "methods"],
            "not one of these marks a test, so the key is absent entirely and the def keeps its pre-stage bytes"
        );
    }

    #[test]
    fn test_coverage_mstest_methods_count_only_inside_a_test_class() {
        let source = |class_attribute: &str| {
            format!(
                "namespace App.Tests;\n\n{class_attribute}public class CartTests\n{{\n  [TestMethod]\n  public void Places() {{ }}\n\n  [DataTestMethod]\n  [DataRow(1)]\n  public void Prices(int n) {{ }}\n}}\n"
            )
        };
        let ungated = extract_src(&source(""));
        assert!(
            find_def(&ungated, "App.Tests.CartTests").expect("CartTests def present").test_methods.is_empty(),
            "MSTest does not discover a [TestMethod] whose class lacks [TestClass] -- neither does this"
        );

        let gated = extract_src(&source("[TestClass]\n"));
        assert_eq!(
            find_def(&gated, "App.Tests.CartTests")
                .expect("CartTests def present")
                .test_methods,
            vec!["Places", "Prices"]
        );
    }

    #[test]
    fn test_coverage_a_nested_type_does_not_inherit_an_enclosing_test_class() {
        let e = extract_src(
            "namespace App.Tests;\n\n[TestClass]\npublic class OuterTests\n{\n  [TestMethod]\n  public void Outer() { }\n\n  public class Inner\n  {\n    [TestMethod]\n    public void Nested() { }\n  }\n}\n",
        );
        assert_eq!(
            find_def(&e, "App.Tests.OuterTests")
                .expect("OuterTests def present")
                .test_methods,
            vec!["Outer"]
        );
        assert!(
            find_def(&e, "App.Tests.OuterTests+Inner")
                .expect("Inner def present")
                .test_methods
                .is_empty(),
            "each type computes its own list from its OWN attribute_list"
        );
    }

    #[test]
    fn test_coverage_an_interface_or_enum_body_never_emits_test_methods() {
        let e = extract_src(
            "namespace App.Tests;\n\npublic interface ITestContract\n{\n  [Fact]\n  void Runs();\n}\n\npublic enum Speed\n{\n  Fast,\n  Slow,\n}\n",
        );
        assert!(find_def(&e, "App.Tests.ITestContract")
            .expect("interface def present")
            .test_methods
            .is_empty());
        assert!(find_def(&e, "App.Tests.Speed")
            .expect("enum def present")
            .test_methods
            .is_empty());
        assert!(find_def(&e, "App.Tests.Speed.Fast")
            .expect("enum-member def present")
            .test_methods
            .is_empty());
    }

    #[test]
    fn test_coverage_a_struct_and_a_record_carry_test_methods_too_and_a_local_function_never_does()
    {
        let e = extract_src(
            "namespace App.Tests;\n\npublic struct ValueTests\n{\n  [Fact]\n  public void Holds() { }\n}\n\npublic record RecordTests\n{\n  [Fact]\n  public void Keeps()\n  {\n    [Fact]\n    void Inner() { }\n  }\n}\n",
        );
        assert_eq!(
            find_def(&e, "App.Tests.ValueTests")
                .expect("struct def present")
                .test_methods,
            vec!["Holds"]
        );
        assert_eq!(
            find_def(&e, "App.Tests.RecordTests")
                .expect("record def present")
                .test_methods,
            vec!["Keeps"],
            "a local function is not a method_declaration at type body level"
        );
    }

    // --- v8: the ref `outer_types` enclosing-type stack -------------------

    fn find_ref<'a>(e: &'a Extraction, kind: &str, name: &str) -> Option<&'a RefRecord> {
        e.refs.iter().find(|r| r.kind == kind && r.name == name)
    }

    #[test]
    fn v8_a_ref_inside_a_nested_type_records_outer_types_outermost_first() {
        let e = extract_src(
            "namespace App.Core;\n\npublic class Outer\n{\n  public class Inner\n  {\n    private Marker _m;\n  }\n}\n\npublic class Marker { }\n",
        );
        let r = find_ref(&e, "uses-type", "Marker").expect("the field type ref is recorded");
        // Outermost first is the order type_id joins with "+", so the resolver
        // rebuilds a nested id by prefix rather than by reversing.
        assert_eq!(
            r.outer_types,
            vec!["Outer".to_string(), "Inner".to_string()]
        );
    }

    #[test]
    fn v8_a_namespace_level_ref_and_an_imports_ref_carry_no_outer_types() {
        let e = extract_src(
            "using App.Other;\n\nnamespace App.Core;\n\npublic class Widget : Marker { }\n",
        );
        assert!(find_ref(&e, "imports", "App.Other")
            .expect("using ref")
            .outer_types
            .is_empty());
        assert!(find_ref(&e, "inherits", "Marker")
            .expect("base ref")
            .outer_types
            .is_empty());
    }

    #[test]
    fn v8_a_member_ref_carries_outer_types_alongside_every_other_receiver_fact() {
        let e = extract_src(
            "namespace App.Core;\n\npublic class Outer\n{\n  public class Inner\n  {\n    private Store<int> _s;\n\n    public void Go()\n    {\n      _s.Add(1);\n      Box<int>.Make();\n    }\n  }\n}\n",
        );
        let with_receiver = e
            .refs
            .iter()
            .find(|r| r.member.as_deref() == Some("Add"))
            .expect("receiver-fact ref");
        assert_eq!(with_receiver.receiver_type.as_deref(), Some("Store"));
        assert_eq!(with_receiver.receiver_args, Some(vec!["int".to_string()]));
        assert_eq!(
            with_receiver.outer_types,
            vec!["Outer".to_string(), "Inner".to_string()]
        );
        // A generic qualifier earns no receiver fact; the stack lands on it all
        // the same.
        let generic = e
            .refs
            .iter()
            .find(|r| r.member.as_deref() == Some("Make"))
            .expect("generic-qualifier ref");
        assert!(generic.generic);
        assert_eq!(
            generic.outer_types,
            vec!["Outer".to_string(), "Inner".to_string()]
        );
    }

    #[test]
    fn v8_a_base_list_ref_carries_the_declaring_types_outer_stack_with_self_excluded() {
        let e = extract_src(
            "namespace App.Core;\n\npublic class Outer\n{\n  public class Inner : Marker\n  {\n  }\n}\n",
        );
        // The same stack record_type_def used to build Inner's own id.
        assert_eq!(
            find_ref(&e, "inherits", "Marker")
                .expect("base ref")
                .outer_types,
            vec!["Outer".to_string()]
        );
    }

    #[test]
    fn v8_a_dotted_chain_tail_ref_carries_the_sites_own_outer_types() {
        let e = extract_src(
            "namespace App.Core;\n\npublic class Outer\n{\n  public class Inner\n  {\n    public void Go() { Alpha.Beta.Gamma(); }\n  }\n}\n",
        );
        let tail = e
            .refs
            .iter()
            .find(|r| r.qualified.as_deref() == Some("Alpha.Beta"))
            .expect("chain-tail ref");
        // Positional: the stack of the SITE, never inherited from the head.
        assert_eq!(
            tail.outer_types,
            vec!["Outer".to_string(), "Inner".to_string()]
        );
    }

    // --- The per-file `names` list -------------------------------

    fn names_of(e: &Extraction) -> Vec<(&str, &str, usize, &str)> {
        e.names
            .iter()
            .map(|n| (n.name.as_str(), n.kind.as_str(), n.line, n.owner.as_str()))
            .collect()
    }

    #[test]
    fn names_record_every_member_kind_with_no_accessibility_filter() {
        let e = extract_src(
            "namespace Ns;\npublic class Ledger\n{\n\tprivate int _a, _b;\n\tpublic string Label { get; set; }\n\tpublic event EventHandler Retired;\n\tpublic event EventHandler Sealed { add { } remove { } }\n\tprivate void Hidden() { }\n}\n",
        );
        assert_eq!(
            names_of(&e),
            vec![
                ("_a", "field", 4, "Ns.Ledger"),
                ("_b", "field", 4, "Ns.Ledger"),
                ("Label", "property", 5, "Ns.Ledger"),
                ("Retired", "event", 6, "Ns.Ledger"),
                ("Sealed", "event", 7, "Ns.Ledger"),
                ("Hidden", "method", 8, "Ns.Ledger"),
            ]
        );
    }

    #[test]
    fn a_names_line_is_the_name_token_row_not_the_attribute_the_declaration_starts_at() {
        let e = extract_src(
            "namespace Ns;\npublic class Ledger\n{\n\t[Obsolete]\n\tpublic int Total { get; }\n}\n",
        );
        assert_eq!(
            names_of(&e),
            vec![("Total", "property", 5, "Ns.Ledger")],
            "line 5 is the name, line 4 is the attribute"
        );
    }

    #[test]
    fn overloads_are_two_names_and_a_nested_type_owns_its_own_members() {
        let e = extract_src(
            "namespace Ns;\npublic class Ledger\n{\n\tpublic void Add() { }\n\tpublic void Add(int n) { }\n\tpublic class Tally\n\t{\n\t\tpublic void Add(long n) { }\n\t}\n}\n",
        );
        assert_eq!(
            names_of(&e),
            vec![
                ("Add", "method", 4, "Ns.Ledger"),
                ("Add", "method", 5, "Ns.Ledger"),
                ("Add", "method", 8, "Ns.Ledger+Tally"),
            ],
            "two overloads are two declarations at two lines; the nested type's member is owned by the nested id"
        );
    }

    #[test]
    fn an_interface_body_and_an_enum_body_contribute_what_they_declare_and_nothing_else() {
        let e = extract_src("namespace Ns;\npublic interface ISink\n{\n\tvoid Accept();\n}\npublic enum Mode\n{\n\tOn,\n}\n");
        assert_eq!(
            names_of(&e),
            vec![("Accept", "method", 4, "Ns.ISink")],
            "an enum's members are defs already, never names rows"
        );
    }

    // -----------------------------------------------------------------
    // The TS/TSX reference fragment. The cross-file resolution
    // these facts feed is tsgraph.rs's own test module; everything here is
    // about what ONE file says, which is the whole of this extractor's job.
    // -----------------------------------------------------------------

    fn ts_fragment(src: &str, grammar: crate::parse::TsGrammar) -> TsFragment {
        let units = crate::parse::utf16_units(src);
        let tree = crate::parse::parse_ts_js(&units, grammar).expect("fixture parses");
        extract_ts_fragment(tree.root_node(), &crate::parse::utf16_bytes(&units))
    }

    fn ref_tuples(f: &TsFragment) -> Vec<(&str, &str, Option<&str>, usize)> {
        f.refs
            .iter()
            .map(|r| {
                (
                    r.kind.as_str(),
                    r.name.as_str(),
                    r.member.as_deref(),
                    r.line,
                )
            })
            .collect()
    }

    #[test]
    fn an_import_clause_records_default_named_aliased_and_namespace_bindings() {
        let f = ts_fragment(
            "import Thing, { one, two as alias } from './m';\nimport * as ns from './n';\n",
            crate::parse::TsGrammar::Typescript,
        );
        assert_eq!(f.imports.len(), 2);
        assert_eq!(f.imports[0].spec, "./m");
        assert_eq!(f.imports[0].line, 1);
        let first: Vec<(&str, &str)> = f.imports[0]
            .bindings
            .iter()
            .map(|b| (b.local.as_str(), b.imported.as_str()))
            .collect();
        assert_eq!(
            first,
            vec![("Thing", "default"), ("one", "one"), ("alias", "two")]
        );
        let second: Vec<(&str, &str)> = f.imports[1]
            .bindings
            .iter()
            .map(|b| (b.local.as_str(), b.imported.as_str()))
            .collect();
        assert_eq!(
            second,
            vec![("ns", "*")],
            "a namespace clause binds '*', never a name of its own"
        );
    }

    #[test]
    fn a_star_reexport_a_named_one_and_a_namespace_one_are_three_different_rows() {
        let f = ts_fragment(
            "export * from './a';\nexport { x as y } from './b';\nexport * as NS from './c';\n",
            crate::parse::TsGrammar::Typescript,
        );
        assert_eq!(f.reexports.len(), 3);
        assert!(f.reexports[0].star && f.reexports[0].names.is_empty());
        assert!(!f.reexports[1].star);
        assert_eq!(f.reexports[1].names[0].exported, "y");
        assert_eq!(f.reexports[1].names[0].imported, "x");
        assert!(
            !f.reexports[2].star && f.reexports[2].names.is_empty(),
            "`export * as NS` contributes its import edge and no name mapping"
        );
    }

    #[test]
    fn only_names_this_file_declares_earn_a_def_and_the_default_name_is_recorded_separately() {
        let f = ts_fragment(
            "export function run() {}\nconst helper = 1;\nexport { helper };\nexport { missing } from './elsewhere';\nexport default run;\n",
            crate::parse::TsGrammar::Typescript,
        );
        let defs: Vec<(&str, &str, usize)> = f
            .defs
            .iter()
            .map(|d| (d.name.as_str(), d.kind.as_str(), d.line))
            .collect();
        assert_eq!(defs, vec![("run", "function", 1), ("helper", "const", 2)]);
        assert_eq!(
            f.default.as_deref(),
            Some("run"),
            "one def, two importable names -- never a second row"
        );
    }

    #[test]
    fn a_commonjs_file_exports_by_name_and_binds_require_the_same_way_an_import_clause_does() {
        let f = ts_fragment(
            "const { logInfo } = require('./logger');\nconst bag = require('./logger');\nfunction report() { return logInfo(1) + bag.logInfo(2); }\nmodule.exports = { report };\n",
            crate::parse::TsGrammar::Javascript,
        );
        assert_eq!(
            f.defs.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            vec!["report"]
        );
        let bindings: Vec<(&str, &str)> = f
            .imports
            .iter()
            .flat_map(|i| i.bindings.iter())
            .map(|b| (b.local.as_str(), b.imported.as_str()))
            .collect();
        assert_eq!(bindings, vec![("logInfo", "logInfo"), ("bag", "*")]);
        assert_eq!(
            ref_tuples(&f),
            vec![
                ("call", "logInfo", None, 3),
                ("call", "bag", Some("logInfo"), 3)
            ],
            "a member call records the QUALIFIER and its property, never the chain's tail alone"
        );
    }

    #[test]
    fn a_dispatching_call_consumes_its_argument_so_it_is_never_also_a_plain_call() {
        let f = ts_fragment(
            "import { load, reset } from './actions';\nexport function go(dispatch) {\n  dispatch(load());\n  dispatch(reset);\n}\n",
            crate::parse::TsGrammar::Typescript,
        );
        assert_eq!(
            ref_tuples(&f),
            vec![
                ("dispatch", "load", None, 3),
                ("dispatch", "reset", None, 4)
            ],
            "the argument is the action creator; the outer dispatch(...) is not itself a call ref"
        );
    }

    #[test]
    fn a_jsx_tag_is_a_reference_only_when_it_names_a_component_and_that_name_is_known() {
        let f = ts_fragment(
            "import { Card, Panel } from './ui';\nexport function View() {\n  return <div><Card /><Panel.Header /><section /><Unknown /></div>;\n}\n",
            crate::parse::TsGrammar::Tsx,
        );
        assert_eq!(
            ref_tuples(&f),
            vec![
                ("jsx-use", "Card", None, 3),
                ("jsx-use", "Panel", Some("Header"), 3)
            ],
            "lowercase tags are intrinsic elements; an unbound capitalised one is not a known name"
        );
    }

    #[test]
    fn a_reference_to_a_name_this_file_neither_imports_nor_exports_is_never_recorded() {
        let f = ts_fragment(
            "export function go() {\n  const local = () => 1;\n  return local() + globalThing();\n}\n",
            crate::parse::TsGrammar::Typescript,
        );
        assert!(
            ref_tuples(&f).is_empty(),
            "the known-name filter is the contract, not an optimisation"
        );
    }

    #[test]
    fn a_new_expression_records_the_constructor_as_a_call() {
        let f = ts_fragment(
            "import { Service } from './service';\nexport const make = () => new Service();\n",
            crate::parse::TsGrammar::Typescript,
        );
        assert_eq!(ref_tuples(&f), vec![("call", "Service", None, 2)]);
    }

    #[test]
    fn a_file_with_no_default_export_keeps_the_shorter_fragment_shape() {
        let f = ts_fragment("export const x = 1;\n", crate::parse::TsGrammar::Typescript);
        assert_eq!(f.default, None);
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(
            json,
            r#"{"ts":1,"defs":[{"name":"x","kind":"const","line":1,"endLine":1}],"imports":[],"reexports":[],"refs":[]}"#
        );
    }

    #[test]
    fn the_fragment_serializes_ts_first_and_default_last() {
        let f = ts_fragment(
            "export function run() {}\nexport default run;\n",
            crate::parse::TsGrammar::Typescript,
        );
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(
            json,
            r#"{"ts":1,"defs":[{"name":"run","kind":"function","line":1,"endLine":1}],"imports":[],"reexports":[],"refs":[],"default":"run"}"#
        );
    }

    #[test]
    fn typescript_fragment_records_multiline_declaration_end_line() {
        let f = ts_fragment(
            "export interface Widget {\n  id: number;\n}\n",
            crate::parse::TsGrammar::Typescript,
        );
        assert_eq!((f.defs[0].line, f.defs[0].end_line), (1, 3));
    }

    // --- Property types and var-from-invocation ----------

    #[test]
    fn ds0012_def_records_property_types_in_source_order_with_generic_args() {
        let e = extract_src(
            r#"
namespace App.PropTypes;

public class Widget
{
  public Settings Config { get; set; }
  public string Label { get; set; }
  public Box<Gadget> Slots { get; set; }
  public Settings Config { get; set; }
}
"#,
        );
        let d = find_def(&e, "App.PropTypes.Widget").expect("Widget def present");
        assert_eq!(d.properties, vec!["Config", "Label", "Slots"]);
        let recorded: Vec<_> = d
            .property_types
            .iter()
            .map(|(n, f)| (n.as_str(), f.type_name.as_str(), f.args.as_ref()))
            .collect();
        // A predefined type vouches for nothing, exactly as it does for a local
        // or a method return, so `Label` has no entry -- the map is parallel to
        // `properties` but not equal in length.
        assert_eq!(
            recorded,
            vec![
                ("Config", "Settings", None),
                ("Slots", "Box", Some(&vec!["Gadget".to_string()]))
            ]
        );
    }

    #[test]
    fn ds0012_a_property_vouches_for_its_own_name_like_a_field() {
        let e = extract_src(
            r#"
namespace App.PropFacts;

public class Host
{
  private Widget _field;
  public Widget Current { get; set; }
  public string Name { get; set; }

  public void Run()
  {
    _field.Render();
    Current.Render();
    Name.Trim();
  }
}
"#,
        );
        // A predefined property type is still no fact, and the name stays
        // TAKEN: `Name` can never be read back as a TYPE named Name.
        assert_eq!(
            member_facts(&e),
            vec![
                ("Render", Some("Widget")),
                ("Render", Some("Widget")),
                ("Trim", None)
            ]
        );
    }

    #[test]
    fn ds0012_receiver_property_owner_is_recorded_only_for_a_typed_two_segment_chain() {
        let e = extract_src(
            r#"
namespace App.ChainHeads;

public class Host
{
  private Widget _widget;
  private string _text;

  public void Run(Widget param)
  {
    _widget.Config.Reload();
    param.Config.Reload();
    _text.Config.Reload();
    unknown.Config.Reload();
    _widget.Config.Inner.Reload();
    App.Other.Thing.Reload();
  }
}
"#,
        );
        let owners: Vec<_> = e
            .refs
            .iter()
            .filter(|r| r.kind == "uses-member")
            .map(|r| {
                (
                    r.qualified.as_deref().unwrap_or(r.name.as_str()),
                    r.receiver_property_owner.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            owners,
            vec![
                ("_widget.Config", Some("Widget")),
                // The head window itself is a BARE qualifier: a receiverType,
                // never an owner.
                ("_widget", None),
                ("param.Config", Some("Widget")),
                ("param", None),
                // Head typed `string`: no fact, so no owner.
                ("_text.Config", None),
                ("_text", None),
                // Head nothing in scope declares: no fact, so no owner.
                ("unknown.Config", None),
                ("unknown", None),
                // Three segments: only the innermost pair is a hop this fact
                // can start.
                ("_widget.Config.Inner", None),
                ("_widget.Config", Some("Widget")),
                ("_widget", None),
                // A namespace path is not a typed head.
                ("App.Other.Thing", None),
                ("App.Other", None),
                ("App", None),
            ]
        );
    }

    #[test]
    fn ds0010_var_from_a_qualified_invocation_records_the_callee_owner_and_member() {
        let e = extract_src(
            r#"
namespace App.CallFacts;

public class Host
{
  private Factory _factory;

  public void Run()
  {
    var made = _factory.Make();
    made.Render();
    var stat = Factory.Create();
    stat.Render();
    var bare = Compute();
    bare.Render();
    var chained = made.Wrap();
    chained.Render();
  }
}
"#,
        );
        let facts: Vec<_> = e
            .refs
            .iter()
            .filter(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Render"))
            .map(|r| {
                (
                    r.name.as_str(),
                    r.receiver_type.as_deref(),
                    r.receiver_call_owner.as_deref(),
                    r.receiver_call_member.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            facts,
            vec![
                // An instance callee: the owner is what the QUALIFIER's own
                // fact says.
                ("made", None, Some("Factory"), Some("Make")),
                // A static callee: nothing in scope claims the name, so the
                // qualifier text is itself the type name candidate.
                ("stat", None, Some("Factory"), Some("Create")),
                // A bare call has no qualifier to put through the ladder.
                ("bare", None, None, None),
                // The qualifier is itself one of these locals: one hop, never
                // a chain.
                ("chained", None, None, None),
            ]
        );
    }

    #[test]
    fn ds0010_a_conflicting_second_declaration_cancels_the_call_fact() {
        let e = extract_src(
            r#"
namespace App.CallConflict;

public class Host
{
  private Widget made;

  public void Run(bool flag)
  {
    if (flag) { var made = Factory.Make(); made.Render(); }
    else { Gadget made = new Gadget(); made.Render(); }
  }
}
"#,
        );
        let facts: Vec<_> = e
            .refs
            .iter()
            .filter(|r| r.kind == "uses-member" && r.member.as_deref() == Some("Render"))
            .map(|r| (r.receiver_type.as_deref(), r.receiver_call_owner.as_deref()))
            .collect();
        // Both windows read the same conflicted slot: no type fact, no call
        // fact, and no fall-through to the same-named FIELD of a different
        // type.
        assert_eq!(facts, vec![(None, None), (None, None)]);
    }

    // --- Foreach element type fact -----------------------------------

    #[test]
    fn ds0011_var_over_a_generic_collection_resolves_the_single_type_argument() {
        let e = extract_src(
            r#"
namespace App.ForEachVar;

public class Host
{
  private List<Widget> _field;

  public void Run(IEnumerable<Widget> param)
  {
    List<Widget> local = null;
    foreach (var a in _field) { a.Go(); }
    foreach (var b in param) { b.Go(); }
    foreach (var c in local) { c.Go(); }
  }
}
"#,
        );
        // A field, a parameter and a local: the same lookup the call
        // hop uses for its qualifier, reading whichever table vouches for the
        // collection's name.
        assert_eq!(
            member_facts(&e),
            vec![
                ("Go", Some("Widget")),
                ("Go", Some("Widget")),
                ("Go", Some("Widget"))
            ]
        );
    }

    #[test]
    fn ds0011_a_var_foreach_stays_unknown_unless_the_collection_is_a_bare_identifier_with_one_type_argument(
    ) {
        let e = extract_src(
            r#"
namespace App.ForEachUnknown;

public class Host
{
  private Widget[] _array;
  private Dictionary<string, Widget> _pair;
  private Map map;

  public void Run<T>(List<T> generic)
  {
    foreach (var a in GetItems()) { a.Go(); }
    foreach (var b in map.Items) { b.Go(); }
    foreach (var c in _array) { c.Go(); }
    foreach (var d in _pair) { d.Go(); }
    foreach (var f in generic) { f.Go(); }
  }

  private List<Widget> GetItems() { return null; }
}
"#,
        );
        // A call, a dotted chain, an array (no top-level type-argument list
        // at all), a two-argument generic (not a SINGLE type argument), and a
        // one-argument generic whose argument is the method's own type
        // parameter (the wildcard descriptor, "*" -- nothing at this site
        // knows what it is bound to) all leave the loop variable exactly as
        // taken-but-unknown as every other unresolvable local. Filtered to
        // `Go` alone: `map.Items` is itself an ordinary tier-(e) member
        // access on the FIELD `map` (declared type `Map`), unrelated to this
        // ticket, so it earns its own unaffected receiver fact.
        let go_facts: Vec<_> = e
            .refs
            .iter()
            .filter(|r| r.member.as_deref() == Some("Go"))
            .map(|r| r.receiver_type.as_deref())
            .collect();
        assert_eq!(go_facts, vec![None, None, None, None, None]);
    }

    #[test]
    fn ds0011_a_foreach_variable_used_as_another_foreachs_collection_is_never_a_fact() {
        let e = extract_src(
            r#"
namespace App.ForEachChain;

public class Host
{
  private List<Widget> _bag;

  public void Nested()
  {
    foreach (var outer in _bag)
    {
      outer.Go();
      foreach (var inner in outer) { inner.Go(); }
    }
  }
}
"#,
        );
        // `outer` resolves to a REAL fact (`_bag`'s single type argument), but
        // that derived fact never carries a type argument of its own (see
        // `collection_element_fact`), so `inner` -- one nested hop further --
        // finds no single argument to read and stays taken-but-unknown. One
        // hop, never a chain, structurally: a foreach element fact can be a
        // RECEIVER but never itself a collection another `var` derives from.
        assert_eq!(member_facts(&e), vec![("Go", Some("Widget")), ("Go", None)]);
    }

    #[test]
    fn ds0011_a_foreach_variable_conflicting_with_another_declaration_stays_taken_but_unknown() {
        let e = extract_src(
            r#"
namespace App.ForEachConflict;

public class Host
{
  private List<Widget> made;

  public void Run(bool flag)
  {
    if (flag) { foreach (var made in Gadgets()) { made.Go(); } }
    else { Widget made = new Widget(); made.Go(); }
  }

  private List<Gadget> Gadgets() { return null; }
}
"#,
        );
        // `made` is declared twice with conflicting shapes in sibling blocks
        // of the SAME method (a foreach-derived local in one arm, an
        // explicit local in the other): the flat member table collapses both
        // to the same taken-but-unknown slot -- the ordinary `add_fact`
        // conflict rule, unchanged by this ticket -- with no fall-through to
        // the enclosing field of the same name.
        assert_eq!(member_facts(&e), vec![("Go", None), ("Go", None)]);
    }

    #[test]
    fn ds0011_a_destructuring_foreach_variable_is_left_alone() {
        // `foreach (var (a, b) in pairs)` has no single name for the element
        // fact to name -- deliberately outside this ticket's scope, and its
        // absence must not disturb an ordinary sibling declaration's own fact.
        let e = extract_src(
            r#"
namespace App.ForEachTuple;

public class Host
{
  public void Run(List<(int, int)> pairs)
  {
    Widget w = new Widget();
    foreach (var (a, b) in pairs) { }
    w.Go();
  }
}
"#,
        );
        assert_eq!(member_facts(&e), vec![("Go", Some("Widget"))]);
    }
}
