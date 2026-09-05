// Lists IStamp among its bases but satisfies it through Stamper's inherited Stamp(): a
// Deep-typed receiver binds Stamper, never the interface.
namespace Fixture.Shapes;

public class Deep : Stamper, IStamp
{
}
