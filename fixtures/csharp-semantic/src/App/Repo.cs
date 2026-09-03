// Case l: LoadAsync returns Task<Order>; Worker.ProbeAwaitedStatic awaits a call to this
// static-qualified method and uses the result -- probes the one-layer Task<T> unwrap for an
// awaited static-qualifier local.
using Fixture.Domain;

namespace Fixture.App;

public static class Repo
{
    public static Task<Order> LoadAsync() => Task.FromResult(Order.Load("r"));
}
