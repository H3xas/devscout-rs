namespace Toolworks.Pricing.Readers;

public class Reader06
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
