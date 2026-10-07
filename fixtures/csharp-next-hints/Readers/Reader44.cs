namespace Toolworks.Pricing.Readers;

public class Reader44
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
