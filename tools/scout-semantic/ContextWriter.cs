using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace ScoutSemantic;

/// <summary>
/// Serialises the build-context envelope: one JSON object, header keys first
/// and <c>compilations</c> last, indented with two spaces, UTF-8 without BOM,
/// LF newlines, trailing newline -- the same house style <see cref="FactsWriter"/>
/// already uses for the sibling fact document.
/// </summary>
internal static class ContextWriter
{
    private static readonly JsonWriterOptions Pretty = new()
    {
        Indented = true,
        NewLine = "\n",
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
    };

    /// <summary>
    /// Writes the envelope to <paramref name="path"/>, or to stdout when the
    /// path is <c>-</c>. Every record is validated first, so a schema
    /// violation leaves no partial file behind. Parent directories are
    /// created.
    /// </summary>
    public static void Write(string path, ContextEnvelope envelope)
    {
        var bytes = Render(envelope);
        if (path == "-")
        {
            using var stdout = Console.OpenStandardOutput();
            stdout.Write(bytes, 0, bytes.Length);
            stdout.Flush();
            return;
        }

        var directory = Path.GetDirectoryName(Path.GetFullPath(path));
        if (!string.IsNullOrEmpty(directory))
        {
            Directory.CreateDirectory(directory);
        }

        File.WriteAllBytes(path, bytes);
    }

    private static byte[] Render(ContextEnvelope envelope)
    {
        foreach (var record in envelope.Compilations)
        {
            ContextSchema.Validate(record);
        }

        using var buffer = new MemoryStream();
        using (var writer = new Utf8JsonWriter(buffer, Pretty))
        {
            writer.WriteStartObject();
            writer.WriteNumber("schemaVersion", envelope.SchemaVersion);
            writer.WriteString("producer", envelope.Producer);
            writer.WriteString("version", envelope.Version);
            writer.WriteString("repo", envelope.Repo);
            writer.WriteString("solution", envelope.Solution);

            writer.WriteStartArray("compilations");
            foreach (var record in envelope.Compilations)
            {
                WriteRecord(writer, record);
            }

            writer.WriteEndArray();
            writer.WriteEndObject();
        }

        buffer.WriteByte((byte)'\n');
        return buffer.ToArray();
    }

    /// <summary>Writes one compilation identity object: project path/name,
    /// requested/effective target, configuration, platform, declared
    /// targets. Exposed so <see cref="CompilerFactsEmitter"/> (the embedded
    /// per-occurrence <c>compilation.identity</c> value) and
    /// <see cref="DerivedContextSummary"/> (the canonicalization input) write
    /// or fold the exact same shape a compiled envelope's own
    /// <c>compilations[i].identity</c> carries, never a second, drifting
    /// definition of the same fields.</summary>
    internal static void WriteIdentity(Utf8JsonWriter writer, ContextIdentity identity)
    {
        writer.WriteStartObject();
        writer.WriteString("projectPath", identity.ProjectPath);
        writer.WriteString("projectName", identity.ProjectName);
        WriteNullableString(writer, "requestedTfm", identity.RequestedTfm);
        WriteNullableString(writer, "effectiveTfm", identity.EffectiveTfm);
        WriteNullableString(writer, "configuration", identity.Configuration);
        WriteNullableString(writer, "platform", identity.Platform);
        if (identity.DeclaredTfms is { } declared)
        {
            writer.WriteStartArray("declaredTfms");
            foreach (var tfm in declared)
            {
                writer.WriteStringValue(tfm);
            }

            writer.WriteEndArray();
        }
        else
        {
            writer.WriteNull("declaredTfms");
        }

        writer.WriteEndObject();
    }

