// Rust path binding: a path's first segment through the scope, and each
// later segment through the module, type or trait the previous one named.
//
// Scope order, nearest first: the enclosing recorded blocks' imports, the
// module's own items and explicit imports, its globs, and last the extern
// prelude, where only the package's library name binds (std and external
// crates bind nothing). A level with a glob that cannot be listed stops a
// lookup that finds nothing there, except for an import path's first segment. `self`, repeated `super`, `crate` and a leading `::`
// name modules directly. `Type::f` finds `f` among the items of every `impl`
// whose self type resolves to `Type`, or among a trait's own items; a type
// alias, a generic parameter and a qualified path bind nothing. One
// candidate binds; two or more stop the walk as ambiguous.

use std::collections::{HashMap, HashSet, VecDeque};

use super::modtree::{split_last, ModTree};
use super::scope::{Bound, Leaf, Ns, Probe, Tables, Target};
use crate::extract::RsFragment;

/// How one segment of a path bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Step {
    /// A leading `::`, which names nothing itself.
    Skip,
    Bound(Target),
    Ambiguous(Vec<Target>),
    Nothing,
}

/// Where a path is resolved from.
pub(super) struct From {
    pub module: usize,
    pub file: usize,
    pub block: Option<usize>,
    pub self_def: Option<usize>,
    /// Whether this is a `use` path; only its first segment may pass an opaque glob.
    pub import: bool,
}

/// The module tree and its tables, bound together.
pub(super) struct Binder<'a> {
    pub tree: &'a ModTree,
    pub t: Tables,
}

