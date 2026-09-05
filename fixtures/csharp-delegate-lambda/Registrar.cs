using System;
using System.Linq.Expressions;

namespace Fixture.Domain;

public delegate void Wiring(Channel channel);

public class Registrar
{
    public void Register(Action<Options> configure) { }

    public void Pick(Func<Options, bool> selector) { }

    public void Select(Expression<Func<Options, object>> selector) { }

    public void Route(string name, Action<Options, Endpoint> configure) { }

    public void Wire(Wiring wiring) { }

    // Two overloads that disagree on the delegate's parameter type: a lambda
    // passed here stays untyped.
    public void Attach(Action<Options> configure) { }

    public void Attach(Action<Endpoint> configure) { }

    // Two overloads that agree: the lambda is typed.
    public void Same(Action<Options> configure) { }

    public void Same(Action<Options> configure, bool eager) { }

    public void Generic<T>(Action<T> configure) { }
}

public static class RegistrarExtensions
{
    public static void Extend(this Registrar registrar, Action<Endpoint> configure) { }
}
