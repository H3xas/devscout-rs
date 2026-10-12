namespace Toolworks.Pricing.Readers;

public class Reader41
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
