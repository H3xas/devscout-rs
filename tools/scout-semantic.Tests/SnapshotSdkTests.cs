using System.Diagnostics;
using Xunit;

namespace ScoutSemanticTests;

/// <summary>
/// The refusals <see cref="SnapshotSdk"/> makes before any project-building test can run. A
/// missing or wrong install has to stop the run with a message naming what is wrong, because the
/// alternative -- quietly selecting whatever SDK the host would have chosen -- is what moves the
/// recorded SDK version and every reference identity away from the pinned install.
/// </summary>
public sealed class SnapshotSdkTests
{
    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    public void an_unset_variable_is_refused_with_the_variable_named(string? configured)
    {
        var error = Assert.Throws<InvalidOperationException>(() => SnapshotSdk.Validate(configured));

        Assert.Contains(SnapshotSdk.RootVariable, error.Message, StringComparison.Ordinal);
        Assert.Contains(SnapshotSdk.SdkVersion, error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void a_path_that_is_not_a_directory_is_refused()
    {
        var error = Assert.Throws<InvalidOperationException>(
            () => SnapshotSdk.Validate(Path.Combine(Path.GetTempPath(), $"absent-{Guid.NewGuid():N}")));

        Assert.Contains("is not a directory", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void a_directory_without_a_dotnet_host_is_refused()
    {
        var directory = Path.Combine(Path.GetTempPath(), $"scout-semantic-no-host-{Guid.NewGuid():N}");
        Directory.CreateDirectory(directory);
        try
        {
            var error = Assert.Throws<InvalidOperationException>(() => SnapshotSdk.Validate(directory));

            Assert.Contains("carries no dotnet host", error.Message, StringComparison.Ordinal);
        }
        finally
        {
            Directory.Delete(directory, recursive: true);
        }
    }

    [Fact]
    public void the_selected_root_is_the_environment_variables_own()
    {
        Assert.Equal(
            Path.GetFullPath(Environment.GetEnvironmentVariable(SnapshotSdk.RootVariable)!),
            SnapshotSdk.Root);
    }

    [Fact]
    public void the_reference_pack_directory_belongs_to_the_selected_root()
    {
        Assert.StartsWith(SnapshotSdk.Root, SnapshotSdk.ReferenceAssemblyDirectory, StringComparison.Ordinal);
        Assert.NotEmpty(SnapshotSdk.ReferenceAssemblyPaths());
    }

    [Fact]
    public void the_pin_declares_the_version_the_selected_root_lists()
    {
        Assert.Matches(@"^\d+\.\d+\.\d+$", SnapshotSdk.SdkVersion);
    }

    /// <summary>A child this helper starts must not carry the launching host's own SDK selection.
    /// The host that runs <c>dotnet test</c> exports where its targets and tasks live, and it does
    /// not overwrite a value it finds already set, so a suite launched by a newer SDK would point
    /// the pinned child at the wrong <c>Sdks</c> directory. The decoys below stand in for that
    /// launcher; the child's own MSBuild run is what proves the pinned install won.
    /// </summary>
    [Fact]
    public void a_child_drops_the_launchers_own_sdk_selection()
    {
        var directory = Path.Combine(Path.GetTempPath(), $"scout-semantic-sdk-selection-{Guid.NewGuid():N}");
        Directory.CreateDirectory(directory);
        try
        {
            var decoy = Path.Combine(directory, "launcher-sdk");
            var output = Path.Combine(directory, "sdk-path.txt");
            File.WriteAllText(Path.Combine(directory, "probe.proj"), """
                <Project>
                  <Target Name="WriteSdkPath">
                    <WriteLinesToFile File="$(Out)" Lines="$(MSBuildSDKsPath)" Overwrite="true" />
                  </Target>
                </Project>
                """);

            var psi = new ProcessStartInfo(SnapshotSdk.DotnetPath)
            {
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                WorkingDirectory = directory,
            };
            foreach (var variable in SnapshotSdk.SdkSelectionVariables)
            {
                psi.Environment[variable] = Path.Combine(decoy, variable);
            }

            SnapshotSdk.ApplyTo(psi);
            foreach (var variable in SnapshotSdk.SdkSelectionVariables)
            {
                Assert.DoesNotContain(variable, psi.Environment.Keys);
            }

            psi.ArgumentList.Add("msbuild");
            psi.ArgumentList.Add("probe.proj");
            psi.ArgumentList.Add("-nologo");
            psi.ArgumentList.Add("-t:WriteSdkPath");
            psi.ArgumentList.Add($"-p:Out={output}");
            var (exitCode, stderr) = RunProbe(psi);

            Assert.True(exitCode == 0, $"the child exited {exitCode}: {stderr}");
            Assert.Equal(
                Path.Combine(SnapshotSdk.Root, "sdk", SnapshotSdk.SdkVersion, "Sdks"),
                File.ReadAllText(output).Trim());
        }
        finally
        {
            Directory.Delete(directory, recursive: true);
        }
    }

    private static (int ExitCode, string StdErr) RunProbe(ProcessStartInfo psi)
    {
        using var process = Process.Start(psi) ?? throw new InvalidOperationException("failed to start dotnet");
        var stderrTask = process.StandardError.ReadToEndAsync();
        process.StandardOutput.ReadToEnd();
        process.WaitForExit();
        return (process.ExitCode, stderrTask.Result);
    }
}
