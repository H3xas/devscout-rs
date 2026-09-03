// Case b: _logger.LogInformation / _logger.LogWarning -- ILogger<Worker> LoggerExtensions calls,
// external targets that must not resolve into IUpgradeLog/UpgradeLogAdapter (case b).
// Case c1: local `using` for the Registration namespace, redundant with the `global using` in
// src/App/AppDbContext.cs (case c3) -- probes tier (f) resolution when the static class's
// namespace is a local using.
// Case g-await: `var order = await _client.FetchAsync();` -- the receiver `_client` is a field,
// so this gets a call fact whose owner is the field's type (src/resolve.rs:1196-1210), unlike an
// awaited call on a bare type/static qualifier, which gets none (src/extract.rs:2040-2055).
// Case enum: `order.Status == OrderStatus.Open` exercises the OrderStatus enum-member edge.
using Fixture.Ext.Adapters.Registration;
using Fixture.Domain;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;

namespace Fixture.App;

public class Worker
{
    private readonly ILogger<Worker> _logger;
    private readonly ApiClient _client;

    public Worker(ILogger<Worker> logger, ApiClient client)
    {
        _logger = logger;
        _client = client;
    }

    public void Register(IServiceCollection s) => s.AddWidgets();

    public async Task RunAsync()
    {
        _logger.LogInformation("run");
        var order = await _client.FetchAsync();
        order.Validate();
        if (order.Status == OrderStatus.Open)
        {
            _logger.LogWarning("open");
        }
    }
}
