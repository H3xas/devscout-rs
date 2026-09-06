// Statements: if/for/foreach/while/do, switch/goto, try/catch/finally, lock, using, checked/unchecked, local scoping.
namespace Syntax.Statements;

class StmtRes : IDisposable
{
    public void Dispose()
    {
    }

    public void Touch()
    {
    }
}

class StmtGate
{
    public object Sync = new();
    public int Value;
}

class StmtError : Exception
{
    public int Code;
}

class StmtUser
{
    public void Run(int[] xs, StmtGate? gate)
    {
        int a = 1;
        if (a == 1)
        {
            a = 2;
            _ = gate?.Sync;
        }
        else if (a == 2)
        {
            a = 3;
        }
        else
        {
            a = 4;
        }

        for (int i = 0; i < xs.Length; i++)
        {
            if (xs[i] < 0)
            {
                continue;
            }

            if (xs[i] > 100)
            {
                break;
            }
        }

        foreach (var x in xs)
        {
            _ = x;
        }

        foreach (int typed in xs)
        {
            _ = typed;
        }

        int w = 0;
        while (w < 3)
        {
            w++;
        }

        do
        {
            w--;
        }
        while (w > 0);

        switch (a)
        {
            case 1:
                goto case 2;
            case 2:
                goto default;
            default:
                break;
        }

    label:
        if (a > 10)
        {
            goto label;
        }

        try
        {
            if (a == 4)
            {
                throw new StmtError();
            }
        }
        catch (StmtError e) when (e.Code > 0)
        {
        }
        catch (Exception)
        {
        }
        catch
        {
        }
        finally
        {
        }

        try
        {
            throw new StmtError();
        }
        catch
        {
            throw;
        }

        var g = gate ?? throw new StmtError();
        lock (g.Sync)
        {
            g.Value++;
        }

        using (var r = new StmtRes())
        {
            r.Touch();
        }

        using var r2 = new StmtRes();
        r2.Touch();

        checked
        {
            int sum = a + w;
            _ = sum;
        }

        unchecked
        {
            int sum = a + w;
            _ = sum;
        }

        ;

        {
        }

        int cond = a > 0 ? 1 : 0;
        int? maybe = null;
        int coalesced = maybe ?? 0;
        maybe ??= 5;
        a += 1;
        bool isNull = maybe is null;

        string label2 = a switch
        {
            1 => "one",
            _ => "other",
        };

        const int limit = 10;
        int p = 1, q = 2;

        _ = cond;
        _ = coalesced;
        _ = isNull;
        _ = label2;
        _ = limit;
        _ = p;
        _ = q;
    }

    public void Loops(Dictionary<int, StmtRes> map, List<StmtRes> list)
    {
        foreach (var inferred in list)
        {
            inferred.Touch();
        }

        foreach (StmtRes declared in list)
        {
            declared.Touch();
        }

        foreach (var pair in map)
        {
            pair.Value.Touch();
        }

        foreach (KeyValuePair<int, StmtRes> kv in map)
        {
            kv.Value.Touch();
        }
    }
}
