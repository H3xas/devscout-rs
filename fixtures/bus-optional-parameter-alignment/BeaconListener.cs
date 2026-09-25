using System.Threading.Tasks;

namespace BusOptionalAlignment
{
    public class BeaconListener : IConsumer<Beacon>
    {
        public Task Consume(Beacon message) => Task.CompletedTask;
    }

    public class BeaconEmitter
    {
        public Task Raise(ISignalBus bus) => bus.Publish(new Beacon());
    }
}
