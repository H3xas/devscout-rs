using System.Threading.Tasks;

namespace BusVocabulary
{
    public class ReturnsDeskWorker : ShelfWorkerBase<ReturnsDeskNotice>
    {
        public override Task Work(ReturnsDeskNotice notice) => Task.CompletedTask;
    }

    public static class WorkshopInstallation
    {
        public static void Install(IWorkshopRegistry registry)
        {
            registry.AddShelfWorker<ReturnsDeskWorker>();
        }
    }

    public class ReturnsDeskPublisher
    {
        private readonly IShelfBus _bus;

        public ReturnsDeskPublisher(IShelfBus bus) => _bus = bus;

        public Task Announce() => _bus.Publish(new ReturnsDeskNotice { Aisle = "12B" });
    }
}
