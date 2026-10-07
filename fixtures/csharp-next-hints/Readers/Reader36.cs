namespace Toolworks.Pricing.Readers;

public class Reader36
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
