using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Microsoft.Extensions.DependencyInjection;
using ShaPrint.Core;
using ShaPrint.Platform.Abstractions;
using ShaPrint.UI.Models;
using System;
using System.IO;
using System.Text.Json;

namespace ShaPrint.UI.ViewModels.Pages;

/// <summary>
/// Mode-selection landing page. Migrated from
/// <c>ShaPrint.WpfApp/ViewModels/Pages/WelcomeViewModel.cs</c> (Task 5).
///
/// WPF -> Avalonia adaptations:
/// <list type="bullet">
/// <item>WPF-UI <c>INavigationService</c> -> <see cref="MainWindowViewModel"/> navigation methods
/// (page ViewModels are resolved lazily from DI, mirroring WpfApp's <c>App.GetService&lt;T&gt;()</c>).</item>
/// <item><c>System.Printing.LocalPrintServer</c> printer detection -> <see cref="IPrinterManager"/>.
/// The abstraction does not expose the spooler's idle/queue status, so the suggestion counts all
/// local printers (see <see cref="DetectAndSuggestMode"/>).</item>
/// <item><c>ShaPrint.WpfApp.Models.AppSettings</c> -> <see cref="AppSettings"/>, with the channel
/// kept plaintext (see Models/AppSettings.cs).</item>
/// </list>
/// </summary>
public partial class WelcomeViewModel : ObservableObject
{
    private readonly MainWindowViewModel _mainWindowViewModel;
    private readonly IServiceProvider _serviceProvider;
    private readonly string _modeFile;

    [ObservableProperty]
    private string _channelName;

