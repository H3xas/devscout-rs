using Volumes;

namespace Readers;

public class Piece { }

public class Cartographer
{
    public void Catalog()
    {
        Codex.Entry();
        Codex<Piece>.Entry();
    }
}
