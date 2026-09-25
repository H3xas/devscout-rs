// A delegate-shaped argument's own parameter count (a lambda literal, or a
// local function passed as a method group), checked against the
// candidate's delegate parameter shape at that position -- the gate
// `declares_here`'s plain argument-count check (`method_arity_admits`)
// cannot express, because it never looks past the COUNT of arguments into
// what any one of them actually is.
//
// Kept as its own module rather than folded into `members.rs` or inlined at
// the two call sites in `assembly.rs`: the call sites are at their own size
// ceiling or close to it, and this gate reads a fact (`lambda_arg_arity`)
// neither `declares_member` nor `method_arity_admits` needs for anything
// else they do.

use super::index::DefIndex;
use super::receiver::delegate_parameters;
use super::scope::FileContext;
use std::collections::HashMap;

/// Whether `idx`'s own overloads of `member` admit the ref's recorded
/// delegate-shaped argument facts: for every argument position at which
/// `lambda_arg_arity` names such an argument's own parameter count, at
/// least one overload's parameter at that position must be a delegate
/// whose OWN parameter list (`delegate_parameters`) is exactly that long.
///
/// Fails OPEN only where there is nothing to judge, matching
/// `method_arity_admits`'s own rule for a missing arity entry: no
/// `lambda_arg_arity` at all, or no overload named `member`. At a checked
/// position an overload admits only through a parameter that
/// `delegate_parameters` reads as a delegate of exactly that length, so a
/// parameter it reads no list from -- the zero-parameter `Action` and
/// `Func<TResult>` shapes included -- admits nothing there.
pub(super) fn lambda_arity_admits(
    index: &DefIndex,
    file_contexts: &HashMap<String, FileContext>,
    idx: usize,
    member: Option<&str>,
    lambda_arg_arity: Option<&[Option<usize>]>,
) -> bool {
    let (Some(member), Some(positions)) = (member, lambda_arg_arity) else {
        return true;
    };
    let Some(overloads) = index.member_lists[idx].method_params.get(member) else {
        return true;
    };
    positions.iter().enumerate().all(|(i, want)| {
        let Some(want) = *want else { return true };
        overloads.iter().any(|overload| {
            overload
                .params
                .get(i)
                .and_then(|p| delegate_parameters(p, index, file_contexts, idx, &overload.file))
                .is_some_and(|dp| dp.len() == want)
        })
    })
}
