using App.Transports.Fabric;

namespace App.Bus
{
    public class Other
    {
        public ExchangeType Pick() => ExchangeType.Fanout;
    }
}
