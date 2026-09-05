// Two small interface/implementation pairs so iface_impl resolution has a
// primary example (IParcelRepository) and a second, unrelated one (ILabelPrinter).
namespace Courier.Api.Repositories;

public interface IParcelRepository
{
    string? Find(int id);
}

public interface ILabelPrinter
{
    string Print(string parcelId);
}
