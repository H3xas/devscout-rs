namespace Toolworks.Pricing.Readers;

public class Reader26
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
