using System.Text.Json;

using ScoutSemantic;

using Xunit;

namespace ScoutSemanticTests;

/// <summary>
/// Exercises <c>CompilerFactsEmitter.Render</c>'s <c>sourceSnapshot</c> block directly
/// (unit-level, no fixture run and no real git checkout needed): freshness has two
/// legs, which requires <c>dirty</c>/<c>dirtyDigest</c> to adopt the exact
/// convention <see cref="GitIdentity"/> already carries for the flow-tracer document,
/// rather than a second, <c>headSha</c>-only scheme that is blind to an uncommitted edit.
/// </summary>
public sealed class CompilerFactsSourceSnapshotTests
{
    private static JsonDocument Render(GitIdentity? git)
    {
        var options = new Options { Root = "unused", CompilerFacts = "unused.json" };
        var acc = new CompilerFactsAccumulator();
        var bytes = CompilerFactsEmitter.Render(options, acc, git, new List<ContextRecord>(), occurrences: null);
        return JsonDocument.Parse(bytes);
    }

    [Fact]
    public void no_git_identity_omits_source_snapshot_entirely()
    {
        using var doc = Render(git: null);
        Assert.False(
            doc.RootElement.TryGetProperty("sourceSnapshot", out _),
            "matches --no-git: no source-snapshot identity is stamped");
    }

    [Fact]
    public void a_clean_checkout_carries_a_false_dirty_flag_and_its_own_digest()
    {
        var git = new GitIdentity
        {
            HeadSha = "abc123",
            Dirty = false,
            DirtyDigest = FactsWriter.Sha1(""),
            FileCount = 3,
        };

        using var doc = Render(git);
        var snapshot = doc.RootElement.GetProperty("sourceSnapshot");
        Assert.Equal("abc123", snapshot.GetProperty("headSha").GetString());
        Assert.False(snapshot.GetProperty("dirty").GetBoolean());
        Assert.Equal(FactsWriter.Sha1(""), snapshot.GetProperty("dirtyDigest").GetString());
    }

    [Fact]
    public void a_dirty_checkout_carries_a_true_dirty_flag_and_a_digest_of_its_porcelain_lines()
    {
        var git = new GitIdentity
        {
            HeadSha = "abc123",
            Dirty = true,
            DirtyDigest = FactsWriter.Sha1(" M Widgets.cs"),
            FileCount = 3,
        };

        using var doc = Render(git);
        var snapshot = doc.RootElement.GetProperty("sourceSnapshot");
        Assert.Equal("abc123", snapshot.GetProperty("headSha").GetString());
        Assert.True(snapshot.GetProperty("dirty").GetBoolean());
        Assert.Equal(FactsWriter.Sha1(" M Widgets.cs"), snapshot.GetProperty("dirtyDigest").GetString());
    }

    [Fact]
    public void the_same_head_sha_with_different_dirty_state_carries_a_different_digest()
    {
        var clean = new GitIdentity { HeadSha = "abc123", Dirty = false, DirtyDigest = FactsWriter.Sha1(""), FileCount = 1 };
        var dirty = new GitIdentity { HeadSha = "abc123", Dirty = true, DirtyDigest = FactsWriter.Sha1(" M X.cs"), FileCount = 1 };

        using var cleanDoc = Render(clean);
        using var dirtyDoc = Render(dirty);
        var cleanSnapshot = cleanDoc.RootElement.GetProperty("sourceSnapshot");
        var dirtySnapshot = dirtyDoc.RootElement.GetProperty("sourceSnapshot");

        Assert.Equal(cleanSnapshot.GetProperty("headSha").GetString(), dirtySnapshot.GetProperty("headSha").GetString());
        Assert.NotEqual(
            cleanSnapshot.GetProperty("dirtyDigest").GetString(),
            dirtySnapshot.GetProperty("dirtyDigest").GetString());
    }
}
