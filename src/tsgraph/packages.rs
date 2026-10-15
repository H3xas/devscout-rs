use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::graph::OrderedMap;

use super::modes::RefusedConfig;
use super::{dir_of, join_rel};

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub(super) enum OrderedValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<OrderedValue>),
    Object(OrderedMap<OrderedValue>),
}

impl OrderedValue {
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub fn as_object(&self) -> Option<&OrderedMap<Self>> {
        if let Self::Object(value) = self {
            Some(value)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Package {
    pub dir: String,
    pub name: Option<String>,
    pub module_type: Option<String>,
    pub main: Option<String>,
    pub exports: Option<OrderedValue>,
    pub imports: Option<OrderedValue>,
    pub workspaces: Option<Vec<String>>,
}

#[derive(Default)]
pub(super) struct Packages {
    pub by_dir: HashMap<String, Package>,
    pub refused: Vec<RefusedConfig>,
    pub refused_dirs: HashSet<String>,
}

impl Packages {
    pub fn nearest(&self, file: &str) -> Option<&Package> {
        let mut dir = dir_of(file);
        loop {
            if let Some(package) = self.by_dir.get(dir) {
                return Some(package);
            }
            if dir.is_empty() {
                return None;
            }
            dir = dir_of(dir);
        }
    }

    pub fn nearest_refused(&self, file: &str) -> bool {
        let mut dir = dir_of(file);
        loop {
            if self.refused_dirs.contains(dir) {
                return true;
            }
            if self.by_dir.contains_key(dir) || dir.is_empty() {
                return false;
            }
            dir = dir_of(dir);
        }
    }
}

pub(super) fn read_packages(root: &Path) -> Packages {
    let mut packages = Packages::default();
    let mut dirs = vec![String::new()];
    while let Some(dir) = dirs.pop() {
        let full = root.join(&dir);
        let Ok(entries) = fs::read_dir(&full) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if kind.is_dir() {
                if !matches!(name.as_str(), ".git" | "node_modules" | "target") {
                    if let Some(child) = join_rel(&dir, &name) {
                        dirs.push(child)
                    }
                }
            } else if kind.is_file() && name == "package.json" {
                let rel = join_rel(&dir, &name).unwrap_or(name);
                let parsed = fs::read_to_string(entry.path())
                    .map_err(|_| "document")
                    .and_then(|text| parse_package(&dir, &text));
                match parsed {
                    Ok(package) => {
                        packages.by_dir.insert(dir.clone(), package);
                    }
                    Err(field) => {
                        packages.refused.push(RefusedConfig {
                            file: rel,
                            field: field.into(),
                        });
                        packages.refused_dirs.insert(dir.clone());
                    }
                }
            }
        }
    }
    packages.refused.sort_by(|a, b| a.file.cmp(&b.file));
    packages
}

fn parse_package(dir: &str, text: &str) -> Result<Package, &'static str> {
    let object: OrderedMap<OrderedValue> = serde_json::from_str(text).map_err(|_| "document")?;
    let name = field_string(&object, "name")?;
    if name.as_deref() == Some("") {
        return Err("name");
    }
    let module_type = field_string(&object, "type")?;
    if module_type
        .as_deref()
        .is_some_and(|value| !matches!(value, "module" | "commonjs"))
    {
        return Err("type");
    }
    let main = field_string(&object, "main")?;
    let exports = object.get("exports").cloned();
    if exports.as_ref().is_some_and(|value| !valid_map(value))
        || exports.as_ref().is_some_and(mixed_exports)
    {
        return Err("exports");
    }
    let imports = object.get("imports").cloned();
    if imports.as_ref().is_some_and(|value| !valid_map(value)) {
        return Err("imports");
    }
    let workspaces = match object.get("workspaces") {
        None => None,
        Some(OrderedValue::Array(items)) => Some(strings(items).ok_or("workspaces")?),
        Some(OrderedValue::Object(fields)) => match fields.get("packages") {
            Some(OrderedValue::Array(items)) => Some(strings(items).ok_or("workspaces")?),
            _ => return Err("workspaces"),
        },
        _ => return Err("workspaces"),
    };
    Ok(Package {
        dir: dir.into(),
        name,
        module_type,
        main,
        exports,
        imports,
        workspaces,
    })
}

fn field_string(
    object: &OrderedMap<OrderedValue>,
    name: &'static str,
) -> Result<Option<String>, &'static str> {
    object.get(name).map_or(Ok(None), |value| {
        value.as_str().map(|v| Some(v.to_string())).ok_or(name)
    })
}

fn strings(items: &[OrderedValue]) -> Option<Vec<String>> {
    items
        .iter()
        .map(|item| item.as_str().map(str::to_string))
        .collect()
}

fn valid_map(value: &OrderedValue) -> bool {
    match value {
        OrderedValue::Null | OrderedValue::String(_) => true,
        OrderedValue::Array(items) => items.iter().all(valid_map),
        OrderedValue::Object(object) => object.iter().all(|(_, value)| valid_map(value)),
        OrderedValue::Bool(_) | OrderedValue::Number(_) => false,
    }
}

fn mixed_exports(value: &OrderedValue) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let subpaths = object.iter().any(|(key, _)| key.starts_with('.'));
    let conditions = object.iter().any(|(key, _)| !key.starts_with('.'));
    subpaths && conditions
}

#[cfg(test)]
mod tests;
