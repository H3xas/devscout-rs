using System.Threading;

namespace BusSignals
{
    // The verify half of the same shape `TestDoubleSetupLambda.cs` covers:
    // a call spelled `Verify` -- also Moq's own public surface, also
    // declared nowhere in this repository -- wrapping a lambda that
    // publishes `LoanRequested`. Must earn no hop, for the same reason.
    public class LoanRequestedVerifyExpectation
    {
        public void Assert(IBus bus)
        {
            bus.Verify(x => x.PublishAsync<LoanRequested>(CancellationToken.None));
        }
    }
}
