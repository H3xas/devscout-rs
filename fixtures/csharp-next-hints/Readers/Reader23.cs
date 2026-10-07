namespace Toolworks.Pricing.Readers;

public class Reader23
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
