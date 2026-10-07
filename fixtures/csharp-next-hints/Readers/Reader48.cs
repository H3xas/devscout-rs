namespace Toolworks.Pricing.Readers;

public class Reader48
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
