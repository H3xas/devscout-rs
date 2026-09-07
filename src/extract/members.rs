use std::collections::HashSet;

use tree_sitter::Node;

use super::receivers::type_fact;
use super::refs::{
    base_type_identifier, generic_arg_descriptors, type_descriptor, type_parameter_names,
};
use super::text::{declared_name, is_public, named_children, text};
use super::types::{ExtensionMethod, Fact};

// Deliberately NOT public_method_names: that one strips a trailing "Async"
// and dedupes across the whole file for purpose-signature compactness. A
// graph def needs the real name, unabridged.
pub(super) fn raw_method_names(node: Node, src: &[u8], kind: &str) -> Vec<String> {
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

// `non_public_methods`: the exact complement of `is_recorded_method` among
// method_declaration nodes -- every method NOT recorded in `methods`. An
// interface's methods are ALL recorded as public by `is_recorded_method`
// (the `kind == "interface"` short-circuit), so this list is always empty
// for an interface, by construction rather than by a second check here.
pub(super) fn raw_non_public_method_names(node: Node, src: &[u8], kind: &str) -> Vec<String> {
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    named_children(body)
        .into_iter()
        .filter(|c| c.kind() == "method_declaration" && !is_recorded_method(*c, src, kind))
        .map(|c| declared_name(c, src))
        .filter(|n| !n.is_empty())
        .collect()
}

// Every `method_declaration`'s own (name, arity RANGE) fact, regardless of
// accessibility -- unlike `raw_method_returns`/`is_recorded_method`, this is
// NOT filtered to public methods: the resolver's arity-aware call vouching
// (Unit A4 item 2) needs an overload's range whether `methods` or
// `non_public_methods` is the list answering "does this def declare the
// name". One (name, ranges) pair per DISTINCT name, in first-occurrence
// source order (a `Vec` of pairs, not a map: the serialized key order is
// significant, same reason as `method_returns`); `ranges` collects EVERY
// overload sharing that name, each its own (min, max) tuple, in declaration
// order -- unlike `method_returns`'s first-wins gate, a later overload's
// range is never discarded, since the resolver needs the OR of every
// overload to answer "does some overload admit N arguments". `max` uses the
// same -1-for-unbounded sentinel as `ExtensionMethod::arity_max`. A
// `parameters` field that cannot be read (not expected for a valid
// `method_declaration`, but the extractor never assumes a shape it has not
// verified) contributes the unbounded range (0, -1) rather than no entry at
// all -- precision-first: an arity fact the extractor could not read must
// never silently NARROW a call the resolver would otherwise decline to
// widen.
pub(super) fn raw_method_arities(node: Node, src: &[u8]) -> Vec<(String, Vec<(usize, i64)>)> {
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut pairs: Vec<(String, Vec<(usize, i64)>)> = Vec::new();
    for c in named_children(body) {
        if c.kind() != "method_declaration" {
            continue;
        }
        let name = declared_name(c, src);
        if name.is_empty() {
            continue;
        }
        let range = match c.child_by_field_name("parameters") {
            Some(parameters) => method_arity_range(parameters, src),
            None => (0, -1),
        };
        match pairs.iter_mut().find(|(n, _)| *n == name) {
            Some((_, ranges)) => ranges.push(range),
            None => pairs.push((name, vec![range])),
        }
    }
    pairs
}

// Every `method_declaration`'s own (name, per-overload parameter
// descriptors) fact, regardless of accessibility -- same method set as
// `raw_method_arities` (all visibilities, one entry per overload, in
// declaration order). A parameter carrying the `this` modifier records
// `"this <descriptor>"` -- the same modifier `raw_extension_methods` itself
// reads off the first parameter -- and `params`/`ref`/`out`/`in` modifiers
// are ignored (a plain descriptor). The type-parameter set for one method
// is the enclosing type's own (`type_params`, the same set
// `raw_extension_methods` starts its own per-method union from) plus this
// method's OWN type parameters unioned on top, exactly like
// `raw_extension_methods`'s `this_args` capture does.
//
// A `delegate_declaration` (no body at all) is the one caller-recognized
// exception: it returns exactly one entry, `("Invoke", ...)`, built from
// the delegate's own `parameters` field and its own type parameters unioned
// onto `type_params` the same way -- callers pass an empty enclosing set for
// a delegate, same as `record_type_def` does for every other
// per-declaration fact of one.
pub(super) fn raw_method_params(
    node: Node,
    src: &[u8],
    type_params: &HashSet<String>,
) -> Vec<(String, Vec<Vec<String>>)> {
    if node.kind() == "delegate_declaration" {
        let Some(parameters) = node.child_by_field_name("parameters") else {
            return Vec::new();
        };
        let mut own_type_params = type_params.clone();
        own_type_params.extend(type_parameter_names(node, src));
        return vec![(
            "Invoke".to_string(),
            vec![method_param_descriptors(parameters, src, &own_type_params)],
        )];
    }
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut pairs: Vec<(String, Vec<Vec<String>>)> = Vec::new();
    for c in named_children(body) {
        if c.kind() != "method_declaration" {
            continue;
        }
        let name = declared_name(c, src);
        if name.is_empty() {
            continue;
        }
        let mut own_type_params = type_params.clone();
        own_type_params.extend(type_parameter_names(c, src));
        let descriptors = match c.child_by_field_name("parameters") {
            Some(parameters) => method_param_descriptors(parameters, src, &own_type_params),
            None => Vec::new(),
        };
        match pairs.iter_mut().find(|(n, _)| *n == name) {
            Some((_, overloads)) => overloads.push(descriptors),
            None => pairs.push((name, vec![descriptors])),
        }
    }
    pairs
}

// One parameter list's descriptors, in source order -- covers the ORDINARY
// `parameter` node shape AND the flattened `params`-array shape the
// grammar uses instead of wrapping a `params` parameter in its own
// `parameter` node (see `parameter_arity_range`'s own doc comment: a
// `params` array's `type`/`name` land as direct FIELD-tagged children of
// the parameter_list itself, never inside a nested `parameter` node). A
// `this` modifier writes `"this <descriptor>"`; every other modifier
// (`params`, `ref`, `out`, `in`) is ignored -- a plain descriptor.
fn method_param_descriptors(
    parameters: Node,
    src: &[u8],
    type_params: &HashSet<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0..parameters.child_count() as u32 {
        let Some(c) = parameters.child(i) else {
            continue;
        };
        if c.kind() == "parameter" {
            let is_this = named_children(c)
                .iter()
                .any(|m| m.kind() == "modifier" && text(*m, src) == "this");
            let descriptor = type_descriptor(c.child_by_field_name("type"), src, type_params);
            out.push(if is_this {
                format!("this {descriptor}")
            } else {
                descriptor
            });
            continue;
        }
        // The flattened `params`-array shape: its type is a direct child
        // of the parameter_list tagged with the list's own `type` field
        // (its `name` field carries the flattened parameter's NAME the
        // same way -- skipped here, one descriptor per position, not two).
        if parameters.field_name_for_child(i) == Some("type") {
            out.push(type_descriptor(Some(c), src, type_params));
        }
    }
    out
}

// Declared property names, source order, deduped. Indexers are
// a different grammar node (indexer_declaration) so they are excluded by
// construction; expression-bodied properties are property_declaration like
// any other, so they are included. No accessibility filter -- see
// DefRecord::properties.
pub(super) fn raw_property_names(node: Node, src: &[u8]) -> Vec<String> {
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
pub(super) fn raw_property_types(
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
pub(super) fn raw_field_names(node: Node, src: &[u8]) -> Vec<String> {
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

// (field name, fact) pairs for exactly the fields `raw_field_names`
// records, in the same source order and under the same dedup -- a field
// whose declared type yields no fact (a predefined type) simply has no
// entry. Every declarator of one `field_declaration` shares that
// declaration's own type node ("private int a, b;" gives `a` and `b` the
// SAME fact), the same sharing `raw_field_names`'s own declarator loop
// already relies on. Mirrors `raw_property_types` field for field, so a
// field-typed receiver goes through the exact same resolution shape a
// property-typed one already does.
pub(super) fn raw_field_types(
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
        if c.kind() != "field_declaration" {
            continue;
        }
        let Some(vd) = named_children(c)
            .into_iter()
            .find(|k| k.kind() == "variable_declaration")
        else {
            continue;
        };
        let Some(fact) = type_fact(vd.child_by_field_name("type"), src, type_params) else {
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
            pairs.push((name, fact.clone()));
        }
    }
    pairs
}

// (name, returnTypeName) pairs in first-declaration order, for
// exactly the methods `raw_method_names` records. FIRST declaration of a name
// claims the slot outright: a later overload with a different return type is
// ignored, and a first declaration whose return type yields no fact (void,
// var, a predefined type) BLOCKS the name rather than letting a later
// overload's return type stand in for it -- picking a non-first overload
// would be a guess.
pub(super) fn raw_method_returns(node: Node, src: &[u8], kind: &str) -> Vec<(String, String)> {
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

// The generic-argument descriptors `raw_method_returns` throws away, keyed
// by the same method name, under the exact same first-declaration-wins gate
// (a name's `seen` slot is claimed by its FIRST declaration whether or not
// that declaration turns out to carry a type-argument list, so a later
// overload can never contribute an entry `raw_method_returns` itself would
// have ignored). Every name recorded here is one `raw_method_returns` also
// recorded a plain return-type name for; a return type with no top-level
// type-argument list at all contributes no entry, exactly like
// `raw_base_generic_args` does for a non-generic base.
pub(super) fn raw_method_return_args(
    node: Node,
    src: &[u8],
    kind: &str,
    type_params: &HashSet<String>,
) -> Vec<(String, Vec<String>)> {
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
        let returns_node = c.child_by_field_name("returns");
        if base_type_identifier(returns_node, src, false).is_none() {
            continue;
        }
        if let Some(args) = generic_arg_descriptors(returns_node, src, type_params) {
            pairs.push((name, args));
        }
    }
    pairs
}

// The two halves of a parameter list's acceptable argument COUNT, read off
// the list as written:
//   min -- parameters a caller cannot leave out: no default value AND no
//     `params` modifier.
//   max -- the total parameter count, or -1 (unbounded) when the trailing
//     parameter is a `params` array.
// A single exact arity would under-match every optional-parameter and
// `params` call site and -- worse -- make two classes (or two overloads)
// look like ONE candidate when only one of them could actually bind the
// call: the false-uniqueness shape.
//
// `skip_first` is `extension_arity_range`'s own need: an extension method's
// FIRST `parameter` node is its this-parameter (the caller has already
// verified its `this` modifier), never counted toward either half.
// `method_arity_range` (an ordinary, non-extension overload) passes `false`
// -- every parameter counts, there is no this-parameter to skip.
//
// The loop reads ALL children, not named_children, because tree-sitter-c-sharp
// does NOT wrap a `params` parameter in a `parameter` node: it emits a bare
// anonymous `params` token followed by that parameter's type and name nodes as
// direct children of the parameter_list. A named-children count therefore reads
// `(this T t, params X[] xs)` as THREE parameters and a `parameter`-node count
// reads it as one; the token itself is the only reliable signal, and since C#
// requires `params` to be last, seeing it at all means unbounded.
fn parameter_arity_range(parameters: Node, src: &[u8], skip_first: bool) -> (usize, i64) {
    let mut total: i64 = 0;
    let mut min: usize = 0;
    let mut unbounded = false;
    let mut skip_next = skip_first;
    let mut cursor = parameters.walk();
    for c in parameters.children(&mut cursor) {
        if c.kind() == "params" {
            unbounded = true;
            continue;
        }
        if c.kind() != "parameter" {
            continue;
        }
        if skip_next {
            skip_next = false;
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

fn extension_arity_range(parameters: Node, src: &[u8]) -> (usize, i64) {
    parameter_arity_range(parameters, src, true)
}

// The same range, for an ORDINARY (non-extension) method overload: every
// parameter counts, there is no this-parameter to skip. Unit A4 item 2's own
// input -- `raw_method_arities` calls this once per `method_declaration`,
// public and non-public alike.
fn method_arity_range(parameters: Node, src: &[u8]) -> (usize, i64) {
    parameter_arity_range(parameters, src, false)
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
pub(super) fn raw_extension_methods(node: Node, src: &[u8]) -> Vec<ExtensionMethod> {
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
pub(super) fn raw_base_names(node: Node, src: &[u8]) -> Vec<String> {
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
pub(super) fn raw_base_generic_args(
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
pub(super) fn raw_test_methods(node: Node, src: &[u8], kind: &str) -> Vec<String> {
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
