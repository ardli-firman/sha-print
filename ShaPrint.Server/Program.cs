using System.Threading;

namespace ShaPrint.Server;

static class Program
{
    private static Mutex? mutex = null;

    /// <summary>
    ///  The main entry point for the application.
    /// </summary>
    [STAThread]
    static void Main()
    {
        const string appName = "ShaPrint.Server.InstanceMutex";
        bool createdNew;

        mutex = new Mutex(true, appName, out createdNew);

        if (!createdNew)
        {
            // app is already running! Exiting the application  
            MessageBox.Show("ShaPrint Server is already running in the background.", "ShaPrint", MessageBoxButtons.OK, MessageBoxIcon.Information);
            return;
        }

        ApplicationConfiguration.Initialize();
        Application.Run(new MainForm());
        
        GC.KeepAlive(mutex);
    }
}