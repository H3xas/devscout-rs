using System.Text.Json;

using Microsoft.CodeAnalysis;

namespace ScoutSemantic;

/// <summary>One entry of an assets file's <c>logs</c> array. A field that is absent or not a
/// JSON string reads as null.</summary>
internal sealed record AssetsLogEntry(string? Code, string? Level, string? Message);

/// <summary>Reads the <c>logs</c> array NuGet writes into a project's assets file.</summary>
internal static class AssetsLogs
{
    /// <summary>Every object entry of the file's <c>logs</c> array, in file order. Null when the
    /// file cannot be opened or read, is not JSON, has no object root, or has no <c>logs</c>
    /// array: a caller that needs the entries to prove something gets nothing to prove it
    /// with.</summary>
    public static IReadOnlyList<AssetsLogEntry>? Read(string assetsFile)
    {
        try
        {
            using var stream = File.OpenRead(assetsFile);
            using var document = JsonDocument.Parse(stream);
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object
                || !root.TryGetProperty("logs", out var logs)
                || logs.ValueKind != JsonValueKind.Array)
            {
                return null;
            }

            var entries = new List<AssetsLogEntry>();
            foreach (var entry in logs.EnumerateArray())
            {
                if (entry.ValueKind == JsonValueKind.Object)
                {
                    entries.Add(new AssetsLogEntry(StringOf(entry, "code"), StringOf(entry, "level"), StringOf(entry, "message")));
                }
            }

            return entries;
        }
        catch (Exception e) when (e is not OutOfMemoryException)
        {
            return null;
        }
    }

    private static string? StringOf(JsonElement entry, string name) =>
        entry.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.String ? value.GetString() : null;
}

/// <summary>
/// NuGet's vulnerability audit (<c>NU1900</c> to <c>NU1905</c>) logs a warning into a project's
/// assets file, and the SDK replays that entry at design time, where MSBuildWorkspace reports it
/// as a workspace <c>Failure</c> carrying only a wrapped message: no code, no level. The audit
/// never changes what compiles, so such a diagnostic must not demote a compilation. It is told
/// apart from a real failure by the code and level the restore itself recorded, and by NuGet's
/// own message bytes ending the diagnostic's text. The wrapper around that message is a
/// localizable resource, so it is never parsed.
/// </summary>
internal static class NuGetAuditWarnings
{
    private static readonly HashSet<string> AuditCodes = new(StringComparer.Ordinal)
    {
        "NU1900", "NU1901", "NU1902", "NU1903", "NU1904", "NU1905",
    };

    /// <summary>Whether <paramref name="rawMessage"/>, unredacted, ends with the message of an
    /// audit entry logged at <c>Warning</c> level. An empty message would end every text, so it
    /// never matches; with no entries nothing matches.</summary>
    public static bool Matches(string rawMessage, IReadOnlyList<AssetsLogEntry>? entries) =>
        entries is not null
        && entries.Any(entry =>
            entry.Code is not null
            && AuditCodes.Contains(entry.Code)
            && string.Equals(entry.Level, "Warning", StringComparison.OrdinalIgnoreCase)
            && !string.IsNullOrEmpty(entry.Message)
            && rawMessage.EndsWith(entry.Message, StringComparison.Ordinal));
}

/// <summary>
/// The per-run join between workspace diagnostics and the assets files of the projects they
/// name. A project's assets file is located and read only once a <c>Failure</c> is weighed
/// against it, and at most once per run; a locator that fails for any reason leaves the project
/// with no entries, so its failures keep demoting.
/// </summary>
internal sealed class AuditWarningJoin
{
    private readonly IReadOnlyDictionary<string, string?> projects;
    private readonly Func<string, string?, string?> locate;
    private readonly Dictionary<string, IReadOnlyList<AssetsLogEntry>?> entriesByProject = new(StringComparer.Ordinal);

    /// <param name="projects">Every project file path the workspace opened, with the target
    /// framework its assets file is evaluated for.</param>
    /// <param name="locate">Project path and target framework to the assets file path, or null
    /// when it cannot be evaluated.</param>
    public AuditWarningJoin(IReadOnlyDictionary<string, string?> projects, Func<string, string?, string?> locate)
    {
        this.projects = projects;
        this.locate = locate;
    }

    /// <summary>The join for one run: locates each assets file the way the restore step does,
    /// with the run's own properties, and resolves the dotnet host only when first needed.</summary>
    public static AuditWarningJoin ForRun(LoadResult load, Options options)
    {
        var host = new Lazy<string?>(() => OfflineRestore.ResolveDotnetHost(Program.MsBuildPath));
        return new AuditWarningJoin(
            OfflineRestore.OpenedProjects(load),
            (path, tfm) => OfflineRestore.LocateAssetsFile(host.Value, path, options.Properties, tfm));
    }

    /// <summary>Whether a compilation's own workspace diagnostics demote it: some
    /// <c>Failure</c> among them is not an audit warning for its own project.</summary>
    public bool DemotesRecord(IEnumerable<WorkspaceDiagnostic> ownDiagnostics, string projectPath) =>
        ownDiagnostics.Any(d => d.Kind == WorkspaceDiagnosticKind.Failure && !IsAuditWarningFor(d, projectPath));

    /// <summary>Whether a solution-wide diagnostic counts as a load failure for
    /// <c>--strict</c>. A <c>Failure</c> is excused only when it names at least one opened
    /// project and is an audit warning for every project it names.</summary>
    public bool IsLoadFailure(WorkspaceDiagnostic diagnostic)
    {
        if (diagnostic.Kind != WorkspaceDiagnosticKind.Failure)
        {
            return false;
        }

        var named = projects.Keys.Where(p => diagnostic.Message.Contains(p, StringComparison.Ordinal)).ToList();
        return named.Count == 0 || named.Any(p => !IsAuditWarningFor(diagnostic, p));
    }

    private bool IsAuditWarningFor(WorkspaceDiagnostic diagnostic, string projectPath) =>
        NuGetAuditWarnings.Matches(diagnostic.Message, EntriesFor(projectPath));

    private IReadOnlyList<AssetsLogEntry>? EntriesFor(string projectPath)
    {
        if (entriesByProject.TryGetValue(projectPath, out var cached))
        {
            return cached;
        }

        IReadOnlyList<AssetsLogEntry>? entries;
        try
        {
            var assetsFile = locate(projectPath, projects.GetValueOrDefault(projectPath));
            entries = string.IsNullOrEmpty(assetsFile) ? null : AssetsLogs.Read(assetsFile);
        }
        catch (Exception e) when (e is not OutOfMemoryException)
        {
            entries = null;
        }

        entriesByProject[projectPath] = entries;
        return entries;
    }
}
