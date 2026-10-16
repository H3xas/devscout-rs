namespace NegativeBinding.Shadow
{
    public class Panel
    {
        public void Paint() { }
    }

    namespace Inner
    {
        public class Panel
        {
            public void Paint() { }
        }

        public class Studio
        {
            public void Sketch()
            {
                var panel = new Panel();
                panel.Paint();
            }
        }
    }
}

namespace NegativeBinding.Tools
{
    public class Seal
    {
        public void Press() { }
    }
}

namespace NegativeBinding.Tools.Live
{
    public class Gauge
    {
        public void Read() { }
    }

    public class Seal
    {
        public void Press() { }
    }
}

namespace NegativeBinding.Host
{
    using NegativeBinding.Tools;

    public class Seal
    {
        public void Press() { }
    }

    public class Clerk
    {
        public void Work()
        {
            var seal = new Seal();
            seal.Press();
        }
    }
}

namespace NegativeBinding.Depot
{
    using NegativeBinding.Tools;
    using Seal = NegativeBinding.Tools.Live.Seal;

    public class Packer
    {
        public void Pack()
        {
            var seal = new Seal();
            seal.Press();
        }
    }
}

namespace NegativeBinding.Workshop
{
    using NegativeBinding.Tools.Live;

    public class Bench
    {
        public void Measure()
        {
            var gauge = new global::NegativeBinding.Tools.Gauge();
            gauge.Read();
        }
    }
}
