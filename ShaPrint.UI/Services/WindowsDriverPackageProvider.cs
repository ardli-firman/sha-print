#if WINDOWS
using ShaPrint.Core.Abstractions;
using ShaPrint.Core.Network;
using ShaPrint.Platform.Windows;

namespace ShaPrint.UI.Services
{
    /// <summary>
    /// Windows implementation of <see cref="IDriverPackageProvider"/> wrapping the pnputil-backed
    /// <see cref="DriverPackageService"/> (migrated from ShaPrint.WpfApp). The service is
    /// constructed exactly like the WpfApp ServerViewModel did
    /// (<c>new DriverPackageService(new RealProcessRunner(), new RealFileSystem())</c>), so
    /// provisioning/cache behavior is unchanged.
    ///
    /// <para>NOTE: this adapter deliberately lives in ShaPrint.UI (under #if WINDOWS) rather than
    /// in ShaPrint.Platform.Windows/Adapters: it must implement <see cref="IDriverPackageProvider"/>,
    /// which is defined in ShaPrint.UI — placing it in Platform.Windows would create a circular
    /// project reference (ShaPrint.UI's Windows TFM references Platform.Windows). It is registered
    /// only for the net8.0-windows TFM in <c>AddPlatformWindows</c>.</para>
    /// </summary>
    public sealed class WindowsDriverPackageProvider : IDriverPackageProvider
    {
        private readonly DriverPackageService _service;

        public WindowsDriverPackageProvider()
        {
            _service = new DriverPackageService(new RealProcessRunner(), new RealFileSystem());
        }

        public Task<DriverPackageManifest?> GetDriverPackageAsync(string driverName)
            => _service.GetDriverPackageAsync(driverName);

        public Task<byte[]?> ReadPackageBytesAsync(string driverPackageId)
            => _service.ReadPackageBytesAsync(driverPackageId);
    }
}
#endif