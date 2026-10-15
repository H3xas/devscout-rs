use std::path::Path;

use super::build_output::{read_build_outputs, BuildOutputs};
use super::modes::{read_modes, ModeScope, ModeScopes, RefusedConfig};
use super::packages::{read_packages, Packages};
use super::workspaces::{read_workspaces, Workspaces};
use super::{read_ts_alias_scopes, TsAliasScopes};

/// Immutable module settings read before TypeScript graph resolution.
#[derive(Default)]
pub struct ModuleConfig {
    pub(super) modes: ModeScopes,
    pub(super) packages: Option<Packages>,
    pub(super) workspaces: Option<Workspaces>,
    pub(super) build_outputs: Option<BuildOutputs>,
    pub(super) refused: Vec<RefusedConfig>,
}

impl ModuleConfig {
    /// Whether no indexed file has a declared resolution mode.
    pub fn inert(&self) -> bool {
        self.modes.inert()
    }

    pub(super) fn mode(&self, file: &str) -> Option<&ModeScope> {
        self.modes.get(file)
    }
}

/// Reads path aliases and declared module settings for the indexed files.
pub fn read_ts_resolution<'a>(
    root: &Path,
    files: impl IntoIterator<Item = &'a str>,
) -> (TsAliasScopes, ModuleConfig) {
    let files: Vec<&str> = files.into_iter().collect();
    let alias = read_ts_alias_scopes(root, files.iter().copied());
    let modes = read_modes(root, files.iter().copied());
    let mut config = ModuleConfig {
        refused: modes.refused.clone(),
        modes,
        ..ModuleConfig::default()
    };
    if config.inert() {
        config.refused.retain(|entry| entry.field == "document");
        return (alias, config);
    }
    let packages = read_packages(root);
    let workspaces = read_workspaces(root, &packages);
    let build_outputs = read_build_outputs(root, &packages);
    config.refused.extend(packages.refused.iter().cloned());
    config.refused.extend(workspaces.refused.iter().cloned());
    config.refused.extend(build_outputs.refused.iter().cloned());
    config
        .refused
        .sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.field.cmp(&b.field)));
    config.refused.dedup_by(|a, b| a.file == b.file);
    config.packages = Some(packages);
    config.workspaces = Some(workspaces);
    config.build_outputs = Some(build_outputs);
    (alias, config)
}

#[cfg(test)]
mod tests;
