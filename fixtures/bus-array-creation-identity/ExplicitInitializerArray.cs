using System.Threading;
using System.Threading.Tasks;

namespace BusArrayCreationIdentity
{
    public class BeaconOfficer : IConsumer<BeaconSighting>
    {
        public Task Consume(BeaconSighting message) => Task.CompletedTask;
    }

    public class BeaconBulletin : IConsumer<BeaconSighting[]>
    {
        public Task Consume(BeaconSighting[] message) => Task.CompletedTask;
    }

    public class BeaconDispatcher
    {
        public Task RaiseArray(IWatchBus bus, CancellationToken ct) =>
            bus.Publish(new BeaconSighting[] { new BeaconSighting() }, ct);

        public Task RaiseSingle(IWatchBus bus, CancellationToken ct) =>
            bus.Publish(new BeaconSighting(), ct);
    }
}
