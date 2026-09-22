namespace Truth.FailedBinding;

public sealed class Payload { }

public static class Runner
{
    public static void Run()
    {
        MissingApi.Publish(new Payload());
    }
}
