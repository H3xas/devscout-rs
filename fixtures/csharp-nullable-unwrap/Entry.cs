namespace Fixture.Nullable
{
    public readonly struct Entry
    {
        public Entry Value { get; }

        public int Page { get; }
    }

    public class Reader
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
