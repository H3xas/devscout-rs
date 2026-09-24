using System.Threading;
using System.Threading.Tasks;

namespace BusSignals
{
    public class OverdueLedgerConsumer : IConsumer<OverdueReminderMessage>
    {
        public Task Consume(OverdueReminderMessage message) => Task.CompletedTask;
    }

    public class OverdueNotifierConsumer : IConsumer<OverdueReminderMessage>
    {
        public Task Consume(OverdueReminderMessage message) => Task.CompletedTask;
    }

    public class OverdueReminderTrigger
    {
        public Task Announce(IBus bus, CancellationToken ct) => bus.Publish(new OverdueReminderMessage(), ct);
    }
}
