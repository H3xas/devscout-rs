using System.Threading.Tasks;

namespace BusVocabulary
{
    public class AisleWorker : ShelfWorkerBase<AisleNotice>
    {
        public override Task Work(AisleNotice notice) => Task.CompletedTask;
    }

    public class AisleNotice
    {
        public string Aisle { get; set; }
    }

    public static class AisleInstallation
    {
        public static void Install(IWorkshopRegistry registry)
        {
            registry.AddShelfWorker<AisleWorker>();
        }
    }

    public class AislePublisher
    {
        private readonly IShelfBus _bus;

        public AislePublisher(IShelfBus bus) => _bus = bus;

        public Task Announce() => _bus.Enqueue(new AisleNotice { Aisle = "A1" });
    }
}
