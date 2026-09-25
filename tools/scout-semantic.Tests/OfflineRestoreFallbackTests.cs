using ScoutSemantic;

using Xunit;

namespace ScoutSemanticTests;

/// <summary>
/// The choice between the in-process assets-file evaluation and the SDK's own
/// <c>dotnet msbuild</c>, driven with stand-in evaluators so no MSBuild is loaded. A newer
/// registered MSBuild fails in process with binding errors, not only with project errors, and
/// every one of them has to reach the child-process evaluation instead of ending the run.
/// </summary>
public sealed class OfflineRestoreFallbackTests
{
    private const string OutOfProcessValue = "/out-of-process/obj/project.assets.json";

    public static TheoryData<Exception> InProcessFailures => new()
    {
        new MissingMethodException("Microsoft.NET.StringTools.SpanBasedStringBuilder", "Equals"),
        new TypeLoadException("Microsoft.Build.Evaluation.Expander could not be loaded"),
        new FileLoadException("Microsoft.Build, Version=17.7.2.0"),
        new InvalidOperationException("the project could not be evaluated"),
    };

    [Theory]
    [MemberData(nameof(InProcessFailures))]
    public void an_in_process_failure_returns_the_out_of_process_value(Exception failure)
    {
        var value = OfflineRestore.EvaluateWithFallback(() => throw failure, () => OutOfProcessValue);

        Assert.Equal(OutOfProcessValue, value);
    }

    [Fact]
    public void an_in_process_success_never_starts_the_out_of_process_evaluation()
    {
        var outOfProcessCalls = 0;

        var value = OfflineRestore.EvaluateWithFallback(
            () => "/in-process/obj/project.assets.json",
            () =>
            {
                outOfProcessCalls++;
                return OutOfProcessValue;
            });

        Assert.Equal("/in-process/obj/project.assets.json", value);
        Assert.Equal(0, outOfProcessCalls);
    }

    [Fact]
    public void an_empty_in_process_answer_stays_empty_without_a_child_process()
    {
        var outOfProcessCalls = 0;

        var value = OfflineRestore.EvaluateWithFallback(
            () => null,
            () =>
            {
                outOfProcessCalls++;
                return OutOfProcessValue;
            });

        Assert.Null(value);
        Assert.Equal(0, outOfProcessCalls);
    }

    [Fact]
    public void both_evaluations_failing_leaves_the_location_unknown()
    {
        var value = OfflineRestore.EvaluateWithFallback(
            () => throw new MissingMethodException("Microsoft.NET.StringTools.SpanBasedStringBuilder", "Equals"),
            () => null);

        Assert.Null(value);
    }

    [Fact]
    public void running_out_of_memory_in_process_is_not_swallowed()
    {
        var outOfProcessCalls = 0;

        Assert.Throws<OutOfMemoryException>(() => OfflineRestore.EvaluateWithFallback(
            () => throw new OutOfMemoryException(),
            () =>
            {
                outOfProcessCalls++;
                return OutOfProcessValue;
            }));
        Assert.Equal(0, outOfProcessCalls);
    }
}
