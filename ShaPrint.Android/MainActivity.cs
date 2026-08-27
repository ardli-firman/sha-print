using Android.App;
using Android.Content;
using Android.Content.PM;
using Android.OS;
using Avalonia;
using Avalonia.Android;
using Microsoft.Extensions.DependencyInjection;
// Namespace ShaPrint.Android shadows the global `Android` namespace for inline qualified
// names (CS0234 otherwise), so Android.Net.Uri is aliased via global::.
using AndroidUri = global::Android.Net.Uri;

namespace ShaPrint.Android;

/// <summary>
/// Single Android entry point (Task 9). Splash theme + launcher attributes come from
/// Resources/values/styles.xml and the merged manifest.
/// </summary>
[Activity(
    Label = "ShaPrint",
    Theme = "@style/MyTheme.Splash",
    MainLauncher = true,
    LaunchMode = LaunchMode.SingleTop,
    ConfigurationChanges = ConfigChanges.Orientation | ConfigChanges.ScreenSize |
                           ConfigChanges.ScreenLayout | ConfigChanges.Keyboard |
                           ConfigChanges.KeyboardHidden)]
public class MainActivity : AvaloniaMainActivity<App>
{
    /// <summary>Current activity instance; the SAF file picker (PrintRelayService) needs an
    /// Activity to call <c>StartActivityForResult</c>. Re-assigned on every onCreate.</summary>
    internal static MainActivity? Current { get; private set; }

    private const int FilePickerRequestCode = 0x5A50; // "ZP" — SAF document picker
    private TaskCompletionSource<AndroidUri?>? _filePickerTcs;

    /// <summary>
    /// The app's Application class is ShaPrint.Android.App (choice (b), see App.axaml.cs).
    /// AvaloniaMainActivity's base implementation already does
    /// <c>AppBuilder.Configure&lt;App&gt;().UseAndroid()</c>; we keep it explicit so the
    /// "where does App come from" decision is visible at the entry point.
    /// </summary>
    protected override AppBuilder CreateAppBuilder() =>
        AppBuilder.Configure<App>()
            .UseAndroid();

    /// <summary>
    /// ANTI-RACE pattern (Task 9): the Android platform services must be registered with the
    /// SINGLE DI host, and that host is built in <see cref="App.OnFrameworkInitializationCompleted"/>.
    /// The Avalonia.Android runtime calls this method (via <c>InitializeAvaloniaView</c>) BEFORE
    /// it fires <c>OnFrameworkInitializationCompleted</c> — verified against the 11.2.3 source:
    /// <c>CreateAppBuilder() → CustomizeAppBuilder() → SetupWithLifetime() → OnFrameworkInitializationCompleted</c>.
    /// Setting the static <see cref="App.PlatformServiceRegistration"/> here therefore guarantees
    /// the callback is in place before App builds its host: one host, no race, no double-build.
    /// </summary>
    protected override AppBuilder CustomizeAppBuilder(AppBuilder builder)
    {
        App.PlatformServiceRegistration = static services => services.AddAndroidServices();
        return base.CustomizeAppBuilder(builder)
            .WithInterFont();
    }

    protected override void OnCreate(Bundle? savedInstanceState)
    {
        Current = this;
        base.OnCreate(savedInstanceState);
    }

    /// <summary>
    /// Opens the Storage Access Framework picker (ACTION_OPEN_DOCUMENT). Completes with the
    /// picked <see cref="AndroidUri"/>, or null when the user cancels.
    /// </summary>
    internal Task<AndroidUri?> PickDocumentAsync()
    {
        if (_filePickerTcs != null)
        {
            throw new InvalidOperationException("A file picker request is already in flight.");
        }

        var tcs = new TaskCompletionSource<AndroidUri?>();
        _filePickerTcs = tcs;

        var intent = new Intent(Intent.ActionOpenDocument);
        intent.AddCategory(Intent.CategoryOpenable);
        intent.SetType("*/*");
        StartActivityForResult(intent, FilePickerRequestCode);

        return tcs.Task;
    }

    protected override void OnActivityResult(int requestCode, Result resultCode, Intent? data)
    {
        if (requestCode == FilePickerRequestCode)
        {
            var tcs = _filePickerTcs;
            _filePickerTcs = null;
            tcs?.TrySetResult(resultCode == Result.Ok ? data?.Data : null);
            return;
        }

        base.OnActivityResult(requestCode, resultCode, data);
    }
}
