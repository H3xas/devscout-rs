// Case f: partial class -- second part of Order, declared in a different file than Order.cs;
// devscout must attribute Validate() to the same def id via also_in.
// Case g-this: `this.Name` -- a `this_expression` qualifier, resolved through the enclosing
// type's own declaring def across the partial-class boundary (Name is declared in Order.cs).
// Case i: ValidatePrevious calls through Previous, a field declared in Order.cs and used bare
// here, in the sibling partial file -- probes the cross-file field-typing table.
namespace Fixture.Domain;

public partial class Order
{
    public bool Validate() => this.Name.Length > 0 && Total >= 0;

    public bool ValidatePrevious() => Previous.Validate();
}
