use std::collections::HashMap;

use crate::graph::FragRef;

use super::argument_types::{materialize, substitute_receiver_type};
use super::arity::{call_arity, method_call_admits};
use super::edges::type_probe;
use super::index::{DefIndex, MethodOverloadParams};
use super::ladder::{resolve_ref, Resolution};
use super::lambda_arity::{lambda_arity_admits, lambda_overload_admits};
use super::members::inheritance_walk_find;
use super::scope::FileContext;

pub(super) fn call_admits(
    index: &DefIndex,
    contexts: &HashMap<String, FileContext>,
    idx: usize,
    r: &FragRef,
    source_file: &str,
) -> bool {
    let Some(call) = call_arity(r) else {
        return true;
    };
    let Some(member) = r.member.as_deref() else {
        return false;
    };
    let Some(overloads) = index.member_lists[idx].method_params.get(member) else {
        return method_call_admits(index, idx, member, call)
            && lambda_arity_admits(index, contexts, idx, Some(member), r);
    };
    overloads.iter().any(|overload| {
        overload_count_admits(overload, call.values, call.type_args)
            && lambda_overload_admits(index, contexts, idx, overload, r)
            && argument_positions_admit(index, contexts, idx, overload, r, source_file)
    })
}

fn overload_count_admits(
    overload: &MethodOverloadParams,
    values: usize,
    type_args: Option<usize>,
) -> bool {
    let range_ok = overload
        .arity
        .is_none_or(|(min, max)| min <= values && (max == -1 || (values as i64) <= max));
    let type_args_ok = type_args.is_none_or(|written| {
        overload
            .type_params
            .as_ref()
            .is_none_or(|own| own.count == written)
    });
    range_ok && type_args_ok
}

fn argument_positions_admit(
    index: &DefIndex,
    contexts: &HashMap<String, FileContext>,
    idx: usize,
    overload: &MethodOverloadParams,
    r: &FragRef,
    source_file: &str,
) -> bool {
    let Some(args) = &r.arg_types else {
        return true;
    };
    args.iter().enumerate().all(|(position, arg)| {
        let Some(arg) = arg else { return true };
        let Some(param) = overload.params.get(position) else {
            return true;
        };
        if overload
            .arity
            .is_some_and(|(_, max)| max == -1 && position + 1 >= overload.params.len())
        {
            return true;
        }
        let Some(arg_type) = materialize(arg, source_file, r, index, contexts) else {
            return true;
        };
        if param.contains('*')
            && overload.type_params.as_ref().is_none_or(|own| {
                own.sites
                    .get(position)
                    .is_none_or(|sites| !sites.is_empty())
            })
        {
            // The shared wildcard can name a method parameter. An absent or
            // occupied site cannot be bound from the declaring type's receiver.
            return true;
        }
        let binding = if r.receiver_type.as_deref() == Some(index.defs[idx].name.as_str())
            || r.name == index.defs[idx].name
        {
            r.receiver_args.as_deref()
        } else {
            None
        };
        let Some(param_type) =
            substitute_receiver_type(param, binding, index.member_lists[idx].type_params.len())
        else {
            return true;
        };
        can_accept(
            &CompareContext {
                index,
                contexts,
                source_file,
                param_file: &overload.file,
                reference: r,
                owner_idx: idx,
            },
            &arg_type,
            &param_type,
        )
    })
}

struct CompareContext<'a> {
    index: &'a DefIndex,
    contexts: &'a HashMap<String, FileContext>,
    source_file: &'a str,
    param_file: &'a str,
    reference: &'a FragRef,
    owner_idx: usize,
}

fn can_accept(cx: &CompareContext<'_>, arg: &str, param: &str) -> bool {
    let arg = canonical_primitives(arg);
    let param = canonical_primitives(param);
    let (arg, param) = (arg.as_str(), param.as_str());
    let CompareContext {
        index,
        contexts,
        source_file,
        param_file,
        reference: r,
        owner_idx: idx,
    } = cx;
    let idx = *idx;
    if arg == param || arg == "?" || param == "?" || arg.contains('*') || param.contains('*') {
        return true;
    }
    if matches!(param, "object" | "dynamic") || matches!(arg, "dynamic") {
        return true;
    }
    if arg == "null" {
        return true;
    }
    let arg_head = descriptor_head(arg);
    let param_head = descriptor_head(param);
    if arg_head == param_head {
        return same_head_admits(cx, arg, param, arg_head);
    }
    if numeric(arg_head) && numeric(param_head) {
        return true;
    }
    let arg_def = resolve_type(
        index,
        contexts,
        source_file,
        r.namespace.as_deref().unwrap_or_default(),
        &r.outer_types,
        arg_head,
    );
    let param_def = resolve_type(
        index,
        contexts,
        param_file,
        &index.defs[idx].namespace,
        &[],
        param_head,
    );
    if let (Some(a), Some(p)) = (arg_def, param_def) {
        if inheritance_walk_find(index, contexts, a, |candidate| candidate == p).is_some() {
            return true;
        }
        return has_conversion(index, a)
            || has_conversion(index, p)
            || !index.member_lists[a].bases.is_empty()
            || arg.contains('<')
            || param.contains('<');
    }
    if let (None, Some(p)) = (arg_def, param_def) {
        if fixed_bcl(arg_head).is_some() && !numeric(arg_head) && !has_conversion(index, p) {
            return false;
        }
    }
    if let (Some(a), None) = (arg_def, param_def) {
        if fixed_bcl(param_head).is_some()
            && !has_conversion(index, a)
            && index.member_lists[a].bases.is_empty()
        {
            return false;
        }
    }
    if arg_def.is_none() && param_def.is_none() {
        if let (Some(a), Some(p)) = (fixed_bcl(arg_head), fixed_bcl(param_head)) {
            return a == p;
        }
    }
    true
}

