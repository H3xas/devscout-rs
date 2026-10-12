namespace Toolworks.Pricing.Readers;

public class Reader51
{
    public decimal Read(IPriceSource source) => source.Price("sku");
}
