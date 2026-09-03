// Case b: _logger.LogInformation / _logger.LogWarning -- ILogger<Worker> LoggerExtensions calls,
// external targets that must not resolve into IUpgradeLog/UpgradeLogAdapter (case b).
// Case c1: local `using` for the Registration namespace, redundant with the `global using` in
// src/App/AppDbContext.cs (case c3) -- probes tier (f) resolution when the static class's
// namespace is a local using.
// Case g-await: `var order = await _client.FetchAsync();` -- the receiver `_client` is a field,
// so this gets a call fact whose owner is the field's type (src/resolve.rs:1196-1210), unlike an
// awaited call on a bare type/static qualifier, which gets none (src/extract.rs:2040-2055).
// Case enum: `order.Status == OrderStatus.Open` exercises the OrderStatus enum-member edge.
// Case k: ProbeTypedLocals types one local through a cast, one through an `is` pattern
// designation, and one through an explicitly-typed `out` argument (Order.TryGet,
// src/Domain/Order.cs) -- each then calls a member on the typed local.
// Case l: ProbeAwaitedStatic awaits Repo.LoadAsync() (src/App/Repo.cs), a static-qualified call
// returning Task<Order> -- probes the one-layer Task<T> unwrap on the awaited local.
// Case m: ProbeChainTail calls `_client.Fetch().Validate()` (src/App/ApiClient.cs) as one
// expression -- probes the one-hop call-chain tail.
// Case n: ProbeLambdaElement's `_orders.Where(o => o.Validate())` types the lambda parameter
// from a single-type-argument generic (List<Order>). ProbeLambdaNoFact's
// `_byId.Where(kv => kv.Value.Validate())` does not -- Dictionary<string, Order> carries two
// type arguments, so `kv` earns no fact and `Validate()` is left to the scored tier.
// Case o: ProbeArity calls Order's own two-parameter Send (src/Domain/Order.cs) with two
// arguments, and OrderChannel's one-parameter extension Send (src/Domain/OrderChannel.cs) with
// one -- probes arity-gated call vouching.
// Case p: ProbeInterfaceVsClass calls Render on a Widget-typed local (src/Domain/Widget.cs,
// src/Domain/IWidget.cs) -- probes that a class-typed receiver binds the class's own
// declaration, never the interface's.
using Fixture.Ext.Adapters.Registration;
using Fixture.Domain;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;

namespace Fixture.App;

public class Worker
{
    private readonly ILogger<Worker> _logger;
    private readonly ApiClient _client;
    private readonly List<Order> _orders = new();
    private readonly Dictionary<string, Order> _byId = new();

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

    public bool ProbeTypedLocals(object o)
    {
        var x = (Order)o;
        var okCast = x.Validate();

        var okPattern = false;
        if (o is Order t)
        {
            okPattern = t.Validate();
        }

        Order.TryGet(out Order r);
        var okOut = r.Validate();

        return okCast && okPattern && okOut;
    }

    public async Task<bool> ProbeAwaitedStatic()
    {
        var o = await Repo.LoadAsync();
        return o.Validate();
    }

    public bool ProbeChainTail() => _client.Fetch().Validate();

    public bool ProbeLambdaElement() => _orders.Where(o => o.Validate()).Any();

    public bool ProbeLambdaNoFact() => _byId.Where(kv => kv.Value.Validate()).Any();

    public void ProbeArity()
    {
        var order = Order.Load("p");
        order.Send(1, 2);
        order.Send("x");
    }

    public void ProbeInterfaceVsClass()
    {
        Widget w = new Widget();
        w.Render();
    }
}
