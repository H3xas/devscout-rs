namespace Toolworks.Pricing.Readers;

public class Reader22
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
