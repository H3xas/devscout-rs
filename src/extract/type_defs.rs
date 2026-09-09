use std::collections::HashSet;

use tree_sitter::Node;

use super::members::{
    raw_base_generic_args, raw_base_names, raw_extension_methods, raw_field_names, raw_field_types,
    raw_method_arities, raw_method_names, raw_method_params, raw_method_return_args,
    raw_method_returns, raw_non_public_method_names, raw_override_method_names, raw_property_names,
    raw_property_types, raw_test_methods,
};
use super::refs::{push_ref, type_parameter_names_ordered};
use super::text::{declared_name, named_children, text};
use super::types::{DefRecord, NameRecord, RefRecord, UsingRecord};

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
pub(super) fn record_type_def(
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
    // type_params and base_generic_args, then testMethods, then propertyTypes,
    // fieldTypes and methodReturnArgs, then nonPublicMethods, then
    // methodArities, then methodParams. Each is omitted when empty, so a
    // type with none of them serializes exactly as it did before those
    // additions.
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
        field_types: raw_field_types(node, src, type_params),
        method_return_args: raw_method_return_args(node, src, kind, type_params),
        non_public_methods: raw_non_public_method_names(node, src, kind),
        method_arities: raw_method_arities(node, src),
        method_params: raw_method_params(node, src, type_params),
        override_methods: raw_override_method_names(node, src),
        end_line: node.end_position().row + 1,
    });
}

// Enum members are cheap to record on the same walk. Each member becomes
// its own def, id'd as "<EnumId>.<Member>" -- appending with "." even when
// EnumId itself carries a "+"-joined nested-type suffix, so a member id can
// never collide with the "+"-joined nested-type scheme.
pub(super) fn record_enum_members(
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
            field_types: Vec::new(),
            method_return_args: Vec::new(),
            non_public_methods: Vec::new(),
            method_arities: Vec::new(),
            method_params: Vec::new(),
            override_methods: Vec::new(),
            end_line: member.end_position().row + 1,
        });
    }
}

pub(super) fn record_using(
    node: Node,
    src: &[u8],
    usings: &mut Vec<UsingRecord>,
    refs: &mut Vec<RefRecord>,
) {
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
