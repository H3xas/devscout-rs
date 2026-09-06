using System.ComponentModel;
using System.Diagnostics;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace ScoutSemantic;

/// <summary>Git identity of the analysed working tree, stamped into the fact header.</summary>
internal sealed class GitIdentity
{
    /// <summary>Commit the tree is checked out at.</summary>
    public required string HeadSha { get; init; }

    /// <summary>True when <c>git status --porcelain</c> reported at least one line.</summary>
    public required bool Dirty { get; init; }

    /// <summary>SHA-1 of the ordinal-sorted porcelain lines joined with newlines.</summary>
    public required string DirtyDigest { get; init; }

    /// <summary>Number of tracked files.</summary>
    public required int FileCount { get; init; }
}

/// <summary>Everything the fact document carries before the <c>facts</c> array.</summary>
internal sealed class FactsHeader
{
    /// <summary>Informational version of this tool.</summary>
    public required string Version { get; init; }

    /// <summary>Repository id the consumer keys the fact set by.</summary>
    public required string Repo { get; init; }

    /// <summary>Root-relative, forward-slashed path of the analysed solution or project.</summary>
    public required string Solution { get; init; }

    /// <summary>One <c>Name|tfm</c> entry per loaded project, ordinal-sorted.</summary>
    public required List<string> Units { get; init; }

    /// <summary>Git identity, or null when it is not stamped.</summary>
    public GitIdentity? Git { get; init; }
}

/// <summary>
/// Serialises the fact document: one JSON object, header keys first and the
/// <c>facts</c> array last, indented with two spaces, UTF-8 without BOM, LF
/// newlines and a trailing newline.
/// </summary>
internal static class FactsWriter
{
    private const string Producer = "scout-semantic";

    private static readonly JsonWriterOptions Pretty = new()
    {
        Indented = true,
        NewLine = "\n",
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
    };

    private static readonly JsonWriterOptions Compact = new()
    {
        Indented = false,
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
    };

    /// <summary>Informational version of the entry assembly, e.g. <c>0.1.0</c>.</summary>
    public static string ProducerVersion()
    {
        var assembly = Assembly.GetEntryAssembly();
        var informational = assembly?.GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion;
        if (!string.IsNullOrEmpty(informational))
        {
            return informational;
        }

        return assembly?.GetName().Version?.ToString() ?? "0.0.0";
    }

    /// <summary>Lower-case hex SHA-1 of the UTF-8 bytes of <paramref name="text"/>.</summary>
    public static string Sha1(string text) =>
        Convert.ToHexString(SHA1.HashData(Encoding.UTF8.GetBytes(text))).ToLowerInvariant();

    /// <summary>
    /// Sorts by (file, line, type, canonical single-line JSON) and drops exact
    /// duplicates, which a source file linked into two projects would otherwise
    /// contribute twice.
    /// </summary>
    public static List<FactRecord> Order(IEnumerable<FactRecord> facts)
    {
        var keyed = facts.Select(f => (Fact: f, Json: Canonical(f))).ToList();
        keyed.Sort((a, b) =>
        {
            var c = string.CompareOrdinal(a.Fact.File, b.Fact.File);
            if (c != 0)
            {
                return c;
            }

            c = a.Fact.Line.CompareTo(b.Fact.Line);
            if (c != 0)
            {
                return c;
            }

            c = string.CompareOrdinal(a.Fact.Type, b.Fact.Type);
            return c != 0 ? c : string.CompareOrdinal(a.Json, b.Json);
        });

        var kept = new List<FactRecord>(keyed.Count);
        string? previous = null;
        foreach (var (fact, json) in keyed)
        {
            if (previous is not null && string.Equals(previous, json, StringComparison.Ordinal))
            {
                continue;
            }

            kept.Add(fact);
            previous = json;
        }

        return kept;
    }

