// References: tuple literals, tuple deconstruction, with-expressions, ranges and indices.
namespace Syntax.Tuples;

record TupleRecord(string Name, int Age)
{
    public TupleRecord? Parent { get; init; }
}

record struct TuplePoint(int X, int Y);

class TupleHolder
{
    public int Left;
    public int Right;

    public void Deconstruct(out int l, out int r)
    {
        l = Left;
        r = Right;
    }

    public (int Id, string Name) Pair() => (1, "a");

    public (TupleRecord Rec, int N) RecPair() => (new("n", 1), 2);
}

class TupleUser
{
    public void Run(TupleHolder holder, Dictionary<int, TupleHolder> dict, int[] arr)
    {
        var t = (1, "a");
        (int Id, string Name) named = (2, "b");
        _ = named.Name;
        _ = t.Item1;

        var (id, name) = holder.Pair();
        (int a, string b) = holder.Pair();
        (a, b) = holder.Pair();
        var (l, r) = holder;
        var (_, only) = holder.Pair();
        var nested = ((1, 2), 3);

        foreach (var (k, v) in dict)
        {
            v.Pair();
            _ = k;
        }

        (a, id) = (id, a);

        var rec = new TupleRecord("x", 1);
        var rec2WithChange = rec with { Name = "x" };
        var pt = new TuplePoint(1, 2);
        var pt2 = pt with { X = 1 };
        var anon = new { Id = 1 };
        var anon2 = anon with { Id = 2 };

        _ = arr[^1];
        _ = arr[1..];
        _ = arr[..^1];
        Range rg = 1..3;
        Index ix = ^1;

        var (rec2, n) = holder.RecPair();
        _ = rec2.Parent?.Name;

        _ = name;
        _ = l;
        _ = r;
        _ = only;
        _ = nested;
        _ = rec2WithChange;
        _ = pt2;
        _ = anon2;
        _ = rg;
        _ = ix;
        _ = n;
    }
}
