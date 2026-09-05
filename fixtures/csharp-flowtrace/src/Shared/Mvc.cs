// Stand-ins for an MVC-style controller framework: routing attributes,
// parameter-binding markers, and the base controller with its result types.
namespace Courier.Framework;

[AttributeUsage(AttributeTargets.Class | AttributeTargets.Method, AllowMultiple = true)]
public sealed class RouteAttribute : Attribute
{
    public RouteAttribute(string template) => Template = template;

    public string Template { get; }
}

[AttributeUsage(AttributeTargets.Method)]
public sealed class HttpGetAttribute : Attribute
{
    public HttpGetAttribute(string? template = null) => Template = template;

    public string? Template { get; }
}

[AttributeUsage(AttributeTargets.Method)]
public sealed class HttpPostAttribute : Attribute
{
    public HttpPostAttribute(string? template = null) => Template = template;

    public string? Template { get; }
}

[AttributeUsage(AttributeTargets.Method)]
public sealed class HttpPutAttribute : Attribute
{
    public HttpPutAttribute(string? template = null) => Template = template;

    public string? Template { get; }
}

[AttributeUsage(AttributeTargets.Method)]
public sealed class HttpDeleteAttribute : Attribute
{
    public HttpDeleteAttribute(string? template = null) => Template = template;

    public string? Template { get; }
}

[AttributeUsage(AttributeTargets.Method)]
public sealed class HttpPatchAttribute : Attribute
{
    public HttpPatchAttribute(string? template = null) => Template = template;

    public string? Template { get; }
}

[AttributeUsage(AttributeTargets.Parameter)]
public sealed class FromBodyAttribute : Attribute
{
}

[AttributeUsage(AttributeTargets.Parameter)]
public sealed class FromQueryAttribute : Attribute
{
}

[AttributeUsage(AttributeTargets.Parameter)]
public sealed class FromRouteAttribute : Attribute
{
}

public interface IActionResult
{
}

public sealed class OkResult : IActionResult
{
    public OkResult(object? value = null) => Value = value;

    public object? Value { get; }
}

public sealed class NotFoundResult : IActionResult
{
}

public abstract class ControllerBase
{
    protected IActionResult Ok(object? value = null) => new OkResult(value);

    protected IActionResult NotFound() => new NotFoundResult();
}
