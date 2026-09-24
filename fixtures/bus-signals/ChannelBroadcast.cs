using System.Threading;
using System.Threading.Tasks;

namespace BusSignals
{
    public class ShelfUpdateBroadcaster
    {
        public Task Broadcast(IBus bus, CancellationToken ct) => bus.PublishAsync(Channels.ShelfUpdates, ct);
    }
}
