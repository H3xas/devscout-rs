namespace Truth.ImplementationIdentity;

public interface IGreeter
{
    void Greet();
}

public sealed class Greeter : IGreeter
{
    public void Greet() { }
}

public static class Caller
{
    public static void Run(Greeter greeter)
    {
        greeter.Greet();
    }
}
