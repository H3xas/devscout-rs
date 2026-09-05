using System;
using Fixture.Domain;

namespace Fixture.App;

public class Host
{
    private readonly Registrar _registrar = new Registrar();

    public void Run()
    {
        _registrar.Register(x => x.Configure());
        _registrar.Pick(o => o.Enabled);
        _registrar.Select(s => s.Enabled);
        _registrar.Route("main", (opt, ep) => ep.Bind());
        _registrar.Wire(c => c.Open());
        _registrar.Attach(a => a.Configure());
        _registrar.Same(t => t.Tune());
        _registrar.Generic<Options>(g => g.Configure());
        _registrar.Extend(p => p.Bind());
        Register(y => y.Configure());
    }

    private void Register(Action<Options> configure) { }
}
