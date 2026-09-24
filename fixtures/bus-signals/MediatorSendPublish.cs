using System.Threading.Tasks;

namespace BusSignals
{
    public class CatalogueLookupRouter
    {
        public Task<object> Route(IMediator mediator, CatalogueLookupRequest request) => mediator.Send(request);
    }
}
