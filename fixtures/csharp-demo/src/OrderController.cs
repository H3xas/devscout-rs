namespace Shop.Data;

public class OrderController
{
    private readonly OrderRepository _repository = new OrderRepository();

    public int GetTotal(int orderId)
    {
        return _repository.GetOrderTotal(orderId);
    }
}
