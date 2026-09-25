using System.Diagnostics;
using System.Text.Json;

using Xunit;

namespace ScoutSemanticTests;

/// <summary>
/// Drives the built CLI as a real subprocess, the way CI's own bash steps
/// do, because this defect lives in the composition of
/// <c>Runner.Run</c>'s "zero projects loaded" guard and Loader's
/// <c>--projects</c> filtering -- two pieces no existing unit test exercises
/// together. Requires <c>fixtures/csharp-semantic</c> and
/// <c>fixtures/csharp-context/src/{Clean,Legacy,Broken}</c> already restored
/// and <c>tools/scout-semantic</c> already built, exactly as the CI job's own
/// step ordering (restore, build oracle, then this test) guarantees.
/// </summary>
public sealed class CliProjectsFilterTests
{
    private static readonly string RepoRoot = FindRepoRoot();
    private static readonly string CliDll = Path.Combine(AppContext.BaseDirectory, "scout-semantic.dll");

    private static string FindRepoRoot()
    {
        var dir = AppContext.BaseDirectory;
        while (dir is not null && !File.Exists(Path.Combine(dir, "fixtures", "csharp-semantic", "Fixture.sln")))
        {
            var parent = Path.GetDirectoryName(dir.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar));
            dir = parent == dir ? null : parent;
        }

        return dir ?? throw new InvalidOperationException(
            $"could not locate the repository root (fixtures/csharp-semantic/Fixture.sln) walking up from {AppContext.BaseDirectory}");
    }

    private static (int ExitCode, string StdOut, string StdErr) Run(params string[] args)
    {
        var psi = new ProcessStartInfo("dotnet")
        {
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
            WorkingDirectory = RepoRoot,
        };
        psi.ArgumentList.Add(CliDll);
        foreach (var arg in args)
        {
            psi.ArgumentList.Add(arg);
        }

        using var process = Process.Start(psi) ?? throw new InvalidOperationException("failed to start dotnet");
        var stdout = process.StandardOutput.ReadToEnd();
        var stderr = process.StandardError.ReadToEnd();
        process.WaitForExit();
        return (process.ExitCode, stdout, stderr);
    }

    [Fact]
    public void a_projects_filter_matching_nothing_is_still_zero_projects_loaded_under_strict_context()
    {
        Assert.True(File.Exists(CliDll), $"expected the built CLI at {CliDll}");
        var outPath = Path.Combine(Path.GetTempPath(), $"scout-semantic-cli-b1-{Guid.NewGuid():N}.json");
        try
        {
            var (exitCode, _, stderr) = Run(
                Path.Combine("fixtures", "csharp-semantic", "Fixture.sln"),
                "--root", Path.Combine("fixtures", "csharp-semantic"),
                "--projects", "NoSuchProjectName",
                "--emit", "context",
                "--context", outPath,
                "--strict");

            Assert.Equal(3, exitCode);
            Assert.Contains("zero projects loaded", stderr);
            Assert.False(File.Exists(outPath));
        }
        finally
        {
            if (File.Exists(outPath))
            {
                File.Delete(outPath);
            }
        }
    }

    [Fact]
    public void the_same_run_without_strict_is_also_zero_projects_loaded()
    {
        var outPath = Path.Combine(Path.GetTempPath(), $"scout-semantic-cli-b1-nostrict-{Guid.NewGuid():N}.json");
        try
        {
            var (exitCode, _, stderr) = Run(
                Path.Combine("fixtures", "csharp-semantic", "Fixture.sln"),
                "--root", Path.Combine("fixtures", "csharp-semantic"),
                "--projects", "NoSuchProjectName",
                "--emit", "context",
                "--context", outPath);

            Assert.Equal(3, exitCode);
            Assert.Contains("zero projects loaded", stderr);
            Assert.False(File.Exists(outPath));
        }
        finally
        {
            if (File.Exists(outPath))
            {
                File.Delete(outPath);
            }
        }
    }

    [Fact]
    public void a_projects_filter_excludes_a_solution_declared_project_that_never_loaded()
    {
        // M3: Vanished is named in Fixture.sln but has no project file on disk,
        // so it never reaches load.Projects/Unsupported/Excluded/Filtered at
        // all -- it is caught only by the solution cross-check. Before the
        // fix that check always labelled it failed/project-not-loaded, even
        // when this same --projects filter would also have excluded it by
        // name; it must now read excluded/not-requested, the same as Clean
        // and Broken. No --strict here: Broken's
        // own workspace diagnostic (a missing ProjectReference) comes from
        // MSBuildWorkspace's whole-solution load, which happens before
        // --projects' own filtering, so a --projects-scoped *.sln run still
        // carries it and trips the pre-existing load-failure check regardless
        // of labelling -- documented in .github/workflows/ci.yml's own
        // "--projects excludes rather than fails a filtered-out project"
        // step, which this test otherwise mirrors.
        var outPath = Path.Combine(Path.GetTempPath(), $"scout-semantic-cli-m3-{Guid.NewGuid():N}.json");
        try
        {
            var (exitCode, _, stderr) = Run(
                Path.Combine("fixtures", "csharp-context", "Fixture.sln"),
                "--root", Path.Combine("fixtures", "csharp-context"),
                "--projects", "Legacy",
                "--tfm", "net9.0",
                "--emit", "context",
                "--context", outPath);

            Assert.True(exitCode == 0, $"expected exit 0, got {exitCode}. stderr:\n{stderr}");
            Assert.True(File.Exists(outPath));

            using var document = JsonDocument.Parse(File.ReadAllText(outPath));
            foreach (var name in new[] { "Clean", "Broken", "Vanished" })
            {
                var record = document.RootElement.GetProperty("compilations").EnumerateArray()
                    .Single(r => r.GetProperty("identity").GetProperty("projectName").GetString() == name);
                Assert.Equal("excluded", record.GetProperty("state").GetString());
                Assert.Equal("not-requested", record.GetProperty("reason").GetString());
            }
        }
        finally
        {
            if (File.Exists(outPath))
            {
                File.Delete(outPath);
            }
        }
    }
}
