namespace Toolworks.Pricing.Readers;

public class Reader24
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
