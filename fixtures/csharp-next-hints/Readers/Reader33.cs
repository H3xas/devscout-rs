namespace Toolworks.Pricing.Readers;

public class Reader33
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
