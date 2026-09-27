// The Rust module tree: which module each file and inline module is, and
// how `mod x;` declarations join files into crates.
//
// Crate roots come from the Cargo package model, in package-directory order
// and then by kind (`lib`, `bin`, `test`, `example`, `bench`, `build`); with
// no model, every file named `lib.rs` or `main.rs` is a root. Each root is
// walked breadth first, and the first position to reach a file wins; a later
// declaration still resolves paths into the file it names. Every file no
// root reaches then becomes a root of its own, in path order, and reaches
// further unreached files the same way.
//
// `mod x;` resolves to `x.rs` or `x/mod.rs` beside a crate root or a `mod.rs`
// file, and to `f/x.rs` or `f/x/mod.rs` from any other file `f.rs`; inside an
// inline module it resolves one directory deeper per enclosing module. A
// declaration whose file is missing, or whose two candidate files both
// exist, reaches nothing.

use std::collections::{BTreeMap, HashMap, VecDeque};

use super::crates::{RootKind, RustCrates};
use crate::extract::RsFragment;

/// One module: a file's root or an inline module inside it.
pub(super) struct ModNode {
    /// The fragment index of the file.
    pub file: usize,
    /// The enclosing module, which `super` names.
    pub parent: Option<usize>,
    /// The crate root module, which `crate` names.
    pub root: usize,
    /// Child modules by name.
    pub children: BTreeMap<String, usize>,
}

/// The module tree over every Rust fragment.
pub(super) struct ModTree {
    pub nodes: Vec<ModNode>,
    /// Module node by (fragment index, inline prefix).
    pub by_scope: HashMap<(usize, String), usize>,
    /// For each crate root node, the library another root of its package
    /// reaches by name: the name and the library's root node.
    pub extern_lib: HashMap<usize, (String, usize)>,
}

impl ModTree {
    /// The module a site or `use` leaf with this scope sits in.
    pub fn node_of(&self, file: usize, scope: &str) -> Option<usize> {
        self.by_scope.get(&(file, scope.to_string())).copied()
    }

    /// The library name reachable from `node`'s crate, and its root.
    pub fn extern_of(&self, node: usize) -> Option<&(String, usize)> {
        self.extern_lib.get(&self.nodes[node].root)
    }
}

struct Builder<'a> {
    fragments: &'a [(String, RsFragment)],
    file_index: HashMap<&'a str, usize>,
    tree: ModTree,
    file_node: Vec<Option<usize>>,
}

/// Builds the module tree over `fragments` from the package model.
pub(super) fn build(fragments: &[(String, RsFragment)], crates: Option<&RustCrates>) -> ModTree {
    let mut b = Builder {
        fragments,
        file_index: fragments
            .iter()
            .enumerate()
            .map(|(i, (f, _))| (f.as_str(), i))
            .collect(),
        tree: ModTree {
            nodes: Vec::new(),
            by_scope: HashMap::new(),
            extern_lib: HashMap::new(),
        },
        file_node: vec![None; fragments.len()],
    };
    for (file, lib) in declared_roots(fragments, crates, &b.file_index) {
        if b.file_node[file].is_some() {
            continue;
        }
        let root = b.walk_root(file);
        if let Some(lib) = lib {
            b.pending_extern(root, lib);
        }
    }
    b.resolve_externs();
    let mut orphans: Vec<usize> = (0..fragments.len()).collect();
    orphans.sort_by(|a, c| fragments[*a].0.cmp(&fragments[*c].0));
    for file in orphans {
        if b.file_node[file].is_none() {
            b.walk_root(file);
        }
    }
    b.tree
}

// A root to walk and, for a non-library root of a package with a library,
// the library's name and root file.
type DeclaredRoot = (usize, Option<(String, usize)>);

fn declared_roots(
    fragments: &[(String, RsFragment)],
    crates: Option<&RustCrates>,
    file_index: &HashMap<&str, usize>,
) -> Vec<DeclaredRoot> {
    let Some(crates) = crates else {
        let mut roots: Vec<usize> = (0..fragments.len())
            .filter(|&i| {
                let name = fragments[i].0.rsplit('/').next().unwrap_or("");
                name == "lib.rs" || name == "main.rs"
            })
            .collect();
        roots.sort_by(|a, b| fragments[*a].0.cmp(&fragments[*b].0));
        return roots.into_iter().map(|r| (r, None)).collect();
    };
    let mut out = Vec::new();
    for package in crates {
        let lib_file = package
            .roots
            .iter()
            .find(|r| r.kind == RootKind::Lib)
            .and_then(|r| file_index.get(r.file.as_str()).copied());
        for root in &package.roots {
            let Some(&file) = file_index.get(root.file.as_str()) else {
                continue;
            };
            let reaches_lib = matches!(
                root.kind,
                RootKind::Bin | RootKind::Test | RootKind::Example | RootKind::Bench
            );
            let lib = match (reaches_lib, &package.lib, lib_file) {
                (true, Some(name), Some(lib)) => Some((name.clone(), lib)),
                _ => None,
            };
            out.push((file, lib));
        }
    }
    out
}

