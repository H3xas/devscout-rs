//! The project inputs transport recognition reads besides source files: the
//! namespaces each project's SDK and `Using` items import, and the settings
//! files in each project directory.
//!
//! Neither is a graph source, so an edit to one moves no indexed file. The
//! sidecar this module writes is how `map` notices such an edit.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::catalogue::IMPLICIT_USINGS;
use crate::graph::{atomic_write_json, transport_inputs_path};
use crate::project::ProjectModel;

/// What one map read for transport recognition, per project unit.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TransportInputs {
    /// One entry per project unit, sorted by unit id.
    pub units: Vec<UnitInputs>,
}

/// The inputs of one project unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitInputs {
    /// The unit's `.csproj` path.
    pub unit: String,
    /// The namespaces the SDK's implicit usings and the `Using` items add.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub namespaces: Vec<String>,
    /// The settings files directly in the unit's directory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<SettingsFile>,
}

/// One settings file and the digest of the bytes it held.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsFile {
    /// The repository path.
    pub file: String,
    /// The SHA-256 of its content.
    pub digest: String,
}

/// Whether a repository path names a settings file a binding reads:
/// `appsettings.json` or `appsettings.<name>.json`.
pub(crate) fn is_settings_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let Some(stem) = name.strip_suffix(".json") else {
        return false;
    };
    stem == "appsettings"
        || stem
            .strip_prefix("appsettings.")
            .is_some_and(|s| !s.is_empty())
}

fn parent_dir(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Discovers the inputs of every unit of `model`. `files` is the run's
/// walked file list, so a settings file outside the scope or ignored by git
/// is never read.
pub(crate) fn discover(
    root: &Path,
    model: Option<&ProjectModel>,
    files: &[String],
) -> TransportInputs {
    let Some(model) = model else {
        return TransportInputs::default();
    };
    let mut units = Vec::new();
    for unit in &model.units {
        let csproj = fs::read(root.join(&unit.id))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        let mut settings: Vec<SettingsFile> = files
            .iter()
            .filter(|f| is_settings_file(f) && parent_dir(f) == unit.dir)
            .filter_map(|f| {
                let bytes = fs::read(root.join(f)).ok()?;
                Some(SettingsFile {
                    file: f.clone(),
                    digest: crate::freshness::record::sha256_hex(&bytes),
                })
            })
            .collect();
        settings.sort_by(|a, b| a.file.cmp(&b.file));
        units.push(UnitInputs {
            unit: unit.id.clone(),
            namespaces: project_namespaces(&csproj),
            settings,
        });
    }
    TransportInputs { units }
}

/// The namespaces a project file imports for every file of the project:
/// the SDK's implicit usings when they are enabled, plus each plain
/// `<Using Include>` item, less each `<Using Remove>` item. An item with an
/// alias or `Static` adds nothing.
pub(crate) fn project_namespaces(csproj: &str) -> Vec<String> {
    let mut namespaces: Vec<String> = Vec::new();
    if implicit_usings_enabled(csproj) {
        let sdk = sdk_name(csproj);
        if let Some((_, table)) = IMPLICIT_USINGS
            .iter()
            .find(|(name, _)| Some(*name) == sdk.as_deref())
        {
            namespaces.extend(table.iter().map(|n| (*n).to_string()));
        }
    }
    for tag in using_items(csproj) {
        if attribute(tag, "Alias").is_some() || attribute(tag, "Static").is_some() {
            continue;
        }
        if let Some(include) = attribute(tag, "Include") {
            namespaces.push(include);
        } else if let Some(remove) = attribute(tag, "Remove") {
            namespaces.retain(|n| *n != remove);
        }
    }
    namespaces.sort();
    namespaces.dedup();
    namespaces
}

fn sdk_name(csproj: &str) -> Option<String> {
    let start = csproj.find("<Project")?;
    let end = csproj[start..].find('>')? + start;
    let sdk = attribute(&csproj[start..end], "Sdk")?;
    Some(sdk.split('/').next().unwrap_or(&sdk).trim().to_string())
}

fn implicit_usings_enabled(csproj: &str) -> bool {
    let mut enabled = false;
    let mut rest = csproj;
    while let Some(at) = rest.find("<ImplicitUsings>") {
        let after = &rest[at + "<ImplicitUsings>".len()..];
        let value = after.split('<').next().unwrap_or("").trim();
        enabled = value.eq_ignore_ascii_case("enable") || value.eq_ignore_ascii_case("true");
        rest = after;
    }
    enabled
}

fn using_items(csproj: &str) -> Vec<&str> {
    let mut tags = Vec::new();
    let mut rest = csproj;
    while let Some(at) = rest.find("<Using") {
        let after = &rest[at + "<Using".len()..];
        let end = after.find('>').unwrap_or(after.len());
        if after.starts_with(|c: char| c.is_whitespace() || c == '/') {
            tags.push(&after[..end]);
        }
        rest = &after[end..];
    }
    tags
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let mut rest = tag;
    while let Some(at) = rest.find(name) {
        let before = rest[..at].chars().last();
        let after = rest[at + name.len()..].trim_start();
        rest = &rest[at + name.len()..];
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let Some(after) = after.strip_prefix('=') else {
            continue;
        };
        let after = after.trim_start();
        let quote = after.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let value = &after[1..];
        return value.find(quote).map(|end| value[..end].to_string());
    }
    None
}

/// Whether the sidecar on disk records other inputs than `inputs`. A
/// missing sidecar is no difference: the last rebuild found no transport
/// site, and a new one moves a source file.
pub(crate) fn sidecar_differs(root: &Path, inputs: &TransportInputs) -> bool {
    match fs::read(transport_inputs_path(root)) {
        Ok(bytes) => serde_json::to_vec(inputs).map_or(true, |fresh| fresh != bytes),
        Err(_) => false,
    }
}

/// Writes the sidecar when a fragment holds a transport site, and removes
/// it otherwise. `None` leaves it as it is.
pub(crate) fn write_sidecar(
    root: &Path,
    inputs: Option<&TransportInputs>,
    any_site: bool,
) -> io::Result<()> {
    let Some(inputs) = inputs else {
        return Ok(());
    };
    let path = transport_inputs_path(root);
    if any_site {
        return atomic_write_json(&path, inputs);
    }
    match fs::remove_file(&path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
#[path = "inputs_tests.rs"]
mod tests;
