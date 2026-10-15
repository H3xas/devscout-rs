use std::collections::HashSet;

use super::config::ModuleConfig;
use super::disclosure::Reason;
use super::modes::Mode;
use super::package_map::{self, MapMatch};
use super::packages::Package;
use super::{
    alias_targets, dir_of, extension_of, join_rel, resolve_specifier, scope_for, TsAliasScopes,
};

#[derive(Clone, Copy)]
pub(super) enum Usage {
    Import,
    Require,
}

impl Usage {
    fn condition(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Require => "require",
        }
    }
}

pub(super) enum Resolution {
    Resolved(String),
    Unresolved(Reason),
    LegacyExternal,
}

pub(super) struct Resolver<'a> {
    pub files: &'a HashSet<&'a str>,
    pub alias: &'a TsAliasScopes,
    pub config: &'a ModuleConfig,
}

impl Resolver<'_> {
    pub fn static_usage(&self, file: &str) -> Usage {
        let Some(scope) = self.config.mode(file) else {
            return Usage::Import;
        };
        match scope.mode {
            Mode::Node16 | Mode::NodeNext => self.file_format(file),
            Mode::Bundler if file.ends_with(".cts") || file.ends_with(".cjs") => Usage::Require,
            _ => Usage::Import,
        }
    }

    fn file_format(&self, file: &str) -> Usage {
        if file.ends_with(".mts") || file.ends_with(".mjs") {
            return Usage::Import;
        }
        if file.ends_with(".cts") || file.ends_with(".cjs") {
            return Usage::Require;
        }
        let package = self
            .config
            .packages
            .as_ref()
            .and_then(|packages| packages.nearest(file));
        if package.and_then(|pkg| pkg.module_type.as_deref()) == Some("module") {
            Usage::Import
        } else {
            Usage::Require
        }
    }

    pub fn resolve(&self, file: &str, spec: &str, usage: Usage) -> Resolution {
        let Some(scope) = self.config.mode(file) else {
            return resolve_specifier(file, spec, self.files, self.alias)
                .map_or(Resolution::LegacyExternal, Resolution::Resolved);
        };
        if scope.mode == Mode::Classic {
            return resolve_specifier(file, spec, self.files, self.alias)
                .map_or(Resolution::LegacyExternal, Resolution::Resolved);
        }
        if matches!(scope.mode, Mode::Node16 | Mode::NodeNext)
            && self
                .config
                .packages
                .as_ref()
                .is_some_and(|packages| packages.nearest_refused(file))
        {
            return Resolution::Unresolved(Reason::ConfigurationRefused);
        }
        if spec.starts_with('.') {
            return self.relative(file, spec, scope.mode, usage);
        }
        let aliases = scope_for(file, self.alias);
        let targets = alias_targets(spec, &aliases.aliases);
        let matched = !targets.is_empty();
        let strict =
            matches!(scope.mode, Mode::Node16 | Mode::NodeNext) && matches!(usage, Usage::Import);
        for candidate in targets.into_iter().flatten() {
            if let Some(target) = self.probe(&candidate, !strict, strict) {
                return Resolution::Resolved(target);
            }
        }
        if !matched {
            if let Some(base) = &aliases.base_url_dir {
                if let Some(path) = join_rel(base, spec) {
                    if let Some(target) = self.probe(&path, !strict, strict) {
                        return Resolution::Resolved(target);
                    }
                }
            }
        }
        if spec.starts_with('#') && scope.mode != Mode::Node10 {
            return self.package_import(file, spec, scope.mode, usage);
        }
        if spec.contains(':') {
            return Resolution::Unresolved(Reason::BarePackageOutsideWorkspace);
        }
        let (name, subpath) = package_name(spec);
        if scope.mode != Mode::Node10 {
            if let Some(own) = self
                .config
                .packages
                .as_ref()
                .and_then(|packages| packages.nearest(file))
            {
                if own.name.as_deref() == Some(name.as_str()) && own.exports.is_some() {
                    return self.package_target(
                        own,
                        &subpath,
                        scope.mode,
                        usage,
                        &scope.custom_conditions,
                    );
                }
            }
        }
        let Some(workspace) = self
            .config
            .workspaces
            .as_ref()
            .and_then(|workspaces| workspaces.nearest(file))
        else {
            return Resolution::Unresolved(Reason::BarePackageOutsideWorkspace);
        };
        if workspace.refused {
            return Resolution::Unresolved(Reason::ConfigurationRefused);
        }
        let Some(members) = workspace.members.get(&name) else {
            return Resolution::Unresolved(Reason::BarePackageOutsideWorkspace);
        };
        if members.len() > 1 {
            return Resolution::Unresolved(Reason::AmbiguousWorkspaceName);
        }
        let Some(package) = self
            .config
            .packages
            .as_ref()
            .and_then(|packages| packages.by_dir.get(&members[0]))
        else {
            return Resolution::Unresolved(Reason::ConfigurationRefused);
        };
        self.package_target(
            package,
            &subpath,
            scope.mode,
            usage,
            &scope.custom_conditions,
        )
    }

    fn relative(&self, file: &str, spec: &str, mode: Mode, usage: Usage) -> Resolution {
        let Some(path) = join_rel(dir_of(file), spec) else {
            return Resolution::Unresolved(Reason::TargetNotIndexed);
        };
        let strict =
            matches!(mode, Mode::Node16 | Mode::NodeNext) && matches!(usage, Usage::Import);
        if !strict {
            return self.probe(&path, true, false).map_or(
                Resolution::Unresolved(Reason::TargetNotIndexed),
                Resolution::Resolved,
            );
        }
        if !extension_of(&path).is_empty() {
            return self.probe(&path, false, true).map_or(
                Resolution::Unresolved(Reason::TargetNotIndexed),
                Resolution::Resolved,
            );
        }
        if self.probe_file(&path, false).is_some() {
            return Resolution::Unresolved(Reason::EsmExtensionRequired);
        }
        if self.probe_directory(&path).is_some() {
            return Resolution::Unresolved(Reason::DirectoryImportNotAllowed);
        }
        Resolution::Unresolved(Reason::TargetNotIndexed)
    }

    fn package_import(&self, file: &str, spec: &str, mode: Mode, usage: Usage) -> Resolution {
        let Some(package) = self
            .config
            .packages
            .as_ref()
            .and_then(|packages| packages.nearest(file))
        else {
            return Resolution::Unresolved(Reason::ImportsSpecifierNotMatched);
        };
        let Some(map) = &package.imports else {
            return Resolution::Unresolved(Reason::ImportsSpecifierNotMatched);
        };
        let conditions = &self
            .config
            .mode(file)
            .expect("declared file")
            .custom_conditions;
        self.map_target(
            package,
            package_map::imports(map, spec, usage.condition(), mode, conditions),
            true,
            matches!(mode, Mode::Node16 | Mode::NodeNext) && matches!(usage, Usage::Import),
        )
    }

    fn package_target(
        &self,
        package: &Package,
        subpath: &str,
        mode: Mode,
        usage: Usage,
        conditions: &[String],
    ) -> Resolution {
        if mode != Mode::Node10 {
            if let Some(map) = &package.exports {
                return self.map_target(
                    package,
                    package_map::exports(map, subpath, usage.condition(), mode, conditions),
                    false,
                    matches!(mode, Mode::Node16 | Mode::NodeNext) && matches!(usage, Usage::Import),
                );
            }
        }
        let path = if subpath == "." {
            package.main.as_deref().unwrap_or("index")
        } else {
            subpath.strip_prefix("./").unwrap_or(subpath)
        };
        let Some(target) = join_rel(&package.dir, path) else {
            return Resolution::Unresolved(Reason::TargetNotIndexed);
        };
        self.package_file(package, &target, true, false)
    }

    fn map_target(
        &self,
        package: &Package,
        selected: MapMatch,
        imports: bool,
        strict: bool,
    ) -> Resolution {
        match selected {
            MapMatch::NotMatched => Resolution::Unresolved(if imports {
                Reason::ImportsSpecifierNotMatched
            } else {
                Reason::ExportsSubpathNotMatched
            }),
            MapMatch::NoCondition => Resolution::Unresolved(Reason::NoMatchingCondition),
            MapMatch::Targets(targets) => {
                let mut last = Reason::TargetNotIndexed;
                for target in targets {
                    if !target.starts_with("./") {
                        continue;
                    }
                    let Some(path) = join_rel(&package.dir, &target) else {
                        continue;
                    };
                    match self.package_file(package, &path, false, strict) {
                        Resolution::Resolved(hit) => return Resolution::Resolved(hit),
                        Resolution::Unresolved(reason) => last = reason,
                        Resolution::LegacyExternal => {}
                    }
                }
                Resolution::Unresolved(last)
            }
        }
    }

    fn package_file(
        &self,
        package: &Package,
        target: &str,
        directory: bool,
        strict: bool,
    ) -> Resolution {
        if let Some(hit) = self
            .config
            .build_outputs
            .as_ref()
            .and_then(|outputs| outputs.source(&package.dir, target, self.files))
        {
            return Resolution::Resolved(hit);
        }
        if let Some(hit) = self.probe(target, directory, strict) {
            return Resolution::Resolved(hit);
        }
        if [".d.ts", ".d.mts", ".d.cts"]
            .iter()
            .any(|suffix| target.ends_with(suffix))
        {
            return Resolution::Unresolved(Reason::DeclarationOnlyTarget);
        }
        if self
            .config
            .build_outputs
            .as_ref()
            .is_some_and(|outputs| outputs.covers(&package.dir, target))
        {
            return Resolution::Unresolved(Reason::BuildOutputWithoutSourceMapping);
        }
        Resolution::Unresolved(Reason::TargetNotIndexed)
    }

    fn probe(&self, path: &str, directory: bool, strict: bool) -> Option<String> {
        self.probe_file(path, strict)
            .or_else(|| directory.then(|| self.probe_directory(path)).flatten())
    }

    fn probe_file(&self, path: &str, strict: bool) -> Option<String> {
        let extension = extension_of(path);
        if (strict || matches!(extension, ".ts" | ".tsx" | ".mts" | ".cts"))
            && !matches!(extension, ".js" | ".jsx" | ".mjs" | ".cjs")
        {
            return (![".d.ts", ".d.mts", ".d.cts"]
                .iter()
                .any(|declaration| path.ends_with(declaration))
                && self.files.contains(path))
            .then(|| path.to_string());
        }
        let candidates: &[&str] = match extension {
            ".js" => &[".ts", ".tsx", ".js", ".jsx"],
            ".jsx" => &[".tsx", ".ts", ".jsx", ".js"],
            ".mjs" => &[".mts", ".mjs"],
            ".cjs" => &[".cts", ".cjs"],
            "" => &[".ts", ".tsx", ".js", ".jsx"],
            _ => &[".ts", ".tsx", ".js", ".jsx", extension],
        };
        let stem = if extension.is_empty() || !matches!(extension, ".js" | ".jsx" | ".mjs" | ".cjs")
        {
            path
        } else {
            &path[..path.len() - extension.len()]
        };
        for suffix in candidates {
            let candidate = if *suffix == extension && !extension.is_empty() && stem == path {
                path.to_string()
            } else {
                format!("{stem}{suffix}")
            };
            if ![".d.ts", ".d.mts", ".d.cts"]
                .iter()
                .any(|declaration| candidate.ends_with(declaration))
                && self.files.contains(candidate.as_str())
            {
                return Some(candidate);
            }
        }
        None
    }

    fn probe_directory(&self, path: &str) -> Option<String> {
        if let Some(package) = self
            .config
            .packages
            .as_ref()
            .and_then(|packages| packages.by_dir.get(path))
        {
            if let Some(main) = &package.main {
                if let Some(candidate) = join_rel(path, main) {
                    if let Some(hit) = self.probe_file(&candidate, false) {
                        return Some(hit);
                    }
                }
            }
        }
        self.probe_file(&format!("{path}/index"), false)
    }
}

fn package_name(spec: &str) -> (String, String) {
    let count = if spec.starts_with('@') { 2 } else { 1 };
    let mut parts = spec.split('/');
    let name = parts.by_ref().take(count).collect::<Vec<_>>().join("/");
    let rest = parts.collect::<Vec<_>>().join("/");
    let subpath = if rest.is_empty() {
        ".".to_string()
    } else {
        format!("./{rest}")
    };
    (name, subpath)
}

#[cfg(test)]
mod tests;
