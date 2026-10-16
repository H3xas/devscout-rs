using NegativeBinding.Ext.Far;

namespace NegativeBinding.Ext
{
    public class Marker
    {
        public void Stamp() { }
    }

    public static class MarkerExtensions
    {
        public static void Stamp(this Marker marker) { }
    }

    public class Stamping
    {
        public void Go()
        {
            var marker = new Marker();
            marker.Stamp();
        }
    }

    public class Meter
    {
        public void Scale(int factor) { }
    }

    public static class MeterExtensions
    {
        public static void Scale(this Meter meter, string factor) { }
    }

    public class Metering
    {
        public void Go()
        {
            var meter = new Meter();
            meter.Scale("x2");
        }
    }

    public class Tuner { }

    public static class LocalHooks
    {
        public static void Tune(this Tuner tuner) { }
    }

    public class Tuning
    {
        public void Go()
        {
            var tuner = new Tuner();
            tuner.Tune();
        }
    }

    public interface ILoadable { }

    public class Loader : ILoadable { }

    public static class LoadableHooks
    {
        public static void Prime(this ILoadable target) { }
    }

    public static class LoaderHooks
    {
        public static void Prime(this Loader target) { }
    }

    public class Priming
    {
        public void Go()
        {
            var loader = new Loader();
            loader.Prime();
        }
    }
}

namespace NegativeBinding.Ext.Far
{
    public static class DistantHooks
    {
        public static void Tune(this NegativeBinding.Ext.Tuner tuner) { }
    }
}
