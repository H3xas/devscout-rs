namespace ScoutSemantic;

/// <summary>
/// One compilation identity: the project path and name with the requested
/// target, configuration and platform. Distinct targets or configurations of
/// one project are distinct identities and never merge.
/// </summary>
internal sealed class ContextIdentity
{
    public required string ProjectPath { get; init; }

    public required string ProjectName { get; init; }

    /// <summary>The <c>--tfm</c> that produced this record, or null when none was requested.</summary>
    public required string? RequestedTfm { get; init; }

    /// <summary>The target Roslyn actually compiled, when a compilation exists.</summary>
    public string? EffectiveTfm { get; init; }

    public string? Configuration { get; init; }

    public string? Platform { get; init; }

    /// <summary>Every target a variant of this project actually declares; populated for an <c>unsupported</c> record so it can name both sides.</summary>
    public List<string>? DeclaredTfms { get; init; }
}

/// <summary>A metadata or project reference the compilation was given.</summary>
internal sealed class ContextReference
{
    /// <summary><c>metadata</c> or <c>project</c>.</summary>
    public required string Kind { get; init; }

    public required string Name { get; init; }

    /// <summary>Assembly identity (name + MVID, or a content hash fallback) for a metadata reference.</summary>
    public string? Identity { get; init; }

    /// <summary>The referenced project's own fingerprint, for a project reference.</summary>
    public string? Fingerprint { get; init; }
}

/// <summary>An imported build file, recorded by normalized identity plus content hash, never by absolute local path.</summary>
internal sealed class ContextImport
{
    public required string Identity { get; init; }

    public required string Hash { get; init; }
}

/// <summary>One document the expected inventory named but the loaded workspace did not carry, with why.</summary>
internal sealed class DroppedDocument
{
    public required string Path { get; init; }

    /// <summary><c>missing</c>, <c>linked-outside-root</c>, <c>skipped-directory</c>, or <c>out-of-scope</c>.</summary>
    public required string Reason { get; init; }
}

/// <summary>Expected-versus-loaded document inventory, with every difference classified.</summary>
internal sealed class ContextDocuments
{
    public List<string> Expected { get; init; } = new();

    public List<string> Loaded { get; init; } = new();

    public List<DroppedDocument> Dropped { get; init; } = new();
}

/// <summary>One source-generated document, inventoried separately from authored documents.</summary>
internal sealed class GeneratedDocument
{
    public required string HintName { get; init; }

    /// <summary>The generator's display name, or null when it could not be determined (fallback path).</summary>
    public string? Generator { get; init; }
}

internal sealed class ContextGenerated
{
    public List<GeneratedDocument> Documents { get; init; } = new();

    public List<string> Diagnostics { get; init; } = new();
}

/// <summary>One <see cref="Microsoft.CodeAnalysis.WorkspaceDiagnostic"/>, reported verbatim under Roslyn's own vocabulary.</summary>
internal sealed class WorkspaceDiagnosticRecord
{
    /// <summary><c>Failure</c> or <c>Warning</c> -- Roslyn does not distinguish restore/evaluation/load stages further.</summary>
    public required string Kind { get; init; }

    public required string Message { get; init; }
}

/// <summary>One raw compiler diagnostic.</summary>
internal sealed class CompilerDiagnosticRecord
{
    public required string Severity { get; init; }

    public required string Id { get; init; }

    public required string Message { get; init; }

    public string? File { get; init; }

    public int? Line { get; init; }
}

/// <summary>Raw restore/workspace and compiler diagnostics, never a count.</summary>
internal sealed class ContextDiagnostics
{
    public List<WorkspaceDiagnosticRecord> Workspace { get; init; } = new();

    public List<CompilerDiagnosticRecord> Compiler { get; init; } = new();
}

/// <summary>The SDK, MSBuild, compiler and engine versions that produced one record.</summary>
internal sealed class ContextVersions
{
    public required string Sdk { get; init; }

    public required string Msbuild { get; init; }

    public required string Compiler { get; init; }

    public required string Engine { get; init; }
}

/// <summary>
/// One record per requested/selected/excluded compilation identity. State is
/// never inferred from a non-null compilation alone; it is set by the caller
/// from the load and inventory results, and only agrees when every input --
/// diagnostics, requested target, inventory -- actually earns it.
/// </summary>
internal sealed class ContextRecord
{
    public required ContextIdentity Identity { get; init; }

    /// <summary><c>complete</c>, <c>partial</c>, <c>unsupported</c>, <c>failed</c>, or <c>excluded</c>.</summary>
    public required string State { get; init; }

    /// <summary>Machine-readable, non-empty, e.g. <c>not-requested</c>, <c>undeclared-target</c>, <c>binding-error</c>.</summary>
    public required string Reason { get; init; }

    /// <summary>The context fingerprint, null only for a record with no compilation at all (<c>unsupported</c>).</summary>
    public string? Fingerprint { get; init; }

    public ContextVersions? Versions { get; init; }

    public List<ContextReference> References { get; init; } = new();

    public List<ContextImport> Imports { get; init; } = new();

    public Dictionary<string, string?> LanguageOptions { get; init; } = new();

    public List<string> PreprocessorSymbols { get; init; } = new();

    public ContextDocuments Documents { get; init; } = new();

    public ContextGenerated Generated { get; init; } = new();

    public ContextDiagnostics Diagnostics { get; init; } = new();
}

/// <summary>The top-level build-context document (<c>--emit context</c>). <c>schemaVersion</c> starts at 1, a stable literal a downstream admission path keys on.</summary>
internal sealed class ContextEnvelope
{
    public int SchemaVersion { get; init; } = 1;

    public required string Producer { get; init; }

    public required string Version { get; init; }

    public required string Repo { get; init; }

    /// <summary>Root-relative, forward-slashed path of the analysed solution or project.</summary>
    public required string Solution { get; init; }

    public List<ContextRecord> Compilations { get; init; } = new();
}
