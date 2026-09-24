using Moq;

namespace BusNonPublicOverload
{
    public class FlareExpectation
    {
        public void Arrange(Mock<IRelayBus> relay)
        {
            relay.Setup(x => x.Publish<Flare>(Match.Any<Flare>(), default));
        }
    }
}
