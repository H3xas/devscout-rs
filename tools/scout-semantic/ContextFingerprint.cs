namespace ScoutSemantic;

/// <summary>
/// The context fingerprint: one SHA-1 over a fixed-order, newline-joined,
/// ordinal-sorted pre-image built from seven labeled groups, so the same
/// inputs always fold to the same digest and an unrelated change (an
/// `Authors` element, say) never moves it. Reuses <see cref="FactsWriter.Sha1"/>
/// rather than a second hash primitive.
/// </summary>
internal static class ContextFingerprint
{
    /// <summary>
    /// Folds reference identity, import content hashes, build symbols,
    /// binding-relevant language options, SDK/MSBuild/compiler versions, the
    /// project's own narrow build identity, and every project reference's
    /// already-computed fingerprint into one digest.
    /// </summary>
    public static string Compute(
        IEnumerable<string> metadataReferenceIdentities,
        IEnumerable<string> projectReferenceFingerprints,
        IEnumerable<string> importContentHashes,
        IEnumerable<string> preprocessorSymbols,
        IReadOnlyDictionary<string, string?> languageOptions,
        ContextVersions versions,
        string configuration,
        string platform,
        string targetFramework,
        string? assemblyName,
        string? rootNamespace)
    {
        var groups = new[]
        {
            Group("metadata-references", metadataReferenceIdentities),
            Group("project-references", projectReferenceFingerprints),
            Group("imports", importContentHashes),
            Group("preprocessor-symbols", preprocessorSymbols),
            Group(
                "language-options",
                languageOptions
                    .OrderBy(kv => kv.Key, StringComparer.Ordinal)
                    .Select(kv => kv.Key + "=" + (kv.Value ?? ""))),
            Group(
                "versions",
                new[] { versions.Sdk, versions.Msbuild, versions.Compiler }),
            Group(
                "build-identity",
                new[]
                {
                    "configuration=" + configuration,
                    "platform=" + platform,
                    "targetFramework=" + targetFramework,
                    "assemblyName=" + (assemblyName ?? ""),
                    "rootNamespace=" + (rootNamespace ?? ""),
                }),
        };

        return FactsWriter.Sha1(string.Join("\n", groups));
    }

    private static string Group(string label, IEnumerable<string> values) =>
        label + ":" + string.Join("|", values.OrderBy(v => v, StringComparer.Ordinal));
}
