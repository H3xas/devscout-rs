namespace Toolworks.Pricing.Readers;

public class Reader03
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
