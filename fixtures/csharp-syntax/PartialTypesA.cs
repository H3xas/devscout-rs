// Declarations: partial class, partial method (declaring), partial property (declaring), partial struct, partial interface, partial record - part A.
namespace Syntax.Partial;

public partial class PartialHost
{
    partial void OnLoaded();

    public partial int Count { get; }

    public void Alpha()
    {
        OnLoaded();
    }
}

public partial struct PartialPoint
{
    public int X;

    public void SetX(int x)
    {
        X = x;
    }
}

public partial interface IPartialHook
{
    void Ping();
}

public partial record PartialRecord(int Id)
{
    public int DoubledId => Id * 2;
}
