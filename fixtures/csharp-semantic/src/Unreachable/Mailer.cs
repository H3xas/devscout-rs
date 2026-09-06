namespace Fixture.Unreachable;

public class Mailer
{
    private readonly List<string> _outbox = new();

    public void Enqueue(string m) => _outbox.Add(m);
}
