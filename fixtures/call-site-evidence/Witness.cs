namespace Evidence;

// A tiny, package-free, independently authored synthetic target: one member
// name used from several call shapes, plus an overload and an async twin so
// every named witness shape below has something real to call.
public class Ledger
{
    public void Record()
    {
    }

    public void Record(int amount)
    {
    }

    public System.Threading.Tasks.Task RecordAsync()
    {
        return System.Threading.Tasks.Task.CompletedTask;
    }
}

// Every method below is one independently authored witness case, named for
// the shape it demonstrates -- expected call sites for each are recorded
// by hand in EXPECTED.md before any devscout command is run against this
// fixture (see that file's own header for the rule this order enforces).
public class Caller
{
    private readonly Ledger _ledger = new Ledger();

    // Witness: repeated calls to one target, on two different lines.
    public void RepeatedCallsDifferentLines()
    {
        _ledger.Record();
        _ledger.Record();
    }

    // Witness: two calls to one target on the SAME line.
    public void TwoCallsOneLine()
    {
        _ledger.Record(); _ledger.Record();
    }

    // Witness: overload ambiguity -- two calls on adjacent lines that name
    // the same member but bind different overloads by argument shape.
    public void OverloadAmbiguity()
    {
        _ledger.Record();
        _ledger.Record(7);
    }

    // Witness: recursion -- a call site inside the callee's own body. Qualified
    // with `this.` on purpose: an unqualified same-type call has no member
    // access expression to extract a reference from, so it establishes
    // nothing on the native surface -- a capability gap this fixture records
    // separately rather than silently working around. `this.` keeps this
    // witness inside what native devscout already establishes, like every
    // other case here.
    public void Recurse(int depth)
    {
        if (depth <= 0)
        {
            return;
        }
        this.Recurse(depth - 1);
    }

    // Witness: an awaited sequence -- two awaited calls to the same async
    // target, one after another.
    public async System.Threading.Tasks.Task AwaitedSequence()
    {
        await _ledger.RecordAsync();
        await _ledger.RecordAsync();
    }

    // Witness: a parallel launch/join -- two calls launched before either is
    // awaited, then joined together.
    public async System.Threading.Tasks.Task ParallelLaunchJoin()
    {
        var first = _ledger.RecordAsync();
        var second = _ledger.RecordAsync();
        await System.Threading.Tasks.Task.WhenAll(first, second);
    }
}
