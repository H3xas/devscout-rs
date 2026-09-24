using System.Threading.Tasks;

namespace BusVocabulary
{
    public abstract class BranchWorkerBase<TNotice> : ShelfWorkerBase<TNotice>
    {
        public override Task Work(TNotice notice) => Accept(notice);

        protected abstract Task Accept(TNotice notice);
    }

    public class OverdueWorker : BranchWorkerBase<OverdueNotice>
    {
        protected override Task Accept(OverdueNotice notice) => Task.CompletedTask;
    }

    public class OverduePublisher
    {
        private readonly IShelfBus _bus;

        public OverduePublisher(IShelfBus bus) => _bus = bus;

        public Task Announce() => _bus.Publish(new OverdueNotice { DaysLate = 9 });
    }
}
