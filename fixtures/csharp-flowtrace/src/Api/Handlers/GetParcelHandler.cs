// Exercises di_binding through the request-handler interface, plus a
// request/response pair shaped as records.
namespace Courier.Api.Handlers;

using Courier.Api.Repositories;
using Courier.Framework;

public sealed record GetParcelQuery(int Id) : IRequest<ParcelDto>;

public sealed record ParcelDto(int Id, string Description);

public sealed class GetParcelHandler : IRequestHandler<GetParcelQuery, ParcelDto>
{
    private readonly IParcelRepository _repository;

    public GetParcelHandler(IParcelRepository repository)
    {
        _repository = repository;
    }

    public Task<ParcelDto> Handle(GetParcelQuery request, CancellationToken ct)
    {
        var description = _repository.Find(request.Id) ?? "unknown";
        return Task.FromResult(new ParcelDto(request.Id, description));
    }
}
