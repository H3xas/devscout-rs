using System.Diagnostics;
using System.Text.Json;

namespace ScoutSemanticTests;

/// <summary>
/// The one dedicated, pinned .NET SDK install the tests that build projects of their own compile
/// and restore through. Restore resolves the SDK upward from the working directory, so a project a
/// test creates under the system temporary directory has no pin in reach and resolves whichever
/// SDK the host happens to select -- a newer one on a machine that has one installed, which is
/// enough to move the reference identities and the SDK the compilation records. Every such test
/// therefore takes its host, its reference assemblies and its pin from the install named by
/// <see cref="RootVariable"/> instead of from the ambient machine, and fails loudly when that
/// install is absent or is not the install the committed snapshots were produced from.
///
/// The pinned values are read from <c>tools/dotnet-snapshot-sdk.json</c>, the same declaration the
/// regeneration tool validates a root against, so the two cannot describe different installs.
/// </summary>
internal static class SnapshotSdk
{
    /// <summary>Names the dedicated install every project-building test runs against.</summary>
    internal const string RootVariable = "SNAPSHOT_DOTNET_ROOT";

    private const string ManifestRelativePath = "tools/dotnet-snapshot-sdk.json";

    private static readonly Lazy<Pin> Loaded = new(LoadPin, LazyThreadSafetyMode.ExecutionAndPublication);

    private static readonly Lazy<string> ValidatedRoot =
        new(ResolveRoot, LazyThreadSafetyMode.ExecutionAndPublication);

    /// <summary>The repository root, found by walking up from the test assembly to the pin.</summary>
    internal static string RepoRoot { get; } = FindRepoRoot();

    private static Pin PinValue => Loaded.Value;

    /// <summary>The validated dedicated install's own root.</summary>
    internal static string Root => ValidatedRoot.Value;

    /// <summary>That install's host, invoked by absolute path so a machine's own <c>dotnet</c>
    /// never enters a child's selection.</summary>
    internal static string DotnetPath => Path.Combine(Root, "dotnet");

    /// <summary>The reference-assembly directory of the pinned reference pack, the set a stub
    /// assembly compiled in this process has to bind against so the library a test packs and the
    /// project that later consumes it agree on every shared base-class-library type.</summary>
    internal static string ReferenceAssemblyDirectory => Path.Combine(
        Root, "packs", PinValue.ReferencePackId, PinValue.ReferencePackVersion, "ref", PinValue.ReferencePackTargetFramework);

    internal static string[] ReferenceAssemblyPaths() =>
        Directory.GetFiles(ReferenceAssemblyDirectory, "*.dll").OrderBy(path => path, StringComparer.Ordinal).ToArray();

    /// <summary>Points a child process at the dedicated install: this root's host first on
    /// <c>PATH</c>, this root as <c>DOTNET_ROOT</c>, and no multi-level lookup, so the restore the
    /// child spawns and the MSBuild it registers both come from it. The nologo and telemetry
    /// switches only keep the captured output free of banner text a test would otherwise have to
    /// step over.</summary>
    internal static void ApplyTo(ProcessStartInfo startInfo)
    {
        startInfo.Environment["DOTNET_ROOT"] = Root;
        startInfo.Environment["DOTNET_MULTILEVEL_LOOKUP"] = "0";
        startInfo.Environment["DOTNET_NOLOGO"] = "1";
        startInfo.Environment["DOTNET_CLI_TELEMETRY_OPTOUT"] = "1";
        var path = startInfo.Environment.TryGetValue("PATH", out var inherited) && inherited is not null
            ? inherited
            : Environment.GetEnvironmentVariable("PATH") ?? string.Empty;
        startInfo.Environment["PATH"] = Root + Path.PathSeparator + path;
    }

    /// <summary>Writes the exact pin into a test-owned project root, so a project under it resolves
    /// that one SDK or fails rather than falling back to whatever the host would have chosen, and
    /// so a project reference reached through it inherits the same pin.</summary>
    internal static void WriteGlobalJson(string projectRoot)
    {
        Directory.CreateDirectory(projectRoot);
        File.WriteAllText(
            Path.Combine(projectRoot, "global.json"),
            $"{{ \"sdk\": {{ \"version\": \"{PinValue.SdkVersion}\", \"rollForward\": \"disable\" }} }}\n");
    }

    /// <summary>The version a child running with <see cref="ApplyTo"/>'s environment selects;
    /// asserted against the oracle's own registered instance by the tests that care.</summary>
    internal static string SdkVersion => PinValue.SdkVersion;

