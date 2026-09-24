using System.Threading.Tasks;

namespace BusSignals
{
    public class MembershipRenewalSaga : IAmInitiatedBy<MembershipLapsedMessage>
    {
        public Task Handle(MembershipLapsedMessage message) => Task.CompletedTask;
    }
}