impl Builder<'_> {
    fn new_node(
        &mut self,
        file: usize,
        prefix: String,
        parent: Option<usize>,
        root: Option<usize>,
    ) -> usize {
        let id = self.tree.nodes.len();
        self.tree.nodes.push(ModNode {
            file,
            parent,
            root: root.unwrap_or(id),
            children: BTreeMap::new(),
        });
        self.tree.by_scope.insert((file, prefix), id);
        id
    }

    // Walks one crate root breadth first and returns its node.
    fn walk_root(&mut self, file: usize) -> usize {
        let root = self.new_node(file, String::new(), None, None);
        self.file_node[file] = Some(root);
        let mut queue = VecDeque::from([root]);
        while let Some(node) = queue.pop_front() {
            self.expand_file(node, &mut queue);
        }
        root
    }

    // Adds a file node's inline modules and resolves its `mod x;`
    // declarations, queueing each newly reached file.
    fn expand_file(&mut self, file_root: usize, queue: &mut VecDeque<usize>) {
        let file = self.tree.nodes[file_root].file;
        let root = self.tree.nodes[file_root].root;
        let fragment = &self.fragments[file].1;
        let decls: std::collections::HashSet<&str> =
            fragment.mod_files.iter().map(String::as_str).collect();
        for item in fragment.items.iter().filter(|i| i.kind == "mod") {
            let (parent_prefix, _) = split_last(&item.path);
            let Some(&parent) = self.tree.by_scope.get(&(file, parent_prefix.to_string())) else {
                continue;
            };
            let child = if decls.contains(item.path.as_str()) {
                self.declared_file(file, parent_prefix, &item.name, parent, root, queue)
            } else {
                Some(self.new_node(file, item.path.clone(), Some(parent), Some(root)))
            };
            if let Some(child) = child {
                self.tree.nodes[parent]
                    .children
                    .entry(item.name.clone())
                    .or_insert(child);
            }
        }
    }

    fn declared_file(
        &mut self,
        file: usize,
        prefix: &str,
        name: &str,
        parent: usize,
        root: usize,
        queue: &mut VecDeque<usize>,
    ) -> Option<usize> {
        let target = self.module_file(file, prefix, name, root)?;
        if let Some(existing) = self.file_node[target] {
            return Some(existing);
        }
        let node = self.new_node(target, String::new(), Some(parent), Some(root));
        self.file_node[target] = Some(node);
        queue.push_back(node);
        Some(node)
    }

    // The file a `mod name;` in `file` under inline `prefix` names.
    fn module_file(&self, file: usize, prefix: &str, name: &str, root: usize) -> Option<usize> {
        let path = self.fragments[file].0.as_str();
        let (dir, base) = match path.rfind('/') {
            Some(i) => (&path[..i], &path[i + 1..]),
            None => ("", path),
        };
        let owns_dir = base == "mod.rs" || self.file_node[file] == Some(root);
        let mut parts: Vec<&str> = Vec::new();
        if !dir.is_empty() {
            parts.push(dir);
        }
        let stem = base.strip_suffix(".rs").unwrap_or(base);
        if !owns_dir {
            parts.push(stem);
        }
        parts.extend(prefix.split("::").filter(|p| !p.is_empty()));
        let folder = parts.join("/");
        let join = |tail: &str| {
            if folder.is_empty() {
                tail.to_string()
            } else {
                format!("{folder}/{tail}")
            }
        };
        let flat = self
            .file_index
            .get(join(&format!("{name}.rs")).as_str())
            .copied();
        let nested = self
            .file_index
            .get(join(&format!("{name}/mod.rs")).as_str())
            .copied();
        match (flat, nested) {
            (Some(f), None) | (None, Some(f)) => Some(f),
            _ => None,
        }
    }

    fn pending_extern(&mut self, root: usize, lib: (String, usize)) {
        self.tree.extern_lib.insert(root, lib);
    }

    // Swaps each pending library file index for the node the library's
    // root file ended up as; a library file some other root reached first
    // is still that file's node.
    fn resolve_externs(&mut self) {
        let pending: Vec<(usize, (String, usize))> = self.tree.extern_lib.drain().collect();
        for (root, (name, lib_file)) in pending {
            if let Some(node) = self.file_node[lib_file] {
                self.tree.extern_lib.insert(root, (name, node));
            }
        }
    }
}

/// `(parent path, last segment)` of an item path.
pub(super) fn split_last(path: &str) -> (&str, &str) {
    match path.rfind("::") {
        Some(i) => (&path[..i], &path[i + 2..]),
        None => ("", path),
    }
}
