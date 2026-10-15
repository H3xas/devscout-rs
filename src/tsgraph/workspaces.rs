use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use super::join_rel;
use super::modes::RefusedConfig;
use super::packages::Packages;

#[derive(Default)]
pub(super) struct Workspaces {
    pub roots: Vec<Workspace>,
    pub refused: Vec<RefusedConfig>,
}

pub(super) struct Workspace {
    pub dir: String,
    pub members: HashMap<String, Vec<String>>,
    pub refused: bool,
}

impl Workspaces {
    pub fn nearest(&self, file: &str) -> Option<&Workspace> {
        self.roots
            .iter()
            .find(|root| root.dir.is_empty() || file.starts_with(&format!("{}/", root.dir)))
    }
}

pub(super) fn read_workspaces(root: &Path, packages: &Packages) -> Workspaces {
    let (inputs, mut refused, bad_dirs) = collect_inputs(root, packages);
    let mut roots = Vec::new();
    for (dir, declarations) in inputs {
        let mut workspace = build_root(dir, declarations, packages, &mut refused);
        workspace.refused |= bad_dirs.contains(&workspace.dir);
        roots.push(workspace);
    }
    for dir in bad_dirs {
        if !roots.iter().any(|workspace| workspace.dir == dir) {
            roots.push(Workspace {
                dir,
                members: HashMap::new(),
                refused: true,
            });
        }
    }
    roots.sort_by(|a, b| {
        b.dir
            .len()
            .cmp(&a.dir.len())
            .then_with(|| a.dir.cmp(&b.dir))
    });
    refused.sort_by(|a, b| a.file.cmp(&b.file));
    Workspaces { roots, refused }
}

type Declarations = HashMap<String, Vec<(Vec<String>, String, &'static str)>>;

fn collect_inputs(
    root: &Path,
    packages: &Packages,
) -> (Declarations, Vec<RefusedConfig>, HashSet<String>) {
    let mut inputs = Declarations::new();
    let mut refused = Vec::new();
    let mut bad_dirs = HashSet::new();
    for package in packages.by_dir.values() {
        if let Some(patterns) = &package.workspaces {
            let file = join_rel(&package.dir, "package.json").unwrap_or_default();
            inputs.entry(package.dir.clone()).or_default().push((
                patterns.clone(),
                file,
                "workspaces",
            ));
        }
    }
    let mut dirs = vec![String::new()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = fs::read_dir(root.join(&dir)) else {
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
            } else if kind.is_file() && name == "pnpm-workspace.yaml" {
                let file = join_rel(&dir, &name).unwrap_or(name);
                let result = fs::read_to_string(entry.path())
                    .map_err(|_| "document")
                    .and_then(|text| parse_pnpm(&text));
                match result {
                    Ok(patterns) => inputs
                        .entry(dir.clone())
                        .or_default()
                        .push((patterns, file, "packages")),
                    Err(field) => {
                        bad_dirs.insert(dir.clone());
                        refused.push(RefusedConfig {
                            file,
                            field: field.into(),
                        });
                    }
                }
            }
        }
    }
    (inputs, refused, bad_dirs)
}

