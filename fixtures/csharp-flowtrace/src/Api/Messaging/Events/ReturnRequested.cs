// A marker-interface message used by the partial-class consumer next door,
// so the consume fact for a split class has a message type to point at.
namespace Courier.Api.Messaging.Events;

using Courier.Framework;

public sealed record ReturnRequested(string ParcelId) : IMessage;
