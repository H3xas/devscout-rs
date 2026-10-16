namespace Fixture.Contracts
{
    public record SubmitOrder(string OrderId);
    public record OrderSubmitted(string OrderId);
    public record OrderNoted(string OrderId);
    public record CheckStock(string Sku);
    public record CheckCredit(string Account);
    public record InvoiceDue(string InvoiceId);
    public record ShipmentBooked(string ShipmentId);
}
