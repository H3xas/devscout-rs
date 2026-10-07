namespace Toolworks.Pricing.Readers;

public class Reader14
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
