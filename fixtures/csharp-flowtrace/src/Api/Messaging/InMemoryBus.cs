// The one concrete IPublishEndpoint implementation, registered in Program.cs
// so DI resolution has somewhere real to land.
namespace Courier.Api.Messaging;

using Courier.Framework;

public sealed class InMemoryBus : IPublishEndpoint
{
    public Task Publish<T>(T message, CancellationToken ct = default) where T : class => Task.CompletedTask;

    public Task PublishAsync(object message) => Task.CompletedTask;

    public Task SubmitJob<T>(T job, CancellationToken ct = default) where T : class => Task.CompletedTask;
}
