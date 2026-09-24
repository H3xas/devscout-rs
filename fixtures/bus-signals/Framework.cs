using System;
using System.Threading;
using System.Threading.Tasks;

namespace BusSignals
{
    public interface IBus
    {
        Task Publish<TMessage>(TMessage message, CancellationToken ct = default) where TMessage : class;
        Task PublishAsync<TMessage>(CancellationToken ct = default) where TMessage : class, new();
        Task PublishAsync(Action<ISagaInitContext> configureSaga);
        Task PublishAsync(string channel, CancellationToken ct = default);
        Task SubmitJob<TJob>(CancellationToken ct = default) where TJob : class, new();
        Task Reply<TResponse>(CancellationToken ct = default) where TResponse : class, new();
    }

    public interface ISagaInitContext
    {
        void Init<TMessage>(TMessage message);
    }

    public interface IMediator
    {
        Task<object> Send(object request);
    }

    public interface IConsumer<TMessage>
    {
        Task Consume(TMessage message);
    }

    public abstract class BaseConsumer<TMessage> : IConsumer<TMessage>
    {
        public abstract Task Consume(TMessage message);
    }

    public interface IAmInitiatedBy<TMessage>
    {
        Task Handle(TMessage message);
    }

    public interface IWorkHandler<TWork>
    {
        Task Process(TWork work);
    }

    public interface IRequestHandler<TRequest, TResponse>
    {
        Task<TResponse> Handle(TRequest request);
    }

    public class Batch<TMessage>
    {
        public TMessage[] Messages { get; set; }
    }

    public static class Channels
    {
        public const string ShelfUpdates = "shelf-updates";
    }

    public static class Telemetry
    {
        public static void Tag<TMessage>()
        {
        }
    }
}
