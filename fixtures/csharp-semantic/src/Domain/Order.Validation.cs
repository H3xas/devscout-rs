// Case f: partial class -- second part of Order, declared in a different file than Order.cs;
// devscout must attribute Validate() to the same def id via also_in.
// Case g-this: `this.Name` -- `this` is not an accepted member-access qualifier
// (src/extract.rs:697-720), so this call is a documented recall miss.
namespace Fixture.Domain;

public partial class Order
{
    public bool Validate() => this.Name.Length > 0 && Total >= 0;
}
