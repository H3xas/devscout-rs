use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Reason {
    BarePackageOutsideWorkspace,
    EsmExtensionRequired,
    DirectoryImportNotAllowed,
    ExportsSubpathNotMatched,
    ImportsSpecifierNotMatched,
    NoMatchingCondition,
    BuildOutputWithoutSourceMapping,
    DeclarationOnlyTarget,
    TargetNotIndexed,
    AmbiguousWorkspaceName,
    ConfigurationRefused,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BarePackageOutsideWorkspace => "bare-package-outside-workspace",
            Self::EsmExtensionRequired => "esm-extension-required",
            Self::DirectoryImportNotAllowed => "directory-import-not-allowed",
            Self::ExportsSubpathNotMatched => "exports-subpath-not-matched",
            Self::ImportsSpecifierNotMatched => "imports-specifier-not-matched",
            Self::NoMatchingCondition => "no-matching-condition",
            Self::BuildOutputWithoutSourceMapping => "build-output-without-source-mapping",
            Self::DeclarationOnlyTarget => "declaration-only-target",
            Self::TargetNotIndexed => "target-not-indexed",
            Self::AmbiguousWorkspaceName => "ambiguous-workspace-name",
            Self::ConfigurationRefused => "configuration-refused",
        }
    }
}

/// An import that a declared mode could not place in the indexed repository.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnresolvedImport {
    /// Repository-relative importing file.
    pub from_file: String,
    /// One-based line of the import form.
    pub from_line: usize,
    /// Module specifier as written.
    pub target: String,
    /// One stable reason from the declared resolver's vocabulary.
    pub reason: String,
}

/// A tsconfig chain that declares the recognised but unsupported classic mode.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnsupportedModeScope {
    /// Repository-relative config path.
    pub config: String,
    /// The unsupported mode name.
    pub mode: String,
}

/// A configuration document the module resolver refused to use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RefusedConfigRecord {
    /// Repository-relative configuration path.
    pub file: String,
    /// The first invalid field in the document.
    pub field: String,
}

pub(super) const REASONS: &[Reason] = &[
    Reason::BarePackageOutsideWorkspace,
    Reason::EsmExtensionRequired,
    Reason::DirectoryImportNotAllowed,
    Reason::ExportsSubpathNotMatched,
    Reason::ImportsSpecifierNotMatched,
    Reason::NoMatchingCondition,
    Reason::BuildOutputWithoutSourceMapping,
    Reason::DeclarationOnlyTarget,
    Reason::TargetNotIndexed,
    Reason::AmbiguousWorkspaceName,
    Reason::ConfigurationRefused,
];
