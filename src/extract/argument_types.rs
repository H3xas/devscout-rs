use tree_sitter::Node;

use super::qualifiers::{member_name_text, resolve_member_qualifier, Scope};
use super::refs::{invocation_arguments, type_descriptor};
use super::text::{named_children, text};
use super::types::{ArgTypeRecord, Fact, RefRecord};

pub(super) fn stamp_arg_types(
    refs: &mut [RefRecord],
    callee: Node,
    src: &[u8],
    scope: &Scope,
    type_stack: &[String],
) {
    let Some(arguments) = invocation_arguments(callee) else {
        return;
    };
    let Some(last) = refs.last_mut() else { return };
    if arguments
        .iter()
        .any(|a| a.child_by_field_name("name").is_some())
    {
        return;
    }
    last.arg_types = Some(
        arguments
            .into_iter()
            .map(|a| argument_type(a, src, scope, type_stack))
            .collect(),
    );
}

fn argument_type(
    argument: Node,
    src: &[u8],
    scope: &Scope,
    type_stack: &[String],
) -> Option<ArgTypeRecord> {
    if argument.child_by_field_name("name").is_some() {
        return None;
    }
    let raw = text(argument, src);
    if ["ref ", "out ", "in "]
        .iter()
        .any(|prefix| raw.trim_start().starts_with(prefix))
    {
        return None;
    }
    let expression = if argument.kind() == "argument" {
        named_children(argument).into_iter().next()?
    } else {
        argument
    };
    expression_type(expression, src, scope, type_stack)
}

fn expression_type(
    expression: Node,
    src: &[u8],
    scope: &Scope,
    type_stack: &[String],
) -> Option<ArgTypeRecord> {
    let known = |name: &str| Some(ArgTypeRecord::Known(name.to_string()));
    match expression.kind() {
        "string_literal"
        | "raw_string_literal"
        | "verbatim_string_literal"
        | "interpolated_string_expression" => known("string"),
        "character_literal" => known("char"),
        "boolean_literal" => known("bool"),
        "null_literal" => known("null"),
        "integer_literal" => known(integer_type(&text(expression, src))),
        "real_literal" => known(real_type(&text(expression, src))),
        "typeof_expression" => known("Type"),
        "object_creation_expression" | "cast_expression" | "as_expression" => {
            let type_node = expression.child_by_field_name("type").or_else(|| {
                (expression.kind() == "as_expression")
                    .then(|| named_children(expression).into_iter().last())
                    .flatten()
            });
            let descriptor = type_descriptor(type_node, src, &scope.type_params);
            (descriptor != "?").then_some(ArgTypeRecord::Known(descriptor))
        }
        "parenthesized_expression" => named_children(expression)
            .into_iter()
            .next()
            .and_then(|inner| expression_type(inner, src, scope, type_stack)),
        "identifier" => {
            let name = text(expression, src);
            scope.argument_fact_for(&name, src).and_then(fact_type)
        }
        "member_access_expression" => member_type(expression, false, src, scope, type_stack),
        "invocation_expression" => {
            let function = expression.child_by_field_name("function")?;
            member_type(function, true, src, scope, type_stack)
        }
        _ => None,
    }
}

fn fact_type(fact: Fact) -> Option<ArgTypeRecord> {
    if let Some(member) = fact.call {
        return Some(ArgTypeRecord::Member {
            owner: fact.type_name,
            member,
            receiver_args: fact.args,
            call: true,
        });
    }
    if fact.lambda.is_some() || fact.type_name.is_empty() {
        return None;
    }
    if let Some(descriptor) = fact.descriptor.filter(|d| d != "?") {
        return Some(ArgTypeRecord::Known(descriptor));
    }
    let mut descriptor = fact.type_name;
    if let Some(args) = fact.args {
        descriptor.push('<');
        descriptor.push_str(&args.join(","));
        descriptor.push('>');
    }
    if fact.is_array {
        descriptor.push_str("[]");
    }
    Some(ArgTypeRecord::Known(descriptor))
}

fn member_type(
    expression: Node,
    call: bool,
    src: &[u8],
    scope: &Scope,
    type_stack: &[String],
) -> Option<ArgTypeRecord> {
    if expression.kind() != "member_access_expression" {
        return None;
    }
    let qualifier = expression.child_by_field_name("expression")?;
    let resolved = resolve_member_qualifier(Some(qualifier), src, type_stack, scope)?;
    let member = member_name_text(expression.child_by_field_name("name"), src)?;
    let (owner, receiver_args) = if let Some(fact) = resolved.receiver {
        if fact.call.is_some() || fact.lambda.is_some() {
            return None;
        }
        (fact.type_name, fact.args)
    } else {
        if qualifier.kind() == "identifier" && scope.name_is_claimed(&resolved.text, src) {
            return None;
        }
        (resolved.text, None)
    };
    Some(ArgTypeRecord::Member {
        owner,
        member,
        receiver_args,
        call,
    })
}

fn integer_type(value: &str) -> &'static str {
    let suffix = value.trim_end_matches(|c: char| c.is_ascii_digit() || c == '_');
    let lower = suffix.to_ascii_lowercase();
    if lower.ends_with("ul") || lower.ends_with("lu") {
        "ulong"
    } else if lower.ends_with('u') {
        "uint"
    } else if lower.ends_with('l') {
        "long"
    } else {
        "int"
    }
}

fn real_type(value: &str) -> &'static str {
    match value.chars().last().map(|c| c.to_ascii_lowercase()) {
        Some('f') => "float",
        Some('m') => "decimal",
        _ => "double",
    }
}
