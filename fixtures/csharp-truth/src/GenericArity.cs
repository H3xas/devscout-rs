namespace Truth.GenericArity;

public sealed class Handler
{
    public void Handle<T>(T value) { }

    public void Handle<T1, T2>(T1 first, T2 second) { }
}

public static class Caller
{
    public static void Run(Handler handler)
    {
        handler.Handle(1);
        handler.Handle(1, "two");
    }
}
