// Exercises message_class resolution three different ways in one folder:
// by path and suffix together, by marker interface alone, and by neither.
namespace Courier.Api.Messaging.Messages;

public sealed record ParcelDispatchedMessage(string ParcelId, string Carrier);

public sealed record DeliveryScheduled(string ParcelId, DateTimeOffset ScheduledFor) : Courier.Framework.ICorrelatedMessage
{
    public Guid CorrelationId { get; init; } = Guid.NewGuid();
}

public sealed class RouteNote
{
    public RouteNote(string parcelId, string note)
    {
        ParcelId = parcelId;
        Note = note;
    }

    public string ParcelId { get; }

    public string Note { get; }
}
