namespace Toolworks.Pricing.Readers;

public class Reader16
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
