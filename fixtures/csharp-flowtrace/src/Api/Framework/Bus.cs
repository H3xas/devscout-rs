// Stand-ins for a message-bus framework: consumer contracts, batching, and
// the publish surface used by handlers throughout this fixture.
namespace Courier.Framework;

public interface IMessage
{
}

public interface ICorrelatedMessage : IMessage
{
    Guid CorrelationId { get; }
}

public interface IConsumer<T>
{
    Task Consume(T message);
}

public sealed class Batch<T> : IEnumerable<T>
{
    private readonly List<T> _items = new();

    public void Add(T item) => _items.Add(item);

    public IEnumerator<T> GetEnumerator() => _items.GetEnumerator();

    System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
}

public abstract class BaseConsumer<T> : IConsumer<T>
{
    public abstract Task Consume(T message);
}

public interface IPublishEndpoint
{
    Task Publish<T>(T message, CancellationToken ct = default) where T : class;

    Task PublishAsync(object message);

    Task SubmitJob<T>(T job, CancellationToken ct = default) where T : class;
}
