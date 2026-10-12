namespace Toolworks.Pricing.Readers;

public class Reader27
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
