// Case o: Send(this Order, string) is a same-named extension against Order's own two-parameter
// Send(int, int) (src/Domain/Order.cs) -- probes arity-gated call vouching: Worker.ProbeArity's
// `order.Send("x")`, one argument, admits no instance overload and falls through to this
// extension, while its `order.Send(1, 2)`, two arguments, binds the instance member directly.
namespace Fixture.Domain;

public static class OrderChannel
{
    public static void Send(this Order order, string channel) { }
}
