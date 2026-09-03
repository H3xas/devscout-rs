// Case f: partial class -- this file supplies the first-insertion part of Order; devscout's
// Def.file/also_in bookkeeping (src/graph.rs:17-23) must not double-count members across parts.
// Case enum: OrderStatus backs the two-spelling enum-member resolution (Ns.E.Member / Ns.E)
// exercised by Worker.cs's `order.Status == OrderStatus.Open` reference.
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

    public static Order Load(string id) => new Order
    {
        Id = int.TryParse(id, out var parsed) ? parsed : 0,
        Name = id,
        Total = 0m,
        Status = OrderStatus.Open,
    };
}
