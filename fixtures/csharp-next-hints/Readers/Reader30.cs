namespace Toolworks.Pricing.Readers;

public class Reader30
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
