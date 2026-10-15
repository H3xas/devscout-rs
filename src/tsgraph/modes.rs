use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde_json::Value;

use super::{dir_of, join_rel, strip_jsonc, TSCONFIG_CHAIN_LIMIT, TSCONFIG_ROOTS};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Mode {
    Node10,
    Node16,
    NodeNext,
    Bundler,
    Classic,
}

#[derive(Clone, Debug)]
pub(super) struct ModeScope {
    pub config: String,
    pub mode: Mode,
    pub custom_conditions: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RefusedConfig {
    pub file: String,
    pub field: String,
}

#[derive(Default)]
pub(super) struct ModeScopes {
    pub by_file: HashMap<String, ModeScope>,
    pub refused: Vec<RefusedConfig>,
}

impl ModeScopes {
    pub fn get(&self, file: &str) -> Option<&ModeScope> {
        self.by_file.get(file)
    }

    pub fn inert(&self) -> bool {
        self.by_file.is_empty()
    }
}

pub(super) fn read_modes<'a>(root: &Path, files: impl IntoIterator<Item = &'a str>) -> ModeScopes {
    let mut refused = Vec::new();
    let mut cache: HashMap<String, Option<ModeScope>> = HashMap::new();
    let mut by_file = HashMap::new();
    for file in files {
        let mut dir = dir_of(file);
        let mut owner = None;
        while !dir.is_empty() {
            let candidate = format!("{dir}/tsconfig.json");
            let scope = cache
                .entry(candidate.clone())
                .or_insert_with(|| read_mode_chain(root, &candidate, &mut refused));
            if scope.is_some() {
                owner = scope.clone();
                break;
            }
            dir = dir_of(dir);
        }
        if owner.is_none() {
            for &candidate in TSCONFIG_ROOTS {
                let scope = cache
                    .entry(candidate.to_string())
                    .or_insert_with(|| read_mode_chain(root, candidate, &mut refused));
                if scope.is_some() {
                    owner = scope.clone();
                    break;
                }
            }
        }
        if let Some(scope) = owner {
            by_file.insert(file.to_string(), scope);
        }
    }
    refused.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.field.cmp(&b.field)));
    refused.dedup_by(|a, b| a.file == b.file);
    ModeScopes { by_file, refused }
}

fn read_mode_chain(
    root: &Path,
    start: &str,
    refused: &mut Vec<RefusedConfig>,
) -> Option<ModeScope> {
    let mut rel = start.to_string();
    let mut seen = HashSet::new();
    let mut mode = None;
    let mut module = None;
    let mut custom_conditions = None;
    for _ in 0..TSCONFIG_CHAIN_LIMIT {
        if !seen.insert(rel.clone()) {
            break;
        }
        let path = root.join(&rel);
        if !path.is_file() {
            break;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            refuse(refused, &rel, "document");
            return None;
        };
        let Ok(value) = serde_json::from_str::<Value>(&strip_jsonc(&text)) else {
            refuse(refused, &rel, "document");
            return None;
        };
        if let Some(options) = value.get("compilerOptions").and_then(Value::as_object) {
            if let Some(field) = invalid_field(options) {
                refuse(refused, &rel, field);
                return None;
            }
            if mode.is_none() {
                mode = options
                    .get("moduleResolution")
                    .and_then(Value::as_str)
                    .and_then(parse_mode);
            }
            if module.is_none() {
                module = options
                    .get("module")
                    .and_then(Value::as_str)
                    .map(str::to_ascii_lowercase);
            }
            if custom_conditions.is_none() {
                custom_conditions = options
                    .get("customConditions")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    });
            }
        }
        let next = value
            .get("extends")
            .and_then(Value::as_str)
            .filter(|v| v.starts_with('.'));
        let Some(next) = next else { break };
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
    let mode = mode.or_else(|| implied_mode(module.as_deref()))?;
    Some(ModeScope {
        config: start.to_string(),
        mode,
        custom_conditions: custom_conditions.unwrap_or_default(),
    })
}

fn refuse(out: &mut Vec<RefusedConfig>, file: &str, field: &str) {
    out.push(RefusedConfig {
        file: file.to_string(),
        field: field.to_string(),
    });
}

fn invalid_field(options: &serde_json::Map<String, Value>) -> Option<&'static str> {
    let resolution = options
        .get("moduleResolution")
        .is_none_or(|value| value.as_str().and_then(parse_mode).is_some());
    if !resolution {
        return Some("compilerOptions.moduleResolution");
    }
    let module = options
        .get("module")
        .is_none_or(|value| value.as_str().is_some_and(valid_module));
    if !module {
        return Some("compilerOptions.module");
    }
    let conditions = options.get("customConditions").is_none_or(|value| {
        value
            .as_array()
            .is_some_and(|items| items.iter().all(Value::is_string))
    });
    if !conditions {
        return Some("compilerOptions.customConditions");
    }
    for (key, field) in [
        ("outDir", "compilerOptions.outDir"),
        ("rootDir", "compilerOptions.rootDir"),
    ] {
        if options
            .get(key)
            .is_some_and(|value| !value.as_str().is_some_and(|v| !v.is_empty()))
        {
            return Some(field);
        }
    }
    if options
        .get("composite")
        .is_some_and(|value| !value.is_boolean())
    {
        return Some("compilerOptions.composite");
    }
    None
}

fn parse_mode(value: &str) -> Option<Mode> {
    match value.to_ascii_lowercase().as_str() {
        "node" | "node10" => Some(Mode::Node10),
        "node16" => Some(Mode::Node16),
        "nodenext" => Some(Mode::NodeNext),
        "bundler" => Some(Mode::Bundler),
        "classic" => Some(Mode::Classic),
        _ => None,
    }
}

fn valid_module(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "none"
            | "commonjs"
            | "amd"
            | "system"
            | "umd"
            | "es6"
            | "es2015"
            | "es2020"
            | "es2022"
            | "esnext"
            | "node16"
            | "node18"
            | "node20"
            | "nodenext"
            | "preserve"
    )
}

fn implied_mode(module: Option<&str>) -> Option<Mode> {
    match module? {
        "node16" | "node18" | "node20" => Some(Mode::Node16),
        "nodenext" => Some(Mode::NodeNext),
        "preserve" => Some(Mode::Bundler),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
