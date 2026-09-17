namespace Truth.NativeDispatchCounterexample;

public interface IContract
{
    void Run(int value);
}

public sealed class Unrelated
{
    public void Run(string value) { }
}

public sealed class Catalogue
{
    public void AddScoped<T, U>() { }
}

public static class Caller
{
    public static void Use(Catalogue catalogue)
    {
        catalogue.AddScoped<IContract, Unrelated>();
    }
}
