using System;
using System.Windows.Forms;

namespace ShaPrint.Updater
{
    internal static class Program
    {
        /// <summary>
        ///  The main entry point for the application.
        /// </summary>
        [STAThread]
        static void Main(string[] args)
        {
            ApplicationConfiguration.Initialize();

            string downloadUrl = string.Empty;
            if (args.Length > 0 && args[0] == "--url" && args.Length > 1)
            {
                downloadUrl = args[1];
            }

            if (string.IsNullOrEmpty(downloadUrl))
            {
                MessageBox.Show("Updater cannot run independently. Please launch from ShaPrint App.", "Updater Error", MessageBoxButtons.OK, MessageBoxIcon.Error);
                return;
            }

            Application.Run(new MainForm(downloadUrl));
        }
    }
}