using System.Threading;

namespace BusSignals
{
    // A call spelled `Setup` -- Moq's own public verb surface, declared
    // nowhere in this repository -- wrapping a lambda whose body publishes
    // `LoanRequested`. The lambda never actually runs: it is data a mocking
    // library inspects to arrange an expectation, not a delegate this
    // engine's static analysis should read as a real dispatch. Must earn no
    // hop.
    public class LoanRequestedSetupExpectation
    {
        public void Arrange(IBus bus)
        {
            bus.Setup(x => x.PublishAsync<LoanRequested>(CancellationToken.None));
        }
    }
}
