namespace Fixture.App.Tests;

public class FakeServer
{
    public Task<string> GetAsync(string path) => Task.FromResult("{}");
}
