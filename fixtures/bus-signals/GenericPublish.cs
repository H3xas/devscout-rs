using System.Threading;
using System.Threading.Tasks;

namespace BusSignals
{
    public class ShelfClerk
    {
        public Task Announce(IBus bus, CancellationToken ct) => bus.PublishAsync<LoanRequested>(ct);
    }
}
