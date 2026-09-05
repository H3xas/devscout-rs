// Stand-ins for a minimal-API hosting framework: the service collection,
// endpoint routing surface, request-handler contract, and one deliberately
// misplaced framework type (HttpContext) that lives under a Microsoft.*
// namespace so the framework-type exclusion rule has something to match.
namespace Courier.Framework
{
    public interface IServiceCollection
    {
    }

    public interface IEndpointRouteBuilder
    {
    }

    public sealed class RouteGroupBuilder : IEndpointRouteBuilder
    {
    }

    public sealed class WebApplication : IEndpointRouteBuilder
    {
        public static WebApplication Create() => new();

        public IServiceCollection Services { get; } = new ServiceCollectionImpl();

        public void Run()
        {
        }

        private sealed class ServiceCollectionImpl : IServiceCollection
        {
        }
    }

    public static class ServiceCollectionExtensions
    {
        public static IServiceCollection AddScoped<TService, TImpl>(this IServiceCollection services)
            where TImpl : class, TService
            => services;

        public static IServiceCollection AddSingleton<TService>(this IServiceCollection services, Func<IServiceProvider, TService> factory)
            => services;

        public static IServiceCollection AddTransient<TService, TImpl>(this IServiceCollection services)
            where TImpl : class, TService
            => services;

        public static IServiceCollection Register<TService, TImpl>(this IServiceCollection services)
            where TImpl : class, TService
            => services;
    }

    public static class EndpointExtensions
    {
        public static RouteGroupBuilder MapGroup(this IEndpointRouteBuilder builder, string prefix) => new();

        public static IEndpointRouteBuilder MapGet(this IEndpointRouteBuilder builder, string pattern, Delegate handler) => builder;

        public static IEndpointRouteBuilder MapPost(this IEndpointRouteBuilder builder, string pattern, Delegate handler) => builder;

        public static IEndpointRouteBuilder MapPut(this IEndpointRouteBuilder builder, string pattern, Delegate handler) => builder;

        public static IEndpointRouteBuilder MapDelete(this IEndpointRouteBuilder builder, string pattern, Delegate handler) => builder;

        public static IEndpointRouteBuilder MapPatch(this IEndpointRouteBuilder builder, string pattern, Delegate handler) => builder;

        public static IEndpointRouteBuilder MapMethods(this IEndpointRouteBuilder builder, string pattern, IEnumerable<string> methods, Delegate handler) => builder;
    }

    public interface IRequest<TResponse>
    {
    }

    public interface IRequestHandler<TRequest, TResponse>
    {
        Task<TResponse> Handle(TRequest request, CancellationToken ct);
    }
}

namespace Microsoft.AspNetCore.Http
{
    public sealed class HttpContext
    {
        public string ConnectionId { get; init; } = string.Empty;
    }
}
