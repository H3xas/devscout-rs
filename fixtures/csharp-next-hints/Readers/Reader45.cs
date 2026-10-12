namespace Toolworks.Pricing.Readers;

public class Reader45
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
