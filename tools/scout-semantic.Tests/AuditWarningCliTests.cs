using System.Text.Json.Nodes;

using Xunit;

using static ScoutSemanticTests.OfflineRestoreCliTests;

namespace ScoutSemanticTests;

/// <summary>
/// The built CLI over a consumer restored offline whose assets file then carries one audit-style
/// log entry. The SDK replays that entry at design time as a workspace failure naming the
/// project, which is the path a real offline vulnerability audit takes; the entry's code and
/// level decide whether the compilation and <c>--strict</c> treat it as a warning or a failure.
/// </summary>
public sealed class AuditWarningCliTests
{
    private const string InjectedMessage =
        "Error occurred while getting package vulnerability data: the feed used by this test is unreachable.";

    private sealed record Run(int ExitCode, string StdErr, JsonNode Compilation, byte[] AssetsBefore, byte[] AssetsAfter);

    private static Run RunOver(string code, string level, bool noRestore)
    {
        var label = Guid.NewGuid().ToString("N");
        var root = Path.Combine(Path.GetTempPath(), $"scout-semantic-audit-cli-{label}");
        var gpf = Path.Combine(Path.GetTempPath(), $"scout-semantic-audit-cli-gpf-{label}");
        var outPath = Path.Combine(Path.GetTempPath(), $"scout-semantic-audit-cli-out-{label}.json");
        try
        {
            Directory.CreateDirectory(gpf);
            WriteStubPackage(gpf);
            var project = WriteConsumerProject(root);
            var restore = RunDotnetRestore(project, gpf, gpf);
            Assert.True(restore.ExitCode == 0, $"setup restore failed: {restore.Output}");

            var assetsPath = Path.Combine(root, "obj", "project.assets.json");
            var assets = JsonNode.Parse(File.ReadAllText(assetsPath))!;
            assets["logs"] = new JsonArray(new JsonObject
            {
                ["code"] = code,
                ["level"] = level,
                ["warningLevel"] = 1,
                ["message"] = InjectedMessage,
            });
            File.WriteAllText(assetsPath, assets.ToJsonString());
            var before = File.ReadAllBytes(assetsPath);

            var args = new List<string> { "Consumer.csproj", "--root", ".", "--emit", "context", "--context", outPath, "--tfm", Tfm, "--strict" };
            if (noRestore)
            {
                args.Add("--no-restore");
            }

            var (exitCode, _, stderr, _) = RunCli(root, gpf, args.ToArray());
            var context = JsonNode.Parse(File.ReadAllText(outPath))!;
            var compilation = context["compilations"]!.AsArray().Single()!;
            return new Run(exitCode, stderr, compilation.DeepClone(), before, File.ReadAllBytes(assetsPath));
        }
        finally
        {
            DeleteAll(root, gpf, outPath);
        }
    }

    private static void AssertReplayedFailureRecorded(Run run)
    {
        var workspace = run.Compilation["diagnostics"]!["workspace"]!.AsArray();
        var diagnostic = Assert.Single(workspace)!;
        Assert.Equal("Failure", diagnostic["kind"]!.GetValue<string>());
        Assert.EndsWith(InjectedMessage, diagnostic["message"]!.GetValue<string>(), StringComparison.Ordinal);
        Assert.Equal(run.AssetsBefore, run.AssetsAfter);
    }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public void an_audit_warning_in_the_assets_file_keeps_the_compilation_complete_under_strict(bool noRestore)
    {
        var run = RunOver("NU1900", "Warning", noRestore);

        Assert.True(run.ExitCode == 0, $"expected exit 0, got {run.ExitCode}. stderr:\n{run.StdErr}");
        Assert.Equal("complete", run.Compilation["state"]!.GetValue<string>());
        AssertReplayedFailureRecorded(run);
    }

    [Theory]
    [InlineData("NU1603", "Warning", false)]
    [InlineData("NU1603", "Warning", true)]
    [InlineData("NU1900", "Error", true)]
    public void any_other_code_or_level_still_demotes_and_fails_strict(string code, string level, bool noRestore)
    {
        var run = RunOver(code, level, noRestore);

        Assert.True(run.ExitCode == 2, $"expected exit 2, got {run.ExitCode}. stderr:\n{run.StdErr}");
        Assert.Equal("partial", run.Compilation["state"]!.GetValue<string>());
        Assert.Equal("workspace-failure", run.Compilation["reason"]!.GetValue<string>());
        AssertReplayedFailureRecorded(run);
    }
}
