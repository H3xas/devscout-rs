using System.Text.Json;

using Xunit;

namespace ScoutSemanticTests;

/// <summary>
/// The tool's own dependency manifest, the one <c>dotnet scout-semantic.dll</c> binds from. No
/// MSBuild-family library may carry a runtime asset there: MSBuildLocator can redirect only an
/// assembly the tool does not ship itself, so an app-local copy wins over the registered SDK's
/// newer one and fails on the first member it lacks.
/// </summary>
public sealed class MsBuildRuntimeAssetsTests
{
    private static readonly string ManifestPath = Path.Combine(AppContext.BaseDirectory, "scout-semantic.deps.json");

    private static bool IsMsBuildFamily(string packageId)
    {
        if (packageId.Equals("Microsoft.NET.StringTools", StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        if (packageId.Equals("Microsoft.Build.Locator", StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        return packageId.Equals("Microsoft.Build", StringComparison.OrdinalIgnoreCase)
            || packageId.StartsWith("Microsoft.Build.", StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void no_msbuild_family_library_ships_a_runtime_asset()
    {
        Assert.True(File.Exists(ManifestPath), $"the tool's dependency manifest is missing: {ManifestPath}");
        using var document = JsonDocument.Parse(File.ReadAllText(ManifestPath));

        var offenders = new SortedSet<string>(StringComparer.Ordinal);
        var familySeen = new SortedSet<string>(StringComparer.Ordinal);
        foreach (var target in document.RootElement.GetProperty("targets").EnumerateObject())
        {
            foreach (var library in target.Value.EnumerateObject())
            {
                var packageId = library.Name.Split('/')[0];
                if (!IsMsBuildFamily(packageId))
                {
                    continue;
                }

                familySeen.Add(packageId);
                if (library.Value.TryGetProperty("runtime", out var runtime)
                    && runtime.ValueKind == JsonValueKind.Object
                    && runtime.EnumerateObject().Any())
                {
                    offenders.Add($"{library.Name} ({string.Join(", ", runtime.EnumerateObject().Select(a => a.Name))})");
                }
            }
        }

        // Guards against a manifest shape change that would let the check pass vacuously.
        Assert.Contains("Microsoft.Build", familySeen);
        Assert.True(
            offenders.Count == 0,
            "MSBuild-family libraries the tool ships instead of the registered SDK: " + string.Join("; ", offenders));
    }

    [Fact]
    public void the_locator_itself_still_ships()
    {
        using var document = JsonDocument.Parse(File.ReadAllText(ManifestPath));

        var locatorAssets = document.RootElement.GetProperty("targets").EnumerateObject()
            .SelectMany(target => target.Value.EnumerateObject())
            .Where(library => library.Name.StartsWith("Microsoft.Build.Locator/", StringComparison.Ordinal))
            .Where(library => library.Value.TryGetProperty("runtime", out var runtime) && runtime.EnumerateObject().Any())
            .ToList();

        Assert.NotEmpty(locatorAssets);
    }
}
