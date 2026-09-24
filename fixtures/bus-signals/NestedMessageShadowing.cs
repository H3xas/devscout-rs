using System.Threading;
using System.Threading.Tasks;

namespace BusSignals
{
    // `ReturnsDesk` nests its own `LoanRequested`, distinct from the
    // top-level `Messages.cs` one. `Trigger` and `Handler`, both declared
    // inside `ReturnsDesk`, each name the message by its own bare,
    // unqualified name -- a reference at that position resolves the same
    // way an ordinary type reference nested this way would, so neither must
    // ever land on the top-level message instead of its own sibling.
    public class ReturnsDesk
    {
        public class LoanRequested
        {
        }

        public class Trigger
        {
            public Task Announce(IBus bus, CancellationToken ct) => bus.PublishAsync<LoanRequested>(ct);
        }

        public class Handler : IConsumer<LoanRequested>
        {
            public Task Consume(LoanRequested message) => Task.CompletedTask;
        }
    }
}
