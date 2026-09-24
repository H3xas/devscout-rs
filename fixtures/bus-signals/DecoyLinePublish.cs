using System.Threading;
using System.Threading.Tasks;

namespace BusSignals
{
    public class ExternalNotice
    {
    }

    public class DecoyAnnouncer
    {
        public Task Announce(IBus bus, CancellationToken ct) { Telemetry.Tag<LoanRequested>(); return bus.Publish(new ExternalNotice(), ct); }
    }
}
