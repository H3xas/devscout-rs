namespace Fixture.Service;

using MassTransit;

public static class LocalHarness
{
    public static void Substitute(IServiceCollection services)
    {
        services.AddMassTransitTestHarness();
    }
}
