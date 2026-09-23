using System.Text.Encodings.Web;
using System.Text.Json;

using Microsoft.CodeAnalysis;

namespace ScoutSemantic;

/// <summary>One compiler-verified symbol fact: a named type (<c>Member</c>
/// null) or one of its ordinary methods/constructors, with complete
/// identity and its declaration span.</summary>
internal sealed record CompilerSymbolFact(
    string Assembly,
    string Type,
    string? Member,
    int GenericArity,
    string OverloadSignature,
    string File,
    int Line);

/// <summary>One compiler diagnostic attributed to a source location.</summary>
internal sealed record CompilerDiagnosticFact(
    string Severity,
    string Code,
    string Message,
    string File,
    int Line);

/// <summary>One unit that loaded but reports at least one error diagnostic,
/// with the compiler's own first-error reason.</summary>
internal sealed record CompilerIncompleteUnit(string Unit, string Reason);

/// <summary>
/// Accumulates compiler-facts data across every loaded project, gathered
/// from the same <see cref="Compilation"/> the run's main loop already
/// fetched -- never a second pass after the workspace is disposed.
/// </summary>
internal sealed class CompilerFactsAccumulator
{
    public List<string> Processed { get; } = new();

    public List<string> Missing { get; } = new();

    public List<CompilerIncompleteUnit> Incomplete { get; } = new();

    public List<CompilerSymbolFact> Symbols { get; } = new();

    public List<CompilerDiagnosticFact> Diagnostics { get; } = new();

    /// <summary>Records one loaded, compiled unit: its diagnostics and every
    /// named type and ordinary member it declares. <paramref name="unrestoredReason"/>,
    /// when set, names an offline-restore failure that is this
    /// unit's own root cause; it replaces the compiler's first-error reason in
    /// <see cref="Incomplete"/> rather than competing with it, and is reported even
    /// when the compilation carries no error diagnostic of its own.</summary>
    public void CollectFromCompilation(Compilation compilation, string unitId, RepoPaths paths, string? unrestoredReason = null)
    {
        Processed.Add(unitId);

        Diagnostic? firstError = null;
        foreach (var diagnostic in compilation.GetDiagnostics())
        {
            if (diagnostic.Severity is not (DiagnosticSeverity.Error or DiagnosticSeverity.Warning))
            {
                continue;
            }

            var (file, line) = LocationOf(diagnostic.Location, paths);
            Diagnostics.Add(new CompilerDiagnosticFact(
                diagnostic.Severity == DiagnosticSeverity.Error ? "error" : "warning",
                diagnostic.Id,
                diagnostic.GetMessage(),
                file,
                line));
            firstError ??= diagnostic.Severity == DiagnosticSeverity.Error ? diagnostic : null;
        }

        if (unrestoredReason is not null)
        {
            Incomplete.Add(new CompilerIncompleteUnit(unitId, unrestoredReason));
        }
        else if (firstError is { } error)
        {
            Incomplete.Add(new CompilerIncompleteUnit(unitId, $"{error.Id}: {error.GetMessage()}"));
        }

        var assemblyName = compilation.AssemblyName ?? unitId;
        WalkNamespace(compilation.Assembly.GlobalNamespace, assemblyName, paths);
    }

    private void WalkNamespace(INamespaceSymbol ns, string assemblyName, RepoPaths paths)
    {
        foreach (var type in ns.GetTypeMembers())
        {
            WalkType(type, assemblyName, paths);
        }

        foreach (var child in ns.GetNamespaceMembers())
        {
            WalkNamespace(child, assemblyName, paths);
        }
    }

    /// <summary>The declared-symbol facts' own type-identity encoding:
    /// <see cref="SymbolDisplayFormat.FullyQualifiedFormat"/> with the
    /// <c>global::</c> prefix stripped, so a nested type reads
    /// <c>Outer.Inner</c> and a generic type keeps its type parameters
    /// (<c>Outer.Inner&lt;T&gt;</c>). Occurrence facts share this exact
    /// encoding for <c>caller</c>/<c>target</c>/<c>candidates[].type</c> so a
    /// nested or generic type reads identically everywhere in one artifact --
    /// see the artifact's own <c>occurrences.identityEncoding</c> literal.
    /// Distinct from <see cref="Ids.NamedTypeId"/>, devscout's own graph
    /// def-id convention (<c>Outer+Inner</c>, arity dropped), which this
    /// document does not use for any type identity.</summary>
    internal static string SymbolTypeId(ITypeSymbol type) => type
        .ToDisplayString(SymbolDisplayFormat.FullyQualifiedFormat)
        .Replace("global::", "", StringComparison.Ordinal);

