namespace Storefront;

public class CheckoutControllerTests
{
    public void PlaceOrder_Succeeds()
    {
        var controller = new CheckoutController();
        controller.PlaceOrder();
    }
}
