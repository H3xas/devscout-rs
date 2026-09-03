// Case d: FakeServer.GetAsync shares a name with HttpClient.GetAsync (see src/App/ApiClient.cs,
// also case d); used only from this test project -- probes cross-project
// structural-impossibility guessing.
namespace Fixture.App.Tests;

public class FakeServer
{
    public Task<string> GetAsync(string path) => Task.FromResult("{}");
}