    private void WalkType(INamedTypeSymbol type, string assemblyName, RepoPaths paths)
    {
        if (type.IsImplicitlyDeclared)
        {
            return;
        }

        var typeName = SymbolTypeId(type);
        var (typeFile, typeLine) = LocationOf(type.Locations.FirstOrDefault(l => l.IsInSource), paths);
        if (typeFile.Length > 0)
        {
            Symbols.Add(new CompilerSymbolFact(assemblyName, typeName, null, type.Arity, "", typeFile, typeLine));
        }

        foreach (var member in type.GetMembers().OfType<IMethodSymbol>())
        {
            if (member.IsImplicitlyDeclared || member.MethodKind is not (MethodKind.Ordinary or MethodKind.Constructor))
            {
                continue;
            }

            var (file, line) = LocationOf(member.Locations.FirstOrDefault(l => l.IsInSource), paths);
            if (file.Length == 0)
            {
                continue;
            }

            var parameters = string.Join(",", member.Parameters.Select(
                p => p.Type.ToDisplayString(SymbolDisplayFormat.MinimallyQualifiedFormat)));
            var returnType = member.ReturnsVoid
                ? "void"
                : member.ReturnType.ToDisplayString(SymbolDisplayFormat.MinimallyQualifiedFormat);
            var signature = $"({parameters})->{returnType}";
            Symbols.Add(new CompilerSymbolFact(
                assemblyName, typeName, member.Name, member.Arity, signature, file, line));
        }

        foreach (var nested in type.GetTypeMembers())
        {
            WalkType(nested, assemblyName, paths);
        }
    }

    private static (string File, int Line) LocationOf(Location? location, RepoPaths paths)
    {
        if (location is null || !location.IsInSource)
        {
            return ("", 0);
        }

        var span = location.GetLineSpan();
        return (paths.Relative(span.Path) ?? "", span.StartLinePosition.Line + 1);
    }
}

/// <summary>
/// Renders and writes the compiler-facts protocol document: fixed
/// identity/negotiation header fields plus the accumulated symbol and
/// diagnostic facts, as one validated JSON object written before any
/// Rust-side admission check runs. Independent of the existing
/// oracle/flowtrace-facts walkers, which read syntax plus a semantic model
/// for resolver-facing facts; this mode reports compiler-verified symbol
/// identity and diagnostics directly from Roslyn symbols.
/// </summary>
internal static class CompilerFactsEmitter
{
    /// <summary>The wire-contract format this mode writes.</summary>
    public const string ContractFormat = "compiler-facts";

    /// <summary>The wire-contract version this mode writes.</summary>
    public const int ContractVersion = 1;

    /// <summary>This document's own schema version.</summary>
    public const int ArtifactSchemaVersion = 2;

    /// <summary>
    /// This engine build's protocol revision. Bump together with a
    /// corresponding change to the Rust admission path's own compiled-in
    /// expectation. "2" means this engine can emit occurrence facts (Slice
    /// A); an artifact from a "1" engine unambiguously never carries them.
    /// </summary>
    public const string EngineRevision = "2";

    /// <summary>
    /// The sha256 digest of this project's own <c>packages.lock.json</c>.
    /// Kept as a literal, matching the Rust admission path's own compiled-in
    /// constant; recompute and update both when the lock file changes.
    /// </summary>
    public const string DependencyFingerprint =
        "1b08b298ead60b49666b3bfa8d389386770d87dc150a9b1eced58896652f3d43";

    /// <summary>
    /// The compilation-context envelope version this mode embeds. Names
    /// <see cref="ContextEnvelope.SchemaVersion"/>, embedded here as the real
    /// per-compilation envelope -- unaffected by this document's own
    /// <see cref="ArtifactSchemaVersion"/>, which is a different, higher-level
    /// schema.
    /// </summary>
    public const int ContextSchemaVersion = 1;

