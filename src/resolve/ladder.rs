use super::index::{name_probe, DefIndex};
use super::members::inheritance_walk_matches;
use super::receiver::{extractor_vouches_instance, nested_candidate_visible_from_site};
use super::scope::FileContext;
use crate::graph::{Candidate, FragRef};
use std::collections::{HashMap, HashSet};

const AMBIGUOUS_CAP: usize = 5;

// ---------------------------------------------------------------------------
// Admission: the project model's veto over the two HEURISTIC tiers.
// ---------------------------------------------------------------------------

/// The structural gate the two heuristic tiers consult before naming a def.
///
/// A heuristic tier guesses from a member NAME; the project model is the one
/// fact available here that can disprove such a guess without reading a single
/// line of the candidate's body -- the site's assembly could not reference the
/// candidate's assembly, so the call the guess describes could not compile,
/// whatever the name says.
///
/// Two refusals, both structural:
///   - REACHABILITY: the candidate's project is not on the transitive
///     `ProjectReference` closure of the site's project.
///   - TEST DIRECTION: the candidate's project is a test project and the
///     site's is not. Production code never calls into a test assembly, and
///     this half catches the fixture/helper classes that carry no test
///     attribute of their own and so are invisible to def-level test
///     detection.
///
/// Everything else FAILS OPEN, deliberately and in three places: no model at
/// all (a repo with no `.csproj`), a site file no project owns, and a
/// candidate file no project owns. Ownership here is path-based and knows
/// nothing about linked or globbed `Compile Include` items, so an ownership
/// answer this resolver could not compute must never delete an edge it would
/// otherwise have emitted.
///
/// Only the heuristic tiers consult it. The precise tiers resolve a type
/// first and emit on a FACT, and the ctor-DI resolver picks an implementor
/// from an interface the site demonstrably names -- neither is a guess the
/// model is entitled to overrule.
pub(super) struct Admission<'m> {
    pub(super) model: Option<&'m crate::project::ProjectModel>,
    /// `unit_of_def[i]` is the unit owning `index.defs[i]`'s declaring file,
    /// computed once per resolve rather than per candidate. Always `None`
    /// when there is no model.
    pub(super) unit_of_def: Vec<Option<usize>>,
}

impl Admission<'_> {
    pub(super) fn admits(&self, site_unit: Option<usize>, cand: usize) -> bool {
        let Some(model) = self.model else {
            return true;
        };
        let (Some(site), Some(cand)) = (site_unit, self.unit_of_def.get(cand).copied().flatten())
        else {
            return true;
        };
        model.reachable(site, cand) && !(model.units[cand].test && !model.units[site].test)
    }
}

// ---------------------------------------------------------------------------
// The ladder itself.
// ---------------------------------------------------------------------------

/// The `via` field on a resolved return -- names the ladder step that answered.
/// Only the uses-member emission tiers consume it (an exact-qualified
/// resolution is type-certain in a way a bare-name fallthrough is not); nothing
/// in graph.json carries it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Via {
    Alias,
    Nested,
    Qualified,
    Usings,
    Namespace,
    Global,
}

pub(super) enum Resolution {
    Resolved(usize, Via),
    /// Several same-named defs the ladder refused to choose between, plus the
    /// step that pooled them -- only steps 1b, 2 and 4 can produce this, so
    /// the `Via` is always `Usings` or `Global`. It rides along so
    /// `narrow_by_reachability` can hand back a `Resolved` carrying the step
    /// that actually answered instead of inventing one: the uses-member
    /// emission tier reads that step (`via == Via::Qualified`) as one of its
    /// type-certainty signals, and a narrowed resolution must be judged by the
    /// same rule as any other.
    Ambiguous(Vec<usize>, Via),
    External,
}

