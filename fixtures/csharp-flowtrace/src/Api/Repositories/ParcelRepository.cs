// Implementations for the two repository-shaped interfaces declared alongside.
namespace Courier.Api.Repositories;

public sealed class ParcelRepository : IParcelRepository
{
    public string? Find(int id) => $"parcel-{id}";
}

public sealed class ZplLabelPrinter : ILabelPrinter
{
    public string Print(string parcelId) => $"^XA^FD{parcelId}^FS^XZ";
}
