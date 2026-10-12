namespace Toolworks.Pricing.Readers;

public class Reader11
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
