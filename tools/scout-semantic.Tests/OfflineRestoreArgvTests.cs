using ScoutSemantic;

using Xunit;

namespace ScoutSemanticTests;

/// <summary>
/// The pieces of the offline restore step that need no real project, no
/// workspace and no package: the exact argv one restore invocation carries,
/// and the two filesystem lookups (the <c>dotnet</c> host, the global
/// packages folder) that decide where it points.
/// </summary>
public sealed class OfflineRestoreArgvTests
{
    private static readonly string[] Pinned =
    {
        "-p:NuGetAudit=false", "-p:RestoreSources=/tmp/gpf", "-p:RestoreAdditionalProjectSources=",
    };

    [Fact]
    public void restore_argv_carries_only_the_local_source_and_the_runs_own_properties()
    {
        var properties = new Dictionary<string, string> { ["Configuration"] = "Debug", ["Platform"] = "AnyCPU" };

        var args = OfflineRestore.BuildRestoreArguments("/tmp/Proj.csproj", "/tmp/gpf", properties);

        Assert.Equal(
            new[]
            {
                "restore", "/tmp/Proj.csproj", "--no-dependencies", "--source", "/tmp/gpf",
                "-p:Configuration=Debug", "-p:Platform=AnyCPU",
            }.Concat(Pinned),
            args);
    }

    [Fact]
    public void restore_argv_names_exactly_one_source_and_no_other_feed_shape()
    {
        // A second --source, a feed URL, or an interactive credential
        // prompt anywhere in the argv would widen the restore beyond the
        // global packages folder.
        var args = OfflineRestore.BuildRestoreArguments(
            "/tmp/Proj.csproj", "/tmp/gpf", new Dictionary<string, string>());

        Assert.Single(args, a => a == "--source");
        var sourceIndex = args.IndexOf("--source");
        Assert.Equal("/tmp/gpf", args[sourceIndex + 1]);
        Assert.DoesNotContain(args, a => a.Contains("nuget.org", StringComparison.OrdinalIgnoreCase));
        Assert.DoesNotContain(args, a => a.Contains("://", StringComparison.Ordinal));
        Assert.DoesNotContain(args, a => a.Equals("--interactive", StringComparison.Ordinal));
    }

    [Fact]
    public void restore_argv_pins_the_source_properties_after_every_forwarded_property()
    {
        // NuGet reads RestoreSources and RestoreAdditionalProjectSources as
        // ordinary MSBuild properties, and the last -p: of a name wins, so the
        // pinned values must come last and a forwarded value of the same name
        // must not survive to re-widen the source list.
        var properties = new Dictionary<string, string>
        {
            ["Configuration"] = "Debug",
            ["restoresources"] = "https://127.0.0.1:9/v3/index.json",
            ["RestoreAdditionalProjectSources"] = "https://127.0.0.1:7/v3/index.json",
            ["NuGetAudit"] = "true",
        };

        var args = OfflineRestore.BuildRestoreArguments("/tmp/Proj.csproj", "/tmp/gpf", properties);

        Assert.Equal(
            new[] { "restore", "/tmp/Proj.csproj", "--no-dependencies", "--source", "/tmp/gpf", "-p:Configuration=Debug" }
                .Concat(Pinned),
            args);
        Assert.Equal(Pinned, args.TakeLast(Pinned.Length));
        Assert.DoesNotContain(args, a => a.Contains("127.0.0.1", StringComparison.Ordinal));
    }

    [Fact]
    public void restore_argv_with_no_run_properties_carries_only_the_pinned_ones()
    {
        var args = OfflineRestore.BuildRestoreArguments("/tmp/Proj.csproj", "/tmp/gpf", new Dictionary<string, string>());

        Assert.Equal(
            new[] { "restore", "/tmp/Proj.csproj", "--no-dependencies", "--source", "/tmp/gpf" }.Concat(Pinned),
            args);
    }

