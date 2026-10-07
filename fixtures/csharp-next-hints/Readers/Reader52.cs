namespace Toolworks.Pricing.Readers;

public class Reader52
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
