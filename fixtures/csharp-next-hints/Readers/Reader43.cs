namespace Toolworks.Pricing.Readers;

public class Reader43
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
