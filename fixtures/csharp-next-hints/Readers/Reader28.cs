namespace Toolworks.Pricing.Readers;

public class Reader28
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
