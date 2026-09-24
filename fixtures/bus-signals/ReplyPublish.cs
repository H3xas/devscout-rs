using System.Threading;
using System.Threading.Tasks;

namespace BusSignals
{
    public class LoanApprovalConsumer : IConsumer<LoanApprovalReply>
    {
        public Task Consume(LoanApprovalReply message) => Task.CompletedTask;
    }

    public class LoanApprover
    {
        public Task Answer(IBus bus, CancellationToken ct) => bus.Reply<LoanApprovalReply>(ct);
    }
}
