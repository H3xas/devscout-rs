namespace Toolworks.Pricing.Readers;

public class Reader15
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
