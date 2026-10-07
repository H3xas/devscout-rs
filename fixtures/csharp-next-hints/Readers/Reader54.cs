namespace Toolworks.Pricing.Readers;

public class Reader54
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