    /// <summary>Exposed (not private) so <see cref="CompilerFactsEmitter"/> can
    /// embed a compilation record byte-for-byte identically under a
    /// different top-level key, without a second serialization to keep in
    /// sync by hand.</summary>
    internal static void WriteRecord(Utf8JsonWriter writer, ContextRecord record)
    {
        writer.WriteStartObject();

        writer.WritePropertyName("identity");
        WriteIdentity(writer, record.Identity);

        writer.WriteString("state", record.State);
        writer.WriteString("reason", record.Reason);
        WriteNullableString(writer, "fingerprint", record.Fingerprint);

        if (record.Versions is { } versions)
        {
            writer.WriteStartObject("versions");
            writer.WriteString("sdk", versions.Sdk);
            writer.WriteString("msbuild", versions.Msbuild);
            writer.WriteString("compiler", versions.Compiler);
            writer.WriteString("engine", versions.Engine);
            writer.WriteEndObject();
        }
        else
        {
            // Every other optional field on a record is explicitly nulled
            // rather than left out, so a consumer can always distinguish
            // "known absent" from "this build of the tool never wrote the
            // key at all" -- `versions` was the one exception.
            writer.WriteNull("versions");
        }

        writer.WriteStartArray("references");
        foreach (var reference in record.References)
        {
            writer.WriteStartObject();
            writer.WriteString("kind", reference.Kind);
            writer.WriteString("name", reference.Name);
            WriteNullableString(writer, "identity", reference.Identity);
            WriteNullableString(writer, "fingerprint", reference.Fingerprint);
            writer.WriteEndObject();
        }

        writer.WriteEndArray();

        writer.WriteStartArray("imports");
        foreach (var import in record.Imports)
        {
            writer.WriteStartObject();
            writer.WriteString("identity", import.Identity);
            writer.WriteString("hash", import.Hash);
            writer.WriteEndObject();
        }

        writer.WriteEndArray();

        writer.WriteStartObject("languageOptions");
        foreach (var (key, value) in record.LanguageOptions.OrderBy(kv => kv.Key, StringComparer.Ordinal))
        {
            WriteNullableString(writer, key, value);
        }

        writer.WriteEndObject();

        writer.WriteStartArray("preprocessorSymbols");
        foreach (var symbol in record.PreprocessorSymbols)
        {
            writer.WriteStringValue(symbol);
        }

        writer.WriteEndArray();

        writer.WriteStartObject("documents");
        writer.WriteStartArray("expected");
        foreach (var doc in record.Documents.Expected)
        {
            writer.WriteStringValue(doc);
        }

        writer.WriteEndArray();
        writer.WriteStartArray("loaded");
        foreach (var doc in record.Documents.Loaded)
        {
            writer.WriteStringValue(doc);
        }

        writer.WriteEndArray();
        writer.WriteStartArray("dropped");
        foreach (var dropped in record.Documents.Dropped)
        {
            writer.WriteStartObject();
            writer.WriteString("path", dropped.Path);
            writer.WriteString("reason", dropped.Reason);
            writer.WriteEndObject();
        }

        writer.WriteEndArray();
        writer.WriteBoolean("inventoryAvailable", record.Documents.InventoryAvailable);
        writer.WriteEndObject();

        writer.WriteStartObject("generated");
        writer.WriteStartArray("documents");
        foreach (var generated in record.Generated.Documents)
        {
            writer.WriteStartObject();
            writer.WriteString("hintName", generated.HintName);
            WriteNullableString(writer, "generator", generated.Generator);
            writer.WriteEndObject();
        }

        writer.WriteEndArray();
        writer.WriteStartArray("diagnostics");
        foreach (var diagnostic in record.Generated.Diagnostics)
        {
            writer.WriteStringValue(diagnostic);
        }

        writer.WriteEndArray();
        writer.WriteEndObject();

        writer.WriteStartObject("diagnostics");
        writer.WriteStartArray("workspace");
        foreach (var workspace in record.Diagnostics.Workspace)
        {
            writer.WriteStartObject();
            writer.WriteString("kind", workspace.Kind);
            writer.WriteString("message", workspace.Message);
            writer.WriteEndObject();
        }

        writer.WriteEndArray();
        writer.WriteStartArray("compiler");
        foreach (var compiler in record.Diagnostics.Compiler)
        {
            writer.WriteStartObject();
            writer.WriteString("severity", compiler.Severity);
            writer.WriteString("id", compiler.Id);
            writer.WriteString("message", compiler.Message);
            WriteNullableString(writer, "file", compiler.File);
            if (compiler.Line is { } line)
            {
                writer.WriteNumber("line", line);
            }
            else
            {
                writer.WriteNull("line");
            }

            writer.WriteEndObject();
        }

        writer.WriteEndArray();
        writer.WriteEndObject();

        writer.WriteEndObject();
    }

    private static void WriteNullableString(Utf8JsonWriter writer, string key, string? value)
    {
        if (value is null)
        {
            writer.WriteNull(key);
        }
        else
        {
            writer.WriteString(key, value);
        }
    }
}
