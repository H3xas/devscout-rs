using System.Threading;
using System.Threading.Tasks;

namespace BusArrayCreationIdentity
{
    public class TideOfficer : IConsumer<TideGauge>
    {
        public Task Consume(TideGauge message) => Task.CompletedTask;
    }

    public class TideGaugeBulletin : IConsumer<TideGauge[]>
    {
        public Task Consume(TideGauge[] message) => Task.CompletedTask;
    }

    public class TideGaugeDispatcher
    {
        public Task RaiseArray(IWatchBus bus, CancellationToken ct) =>
            bus.Publish(new[] { new TideGauge(), new TideGauge() }, ct);

        public Task RaiseSingle(IWatchBus bus, CancellationToken ct) =>
            bus.Publish(new TideGauge(), ct);
    }
}
