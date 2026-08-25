namespace Shop.Data;

public interface IOrderRepository
{
    int GetOrderTotal(int orderId);
}
