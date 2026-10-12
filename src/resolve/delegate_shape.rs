use super::index::{name_probe, DefIndex};
use super::ladder::{resolve_ref, Resolution};
use super::receiver::{def_outer_types, descriptor_args, descriptor_head};
use super::scope::FileContext;
use std::collections::HashMap;

// What a parameter descriptor says about the delegate a lambda converts to:
// its parameter list, whether it returns `void`, and which of its top-level
// type arguments are parameter types of that delegate. The last two are
// `None` where the descriptor does not settle them, so a reader can fail open
// on each fact separately.
pub(super) struct DelegateShape {
    pub(super) params: Vec<String>,
    pub(super) returns_void: Option<bool>,
    pub(super) input_args: Option<Vec<bool>>,
}

// The three BCL delegate shapes are read structurally -- `Action<A,B>` takes
// its arguments as they stand and returns nothing, `Func<A,B,R>` drops the
// return type, `Predicate<A>` takes its single argument and returns `bool` --
// because no in-graph def declares them. Any other head is a name, resolved
// under the DECLARING file's context and answered only when it lands on a
// `delegate` def, whose parameter list sits under `Invoke` and whose return
// and type-parameter occurrences sit in its signature fact.
//
// An ARRAY of delegates is not a delegate, so a descriptor ending in brackets
// answers nothing. `Expression<D>` is the expression-tree wrapper the LINQ
// shapes are written with; the delegate inside it is what the lambda converts
// to. It is unwrapped exactly once, since a doubly wrapped expression is not a
// shape C# accepts a lambda for.
pub(super) fn delegate_shape(
    text: &str,
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    declaring_def: usize,
    declaring_file: &str,
) -> Option<DelegateShape> {
    if text.ends_with(']') {
        return None;
    }
    let args = descriptor_args(text);
    if descriptor_head(text) == "Expression" && args.len() == 1 {
        return invoke_shape(
            &args[0],
            index,
            file_contexts,
            declaring_def,
            declaring_file,
        );
    }
    invoke_shape(text, index, file_contexts, declaring_def, declaring_file)
}

fn invoke_shape(
    text: &str,
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    declaring_def: usize,
    declaring_file: &str,
) -> Option<DelegateShape> {
    if text.ends_with(']') {
        return None;
    }
    let head = descriptor_head(text);
    let args = descriptor_args(text);
    let structural = |params: Vec<String>, returns_void: bool, input_args: Vec<bool>| {
        Some(DelegateShape {
            params,
            returns_void: Some(returns_void),
            input_args: Some(input_args),
        })
    };
    match head {
        "Action" if !args.is_empty() => {
            let inputs = vec![true; args.len()];
            structural(args, true, inputs)
        }
        "Func" if args.len() >= 2 => {
            let mut inputs = vec![true; args.len()];
            inputs[args.len() - 1] = false;
            let mut params = args;
            params.pop();
            structural(params, false, inputs)
        }
        "Predicate" if args.len() == 1 => structural(args, false, vec![true]),
        "Action" | "Func" | "Predicate" | "" => None,
        _ => {
            let didx =
                in_graph_delegate(head, index, file_contexts, declaring_def, declaring_file)?;
            let lists = &index.member_lists[didx];
            let params = lists
                .method_params
                .get("Invoke")
                .and_then(|overloads| overloads.first())
                .map(|o| o.params.clone())?;
            let signature = lists.delegate_signature.as_ref();
            Some(DelegateShape {
                params,
                returns_void: signature.map(|s| s.returns == "void"),
                input_args: signature
                    .filter(|s| s.type_params.len() == args.len())
                    .map(|s| s.type_params.iter().map(|u| u == "params").collect()),
            })
        }
    }
}

fn in_graph_delegate(
    head: &str,
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    declaring_def: usize,
    declaring_file: &str,
) -> Option<usize> {
    let ctx = file_contexts.get(declaring_file)?;
    let def = &index.defs[declaring_def];
    let probe = name_probe(
        head.to_string(),
        def.namespace.as_str(),
        def_outer_types(def),
    );
    let Resolution::Resolved(didx, _) = resolve_ref(
        &probe,
        &ctx.usings,
        def.namespace.as_str(),
        index,
        &ctx.aliases,
        file_contexts,
    ) else {
        return None;
    };
    (index.defs[didx].kind == "delegate").then_some(didx)
}
