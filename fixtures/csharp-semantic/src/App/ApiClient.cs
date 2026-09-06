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