    private static readonly string[] SupportedCapabilities = { "symbols", "diagnostics", "occurrences" };

    private static readonly JsonWriterOptions Pretty = new()
    {
        Indented = true,
        NewLine = "\n",
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
    };

    /// <summary>The requested/provided capability lists for this run, resolved
    /// once from <paramref name="options"/> against <see cref="SupportedCapabilities"/>.
    /// Computed before the per-project walk loop (not only inside <see cref="Render"/>)
    /// so a caller can decide whether to run the occurrence walker at all.</summary>
    public static (List<string> Requested, List<string> Provided) ResolveCapabilities(Options options)
    {
        var requested = options.Capabilities.Count > 0 ? options.Capabilities : SupportedCapabilities.ToList();
        var provided = requested.Where(c => SupportedCapabilities.Contains(c, StringComparer.Ordinal)).ToList();
        return (requested, provided);
    }

    /// <summary>Renders <paramref name="acc"/> and writes it to
    /// <c>options.CompilerFacts</c> (or stdout for <c>-</c>). Returns a
    /// non-zero exit code on an I/O failure; never writes a partial
    /// document -- the whole byte buffer is built in memory first.
    /// <paramref name="orderedContextRecords"/> is this run's real context
    /// envelope, already in the same order <c>--emit context</c>
    /// itself would write; <paramref name="occurrences"/> is null exactly
    /// when the <c>occurrences</c> capability was not provided.</summary>
    public static int Write(
        Options options,
        CompilerFactsAccumulator acc,
        List<ContextRecord> orderedContextRecords,
        List<CompilerOccurrenceFact>? occurrences)
    {
        var path = options.CompilerFacts;
        if (string.IsNullOrEmpty(path))
        {
            Console.Error.WriteLine("error: --compiler-facts is required for --emit compiler-facts");
            return 1;
        }

        var git = options.NoGit ? null : FactsWriter.Probe(options.Root);
        var bytes = Render(options, acc, git, orderedContextRecords, occurrences);

        try
        {
            if (path == "-")
            {
                using var stdout = Console.OpenStandardOutput();
                stdout.Write(bytes, 0, bytes.Length);
                stdout.Flush();
            }
            else
            {
                var directory = Path.GetDirectoryName(Path.GetFullPath(path));
                if (!string.IsNullOrEmpty(directory))
                {
                    Directory.CreateDirectory(directory);
                }

                File.WriteAllBytes(path, bytes);
            }
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            Console.Error.WriteLine($"error: {e.Message}");
            return 1;
        }

        Console.Error.WriteLine(
            $"compiler-facts: {acc.Symbols.Count} symbols, {acc.Diagnostics.Count} diagnostics, "
            + $"{acc.Incomplete.Count} incomplete unit(s) -> {path}");
        return 0;
    }

