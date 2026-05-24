using System;
using System.IO;
using System.Text.Json;
using ShaPrint.Core;

namespace ShaPrint.App
{
    public class AppSettingsData
    {
        public bool AutoUpdateEnabled { get; set; } = true;
        public DateTime LastUpdateCheck { get; set; } = DateTime.MinValue;
    }

    public static class AppSettings
    {
        private static string _settingsFile;
        private static AppSettingsData _current;

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
                    string raw = File.ReadAllText(_settingsFile);
                    _current = JsonSerializer.Deserialize<AppSettingsData>(raw) ?? new AppSettingsData();
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
}
