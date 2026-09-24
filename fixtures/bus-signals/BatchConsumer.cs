using System.Threading.Tasks;

namespace BusSignals
{
    public class LoanBatchConsumer : IConsumer<Batch<LoanRequested>>
    {
        public Task Consume(Batch<LoanRequested> message) => Task.CompletedTask;
    }
}
