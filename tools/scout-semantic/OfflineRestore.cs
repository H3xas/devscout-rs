using System.Diagnostics;
using System.Text.Json;

using Microsoft.Build.Evaluation;
using Microsoft.Build.Exceptions;

namespace ScoutSemantic;

/// <summary>One workspace project's outcome of the pre-load offline restore step: whether this
/// run attempted a restore for it, whether it is still unrestored (the attempt failed, or an
/// earlier restore recorded an error in its assets file), and a human-readable detail the run's
/// existing diagnostics fields carry verbatim -- never a new schema field.</summary>
internal sealed record RestoreOutcome(bool Attempted, bool Unrestored, string Detail);

/// <summary>
/// Restores exactly the workspace projects whose MSBuild-evaluated assets file is missing, from
/// the local NuGet global packages folder alone, before the workspace is used for facts, so every
/// emit mode reads the same restored inputs. A project whose assets file already exists is never
/// restored again; if that file records a failed restore, the project is reported unrestored from
/// the recorded error instead. The only package source is the global packages folder NuGet
/// resolves for the analysed root: <c>--source</c> replaces the configured feeds, and the source
/// properties pinned after the forwarded run properties stop a project's own
/// <c>RestoreAdditionalProjectSources</c> or a forwarded <c>RestoreSources</c> from widening it.
/// </summary>
internal static class OfflineRestore
{
    /// <summary>Everything one <see cref="RestoreMissing"/> call found and did.</summary>
    public sealed class Result
    {
        /// <summary>True once at least one project's restore actually ran, regardless of whether
        /// it succeeded -- the caller's own signal to dispose and reopen the workspace.</summary>
        public bool AnyRestored { get; set; }

        /// <summary>Keyed by the loaded project's own absolute file path.</summary>
        public Dictionary<string, RestoreOutcome> Outcomes { get; } = new(StringComparer.Ordinal);
    }

    public static Result RestoreMissing(LoadResult load, Options options)
    {
        var result = new Result();

        // One entry per distinct project file path -- a multi-targeting
        // project's variants share one assets file, so restoring for one
        // variant restores for all of them. Every project the WORKSPACE
        // opened, not only the ones target selection kept in load.Projects:
        // a selected project's own project reference to a variant target
        // selection excluded (a declared target --tfm did not request, or a
        // project a --projects filter left out) still needs to compile, and
        // Roslyn opens that referenced project into the same Solution
        // regardless of whether Loader kept it. The selected set is walked
        // first so its own accurately-resolved requested/effective tfm wins
        // over the name-parsed guess load.Solution.Projects alone can offer.
        var distinct = new Dictionary<string, string?>(StringComparer.Ordinal);
        foreach (var loaded in load.Projects)
        {
            var path = loaded.Project.FilePath;
            if (string.IsNullOrEmpty(path))
            {
                continue;
            }

            distinct.TryAdd(path, loaded.RequestedTfm ?? loaded.Tfm);
        }

        foreach (var project in load.Solution.Projects)
        {
            var path = project.FilePath;
            if (string.IsNullOrEmpty(path) || distinct.ContainsKey(path))
            {
                continue;
            }

            var (_, tfm) = Loader.SplitName(project.Name);
            distinct[path] = tfm;
        }

        if (distinct.Count == 0)
        {
            return result;
        }

        var dotnetHost = ResolveDotnetHost(Program.MsBuildPath);
        using var collection = new ProjectCollection();
        var missing = new List<string>();
        foreach (var (path, tfm) in distinct)
        {
            var assetsFile = LocateAssetsFile(collection, dotnetHost, path, options.Properties, tfm);
            if (assetsFile is null)
            {
                // Guessing a location could restore a project whose real
                // assets file exists elsewhere and rewrite it, so a project
                // whose location cannot be evaluated is left as it is.
                Console.Error.WriteLine(
                    $"warning: restore: {path}: could not evaluate the assets file location; not restoring it");
                continue;
            }

            if (!File.Exists(assetsFile))
            {
                missing.Add(path);
                continue;
            }

            if (RecordedRestoreError(assetsFile) is { } recorded)
            {
                result.Outcomes[path] = new RestoreOutcome(
                    Attempted: false, Unrestored: true, Detail: $"unrestored: an earlier restore recorded {recorded}");
            }
        }

        if (missing.Count == 0)
        {
            return result;
        }

        var globalPackagesFolder = dotnetHost is null ? null : ResolveGlobalPackagesFolder(dotnetHost, options.Root);

        if (dotnetHost is null || globalPackagesFolder is null)
        {
            var detail = dotnetHost is null
                ? "unrestored: could not resolve the dotnet host from the registered MSBuild instance"
                : "unrestored: could not resolve the NuGet global packages folder for the analysed root";
            foreach (var path in missing)
            {
                result.Outcomes[path] = new RestoreOutcome(Attempted: false, Unrestored: true, Detail: detail);
            }

            return result;
        }

        foreach (var path in missing)
        {
            Console.Error.WriteLine($"  restoring (offline): {path}");
            var (exitCode, output) = RunRestore(dotnetHost, path, globalPackagesFolder, options.Properties);
            result.AnyRestored = true;

            // The restore's own exit code, not a second look at the assets
            // file, decides success: NuGet can still write a project.assets
            // .json recording an unresolved dependency graph when a restore
            // fails outright (a cache miss, a source error), so the file's
            // mere presence cannot tell "restored" apart from "attempted and
            // failed" the way it can tell "never attempted" apart from both.
            result.Outcomes[path] = exitCode != 0
                ? new RestoreOutcome(true, true, $"unrestored: dotnet restore exit code {exitCode} -- {FirstUsefulLine(output)}")
                : new RestoreOutcome(true, false, "restored");
        }

        return result;
    }

