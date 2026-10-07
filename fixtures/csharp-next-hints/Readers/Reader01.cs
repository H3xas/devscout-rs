namespace Toolworks.Pricing.Readers;

public class Reader01
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
