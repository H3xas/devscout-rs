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
