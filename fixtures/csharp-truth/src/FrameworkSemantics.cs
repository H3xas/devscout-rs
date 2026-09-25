namespace Truth.FrameworkSemantics;

public interface IWidget
{
    void Announce();
}

public sealed class Widget : IWidget
{
    public void Announce() { }
}

public sealed class Catalogue
{
    public void AddScoped<TService, TImplementation>() { }
}

public static class Registrations
{
    public static void Configure(Catalogue services)
    {
        services.AddScoped<IWidget, Widget>();
    }
}

public static class Diagnostics
{
    public static void Report()
    {
        int x;
        System.Console.WriteLine(x);
    }
}
