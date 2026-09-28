// Name tables for the Rust module tree: what each module and each recorded
// block binds, by namespace, and the `use` leaves resolved into them.
//
// The type namespace holds modules, structs, enums, unions, traits and type
// aliases; the value namespace holds functions, consts, statics and the
// tuple and unit structs listed as constructors. A module's own items and
// its explicit imports bind at one level, its globs at the next: a glob
// brings in every name the target declares or imports, its own globs
// included. An explicit import nothing resolves (an external path) still
// shadows a glob of that name. Visibility is not modelled.
//
// A glob whose names cannot be listed (of std, an external crate or an
// enum, a macro-extended crate module, or an external import) is opaque:
// any name may come from it. Outside an import, a lookup that finds the
// name nowhere at a level with an opaque glob stops there instead of going
// on to an enclosing block, the module or the extern prelude. An import
// path goes on, because rustc rejects an import whose first segment a glob
// could shadow from an outer scope.
//
// Module-level leaves depend on each other (a `pub use` chain, a glob of a
// module that itself imports), so they are resolved by a worklist to a fixed
// point: each resolution records the modules it consulted, and a leaf whose
// result changes requeues every leaf that consulted its module. Nothing
// recurses, so a cycle resolves to nothing and a long chain costs no stack.

use std::collections::{HashMap, HashSet, VecDeque};

use super::modtree::{split_last, ModTree};
use crate::extract::RsFragment;

/// A name's binding: a module node or a recorded item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) enum Target {
    Module(usize),
    Def(usize),
}

/// The two namespaces a path segment is looked up in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ns {
    Type,
    Value,
}

/// One recorded Rust item as the resolver sees it.
pub(super) struct DefRef {
    pub file: usize,
    pub rel: String,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: String,
}

impl DefRef {
    /// Whether the item names a type: a struct, enum, union, trait or alias.
    pub fn is_type(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "struct" | "enum" | "union" | "trait" | "type"
        )
    }
}

/// The targets one name has in each namespace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Bound {
    pub types: Vec<Target>,
    pub values: Vec<Target>,
}

impl Bound {
    pub fn get(&self, ns: Ns) -> &[Target] {
        match ns {
            Ns::Type => &self.types,
            Ns::Value => &self.values,
        }
    }

