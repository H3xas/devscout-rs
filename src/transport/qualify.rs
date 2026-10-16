//! The repository-wide context the assembly stage reads: which namespaces
//! each file imports, which types and methods the repository declares, and
//! how a written type name qualifies against both.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::catalogue;
use super::inputs::TransportInputs;
use super::sites::written_parts;
use crate::graph::{Def, FragUsing, Fragment, Graph};
use crate::project::ProjectModel;

/// What one written type name qualifies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Qual {
    Catalogue(&'static str),
    Repo(usize),
    Nothing,
    Ambiguous,
}

const PREDEFINED: [&str; 16] = [
    "bool", "byte", "char", "decimal", "double", "float", "int", "long", "object", "sbyte",
    "short", "string", "uint", "ulong", "ushort", "dynamic",
];

pub(crate) struct Ctx<'a> {
    pub(crate) root: &'a Path,
    pub(crate) graph: &'a Graph,
    usings: HashMap<&'a str, Vec<&'a str>>,
    unit_of_file: HashMap<&'a str, Option<usize>>,
    unit_namespaces: HashMap<usize, Vec<String>>,
    repo_wide: Vec<String>,
    def_ids: HashMap<&'a str, usize>,
    def_names: HashSet<&'a str>,
    extensions: HashMap<&'a str, Vec<&'a str>>,
    pub(crate) model: Option<&'a ProjectModel>,
    pub(crate) inputs: &'a TransportInputs,
}

