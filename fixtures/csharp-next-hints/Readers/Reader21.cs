namespace Toolworks.Pricing.Readers;

public class Reader21
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
