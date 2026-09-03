// Case d: exercises FakeServer.GetAsync from a test method (see FakeServer.cs, also case d).
// Case g-chain: `Order.Load("x").Validate()` -- receiverKind "call" (the qualifier is itself an
// invocation result); `Order.Load` is a static-qualified one-hop call-chain tail, resolved
// precisely through the same `method_returns` lookup a `var x = Q.M()` local uses.
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
