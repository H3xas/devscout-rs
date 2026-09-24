using System;
using System.Threading;
using System.Threading.Tasks;

namespace BusSignals
{
    public class ExternalMessagePublisher
    {
        public Task Announce(IBus bus, CancellationToken ct) => bus.Publish(new Uri("about:blank"), ct);
    }
}
