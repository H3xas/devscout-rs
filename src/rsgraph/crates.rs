//! Cargo packages in the mapped scope: their crate roots and library names.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// A `Cargo.toml` is read by a line scan of section headers and `name` and
// `path` keys, with no TOML dependency. Roots follow cargo's conventions: the
// `path` of each `[lib]`, `[[bin]]`, `[[test]]`, `[[example]]` and
// `[[bench]]` entry, plus the default target files the walk listed:
// `src/lib.rs`, `src/main.rs`, `build.rs`, and each `tests`, `examples` and
// `benches` folder's `<name>.rs` and `<name>/main.rs` files. A root
// is only ever a file the walk already listed inside the package, so a
// manifest can never point the map outside the mapped root. A
// `[workspace]`-only manifest names no root.
//
// The model is rediscovered on every map and compared with a sidecar in the
// graph directory: a manifest is not a source file, so no walked mtime moves
// when one changes, and the sidecar is how a manifest edit rebuilds.
// ---------------------------------------------------------------------------

/// What a crate root builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RootKind {
    /// The library.
    Lib,
    /// A binary.
    Bin,
    /// An integration test.
    Test,
    /// An example.
    Example,
    /// A benchmark.
    Bench,
    /// The build script.
    Build,
}

/// One crate root file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrateRoot {
    /// The root file, relative to the mapped root.
    pub file: String,
    /// What it builds.
    pub kind: RootKind,
}

/// One Cargo package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustPackage {
    /// The package directory, relative to the mapped root (`""` at the root).
    pub dir: String,
    /// The `[package] name`.
    pub name: String,
    /// The library crate's name, present only when a library root exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lib: Option<String>,
    /// Every crate root, library first, then binaries, tests, examples,
    /// benchmarks and the build script, each by file.
    pub roots: Vec<CrateRoot>,
}

/// Every Cargo package in the mapped scope, sorted by directory.
pub type RustCrates = Vec<RustPackage>;

/// Discovers the Cargo packages in `scope`, `None` when no manifest with a
/// `[package]` section is there. `rs_files` is the walk's own list of `.rs`
/// files, the only files a root can be.
pub fn discover(
    root: &Path,
    scope: &[String],
    rs_files: &[String],
) -> io::Result<Option<RustCrates>> {
    let walked: HashSet<&str> = rs_files.iter().map(String::as_str).collect();
    let mut packages = Vec::new();
    for rel in crate::walk::list_files_with_ext(root, scope, &[".toml"])? {
        if rel != "Cargo.toml" && !rel.ends_with("/Cargo.toml") {
            continue;
        }
        let Ok(bytes) = fs::read(root.join(&rel)) else {
            continue;
        };
        let dir = rel
            .strip_suffix("Cargo.toml")
            .unwrap_or("")
            .trim_end_matches('/');
        if let Some(package) = package_of(dir, &String::from_utf8_lossy(&bytes), &walked) {
            packages.push(package);
        }
    }
    packages.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok((!packages.is_empty()).then_some(packages))
}

// The facts one manifest's line scan collects.
#[derive(Default)]
struct Manifest {
    package: Option<String>,
    lib_name: Option<String>,
    lib_path: Option<String>,
    targets: Vec<(RootKind, String)>,
}

fn scan(text: &str) -> Manifest {
    let mut m = Manifest::default();
    let mut section = String::new();
    let mut has_package = false;
    for raw in text.lines() {
        let line = strip_comment(raw.trim_end_matches('\r')).trim();
        if let Some(header) = line.strip_prefix('[') {
            section = header
                .trim_matches(|c| c == '[' || c == ']' || c == ' ')
                .to_string();
            has_package |= section == "package";
            continue;
        }
        let Some((key, value)) = key_value(line) else {
            continue;
        };
        match (section.as_str(), key) {
            ("package", "name") => m.package = Some(value),
            ("lib", "name") => m.lib_name = Some(value),
            ("lib", "path") => m.lib_path = Some(value),
            (s, "path") => {
                if let Some(kind) = array_kind(s) {
                    m.targets.push((kind, value));
                }
            }
            _ => {}
        }
    }
    if !has_package {
        m.package = None;
    }
    m
}

