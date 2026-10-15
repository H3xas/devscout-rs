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
}