    /// <summary>Every argument one restore invocation carries, in a fixed order -- a pure function
    /// kept separate from process launch so the exact source list and property forwarding stay
    /// independently checkable without spawning a process.</summary>
    internal static List<string> BuildRestoreArguments(
        string projectFullPath, string globalPackagesFolder, IReadOnlyDictionary<string, string> properties)
    {
        var args = new List<string>
        {
            "restore",
            projectFullPath,
            "--no-dependencies",
            "--source",
            globalPackagesFolder,
        };

        // The last -p: of a name wins, so the pinned properties go after every
        // forwarded one and a forwarded value of the same name is dropped.
        // NuGetAudit's vulnerability check would itself reach a remote feed.
        var pinned = new List<(string Name, string Value)>
        {
            ("NuGetAudit", "false"),
            ("RestoreSources", globalPackagesFolder),
            ("RestoreAdditionalProjectSources", ""),
        };
        var pinnedNames = new HashSet<string>(pinned.Select(p => p.Name), StringComparer.OrdinalIgnoreCase);

        foreach (var pair in properties.OrderBy(p => p.Key, StringComparer.Ordinal))
        {
            if (!pinnedNames.Contains(pair.Key))
            {
                args.Add($"-p:{pair.Key}={pair.Value}");
            }
        }

        args.AddRange(pinned.Select(p => $"-p:{p.Name}={p.Value}"));
        return args;
    }

