using System.Threading;
using System.Threading.Tasks;

namespace BusSignals
{
    public class ReservationConsumer : IConsumer<ReservationHeldMessage>
    {
        public Task Consume(ReservationHeldMessage message) => Task.CompletedTask;
    }

    public class ReservationClerk
    {
        public Task Confirm(IBus bus, CancellationToken ct)
        {
            var msg = new ReservationHeldMessage();
            var queuedAt = System.DateTime.UtcNow;
            LogQueued(queuedAt);
            return bus.Publish(msg, ct);
        }

        private void LogQueued(System.DateTime at)
        {
        }
    }
}
