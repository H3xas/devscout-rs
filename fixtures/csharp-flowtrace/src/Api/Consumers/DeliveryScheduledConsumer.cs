// A classic (non-primary) consumer: fields assigned from ctor parameters,
// consuming via a base class, and a publish call sitting inside a private
// helper method rather than in Consume itself.
namespace Courier.Api.Consumers;

using Courier.Api.Messaging.Events;
using Courier.Api.Messaging.Messages;
using Courier.Api.Repositories;
using Courier.Framework;

public sealed class DeliveryScheduledConsumer : BaseConsumer<DeliveryScheduled>
{
    private readonly IParcelRepository _repository;
    private readonly IPublishEndpoint _bus;

    public DeliveryScheduledConsumer(IParcelRepository repository, IPublishEndpoint bus)
    {
        _repository = repository ?? throw new ArgumentNullException(nameof(repository));
        _bus = bus;
    }

    public override Task Consume(DeliveryScheduled message) => NotifyLost(new ParcelLostEvent(message.ParcelId));

    private Task NotifyLost(ParcelLostEvent evt)
    {
        _ = _repository.Find(evt.ParcelId.GetHashCode());
        return _bus.Publish<ParcelLostEvent>(evt);
    }
}
