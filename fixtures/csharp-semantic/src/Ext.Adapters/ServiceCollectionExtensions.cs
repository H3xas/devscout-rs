// Case c: AddWidgets is a static extension method on IServiceCollection -- probes tier (f)
// extension-method resolution (src/resolve.rs:1291-1435).
// Case c2: DeepRegistration below calls AddWidgets with no using directive of its own; it is
// resolved only because Deep is lexically nested inside the Registration namespace block, so
// Registration is an enclosing namespace, not an explicit import -- probes the namespace-
// visibility bound of tier (f). Contrast with the local `using` in src/App/Worker.cs (case c1)
// and the file-scoped `global using` in src/App/AppDbContext.cs (case c3).
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
