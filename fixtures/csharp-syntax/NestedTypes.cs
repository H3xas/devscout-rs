// Declarations: nested class, nested enum, nested struct, nested interface, nested record, nested delegate, doubly-nested class.
namespace Syntax.Nesting;

public class NestOuter
{
    public class NestInner
    {
        public int Tag;
    }

    public enum NestMode
    {
        Slow,
        Fast,
    }

    public struct NestPoint
    {
        public int X;
        public int Y;
    }

    public interface INestHook
    {
        void Hook();
    }

    public record NestRecord(int Id);

    public delegate void NestCallback();

    public class NestInner2
    {
        public class NestDeep
        {
            public int Depth;
        }
    }
}

public class NestConsumer : NestOuter.INestHook
{
    private NestOuter.NestInner _inner = new();

    public void Hook()
    {
    }

    public void Run()
    {
        var deep = new NestOuter.NestInner2.NestDeep { Depth = 3 };
        int depth = deep.Depth;
        var mode = NestOuter.NestMode.Fast;
        var point = new NestOuter.NestPoint { X = 1, Y = 2 };
        var record = new NestOuter.NestRecord(1);
        NestOuter.NestCallback callback = Hook;
        callback();
        _inner.Tag = depth;
        Console.WriteLine($"{mode}{point.X}{point.Y}{record}{_inner.Tag}");
    }
}
