namespace Toolworks.Pricing.Readers;

public class Reader32
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