    /// <summary>The MSBuild-evaluated assets file location for one project, honouring a custom
    /// intermediate path. Evaluated in process first; a project the in-process evaluator cannot
    /// load is evaluated by the SDK's own MSBuild in a child process. Null when neither can
    /// evaluate it.</summary>
    private static string? LocateAssetsFile(
        ProjectCollection collection,
        string? dotnetHost,
        string projectFullPath,
        IReadOnlyDictionary<string, string> baseGlobalProperties,
        string? requestedTfm)
    {
        var globals = new Dictionary<string, string>(baseGlobalProperties, StringComparer.OrdinalIgnoreCase);
        if (!string.IsNullOrEmpty(requestedTfm) && !globals.ContainsKey("TargetFramework"))
        {
            globals["TargetFramework"] = requestedTfm;
        }

        var projectDir = Path.GetDirectoryName(projectFullPath) is { Length: > 0 } dir ? dir : ".";
        Project project;
        try
        {
            project = new Project(projectFullPath, globals, toolsVersion: null, collection);
        }
        catch (Exception e) when (e is InvalidProjectFileException or IOException or InvalidOperationException)
        {
            // The in-process Microsoft.Build is pinned older than the installed
            // SDK, and some SDK targets (netstandard2.0 projects, for one) call
            // intrinsics it does not implement. The SDK's own MSBuild can.
            var outOfProcess = dotnetHost is null ? null : EvaluateOutOfProcess(dotnetHost, projectFullPath, globals);
            return outOfProcess is null ? null : Path.GetFullPath(outOfProcess, projectDir);
        }

        try
        {
            var value = project.GetPropertyValue("ProjectAssetsFile");
            return string.IsNullOrEmpty(value) ? null : Path.GetFullPath(value, projectDir);
        }
        finally
        {
            collection.UnloadProject(project);
        }
    }

    /// <summary>Every argument one out-of-process <c>ProjectAssetsFile</c> evaluation carries --
    /// the same global properties the in-process evaluation would have used.</summary>
    internal static List<string> BuildEvaluationArguments(string projectFullPath, IReadOnlyDictionary<string, string> globals)
    {
        var args = new List<string> { "msbuild", projectFullPath, "-nologo", "-getProperty:ProjectAssetsFile" };
        args.AddRange(globals.OrderBy(p => p.Key, StringComparer.Ordinal).Select(p => $"-p:{p.Key}={p.Value}"));
        return args;
    }

    private static string? EvaluateOutOfProcess(string dotnetHost, string projectFullPath, IReadOnlyDictionary<string, string> globals)
    {
        var psi = new ProcessStartInfo(dotnetHost)
        {
            WorkingDirectory = Path.GetDirectoryName(projectFullPath) is { Length: > 0 } dir ? dir : ".",
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
        };
        foreach (var arg in BuildEvaluationArguments(projectFullPath, globals))
        {
            psi.ArgumentList.Add(arg);
        }

        var (exitCode, stdout, _) = RunCaptured(psi);
        if (exitCode != 0)
        {
            return null;
        }

        var value = stdout.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .LastOrDefault();
        return string.IsNullOrEmpty(value) ? null : value;
    }

    /// <summary>The first error-level entry an existing assets file's <c>logs</c> records, as
    /// <c>CODE -- message</c>. NuGet writes the assets file even when a restore fails, so its mere
    /// presence does not mean the project restored. Null when the file records no error or cannot
    /// be read as JSON.</summary>
    internal static string? RecordedRestoreError(string assetsFile)
    {
        try
        {
            using var stream = File.OpenRead(assetsFile);
            using var document = JsonDocument.Parse(stream);
            if (!document.RootElement.TryGetProperty("logs", out var logs) || logs.ValueKind != JsonValueKind.Array)
            {
                return null;
            }

            foreach (var entry in logs.EnumerateArray())
            {
                if (entry.ValueKind != JsonValueKind.Object
                    || !entry.TryGetProperty("level", out var level)
                    || !string.Equals(level.GetString(), "Error", StringComparison.OrdinalIgnoreCase))
                {
                    continue;
                }

                var code = entry.TryGetProperty("code", out var c) ? c.GetString() : null;
                var message = entry.TryGetProperty("message", out var m) ? m.GetString() : null;
                var firstLine = message?.Split('\n', StringSplitOptions.TrimEntries).FirstOrDefault(l => l.Length > 0);
                return $"{code ?? "an error"} -- {firstLine ?? "no message"}";
            }

            return null;
        }
        catch (Exception e) when (e is JsonException or IOException or UnauthorizedAccessException)
        {
            return null;
        }
    }