fn build_root(
    dir: String,
    declarations: Vec<(Vec<String>, String, &'static str)>,
    packages: &Packages,
    refused: &mut Vec<RefusedConfig>,
) -> Workspace {
    let mut included = HashSet::new();
    let mut excluded = HashSet::new();
    let mut bad = false;
    for (patterns, file, field) in declarations {
        if patterns.iter().any(|pattern| !valid_pattern(pattern)) {
            refused.push(RefusedConfig {
                file,
                field: field.into(),
            });
            bad = true;
            continue;
        }
        for pattern in patterns {
            let (negated, raw) = pattern
                .strip_prefix('!')
                .map_or((false, pattern.as_str()), |p| (true, p));
            let clean = raw.strip_prefix("./").unwrap_or(raw).trim_end_matches('/');
            for package in packages.by_dir.values() {
                let relative = if dir.is_empty() {
                    Some(package.dir.as_str())
                } else {
                    package.dir.strip_prefix(&format!("{dir}/"))
                };
                if let Some(relative) = relative {
                    if glob(clean, relative) {
                        if negated {
                            excluded.insert(package.dir.clone());
                        } else {
                            included.insert(package.dir.clone());
                        }
                    }
                }
            }
        }
    }
    let mut members: HashMap<String, Vec<String>> = HashMap::new();
    if !bad {
        for member in included.difference(&excluded) {
            let package = &packages.by_dir[member];
            if let Some(name) = &package.name {
                members
                    .entry(name.clone())
                    .or_default()
                    .push(member.clone());
            }
        }
        for dirs in members.values_mut() {
            dirs.sort();
        }
    }
    Workspace {
        dir,
        members,
        refused: bad,
    }
}

fn valid_pattern(pattern: &str) -> bool {
    !pattern.is_empty() && !pattern.contains(['?', '[', ']', '{', '}'])
}

fn glob(pattern: &str, path: &str) -> bool {
    let a: Vec<&str> = pattern.split('/').collect();
    let b: Vec<&str> = path.split('/').collect();
    fn matches(a: &[&str], b: &[&str]) -> bool {
        let Some((&first, rest)) = a.split_first() else {
            return b.is_empty();
        };
        if first == "**" {
            return matches(rest, b) || (!b.is_empty() && matches(a, &b[1..]));
        }
        !b.is_empty() && segment(first, b[0]) && matches(rest, &b[1..])
    }
    matches(&a, &b)
}

fn segment(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let pieces: Vec<&str> = pattern.split('*').collect();
    if pieces.len() == 1 {
        return pattern == value;
    }
    if !value.starts_with(pieces[0]) || !value.ends_with(pieces[pieces.len() - 1]) {
        return false;
    }
    let mut remaining = &value[pieces[0].len()..];
    for piece in &pieces[1..pieces.len() - 1] {
        let Some(pos) = remaining.find(piece) else {
            return false;
        };
        remaining = &remaining[pos + piece.len()..];
    }
    true
}

fn parse_pnpm(text: &str) -> Result<Vec<String>, &'static str> {
    let mut found = false;
    let mut in_packages = false;
    let mut patterns = Vec::new();
    let mut document = 0;
    let mut seen_content = false;
    for line in text.lines() {
        if line
            .chars()
            .take_while(|c| c.is_whitespace())
            .any(|c| c == '\t')
        {
            return Err("document");
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed == "---" {
            document += 1;
            if document > 1 || seen_content {
                return Err("document");
            }
            continue;
        }
        seen_content = true;
        if let Some(rest) = line.strip_prefix("packages:") {
            if found {
                return Err("packages");
            }
            found = true;
            in_packages = true;
            let tail = rest.trim();
            if !tail.is_empty() {
                if !tail.starts_with('[') || !tail.ends_with(']') {
                    return Err("packages");
                }
                for value in tail[1..tail.len() - 1].split(',') {
                    patterns.push(scalar(value.trim())?);
                }
                in_packages = false;
            }
        } else if in_packages && trimmed.starts_with("- ") {
            patterns.push(scalar(trimmed[2..].trim())?);
        } else if !line.starts_with(' ') {
            in_packages = false;
        } else if in_packages {
            return Err("packages");
        }
    }
    Ok(patterns)
}

fn scalar(value: &str) -> Result<String, &'static str> {
    if value.is_empty() || value.starts_with('!') || value.starts_with(['&', '*', '|', '>']) {
        return Err("packages");
    }
    let unquoted = if value.starts_with('"') && value.ends_with('"')
        || value.starts_with('\'') && value.ends_with('\'')
    {
        &value[1..value.len() - 1]
    } else {
        value
    };
    if unquoted.is_empty() {
        return Err("packages");
    }
    Ok(unquoted.to_string())
}

#[cfg(test)]
mod tests;
