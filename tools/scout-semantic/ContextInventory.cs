using System.Text.RegularExpressions;
using System.Xml.Linq;

using Microsoft.Build.Evaluation;

namespace ScoutSemantic;

/// <summary>One project the solution file names, independent of what the workspace actually loaded.</summary>
internal sealed record ExpectedProject(string Name, string AbsolutePath);

/// <summary>What a fresh, workspace-independent MSBuild evaluation of one project reads.</summary>
internal sealed class ProjectInventory
{
    /// <summary>
    /// Every <c>Compile</c> item the evaluation named, root-relative when it
    /// resolves under <c>--root</c>, else a <c>../</c>-relative display path
    /// -- never an absolute local path. A superset of <see cref="Dropped"/>'s
    /// paths union the loaded set: nothing here is silently left out.
    /// </summary>
    public required List<string> ExpectedDisplayPaths { get; init; }

    public required List<DroppedDocument> Dropped { get; init; }

    public required List<ContextImport> Imports { get; init; }

    public required string? Configuration { get; init; }

    public required string? Platform { get; init; }

    public required string? EffectiveTfm { get; init; }

    public required string? AssemblyName { get; init; }

    public required string? RootNamespace { get; init; }

    public required string? LanguageVersion { get; init; }

    public required string? Nullable { get; init; }

    public required bool AllowUnsafeBlocks { get; init; }

    public required List<string> PreprocessorSymbols { get; init; }
}

/// <summary>
/// Reads the expected project/document/import list independently of the
/// Roslyn workspace, by parsing the solution file directly and re-evaluating
/// each project through its own, fresh <see cref="ProjectCollection"/> --
/// never the ambient one <c>MSBuildWorkspace</c> uses -- so a project or
/// document Roslyn's own load silently drops is still reportable. A leaf
/// module: nothing in <see cref="Walker"/> or <c>FactsWalker</c> calls into
/// it, so the oracle and fact walk this ticket must leave byte-identical
/// stay untouched by construction.
/// </summary>
internal static class ContextInventory
{
    private static readonly Regex SlnProjectLine = new(
        """^Project\("\{[0-9A-Fa-f-]+\}"\)\s*=\s*"(?<name>[^"]+)"\s*,\s*"(?<path>[^"]+)"\s*,\s*"\{[0-9A-Fa-f-]+\}"$""",
        RegexOptions.Compiled | RegexOptions.Multiline);

    /// <summary>Every project a <c>.sln</c> or <c>.slnx</c> names, parsed from the solution text itself.</summary>
    public static List<ExpectedProject> ExpectedProjectsOfSolution(string solutionPath)
    {
        var extension = Path.GetExtension(solutionPath).ToLowerInvariant();
        var directory = Path.GetDirectoryName(Path.GetFullPath(solutionPath)) ?? ".";
        return extension == ".slnx"
            ? ExpectedProjectsOfSlnx(solutionPath, directory)
            : ExpectedProjectsOfSln(solutionPath, directory);
    }

    private static List<ExpectedProject> ExpectedProjectsOfSln(string path, string directory)
    {
        var text = File.ReadAllText(path);
        var projects = new List<ExpectedProject>();
        foreach (Match m in SlnProjectLine.Matches(text))
        {
            var relative = m.Groups["path"].Value.Replace('\\', '/');
            if (!IsRecognizedProjectExtension(relative))
            {
                continue;
            }

            projects.Add(new ExpectedProject(m.Groups["name"].Value, Path.GetFullPath(Path.Combine(directory, relative))));
        }

        return projects;
    }

    private static List<ExpectedProject> ExpectedProjectsOfSlnx(string path, string directory)
    {
        var document = XDocument.Load(path);
        var projects = new List<ExpectedProject>();
        foreach (var element in document.Descendants().Where(e => e.Name.LocalName == "Project"))
        {
            var relative = element.Attribute("Path")?.Value;
            if (string.IsNullOrEmpty(relative) || !IsRecognizedProjectExtension(relative))
            {
                continue;
            }

            var normalized = relative.Replace('\\', '/');
            var name = Path.GetFileNameWithoutExtension(normalized);
            projects.Add(new ExpectedProject(name, Path.GetFullPath(Path.Combine(directory, normalized))));
        }

        return projects;
    }

