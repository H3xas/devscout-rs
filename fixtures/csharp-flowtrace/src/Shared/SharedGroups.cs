// A route group declared in a referenced project (Shared), rather than in
// the entry-point project, so cross-project route-group resolution has a
// fixture to exercise.
namespace Courier.Framework;

public static class SharedGroups
{
    public static readonly RouteGroupBuilder Admin = WebApplication.Create().MapGroup("admin");
}
