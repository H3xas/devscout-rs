// Exercises message_class by marker interface outside the Messages path,
// alongside a plain type in the same folder that is not a message at all.
namespace Courier.Api.Messaging.Events;

public sealed class ParcelLostEvent : Courier.Framework.IMessage
{
    public ParcelLostEvent(string parcelId)
    {
        ParcelId = parcelId;
    }

    public string ParcelId { get; }
}

public sealed class DispatchAudit
{
    public DispatchAudit(string parcelId, DateTimeOffset recordedAt)
    {
        ParcelId = parcelId;
        RecordedAt = recordedAt;
    }

    public string ParcelId { get; }

    public DateTimeOffset RecordedAt { get; }
}