    private static bool IsRecognizedProjectExtension(string relativePath) =>
        relativePath.EndsWith(".csproj", StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// Evaluates one project through <paramref name="collection"/> -- a fresh
    /// collection the caller owns and disposes, distinct from
    /// <c>MSBuildWorkspace</c>'s ambient one -- and reads the properties and
    /// items this ticket's context report needs.
    /// </summary>
    public static ProjectInventory Evaluate(
        ProjectCollection collection,
        string projectFullPath,
        IReadOnlyDictionary<string, string> baseGlobalProperties,
        string? requestedTfm,
        RepoPaths paths)
    {
        var globals = new Dictionary<string, string>(baseGlobalProperties, StringComparer.OrdinalIgnoreCase);
        if (!string.IsNullOrEmpty(requestedTfm) && !globals.ContainsKey("TargetFramework"))
        {
            globals["TargetFramework"] = requestedTfm;
        }

        var project = new Project(projectFullPath, globals, toolsVersion: null, collection);
        try
        {
            var expected = new List<string>();
            var dropped = new List<DroppedDocument>();
            foreach (var item in project.GetItems("Compile"))
            {
                var full = item.GetMetadataValue("FullPath");
                var (rel, dropReason) = paths.Classify(full);
                var display = rel ?? DisplayRelativePath(paths.Root, full);
                expected.Add(display);

                if (dropReason is not null)
                {
                    dropped.Add(new DroppedDocument { Path = display, Reason = dropReason });
                }
                else if (!File.Exists(full))
                {
                    dropped.Add(new DroppedDocument { Path = display, Reason = "missing" });
                }
            }

            expected = expected.Distinct(StringComparer.Ordinal).OrderBy(p => p, StringComparer.Ordinal).ToList();
            dropped = dropped
                .GroupBy(d => (d.Path, d.Reason))
                .Select(g => g.First())
                .OrderBy(d => d.Path, StringComparer.Ordinal)
                .ToList();

            var imports = new List<ContextImport>();
            foreach (var import in project.Imports)
            {
                var importPath = import.ImportedProject.FullPath;
                if (string.IsNullOrEmpty(importPath) || !File.Exists(importPath))
                {
                    continue;
                }

                if (IsGeneratedRestoreArtifact(importPath))
                {
                    // obj/ is already a skip directory for every authored
                    // document this tool walks; a restore-generated import
                    // under it (*.nuget.g.props/.targets) is the same kind
                    // of build artifact, not a project- or repo-authored
                    // build customization -- and its content embeds the
                    // local machine's absolute NuGet global-packages root,
                    // so folding it would make the fingerprint depend on
                    // where the repository happens to be checked out.
                    // Package-version changes are already visible through
                    // the resolved metadata references themselves.
                    continue;
                }

                string content;
                try
                {
                    content = File.ReadAllText(importPath);
                }
                catch (IOException)
                {
                    continue;
                }

                imports.Add(new ContextImport { Identity = NormalizeImportIdentity(importPath, paths), Hash = FactsWriter.Sha1(content) });
            }

            imports = imports
                // The installed SDK's own hundreds of .props/.targets files
                // import every SDK-style project alike and move in lockstep
                // with the SDK version already carried in this record's own
                // `versions.sdk`/`versions.msbuild` -- listing every one of
                // them here would swamp a repo-local build customization
                // (Directory.Build.props, a NuGet package's own .targets)
                // in noise without adding an independent signal.
                .Where(i => !i.Identity.StartsWith("sdk-file:", StringComparison.Ordinal))
                .GroupBy(i => i.Identity, StringComparer.Ordinal)
                .Select(g => g.First())
                .OrderBy(i => i.Identity, StringComparer.Ordinal)
                .ToList();

            var defineConstants = project.GetPropertyValue("DefineConstants");
            var symbols = defineConstants
                .Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
                .Distinct(StringComparer.Ordinal)
                .OrderBy(s => s, StringComparer.Ordinal)
                .ToList();

            return new ProjectInventory
            {
                ExpectedDisplayPaths = expected,
                Dropped = dropped,
                Imports = imports,
                Configuration = NullIfEmpty(project.GetPropertyValue("Configuration")),
                Platform = NullIfEmpty(project.GetPropertyValue("Platform")),
                EffectiveTfm = NullIfEmpty(project.GetPropertyValue("TargetFramework")),
                AssemblyName = NullIfEmpty(project.GetPropertyValue("AssemblyName")),
                RootNamespace = NullIfEmpty(project.GetPropertyValue("RootNamespace")),
                LanguageVersion = NullIfEmpty(project.GetPropertyValue("LangVersion")),
                Nullable = NullIfEmpty(project.GetPropertyValue("Nullable")),
                AllowUnsafeBlocks = string.Equals(
                    project.GetPropertyValue("AllowUnsafeBlocks"), "true", StringComparison.OrdinalIgnoreCase),
                PreprocessorSymbols = symbols,
            };
        }
        finally
        {
            collection.UnloadProject(project);
        }
    }

    private static string? NullIfEmpty(string value) => value.Length == 0 ? null : value;

    /// <summary>
    /// A restore-generated import living under any project's own <c>obj/</c>
    /// directory: <c>*.nuget.g.props</c>, <c>*.nuget.g.targets</c>, and any
    /// other file MSBuild writes there. Checked by directory component, the
    /// same way <see cref="RepoPaths"/>'s skip-dir list is, not by file name
    /// alone, since a restore can regenerate more than the two well-known
    /// names.
    /// </summary>
    private static bool IsGeneratedRestoreArtifact(string fullPath)
    {
        var normalized = fullPath.Replace('\\', '/');
        return normalized.Split('/').Any(segment => string.Equals(segment, "obj", StringComparison.Ordinal));
    }

    /// <summary>A relative display path for a document that <see cref="RepoPaths.Classify"/> dropped, so its reason is still reportable without an absolute local path.</summary>
    private static string DisplayRelativePath(string root, string absolute)
    {
        try
        {
            return Path.GetRelativePath(root, absolute).Replace('\\', '/');
        }
        catch (ArgumentException)
        {
            return Path.GetFileName(absolute);
        }
    }

    /// <summary>
    /// Path segments that mark a file as owned by the installed .NET SDK
    /// rather than by the analysed repository: the SDK's own tree
    /// (<c>/sdk/</c>) and, separately, an installed workload's manifest tree
    /// (<c>/sdk-manifests/</c> -- present whenever any workload, e.g. MAUI or
    /// Android, is installed alongside the SDK actually in use here; it does
    /// not require the analysed solution to use that workload). Both move in
    /// lockstep with the installed SDK, never with this repository's own
    /// commits, which is exactly what <see cref="NormalizeImportIdentity"/>'s
    /// caller filters <c>sdk-file:</c> identities out for.
    /// </summary>
    private static readonly string[] SdkOwnedMarkers = ["/sdk/", "/sdk-manifests/"];

    /// <summary>
    /// An SDK <c>.props</c>/<c>.targets</c> file or a NuGet package's build
    /// file lives outside the analysed repository; it is recorded by a
    /// normalized identity (package id + version when the well-known NuGet
    /// global-packages path shape is recognizable, an SDK-relative tail when
    /// an <see cref="SdkOwnedMarkers"/> segment is recognizable, else the
    /// file's bare name) rather than its absolute local path, which
    /// <see cref="ContextSchema"/> also rejects as defense in depth. A
    /// repository-authored import (<c>Directory.Build.props</c> and its kin,
    /// found by <paramref name="paths"/> the same way a <c>Compile</c> item
    /// is) is recorded by its own root-relative path instead of its bare
    /// name: more than one same-named override file at different repository
    /// depths -- a root <c>Directory.Build.props</c> plus a subtree's own
    /// override, an ordinary MSBuild pattern -- otherwise collide onto one
    /// identity, and the consumer's freshness re-hash, which joins the
    /// identity under the repository root, then compares different files'
    /// content against each other and reports every one of them changed. An
    /// unrecognized SDK-owned or otherwise-external path shape falls through
    /// to the bare-name case, which the consumer's own freshness check
    /// re-hashes against the repository root and finds missing there --
    /// silently degrading the whole artifact to stale on every run touching
    /// that path, exactly the failure an unrecognized workload-manifest
    /// import already caused before <c>/sdk-manifests/</c> was added here.
    /// </summary>
    internal static string NormalizeImportIdentity(string fullPath, RepoPaths paths)
    {
        var normalized = fullPath.Replace('\\', '/');
        var nugetMarker = "/.nuget/packages/";
        var nugetIndex = normalized.IndexOf(nugetMarker, StringComparison.OrdinalIgnoreCase);
        if (nugetIndex >= 0)
        {
            var rest = normalized[(nugetIndex + nugetMarker.Length)..].Split('/');
            if (rest.Length >= 3)
            {
                return "nuget:" + rest[0] + "/" + rest[1] + "/" + string.Join('/', rest.Skip(2));
            }
        }

        var repoRelative = paths.RelativeProjectPath(fullPath);
        if (repoRelative is not null)
        {
            return "external-file:" + repoRelative;
        }

        foreach (var sdkMarker in SdkOwnedMarkers)
        {
            var sdkIndex = normalized.ToLowerInvariant().IndexOf(sdkMarker, StringComparison.Ordinal);
            if (sdkIndex >= 0)
            {
                var tail = normalized[(sdkIndex + sdkMarker.Length)..].Split('/');
                return "sdk-file:" + string.Join('/', tail.TakeLast(Math.Min(3, tail.Length)));
            }
        }

        return "external-file:" + Path.GetFileName(normalized);
    }
}
