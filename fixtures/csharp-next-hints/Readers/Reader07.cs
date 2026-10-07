namespace Toolworks.Pricing.Readers;

public class Reader07
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
