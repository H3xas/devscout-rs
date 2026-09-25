using System.Threading.Tasks;

namespace BusVocabulary
{
    public abstract class LedgerReaderBase<TNotice>
    {
        public abstract Task Read(TNotice notice);
    }

    public class UnregisteredReader : LedgerReaderBase<UnregisteredNotice>
    {
        public override Task Read(UnregisteredNotice notice) => Task.CompletedTask;
    }

    public class UnregisteredPublisher
    {
        private readonly IShelfBus _bus;

        public UnregisteredPublisher(IShelfBus bus) => _bus = bus;

        public Task Announce() => _bus.Publish(new UnregisteredNotice { Reason = "none" });
    }
}
