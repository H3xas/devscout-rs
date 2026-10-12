namespace Toolworks.Pricing.Readers;

public class Reader25
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
