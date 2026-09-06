use crate::graph::{FragUsing, Fragment};
use std::collections::{HashMap, HashSet};

// The half of C#'s namespace visibility a `using` directive does NOT account
// for: a type declared in an ENCLOSING namespace of the reference site is in
// scope there with no import at all -- `App.Ext.LogExt` is nameable from
// inside `namespace App.Ext.Deep`, and no file has to say so.
//
// Tier (f) needs this because its admission test is the only place in this
// module that asks "is this def visible here" WITHOUT going through the
// ladder (which has walked enclosing namespaces since step 3). Until the
// project model arrived, the repo-wide `global using` pool papered over the
// gap -- one file anywhere in the repo importing the namespace made it
// visible everywhere, its own enclosing-namespace children included. Scoping
// global usings per project removed that accident and left the real rule
// missing, so here it is, stated.
//
// The global namespace is deliberately NOT treated as enclosing: a static
// class declared with no namespace at all would otherwise become a candidate
// at every ref site in the repo at once, which is a far wider change than the
// lexical rule this implements.
pub(super) fn namespace_encloses(outer: &str, inner: &str) -> bool {
    inner
        .strip_prefix(outer)
        .is_some_and(|rest| rest.starts_with('.'))
}

// The whole scoring function, deterministic by construction and with no tie
// left to chance: same namespace as the ref site beats a namespace the file
// merely imports, which beats anything else. The global namespace is `""` on
// BOTH sides here (the extractor records an empty string, never a missing key,
// and `resolve_graph` folds a ref's absent namespace to `""` the same way), so
// it matches itself and correctly scores 3.
pub(super) fn score_candidate(
    def_namespace: &str,
    ref_namespace: &str,
    usings: &HashSet<String>,
) -> u8 {
    if def_namespace == ref_namespace {
        return 3;
    }
    if usings.contains(def_namespace) {
        return 2;
    }
    1
}

/// One file's using/alias context.
pub(super) struct FileContext {
    pub(super) usings: HashSet<String>,
    pub(super) aliases: HashMap<String, String>,
}

// Every file's own context (local ∪ every `global using` IN SCOPE for it, with
// a local alias shadowing a same-named global one), built once instead of once
// per ref. The main loop needs it for the file it is walking; the
// instance-member veto needs it for a DIFFERENT file -- the one that declares
// the base type it is resolving -- which is why it is a map rather than two
// locals.
//
// "In scope" is a project-model question. A `global using` belongs to the
// COMPILATION that declares it and does not flow across a `ProjectReference`,
// so with a model in hand each file OWNED BY A UNIT is seeded from that unit's
// globals (`by_unit`) and sees nothing another project declared -- an owned
// unit that declared none seeds from nothing at all.
//
// A file NO unit owns is the separate case: there is no compilation to read
// boundaries from, so it falls open to the repo-wide pool, exactly as a resolve
// with no model at all does. That is the documented over-approximation this
// resolver has always used, and it is the only answer that does not silently
// strip a loose file of every global using in the tree.
pub(super) fn build_file_contexts(
    fragments_by_file: &[(String, Fragment)],
    repo_wide: &GlobalUsings,
    by_unit: Option<UnitGlobals<'_>>,
) -> HashMap<String, FileContext> {
    let mut contexts = HashMap::new();
    // The seed for a file whose OWNING unit declared no `global using` at all:
    // built once here so the match below can hand back a reference with the
    // same lifetime as the real pools.
    let empty: GlobalUsings = (HashSet::new(), HashMap::new());
    for (file, frag) in fragments_by_file {
        let seed = match by_unit {
            Some((unit_of_file, by_unit)) => match unit_of_file.get(file).copied().flatten() {
                Some(u) => by_unit.get(&u).unwrap_or(&empty),
                None => repo_wide,
            },
            None => repo_wide,
        };
        let mut usings = seed.0.clone();
        let mut aliases = seed.1.clone();
        for u in &frag.usings {
            match u {
                FragUsing::Alias { alias, target, .. } => {
                    aliases.insert(alias.clone(), target.clone());
                }
                FragUsing::Plain { text, .. } => {
                    usings.insert(text.clone());
                }
            }
        }
        contexts.insert(file.clone(), FileContext { usings, aliases });
    }
    contexts
}

// ---------------------------------------------------------------------------
// Global usings/aliases.
// ---------------------------------------------------------------------------

/// The per-unit half of the `global using` picture, as `build_file_contexts`
/// takes it: which unit owns each file, and each unit's own pool.
type UnitGlobals<'a> = (
    &'a HashMap<String, Option<usize>>,
    &'a HashMap<usize, GlobalUsings>,
);

/// One pool of `global using` facts: the plain namespaces, and the aliases
/// keyed by alias name. Used both repo-wide and per project unit.
type GlobalUsings = (HashSet<String>, HashMap<String, String>);

// Every `global using` in the fragment set, collected twice over the same
// single pass: once repo-wide (what a resolve with no project model uses, and
// what a file no project owns falls back to) and once per owning unit (what a
// resolve WITH a model uses, because a global using is a per-compilation fact).
//
// A file whose `unit_of_file` entry is absent or `None` contributes to the
// repo-wide pool only: its globals are real, but there is no project to
// attribute them to, and inventing one would leak them into whichever project
// happened to be nearest.
pub(super) fn collect_global_usings_by_unit(
    fragments_by_file: &[(String, Fragment)],
    unit_of_file: &HashMap<String, Option<usize>>,
) -> (GlobalUsings, HashMap<usize, GlobalUsings>) {
    let mut repo_wide: GlobalUsings = (HashSet::new(), HashMap::new());
    let mut by_unit: HashMap<usize, GlobalUsings> = HashMap::new();
    for (file, frag) in fragments_by_file {
        let unit = unit_of_file.get(file).copied().flatten();
        for u in &frag.usings {
            match u {
                FragUsing::Alias {
                    alias,
                    target,
                    global,
                } => {
                    if *global {
                        // First global alias for a given name wins -- NOT
                        // last-wins. `entry(..).or_insert(..)` only writes on a
                        // vacant slot. The per-unit pools apply the same rule
                        // within their own scope, so a unit's own first
                        // declaration wins there even if some other unit
                        // declared that alias earlier in file order.
                        repo_wide
                            .1
                            .entry(alias.clone())
                            .or_insert_with(|| target.clone());
                        if let Some(idx) = unit {
                            by_unit
                                .entry(idx)
                                .or_default()
                                .1
                                .entry(alias.clone())
                                .or_insert_with(|| target.clone());
                        }
                    }
                }
                FragUsing::Plain { text, global } => {
                    if *global {
                        repo_wide.0.insert(text.clone());
                        if let Some(idx) = unit {
                            by_unit.entry(idx).or_default().0.insert(text.clone());
                        }
                    }
                }
            }
        }
    }
    (repo_wide, by_unit)
}