    /// <summary>Runs one query against a candidate root's host, outside any environment the
    /// calling test has already prepared. The root is a parameter rather than the validated
    /// property, because validating a root is what establishes that property.</summary>
    private static string QueryHost(string root, string argument)
    {
        var host = Path.Combine(root, "dotnet");
        using var process = Process.Start(new ProcessStartInfo(host, argument)
        {
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
            WorkingDirectory = RepoRoot,
        }) ?? throw new InvalidOperationException($"could not start {host}");
        var stdout = process.StandardOutput.ReadToEnd();
        process.StandardError.ReadToEnd();
        process.WaitForExit();
        if (process.ExitCode != 0)
        {
            throw new InvalidOperationException($"{host} {argument} exited {process.ExitCode}");
        }

        return stdout;
    }

    private static string ResolveRoot() => Validate(Environment.GetEnvironmentVariable(RootVariable));

    /// <summary>Checks one configured value and returns the root it names. Kept separate from the
    /// environment read so a test can drive each refusal without disturbing this process's own
    /// selection.</summary>
    internal static string Validate(string? configured)
    {
        if (string.IsNullOrWhiteSpace(configured))
        {
            throw new InvalidOperationException(
                $"{RootVariable} is not set. These tests build projects of their own, so they need a dedicated .NET SDK "
                + $"install holding exactly SDK {PinValue.SdkVersion}, and cannot fall back to an ambient one: that would "
                + "record a different SDK and different reference identities than the committed snapshots carry. Point it "
                + "at such an install (tools/scout-semantic/README.md documents how to obtain and validate one).");
        }

        var root = Path.GetFullPath(configured);
        if (!Directory.Exists(root))
        {
            throw new InvalidOperationException($"{RootVariable} is not a directory: {root}");
        }

        var host = Path.Combine(root, "dotnet");
        if (!File.Exists(host))
        {
            throw new InvalidOperationException($"{RootVariable}={root} carries no dotnet host at {host}");
        }

        var listed = QueryHost(root, "--list-sdks")
            .Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Select(line => line.Split(' ')[0])
            .ToArray();
        if (listed.Length != 1 || listed[0] != PinValue.SdkVersion)
        {
            throw new InvalidOperationException(
                $"{RootVariable}={root} must hold exactly one SDK, {PinValue.SdkVersion}; it lists "
                + (listed.Length == 0 ? "none" : string.Join(", ", listed))
                + ". The snapshots only reproduce from that one install.");
        }

        var version = QueryHost(root, "--version").Trim();
        if (version != PinValue.SdkVersion)
        {
            throw new InvalidOperationException($"{RootVariable}={root} resolves SDK {version} instead of {PinValue.SdkVersion}");
        }

        var references = Path.Combine(
            root, "packs", PinValue.ReferencePackId, PinValue.ReferencePackVersion, "ref", PinValue.ReferencePackTargetFramework);
        if (!Directory.Exists(references))
        {
            throw new InvalidOperationException(
                $"{RootVariable}={root} carries no {PinValue.ReferencePackId} {PinValue.ReferencePackVersion} "
                + $"{PinValue.ReferencePackTargetFramework} reference pack at {references}");
        }

        return root;
    }

    private static Pin LoadPin()
    {
        var path = Path.Combine(RepoRoot, ManifestRelativePath);
        if (!File.Exists(path))
        {
            throw new InvalidOperationException($"the snapshot SDK pin is missing at {path}");
        }

        using var document = ParsePin(path);
        var root = document.RootElement;
        try
        {
            var pack = root.GetProperty("referencePack");
            string Text(JsonElement element, string name) =>
                element.GetProperty(name).GetString()
                ?? throw new InvalidOperationException($"the snapshot SDK pin at {path} has a non-string {name}");
            return new Pin(
                Text(root, "sdkVersion"),
                Text(pack, "id"),
                Text(pack, "version"),
                Text(pack, "targetFramework"));
        }
        catch (KeyNotFoundException error)
        {
            throw new InvalidOperationException($"the snapshot SDK pin at {path} is missing a field: {error.Message}");
        }
    }

    private static JsonDocument ParsePin(string path)
    {
        try
        {
            return JsonDocument.Parse(File.ReadAllText(path));
        }
        catch (JsonException error)
        {
            throw new InvalidOperationException($"the snapshot SDK pin at {path} is not readable JSON: {error.Message}");
        }
    }

    private static string FindRepoRoot()
    {
        var directory = AppContext.BaseDirectory;
        while (directory is not null && !File.Exists(Path.Combine(directory, ManifestRelativePath)))
        {
            var parent = Path.GetDirectoryName(directory.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar));
            directory = parent == directory ? null : parent;
        }

        return directory ?? throw new InvalidOperationException(
            $"could not locate the repository root ({ManifestRelativePath}) walking up from {AppContext.BaseDirectory}");
    }

    private sealed record Pin(
        string SdkVersion,
        string ReferencePackId,
        string ReferencePackVersion,
        string ReferencePackTargetFramework);
}
