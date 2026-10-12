namespace Toolworks.Pricing.Readers;

public class Reader19
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
