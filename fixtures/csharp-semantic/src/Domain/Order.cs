// Case f: partial class -- this file supplies the first-insertion part of Order; devscout's
// Def.file/also_in bookkeeping (src/graph.rs:17-23) must not double-count members across parts.
// Case enum: OrderStatus backs the two-spelling enum-member resolution (Ns.E.Member / Ns.E)
// exercised by Worker.cs's `order.Status == OrderStatus.Open` reference.
// Case i: Previous is a field declared here and used bare as a receiver from
// Order.Validation.cs, a different file of the same partial class -- probes the cross-file
// field-typing table (see AuditableEntity.Root, case j, for the same table walked across a
// base instead of a partial sibling).
// Case k: TryGet's caller writes `out Order r` inline at the call site -- probes a local typed
// through an explicit `out` designation, alongside a cast and an `is` pattern
// (Worker.ProbeTypedLocals).
// Case o: Send(int, int) is a two-parameter instance overload; OrderChannel.Send(this Order,
// string) (src/Domain/OrderChannel.cs) is a same-named extension taking one -- probes
// arity-gated call vouching: a call whose argument count admits no instance overload falls
// through to the extension.
namespace Fixture.Domain;

public enum OrderStatus
{
    Open,
    Closed,
}

public partial class Order
{
    public int Id { get; set; }
    public string Name { get; set; } = string.Empty;
    public decimal Total { get; set; }
    public OrderStatus Status { get; set; }
    public Order Previous = null!;

    public static Order Load(string id) => new Order
    {
        Id = int.TryParse(id, out var parsed) ? parsed : 0,
        Name = id,
        Total = 0m,
        Status = OrderStatus.Open,
    };

    public static void TryGet(out Order value) => value = new Order();

    public void Send(int amount, int retries) { }
}
