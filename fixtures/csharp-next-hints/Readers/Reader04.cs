namespace Toolworks.Pricing.Readers;

public class Reader04
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
