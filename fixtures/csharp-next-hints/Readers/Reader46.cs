namespace Toolworks.Pricing.Readers;

public class Reader46
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
