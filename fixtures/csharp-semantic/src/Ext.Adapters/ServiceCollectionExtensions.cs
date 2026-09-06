using Microsoft.Extensions.DependencyInjection;

namespace Fixture.Ext.Adapters.Registration
{
    public static class ServiceCollectionExtensions
    {
        public static IServiceCollection AddWidgets(this IServiceCollection s) => s;
    }

    namespace Deep
    {
        public static class DeepRegistration
        {
            public static void Register(IServiceCollection s) => s.AddWidgets();
        }
    }
}
