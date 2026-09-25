namespace Truth.Overloads;

public sealed class Pinger
{
    public void Ping(int attempt) { }

    public void Ping(string label) { }
}

public static class Caller
{
    public static void Run(Pinger pinger)
    {
        pinger.Ping(1); pinger.Ping("x");
    }
}
