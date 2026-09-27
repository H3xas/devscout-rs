using System.Text.Json;

using Microsoft.CodeAnalysis;

using ScoutSemantic;

using Xunit;

namespace ScoutSemanticTests;

/// <summary>
/// The join that tells a NuGet vulnerability-audit warning apart from a real workspace failure.
/// MSBuildWorkspace hands the producer only a kind and a wrapped message, so the code and level
/// come from the project's own assets file; every case here builds that file on disk and a
/// diagnostic by hand, with placeholder project paths, so no MSBuild or restore is involved.
/// </summary>
public sealed class NuGetAuditWarningTests : IDisposable
{
    private const string AppPath = "/repo/src/App/App.csproj";
    private const string LibPath = "/repo/src/Lib/Lib.csproj";
    private const string FeedMessage =
        "Error occurred while getting package vulnerability data: Unable to load the service index for source https://api.nuget.org/v3/index.json.";

    private readonly string dir = Path.Combine(Path.GetTempPath(), $"scout-semantic-audit-join-{Guid.NewGuid():N}");

    public NuGetAuditWarningTests() => Directory.CreateDirectory(dir);

    public void Dispose()
    {
        if (Directory.Exists(dir))
        {
            Directory.Delete(dir, recursive: true);
        }
    }

    private static WorkspaceDiagnostic Failure(string projectPath, string message) =>
        new(WorkspaceDiagnosticKind.Failure, $"Msbuild failed when processing the file '{projectPath}' with message: {message}");

    private static object Entry(string code, string level, object? message) =>
        new Dictionary<string, object?> { ["code"] = code, ["level"] = level, ["warningLevel"] = 1, ["message"] = message };

    private string WriteAssets(string name, string json)
    {
        var path = Path.Combine(dir, name);
        File.WriteAllText(path, json);
        return path;
    }

    private string WriteLogs(string name, params object[] entries) =>
        WriteAssets(name, JsonSerializer.Serialize(new Dictionary<string, object> { ["version"] = 3, ["logs"] = entries }));

    /// <summary>A join over the given project-to-assets-file map, counting every locator call.</summary>
    private static (AuditWarningJoin Join, Dictionary<string, int> Calls) JoinOver(Dictionary<string, string?> assetsByProject)
    {
        var calls = new Dictionary<string, int>(StringComparer.Ordinal);
        var projects = assetsByProject.Keys.ToDictionary(p => p, _ => (string?)"net9.0", StringComparer.Ordinal);
        var join = new AuditWarningJoin(projects, (path, tfm) =>
        {
            Assert.Equal("net9.0", tfm);
            calls[path] = calls.GetValueOrDefault(path) + 1;
            return assetsByProject.GetValueOrDefault(path);
        });
        return (join, calls);
    }

    private (AuditWarningJoin Join, Dictionary<string, int> Calls) AppJoin(string? assetsFile) =>
        JoinOver(new Dictionary<string, string?>(StringComparer.Ordinal) { [AppPath] = assetsFile });

    private bool IsAuditWarning(string? assetsFile, WorkspaceDiagnostic diagnostic)
    {
        var (join, _) = AppJoin(assetsFile);
        var demotes = join.DemotesRecord(new[] { diagnostic }, AppPath);
        Assert.Equal(demotes, join.IsLoadFailure(diagnostic));
        return !demotes;
    }

    [Theory]
    [InlineData("NU1900")]
    [InlineData("NU1901")]
    [InlineData("NU1902")]
    [InlineData("NU1903")]
    [InlineData("NU1904")]
    [InlineData("NU1905")]
    public void every_audit_code_logged_as_a_warning_with_the_same_message_is_an_audit_warning(string code)
    {
        var assets = WriteLogs("project.assets.json", Entry(code, "Warning", FeedMessage));

        Assert.True(IsAuditWarning(assets, Failure(AppPath, FeedMessage)));
    }

    [Fact]
    public void the_same_entry_logged_as_an_error_still_demotes()
    {
        var assets = WriteLogs("project.assets.json", Entry("NU1900", "Error", FeedMessage));

        Assert.False(IsAuditWarning(assets, Failure(AppPath, FeedMessage)));
    }

    [Theory]
    [InlineData("NU1906")]
    [InlineData("NU1603")]
    [InlineData("NU1899")]
    [InlineData("NU19000")]
    public void a_warning_under_any_other_code_still_demotes(string code)
    {
        var assets = WriteLogs("project.assets.json", Entry(code, "Warning", FeedMessage));

        Assert.False(IsAuditWarning(assets, Failure(AppPath, FeedMessage)));
    }

    [Fact]
    public void the_code_is_compared_exactly_and_the_level_without_case()
    {
        var lowerCode = WriteLogs("lower-code.json", Entry("nu1900", "Warning", FeedMessage));
        var lowerLevel = WriteLogs("lower-level.json", Entry("NU1900", "warning", FeedMessage));

        Assert.False(IsAuditWarning(lowerCode, Failure(AppPath, FeedMessage)));
        Assert.True(IsAuditWarning(lowerLevel, Failure(AppPath, FeedMessage)));
    }

