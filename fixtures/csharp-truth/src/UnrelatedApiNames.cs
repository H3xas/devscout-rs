namespace Truth.UnrelatedApiNames;

public interface IService { }

public sealed class Service : IService { }

public sealed class Payload { }

public sealed class NonMessagingApi
{
    public void Publish(Payload value) { }

    public void AddScoped<TService, TImplementation>() { }

    public void MapGet(string path, System.Action action) { }
}

public static class Caller
{
    public static void Run(NonMessagingApi api)
    {
        api.Publish(new Payload());
        api.AddScoped<IService, Service>();
        api.MapGet("/definitely-not-an-endpoint", () => { });
    }
}