impl Binder<'_> {
    /// Binds each segment of `segs` in turn, the last in `last_ns` and every
    /// qualifier in the type namespace, stopping at the first segment that
    /// binds nothing or is ambiguous.
    pub fn walk(&self, from: &From, segs: &[&str], last_ns: Ns, probe: &mut Probe) -> Vec<Step> {
        let mut steps = Vec::with_capacity(segs.len());
        let mut current: Option<Target> = None;
        let mut i = 0;
        if segs.first() == Some(&"") {
            steps.push(Step::Skip);
            let lib = segs
                .get(1)
                .and_then(|name| self.extern_named(from.module, name));
            match lib {
                Some(root) => {
                    steps.push(Step::Bound(Target::Module(root)));
                    current = Some(Target::Module(root));
                    i = 2;
                }
                None => {
                    steps.push(Step::Nothing);
                    return steps;
                }
            }
        }
        while i < segs.len() {
            let ns = if i + 1 == segs.len() {
                last_ns
            } else {
                Ns::Type
            };
            let found = self.segment(from, current, segs[i], ns, probe);
            match found.as_slice() {
                [] => {
                    steps.push(Step::Nothing);
                    break;
                }
                [one] => {
                    steps.push(Step::Bound(*one));
                    current = Some(*one);
                }
                _ => {
                    steps.push(Step::Ambiguous(found));
                    break;
                }
            }
            i += 1;
        }
        steps
    }

    fn segment(
        &self,
        from: &From,
        current: Option<Target>,
        name: &str,
        ns: Ns,
        probe: &mut Probe,
    ) -> Vec<Target> {
        match (current, name) {
            (None, "crate") => vec![Target::Module(self.tree.nodes[from.module].root)],
            (None, "self") => vec![Target::Module(from.module)],
            (None, "super") => self.parent(from.module),
            (Some(Target::Module(m)), "super") => self.parent(m),
            (None, "Self") => from.self_def.map(Target::Def).into_iter().collect(),
            (None, _) => self.in_scope(from, name, ns, probe),
            (Some(Target::Module(m)), _) => self
                .t
                .in_module(m, name, ns, false, probe)
                .unwrap_or_default(),
            (Some(Target::Def(d)), _) => self.associated(d, name, ns),
        }
    }

    fn parent(&self, module: usize) -> Vec<Target> {
        self.tree.nodes[module]
            .parent
            .map(Target::Module)
            .into_iter()
            .collect()
    }

    fn extern_named(&self, module: usize, name: &str) -> Option<usize> {
        self.tree
            .extern_of(module)
            .filter(|(lib, _)| lib == name)
            .map(|(_, root)| *root)
    }

    fn in_scope(&self, from: &From, name: &str, ns: Ns, probe: &mut Probe) -> Vec<Target> {
        let t = &self.t;
        if let Some(v) = t.in_blocks(from.file, from.block, name, ns, from.import, probe) {
            return v;
        }
        if let Some(v) = t.in_module(from.module, name, ns, from.import, probe) {
            return v;
        }
        match (ns, self.extern_named(from.module, name)) {
            (Ns::Type, Some(root)) => vec![Target::Module(root)],
            _ => Vec::new(),
        }
    }

    // An item of a type or trait: `impl` items for a struct, enum or union,
    // the trait's own items for a trait, nothing for an alias.
    fn associated(&self, def: usize, name: &str, ns: Ns) -> Vec<Target> {
        let d = &self.t.defs[def];
        if !d.is_type() || d.kind == "type" {
            return Vec::new();
        }
        let Some(items) = self.t.assoc.get(&def).and_then(|m| m.get(name)) else {
            return Vec::new();
        };
        items
            .iter()
            .filter(|&&i| match self.t.defs[i].kind.as_str() {
                "type" => ns == Ns::Type,
                _ => ns == Ns::Value,
            })
            .map(|&i| Target::Def(i))
            .collect()
    }

    // What one `use` leaf binds, in each namespace its target has.
    fn resolve_leaf(&self, leaf: &Leaf, probe: &mut Probe) -> Bound {
        let from = From {
            module: leaf.module,
            file: leaf.file,
            block: leaf.block,
            self_def: None,
            import: true,
        };
        let segs: Vec<&str> = leaf.segments.iter().map(String::as_str).collect();
        let mut out = Bound::default();
        let namespaces: &[Ns] = if leaf.is_glob() {
            &[Ns::Type]
        } else {
            &[Ns::Type, Ns::Value]
        };
        for &ns in namespaces {
            let steps = self.walk(&from, &segs, ns, probe);
            let targets = match steps.last() {
                Some(Step::Bound(t)) if steps.len() == segs.len() => vec![*t],
                Some(Step::Ambiguous(v)) if steps.len() == segs.len() => v.clone(),
                _ => Vec::new(),
            };
            for t in targets {
                if leaf.is_glob() && !matches!(t, Target::Module(_)) {
                    continue;
                }
                match ns {
                    Ns::Type => out.types.push(t),
                    Ns::Value => out.values.push(t),
                }
            }
        }
        out
    }

    /// Resolves every `use` leaf: module-level leaves by a worklist to a
    /// fixed point, then block leaves, which only blocks can see.
    pub fn resolve_leaves(&mut self) {
        let module_leaves: Vec<usize> = (0..self.t.leaves.len())
            .filter(|&l| self.t.leaves[l].block.is_none())
            .collect();
        let mut queued = vec![false; self.t.leaves.len()];
        let mut queue: VecDeque<usize> = VecDeque::new();
        for &l in &module_leaves {
            queued[l] = true;
            queue.push_back(l);
        }
        let mut dependents: HashMap<usize, HashSet<usize>> = HashMap::new();
        let mut budget = module_leaves.len().saturating_mul(64) + 64;
        while let Some(l) = queue.pop_front() {
            queued[l] = false;
            if budget == 0 {
                break;
            }
            budget -= 1;
            let mut probe = Vec::new();
            let result = self.resolve_leaf(&self.t.leaves[l], &mut probe);
            for m in probe {
                dependents.entry(m).or_default().insert(l);
            }
            if result != self.t.resolved[l] {
                self.t.resolved[l] = result;
                let module = self.t.leaves[l].module;
                let mut next: Vec<usize> = dependents
                    .get(&module)
                    .map(|d| d.iter().copied().collect())
                    .unwrap_or_default();
                next.sort_unstable();
                for d in next {
                    if !queued[d] {
                        queued[d] = true;
                        queue.push_back(d);
                    }
                }
            }
        }
        self.resolve_block_leaves();
    }

    fn resolve_block_leaves(&mut self) {
        let block_leaves: Vec<usize> = (0..self.t.leaves.len())
            .filter(|&l| self.t.leaves[l].block.is_some())
            .collect();
        for _ in 0..8 {
            let mut changed = false;
            for &l in &block_leaves {
                let result = self.resolve_leaf(&self.t.leaves[l], &mut Vec::new());
                if result != self.t.resolved[l] {
                    self.t.resolved[l] = result;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Indexes associated items: each `impl` block's items under the type
    /// its self type resolves to in the impl's own scope, and each trait's
    /// own items under the trait.
    pub fn index_associated(&mut self, fragments: &[(String, RsFragment)]) {
        for (file, (_, fragment)) in fragments.iter().enumerate() {
            for (k, imp) in fragment.impls.iter().enumerate() {
                let Some(module) = self.tree.node_of(file, &imp.scope) else {
                    continue;
                };
                let from = From {
                    module,
                    file,
                    block: None,
                    self_def: None,
                    import: false,
                };
                let segs: Vec<&str> = imp.self_type.split("::").collect();
                let steps = self.walk(&from, &segs, Ns::Type, &mut Vec::new());
                let owner = match steps.last() {
                    Some(Step::Bound(Target::Def(d))) if steps.len() == segs.len() => *d,
                    _ => continue,
                };
                if !self.t.defs[owner].is_type() || self.t.defs[owner].kind == "type" {
                    continue;
                }
                self.t.impl_self.insert((file, k), owner);
                for path in &imp.items {
                    if let Some(&item) = self.t.def_by_path.get(&(file, path.clone())) {
                        self.add_associated(owner, item);
                    }
                }
            }
        }
        for item in 0..self.t.defs.len() {
            let (parent, _) = split_last(&self.t.defs[item].path);
            if parent.is_empty() {
                continue;
            }
            let key = (self.t.defs[item].file, parent.to_string());
            if let Some(&owner) = self.t.def_by_path.get(&key) {
                if self.t.defs[owner].kind == "trait" {
                    self.add_associated(owner, item);
                }
            }
        }
    }

    fn add_associated(&mut self, owner: usize, item: usize) {
        let name = self.t.defs[item].name.clone();
        let list = self
            .t
            .assoc
            .entry(owner)
            .or_default()
            .entry(name)
            .or_default();
        if !list.contains(&item) {
            list.push(item);
        }
    }
}
