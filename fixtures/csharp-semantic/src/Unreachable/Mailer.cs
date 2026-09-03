// Case e: Mailer.Enqueue shares a name with Queue<string>.Enqueue (see src/App/ApiClient.cs,
// also case e), but the Unreachable project is referenced by nobody -- probes guessing into a
// structurally unreachable project.
namespace Fixture.Unreachable;

public class Mailer
{
    private readonly List<string> _outbox = new();

    public void Enqueue(string m) => _outbox.Add(m);
}
