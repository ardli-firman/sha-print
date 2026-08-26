using System.Text.Json;
using System.Text.Json.Serialization;
using ShaPrint.Core;
using ShaPrint.UI.Services;

namespace ShaPrint.UI.Models;

/// <summary>
/// Driver-sharing toggle (opt-out, default on). Mirrors
/// <c>ShaPrint.WpfApp.Models.DriverSharingSettings</c>.
/// </summary>
public class DriverSharingSettings
{
    public bool Enabled { get; set; } = true;
}

/// <summary>
/// Persisted application settings for the cross-platform UI. Migrated from
/// <c>ShaPrint.WpfApp.Models.AppSettings</c> (Task 5).
///
/// Deviation from the WPF model: <c>NetworkChannel</c> is stored as plaintext JSON. The WPF app
/// encrypted it with DPAPI (<c>SecretProtector</c>, Windows-only); this UI keeps the field
/// platform-neutral so the same <c>AppSettings.json</c> stays usable on macOS/Linux. The two
/// apps tolerate each other's files (JSON ignores unknown properties) — at most the network
/// channel is re-entered when switching between the old and new shell.
/// </summary>
public class AppSettingsData
{
    public bool AutoUpdateEnabled { get; set; } = true;
    public bool AutoPurgeEnabled { get; set; } = true;
    public DateTime LastUpdateCheck { get; set; } = DateTime.MinValue;
    public UpdateChannel Channel { get; set; } = UpdateChannel.Stable;

    public bool AutoSaveScans { get; set; } = false;
    public string DefaultScansFolder { get; set; } = string.Empty;

    public string NetworkChannel { get; set; } = string.Empty;

    public DriverSharingSettings DriverSharing { get; set; } = new DriverSharingSettings();

    /// <summary>Channel used by discovery/MQTT-style key derivation; "DefaultChannel" when unset.</summary>
    [JsonIgnore]
    public string EffectiveNetworkChannel =>
        string.IsNullOrWhiteSpace(NetworkChannel) ? "DefaultChannel" : NetworkChannel;
}

public static class AppSettings
{
    private static string _settingsFile = null!;
    private static AppSettingsData _current = null!;

    static AppSettings()
    {
        string dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ShaPrint");
        if (!Directory.Exists(dir)) Directory.CreateDirectory(dir);
        _settingsFile = Path.Combine(dir, "AppSettings.json");
        Load();
    }

    public static AppSettingsData Current => _current;

    public static void Load()
    {
        if (File.Exists(_settingsFile))
        {
            try
            {
                _current = JsonSerializer.Deserialize<AppSettingsData>(File.ReadAllText(_settingsFile)) ?? new AppSettingsData();
                return;
            }
            catch (Exception ex)
            {
                AppLogger.Error("Failed to load AppSettings", ex);
            }
        }
        _current = new AppSettingsData();
    }

    public static void Save()
    {
        try
        {
            string json = JsonSerializer.Serialize(_current, new JsonSerializerOptions { WriteIndented = true });
            File.WriteAllText(_settingsFile, json);
        }
        catch (Exception ex)
        {
            AppLogger.Error("Failed to save AppSettings", ex);
        }
    }
}