    /// <summary>Resolves the <c>dotnet</c> host by walking up from the MSBuildLocator-registered
    /// SDK's own install path, never by guessing at PATH -- the same host the run's own MSBuild
    /// instance came from, so the restore and the run always agree on which SDK/NuGet resolves the
    /// global packages folder.</summary>
    internal static string? ResolveDotnetHost(string? msbuildPath)
    {
        if (string.IsNullOrEmpty(msbuildPath))
        {
            return null;
        }

        var exeName = OperatingSystem.IsWindows() ? "dotnet.exe" : "dotnet";
        for (var dir = new DirectoryInfo(msbuildPath); dir is not null; dir = dir.Parent)
        {
            var candidate = Path.Combine(dir.FullName, exeName);
            if (File.Exists(candidate))
            {
                return candidate;
            }
        }

        return null;
    }

    /// <summary>Runs <c>dotnet nuget locals global-packages --list</c> from the analysed root, so
    /// it honours that root's own <c>NUGET_PACKAGES</c>/<c>globalPackagesFolder</c> configuration
    /// exactly the way an ordinary restore of that root would.</summary>
    internal static string? ResolveGlobalPackagesFolder(string dotnetHost, string root)
    {
        var psi = new ProcessStartInfo(dotnetHost)
        {
            WorkingDirectory = root,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
        };
        psi.ArgumentList.Add("nuget");
        psi.ArgumentList.Add("locals");
        psi.ArgumentList.Add("global-packages");
        psi.ArgumentList.Add("--list");

        var (exitCode, output, _) = RunCaptured(psi);
        if (exitCode != 0)
        {
            return null;
        }

        const string marker = "global-packages:";
        foreach (var line in output.Split('\n'))
        {
            var trimmed = line.Trim();
            if (trimmed.StartsWith(marker, StringComparison.OrdinalIgnoreCase))
            {
                var value = trimmed[marker.Length..].Trim();
                return value.Length == 0 ? null : value;
            }
        }

        return null;
    }

    private static (int ExitCode, string Output) RunRestore(
        string dotnetHost, string projectPath, string globalPackagesFolder, IReadOnlyDictionary<string, string> properties)
    {
        var psi = new ProcessStartInfo(dotnetHost)
        {
            WorkingDirectory = Path.GetDirectoryName(projectPath) is { Length: > 0 } dir ? dir : ".",
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
        };

        foreach (var arg in BuildRestoreArguments(projectPath, globalPackagesFolder, properties))
        {
            psi.ArgumentList.Add(arg);
        }

        var (exitCode, stdout, stderr) = RunCaptured(psi);
        return (exitCode, stdout.Length > 0 ? stdout : stderr);
    }

    /// <summary>Runs a child process to completion, reading stdout and stderr concurrently so a
    /// child that fills one pipe while the other is being drained cannot block.</summary>
    private static (int ExitCode, string StdOut, string StdErr) RunCaptured(ProcessStartInfo psi)
    {
        using var process = Process.Start(psi);
        if (process is null)
        {
            return (-1, "", "the process did not start");
        }

        var stderrTask = process.StandardError.ReadToEndAsync();
        var stdout = process.StandardOutput.ReadToEnd();
        process.WaitForExit();
        return (process.ExitCode, stdout, stderrTask.Result);
    }

    /// <summary>The most useful single line of a restore's own combined output -- its first
    /// line naming an <c>error</c> (dotnet's own convention for the actual failure, as opposed
    /// to the "Determining projects to restore..." progress line that always comes first on
    /// success or failure alike), falling back to the first non-blank line when none does. Enough
    /// for a human-readable reason without embedding the whole console transcript in an artifact.</summary>
    private static string FirstUsefulLine(string output)
    {
        var lines = output
            .Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Where(line => line.Length > 0)
            .ToList();
        return lines.FirstOrDefault(line => line.Contains("error", StringComparison.OrdinalIgnoreCase))
            ?? lines.FirstOrDefault()
            ?? "no restore output";
    }
}
