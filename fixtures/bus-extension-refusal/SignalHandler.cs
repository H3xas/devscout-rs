using System.Threading.Tasks;

namespace BusExtensionRefusal
{
    public class SignalHandler : IConsumer<Signal>
    {
        public Task Consume(Signal message) => Task.CompletedTask;
    }

    public class SignalDispatcher
    {
        public Task Raise(IWatchBus bus) => bus.Publish(new Signal());
    }
}
