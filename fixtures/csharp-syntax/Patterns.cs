// References: type/property/positional/list/relational/logical patterns, switch statements and expressions.
namespace Syntax.Patterns;

record PatternShape(int Width, int Height)
{
    public PatternShape? Inner { get; init; }

    public int Area => Width * Height;
}

class PatternBox
{
    public PatternShape? Shape;

    public void Deconstruct(out int a, out int b)
    {
        a = 1;
        b = 2;
    }
}

enum PatternKind
{
    A,
    B,
}

class PatternUser
{
    public int Describe(object o, int[] xs, PatternBox box)
    {
        if (o is PatternShape s)
        {
            _ = s.Width;
        }

        switch (o)
        {
            case PatternShape:
                break;
        }

        _ = o is PatternShape { Width: > 0, Height: 1 };
        _ = o is PatternBox { Shape: PatternShape { Width: 1 } };
        _ = o is PatternBox { Shape.Width: 2 };
        _ = o is PatternShape(1, 2);
        _ = box is (1, 2);
        _ = box is (var a, _);
        _ = xs is [1, 2, ..];
        _ = xs is [var first, ..];
        _ = xs.Length is > 1 and < 10;
        _ = o is PatternKind.A or PatternKind.B;
        _ = o is not PatternKind.A;
        _ = o is 3;
        _ = o is PatternKind.A;
        _ = o is var anything;
        _ = o is PatternShape(_, _);

        int fromExpr = o switch
        {
            PatternShape p when p.Width > 0 => p.Height,
            PatternShape q => q.Height,
            _ => 0,
        };

        int fromArea = o switch
        {
            PatternShape r when r.Area > 0 => r.Area,
            PatternShape { Area: 0 } => 0,
            _ => -1,
        };
        _ = fromArea;

        switch (o)
        {
            case PatternShape p when p.Width > 0:
                fromExpr += p.Height;
                break;
            case PatternShape { Height: 0 }:
                break;
            case PatternKind.A or PatternKind.B:
                break;
            default:
                break;
        }

        int tupleResult = (fromExpr, xs.Length) switch
        {
            (1, _) => 1,
            _ => 0,
        };

        if (box.Shape is { Width: 1 } w)
        {
            fromExpr += w.Height;
        }

        _ = anything;
        return fromExpr + tupleResult;
    }
}