    [Theory]
    [InlineData(0)]
    [InlineData(60)]
    [InlineData(-1)]
    public void a_message_that_differs_by_one_character_still_demotes(int position)
    {
        var index = position < 0 ? FeedMessage.Length - 1 : position;
        var altered = FeedMessage[..index] + (FeedMessage[index] == 'x' ? 'y' : 'x') + FeedMessage[(index + 1)..];
        var assets = WriteLogs("project.assets.json", Entry("NU1900", "Warning", altered));

        Assert.False(IsAuditWarning(assets, Failure(AppPath, FeedMessage)));
    }

    [Fact]
    public void an_entry_message_longer_than_the_diagnostic_still_demotes()
    {
        var diagnostic = Failure(AppPath, FeedMessage);
        var assets = WriteLogs("project.assets.json", Entry("NU1900", "Warning", $"Warning: {diagnostic.Message}"));

        Assert.False(IsAuditWarning(assets, diagnostic));
    }

    [Fact]
    public void an_empty_missing_or_non_string_message_never_matches()
    {
        var empty = WriteLogs("empty.json", Entry("NU1900", "Warning", ""));
        var missing = WriteAssets("missing.json", """{"logs":[{"code":"NU1900","level":"Warning"}]}""");
        var number = WriteLogs("number.json", Entry("NU1900", "Warning", 1900));

        foreach (var assets in new[] { empty, missing, number })
        {
            Assert.False(IsAuditWarning(assets, Failure(AppPath, FeedMessage)), assets);
        }
    }

    [Fact]
    public void a_message_is_matched_only_by_its_exact_characters()
    {
        const string accented = "Échec de la récupération des données de vulnérabilité : flux injoignable.";
        const string multiline = "first line of the audit text\nsecond line of the audit text";
        var assets = WriteLogs(
            "project.assets.json", Entry("NU1901", "Warning", accented), Entry("NU1902", "Warning", multiline));

        Assert.True(IsAuditWarning(assets, Failure(AppPath, accented)));
        Assert.True(IsAuditWarning(assets, Failure(AppPath, multiline)));
        Assert.False(IsAuditWarning(assets, Failure(AppPath, accented.Normalize(System.Text.NormalizationForm.FormD))));
        Assert.False(IsAuditWarning(assets, Failure(AppPath, multiline.Replace("\n", "\r\n"))));
    }

    [Fact]
    public void an_assets_file_that_cannot_be_read_as_logs_always_demotes()
    {
        var unreadable = new[]
        {
            Path.Combine(dir, "absent.json"),
            Directory.CreateDirectory(Path.Combine(dir, "a-directory")).FullName,
            WriteAssets("not-json.json", "this is not json"),
            WriteAssets("array-root.json", "[]"),
            WriteAssets("string-root.json", "\"logs\""),
            WriteAssets("no-logs.json", "{}"),
            WriteAssets("object-logs.json", """{"logs":{}}"""),
            WriteAssets("empty.json", ""),
        };

        foreach (var assets in unreadable)
        {
            Assert.False(IsAuditWarning(assets, Failure(AppPath, FeedMessage)), assets);
        }
    }

    [Fact]
    public void malformed_entries_are_skipped_and_a_later_matching_entry_still_counts()
    {
        var assets = WriteAssets("mixed.json", $$"""
            {"logs":[
              "a bare string",
              {"code":1900,"level":"Warning","message":{{JsonSerializer.Serialize(FeedMessage)}}},
              {"code":"NU1900","level":["Warning"],"message":{{JsonSerializer.Serialize(FeedMessage)}}},
              {"code":"NU1900","level":"Warning","message":{{JsonSerializer.Serialize(FeedMessage)}}}
            ]}
            """);
        var onlyMalformed = WriteAssets("malformed.json", $$"""
            {"logs":[
              "a bare string",
              {"code":1900,"level":"Warning","message":{{JsonSerializer.Serialize(FeedMessage)}}}
            ]}
            """);

        Assert.True(IsAuditWarning(assets, Failure(AppPath, FeedMessage)));
        Assert.False(IsAuditWarning(onlyMalformed, Failure(AppPath, FeedMessage)));
    }

    [Fact]
    public void no_locatable_assets_file_demotes()
    {
        Assert.False(IsAuditWarning(null, Failure(AppPath, FeedMessage)));
    }

    [Fact]
    public void a_locator_that_throws_demotes_without_escaping()
    {
        var projects = new Dictionary<string, string?>(StringComparer.Ordinal) { [AppPath] = "net9.0" };
        var join = new AuditWarningJoin(projects, (_, _) => throw new InvalidOperationException("evaluation failed"));
        var diagnostic = Failure(AppPath, FeedMessage);

        Assert.True(join.DemotesRecord(new[] { diagnostic }, AppPath));
        Assert.True(join.IsLoadFailure(diagnostic));
    }

