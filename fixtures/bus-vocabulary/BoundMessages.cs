using System.Threading.Tasks;

namespace BusVocabulary
{
    public class ShelfAuditHandler : IBindingHandler<ReturnsDeskNotice>
    {
        public Chime<ShelfAuditNotice> Audited { get; set; }

        public Task Accept(ReturnsDeskNotice notice) => Task.CompletedTask;
    }

    public static class AuditInstallation
    {
        public static void Install(IWorkshopRegistry registry)
        {
            registry.AddBindingHandler<ShelfAuditHandler>();
        }
    }

    public class ShelfAuditPublisher
    {
        private readonly IShelfBus _bus;

        public ShelfAuditPublisher(IShelfBus bus) => _bus = bus;

        public Task Announce() => _bus.Publish(new ShelfAuditNotice { Shelf = "C4" });
    }
}
