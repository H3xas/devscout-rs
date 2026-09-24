using System.Threading;
using System.Threading.Tasks;
using Xunit;

namespace BusSignals
{
    // A test file that PUBLISHES the same message `ShelfConsumer`/
    // `LoanBatchConsumer` already receive, over a bus hop -- a possible
    // route `tests` must disclose as such, never counted toward the
    // precise test-file/reference counts.
    public class ShelfConsumerPublishingTest
    {
        [Fact]
        public Task PublishesLoanRequested(IBus bus, CancellationToken ct) =>
            bus.PublishAsync<LoanRequested>(ct);
    }
}
