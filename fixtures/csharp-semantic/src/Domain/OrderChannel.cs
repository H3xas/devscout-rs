namespace Fixture.Domain;

public static class OrderChannel
{
    public static void Send(this Order order, string channel) { }
}
