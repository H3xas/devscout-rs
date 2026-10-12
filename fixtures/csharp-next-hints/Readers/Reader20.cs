namespace Toolworks.Pricing.Readers;

public class Reader20
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