    fn push(&mut self, ns: Ns, target: Target) {
        let list = match ns {
            Ns::Type => &mut self.types,
            Ns::Value => &mut self.values,
        };
        if !list.contains(&target) {
            list.push(target);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty() && self.values.is_empty()
    }
}

/// One flattened `use` leaf, placed in its module and block.
pub(super) struct Leaf {
    pub file: usize,
    pub module: usize,
    pub block: Option<usize>,
    pub segments: Vec<String>,
    pub name: String,
}

impl Leaf {
    pub fn is_glob(&self) -> bool {
        self.name == "*"
    }
}

/// What a level's globs give a name: the targets found, and whether a glob
/// that cannot be listed may also provide it.
pub(super) struct Globbed {
    pub found: Vec<Target>,
    pub opaque: bool,
}

/// What an explicit lookup at one level found.
pub(super) enum Level {
    Found(Vec<Target>),
    Shadowed,
    Empty,
}

/// Every table the binder reads.
pub(super) struct Tables {
    pub defs: Vec<DefRef>,
    pub def_by_path: HashMap<(usize, String), usize>,
    pub items: Vec<HashMap<String, Bound>>,
    pub leaves: Vec<Leaf>,
    pub module_named: Vec<HashMap<String, Vec<usize>>>,
    pub module_globs: Vec<Vec<usize>>,
    pub block_named: HashMap<(usize, usize), HashMap<String, Vec<usize>>>,
    pub block_globs: HashMap<(usize, usize), Vec<usize>>,
    pub block_parent: Vec<Vec<Option<usize>>>,
    pub resolved: Vec<Bound>,
    pub assoc: HashMap<usize, HashMap<String, Vec<usize>>>,
    pub impl_self: HashMap<(usize, usize), usize>,
    pub opaque_modules: Vec<bool>,
}

/// Builds the item and leaf tables over the module tree; leaves start
/// unresolved.
pub(super) fn tables(fragments: &[(String, RsFragment)], tree: &ModTree) -> Tables {
    let mut t = Tables {
        defs: Vec::new(),
        def_by_path: HashMap::new(),
        items: vec![HashMap::new(); tree.nodes.len()],
        leaves: Vec::new(),
        module_named: vec![HashMap::new(); tree.nodes.len()],
        module_globs: vec![Vec::new(); tree.nodes.len()],
        block_named: HashMap::new(),
        block_globs: HashMap::new(),
        block_parent: fragments
            .iter()
            .map(|(_, f)| f.blocks.iter().map(|b| b.parent).collect())
            .collect(),
        resolved: Vec::new(),
        assoc: HashMap::new(),
        impl_self: HashMap::new(),
        opaque_modules: vec![false; tree.nodes.len()],
    };
    for (file, (rel, fragment)) in fragments.iter().enumerate() {
        add_items(&mut t, tree, file, rel, fragment);
        add_leaves(&mut t, tree, file, fragment);
        for scope in &fragment.opaque_scopes {
            if let Some(module) = tree.node_of(file, scope) {
                t.opaque_modules[module] = true;
            }
        }
    }
    t.resolved = vec![Bound::default(); t.leaves.len()];
    t
}

fn add_items(t: &mut Tables, tree: &ModTree, file: usize, rel: &str, fragment: &RsFragment) {
    let ctors: HashSet<&str> = fragment.ctors.iter().map(String::as_str).collect();
    for item in &fragment.items {
        let index = t.defs.len();
        t.defs.push(DefRef {
            file,
            rel: rel.to_string(),
            id: super::rs_def_id(rel, &item.path),
            name: item.name.clone(),
            kind: item.kind.clone(),
            path: item.path.clone(),
        });
        t.def_by_path.insert((file, item.path.clone()), index);
        let (parent, _) = split_last(&item.path);
        let Some(module) = tree.node_of(file, parent) else {
            continue;
        };
        let entry = t.items[module].entry(item.name.clone()).or_default();
        match item.kind.as_str() {
            "mod" => {
                if let Some(&child) = tree.nodes[module].children.get(&item.name) {
                    entry.push(Ns::Type, Target::Module(child));
                }
            }
            "struct" | "enum" | "union" | "trait" | "type" => {
                entry.push(Ns::Type, Target::Def(index));
                if ctors.contains(item.path.as_str()) {
                    entry.push(Ns::Value, Target::Def(index));
                }
            }
            "fn" | "const" | "static" => entry.push(Ns::Value, Target::Def(index)),
            _ => {}
        }
    }
}

fn add_leaves(t: &mut Tables, tree: &ModTree, file: usize, fragment: &RsFragment) {
    for u in &fragment.uses {
        let Some(module) = tree.node_of(file, &u.scope) else {
            continue;
        };
        let index = t.leaves.len();
        t.leaves.push(Leaf {
            file,
            module,
            block: u.block,
            segments: u.path.split("::").map(str::to_string).collect(),
            name: u.name.clone(),
        });
        match (u.block, u.name == "*") {
            (None, true) => t.module_globs[module].push(index),
            (None, false) => t.module_named[module]
                .entry(u.name.clone())
                .or_default()
                .push(index),
            (Some(b), true) => t.block_globs.entry((file, b)).or_default().push(index),
            (Some(b), false) => t
                .block_named
                .entry((file, b))
                .or_default()
                .entry(u.name.clone())
                .or_default()
                .push(index),
        }
    }
}

/// The modules a lookup consulted, which the fixed point tracks.
pub(super) type Probe = Vec<usize>;

impl Tables {
    /// Items and explicit imports of `name` in a module.
    pub fn explicit(&self, module: usize, name: &str, ns: Ns, probe: &mut Probe) -> Level {
        probe.push(module);
        let mut found: Vec<Target> = self.items[module]
            .get(name)
            .map(|b| b.get(ns).to_vec())
            .unwrap_or_default();
        let leaves = self.module_named[module]
            .get(name)
            .map_or(&[][..], Vec::as_slice);
        level(&mut found, leaves, &self.resolved, ns)
    }

    /// The modules a set of globs import, as resolved so far, and whether
    /// any of them reaches no crate module and so cannot be listed.
    fn glob_targets(&self, globs: &[usize]) -> (Vec<usize>, bool) {
        let mut modules = Vec::new();
        let mut opaque = false;
        for &g in globs {
            let before = modules.len();
            modules.extend(self.resolved[g].types.iter().filter_map(|t| match t {
                Target::Module(m) => Some(*m),
                Target::Def(_) => None,
            }));
            opaque |= modules.len() == before;
        }
        (modules, opaque)
    }

