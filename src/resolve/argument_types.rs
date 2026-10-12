use std::collections::HashMap;

use crate::graph::{FragArgType, FragRef};

use super::edges::type_probe;
use super::index::DefIndex;
use super::ladder::{resolve_ref, Resolution};
use super::scope::FileContext;

pub(super) fn materialize(
    fact: &FragArgType,
    source_file: &str,
    r: &FragRef,
    index: &DefIndex,
    contexts: &HashMap<String, FileContext>,
) -> Option<String> {
    let FragArgType::Member {
        owner,
        member,
        receiver_args,
        call,
    } = fact
    else {
        return match fact {
            FragArgType::Known(name) => Some(name.clone()),
            _ => None,
        };
    };
    let ctx = contexts.get(source_file)?;
    let ns = r.namespace.as_deref().unwrap_or_default();
    let probe = type_probe(owner, ns, &r.outer_types);
    let idx = match resolve_ref(&probe, &ctx.usings, ns, index, &ctx.aliases, contexts) {
        Resolution::Resolved(idx, _) => idx,
        Resolution::External => return bcl_member_type(owner, member, *call).map(str::to_string),
        Resolution::Ambiguous(_, _) => return None,
    };
    let lists = &index.member_lists[idx];
    let descriptor = if *call {
        if let Some(recorded) = special_type(lists.method_params.get(&format!("@result:{member}")))
        {
            recorded
        } else {
            if lists
                .method_params
                .get(member)
                .is_some_and(|overloads| overloads.len() != 1)
            {
                return None;
            }
            let mut result = lists.method_returns.get(member)?.clone();
            if let Some(args) = lists.method_return_args.get(member) {
                result.push('<');
                result.push_str(&args.join(","));
                result.push('>');
            }
            result
        }
    } else {
        if let Some(recorded) =
            special_type(lists.method_params.get(&format!("@property:{member}")))
                .or_else(|| special_type(lists.method_params.get(&format!("@field:{member}"))))
        {
            return substitute_receiver_type(
                &recorded,
                receiver_args.as_deref(),
                lists.type_params.len(),
            );
        }
        let fact = lists
            .property_types
            .get(member)
            .or_else(|| lists.field_types.get(member))?;
        let mut result = fact.type_name.clone();
        if let Some(args) = &fact.args {
            result.push('<');
            result.push_str(&args.join(","));
            result.push('>');
        }
        result
    };
    if *call
        && descriptor.contains('*')
        && lists.method_params.get(member).is_none_or(|overloads| {
            overloads.iter().any(|overload| {
                overload
                    .type_params
                    .as_ref()
                    .is_none_or(|own| own.count > 0)
            })
        })
    {
        // The descriptor uses the same wildcard for the receiver's and the
        // method's own type parameters. Only a known nongeneric method lets
        // the receiver binding replace that wildcard soundly.
        return None;
    }
    substitute_receiver_type(
        &descriptor,
        receiver_args.as_deref(),
        lists.type_params.len(),
    )
}

fn special_type(overloads: Option<&Vec<super::index::MethodOverloadParams>>) -> Option<String> {
    let overloads = overloads?;
    let first = overloads.first()?.params.first()?;
    (overloads
        .iter()
        .all(|entry| entry.params.as_slice() == [first.as_str()]))
    .then(|| first.clone())
}

pub(super) fn substitute_receiver_type(
    descriptor: &str,
    receiver_args: Option<&[String]>,
    type_params: usize,
) -> Option<String> {
    if !descriptor.contains('*') {
        return Some(descriptor.to_string());
    }
    if type_params != 1 {
        return None;
    }
    let replacement = receiver_args?.first()?;
    if replacement.contains('*') || replacement == "?" {
        return None;
    }
    Some(descriptor.replace('*', replacement))
}

fn bcl_member_type(owner: &str, member: &str, call: bool) -> Option<&'static str> {
    if owner.contains('.') && !owner.starts_with("System.") {
        return None;
    }
    match (owner.rsplit('.').next().unwrap_or(owner), member, call) {
        (
            "TimeSpan",
            "FromSeconds" | "FromMinutes" | "FromHours" | "FromDays" | "FromMilliseconds",
            true,
        ) => Some("TimeSpan"),
        ("CancellationToken", "None", false) => Some("CancellationToken"),
        ("Exception", "Message", false) => Some("string"),
        _ => None,
    }
}
