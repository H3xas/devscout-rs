// Directives: global using (plain, static, alias), using static, using alias, tuple and generic aliases.
global using System.Collections.Concurrent;
global using static System.Math;
global using GlobalBuilder = System.Text.StringBuilder;
using static Syntax.Usings.UsingStatics;
using Builder = System.Text.StringBuilder;
using Pair = (int Left, int Right);
using ItemList = System.Collections.Generic.List<Syntax.Usings.UsingItem>;

namespace Syntax.Usings;

public static class UsingStatics
{
    public static int Counter;

    public static void Reset()
    {
        Counter = 0;
    }
}

public class UsingItem
{
    public int Value;
}

public class UsingConsumer
{
    public void Run()
    {
        Reset();
        Counter++;

        var builder = new Builder();
        builder.Append("x");

        var global = new GlobalBuilder();
        global.Append("y");

        Pair pair = (1, 2);
        _ = pair.Left;

        ItemList items = new();
        items.Add(new UsingItem());
        _ = items[0].Value;

        _ = Abs(-1);

        var bag = new ConcurrentBag<int>();
        bag.Add(1);
    }
}
