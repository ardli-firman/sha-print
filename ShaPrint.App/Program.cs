using System;
using System.Threading;
using System.Windows.Forms;

namespace ShaPrint.App
{
    static class Program
    {
        private static Mutex? _mutex = null;

        [STAThread]
        static void Main(string[] args)
        {
            const string appName = "ShaPrint_SingleInstance_Mutex";
            bool createdNew;

            _mutex = new Mutex(true, appName, out createdNew);

            if (!createdNew)
            {
                // Another instance is already running
                MessageBox.Show("ShaPrint is already running. Check your system tray.", "ShaPrint", MessageBoxButtons.OK, MessageBoxIcon.Information);
                return;
            }

            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);
            
            bool isStartup = args.Length > 0 && args[0] == "--startup";
            
            var launcher = new LauncherForm(isStartup);
            launcher.CheckSavedModeAndLaunch();
            
            // If the launcher form was closed without choosing a mode, application will exit.
            // Application.Run() without a main form will block indefinitely unless we call Application.Exit().
            if (launcher.HasLaunchedMode)
            {
                Application.Run();
            }
        }
    }
}