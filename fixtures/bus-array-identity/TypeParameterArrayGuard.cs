using System.Threading;
using System.Threading.Tasks;

namespace BusArrayIdentity
{
    public abstract class Envelope<TMessage> : IConsumer<TMessage[]>
    {
        public abstract Task Consume(TMessage[] message);
    }

    public class ReliefOfficer : IConsumer<ReliefRequest>
    {
        public Task Consume(ReliefRequest message) => Task.CompletedTask;
    }

    public class ReliefDispatcher
    {
        public Task Raise(IWatchBus bus, ReliefRequest request, CancellationToken ct) =>
            bus.Publish<ReliefRequest>(request, ct);
    }
}
