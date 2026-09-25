using System.Threading;
using System.Threading.Tasks;

namespace BusSignals
{
    public class ShelfAuditTrigger
    {
        public Task Trigger(IBus bus, CancellationToken ct) => bus.SubmitJob<ShelfAuditJob>(ct);
    }
}
