use std::collections::HashSet;

use tree_sitter::Node;

use super::qualifiers::{
    member_name_text, resolve_call_chain_tail, resolve_member_qualifier, Scope, MEMBER_SCOPE_NODES,
};
use super::receivers::type_fact;
use super::refs::{
    invocation_arg_count, invocation_lambda_arg_arity, push_ctor_param_ref, push_member_ref,
    record_base_list, record_single_type, registration_fact, type_parameter_names,
};
use super::text::{
    declared_name, format_segments, named_children, namespace_level_types, new_parser, text,
    type_kind_label, type_segment_parts, SegmentParts,
};
use super::type_defs::{record_enum_members, record_type_def, record_using};
use super::types::{Extraction, Fact};

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
#[allow(
    clippy::too_many_lines,
    reason = "one recursive descent over every node kind the extractor cares about, sharing the same ns/type_stack/scope state at each depth"
)]
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
            // A registration fact needs only that this access is
            // actually invoked -- the shape rule itself (name pattern, exactly
            // two type arguments) lives in `registration_fact`. Independent of
            // the uses-member candidate built below: the same call keeps
            // recording its ordinary uses-member/uses-type refs unchanged.
            if invocation_arg_count(node).is_some() {
                if let Some(reg) = registration_fact(node, ns, src) {
                    out.registrations.push(reg);
                }
            }
            let expr_field = node.child_by_field_name("expression");
            // The member itself can be a generic_name too ("Foo.Bar<T>(...)"):
            // normalize to the bare method name so the resolver's method-list
            // membership check sees the name the def actually recorded.
            let member = member_name_text(node.child_by_field_name("name"), src);
            let qualifier = resolve_member_qualifier(expr_field, src, type_stack, scope);
            if let (Some(q), Some(m)) = (&qualifier, &member) {
                if !m.is_empty() {
                    push_member_ref(
                        &mut out.refs,
                        &q.text,
                        m.clone(),
                        node.start_position().row + 1,
                        ns.to_string(),
                        q.generic,
                        q.receiver.clone(),
                        // Asked of THIS node, so a chain window answers for its
                        // own call and never for the one wrapping it.
                        invocation_arg_count(node),
                        type_stack,
                        q.property_owner.clone(),
                        q.receiver_base,
                        q.receiver_local,
                        invocation_lambda_arg_arity(node),
                    );
                }
            } else if let (Some(qn), Some(m)) = (expr_field, &member) {
                // Unit C, chain tail: `resolve_member_qualifier` structurally
                // NEVER succeeds here (`member_qualifier_info` has no arm
                // for `invocation_expression`), so this is a true fallback,
                // never a double emission for the same window.
                if !m.is_empty() {
                    if let Some((owner, inner_member, head_is_base)) =
                        resolve_call_chain_tail(qn, src, type_stack, scope)
                    {
                        push_member_ref(
                            &mut out.refs,
                            // The qualifier's OWN source text ("a.B()",
                            // "Repo.Load()", ...) rather than a name that
                            // could coincidentally collide with a real
                            // type: an invocation_expression's span always
                            // ends at its own closing ')', a character no
                            // C# identifier or dotted type name can ever
                            // contain, so `push_member_ref`'s `name`/
                            // `qualified` split can never accidentally
                            // resolve as a real def. `receiver_type` stays
                            // `None` (a call-shaped Fact, not a resolved
                            // type) -- resolution goes through the same
                            // one-hop `method_returns` path a `var x =
                            // Q.M()` local already uses.
                            &text(qn, src),
                            m.clone(),
                            node.start_position().row + 1,
                            ns.to_string(),
                            false,
                            Some(Fact {
                                type_name: owner,
                                args: None,
                                call: Some(inner_member),
                                awaited: false,
                                is_array: false,
                                nullable: false,
                                lambda: None,
                            }),
                            invocation_arg_count(node),
                            type_stack,
                            None,
                            // `base.Make().Validate()`: the hop must read
                            // Make's return type off the BASE that declares
                            // it, never off an enclosing type that hides
                            // Make with its own.
                            head_is_base,
                            false,
                            invocation_lambda_arg_arity(node),
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
        // `a?.B` / `a?.B(...)` -- a conditional-access window emits the
        // SAME uses-member ref an unconditional `a.B` would, through the
        // identical qualifier resolution (`resolve_member_qualifier`); only
        // the line (this node's own start row) and the arg-count/binding
        // shapes differ structurally. `member_binding_expression` is a
        // CHILD of this node, not a field (confirmed against the shipped
        // grammar's node-types), so it is found by kind rather than
        // `child_by_field_name`. `element_binding_expression` (`a?[i]`) is
        // the conditional_access_expression's other possible child and is
        // not a member access at all -- no ref for it here.
        "conditional_access_expression" => {
            let condition = node.child_by_field_name("condition");
            let binding = named_children(node)
                .into_iter()
                .find(|c| c.kind() == "member_binding_expression");
            if let Some(binding) = binding {
                let member = member_name_text(binding.child_by_field_name("name"), src);
                let qualifier = resolve_member_qualifier(condition, src, type_stack, scope);
                if let (Some(q), Some(m)) = (&qualifier, &member) {
                    if !m.is_empty() {
                        push_member_ref(
                            &mut out.refs,
                            &q.text,
                            m.clone(),
                            node.start_position().row + 1,
                            ns.to_string(),
                            q.generic,
                            q.receiver.clone(),
                            // Asked of the conditional_access_expression
                            // itself -- that is the node an enclosing
                            // `invocation_expression`'s `function` field
                            // names when the binding is invoked
                            // (`a?.B()`), never the inner binding.
                            invocation_arg_count(node),
                            type_stack,
                            q.property_owner.clone(),
                            q.receiver_base,
                            q.receiver_local,
                            invocation_lambda_arg_arity(node),
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
///
/// The source is first stripped of inactive conditional-compilation arms,
/// so only the arm that would actually compile is indexed.
pub fn extract(source: &str) -> Extraction {
    let mut parser = new_parser();
    // At most one arm of every `#if` group reaches the parser (see `preproc`).
    let source = crate::preproc::strip_inactive(source);
    let source = source.as_ref();
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
        registrations: Vec::new(),
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