    // Loading states
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsServerProcessing))]
    [NotifyPropertyChangedFor(nameof(IsClientProcessing))]
    [NotifyPropertyChangedFor(nameof(IsMonitorProcessing))]
    private bool _isProcessing;

    [ObservableProperty]
    private string _processingMessage = "";

    [ObservableProperty]
    private bool _showSuccess;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsServerProcessing))]
    [NotifyPropertyChangedFor(nameof(IsClientProcessing))]
    [NotifyPropertyChangedFor(nameof(IsMonitorProcessing))]
    [NotifyPropertyChangedFor(nameof(IsServerError))]
    [NotifyPropertyChangedFor(nameof(IsClientError))]
    [NotifyPropertyChangedFor(nameof(IsMonitorError))]
    private string? _selectedMode;

    // Error handling & Validation
    [ObservableProperty]
    private string _errorMessage = "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsServerError))]
    [NotifyPropertyChangedFor(nameof(IsClientError))]
    [NotifyPropertyChangedFor(nameof(IsMonitorError))]
    private bool _showError;

    [ObservableProperty]
    private string _validationError = "";

    [ObservableProperty]
    private bool _hasValidationError;

    // Info feedback
    [ObservableProperty]
    private string _validationInfo = "";

    [ObservableProperty]
    private bool _hasValidationInfo;

    // Smart hints
    [ObservableProperty]
    private bool _isServerSuggested;

    [ObservableProperty]
    private bool _isClientSuggested;

    [ObservableProperty]
    private string _serverHintText = "";

    [ObservableProperty]
    private string _clientHintText = "";

    [ObservableProperty]
    private int _detectedPrinterCount;

    // Help system (modal popup; view toggles visibility in Task 6)
    [ObservableProperty]
    private bool _isHelpModalOpen;

    [ObservableProperty]
    private string _helpModalTitle = "";

    [ObservableProperty]
    private string _helpModalBody = "";

    // Dependent properties for UI binding
    public bool IsServerProcessing => IsProcessing && SelectedMode == "Server";
    public bool IsClientProcessing => IsProcessing && SelectedMode == "Client";
    public bool IsMonitorProcessing => IsProcessing && SelectedMode == "Monitor";

    public bool IsServerError => ShowError && SelectedMode == "Server";
    public bool IsClientError => ShowError && SelectedMode == "Client";
    public bool IsMonitorError => ShowError && SelectedMode == "Monitor";

    public WelcomeViewModel(MainWindowViewModel mainWindowViewModel, IServiceProvider serviceProvider)
    {
        _mainWindowViewModel = mainWindowViewModel;
        _serviceProvider = serviceProvider;

        string dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ShaPrint");
        if (!Directory.Exists(dir)) Directory.CreateDirectory(dir);
        _modeFile = Path.Combine(dir, "AppMode.json");

        _channelName = AppSettings.Current.EffectiveNetworkChannel;

        // Ensure sidebar is hidden on welcome page
        _mainWindowViewModel.ShowWelcome();

        // Trigger real-time validation for initial channel name
        ValidateChannel();

        // Detect printer hardware for mode suggestions (non-blocking)
        System.Threading.Tasks.Task.Run(() => DetectAndSuggestMode());
    }

    partial void OnChannelNameChanged(string value)
    {
        ValidateChannel();
    }

    /// <summary>
    /// Reads the persisted mode (AppMode.json) and navigates straight to the saved page.
    /// Called once at app startup (App.axaml.cs) — replaces the WPF WelcomePage code-behind.
    /// </summary>
    public void CheckSavedModeAndNavigate()
    {
        if (File.Exists(_modeFile))
        {
            try
            {
                string json = File.ReadAllText(_modeFile);
                string? mode = JsonSerializer.Deserialize<string>(json);

                if (mode == "Server")
                {
                    NavigateToMode("Server");
                    return;
                }
                else if (mode == "Client")
                {
                    NavigateToMode("Client");
                    return;
                }
                else if (mode == "Monitor")
                {
                    NavigateToMode("Monitor");
                    return;
                }
            }
            catch { }
        }
    }

    /// <summary>
    /// Resolves the target page ViewModel from DI and asks the shell to show it.
    /// Mirror of the WPF call site <c>App.GetService&lt;ServerViewModel&gt;()</c> etc. — page VMs
    /// are singletons, so this both constructs-and-activates the node engines (auto-start).
    /// </summary>
    private void NavigateToMode(string mode)
    {
        switch (mode)
        {
            case "Server":
                _mainWindowViewModel.NavigateToServer(_serviceProvider.GetRequiredService<ServerViewModel>());
                break;
            case "Client":
                _mainWindowViewModel.NavigateToClient(_serviceProvider.GetRequiredService<ClientViewModel>());
                break;
            case "Monitor":
                var monitor = _serviceProvider.GetRequiredService<MonitorViewModel>();
                monitor.Start(); // Start the polling service — same as WpfApp App.OnStartup for Monitor mode.
                _mainWindowViewModel.NavigateToMonitor(monitor);
                break;
        }
    }

    private bool ValidateChannel()
    {
        ValidationInfo = "";
        HasValidationInfo = false;

        if (string.IsNullOrWhiteSpace(ChannelName))
        {
            ValidationError = "Channel name is required";
            HasValidationError = true;
            return false;
        }

        string cleaned = ChannelName.Replace(" ", "");

        if (ChannelName != cleaned)
        {
            ValidationInfo = $"Spaces removed: '{ChannelName}' → '{cleaned}'";
            HasValidationInfo = true;
        }

        if (cleaned.Length < 3)
        {
            ValidationError = "Channel name must be at least 3 characters";
            HasValidationError = true;
            return false;
        }

        if (cleaned.Length > 50)
        {
            ValidationError = "Channel name is too long (max 50 characters)";
            HasValidationError = true;
            return false;
        }

        if (!System.Text.RegularExpressions.Regex.IsMatch(cleaned, @"^[a-zA-Z0-9\-_]+$"))
        {
            ValidationError = "Format: Alphanumeric, dash, underscore only";
            HasValidationError = true;
            return false;
        }

        HasValidationError = false;
        ValidationError = "";

        if (!HasValidationInfo)
        {
            ValidationInfo = "✓ Valid channel name";
            HasValidationInfo = true;
        }

        return true;
    }

    /// <summary>
    /// Detects local printer presence for mode suggestions. The WPF implementation used
    /// <c>System.Printing.LocalPrintServer</c> and filtered idle queues; <see cref="IPrinterManager"/>
    /// exposes the printer list but not queue status, so "idle" is approximated by the full
    /// local printer count. Gracefully does nothing when no platform printer backend exists yet.
    /// </summary>
    public void DetectAndSuggestMode()
    {
        IPrinterManager? printerManager = ViewModelSupport.Resolve<IPrinterManager>(_serviceProvider);
        if (printerManager == null)
        {
            // No printer backend on this platform (macOS/Linux pre-Task 7) — no suggestions.
            PostToUiThread(() =>
            {
                IsServerSuggested = false;
                IsClientSuggested = false;
                ServerHintText = "";
                ClientHintText = "";
                DetectedPrinterCount = 0;
            });
            return;
        }

        try
        {
            var printers = printerManager.GetLocalPrintersAsync().GetAwaiter().GetResult();
            int printerCount = printers.Count;

            PostToUiThread(() =>
            {
                DetectedPrinterCount = printerCount;

                if (printerCount > 0)
                {
                    IsServerSuggested = true;
                    IsClientSuggested = false;
                    ServerHintText = $"✓ {printerCount} printer(s) detected on this PC";
                    ClientHintText = "";
                }
                else
                {
                    IsServerSuggested = false;
                    IsClientSuggested = true;
                    ServerHintText = "";
                    ClientHintText = "No local printers - connect to network printers";
                }
            });
        }
        catch
        {
            // Detection failed silently — no suggestions shown
            PostToUiThread(() =>
            {
                IsServerSuggested = false;
                IsClientSuggested = false;
                ServerHintText = "";
                ClientHintText = "";
                DetectedPrinterCount = 0;
            });
        }
    }

    private static void PostToUiThread(Action action)
    {
        if (Avalonia.Application.Current is not null)
        {
            Dispatcher.UIThread.Post(action);
        }
        else
        {
            action(); // headless / tests
        }
    }

    [RelayCommand]
    public void ShowHelpModal(string mode)
    {
        if (mode == "Server")
        {
            HelpModalTitle = "Server Mode Guide";
            HelpModalBody = "Use Server Mode if this PC is directly connected to a printer via USB or Wi-Fi.\n\n" +
                            "• This PC acts as a host and must remain powered on to share printers with other computers in the channel.\n" +
                            "• Requirements: Active printer drivers installed on this PC.";
        }
        else if (mode == "Client")
        {
            HelpModalTitle = "Client Mode Guide";
            HelpModalBody = "Use Client Mode if you want to print documents wirelessly through other host PCs in the channel.\n\n" +
                            "• This computer will automatically discover and access printers shared by Server computers.\n" +
                            "• Requirements: At least one active Server PC running on the same network channel.";
        }
        else if (mode == "Monitor")
        {
            HelpModalTitle = "Monitor Mode Guide";
            HelpModalBody = "Use Monitor Mode if you are an administrator who needs to audit, manage, or troubleshoot print queues.\n\n" +
                            "• Displays active spool lists, logs, and ink/paper warnings for all devices in the channel.\n" +
                            "• Requirements: Network connection to the channel.";
        }
        IsHelpModalOpen = true;
    }

    [RelayCommand]
    public void CloseHelpModal()
    {
        IsHelpModalOpen = false;
    }

    [RelayCommand]
    private async System.Threading.Tasks.Task SelectServerModeAsync()
    {
        SelectedMode = "Server";
        IsProcessing = true;
        ShowError = false;
        ProcessingMessage = "Saving configuration...";

        try
        {
            if (!ValidateChannel())
            {
                IsProcessing = false;
                return;
            }

            SaveChannel();
            SaveMode("Server");

            ProcessingMessage = "✓ Ready! Opening Server...";
            ShowSuccess = true;
            await System.Threading.Tasks.Task.Delay(300); // UX success pulse (same as WPF)

            NavigateToMode("Server");
        }
        catch (Exception ex)
        {
            ErrorMessage = "Failed to open Server Mode. Please try again.";
            ShowError = true;
            System.Diagnostics.Debug.WriteLine($"Navigation error: {ex.Message}");
            await System.Threading.Tasks.Task.Delay(3000);
            ShowError = false;
        }
        finally
        {
            IsProcessing = false;
            ShowSuccess = false;
            SelectedMode = null;
        }
    }

    [RelayCommand]
    private async System.Threading.Tasks.Task SelectClientModeAsync()
    {
        SelectedMode = "Client";
        IsProcessing = true;
        ShowError = false;
        ProcessingMessage = "Saving configuration...";

        try
        {
            if (!ValidateChannel())
            {
                IsProcessing = false;
                return;
            }

            SaveChannel();
            SaveMode("Client");

            ProcessingMessage = "✓ Ready! Opening Client...";
            ShowSuccess = true;
            await System.Threading.Tasks.Task.Delay(300); // UX success pulse (same as WPF)

            NavigateToMode("Client");
        }
        catch (Exception ex)
        {
            ErrorMessage = "Failed to open Client Mode. Please try again.";
            ShowError = true;
            System.Diagnostics.Debug.WriteLine($"Navigation error: {ex.Message}");
            await System.Threading.Tasks.Task.Delay(3000);
            ShowError = false;
        }
        finally
        {
            IsProcessing = false;
            ShowSuccess = false;
            SelectedMode = null;
        }
    }

    [RelayCommand]
    private async System.Threading.Tasks.Task SelectMonitorModeAsync()
    {
        SelectedMode = "Monitor";
        IsProcessing = true;
        ShowError = false;
        ProcessingMessage = "Saving configuration...";

        try
        {
            if (!ValidateChannel())
            {
                IsProcessing = false;
                return;
            }

            SaveChannel();
            SaveMode("Monitor");

            ProcessingMessage = "✓ Ready! Opening Monitor...";
            ShowSuccess = true;
            await System.Threading.Tasks.Task.Delay(300); // UX success pulse (same as WPF)

            NavigateToMode("Monitor");
        }
        catch (Exception ex)
        {
            ErrorMessage = "Failed to open Monitor Mode. Please try again.";
            ShowError = true;
            System.Diagnostics.Debug.WriteLine($"Navigation error: {ex.Message}");
            await System.Threading.Tasks.Task.Delay(3000);
            ShowError = false;
        }
        finally
        {
            IsProcessing = false;
            ShowSuccess = false;
            SelectedMode = null;
        }
    }

    private void SaveMode(string mode)
    {
        try
        {
            File.WriteAllText(_modeFile, JsonSerializer.Serialize(mode));
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"Failed to save mode: {ex.Message}");
        }
    }

    private void SaveChannel()
    {
        if (string.IsNullOrWhiteSpace(ChannelName))
            ChannelName = "DefaultChannel";
        else if (ChannelName.Contains(" "))
            ChannelName = ChannelName.Replace(" ", "");

        try
        {
            AppSettings.Current.NetworkChannel = ChannelName;
            AppSettings.Save();
            ShaPrint.Core.Constants.SetNetworkChannel(ChannelName);
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"Failed to save channel: {ex.Message}");
            ShaPrint.Core.Constants.SetNetworkChannel(ChannelName);
        }
    }
}