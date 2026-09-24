using System.Threading.Tasks;

namespace BusSignals
{
    public class ShelfConsumer : IConsumer<LoanRequested>
    {
        public Task Consume(LoanRequested message) => Task.CompletedTask;
    }
}
