namespace Toolworks.Pricing.Readers;

public class Reader31
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
