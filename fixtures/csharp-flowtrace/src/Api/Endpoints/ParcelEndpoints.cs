// Static, instance-less endpoint handlers referenced from Program.cs as
// method groups, plus a named-method route registration (as opposed to
// Program.cs's top-level one) so the controller/action naming differs.
namespace Courier.Api.Endpoints;

using Courier.Api.Contracts;
using Courier.Api.Repositories;
using Courier.Framework;

public static class ParcelEndpoints
{
    public static string? Delete(int id, IParcelRepository repository) => repository.Find(id);

    public static string? Update(int id, CreateParcel request, IParcelRepository repository) => repository.Find(id);

    public static void Register(WebApplication app)
    {
        app.MapGroup("api/v2").MapGet("parcels/{id}", (int id, IParcelRepository repository) => repository.Find(id));
    }
}
