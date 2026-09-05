// References: lambda expressions, anonymous methods, method groups, LINQ extension-method chains.
namespace Syntax.Lambdas;

class LambdaOwner
{
    public string? Name;

    public int Rank() => 1;
}

class LambdaItem
{
    public int Value;
    public LambdaOwner Owner = new();

    public int Score() => Value;
}

class LambdaUser
{
    static int Twice(int n) => n * 2;

    int Apply(Func<LambdaItem, int> f) => 0;

    public void Typed(List<LambdaItem> items)
    {
        _ = items.Where(entry => entry.Score() > 0);
        _ = items.Select(single => single.Owner.Rank());
        _ = items.Where((LambdaItem spelled) => spelled.Score() > 0);
        Predicate<LambdaItem> named = delegate (LambdaItem candidate) { return candidate.Score() > 0; };
        _ = named;
    }

    public void Run(List<LambdaItem> items)
    {
        Func<int, int> f = x => x + 1;
        Func<int, int, int> g = (a, b) => a + b;
        Action h = () => { };
        Func<int, int> st = static x => x;
        var typed = (int x) => x;
        var withReturn = int (int x) => x;
        var withDefault = (int x = 1) => x;
        var withParams = (params int[] xs) => xs.Length;
        Func<LambdaItem, int> sel = item => item.Value;
        Func<LambdaItem, int> deep = item => item.Owner.Rank();
        Func<LambdaItem, string?> name = item => item.Owner.Name;

        Predicate<LambdaItem> p = delegate (LambdaItem i)
        {
            return i.Score() > 0;
        };

        Action anon = delegate { };
        Func<int, int> mg = Twice;

        _ = items.Select(i => i.Value);
        _ = items.Where(i => i.Owner.Name != null).Select(i => i.Score());
        _ = items.OrderBy(i => i.Owner.Rank()).First();
        _ = items.Select((i, idx) => i.Value + idx);
        _ = items.Select((i, idx) => i.Owner);
        _ = items.Select(i => i.Owner).Select(o => o.Rank());

        Func<Task<int>> af = async () => await Task.FromResult(1);

        Func<LambdaOwner, int> block = _ =>
        {
            LambdaOwner owner = new();
            return owner.Rank();
        };

        _ = items.Where((LambdaItem i) => i.Score() > 0);
        Func<int, int, int> d = (_, _) => 0;
        _ = Apply(i => i.Score());
        _ = items.Select(i => items.Where(j => j.Value > i.Value));

        _ = f;
        _ = g;
        _ = h;
        _ = st;
        _ = typed;
        _ = withReturn;
        _ = withDefault;
        _ = withParams;
        _ = sel;
        _ = deep;
        _ = name;
        _ = p;
        _ = anon;
        _ = mg;
        _ = af;
        _ = block;
        _ = d;
    }
}
