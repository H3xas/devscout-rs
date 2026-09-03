// Case d: _http.GetAsync -- HttpClient.GetAsync shares a name with
// tests/App.Tests/FakeServer.GetAsync (case d), used only from the test project -- probes
// cross-project structural-impossibility guessing.
// Case e: _queue.Enqueue -- Queue<string>.Enqueue shares a name with
// src/Unreachable/Mailer.Enqueue (case e), a project referenced by nobody -- probes guessing
// into a structurally unreachable project.
// Case g-chain: `response.StatusCode.ToString()` -- a nested member_access_expression qualifier
// (StatusCode is itself a member access on response), probing chained-qualifier resolution.
// Case g-cond: `_http?.Dispose()` -- a `conditional_access_expression` qualifier, resolved to
// the same target a plain `_http.Dispose()` access would name.
// Case m: Fetch returns Order directly (no `await`); Worker.ProbeChainTail calls
// `_client.Fetch().Validate()` as one expression -- probes the one-hop call-chain tail.
using System.Net.Http;
using Fixture.Domain;

namespace Fixture.App;

public class ApiClient : IDisposable
{
    private readonly HttpClient _http;
    private readonly Queue<string> _queue = new();

    public ApiClient(HttpClient http)
    {
        _http = http;
    }

    public async Task<Order> FetchAsync()
    {
        var response = await _http.GetAsync("/o/1");
        _queue.Enqueue(response.ReasonPhrase ?? "");
        var order = Order.Load(response.StatusCode.ToString());
        return order;
    }

    public Order Fetch() => Order.Load("f");

    public void Dispose() => _http?.Dispose();
}
