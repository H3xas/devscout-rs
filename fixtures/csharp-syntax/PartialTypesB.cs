// Declarations: partial class, partial method (implementing), partial property (implementing), partial struct, partial interface, partial record - part B.
namespace Syntax.Partial;

public partial class PartialHost
{
    partial void OnLoaded()
    {
    }

    public partial int Count => 1;

    public void Beta()
    {
        Alpha();
        var c = Count;
        Console.WriteLine(c);
    }
}

public partial struct PartialPoint
{
    public int Y;

    public void SetY(int y)
    {
        Y = y;
    }
}

public partial interface IPartialHook
{
    void Pong();
}

public partial record PartialRecord
{
    public int TripledId => Id * 3;
}

public class PartialUser : IPartialHook
{
    public void Ping()
    {
    }

    public void Pong()
    {
    }

    public void Run()
    {
        var host = new PartialHost();
        host.Beta();
        var point = new PartialPoint();
        point.SetX(1);
        point.SetY(2);
        var record = new PartialRecord(5);
        int doubled = record.DoubledId;
        int tripled = record.TripledId;
        IPartialHook hook = this;
        hook.Ping();
        hook.Pong();
        Console.WriteLine($"{point.X}{point.Y}{doubled}{tripled}");
    }
}
