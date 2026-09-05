// The other half of the split: no base list here, only a private helper
// used by the part that does carry the base list. No consume fact should
// come from this file on its own.
namespace Courier.Api.Consumers;

public sealed partial class ReturnRequestedConsumer
{
    private static void Record(string parcelId)
    {
        _ = parcelId;
    }
}