impl<'a> Ctx<'a> {
    pub(crate) fn new(
        root: &'a Path,
        fragments: &'a [(String, Fragment)],
        graph: &'a Graph,
        model: Option<&'a ProjectModel>,
        inputs: &'a TransportInputs,
    ) -> Ctx<'a> {
        let mut usings: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut unit_of_file = HashMap::new();
        let mut unit_namespaces: HashMap<usize, Vec<String>> = HashMap::new();
        let mut repo_wide = Vec::new();
        for (file, fragment) in fragments {
            let unit = model.and_then(|m| m.unit_of_file(file));
            unit_of_file.insert(file.as_str(), unit);
            for using in &fragment.usings {
                let FragUsing::Plain { text, global, .. } = using else {
                    continue;
                };
                usings.entry(file.as_str()).or_default().push(text.as_str());
                if *global {
                    repo_wide.push(text.clone());
                    if let Some(u) = unit {
                        unit_namespaces.entry(u).or_default().push(text.clone());
                    }
                }
            }
        }
        if let Some(model) = model {
            for (index, unit) in model.units.iter().enumerate() {
                let declared = inputs.units.iter().find(|u| u.unit == unit.id);
                let extra = declared.map(|u| u.namespaces.clone()).unwrap_or_default();
                unit_namespaces.entry(index).or_default().extend(extra);
            }
        }
        let def_ids = graph
            .defs
            .iter()
            .enumerate()
            .map(|(i, d)| (d.id.as_str(), i))
            .collect();
        let def_names = graph.defs.iter().map(|d| d.name.as_str()).collect();
        let mut extensions: HashMap<&str, Vec<&str>> = HashMap::new();
        for (_, fragment) in fragments {
            for def in &fragment.defs {
                for method in &def.extension_methods {
                    extensions
                        .entry(method.name.as_str())
                        .or_default()
                        .push(def.namespace.as_str());
                }
            }
        }
        Ctx {
            root,
            graph,
            usings,
            unit_of_file,
            unit_namespaces,
            repo_wide,
            def_ids,
            def_names,
            extensions,
            model,
            inputs,
        }
    }

    pub(crate) fn def(&self, index: usize) -> &'a Def {
        &self.graph.defs[index]
    }

    pub(crate) fn def_by_id(&self, id: &str) -> Option<usize> {
        self.def_ids.get(id).copied()
    }

    /// Every namespace a site in `file`, inside namespace `ns`, sees types
    /// of without qualification.
    pub(crate) fn namespaces(&self, file: &str, ns: &str) -> Vec<String> {
        let mut all = Vec::new();
        let mut level = ns;
        while !level.is_empty() {
            all.push(level.to_string());
            level = level.rsplit_once('.').map_or("", |(outer, _)| outer);
        }
        if let Some(own) = self.usings.get(file) {
            all.extend(own.iter().map(|u| (*u).to_string()));
        }
        match self.unit_of_file.get(file).copied().flatten() {
            Some(unit) => all.extend(self.unit_namespaces.get(&unit).cloned().unwrap_or_default()),
            None => all.extend(self.repo_wide.iter().cloned()),
        }
        all
    }

    /// Whether `needed` is imported in `file` at namespace `ns`.
    pub(crate) fn imports(&self, file: &str, ns: &str, needed: &str) -> bool {
        self.namespaces(file, ns).iter().any(|n| n == needed)
    }

    /// Whether the repository declares a type with this simple name.
    pub(crate) fn declares_type(&self, simple: &str) -> bool {
        self.def_names.contains(simple)
    }

    /// Whether a repository extension method with this name is in scope
    /// for a call in `file` at namespace `ns`: declared in a namespace the
    /// file imports or encloses, where it would compete with the
    /// catalogue's own.
    pub(crate) fn shadows(&self, name: &str, file: &str, ns: &str) -> bool {
        let Some(declared) = self.extensions.get(name) else {
            return false;
        };
        let visible = self.namespaces(file, ns);
        declared
            .iter()
            .any(|d| d.is_empty() || visible.iter().any(|v| v == d))
    }

    /// Qualifies a written type name in `file`, namespace `ns`, inside the
    /// types `outer`, against the catalogue and the repository at once.
    pub(crate) fn qualify(&self, written: &str, file: &str, ns: &str, outer: &[String]) -> Qual {
        let (base, arity) = written_parts(written);
        let catalogue_name = |full: &str| {
            let name = if arity == 0 {
                full.to_string()
            } else {
                format!("{full}`{arity}")
            };
            catalogue::type_entry(&name).map(|t| t.name)
        };
        let mut found: Vec<Qual> = Vec::new();
        let mut add = |q: Qual| {
            if !found.contains(&q) {
                found.push(q);
            }
        };
        let mut try_full = |full: &str| {
            if let Some(name) = catalogue_name(full) {
                add(Qual::Catalogue(name));
            }
            if let Some(index) = self.def_by_id(full) {
                add(Qual::Repo(index));
            }
        };
        if base.contains('.') {
            try_full(&base);
        } else {
            for depth in (1..=outer.len()).rev() {
                try_full(&nested_id(ns, &outer[..depth], &base));
            }
            for namespace in self.namespaces(file, ns) {
                try_full(&format!("{namespace}.{base}"));
            }
            try_full(&base);
        }
        match found.as_slice() {
            [] => Qual::Nothing,
            [one] => *one,
            _ => Qual::Ambiguous,
        }
    }

    /// Whether a written name qualifies to exactly the catalogue type
    /// `expected`: no other catalogue type and no repository type the file
    /// can see is a candidate for it.
    pub(crate) fn names_catalogue(
        &self,
        written: &str,
        expected: &'static str,
        file: &str,
        ns: &str,
        outer: &[String],
    ) -> bool {
        self.qualify(written, file, ns, outer) == Qual::Catalogue(expected)
    }

    /// Whether a chain start's declared receiver type can carry an
    /// extension method on `on`: a catalogue type that derives from it, or
    /// a type neither the catalogue nor the repository knows.
    pub(crate) fn receiver_fits(&self, written: &str, on: &str, file: &str, ns: &str) -> bool {
        let (base, _) = written_parts(written);
        if PREDEFINED.contains(&base.as_str()) {
            return false;
        }
        match self.qualify(written, file, ns, &[]) {
            Qual::Catalogue(ty) => catalogue::derives(ty, on),
            Qual::Repo(_) | Qual::Ambiguous => false,
            Qual::Nothing => !self.declares_type(catalogue::simple(&base)),
        }
    }
}

/// The def id of a type nested in `outer` inside namespace `ns`.
pub(crate) fn nested_id(ns: &str, outer: &[String], name: &str) -> String {
    let mut types: Vec<&str> = outer.iter().map(String::as_str).collect();
    types.push(name);
    let nested = types.join("+");
    if ns.is_empty() {
        nested
    } else {
        format!("{ns}.{nested}")
    }
}
