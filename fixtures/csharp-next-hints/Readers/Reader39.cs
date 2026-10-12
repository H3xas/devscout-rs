namespace Toolworks.Pricing.Readers;

public class Reader39
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
