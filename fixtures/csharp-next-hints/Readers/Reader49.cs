namespace Toolworks.Pricing.Readers;

public class Reader49
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