    /// What `name` is through a set of globs: breadth first over glob
    /// targets, stopping at each module where the name binds explicitly. A
    /// module whose name is unlisted because of a macro or an external
    /// import makes the result opaque, as does any unlistable glob.
    pub fn through_globs(&self, globs: &[usize], name: &str, ns: Ns, probe: &mut Probe) -> Globbed {
        let mut out: Vec<Target> = Vec::new();
        let mut seen: HashSet<usize> = HashSet::new();
        let (first, mut opaque) = self.glob_targets(globs);
        let mut queue: VecDeque<usize> = first.into();
        while let Some(m) = queue.pop_front() {
            if !seen.insert(m) {
                continue;
            }
            match self.explicit(m, name, ns, probe) {
                Level::Found(v) => extend_unique(&mut out, v),
                Level::Shadowed => opaque = true,
                Level::Empty => {
                    opaque |= self.opaque_modules[m];
                    let (more, unlisted) = self.glob_targets(&self.module_globs[m]);
                    opaque |= unlisted;
                    queue.extend(more);
                }
            }
        }
        Globbed { found: out, opaque }
    }

    /// `name` looked up inside a module: its own level, then its globs.
    /// `None` when nothing binds or shadows the name there; outside an
    /// import, a glob that cannot be listed shadows it.
    pub fn in_module(
        &self,
        module: usize,
        name: &str,
        ns: Ns,
        import: bool,
        probe: &mut Probe,
    ) -> Option<Vec<Target>> {
        match self.explicit(module, name, ns, probe) {
            Level::Found(v) => Some(v),
            Level::Shadowed => Some(Vec::new()),
            Level::Empty => {
                let g = self.through_globs(&self.module_globs[module], name, ns, probe);
                globbed(g, import)
            }
        }
    }

    /// `name` in a recorded block and its enclosing blocks, innermost
    /// first: each block's explicit imports, then its globs. `None` when no
    /// block binds or shadows the name; outside an import, a block glob
    /// that cannot be listed shadows it.
    pub fn in_blocks(
        &self,
        file: usize,
        block: Option<usize>,
        name: &str,
        ns: Ns,
        import: bool,
        probe: &mut Probe,
    ) -> Option<Vec<Target>> {
        let mut current = block;
        let mut guard = 0;
        while let Some(b) = current {
            let leaves = self
                .block_named
                .get(&(file, b))
                .and_then(|m| m.get(name))
                .map_or(&[][..], Vec::as_slice);
            let mut found = Vec::new();
            match level(&mut found, leaves, &self.resolved, ns) {
                Level::Found(v) => return Some(v),
                Level::Shadowed => return Some(Vec::new()),
                Level::Empty => {}
            }
            if let Some(globs) = self.block_globs.get(&(file, b)) {
                let g = self.through_globs(globs, name, ns, probe);
                if let Some(v) = globbed(g, import) {
                    return Some(v);
                }
            }
            current = self.block_parent[file].get(b).copied().flatten();
            guard += 1;
            if guard > self.block_parent[file].len() {
                break;
            }
        }
        None
    }
}

// A level's glob result: its targets, an empty (shadowing) result for an
// opaque level outside an import, or `None` to go on outward.
fn globbed(g: Globbed, import: bool) -> Option<Vec<Target>> {
    if g.opaque && !import {
        Some(Vec::new())
    } else if !g.found.is_empty() {
        Some(g.found)
    } else {
        None
    }
}

fn level(found: &mut Vec<Target>, leaves: &[usize], resolved: &[Bound], ns: Ns) -> Level {
    let mut shadow = false;
    for &leaf in leaves {
        let r = &resolved[leaf];
        if r.is_empty() {
            shadow = true;
        }
        extend_unique(found, r.get(ns).to_vec());
    }
    if !found.is_empty() {
        Level::Found(std::mem::take(found))
    } else if shadow {
        Level::Shadowed
    } else {
        Level::Empty
    }
}

pub(super) fn extend_unique(out: &mut Vec<Target>, more: Vec<Target>) {
    for t in more {
        if !out.contains(&t) {
            out.push(t);
        }
    }
}
