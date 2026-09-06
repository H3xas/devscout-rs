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
