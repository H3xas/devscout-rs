using ScoutSemantic;

using Xunit;

namespace ScoutSemanticTests;

/// <summary>
/// <see cref="ContextFingerprint.Compute"/> folds seven labeled groups into
/// one digest; each test below holds every group fixed but one and proves
/// that group alone moves the result. Property tests, not fixture runs, so
/// the "which single input moved" claim is exact rather than inferred from
/// an end-to-end project evaluation.
/// </summary>
public sealed class ContextFingerprintTests
{
    private static readonly ContextVersions Versions = new()
    {
        Sdk = "9.0.305",
        Msbuild = "9.0.305",
        Compiler = "4.14.0",
        Engine = "0.1.0",
    };

    private static string Compute(
        IEnumerable<string>? metadataReferences = null,
        IEnumerable<string>? projectReferences = null,
        IEnumerable<string>? imports = null,
        IEnumerable<string>? analyzerReferences = null,
        IEnumerable<string>? generatorInputs = null,
        IEnumerable<string>? symbols = null,
        ContextVersions? versions = null,
        string configuration = "Debug",
        string targetFramework = "net9.0") =>
        ContextFingerprint.Compute(
            metadataReferences ?? new[] { "System.Runtime|mvid:00000000-0000-0000-0000-000000000000" },
            projectReferences ?? Array.Empty<string>(),
            imports ?? new[] { "Directory.Build.props|sha1:abc" },
            analyzerReferences ?? Array.Empty<string>(),
            generatorInputs ?? Array.Empty<string>(),
            symbols ?? new[] { "TRACE" },
            new Dictionary<string, string?> { ["languageVersion"] = "13.0", ["nullable"] = "enable" },
            versions ?? Versions,
            configuration,
            "AnyCPU",
            targetFramework,
            "App",
            "App");

    [Fact]
    public void identical_inputs_produce_identical_fingerprints()
    {
        Assert.Equal(Compute(), Compute());
    }

    [Fact]
    public void an_analyzer_reference_alone_moves_the_fingerprint()
    {
        var before = Compute(analyzerReferences: Array.Empty<string>());
        var after = Compute(analyzerReferences: new[] { "Analyzer.dll|sha1:def" });
        Assert.NotEqual(before, after);
    }

    [Fact]
    public void a_generator_input_alone_moves_the_fingerprint()
    {
        var before = Compute(generatorInputs: Array.Empty<string>());
        var after = Compute(generatorInputs: new[] { "Gen.txt|abc123" });
        Assert.NotEqual(before, after);
    }

    [Fact]
    public void a_metadata_reference_alone_moves_the_fingerprint()
    {
        var before = Compute();
        var after = Compute(metadataReferences: new[] { "System.Runtime|mvid:11111111-1111-1111-1111-111111111111" });
        Assert.NotEqual(before, after);
    }

    [Fact]
    public void a_project_reference_fingerprint_alone_moves_the_fingerprint()
    {
        var before = Compute(projectReferences: new[] { "fingerprint-v1" });
        var after = Compute(projectReferences: new[] { "fingerprint-v2" });
        // A dependency compilation's own moved fingerprint propagates.
        Assert.NotEqual(before, after);
    }

    [Fact]
    public void an_import_content_hash_alone_moves_the_fingerprint()
    {
        var before = Compute();
        var after = Compute(imports: new[] { "Directory.Build.props|sha1:changed" });
        Assert.NotEqual(before, after);
    }

    [Fact]
    public void a_preprocessor_symbol_alone_moves_the_fingerprint()
    {
        var before = Compute();
        var after = Compute(symbols: new[] { "TRACE", "PROBE_FLAG" });
        Assert.NotEqual(before, after);
    }

    [Fact]
    public void a_build_configuration_alone_moves_the_fingerprint()
    {
        var before = Compute(configuration: "Debug");
        var after = Compute(configuration: "Release");
        Assert.NotEqual(before, after);
    }

    [Fact]
    public void an_sdk_version_alone_moves_the_fingerprint()
    {
        var otherSdk = new ContextVersions { Sdk = "8.0.121", Msbuild = "8.0.121", Compiler = Versions.Compiler, Engine = Versions.Engine };
        var before = Compute(versions: Versions);
        var after = Compute(versions: otherSdk);
        Assert.NotEqual(before, after);
    }
}
