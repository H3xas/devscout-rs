namespace Toolworks.Pricing.Readers;

public class Reader29
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