    [Fact]
    public void the_dotnet_host_is_resolved_by_walking_up_from_the_registered_sdk_path_never_by_guessing_a_bare_name()
    {
        var root = Path.Combine(Path.GetTempPath(), $"scout-semantic-dotnet-host-{Guid.NewGuid():N}");
        var sdkDir = Path.Combine(root, "sdk", "9.0.100");
        Directory.CreateDirectory(sdkDir);
        var hostName = OperatingSystem.IsWindows() ? "dotnet.exe" : "dotnet";
        var hostPath = Path.Combine(root, hostName);
        File.WriteAllText(hostPath, "not a real binary, only its presence matters here");

        try
        {
            var resolved = OfflineRestore.ResolveDotnetHost(sdkDir);

            Assert.Equal(hostPath, resolved);
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public void an_msbuild_path_with_no_dotnet_host_anywhere_above_it_resolves_to_null()
    {
        var root = Path.Combine(Path.GetTempPath(), $"scout-semantic-dotnet-host-missing-{Guid.NewGuid():N}");
        var sdkDir = Path.Combine(root, "sdk", "9.0.100");
        Directory.CreateDirectory(sdkDir);

        try
        {
            Assert.Null(OfflineRestore.ResolveDotnetHost(sdkDir));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public void the_global_packages_folder_honours_nuget_packages_for_the_given_root()
    {
        var root = Path.Combine(Path.GetTempPath(), $"scout-semantic-gpf-root-{Guid.NewGuid():N}");
        var gpf = Path.Combine(Path.GetTempPath(), $"scout-semantic-gpf-target-{Guid.NewGuid():N}");
        Directory.CreateDirectory(root);
        Directory.CreateDirectory(gpf);
        var previous = Environment.GetEnvironmentVariable("NUGET_PACKAGES");
        Environment.SetEnvironmentVariable("NUGET_PACKAGES", gpf);
        try
        {
            var resolved = OfflineRestore.ResolveGlobalPackagesFolder("dotnet", root);

            Assert.NotNull(resolved);
            Assert.Equal(
                Path.GetFullPath(gpf).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar),
                Path.GetFullPath(resolved!).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar));
        }
        finally
        {
            Environment.SetEnvironmentVariable("NUGET_PACKAGES", previous);
            Directory.Delete(root, recursive: true);
            Directory.Delete(gpf, recursive: true);
        }
    }

    [Theory]
    [InlineData("""{"logs":[{"code":"NU1603","level":"Warning","message":"approximate match"},{"code":"NU1101","level":"Error","message":"Unable to find package X."}]}""", "NU1101 -- Unable to find package X.")]
    [InlineData("""{"logs":[{"code":"NU1603","level":"Warning","message":"approximate match"}]}""", null)]
    [InlineData("""{"version":3,"targets":{}}""", null)]
    [InlineData("not json", null)]
    public void an_assets_file_reports_only_its_first_recorded_restore_error(string content, string? expected)
    {
        var path = Path.Combine(Path.GetTempPath(), $"scout-semantic-assets-log-{Guid.NewGuid():N}.json");
        File.WriteAllText(path, content);
        try
        {
            Assert.Equal(expected, OfflineRestore.RecordedRestoreError(path));
        }
        finally
        {
            File.Delete(path);
        }
    }

    [Fact]
    public void out_of_process_evaluation_carries_the_same_global_properties()
    {
        var globals = new Dictionary<string, string> { ["TargetFramework"] = "netstandard2.0", ["Configuration"] = "Debug" };

        var args = OfflineRestore.BuildEvaluationArguments("/tmp/Lib.csproj", globals);

        Assert.Equal(
            new[]
            {
                "msbuild", "/tmp/Lib.csproj", "-nologo", "-getProperty:ProjectAssetsFile",
                "-p:Configuration=Debug", "-p:TargetFramework=netstandard2.0",
            },
            args);
    }
}
