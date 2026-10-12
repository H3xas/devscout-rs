namespace Toolworks.Pricing.Readers;

public class Reader50
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
