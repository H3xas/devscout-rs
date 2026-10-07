namespace Toolworks.Pricing.Readers;

public class Reader05
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
