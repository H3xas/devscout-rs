namespace ScoutSemantic;

/// <summary>Raised when a context record does not satisfy the build-context schema.</summary>
internal sealed class ContextSchemaException : Exception
{
    public ContextSchemaException(string reason)
        : base(reason)
    {
    }
}

/// <summary>
/// Checks the build-context schema's runtime-only invariants: the shape a
/// required init-only property already guarantees needs no check here, so
/// this is left with exactly what the type system cannot express -- forward
/// slashes, 1-based lines, a non-empty reason, and no absolute local path
/// leaking into a path-shaped field.
/// </summary>
internal static class ContextSchema
{
    private static readonly HashSet<string> ValidStates = new(StringComparer.Ordinal)
    {
        "complete", "partial", "unsupported", "failed", "excluded",
    };

    /// <summary>
    /// Throws <see cref="ContextSchemaException"/> on the first violation found
    /// in <paramref name="record"/>. Called once per record before
    /// <see cref="ContextWriter"/> opens any output, so one bad record leaves
    /// no partial file behind.
    /// </summary>
    public static void Validate(ContextRecord record)
    {
        if (!ValidStates.Contains(record.State))
        {
            throw new ContextSchemaException($"unknown context state '{record.State}'");
        }

        if (record.Reason.Length == 0)
        {
            throw new ContextSchemaException($"{record.Identity.ProjectName}: reason is empty");
        }

        RequireRelative(record.Identity.ProjectPath, "identity.projectPath", record);

        foreach (var doc in record.Documents.Expected)
        {
            RequireRelative(doc, "documents.expected[]", record);
        }

        foreach (var doc in record.Documents.Loaded)
        {
            RequireRelative(doc, "documents.loaded[]", record);
        }

        foreach (var dropped in record.Documents.Dropped)
        {
            RequireRelative(dropped.Path, "documents.dropped[].path", record);
            if (dropped.Reason.Length == 0)
            {
                throw new ContextSchemaException(
                    $"{record.Identity.ProjectName}: a dropped document has an empty reason");
            }
        }

        foreach (var import in record.Imports)
        {
            RequireNoAbsolutePath(import.Identity, "imports[].identity", record);
        }

        foreach (var generated in record.Generated.Documents)
        {
            RequireNoAbsolutePath(generated.HintName, "generated.documents[].hintName", record);
        }

        foreach (var diagnostic in record.Diagnostics.Compiler)
        {
            if (diagnostic.File is { } file)
            {
                RequireRelative(file, "diagnostics.compiler[].file", record);
            }

            if (diagnostic.Line is { } line && line < 1)
            {
                throw new ContextSchemaException(
                    $"{record.Identity.ProjectName}: a compiler diagnostic reports line {line}, expected 1-based");
            }

            RequireNoAbsolutePathInFreeText(diagnostic.Message, "diagnostics.compiler[].message", record);
        }

        foreach (var diagnostic in record.Diagnostics.Workspace)
        {
            RequireNoAbsolutePathInFreeText(diagnostic.Message, "diagnostics.workspace[].message", record);
        }

        foreach (var generatorDiagnostic in record.Generated.Diagnostics)
        {
            RequireNoAbsolutePathInFreeText(generatorDiagnostic, "generated.diagnostics[]", record);
        }
    }

    private static void RequireRelative(string path, string field, ContextRecord record)
    {
        RequireNoAbsolutePath(path, field, record);
        if (path.Contains('\\', StringComparison.Ordinal))
        {
            throw new ContextSchemaException(
                $"{record.Identity.ProjectName}: {field} '{path}' is not forward-slashed");
        }
    }

    /// <summary>
    /// The leak guard: an SDK or NuGet build file resolved outside the fixture
    /// tree is folded into the fingerprint by content hash, but a path-shaped
    /// field must never carry the local absolute path itself.
    /// </summary>
    private static void RequireNoAbsolutePath(string path, string field, ContextRecord record)
    {
        if (IsAbsolute(path))
        {
            throw new ContextSchemaException(
                $"{record.Identity.ProjectName}: {field} '{path}' is an absolute local path");
        }
    }

    private static bool IsAbsolute(string path) =>
        path.StartsWith('/') || (path.Length >= 2 && path[1] == ':');

    /// <summary>
    /// The leak guard's free-text counterpart: a path-shaped field can only
    /// ever carry a single path, but a diagnostic message is prose that can
    /// embed one anywhere in it (a build tool's own error text quoting the
    /// project file it was processing, for example). Every whitespace- or
    /// quote-delimited token is checked the same way a bare path field is,
    /// so a leak in free text is caught by the same rule rather than a
    /// second, weaker one.
    /// </summary>
    private static void RequireNoAbsolutePathInFreeText(string text, string field, ContextRecord record)
    {
        foreach (var token in text.Split(FreeTextDelimiters, StringSplitOptions.RemoveEmptyEntries))
        {
            if (IsAbsolute(token))
            {
                throw new ContextSchemaException(
                    $"{record.Identity.ProjectName}: {field} contains an absolute local path");
            }
        }
    }

    private static readonly char[] FreeTextDelimiters = { ' ', '\t', '\n', '\'', '"', '(', ')' };
}
