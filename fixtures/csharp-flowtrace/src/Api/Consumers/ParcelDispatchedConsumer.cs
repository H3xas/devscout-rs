// A primary-constructor consumer that publishes a locally constructed
// message (RouteNote) and submits a second message through the SubmitJob
// publish surface, without storing either dependency as a field.
namespace Courier.Api.Consumers;

using Courier.Api.Messaging.Events;
using Courier.Api.Messaging.Messages;
using Courier.Api.Repositories;
using Courier.Framework;

public sealed class ParcelDispatchedConsumer(IParcelRepository repository, IPublishEndpoint bus) : IConsumer<ParcelDispatchedMessage>
{
    public async Task Consume(ParcelDispatchedMessage message)
    {
        var ct = CancellationToken.None;
        var known = repository.Find(message.ParcelId.GetHashCode());
        var note = new RouteNote(message.ParcelId, known ?? $"dispatched via {message.Carrier}");
        await bus.Publish(note, ct);
        await bus.SubmitJob(new ParcelLostEvent(message.ParcelId));
    }
}
