using ScoutSemantic;

using Xunit;

namespace ScoutSemanticTests;

/// <summary>
/// Every record is validated before <see cref="ContextWriter.Write"/> opens
/// the output stream at all, so a schema violation in a batch leaves
/// whatever was already at the target path completely untouched -- the same
/// validate-then-write invariant the sibling flow-tracer fact document
/// claims but had no direct test proving, until now.
/// </summary>
public sealed class ContextWriterTests
{
    private static ContextRecord Valid(string name) => new()
    {
        Identity = new ContextIdentity { ProjectPath = $"src/{name}/{name}.csproj", ProjectName = name, RequestedTfm = null },
        State = "complete",
        Reason = "complete",
    };

    [Fact]
    public void a_violation_anywhere_in_the_batch_writes_nothing_even_when_it_is_the_last_record()
    {
        var path = Path.Combine(Path.GetTempPath(), $"scout-semantic-context-writer-test-{Guid.NewGuid():N}.json");
        const string sentinel = "sentinel: pre-existing file, must survive a failed write untouched";
        File.WriteAllText(path, sentinel);
        try
        {
            var envelope = new ContextEnvelope
            {
                Producer = "scout-semantic",
                Version = "0.0.0-test",
                Repo = "test",
                Solution = "Test.sln",
                Compilations = new List<ContextRecord>
                {
                    Valid("A"),
                    Valid("B"),
                    new()
                    {
                        Identity = new ContextIdentity { ProjectPath = "src/C/C.csproj", ProjectName = "C", RequestedTfm = null },
                        State = "complete",
                        Reason = "", // the violation: an empty reason, caught only at validation time
                    },
                },
            };

            Assert.Throws<ContextSchemaException>(() => ContextWriter.Write(path, envelope));
            Assert.Equal(sentinel, File.ReadAllText(path));
        }
        finally
        {
            File.Delete(path);
        }
    }

    [Fact]
    public void a_fully_valid_batch_writes_the_envelope()
    {
        var path = Path.Combine(Path.GetTempPath(), $"scout-semantic-context-writer-test-{Guid.NewGuid():N}.json");
        try
        {
            var envelope = new ContextEnvelope
            {
                Producer = "scout-semantic",
                Version = "0.0.0-test",
                Repo = "test",
                Solution = "Test.sln",
                Compilations = new List<ContextRecord> { Valid("A"), Valid("B") },
            };

            ContextWriter.Write(path, envelope);

            Assert.True(File.Exists(path));
            var written = File.ReadAllText(path);
            Assert.Contains("\"schemaVersion\": 1", written);
            Assert.EndsWith("\n", written);
            Assert.DoesNotContain("\r", written);
        }
        finally
        {
            File.Delete(path);
        }
    }

    [Fact]
    public void a_record_with_no_versions_writes_an_explicit_null_rather_than_omitting_the_key()
    {
        var path = Path.Combine(Path.GetTempPath(), $"scout-semantic-context-writer-test-{Guid.NewGuid():N}.json");
        try
        {
            var envelope = new ContextEnvelope
            {
                Producer = "scout-semantic",
                Version = "0.0.0-test",
                Repo = "test",
                Solution = "Test.sln",
                Compilations = new List<ContextRecord>
                {
                    new()
                    {
                        Identity = new ContextIdentity { ProjectPath = "src/A/A.csproj", ProjectName = "A", RequestedTfm = "net48" },
                        State = "unsupported",
                        Reason = "undeclared-target",
                        // Versions intentionally left unset.
                    },
                },
            };

            ContextWriter.Write(path, envelope);

            var written = File.ReadAllText(path);
            Assert.Contains("\"versions\": null", written);
        }
        finally
        {
            File.Delete(path);
        }
    }
}
