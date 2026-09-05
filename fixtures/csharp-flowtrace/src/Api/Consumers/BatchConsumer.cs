// Exercises Batch<T> unwrapping on a concrete consumer, plus an abstract
// consumer that must not produce a consume fact of its own.
namespace Courier.Api.Consumers;

using Courier.Api.Messaging.Messages;
using Courier.Framework;

public sealed class ParcelBatchConsumer : IConsumer<Batch<ParcelDispatchedMessage>>
{
    public Task Consume(Batch<ParcelDispatchedMessage> message)
    {
        foreach (var _ in message)
        {
        }

        return Task.CompletedTask;
    }
}

public abstract class AuditingConsumer<T> : IConsumer<T>
{
    public abstract Task Consume(T message);
}