/// The project model's answer to an ambiguity the ladder could not settle:
/// C# cannot name a type in an assembly this one does not reference, so such
/// a candidate was never really a candidate. Applied at the ladder's THREE
/// `Ambiguous` consumers rather than inside `resolve_ref`, because the ladder
/// is a pure name-resolution function that knows nothing about projects and
/// because two of its callers -- the base-closure probes and the ctor-DI
/// resolver -- must keep seeing the unnarrowed answer.
///
/// Every other resolution passes through untouched, and so does every
/// candidate when there is no model (`Admission::admits` then says yes to
/// everything), which is what keeps a csproj-less repo's graph byte-identical.
///
/// One survivor is a FACT, not a guess: the ambiguity was only ever the
/// ladder's refusal to choose, and the reference rule chose for it. Zero
/// survivors is an ordinary `External` -- the same answer the ladder gives for
/// a name it never found, which is exactly what a name whose every candidate
/// is out of reach IS. Two or more stay ambiguous on the FILTERED list, so the
/// reported candidates and `candidate_count` shrink together.
pub(super) fn narrow_by_reachability(
    res: Resolution,
    site_unit: Option<usize>,
    admission: &Admission,
) -> Resolution {
    let Resolution::Ambiguous(candidates, via) = res else {
        return res;
    };
    let reachable: Vec<usize> = candidates
        .into_iter()
        .filter(|&c| admission.admits(site_unit, c))
        .collect();
    match reachable.as_slice() {
        [] => Resolution::External,
        [idx] => Resolution::Resolved(*idx, via),
        _ => Resolution::Ambiguous(reachable, via),
    }
}

/// A narrowed resolution plus the one bit narrowing would otherwise destroy:
/// whether an `External` means "the ladder never found this name" or "the
/// ladder found candidates and the project model put every one of them out of
/// reach".
///
/// The two are the same answer for a precise tier -- neither can produce an
/// edge -- but they are opposite answers for the scored tier. A name the
/// ladder never found may still be a member-name-uniqueness guess. A name
/// whose every candidate was narrowed away has already been ANSWERED: the
/// candidates were real, and the language rule says none of them is nameable
/// here. Falling through to the graph-wide uniqueness pool there would answer
/// a settled question with a stranger, so `narrowed_away` gets an empty pool
/// and emits nothing.
pub(super) struct Narrowed {
    pub(super) res: Resolution,
    pub(super) narrowed_away: bool,
}

/// `narrow_by_reachability`, keeping the pre-narrowing shape as the flag
/// `Narrowed` documents. Used at the two `uses-member` consumers, whose
/// resolutions reach the scored tier; the plain type-reference consumer has no
/// heuristic tier behind it and calls `narrow_by_reachability` directly.
pub(super) fn narrow_tracked(
    res: Resolution,
    site_unit: Option<usize>,
    admission: &Admission,
) -> Narrowed {
    let was_ambiguous = matches!(res, Resolution::Ambiguous(..));
    let res = narrow_by_reachability(res, site_unit, admission);
    Narrowed {
        narrowed_away: was_ambiguous && matches!(res, Resolution::External),
        res,
    }
}

// The dotted text of a qualified reference as a def path would spell it: an
// alias qualifier (`global::`, an extern alias) dropped, and every type
// argument list removed from every segment -- the extractor strips them off
// the tail only, so `Box<string>.Slot` arrives as written. Borrowed when
// there is nothing to strip, which is the common case.
fn written_type_path(qualified: &str) -> std::borrow::Cow<'_, str> {
    let body = match qualified.find("::") {
        Some(at) => &qualified[at + 2..],
        None => qualified,
    };
    if !body.contains('<') {
        return std::borrow::Cow::Borrowed(body);
    }
    let mut out = String::with_capacity(body.len());
    let mut depth = 0usize;
    for c in body.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    std::borrow::Cow::Owned(out)
}

