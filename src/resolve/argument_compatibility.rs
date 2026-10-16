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
        // An interpolated string compares as `string` everywhere below except
        // at the one parameter C# hands it to whole: a marked in-graph handler
        // type. The mark rides on the merged declaring type, so a partial
        // declaration in another file carries it too.
        let interpolated = arg_type == crate::extract::INTERPOLATED_ARG_DESCRIPTOR;
        if interpolated
            && marked_handler(
                index,
                contexts,
                &overload.file,
                &index.defs[idx].namespace,
                &param_type,
            )
        {
            return true;
        }
        let arg_type = if interpolated {
            "string".to_string()
        } else {
            arg_type
        };
        can_accept(
            &CompareContext {
                index,
                contexts,
                source_file,
                param_file: &overload.file,
                reference: r,
                owner_idx: idx,
                reading: Reading::Keyword,
            },
            &arg_type,
            &param_type,
        )
    })
}

#[derive(Clone, Copy)]
struct CompareContext<'a> {
    index: &'a DefIndex,
    contexts: &'a HashMap<String, FileContext>,
    source_file: &'a str,
    param_file: &'a str,
    reference: &'a FragRef,
    owner_idx: usize,
    reading: Reading,
}

// A descriptor drops a `System.` qualifier, so a bare CLR primitive name can be
// the BCL type or an in-graph type of that name. The keyword reading takes the
// BCL type everywhere; the in-graph reading keeps a name that resolves to an
// in-graph type. A pair either reading admits stays admitted, so the in-graph
// reading only turns a decline into an admission.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reading {
    Keyword,
    InGraph,
}

#[derive(Clone, Copy)]
enum Side {
    Argument,
    Parameter,
}

impl<'a> CompareContext<'a> {
    fn scope(&self, side: Side) -> (&'a str, &'a str, &'a [String]) {
        match side {
            Side::Argument => (
                self.source_file,
                self.reference.namespace.as_deref().unwrap_or_default(),
                &self.reference.outer_types,
            ),
            Side::Parameter => (
                self.param_file,
                &self.index.defs[self.owner_idx].namespace,
                &[],
            ),
        }
    }
}

fn can_accept(cx: &CompareContext<'_>, arg: &str, param: &str) -> bool {
    if accepts(cx, &canonical_primitives(arg), &canonical_primitives(param)) {
        return true;
    }
    if !graph_declares_primitive_name(cx.index) {
        return false;
    }
    let (Some(arg), Some(param)) = (
        in_graph_primitives(cx, arg, Side::Argument),
        in_graph_primitives(cx, param, Side::Parameter),
    ) else {
        return false;
    };
    let cx = CompareContext {
        reading: Reading::InGraph,
        ..*cx
    };
    accepts(&cx, &arg, &param)
}

