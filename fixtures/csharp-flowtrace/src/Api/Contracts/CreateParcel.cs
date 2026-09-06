// A plain request contract used by both the controller and the minimal-API
// endpoints, so ctor_field resolution has a real fixture type to bind to.
namespace Courier.Api.Contracts;

public sealed record CreateParcel(string Recipient, string Address);
