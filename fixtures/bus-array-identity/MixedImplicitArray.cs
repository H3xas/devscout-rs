using System.Threading;
using System.Threading.Tasks;

namespace BusArrayIdentity
{
    public class MixedArrayDispatcher
    {
        public Task Raise(IWatchBus bus, CancellationToken ct) =>
            bus.Publish(new[] { new TideNotice(), new HarborNotice() }, ct);
    }
}