fn accepts(cx: &CompareContext<'_>, arg: &str, param: &str) -> bool {
    let CompareContext {
        index,
        contexts,
        source_file,
        param_file,
        reference: r,
        owner_idx: idx,
        ..
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
    let declines = matches!((a,p), (Some(a),Some(p)) if a == p
        && !cx.index.member_lists[a].type_params.is_empty()
        && !cx.index.member_lists[a].method_params.contains_key("@variant")
        && !has_conversion(cx.index, a));
    !declines || canonically_equal(cx, arg, param)
}

// A same-head decline holds only when the two argument lists name different
// types. Each side's descriptor is rewritten to one canonical identity in that
// side's own file context, so two spellings of one type -- a `using` alias,
// `Nullable<T>` against `T?`, `object` against `dynamic` -- compare equal and
// the call binds the declaring type again. The comparison can only turn a
// decline into an admission: whole descriptors with equal text never reach
// `same_head_admits`, and a descriptor this cannot canonicalize keeps the
// text comparison it got before.
fn canonically_equal(cx: &CompareContext<'_>, arg: &str, param: &str) -> bool {
    let argument = canonical(
        cx,
        arg,
        cx.source_file,
        cx.reference.namespace.as_deref().unwrap_or_default(),
        &cx.reference.outer_types,
    );
    let parameter = canonical(
        cx,
        param,
        cx.param_file,
        &cx.index.defs[cx.owner_idx].namespace,
        &[],
    );
    matches!((argument, parameter), (Some(a), Some(p)) if a.same_as(&p))
}

// One type descriptor as a canonical identity, with every type argument
// rewritten recursively inside its own argument list.
enum Canon {
    // `?` is the extractor's placeholder for a descriptor it could not read (a
    // tuple, a pointer, a function pointer) and `*` the wildcard a type
    // parameter leaves. Neither names a type, so neither can prove a mismatch.
    Placeholder,
    // A head -- an in-graph def's identity, or the normalized text of a name
    // the graph does not hold -- with its canonical arguments and array
    // suffix.
    Generic(String, Vec<Canon>, String),
}

impl Canon {
    fn same_as(&self, other: &Canon) -> bool {
        match (self, other) {
            (Canon::Placeholder, _) | (_, Canon::Placeholder) => true,
            (Canon::Generic(ah, aa, asuffix), Canon::Generic(bh, ba, bsuffix)) => {
                ah == bh
                    && asuffix == bsuffix
                    && aa.len() == ba.len()
                    && aa.iter().zip(ba).all(|(x, y)| x.same_as(y))
            }
        }
    }
}

fn canonical(
    cx: &CompareContext<'_>,
    descriptor: &str,
    file: &str,
    ns: &str,
    outer: &[String],
) -> Option<Canon> {
    let descriptor = descriptor.trim();
    if descriptor == "?" || descriptor == "*" {
        return Some(Canon::Placeholder);
    }
    let (head, arguments, suffix) = split_descriptor(descriptor)?;
    let arguments = arguments
        .iter()
        .map(|argument| canonical(cx, argument, file, ns, outer))
        .collect::<Option<Vec<Canon>>>()?;
    if let Some(idx) = resolve_type(cx.index, cx.contexts, file, ns, outer, head) {
        return Some(Canon::Generic(
            format!("@{}", cx.index.defs[idx].id),
            arguments,
            suffix.to_string(),
        ));
    }
    // An alias whose target is no in-graph type still names that target, so it
    // becomes the target's own normalized text and its names are canonicalized
    // from here.
    if let Some(target) = cx
        .contexts
        .get(file)
        .and_then(|ctx| ctx.aliases.get(head))
        .cloned()
    {
        return canonical(
            cx,
            &normalize_alias_target(&target, cx.reading),
            file,
            ns,
            outer,
        );
    }
    if bare_head(head) == "Nullable" && arguments.len() == 1 && suffix.is_empty() {
        return arguments.into_iter().next();
    }
    if bare_head(head) == "dynamic" {
        return Some(Canon::Generic(
            "object".to_string(),
            Vec::new(),
            String::new(),
        ));
    }
    Some(Canon::Generic(
        primitive_keyword(head).unwrap_or(head).to_string(),
        arguments,
        suffix.to_string(),
    ))
}

fn bare_head(head: &str) -> &str {
    head.rsplit('.').next().unwrap_or(head)
}

// `Head<A,B>[]` split into its head text, its top-level type arguments and the
// array suffix after the argument list. `None` -- no spelling claim -- for a
// head that is not a name, an unbalanced argument list, an empty argument and
// text after the list that is not an array suffix.
fn split_descriptor(descriptor: &str) -> Option<(&str, Vec<&str>, &str)> {
    let open = match descriptor.find('<') {
        Some(at) => at,
        None => descriptor.find('[').unwrap_or(descriptor.len()),
    };
    let (head, rest) = descriptor.split_at(open);
    if head.is_empty() || !head.chars().all(is_name_char) {
        return None;
    }
    if !rest.starts_with('<') {
        return is_array_suffix(rest).then_some((head, Vec::new(), rest));
    }
    let mut depth = 0usize;
    let mut arguments = Vec::new();
    let mut start = 1;
    for (at, ch) in rest.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    let last = &rest[start..at];
                    if last.is_empty() {
                        return None;
                    }
                    arguments.push(last);
                    let suffix = &rest[at + ch.len_utf8()..];
                    return is_array_suffix(suffix).then_some((head, arguments, suffix));
                }
            }
            ',' if depth == 1 => {
                let argument = &rest[start..at];
                if argument.is_empty() {
                    return None;
                }
                arguments.push(argument);
                start = at + ch.len_utf8();
            }
            _ => {}
        }
    }
    None
}

fn is_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.')
}

fn is_array_suffix(text: &str) -> bool {
    text.chars().all(|ch| matches!(ch, '[' | ']'))
}

// An alias target rendered the way `type_descriptor` renders a type: every
// dotted name reduced to its last segment, an alias qualifier (`global::Ns.T`)
// dropped with its namespace, spacing removed, CLR primitive names as
// keywords. The in-graph reading rewrites only a name the target writes
// `System.`-qualified and leaves a bare one for `canonical` to resolve.
fn normalize_alias_target(target: &str, reading: Reading) -> String {
    let mut out = String::with_capacity(target.len());
    let mut token = String::new();
    let mut chars = target.chars().peekable();
    while let Some(ch) = chars.next() {
        if is_name_char(ch) {
            token.push(ch);
            continue;
        }
        if ch == ':' {
            while chars.peek() == Some(&':') {
                chars.next();
            }
            token.clear();
            drop_last_segment(&mut out);
            continue;
        }
        if !ch.is_whitespace() {
            flush_segment(&mut out, &mut token, reading);
            out.push(ch);
        }
    }
    flush_segment(&mut out, &mut token, reading);
    match reading {
        Reading::Keyword => canonical_primitives(&out),
        Reading::InGraph => out,
    }
}