fn same_head_admits(cx: &CompareContext<'_>, arg: &str, param: &str, head: &str) -> bool {
    if !arg.contains('<') || !param.contains('<') {
        return true;
    }
    // Receiver facts can retain only the generic head of a nested type
    // argument. That lost binding cannot prove an invariant mismatch.
    if param.matches('<').count() < arg.matches('<').count() {
        return true;
    }
    let a = resolve_type(
        cx.index,
        cx.contexts,
        cx.source_file,
        cx.reference.namespace.as_deref().unwrap_or_default(),
        &cx.reference.outer_types,
        head,
    );
    let p = resolve_type(
        cx.index,
        cx.contexts,
        cx.param_file,
        &cx.index.defs[cx.owner_idx].namespace,
        &[],
        head,
    );
    !matches!((a,p), (Some(a),Some(p)) if a == p
        && !cx.index.member_lists[a].type_params.is_empty()
        && !cx.index.member_lists[a].method_params.contains_key("@variant")
        && !has_conversion(cx.index, a))
}

fn has_conversion(index: &DefIndex, idx: usize) -> bool {
    index.member_lists[idx]
        .method_params
        .contains_key("@implicit-conversion")
}

fn resolve_type(
    index: &DefIndex,
    contexts: &HashMap<String, FileContext>,
    file: &str,
    ns: &str,
    outer: &[String],
    name: &str,
) -> Option<usize> {
    let ctx = contexts.get(file)?;
    let probe = type_probe(name, ns, outer);
    match resolve_ref(&probe, &ctx.usings, ns, index, &ctx.aliases, contexts) {
        Resolution::Resolved(found, _) => Some(found),
        _ => None,
    }
}

fn descriptor_head(descriptor: &str) -> &str {
    descriptor
        .split(['<', '[', '?'])
        .next()
        .unwrap_or(descriptor)
}

fn numeric(name: &str) -> bool {
    matches!(
        name,
        "sbyte"
            | "byte"
            | "short"
            | "ushort"
            | "int"
            | "uint"
            | "long"
            | "ulong"
            | "float"
            | "double"
            | "decimal"
            | "char"
    )
}

fn fixed_bcl(name: &str) -> Option<&str> {
    match name {
        "string" => Some("string"),
        "bool" => Some("bool"),
        "char" => Some("char"),
        "Type" => Some("Type"),
        "DateTime" => Some("DateTime"),
        "TimeSpan" => Some("TimeSpan"),
        "Uri" => Some("Uri"),
        "CancellationToken" => Some("CancellationToken"),
        "int" => Some("int"),
        "long" => Some("long"),
        "double" => Some("double"),
        "decimal" => Some("decimal"),
        _ => None,
    }
}

fn canonical_primitives(descriptor: &str) -> String {
    fn primitive(token: &str) -> &str {
        let bare = token.strip_prefix("System.").unwrap_or(token);
        match bare {
            "SByte" => "sbyte",
            "Byte" => "byte",
            "Int16" => "short",
            "UInt16" => "ushort",
            "Int32" => "int",
            "UInt32" => "uint",
            "Int64" => "long",
            "UInt64" => "ulong",
            "Single" => "float",
            "Double" => "double",
            "Decimal" => "decimal",
            "Char" => "char",
            "Boolean" => "bool",
            "String" => "string",
            "Object" => "object",
            _ => token,
        }
    }

    let mut result = String::with_capacity(descriptor.len());
    let mut rest = descriptor;
    while !rest.is_empty() {
        let token_len = rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.'))
            .count();
        if token_len > 0 {
            result.push_str(primitive(&rest[..token_len]));
            rest = &rest[token_len..];
        } else {
            let ch = rest.chars().next().expect("nonempty descriptor");
            result.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    result
}
