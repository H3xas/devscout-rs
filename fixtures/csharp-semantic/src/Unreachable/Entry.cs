namespace Fixture.Domain
{
    // `Value`'s own declared type is `Entry`, not a predefined type: the
    // property-owner hop that keeps `.Page` resolving after `.Value` reads
    // `Value`'s OWN declared type to continue the chain, and a predefined-
    // typed property carries no such fact at all.
    public readonly struct Entry
    {
        public Entry Value => this;

        public int Page { get; }
    }

    public class EntryReader
    {
        public void ReadMaybe()
        {
            Entry? found = null;

            var page = found.Value.Page;
        }

        public void ReadPresent()
        {
            Entry present = default;

            var value = present.Value;
        }
    }
}
