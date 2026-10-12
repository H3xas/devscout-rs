namespace Toolworks.Pricing.Readers;

public class Reader12
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
