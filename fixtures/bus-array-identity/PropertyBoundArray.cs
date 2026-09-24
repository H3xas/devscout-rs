using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace BusArrayIdentity
{
    public class HarborWarden : IConsumer<HarborNotice>
    {
        public List<HarborNotice[]> Bulletins { get; set; }

        public Task Consume(HarborNotice message) => Task.CompletedTask;
    }

    public class HarborDispatcher
    {
        public Task RaiseSingle(IWatchBus bus, HarborNotice notice, CancellationToken ct) =>
            bus.Publish<HarborNotice>(notice, ct);

        public Task RaiseArray(IWatchBus bus, HarborNotice[] notices, CancellationToken ct) =>
            bus.Publish<HarborNotice[]>(notices, ct);
    }
}
