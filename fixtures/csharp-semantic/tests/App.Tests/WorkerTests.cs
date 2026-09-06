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