    /// <summary>
    /// Writes the document to <paramref name="path"/>, or to stdout when the path
    /// is <c>-</c>. Every fact is validated first, so a schema violation leaves no
    /// partial file behind. Parent directories are created.
    /// </summary>
    public static void Write(string path, FactsHeader header, IReadOnlyList<FactRecord> facts)
    {
        var bytes = Render(header, facts);
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

    /// <summary>
    /// Reads the git identity of <paramref name="root"/> the way the flow tracer
    /// does. Returns null when git is not on PATH or the tree has no HEAD.
    /// </summary>
    public static GitIdentity? Probe(string root)
    {
        var head = RunGit(root, "rev-parse HEAD")?.Trim();
        if (string.IsNullOrEmpty(head))
        {
            return null;
        }

        var changed = NonEmptyLines(RunGit(root, "status --porcelain"));
        changed.Sort(StringComparer.Ordinal);
        return new GitIdentity
        {
            HeadSha = head,
            Dirty = changed.Count > 0,
            DirtyDigest = Sha1(string.Join('\n', changed)),
            FileCount = NonEmptyLines(RunGit(root, "ls-files")).Count,
        };
    }

    private static byte[] Render(FactsHeader header, IReadOnlyList<FactRecord> facts)
    {
        foreach (var fact in facts)
        {
            FactSchema.Validate(fact);
        }

        using var buffer = new MemoryStream();
        using (var writer = new Utf8JsonWriter(buffer, Pretty))
        {
            writer.WriteStartObject();
            writer.WriteNumber("schemaVersion", 1);
            writer.WriteString("producer", Producer);
            writer.WriteString("version", header.Version);
            writer.WriteString("repo", header.Repo);
            writer.WriteString("kind", "backend");
            writer.WriteString("generatedFrom", Producer + " " + header.Version);

            writer.WriteStartObject("compilation");
            writer.WriteString("solution", header.Solution);
            writer.WriteStartArray("units");
            foreach (var unit in header.Units)
            {
                writer.WriteStringValue(unit);
            }

            writer.WriteEndArray();
            writer.WriteString("digest", Sha1(string.Join('\n', header.Units)));
            writer.WriteEndObject();

            if (header.Git is { } git)
            {
                writer.WriteString("headSha", git.HeadSha);
                writer.WriteBoolean("dirty", git.Dirty);
                writer.WriteString("dirtyDigest", git.DirtyDigest);
                writer.WriteNumber("fileCount", git.FileCount);
            }

            writer.WriteStartArray("facts");
            foreach (var fact in facts)
            {
                WriteFact(writer, fact);
            }

            writer.WriteEndArray();
            writer.WriteEndObject();
        }

        buffer.WriteByte((byte)'\n');
        return buffer.ToArray();
    }

    private static string Canonical(FactRecord fact)
    {
        using var buffer = new MemoryStream();
        using (var writer = new Utf8JsonWriter(buffer, Compact))
        {
            WriteFact(writer, fact);
        }

        return Encoding.UTF8.GetString(buffer.ToArray());
    }

    private static void WriteFact(Utf8JsonWriter writer, FactRecord fact)
    {
        writer.WriteStartObject();
        foreach (var (key, value) in fact.Fields)
        {
            switch (value)
            {
                case string text:
                    writer.WriteString(key, text);
                    break;
                case int number:
                    writer.WriteNumber(key, number);
                    break;
                default:
                    throw new FactSchemaException(
                        $"{fact.Type} fact field '{key}' holds an unsupported value");
            }
        }

        writer.WriteEndObject();
    }

    private static List<string> NonEmptyLines(string? output)
    {
        var lines = new List<string>();
        if (output is null)
        {
            return lines;
        }

        foreach (var raw in output.Split('\n'))
        {
            var line = raw.TrimEnd('\r');
            if (line.Trim().Length > 0)
            {
                lines.Add(line);
            }
        }

        return lines;
    }

    private static string? RunGit(string root, string arguments)
    {
        try
        {
            using var process = Process.Start(new ProcessStartInfo("git", arguments)
            {
                WorkingDirectory = root,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
            });

            if (process is null)
            {
                return null;
            }

            // stderr is drained on its own thread: reading the two pipes one
            // after the other deadlocks as soon as git writes more diagnostics
            // than the stderr buffer holds while this thread still blocks on
            // stdout.
            process.ErrorDataReceived += static (_, _) => { };
            process.BeginErrorReadLine();
            var output = process.StandardOutput.ReadToEnd();
            process.WaitForExit();
            return process.ExitCode == 0 ? output : null;
        }
        catch (Exception e) when (e is Win32Exception or InvalidOperationException or IOException or PlatformNotSupportedException)
        {
            // git is not on PATH, or cannot be started here: the header simply
            // carries no identity.
            return null;
        }
    }
}
