using System;
using System.Windows.Forms;

namespace ShaPrint.App
{
    static class Program
    {
        [STAThread]
        static void Main()
        {
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);
            
            var launcher = new LauncherForm();
            launcher.CheckSavedModeAndLaunch();
            
            // If the launcher form was closed without choosing a mode, application will exit.
            // Application.Run() without a main form will block indefinitely unless we call Application.Exit().
            if (Application.OpenForms.Count > 0)
            {
                Application.Run();
            }
        }
    }
}