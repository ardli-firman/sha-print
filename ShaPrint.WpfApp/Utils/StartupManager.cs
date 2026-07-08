using System;
using System.Diagnostics;
using System.IO;
using ShaPrint.Core;

namespace ShaPrint.WpfApp.Utils
{
    public static class StartupManager
    {
        private const string TaskName = "ShaPrint_Startup";

        public static void SetStartup(bool enable)
        {
            try
            {
                string exePath = Environment.ProcessPath ?? string.Empty;
                if (string.IsNullOrEmpty(exePath)) return;

                if (enable)
                {
                    string xmlContent = GenerateXml(exePath);

                    string tempXmlPath = Path.Combine(Path.GetTempPath(), "ShaPrint_Startup.xml");
                    File.WriteAllText(tempXmlPath, xmlContent, System.Text.Encoding.Unicode);

                    try
                    {
                        var psi = new ProcessStartInfo
                        {
                            FileName = "schtasks.exe",
                            Arguments = $"/create /tn \"{TaskName}\" /xml \"{tempXmlPath}\" /f",
                            CreateNoWindow = true,
                            UseShellExecute = false
                        };
                        using var p = Process.Start(psi);
                        p?.WaitForExit();

                        if (p != null && p.ExitCode == 0)
                        {
                            AppLogger.Log("[SYSTEM] Enabled Run on Windows Startup via Task Scheduler (Highest Privileges, Battery Support, No Timeout).");
                        }
                        else
                        {
                            int exitCode = p?.ExitCode ?? -1;
                            AppLogger.Error($"[SYSTEM] Failed to enable startup. schtasks.exe exit code: {exitCode}");
                        }
                    }
                    finally
                    {
                        if (File.Exists(tempXmlPath))
                        {
                            File.Delete(tempXmlPath);
                        }
                    }
                }
                else
                {
                    var psi = new ProcessStartInfo
                    {
                        FileName = "schtasks.exe",
                        Arguments = $"/delete /tn \"{TaskName}\" /f",
                        CreateNoWindow = true,
                        UseShellExecute = false
                    };
                    using var p = Process.Start(psi);
                    p?.WaitForExit();
                    AppLogger.Log("[SYSTEM] Disabled Run on Windows Startup via Task Scheduler.");
                }
            }
            catch (Exception ex)
            {
                AppLogger.Error("[SYSTEM] Failed to change startup settings: " + ex.Message);
            }
        }

        internal static string GenerateXml(string exePath)
        {
            string exeDir = Path.GetDirectoryName(exePath) ?? string.Empty;
            string escapedExe = System.Security.SecurityElement.Escape(exePath);
            string escapedDir = System.Security.SecurityElement.Escape(exeDir);

            return $@"<?xml version=""1.0"" encoding=""UTF-16""?>
<Task version=""1.2"" xmlns=""http://schemas.microsoft.com/windows/2004/02/mit/task"">
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id=""Author"">
      <GroupId>S-1-5-32-545</GroupId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context=""Author"">
    <Exec>
      <Command>&quot;{escapedExe}&quot;</Command>
      <Arguments>--startup</Arguments>
      <WorkingDirectory>{escapedDir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>";
        }

        public static bool IsStartupEnabled()
        {
            try
            {
                var psi = new ProcessStartInfo
                {
                    FileName = "schtasks.exe",
                    Arguments = $"/query /tn \"{TaskName}\"",
                    CreateNoWindow = true,
                    UseShellExecute = false,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true
                };
                using var p = Process.Start(psi);
                if (p == null) return false;
                p.WaitForExit();
                return p.ExitCode == 0;
            }
            catch
            {
                return false;
            }
        }
    }
}
