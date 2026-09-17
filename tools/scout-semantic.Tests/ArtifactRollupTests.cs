using ScoutSemantic;

using Xunit;

namespace ScoutSemanticTests;

/// <summary>
/// Exercises <c>Runner.ArtifactRollup</c> directly (unit-level, no fixture
/// run needed): <c>complete</c> only when every non-<c>excluded</c> record
/// is <c>complete</c>, else the worst present state, in
/// <c>failed &gt; unsupported &gt; partial</c> priority. <c>excluded</c>
/// records never affect the rollup.
/// </summary>
public sealed class ArtifactRollupTests
{
    private static ContextRecord With(string state) => new()
    {
        Identity = new ContextIdentity { ProjectPath = "src/App/App.csproj", ProjectName = "App", RequestedTfm = null },
        State = state,
        Reason = state,
    };

    [Fact]
    public void empty_set_is_complete()
    {
        Assert.Equal("complete", Runner.ArtifactRollup(new List<ContextRecord>()));
    }

    [Fact]
    public void all_complete_is_complete()
    {
        var records = new List<ContextRecord> { With("complete"), With("complete") };
        Assert.Equal("complete", Runner.ArtifactRollup(records));
    }

    [Fact]
    public void excluded_records_are_ignored_and_do_not_prevent_complete()
    {
        var records = new List<ContextRecord> { With("complete"), With("excluded"), With("excluded") };
        Assert.Equal("complete", Runner.ArtifactRollup(records));
    }

    [Fact]
    public void any_partial_demotes_to_partial()
    {
        var records = new List<ContextRecord> { With("complete"), With("partial") };
        Assert.Equal("partial", Runner.ArtifactRollup(records));
    }

    [Fact]
    public void unsupported_outranks_partial()
    {
        var records = new List<ContextRecord> { With("partial"), With("unsupported") };
        Assert.Equal("unsupported", Runner.ArtifactRollup(records));
    }

    [Fact]
    public void failed_outranks_unsupported_and_partial()
    {
        var records = new List<ContextRecord> { With("partial"), With("unsupported"), With("failed") };
        Assert.Equal("failed", Runner.ArtifactRollup(records));
    }

    [Fact]
    public void all_excluded_is_complete()
    {
        var records = new List<ContextRecord> { With("excluded") };
        Assert.Equal("complete", Runner.ArtifactRollup(records));
    }
}
