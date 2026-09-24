using System.Threading.Tasks;

namespace BusSignals
{
    public class CatalogueLookupHandler : IRequestHandler<CatalogueLookupRequest, CatalogueLookupResult>
    {
        public Task<CatalogueLookupResult> Handle(CatalogueLookupRequest request) => Task.FromResult(new CatalogueLookupResult());
    }
}
