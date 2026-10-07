namespace Toolworks.Pricing.Readers;

public class Reader40
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
