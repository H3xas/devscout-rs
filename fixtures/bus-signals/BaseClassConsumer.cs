using System.Threading.Tasks;

namespace BusSignals
{
    public class CatalogueEntryConsumer : BaseConsumer<CatalogueEntryPublishedMessage>
    {
        public override Task Consume(CatalogueEntryPublishedMessage message) => Task.CompletedTask;
    }
}
