namespace Toolworks.Pricing.Readers;

public class Reader55
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
