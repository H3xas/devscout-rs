namespace Toolworks.Pricing;

public interface IPriceSource
{
    decimal Price(string sku);
}
