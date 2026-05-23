using System.Threading;
using System.Windows.Forms;

namespace ShaPrint.Client;

static class Program
{
    private static Mutex? mutex = null;

    /// <summary>
    ///  The main entry point for the application.
    /// </summary>
    [STAThread]
    static void Main()
    {
        const string appName = "ShaPrint.Client.InstanceMutex";
        bool createdNew;

        mutex = new Mutex(true, appName, out createdNew);

        if (!createdNew)
        {
            MessageBox.Show("ShaPrint Client is already running in the background.", "ShaPrint", MessageBoxButtons.OK, MessageBoxIcon.Information);
            return;
        }

        ApplicationConfiguration.Initialize();
        Application.Run(new ClientForm());
        
        GC.KeepAlive(mutex);
    }
}