namespace Toolworks.Pricing.Readers;

public class Reader02
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
