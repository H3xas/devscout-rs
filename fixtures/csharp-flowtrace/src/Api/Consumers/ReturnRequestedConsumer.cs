// One half of a partial class: this part carries the base list
// (IConsumer<ReturnRequested>) and the Consume method itself, so the
// consume fact should be attributed to this part, not duplicated across
// both halves of the split.
namespace Courier.Api.Consumers;

using Courier.Api.Messaging.Events;
using Courier.Framework;

public sealed partial class ReturnRequestedConsumer : IConsumer<ReturnRequested>
{
    public Task Consume(ReturnRequested message)
    {
        Record(message.ParcelId);
        return Task.CompletedTask;
    }
}
