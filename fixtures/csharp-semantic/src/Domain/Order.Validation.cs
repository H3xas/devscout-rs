namespace Fixture.Domain;

public partial class Order
{
    public bool Validate() => this.Name.Length > 0 && Total >= 0;

    public bool ValidatePrevious() => Previous.Validate();
}