    [Fact]
    public void an_entry_only_in_another_projects_assets_file_demotes()
    {
        var appAssets = WriteLogs("app.json", Entry("NU1603", "Warning", "unrelated"));
        var libAssets = WriteLogs("lib.json", Entry("NU1900", "Warning", FeedMessage));
        var (join, _) = JoinOver(new Dictionary<string, string?>(StringComparer.Ordinal)
        {
            [AppPath] = appAssets,
            [LibPath] = libAssets,
        });
        var diagnostic = Failure(AppPath, FeedMessage);

        Assert.True(join.DemotesRecord(new[] { diagnostic }, AppPath));
        Assert.True(join.IsLoadFailure(diagnostic));
    }

    [Fact]
    public void a_diagnostic_that_names_no_workspace_project_is_a_load_failure()
    {
        var assets = WriteLogs("app.json", Entry("NU1900", "Warning", FeedMessage));
        var (join, calls) = AppJoin(assets);
        var diagnostic = Failure("/elsewhere/Other.csproj", FeedMessage);

        Assert.True(join.IsLoadFailure(diagnostic));
        Assert.Empty(calls);
    }

    [Fact]
    public void a_diagnostic_naming_two_projects_is_ignored_only_when_it_matches_both()
    {
        var both = new WorkspaceDiagnostic(
            WorkspaceDiagnosticKind.Failure,
            $"Msbuild failed when processing the file '{AppPath}' (referenced by '{LibPath}') with message: {FeedMessage}");
        var appOnly = JoinOver(new Dictionary<string, string?>(StringComparer.Ordinal)
        {
            [AppPath] = WriteLogs("app.json", Entry("NU1900", "Warning", FeedMessage)),
            [LibPath] = WriteLogs("lib.json", Entry("NU1603", "Warning", "unrelated")),
        }).Join;
        var matchesBoth = JoinOver(new Dictionary<string, string?>(StringComparer.Ordinal)
        {
            [AppPath] = WriteLogs("app2.json", Entry("NU1900", "Warning", FeedMessage)),
            [LibPath] = WriteLogs("lib2.json", Entry("NU1901", "Warning", FeedMessage)),
        }).Join;

        Assert.True(appOnly.IsLoadFailure(both));
        Assert.False(matchesBoth.IsLoadFailure(both));
    }

    [Fact]
    public void a_record_with_only_audit_warnings_is_not_demoted()
    {
        var assets = WriteLogs(
            "project.assets.json", Entry("NU1900", "Warning", FeedMessage), Entry("NU1903", "Warning", "Package 'X' 1.0.0 has a known high severity vulnerability"));
        var (join, _) = AppJoin(assets);
        var own = new[]
        {
            Failure(AppPath, FeedMessage),
            Failure(AppPath, "Package 'X' 1.0.0 has a known high severity vulnerability"),
        };

        Assert.False(join.DemotesRecord(own, AppPath));
        Assert.DoesNotContain(own, join.IsLoadFailure);
    }

    [Fact]
    public void an_audit_warning_next_to_one_real_failure_still_demotes()
    {
        var assets = WriteLogs("project.assets.json", Entry("NU1900", "Warning", FeedMessage));
        var (join, _) = AppJoin(assets);
        var real = new WorkspaceDiagnostic(
            WorkspaceDiagnosticKind.Failure,
            $"Msbuild failed when processing the file '{AppPath}' with message: The referenced project '../Missing/Missing.csproj' does not exist.");
        var own = new[] { Failure(AppPath, FeedMessage), real };

        Assert.True(join.DemotesRecord(own, AppPath));
        Assert.True(own.Any(join.IsLoadFailure));
        Assert.False(join.IsLoadFailure(own[0]));
    }

    [Fact]
    public void a_warning_kind_diagnostic_never_demotes_and_is_never_a_load_failure()
    {
        var (join, calls) = AppJoin(null);
        var warning = new WorkspaceDiagnostic(
            WorkspaceDiagnosticKind.Warning, $"Found project reference without a matching metadata reference: {AppPath}");

        Assert.False(join.DemotesRecord(new[] { warning }, AppPath));
        Assert.False(join.IsLoadFailure(warning));
        Assert.Empty(calls);
    }

    [Fact]
    public void the_assets_file_is_located_only_for_a_failure_and_once_per_project()
    {
        var assets = WriteLogs("project.assets.json", Entry("NU1900", "Warning", FeedMessage));
        var (join, calls) = AppJoin(assets);

        Assert.False(join.DemotesRecord(Array.Empty<WorkspaceDiagnostic>(), AppPath));
        Assert.Empty(calls);

        var first = Failure(AppPath, FeedMessage);
        var second = Failure(AppPath, "Package 'X' 1.0.0 has a known high severity vulnerability");
        join.DemotesRecord(new[] { first, second }, AppPath);
        join.IsLoadFailure(first);
        join.IsLoadFailure(second);

        Assert.Equal(1, calls[AppPath]);
    }
}