// Whether a def id, read with `+` as `.`, ends with `written` at a segment
// boundary -- `App.Widgets.Outer+Inner` ends with `Outer.Inner` and with
// `App.Widgets.Outer.Inner`, never with `Widgets.Outer` or `ter.Inner`.
// Byte-wise so no candidate costs an allocation: a `+`/`.` separator is
// ASCII, and no continuation byte of a multi-byte character can equal one.
fn def_path_ends_with(id: &str, written: &str) -> bool {
    let (id, written) = (id.as_bytes(), written.as_bytes());
    if id.len() < written.len() {
        return false;
    }
    let (head, tail) = id.split_at(id.len() - written.len());
    let boundary = head.last().is_none_or(|&b| b == b'.' || b == b'+');
    boundary
        && tail
            .iter()
            .zip(written)
            .all(|(&a, &b)| a == b || (a == b'+' && b == b'.'))
}

pub(super) fn type_candidate(index: &DefIndex, name: &str, arity: Option<usize>) -> Option<usize> {
    match arity {
        Some(n) => index
            .qualified_name_and_arity_to_def
            .get(&(name.to_string(), n))
            .copied(),
        None => index.qualified_name_to_def.get(name).copied(),
    }
}

// Resolve one ref (a type reference OR a uses-member qualifier -- same shape,
// same ladder) against the current file's using/alias context. `ns` is
// `ref.namespace` with `None` folded to `""`: an EMPTY namespace is treated the
// same as absent for both the step-1 prefix walk and the step-3 same-namespace
// check, so folding `None` to `""` up front avoids re-deriving that check at
// every call site.
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "one ordered ladder of resolution steps tried in a fixed order; splitting it would separate steps whose fallthrough order is the whole point"
)]
pub(super) fn resolve_ref(
    ref_: &FragRef,
    usings: &HashSet<String>,
    ns: &str,
    index: &DefIndex,
    aliases: &HashMap<String, String>,
    file_contexts: &HashMap<String, FileContext>,
) -> Resolution {
    // Every enclosing-namespace prefix of the reference site, innermost first
    // and ending with the empty prefix (the name as literally written).
    // Shared by steps 1, 2 and 3, all three of which walk it.
    let segments: Vec<&str> = if ns.is_empty() {
        Vec::new()
    } else {
        ns.split('.').collect()
    };
    let prefixes: Vec<String> = (0..=segments.len())
        .rev()
        .map(|i| segments[..i].join("."))
        .collect();

    // Step 0: alias short-circuit, bare names only.
    if ref_.qualified.is_none() {
        if let Some(alias_target) = aliases.get(&ref_.name) {
            return match type_candidate(index, alias_target, ref_.type_arg_count) {
                Some(idx) => Resolution::Resolved(idx, Via::Alias),
                None => Resolution::External,
            };
        }
        // Step 0b: the enclosing TYPE chain, longest prefix first (innermost
        // out). A nested def id is its chain joined with "+" onto the ref's own
        // namespace, so this is one exact id lookup per level and can never
        // produce two candidates. A ref with no stack -- every namespace-level
        // ref, every fragment cached without a type stack -- skips it.
        for i in (1..=ref_.outer_types.len()).rev() {
            let stack = ref_.outer_types[..i].join("+");
            let candidate = if ns.is_empty() {
                format!("{stack}+{}", ref_.name)
            } else {
                format!("{ns}.{stack}+{}", ref_.name)
            };
            if let Some(idx) = type_candidate(index, &candidate, ref_.type_arg_count) {
                return Resolution::Resolved(idx, Via::Nested);
            }
        }
    }

    // Step 1: exact qualified name, walking enclosing namespaces innermost
    // first, only for dotted references.
    if let Some(qualified) = &ref_.qualified {
        for prefix in &prefixes {
            let candidate = if prefix.is_empty() {
                qualified.clone()
            } else {
                format!("{prefix}.{qualified}")
            };
            if let Some(idx) = type_candidate(index, &candidate, ref_.type_arg_count) {
                return Resolution::Resolved(idx, Via::Qualified);
            }
        }
        // Step 1a: the qualifier's head segment is a using alias. The alias
        // target is already fully qualified, so the rewritten name gets one
        // exact lookup and no prefix walk -- `using Ns = Some.Namespace;` makes
        // `Ns.MyEnum` read as `Some.Namespace.MyEnum`. A rewritten name that
        // finds nothing continues into step 1b under its expanded text.
        let written = written_type_path(qualified);
        let expanded: Option<String> = written
            .split_once('.')
            .and_then(|(head, rest)| aliases.get(head).map(|target| format!("{target}.{rest}")));
        if let Some(expanded) = &expanded {
            if let Some(idx) = type_candidate(index, expanded, ref_.type_arg_count) {
                return Resolution::Resolved(idx, Via::Qualified);
            }
        }
        // Step 1b: dotted suffix match, the ONLY fallback a dotted reference
        // gets. A qualified name is always written relative to some enclosing
        // scope, so its text is a dot-joined suffix of the full path of
        // whatever it names -- `Outer.Inner` is `App.Widgets.Outer+Inner`
        // read with `+` as `.`. A def whose path does not end that way cannot
        // be what the reference means, however unique its bare last segment
        // is in the graph: `RabbitMQ.Client.ExchangeType`, `System.Text.Json.
        // JsonSerializer` and `expr.Member` name something outside the graph,
        // and finishing them External here is what keeps steps 2-4 -- all
        // three keyed on the bare `ref_.name` -- from binding them to an
        // unrelated same-named def. Arity is filtered exactly as step 4 does.
        //
        // The one path that legitimately does NOT end with the written text
        // is a nested type named through a DERIVED type: `Derived.Item` for
        // an `Item` declared inside `Base`. That is the dotted twin of the
        // bare rule step 4 applies (`nested_candidate_visible_from_site`),
        // with the qualifier standing in for the site's enclosing type: the
        // qualifier is resolved as a type of its own, and a nested candidate
        // is admitted when its enclosing def lies in that type's inheritance
        // closure. The extra walk runs only when the suffix found nothing and
        // a nested candidate exists at all, so an external name whose bare
        // tail is not a nested def in the graph pays one hash lookup.
        let written = expanded.as_deref().unwrap_or(&written);
        let pool: Vec<usize> = index
            .simple_name_to_defs
            .get(&ref_.name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|idx| {
                ref_.type_arg_count
                    .is_none_or(|n| index.member_lists[*idx].type_params.len() == n)
            })
            .collect();
        let mut matches: Vec<usize> = pool
            .iter()
            .copied()
            .filter(|idx| def_path_ends_with(&index.defs[*idx].id, written))
            .collect();
        if matches.is_empty() && pool.iter().any(|idx| index.defs[*idx].id.contains('+')) {
            if let Some((qualifier, _)) = written.rsplit_once('.') {
                let (head, tail) = match qualifier.rsplit_once('.') {
                    Some((_, tail)) => (Some(qualifier.to_string()), tail),
                    None => (None, qualifier),
                };
                let probe = FragRef {
                    name: tail.to_string(),
                    qualified: head,
                    ..name_probe(String::new(), ns, ref_.outer_types.clone())
                };
                if let Resolution::Resolved(qidx, _) =
                    resolve_ref(&probe, usings, ns, index, aliases, file_contexts)
                {
                    matches = pool
                        .iter()
                        .copied()
                        .filter(|idx| {
                            index.defs[*idx]
                                .id
                                .rsplit_once('+')
                                .and_then(|(enclosing, _)| {
                                    index.qualified_name_to_def.get(enclosing)
                                })
                                .is_some_and(|&enclosing| {
                                    inheritance_walk_matches(index, file_contexts, qidx, |i| {
                                        i == enclosing
                                    })
                                })
                        })
                        .collect();
                }
            }
        }
        match matches.as_slice() {
            [idx] => return Resolution::Resolved(*idx, Via::Global),
            [_, _, ..] => return Resolution::Ambiguous(matches, Via::Global),
            // No suffix match: fall through to step 1.5, the one remaining
            // step a dotted reference may take. Steps 2-4 stay closed to it
            // -- the guard just past step 1.5 finishes any dotted reference
            // that got this far as External.
            _ => {}
        }
    }

    // Step 1.5: a dotted qualifier crossing a TYPE boundary -- a nested
    // static class or enum reached through a namespace- or using-qualified
    // head, e.g. `App.Other.Outer.Middle.Leaf.Value`. Nested ids join with
    // '+', so step 1's plain '.' walk can never answer past the outermost
    // type. Walks the qualifier's segments left to right, SHORTEST head
    // first: a longer head is itself a name the later steps (or step 1's own
    // k>1 walk here) could answer via a tail-name fallback before the true
    // nested type is ever considered. Once a head resolves to a type, its
    // tail is walked one exact `{id}+{segment}` lookup per level; a head
    // whose tail walk does not consume every remaining segment is dropped in
    // favour of the next, longer head. A walk that consumes the whole tail is
    // as certain as an exact qualified match, hence `Via::Qualified`.
    //
    // Two gates keep the walk off qualifiers it cannot apply to. A ref the
    // extractor already typed as an INSTANCE (a local, field, property or
    // call receiver) is never a type path, however much its name looks like
    // one -- `Settings.Retry.Max` through a `JobSettings Settings` field must
    // keep its receiver-typed edge, not bind a same-named type's nested
    // `Retry`. And a chain whose last qualifier segment names no def at all
    // (every BCL chain) can never complete, so it skips the recursion.
    if let Some(qualified) = ref_
        .qualified
        .as_ref()
        .filter(|_| !extractor_vouches_instance(ref_))
    {
        let segs: Vec<&str> = qualified.split('.').collect();
        let leaf_known = segs
            .last()
            .is_some_and(|leaf| index.simple_name_to_defs.contains_key(*leaf));
        for k in 1..segs.len() {
            if !leaf_known {
                break;
            }
            let head_idx = if k == 1 {
                let mut head = ref_.clone();
                head.name = segs[0].to_string();
                head.qualified = None;
                head.type_arg_count = None;
                match resolve_ref(&head, usings, ns, index, aliases, file_contexts) {
                    Resolution::Resolved(idx, _) => Some(idx),
                    _ => None,
                }
            } else {
                let head = segs[..k].join(".");
                prefixes.iter().find_map(|prefix| {
                    let candidate = if prefix.is_empty() {
                        head.clone()
                    } else {
                        format!("{prefix}.{head}")
                    };
                    type_candidate(index, &candidate, None)
                })
            };
            let Some(start) = head_idx else {
                continue;
            };
            let mut cur = start;
            let mut complete = true;
            let last = segs.len() - 1;
            for (i, seg) in segs.iter().enumerate().skip(k) {
                // Only the leaf carries the ref's own arity; intermediate
                // segments are looked up arity-less like any qualifier text.
                let arity = if i == last { ref_.type_arg_count } else { None };
                match type_candidate(index, &format!("{}+{}", index.defs[cur].id, seg), arity) {
                    Some(next) => cur = next,
                    None => {
                        complete = false;
                        break;
                    }
                }
            }
            if complete {
                return Resolution::Resolved(cur, Via::Qualified);
            }
        }
    }

    // A dotted reference is finished here. Steps 1, 1a, 1b and 1.5 are the
    // whole ladder it gets: steps 2-4 all key on the bare `ref_.name`, and
    // letting `RabbitMQ.Client.ExchangeType` reach them is exactly how an
    // out-of-graph name binds an unrelated same-named def.
    if ref_.qualified.is_some() {
        return Resolution::External;
    }

    // Step 2: file's usings (already the union of local + global by the time
    // this is called) + simple name, the using's OWN name walked over every
    // enclosing-namespace prefix -- `using Configuration;` inside
    // `namespace A.B.C` reaches `A.Configuration.T`. One directive contributes
    // at most ONE candidate: its innermost reading wins, exactly like step 1's
    // first-match-wins walk, and only then does the 1-vs-many rule run across
    // directives. Dedup by def id -- two different using texts landing on the
    // same def counts once.
    let mut using_matches: Vec<usize> = Vec::new();
    let mut seen_ids: HashSet<&str> = HashSet::new();
    for u in usings {
        for prefix in &prefixes {
            let candidate = if prefix.is_empty() {
                format!("{u}.{}", ref_.name)
            } else {
                format!("{prefix}.{u}.{}", ref_.name)
            };
            if let Some(idx) = type_candidate(index, &candidate, ref_.type_arg_count) {
                let id = index.defs[idx].id.as_str();
                if seen_ids.insert(id) {
                    using_matches.push(idx);
                }
                break;
            }
        }
    }
    if using_matches.len() == 1 {
        return Resolution::Resolved(using_matches[0], Via::Usings);
    }
    if using_matches.len() >= 2 {
        return Resolution::Ambiguous(using_matches, Via::Usings);
    }

    // Step 3: the reference site's namespace AND every ancestor of it,
    // innermost first -- the same walk step 1 runs (`T` inside `A.B.C`
    // reaches `A.B.T` and `A.T`, not only `A.B.C.T`), which is C#'s
    // ancestor-namespace rule.
    for prefix in &prefixes {
        let candidate = if prefix.is_empty() {
            ref_.name.clone()
        } else {
            format!("{prefix}.{}", ref_.name)
        };
        if let Some(idx) = type_candidate(index, &candidate, ref_.type_arg_count) {
            return Resolution::Resolved(idx, Via::Namespace);
        }
    }

    // Step 4: globally unique simple name. Enum members are excluded from
    // this pool (see build_def_index) -- a member named e.g. "Active"
    // sharing a simple name with an unrelated class must not turn that
    // class's previously-unambiguous references ambiguous. Nested definitions
    // remain in the pool, but a bare reference can see one only when its
    // enclosing type inherits from the nested definition's enclosing type.
    // Only bare references reach this step: a dotted one finished at step 1b.
    let matches: Vec<usize> = index
        .simple_name_to_defs
        .get(&ref_.name)
        .into_iter()
        .flatten()
        .copied()
        .filter(|idx| {
            ref_.type_arg_count
                .map_or(true, |n| index.member_lists[*idx].type_params.len() == n)
        })
        .filter(|idx| nested_candidate_visible_from_site(ref_, ns, *idx, index, file_contexts))
        .collect();
    match matches.as_slice() {
        [idx] => Resolution::Resolved(*idx, Via::Global),
        [_, _, ..] => Resolution::Ambiguous(matches, Via::Global),
        _ => Resolution::External,
    }
}

// ---------------------------------------------------------------------------
// Ambiguous-candidate capping.
// ---------------------------------------------------------------------------

// Sorted by id, capped at `AMBIGUOUS_CAP`, using plain Unicode-codepoint
// `str::cmp` rather than locale-aware collation. For every id this extractor can
// produce (C# namespace/type names -- letters, digits, underscore, `.`, `+`)
// codepoint order and locale order coincide in the overwhelming common case
// (PascalCase-leading identifiers, the C# naming convention this ladder's own
// fixtures and every def id observed so far follow). A pathological mix of
// leading-case or comparing `+` against a letter at the exact divergence point
// could reorder (never change the SET of) candidates within the cap -- flagged,
// not solved (no ICU collation available without a new dependency).
pub(super) fn capped_candidates(
    index: &DefIndex,
    mut candidate_indices: Vec<usize>,
) -> Vec<Candidate> {
    candidate_indices.sort_by(|&a, &b| index.defs[a].id.cmp(&index.defs[b].id));
    candidate_indices
        .into_iter()
        .take(AMBIGUOUS_CAP)
        .map(|i| Candidate {
            id: index.defs[i].id.clone(),
            file: index.defs[i].file.clone(),
        })
        .collect()
}
