namespace Toolworks.Pricing.Readers;

public class Reader53
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
