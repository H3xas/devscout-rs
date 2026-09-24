using System.Threading;
using System.Threading.Tasks;

namespace BusSignals
{
    public class CatalogueEntryPublisher
    {
        public Task Announce(IBus bus, CancellationToken ct)
        {
            return bus
                .PublishAsync<CatalogueEntryPublishedMessage>(
                    ct);
        }
    }
}
