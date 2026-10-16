using System;
using System.Threading.Tasks;

namespace Fixture.Refusals.Fakes
{
    public interface IBusControl
    {
        Task GetSendEndpoint(Uri address);
    }

    public class OrderRouter
    {
        public void ReceiveEndpoint(string name, Action<object> configure)
        {
        }
    }

    public class RouterBuilder
    {
    }
}