fn array_kind(section: &str) -> Option<RootKind> {
    Some(match section {
        "bin" => RootKind::Bin,
        "test" => RootKind::Test,
        "example" => RootKind::Example,
        "bench" => RootKind::Bench,
        _ => return None,
    })
}

// A line with its `#` comment removed, a `#` inside a string kept.
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '#') => return &line[..i],
            _ => {}
        }
    }
    line
}

// `key = "value"` or `key = 'value'`; anything else is not a fact.
fn key_value(line: &str) -> Option<(&str, String)> {
    let (key, value) = line.split_once('=')?;
    let value = value.trim();
    let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let inner = value[1..].strip_suffix(quote)?;
    Some((key.trim(), inner.to_string()))
}

fn package_of(dir: &str, text: &str, walked: &HashSet<&str>) -> Option<RustPackage> {
    let m = scan(text);
    let name = m.package?;
    let mut roots: Vec<CrateRoot> = Vec::new();
    let mut add = |kind: RootKind, file: Option<String>| {
        if let Some(file) = file.filter(|f| walked.contains(f.as_str())) {
            if !roots.iter().any(|r| r.file == file) {
                roots.push(CrateRoot { file, kind });
            }
        }
    };
    let lib = m.lib_path.clone().unwrap_or_else(|| "src/lib.rs".into());
    add(RootKind::Lib, join(dir, &lib));
    for (kind, path) in &m.targets {
        add(*kind, join(dir, path));
    }
    add(RootKind::Bin, join(dir, "src/main.rs"));
    for (folder, kind) in [
        ("tests", RootKind::Test),
        ("examples", RootKind::Example),
        ("benches", RootKind::Bench),
    ] {
        let prefix = join(dir, folder).map(|p| p + "/").unwrap_or_default();
        let mut found: Vec<&&str> = walked
            .iter()
            .filter(|f| {
                f.strip_prefix(prefix.as_str())
                    .is_some_and(is_default_target)
            })
            .collect();
        found.sort();
        for file in found {
            add(kind, Some((*file).to_string()));
        }
    }
    add(RootKind::Build, join(dir, "build.rs"));
    roots.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.file.cmp(&b.file)));
    let has_lib = roots.iter().any(|r| r.kind == RootKind::Lib);
    let lib = has_lib.then(|| m.lib_name.unwrap_or_else(|| name.replace('-', "_")));
    Some(RustPackage {
        dir: dir.to_string(),
        name,
        lib,
        roots,
    })
}

// Cargo's two auto-discovered layouts below a target folder: a direct
// `<name>.rs`, or `<name>/main.rs` exactly one directory down.
fn is_default_target(rest: &str) -> bool {
    match rest.split_once('/') {
        None => rest.ends_with(".rs"),
        Some((name, file)) => !name.is_empty() && file == "main.rs",
    }
}

// `dir/path` with `.` and `..` resolved lexically; `None` when the path
// leaves the package directory or is absolute.
pub(super) fn join(dir: &str, path: &str) -> Option<String> {
    if path.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    let rel = parts.join("/");
    Some(if dir.is_empty() {
        rel
    } else {
        format!("{dir}/{rel}")
    })
}

/// Whether the model differs from the sidecar the last rebuild wrote: no
/// model and no sidecar is no difference, and any other mismatch is one.
pub fn sidecar_differs(root: &Path, crates: Option<&RustCrates>) -> bool {
    let existing = fs::read(crate::graph::rust_crates_path(root)).ok();
    match (existing, crates) {
        (None, None) => false,
        (Some(bytes), Some(c)) => serde_json::to_vec(c).map_or(true, |fresh| bytes != fresh),
        _ => true,
    }
}
