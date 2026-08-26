using ShaPrint.Core.Network;

namespace ShaPrint.UI.Services;

/// <summary>
/// Abstraction over the server-side driver package service, so the shared
/// <c>DiscoveryServerService</c>/<c>PrintReceiverService</c> protocol code (driver package
/// request/chunk/complete/error on the 9877 channel) does not depend on the Windows-only
/// <c>DriverPackageService</c> (pnputil export + cache, currently living in
/// <c>ShaPrint.WpfApp/Services/Server</c>).
///
/// Windows wires an implementation that wraps the existing <c>DriverPackageService</c>;
/// on macOS/Linux no provider is registered so driver sharing resolves to "unavailable"
/// and requests are rejected gracefully (same behavior as a server with sharing disabled).
/// </summary>
public interface IDriverPackageProvider
{
    Task<DriverPackageManifest?> GetDriverPackageAsync(string driverName);

    Task<byte[]?> ReadPackageBytesAsync(string driverPackageId);
}