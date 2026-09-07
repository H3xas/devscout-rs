namespace Signals;

public class ServiceRegistry
{
}

public class Wiring
{
    public void Configure(ServiceRegistry services)
    {
        services.AddScoped<IBeacon, LightBeacon>();
        services.AddSingleton<IBeacon, LoudBeacon>();
        services.AddScoped<IBeacon, SilentBeacon>();
        services.AddSingleton<IRelay, NoisyBeacon>();
        services.AddSingleton<IRelay>();
    }
}
