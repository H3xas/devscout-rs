namespace Toolworks.Pricing.Readers;

public class Reader17
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
