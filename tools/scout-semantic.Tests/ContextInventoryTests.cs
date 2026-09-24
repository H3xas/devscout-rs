using ScoutSemantic;

using Xunit;

namespace ScoutSemanticTests;

/// <summary>
/// <see cref="ContextInventory.NormalizeImportIdentity"/> classifies an
/// import path into one of four identities before it ever reaches the
/// compiler-facts artifact: <c>nuget:</c>, a root-relative
/// <c>external-file:</c> for a repository-authored file, <c>sdk-file:</c>
/// for one owned by the installed SDK, or the bare-name <c>external-file:</c>
/// fallback for anything else this checkout has no path to locate. The
/// caller filters every <c>sdk-file:</c> identity out entirely (an installed
/// SDK's own hundreds of imports move with the SDK version, not this
/// repository), and the Rust consumer's freshness check only re-hashes an
/// <c>external-file:</c> identity, by joining it under the repository root
/// -- so a repository-authored path that collapses to its bare name
/// (indistinguishable from a same-named file at a different depth) or an
/// SDK-owned path that is not recognized here both reach the consumer as an
/// identity it cannot correctly re-hash, and the whole artifact reads stale
/// on every run that touches one. Each path shape below has its own case so
/// a future regression fails a test instead of silently degrading every
/// enriched-lane run to syntax-only.
/// </summary>
public sealed class ContextInventoryTests
{
    private static RepoPaths PathsAt(string root) => new(root, Array.Empty<string>());

    [Fact]
    public void a_workload_manifest_path_is_an_sdk_file_identity()
    {
        // The path shape that first exposed this: an optional .NET workload
        // (MAUI, Android, ...) installs its manifest under `sdk-manifests/`,
        // a sibling of `sdk/` inside the same SDK install root, not a
        // sub-path of it -- the original single `/sdk/` marker never matched
        // it.
        var identity = ContextInventory.NormalizeImportIdentity(
            "/usr/local/share/dotnet/sdk-manifests/8.0.100/microsoft.net.sdk.maui/8.0.3/WorkloadManifest.targets",
            PathsAt("/repo"));

        Assert.StartsWith("sdk-file:", identity);
    }

    [Fact]
    public void an_sdk_compiler_path_is_still_an_sdk_file_identity()
    {
        var identity = ContextInventory.NormalizeImportIdentity(
            "/usr/local/share/dotnet/sdk/9.0.305/Sdks/Microsoft.NET.Sdk/targets/Microsoft.NET.Sdk.targets",
            PathsAt("/repo"));

        Assert.StartsWith("sdk-file:", identity);
    }

    [Fact]
    public void a_nuget_global_package_path_is_a_nuget_identity()
    {
        var identity = ContextInventory.NormalizeImportIdentity(
            "/Users/someone/.nuget/packages/microsoft.net.test.sdk/17.11.1/build/Microsoft.NET.Test.Sdk.props",
            PathsAt("/repo"));

        Assert.StartsWith("nuget:microsoft.net.test.sdk/17.11.1/", identity);
    }

    [Fact]
    public void an_unrecognized_external_path_falls_back_to_its_bare_name()
    {
        var identity = ContextInventory.NormalizeImportIdentity(
            "/Users/someone/repo-parent/Directory.Build.props",
            PathsAt("/repo"));

        Assert.Equal("external-file:Directory.Build.props", identity);
    }

    [Fact]
    public void a_repository_root_import_is_identified_by_its_root_relative_path()
    {
        var identity = ContextInventory.NormalizeImportIdentity(
            "/repo/Directory.Build.props",
            PathsAt("/repo"));

        Assert.Equal("external-file:Directory.Build.props", identity);
    }

    [Fact]
    public void a_nested_override_does_not_collide_with_the_root_file_of_the_same_name()
    {
        // The real-world case this fixes: a subtree's own
        // `Directory.Build.props` overrides the root one and has different
        // content. Both used to normalize to the bare name `Directory.
        // Build.props`, so the consumer's freshness re-hash (which only ever
        // checks the root copy) compared the subtree file's recorded hash
        // against the root file's live content and reported a mismatch --
        // even though nothing had actually changed.
        var rootIdentity = ContextInventory.NormalizeImportIdentity(
            "/repo/Directory.Build.props", PathsAt("/repo"));
        var nestedIdentity = ContextInventory.NormalizeImportIdentity(
            "/repo/services/Directory.Build.props", PathsAt("/repo"));

        Assert.Equal("external-file:Directory.Build.props", rootIdentity);
        Assert.Equal("external-file:services/Directory.Build.props", nestedIdentity);
        Assert.NotEqual(rootIdentity, nestedIdentity);
    }
}
