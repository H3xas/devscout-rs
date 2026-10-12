namespace Toolworks.Pricing.Readers;

public class Reader09
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
