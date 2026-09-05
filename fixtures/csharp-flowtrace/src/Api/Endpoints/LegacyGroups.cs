// The two properties reference each other on purpose, so a route walk
// that follows group-builder references needs a revisit guard to avoid
// looping forever. This class is never executed (its one call site,
// ParcelEndpoints.Register, is itself dead code) so the circular
// reference never actually runs.
namespace Courier.Api.Endpoints;

using Courier.Framework;

internal static class LegacyGroups
{
    public static RouteGroupBuilder Left => Right.MapGroup("left");
    public static RouteGroupBuilder Right => Left.MapGroup("right");
}
