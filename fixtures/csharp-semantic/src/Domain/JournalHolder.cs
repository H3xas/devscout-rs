using Fixture.Ext.Contracts;

namespace Fixture.Domain;

public class JournalHolder
{
    public IJournal Journal { get; }

    public JournalHolder(IJournal journal)
    {
        Journal = journal;
    }

    public void Write() => Journal.Record("x");
}
