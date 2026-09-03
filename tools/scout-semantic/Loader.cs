using System.Text.RegularExpressions;

using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.MSBuild;

namespace ScoutSemantic;

/// <summary>One project kept after multi-target de-duplication.</summary>
internal sealed record LoadedProject(Project Project, string Name, string? Tfm);

/// <summary>Everything <see cref="Loader"/> hands back to the runner.</summary>
internal sealed class LoadResult
{
    public required MSBuildWorkspace Workspace { get; init; }

    public required Solution Solution { get; init; }

    public List<LoadedProject> Projects { get; } = new();

    public List<WorkspaceDiagnostic> Failures { get; } = new();
}

/// <summary>
/// Opens the target solution or project through MSBuildWorkspace (§3.3).
/// Roslyn 4.14 evaluates MSBuild in an out-of-process BuildHost, so the only
/// requirement on the caller is that the target has already been restored.
/// </summary>
internal static class Loader
{
    // "App (net9.0)" / "App(net9.0)" -- how Roslyn names one variant of a
    // multi-targeting project.
    private static readonly Regex VariantName = new(@"^(?<n>.+?)\s*\((?<t>[^()]+)\)$", RegexOptions.Compiled);

    private static readonly Regex TfmLike =
        new(@"^(net|netstandard|netcoreapp|uap|monoandroid|xamarin|tizen)[0-9a-z.\-]*$",
            RegexOptions.Compiled | RegexOptions.IgnoreCase);

    public static async Task<LoadResult> LoadAsync(Options options)
    {
        var workspace = MSBuildWorkspace.Create(options.Properties);
        workspace.SkipUnrecognizedProjects = true;

        var failures = new List<WorkspaceDiagnostic>();
        workspace.WorkspaceFailed += (_, e) =>
        {
            failures.Add(e.Diagnostic);
            Console.Error.WriteLine($"  {e.Diagnostic.Kind.ToString().ToLowerInvariant()}: {e.Diagnostic.Message}");
        };

        var extension = Path.GetExtension(options.Input).ToLowerInvariant();
        Solution solution;
        if (extension is ".sln" or ".slnx" or ".slnf")
        {
            Console.Error.WriteLine($"loading solution {options.Input}");
            solution = await workspace.OpenSolutionAsync(options.Input).ConfigureAwait(false);
        }
        else
        {
            Console.Error.WriteLine($"loading project {options.Input}");
            var project = await workspace.OpenProjectAsync(options.Input).ConfigureAwait(false);
            solution = project.Solution;
        }

        var result = new LoadResult { Workspace = workspace, Solution = solution };
        result.Failures.AddRange(failures);

        foreach (var group in solution.Projects.GroupBy(p => p.FilePath ?? p.Name, StringComparer.Ordinal))
        {
            var variants = group.ToList();
            Project chosen = variants[0];
            if (variants.Count > 1 && options.Tfm is { Length: > 0 } wanted)
            {
                chosen = variants.FirstOrDefault(
                    p => p.Name.EndsWith("(" + wanted + ")", StringComparison.Ordinal)) ?? variants[0];
            }

            var (name, tfm) = SplitName(chosen.Name);
            tfm ??= TfmFromOutputPath(chosen.OutputFilePath ?? chosen.CompilationOutputInfo.AssemblyPath);

            if (options.ProjectGlobs.Count > 0 && !options.ProjectGlobs.Any(g => g.IsMatch(name) || g.IsMatch(chosen.Name)))
            {
                continue;
            }

            if (variants.Count > 1)
            {
                Console.Error.WriteLine(
                    $"  {name}: {variants.Count} target variants, keeping {tfm ?? "(unknown tfm)"}");
            }

            result.Projects.Add(new LoadedProject(chosen, name, tfm));
        }

        // List.Sort is unstable, so a name tie must be broken explicitly or the
        // project order -- and with it every downstream record order -- can
        // vary between runs. Two projects can legitimately share a name (the
        // same .csproj name under two directories), and the project file path
        // is the one field that is unique per project.
        result.Projects.Sort((a, b) =>
        {
            var byName = string.CompareOrdinal(a.Name, b.Name);
            return byName != 0 ? byName : string.CompareOrdinal(a.Project.FilePath, b.Project.FilePath);
        });
        return result;
    }

    /// <summary>Splits "App (net9.0)" into ("App", "net9.0"); leaves other names alone.</summary>
    public static (string Name, string? Tfm) SplitName(string projectName)
    {
        var m = VariantName.Match(projectName);
        if (m.Success && TfmLike.IsMatch(m.Groups["t"].Value))
        {
            return (m.Groups["n"].Value, m.Groups["t"].Value);
        }

        return (projectName, null);
    }

    /// <summary>Last resort: bin/Debug/&lt;tfm&gt;/App.dll carries the TFM as a directory name.</summary>
    private static string? TfmFromOutputPath(string? outputPath)
    {
        if (string.IsNullOrEmpty(outputPath))
        {
            return null;
        }

        var dir = Path.GetFileName(Path.GetDirectoryName(outputPath) ?? "");
        return dir.Length > 0 && TfmLike.IsMatch(dir) ? dir : null;
    }

    /// <summary>Project references as unit names, de-duplicated and ordinal-sorted.</summary>
    public static List<string> ReferenceNames(Solution solution, Project project)
    {
        var names = new SortedSet<string>(StringComparer.Ordinal);
        foreach (var reference in project.ProjectReferences)
        {
            var referenced = solution.GetProject(reference.ProjectId);
            if (referenced is not null)
            {
                names.Add(SplitName(referenced.Name).Name);
            }
        }

        return names.ToList();
    }

    /// <summary>§3.3: a unit is a test project when its csproj opts into the test SDK.</summary>
    public static bool IsTestProject(string? projectFilePath)
    {
        if (string.IsNullOrEmpty(projectFilePath) || !File.Exists(projectFilePath))
        {
            return false;
        }

        string text;
        try
        {
            text = File.ReadAllText(projectFilePath);
        }
        catch (IOException)
        {
            return false;
        }

        return text.Contains("Microsoft.NET.Test.Sdk", StringComparison.OrdinalIgnoreCase)
            || text.Replace(" ", "").Contains("<IsTestProject>true", StringComparison.OrdinalIgnoreCase);
    }
}
