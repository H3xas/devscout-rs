// Whether a call's delegate-shaped arguments (lambda literals, and local
// functions passed as method groups) can convert to some overload of the
// candidate -- the gate `declares_here`'s plain argument-count check
// (`method_arity_admits`) cannot express, because it never looks past the
// COUNT of arguments into what any one of them actually is.
//
// Kept as its own module rather than folded into `members.rs` or inlined at
// the call sites in `assembly.rs`: the call sites are at their own size
// ceiling or close to it, and this gate reads facts (`lambda_arg_arity`,
// `lambda_arg_shape`) neither `declares_member` nor `method_arity_admits`
// needs for anything else they do.

use super::delegate_shape::{delegate_shape, DelegateShape};
use super::index::{DefIndex, MethodOverloadParams};
use super::scope::FileContext;
use crate::graph::{FragLambdaShape, FragRef};
use std::collections::HashMap;

/// Whether `idx`'s own overloads of `member` admit the ref's recorded
/// delegate-shaped arguments: ONE overload must pass every check at every
/// argument position that carries a fact, since C# applicability holds per
/// overload, never per position.
///
/// Fails OPEN only where there is nothing to judge, matching
/// `method_arity_admits`'s own rule for a missing arity entry: no lambda fact
/// at all, or no overload named `member`. At a position with an arity fact an
/// overload admits only through a parameter that reads as a delegate of
/// exactly that many parameters, so a parameter no list is read from -- the
/// zero-parameter `Action` and `Func<TResult>` shapes included -- admits
/// nothing there.
pub(super) fn lambda_arity_admits(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    idx: usize,
    member: Option<&str>,
    r: &FragRef,
) -> bool {
    let Some(member) = member else { return true };
    let arities = r.lambda_arg_arity.as_deref().unwrap_or_default();
    let shapes = r.lambda_arg_shape.as_deref().unwrap_or_default();
    if arities.iter().all(Option::is_none) && shapes.iter().all(Option::is_none) {
        return true;
    }
    let Some(overloads) = index.member_lists[idx].method_params.get(member) else {
        return true;
    };
    let call = LambdaCall {
        arities,
        shapes,
        type_args_written: r.method_type_arg_count.is_some(),
    };
    overloads
        .iter()
        .any(|overload| overload_admits(index, file_contexts, idx, overload, &call))
}

pub(super) fn lambda_overload_admits(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    idx: usize,
    overload: &MethodOverloadParams,
    r: &FragRef,
) -> bool {
    let call = LambdaCall {
        arities: r.lambda_arg_arity.as_deref().unwrap_or_default(),
        shapes: r.lambda_arg_shape.as_deref().unwrap_or_default(),
        type_args_written: r.method_type_arg_count.is_some(),
    };
    overload_admits(index, file_contexts, idx, overload, &call)
}

struct LambdaCall<'a> {
    arities: &'a [Option<usize>],
    shapes: &'a [Option<FragLambdaShape>],
    type_args_written: bool,
}

impl LambdaCall<'_> {
    fn positions(&self) -> usize {
        self.arities.len().max(self.shapes.len())
    }

    fn arity(&self, i: usize) -> Option<usize> {
        self.arities.get(i).copied().flatten()
    }

    fn shape(&self, i: usize) -> Option<FragLambdaShape> {
        self.shapes.get(i).copied().flatten()
    }
}

fn overload_admits(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    idx: usize,
    overload: &MethodOverloadParams,
    call: &LambdaCall,
) -> bool {
    let delegates: Vec<Option<DelegateShape>> = (0..call.positions())
        .map(|i| {
            if call.arity(i).is_none() && call.shape(i).is_none() {
                return None;
            }
            overload
                .params
                .get(i)
                .and_then(|p| delegate_shape(p, index, file_contexts, idx, &overload.file))
        })
        .collect();
    let converts = (0..call.positions()).all(|i| {
        let delegate = delegates[i].as_ref();
        let arity_ok = call
            .arity(i)
            .is_none_or(|want| delegate.is_some_and(|d| d.params.len() == want));
        // A body that can only produce a value never converts to a delegate
        // returning `void`; a return the descriptor does not settle admits.
        let shape_ok = call.shape(i).is_none_or(|shape| {
            !shape.value_only || delegate.is_none_or(|d| d.returns_void != Some(true))
        });
        arity_ok && shape_ok
    });
    converts && (call.type_args_written || !uninferable(overload, call, &delegates))
}

// C# infers a method's type arguments from the arguments, and an implicitly
// typed lambda contributes nothing through its parameter types: they are
// what inference has to supply. So a type parameter of the overload that
// occurs ONLY in parameter-type arguments of delegates filled by implicitly
// typed lambdas is never fixed, and C# refuses the overload. Every other
// occurrence -- another parameter (`this`, optional and `params` ones
// included), a delegate's return type, a delegate head this cannot read, or a
// lambda position that is explicitly typed or carries no fact -- could fix it,
// so the overload stays. Missing type-parameter facts admit.
fn uninferable(
    overload: &MethodOverloadParams,
    call: &LambdaCall,
    delegates: &[Option<DelegateShape>],
) -> bool {
    let Some(own) = &overload.type_params else {
        return false;
    };
    let only_in_lambda_inputs = |i: usize, position: i64| {
        let implicit = call.shape(i).is_some_and(|s| s.implicitly_typed);
        let input = delegates.get(i).and_then(Option::as_ref).and_then(|d| {
            let inputs = d.input_args.as_ref()?;
            usize::try_from(position)
                .ok()
                .and_then(|p| inputs.get(p).copied())
        });
        implicit && input == Some(true)
    };
    (0..own.count).any(|tp| {
        let mut sites = own.sites.iter().enumerate().flat_map(|(i, param)| {
            param
                .iter()
                .filter(move |&&(k, _)| k == tp)
                .map(move |&(_, position)| (i, position))
        });
        let first = sites.next();
        first.is_some_and(|(i, position)| only_in_lambda_inputs(i, position))
            && sites.all(|(i, position)| only_in_lambda_inputs(i, position))
    })
}
