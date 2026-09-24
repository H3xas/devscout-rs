using System.Threading;
using System.Threading.Tasks;

namespace BusArrayIdentity
{
    public class StormOfficer : IConsumer<StormWarning>
    {
        public Task Consume(StormWarning message) => Task.CompletedTask;
    }

    public class StormBulletin : IConsumer<StormWarning[]>
    {
        public Task Consume(StormWarning[] message) => Task.CompletedTask;
    }

    public class StormDispatcher
    {
        public Task Raise(IWatchBus bus, CancellationToken ct)
        {
            StormWarning[] warnings = new StormWarning[0];
            return bus.Send(warnings, ct);
        }
    }
}
