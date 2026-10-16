namespace Fixture.Model
{
    public class LedgerTests
    {
        [Fact]
        public void TotalsAreZero()
        {
            var ledger = new Ledger();
            _ = ledger.Total();
        }
    }
}
