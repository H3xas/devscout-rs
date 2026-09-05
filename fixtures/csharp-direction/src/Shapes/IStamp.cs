// A member name shared between an interface and an unrelated base class: Stamper.cs declares
// Stamp() publicly, MixedExplicit.cs derives from Stamper and implements this interface's Stamp()
// explicitly, so a class-typed receiver binds Stamper's member and an IStamp-typed one binds this
// declaration.
namespace Fixture.Shapes;

public interface IStamp
{
    void Stamp();
}
