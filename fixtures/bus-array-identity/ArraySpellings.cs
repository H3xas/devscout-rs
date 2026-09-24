using System.Threading;
using System.Threading.Tasks;

namespace BusArrayIdentity
{
    public class TideBulletin : IConsumer<TideNotice[]>
    {
        public Task Consume(TideNotice[] message) => Task.CompletedTask;
    }

    public class TideDispatcher
    {
        public Task RaiseByGenericArgument(IWatchBus bus, TideNotice[] readings, CancellationToken ct) =>
            bus.Publish<TideNotice[]>(readings, ct);

        public Task RaiseByExplicitCreation(IWatchBus bus, CancellationToken ct) =>
            bus.Publish(new TideNotice[2], ct);

        public Task RaiseByImplicitCreation(IWatchBus bus, CancellationToken ct) =>
            bus.Publish(new[] { new TideNotice(), new TideNotice() }, ct);

        public Task RaiseByDeclaredVariable(IWatchBus bus, CancellationToken ct)
        {
            TideNotice[] readings = new TideNotice[3];
            return bus.Send(readings, ct);
        }
    }
}
