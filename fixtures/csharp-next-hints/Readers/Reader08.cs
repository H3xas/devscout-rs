namespace Toolworks.Pricing.Readers;

public class Reader08
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
