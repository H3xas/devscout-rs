using System.Threading.Tasks;

namespace BusSignals
{
    public class SagaLauncher
    {
        public Task Begin(IBus bus) => bus.PublishAsync(context => context.Init(new MembershipLapsedMessage()));
    }
}
