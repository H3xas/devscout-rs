using System.Threading.Tasks;

namespace BusSignals
{
    // Binds `LoanRequested` twice -- once on its base list, once on a
    // qualifying property -- reached by `GenericPublish.cs`'s single publish
    // site. Must earn exactly one edge for that route, with the base-list
    // evidence word (`base-arg`), never two edges or `property-arg`.
    public class ShelfArchiveConsumer : IConsumer<LoanRequested>
    {
        public Batch<LoanRequested> Recent { get; set; }

        public Task Consume(LoanRequested message) => Task.CompletedTask;
    }
}
