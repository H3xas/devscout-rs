namespace Toolworks.Pricing.Readers;

public class Reader47
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
