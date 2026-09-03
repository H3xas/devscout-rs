// Case p: Render is declared on the interface here; Widget.cs implements it directly. A
// Widget-typed receiver's call must bind Widget's own declaration, never this interface's --
// probes the class-vs-interface base-walk rule (a class-typed receiver never binds an
// interface member, at any depth).
namespace Fixture.Domain;

public interface IWidget
{
    void Render();
}
