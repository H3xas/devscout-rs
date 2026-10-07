namespace Toolworks.Pricing.Readers;

public class Reader42
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
