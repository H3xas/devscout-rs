// Case d: exercises FakeServer.GetAsync from a test method (see FakeServer.cs, also case d).
// Case g-chain: `Order.Load("x").Validate()` -- receiverKind "call" (the qualifier is itself an
// invocation result), a documented recall miss per devscout's extractor's accepted-qualifier
// list (src/extract.rs:697-720).
using Xunit;
using Fixture.Domain;

namespace Fixture.App.Tests;

public class WorkerTests
{
    [Fact]
    public async Task FakeServerAnswers()
    {
        var server = new FakeServer();
        var body = await server.GetAsync("/o/1");
        Assert.NotNull(body);
        Assert.True(Order.Load("x").Validate());
    }
}
