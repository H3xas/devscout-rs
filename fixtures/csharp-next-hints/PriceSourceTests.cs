using Xunit;

namespace Toolworks.Pricing.Tests;

public class PriceSourceTests
{
    [Fact]
    public void Price_IsZeroForAnUnpricedSku()
    {
        IPriceSource source = new CachePriceSource();
        Assert.Equal(0m, source.Price("unpriced"));
    }
}
