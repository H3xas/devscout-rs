using System.Threading;
using System.Threading.Tasks;

namespace BusArrayIdentity
{
    public class AlertOfficer : IConsumer<AlertNotice>
    {
        public Task Consume(AlertNotice message) => Task.CompletedTask;
    }

    public class AlertBulletin : IConsumer<AlertNotice[]>
    {
        public Task Consume(AlertNotice[] message) => Task.CompletedTask;
    }

    public class AlertDispatcher
    {
        public Task Raise(IWatchBus bus, AlertNotice alert, CancellationToken ct) =>
            bus.Publish<AlertNotice>(alert, ct);
    }
}
