using System.Threading.Tasks;

namespace BusNonPublicOverload
{
    public class FlareWatcher : IConsumer<Flare>
    {
        public Task Consume(Flare message) => Task.CompletedTask;
    }

    public class FlareLauncher
    {
        public Task Launch(IRelayBus bus) => bus.Publish(new Flare());
    }
}