fn flush_segment(out: &mut String, token: &mut String, reading: Reading) {
    if !token.is_empty() {
        let qualified_keyword = (reading == Reading::InGraph && token.starts_with("System."))
            .then(|| primitive_keyword(token))
            .flatten();
        out.push_str(qualified_keyword.unwrap_or_else(|| bare_head(token)));
        token.clear();
    }
}

fn drop_last_segment(out: &mut String) {
    let cut = out
        .rfind(|ch: char| !is_name_char(ch))
        .map_or(0, |at| at + 1);
    out.truncate(cut);
}

fn has_conversion(index: &DefIndex, idx: usize) -> bool {
    index.member_lists[idx]
        .method_params
        .contains_key("@implicit-conversion")
}

// Whether the parameter's own type is an in-graph type the extractor marked as
// an interpolated string handler. Only a resolved in-graph type can carry the
// mark, so a fixed-BCL or external parameter type answers false and keeps the
// plain string comparison.
fn marked_handler(
    index: &DefIndex,
    contexts: &HashMap<String, FileContext>,
    param_file: &str,
    ns: &str,
    param: &str,
) -> bool {
    resolve_type(index, contexts, param_file, ns, &[], descriptor_head(param)).is_some_and(|idx| {
        index.member_lists[idx]
            .method_params
            .contains_key(crate::extract::INTERPOLATED_HANDLER_MARK)
    })
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

const CLR_PRIMITIVES: [(&str, &str); 15] = [
    ("SByte", "sbyte"),
    ("Byte", "byte"),
    ("Int16", "short"),
    ("UInt16", "ushort"),
    ("Int32", "int"),
    ("UInt32", "uint"),
    ("Int64", "long"),
    ("UInt64", "ulong"),
    ("Single", "float"),
    ("Double", "double"),
    ("Decimal", "decimal"),
    ("Char", "char"),
    ("Boolean", "bool"),
    ("String", "string"),
    ("Object", "object"),
];

fn primitive_keyword(token: &str) -> Option<&'static str> {
    let bare = token.strip_prefix("System.").unwrap_or(token);
    CLR_PRIMITIVES
        .iter()
        .find(|(name, _)| *name == bare)
        .map(|(_, keyword)| *keyword)
}

fn graph_declares_primitive_name(index: &DefIndex) -> bool {
    CLR_PRIMITIVES
        .iter()
        .any(|(name, _)| index.simple_name_to_defs.contains_key(*name))
}

fn canonical_primitives(descriptor: &str) -> String {
    rewrite_names(descriptor, |token| {
        primitive_keyword(token).unwrap_or(token)
    })
}

// The in-graph reading of one side's descriptor: a CLR primitive name stays
// where it resolves to an in-graph type from that side's scope, and becomes its
// keyword where it does not or where it is written `System.`-qualified. `None`
// -- no in-graph claim -- for type arguments that are not balanced and
// non-empty.
fn in_graph_primitives(cx: &CompareContext<'_>, descriptor: &str, side: Side) -> Option<String> {
    if !balanced_arguments(descriptor) {
        return None;
    }
    let (file, ns, outer) = cx.scope(side);
    Some(rewrite_names(descriptor, |token| {
        match primitive_keyword(token) {
            Some(keyword)
                if token.starts_with("System.")
                    || resolve_type(cx.index, cx.contexts, file, ns, outer, token).is_none() =>
            {
                keyword
            }
            _ => token,
        }
    }))
}

fn balanced_arguments(descriptor: &str) -> bool {
    let mut depth = 0usize;
    let mut empty = false;
    for ch in descriptor.chars().filter(|ch| !ch.is_whitespace()) {
        match ch {
            '<' => {
                depth += 1;
                empty = true;
            }
            ',' if depth > 0 => {
                if empty {
                    return false;
                }
                empty = true;
            }
            '>' => {
                if empty {
                    return false;
                }
                let Some(outer) = depth.checked_sub(1) else {
                    return false;
                };
                depth = outer;
            }
            _ => empty = false,
        }
    }
    depth == 0
}

fn rewrite_names<'t>(descriptor: &'t str, mut rename: impl FnMut(&'t str) -> &'t str) -> String {
    let mut result = String::with_capacity(descriptor.len());
    let mut rest = descriptor;
    while !rest.is_empty() {
        let token_len = rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.'))
            .count();
        if token_len > 0 {
            result.push_str(rename(&rest[..token_len]));
            rest = &rest[token_len..];
        } else {
            let ch = rest.chars().next().expect("nonempty descriptor");
            result.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    result
}
