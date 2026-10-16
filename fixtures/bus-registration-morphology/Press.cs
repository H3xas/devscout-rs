using System;
using System.Reflection;
using System.Threading.Tasks;

namespace BusRegistrationMorphology
{
    public interface IPressBus
    {
        Task Publish<TNotice>(TNotice notice) where TNotice : class;
    }

    public interface IPressRegistry
    {
        IPressRegistry AddEdition<THandler>();
        IPressRegistry AddEdition(Type handlerType);
        IPressRegistry AddFolder<THandler, TState>();
        IPressRegistry Handlers(Type handlerType);
        IPressRegistry With<THandler>();
        IPressRegistry Endpoint<THandler>();
        IPressRegistry AddEditionsFromAssembly(Assembly assembly);
        IPressRegistry RegisterEditionTypes(Assembly assembly);
        IPressRegistry UseMiddleware<TWare>();
    }

    public abstract class PressHandlerBase<TNotice>
    {
        public abstract Task Handle(TNotice notice);
    }

    public abstract class PressMiddlewareBase<TNotice>
    {
        public abstract Task Invoke();
    }
}