    /// <summary>Internal rather than private so <c>scout-semantic.Tests</c> can exercise the
    /// <c>sourceSnapshot</c> shape directly, via <c>InternalsVisibleTo</c>, against a
    /// hand-built <see cref="GitIdentity"/> rather than a real git checkout.</summary>
    internal static byte[] Render(
        Options options,
        CompilerFactsAccumulator acc,
        GitIdentity? git,
        List<ContextRecord> orderedContextRecords,
        List<CompilerOccurrenceFact>? occurrences)
    {
        var symbols = acc.Symbols
            .OrderBy(s => s.File, StringComparer.Ordinal)
            .ThenBy(s => s.Line)
            .ThenBy(s => s.Type, StringComparer.Ordinal)
            .ThenBy(s => s.Member, StringComparer.Ordinal)
            .ThenBy(s => s.OverloadSignature, StringComparer.Ordinal)
            .ToList();
        var diagnostics = acc.Diagnostics
            .OrderBy(d => d.File, StringComparer.Ordinal)
            .ThenBy(d => d.Line)
            .ThenBy(d => d.Code, StringComparer.Ordinal)
            .ToList();
        var processed = acc.Processed.OrderBy(u => u, StringComparer.Ordinal).ToList();
        var missing = acc.Missing.OrderBy(u => u, StringComparer.Ordinal).ToList();
        var incomplete = acc.Incomplete.OrderBy(u => u.Unit, StringComparer.Ordinal).ToList();

        var (requested, provided) = ResolveCapabilities(options);

        using var buffer = new MemoryStream();
        using (var writer = new Utf8JsonWriter(buffer, Pretty))
        {
            writer.WriteStartObject();
            writer.WriteString("format", ContractFormat);
            writer.WriteNumber("contractVersion", ContractVersion);
            writer.WriteNumber("artifactSchemaVersion", ArtifactSchemaVersion);

            writer.WriteStartObject("producer");
            writer.WriteString("name", "scout-semantic");
            writer.WriteString("engineRevision", EngineRevision);
            writer.WriteEndObject();

            writer.WriteStartObject("profile");
            writer.WriteString("target", options.Tfms.Count > 0 ? options.Tfms[0] : "");
            writer.WriteString("configuration", options.Properties.GetValueOrDefault("Configuration", ""));
            writer.WriteString("platform", options.Properties.GetValueOrDefault("Platform", ""));
            writer.WriteEndObject();

            writer.WriteString("dependencyFingerprint", DependencyFingerprint);

            writer.WriteStartObject("context");
            writer.WriteNumber("schemaVersion", ContextSchemaVersion);
            writer.WriteString("contextFingerprint", DerivedContextSummary.Compute(orderedContextRecords));
            writer.WriteStartObject("envelope");
            writer.WriteStartArray("compilations");
            foreach (var record in orderedContextRecords)
            {
                ContextWriter.WriteRecord(writer, record);
            }

            writer.WriteEndArray();
            writer.WriteEndObject();
            writer.WriteEndObject();

            if (git is not null)
            {
                // Two freshness legs, per the Design's own "dirty state" decision: `headSha`
                // alone is blind to an uncommitted edit, so `dirty`/`dirtyDigest` adopt the
                // exact convention `FactsWriter.Render` already writes for the flow-tracer
                // document -- the same `GitIdentity`, the same fields, no second scheme.
                writer.WriteStartObject("sourceSnapshot");
                writer.WriteString("headSha", git.HeadSha);
                writer.WriteBoolean("dirty", git.Dirty);
                writer.WriteString("dirtyDigest", git.DirtyDigest);
                writer.WriteEndObject();
            }

            writer.WriteStartObject("capabilities");
            writer.WriteStartArray("requested");
            foreach (var c in requested)
            {
                writer.WriteStringValue(c);
            }

            writer.WriteEndArray();
            writer.WriteStartArray("provided");
            foreach (var c in provided)
            {
                writer.WriteStringValue(c);
            }

            writer.WriteEndArray();
            writer.WriteEndObject();

            writer.WriteStartObject("completion");
            writer.WriteBoolean("terminal", true);
            writer.WriteEndObject();

            writer.WriteStartObject("units");
            writer.WriteStartArray("processed");
            foreach (var u in processed)
            {
                writer.WriteStringValue(u);
            }

            writer.WriteEndArray();
            writer.WriteStartArray("missing");
            foreach (var u in missing)
            {
                writer.WriteStringValue(u);
            }

            writer.WriteEndArray();
            writer.WriteEndObject();

            writer.WriteStartObject("coverage");
            if (incomplete.Count == 0)
            {
                writer.WriteString("state", "complete");
            }
            else
            {
                writer.WriteString("state", "incomplete");
                writer.WriteStartArray("incompleteUnits");
                foreach (var unit in incomplete)
                {
                    writer.WriteStartObject();
                    writer.WriteString("unit", unit.Unit);
                    writer.WriteString("reason", unit.Reason);
                    writer.WriteEndObject();
                }

                writer.WriteEndArray();
            }

            writer.WriteEndObject();

            writer.WriteStartArray("diagnostics");
            foreach (var d in diagnostics)
            {
                writer.WriteStartObject();
                writer.WriteString("severity", d.Severity);
                writer.WriteString("code", d.Code);
                writer.WriteString("message", d.Message);
                writer.WriteString("file", d.File);
                writer.WriteNumber("line", d.Line);
                writer.WriteEndObject();
            }

            writer.WriteEndArray();

            writer.WriteStartArray("symbols");
            foreach (var s in symbols)
            {
                writer.WriteStartObject();
                writer.WriteString("assembly", s.Assembly);
                writer.WriteString("type", s.Type);
                if (s.Member is not null)
                {
                    writer.WriteString("member", s.Member);
                }

                writer.WriteNumber("genericArity", s.GenericArity);
                writer.WriteString("overloadSignature", s.OverloadSignature);
                writer.WriteString("file", s.File);
                writer.WriteNumber("line", s.Line);
                writer.WriteString("spanEncoding", "utf16-code-unit");
                writer.WriteEndObject();
            }

            writer.WriteEndArray();

            if (occurrences is not null)
            {
                writer.WriteStartObject("occurrences");
                writer.WriteString("spanEncoding", "utf16-code-unit-line1-char0-end-exclusive");
                writer.WriteString("identityEncoding", "fully-qualified-display-format");
                writer.WriteStartArray("sites");
                foreach (var o in occurrences)
                {
                    WriteOccurrence(writer, o);
                }

                writer.WriteEndArray();
                writer.WriteEndObject();
            }

            writer.WriteEndObject();
        }

        buffer.WriteByte((byte)'\n');
        return buffer.ToArray();
    }

