using Volumes;

namespace Readers;

public class Chapter { }

public class Verse { }

public class Curator
{
    public void Catalog()
    {
        Anthology<Chapter>.Collate();
        Anthology.Collate();
        Anthology<Chapter, Verse>.Collate();
    }
}
