using System.Threading.Tasks;

namespace BusSignals
{
    public class CatalogueProcessor : IWorkHandler<ShelfAuditJob>
    {
        public Task Process(ShelfAuditJob work) => Task.CompletedTask;
    }
}
