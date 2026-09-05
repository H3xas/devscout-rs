// Exercises route composition: a class-level route template, a bare-verb
// action, an action combining an explicit Route with two bare verbs, an
// action with no verb at all (any-method), an inline message publish, and
// a private helper that should only ever produce a method_span.
namespace Courier.Api.Controllers;

using Courier.Api.Contracts;
using Courier.Api.Messaging.Messages;
using Courier.Api.Repositories;
using Courier.Framework;

[Route("api/[controller]")]
public sealed class ParcelsController : ControllerBase
{
    private readonly IParcelRepository _repository;
    private readonly IPublishEndpoint _bus;

    public ParcelsController(IParcelRepository repository, IPublishEndpoint bus)
    {
        _repository = repository;
        _bus = bus;
    }

    [HttpGet("{id}")]
    public IActionResult Get(int id)
    {
        var found = _repository.Find(id);
        return found is null ? NotFound() : Ok(found);
    }

    [HttpPost]
    public async Task<IActionResult> Create([FromBody] CreateParcel request)
    {
        await _bus.Publish(new ParcelDispatchedMessage(BuildParcelId(request), "standard"));
        return Ok();
    }

    [Route("search")]
    [HttpGet]
    [HttpPost]
    public IActionResult Search([FromQuery] string term) => Ok(term);

    [Route("[action]")]
    public IActionResult Archive([FromRoute] int id) => Ok(id);

    private string BuildParcelId(CreateParcel request) => $"{request.Recipient}-{request.Address.Length}";
}
