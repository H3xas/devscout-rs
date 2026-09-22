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
    /// named type and ordinary member it declares.</summary>
    public void CollectFromCompilation(Compilation compilation, string unitId, RepoPaths paths)
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

        if (firstError is { } error)
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

    private void WalkType(INamedTypeSymbol type, string assemblyName, RepoPaths paths)
    {
        if (type.IsImplicitlyDeclared)
        {
            return;
        }

        var typeName = type.ToDisplayString(SymbolDisplayFormat.FullyQualifiedFormat)
            .Replace("global::", "", StringComparison.Ordinal);
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
    public const int ArtifactSchemaVersion = 1;

    /// <summary>
    /// This engine build's protocol revision. Bump together with a
    /// corresponding change to the Rust admission path's own compiled-in
    /// expectation.
    /// </summary>
    public const string EngineRevision = "1";

    /// <summary>
    /// The sha256 digest of this project's own <c>packages.lock.json</c>.
    /// Kept as a literal, matching the Rust admission path's own compiled-in
    /// constant; recompute and update both when the lock file changes.
    /// </summary>
    public const string DependencyFingerprint =
        "f0e2aa25d0071aab4aa9de47f3a7629b783a5f17bf625b565f073b48e69d0c83";

    /// <summary>
    /// The compilation-context envelope version this mode embeds. A frozen
    /// placeholder envelope body stands in until a real per-compilation
    /// context producer exists; the Rust admission path checks only this
    /// version literal and never parses the envelope's internal shape, so
    /// the placeholder body can be replaced without a protocol change.
    /// </summary>
    public const int ContextSchemaVersion = 1;

    private const string ContextFingerprint =
        "1e2d3c4b5a69788796a5b4c3d2e1f0a1b2c3d4e5f60718293a4b5c6d7e8f901";

    private static readonly string[] SupportedCapabilities = { "symbols", "diagnostics" };

    private static readonly JsonWriterOptions Pretty = new()
    {
        Indented = true,
        NewLine = "\n",
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
    };

    /// <summary>Renders <paramref name="acc"/> and writes it to
    /// <c>options.CompilerFacts</c> (or stdout for <c>-</c>). Returns a
    /// non-zero exit code on an I/O failure; never writes a partial
    /// document -- the whole byte buffer is built in memory first.</summary>
    public static int Write(Options options, CompilerFactsAccumulator acc)
    {
        var path = options.CompilerFacts;
        if (string.IsNullOrEmpty(path))
        {
            Console.Error.WriteLine("error: --compiler-facts is required for --emit compiler-facts");
            return 1;
        }

        var headSha = options.NoGit ? null : FactsWriter.Probe(options.Root)?.HeadSha;
        var bytes = Render(options, acc, headSha);

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

    private static byte[] Render(Options options, CompilerFactsAccumulator acc, string? headSha)
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

        var requested = options.Capabilities.Count > 0 ? options.Capabilities : SupportedCapabilities.ToList();
        var provided = requested.Where(c => SupportedCapabilities.Contains(c, StringComparer.Ordinal)).ToList();

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
            writer.WriteString("contextFingerprint", ContextFingerprint);
            writer.WriteStartObject("envelope");
            writer.WriteString("fingerprint", ContextFingerprint);
            writer.WriteString("state", incomplete.Count == 0 ? "complete" : "partial");
            writer.WriteEndObject();
            writer.WriteEndObject();

            if (headSha is not null)
            {
                writer.WriteStartObject("sourceSnapshot");
                writer.WriteString("headSha", headSha);
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
            writer.WriteEndObject();
        }

        buffer.WriteByte((byte)'\n');
        return buffer.ToArray();
    }
}