    private static void WriteOccurrence(Utf8JsonWriter writer, CompilerOccurrenceFact o)
    {
        writer.WriteStartObject();
        writer.WriteString("file", o.File);
        writer.WriteString("shape", o.Shape);

        writer.WriteStartObject("span");
        writer.WriteNumber("startLine", o.Span.StartLine);
        writer.WriteNumber("startChar", o.Span.StartChar);
        writer.WriteNumber("endLine", o.Span.EndLine);
        writer.WriteNumber("endChar", o.Span.EndChar);
        writer.WriteEndObject();

        writer.WriteStartObject("name");
        writer.WriteNumber("line", o.Name.Line);
        writer.WriteNumber("char", o.Name.Char);
        writer.WriteEndObject();

        writer.WritePropertyName("caller");
        WriteOccurrenceIdentity(writer, o.Caller);

        writer.WriteString("resolution", o.Resolution);
        writer.WriteString("candidateReason", o.CandidateReason);

        if (o.Target is { } target)
        {
            writer.WritePropertyName("target");
            WriteOccurrenceIdentity(writer, target);
        }
        else
        {
            writer.WriteNull("target");
        }

        writer.WriteStartArray("candidates");
        foreach (var candidate in o.Candidates)
        {
            WriteOccurrenceIdentity(writer, candidate);
        }

        writer.WriteEndArray();

        writer.WriteStartObject("compilation");
        if (o.CompilationIdentity is { } identity)
        {
            writer.WritePropertyName("identity");
            ContextWriter.WriteIdentity(writer, identity);
        }
        else
        {
            writer.WriteNull("identity");
        }

        if (o.CompilationFingerprint is { } fingerprint)
        {
            writer.WriteString("fingerprint", fingerprint);
        }
        else
        {
            writer.WriteNull("fingerprint");
        }

        writer.WriteEndObject();

        writer.WriteString("documentContentIdentity", o.DocumentContentIdentity);
        writer.WriteStartArray("targetDocumentContentIdentities");
        foreach (var docId in o.TargetDocumentContentIdentities)
        {
            writer.WriteStringValue(docId);
        }

        writer.WriteEndArray();
        writer.WriteEndObject();
    }

    private static void WriteOccurrenceIdentity(Utf8JsonWriter writer, OccurrenceIdentity identity)
    {
        writer.WriteStartObject();
        writer.WriteString("assembly", identity.Assembly);
        writer.WriteString("type", identity.Type);
        if (identity.Member is not null)
        {
            writer.WriteString("member", identity.Member);
        }
        else
        {
            writer.WriteNull("member");
        }

        writer.WriteNumber("genericArity", identity.GenericArity);
        writer.WriteString("overloadSignature", identity.OverloadSignature);
        writer.WriteEndObject();
    }
}
