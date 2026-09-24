using System.Threading;
using System.Threading.Tasks;

namespace BusSignals
{
    public class VolumeConsumer : IConsumer<VolumeShelvedMessage>
    {
        public Task Consume(VolumeShelvedMessage message) => Task.CompletedTask;
    }

    public class ReservationDesk
    {
        public Task Hold(IBus bus, CancellationToken ct) => bus.Publish(new VolumeShelvedMessage { Aisle = "12B" }, ct);
    }
}
