namespace BusSignals
{
    // `AisleLedger.Publish` is a same-named, same-arity method the
    // repository itself declares, taking `LoanRequested` -- the message's
    // own concrete resolved type -- as its sole input. A call spelled
    // `Publish` on this shape dispatches nothing: it is a ledger write, not
    // a bus send, and must earn no hop despite the verb and message both
    // matching a real route elsewhere in this fixture.
    public class AisleLedger
    {
        public void Publish(LoanRequested entry)
        {
        }
    }

    public class AisleLedgerWriter
    {
        public void Record(AisleLedger ledger)
        {
            var entry = new LoanRequested();
            ledger.Publish(entry);
        }
    }
}
