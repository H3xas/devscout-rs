use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde_json::Value;

use super::modes::RefusedConfig;
use super::packages::Packages;
use super::{dir_of, join_rel, strip_jsonc, TSCONFIG_CHAIN_LIMIT};

pub(super) struct BuildOutputs {
    mappings: HashMap<String, Mapping>,
    pub refused: Vec<RefusedConfig>,
}

struct Mapping {
    out_dir: String,
    root_dir: Option<String>,
}

impl BuildOutputs {
    pub fn source(
        &self,
        package_dir: &str,
        target: &str,
        file_set: &HashSet<&str>,
    ) -> Option<String> {
        let mapping = self.mappings.get(package_dir)?;
        let root_dir = mapping.root_dir.as_ref()?;
        let relative = target.strip_prefix(&format!("{}/", mapping.out_dir))?;
        let stem = relative.rsplit_once('.').map_or(relative, |(stem, _)| stem);
        let suffix = relative.strip_prefix(stem).unwrap_or("");
        let extensions: &[&str] = match suffix {
            ".mjs" => &[".mts", ".mjs"],
            ".cjs" => &[".cts", ".cjs"],
            _ => &[".ts", ".tsx", ".js", ".jsx"],
        };
        for extension in extensions {
            let candidate = join_rel(root_dir, &format!("{stem}{extension}"))?;
            if file_set.contains(candidate.as_str()) {
                return Some(candidate);
            }
        }
        None
    }

    pub fn covers(&self, package_dir: &str, target: &str) -> bool {
        self.mappings
            .get(package_dir)
            .is_some_and(|mapping| target.starts_with(&format!("{}/", mapping.out_dir)))
    }
}

pub(super) fn read_build_outputs(root: &Path, packages: &Packages) -> BuildOutputs {
    let mut mappings = HashMap::new();
    let mut refused = Vec::new();
    for package in packages.by_dir.values() {
        let mut dir = package.dir.as_str();
        loop {
            let config = join_rel(dir, "tsconfig.json").unwrap_or_default();
            if root.join(&config).is_file() {
                if let Some(mapping) = read_mapping(root, &config, &mut refused) {
                    mappings.insert(package.dir.clone(), mapping);
                }
                break;
            }
            if dir.is_empty() {
                break;
            }
            dir = dir_of(dir);
        }
    }
    refused.sort_by(|a, b| a.file.cmp(&b.file));
    refused.dedup_by(|a, b| a.file == b.file);
    BuildOutputs { mappings, refused }
}

fn read_mapping(root: &Path, start: &str, refused: &mut Vec<RefusedConfig>) -> Option<Mapping> {
    let mut rel = start.to_string();
    let mut seen = HashSet::new();
    let mut out_dir = None;
    let mut root_dir = None;
    let mut composite = None;
    for _ in 0..TSCONFIG_CHAIN_LIMIT {
        if !seen.insert(rel.clone()) {
            break;
        }
        let Ok(text) = fs::read_to_string(root.join(&rel)) else {
            break;
        };
        let Ok(value) = serde_json::from_str::<Value>(&strip_jsonc(&text)) else {
            break;
        };
        if let Some(options) = value.get("compilerOptions").and_then(Value::as_object) {
            for (key, value) in [("outDir", &mut out_dir), ("rootDir", &mut root_dir)] {
                if value.is_none() {
                    if let Some(input) = options.get(key) {
                        let Some(path) = input.as_str().filter(|v| !v.is_empty()) else {
                            refused.push(RefusedConfig {
                                file: rel.clone(),
                                field: format!("compilerOptions.{key}"),
                            });
                            return None;
                        };
                        *value = join_rel(dir_of(&rel), path);
                    }
                }
            }
            if composite.is_none() {
                composite = options.get("composite").and_then(Value::as_bool);
            }
        }
        let Some(next) = value
            .get("extends")
            .and_then(Value::as_str)
            .filter(|v| v.starts_with('.'))
        else {
            break;
        };
        let spelled = if next.ends_with(".json") {
            next.to_string()
        } else {
            format!("{next}.json")
        };
        let Some(joined) = join_rel(dir_of(&rel), &spelled) else {
            break;
        };
        rel = joined;
    }
    let out_dir = out_dir?;
    let root_dir = root_dir.or_else(|| {
        composite
            .filter(|value| *value)
            .map(|_| dir_of(start).to_string())
    });
    Some(Mapping { out_dir, root_dir })
}

#[cfg(test)]
mod tests;
