namespace Toolworks.Pricing.Readers;

public class Reader38
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
