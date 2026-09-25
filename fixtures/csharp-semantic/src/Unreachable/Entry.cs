namespace Fixture.Domain
{
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
