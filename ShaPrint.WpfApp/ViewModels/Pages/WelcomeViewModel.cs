using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using System;
using System.IO;
using System.Text.Json;
using Wpf.Ui;

namespace ShaPrint.WpfApp.ViewModels.Pages
{
    public partial class WelcomeViewModel : ObservableObject
    {
        private readonly INavigationService _navigationService;
        private readonly Windows.MainWindowViewModel _mainWindowViewModel;
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

        // Help system (Modal Popup)
        [ObservableProperty]
        private bool _isHelpModalOpen;

        [ObservableProperty]
        private string _helpModalTitle = "";

        [ObservableProperty]
        private string _helpModalBody = "";

        // Dependent properties for WPF UI binding
        public bool IsServerProcessing => IsProcessing && SelectedMode == "Server";
        public bool IsClientProcessing => IsProcessing && SelectedMode == "Client";
        public bool IsMonitorProcessing => IsProcessing && SelectedMode == "Monitor";

        public bool IsServerError => ShowError && SelectedMode == "Server";
        public bool IsClientError => ShowError && SelectedMode == "Client";
        public bool IsMonitorError => ShowError && SelectedMode == "Monitor";

        public WelcomeViewModel(INavigationService navigationService, Windows.MainWindowViewModel mainWindowViewModel)
        {
            _navigationService = navigationService;
            _mainWindowViewModel = mainWindowViewModel;
            
            string dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ShaPrint");
            if (!Directory.Exists(dir)) Directory.CreateDirectory(dir);
            _modeFile = Path.Combine(dir, "AppMode.json");
            
            _channelName = Models.AppSettings.Current.NetworkChannel;

            // Ensure sidebar is hidden on welcome page
            _mainWindowViewModel.WelcomeVisibility = System.Windows.Visibility.Visible;
            _mainWindowViewModel.SidebarVisibility = System.Windows.Visibility.Collapsed;

            // Trigger real-time validation for initial channel name
            ValidateChannel();

            // Detect printer hardware for mode suggestions (non-blocking)
            System.Threading.Tasks.Task.Run(() => DetectAndSuggestMode());
        }

        partial void OnChannelNameChanged(string value)
        {
            ValidateChannel();
        }

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
                        NavigateToMode("Server", typeof(Views.Pages.ServerPage));
                        return;
                    }
                    else if (mode == "Client")
                    {
                        NavigateToMode("Client", typeof(Views.Pages.ClientPage));
                        return;
                    }
                    else if (mode == "Monitor")
                    {
                        NavigateToMode("Monitor", typeof(Views.Pages.MonitorPage));
                        return;
                    }
                }
                catch { }
            }
        }

        private void NavigateToMode(string mode, Type pageType)
        {
            _mainWindowViewModel.IsServerMode = mode == "Server" ? System.Windows.Visibility.Visible : System.Windows.Visibility.Collapsed;
            _mainWindowViewModel.IsClientMode = mode == "Client" ? System.Windows.Visibility.Visible : System.Windows.Visibility.Collapsed;
            _mainWindowViewModel.IsMonitorMode = mode == "Monitor" ? System.Windows.Visibility.Visible : System.Windows.Visibility.Collapsed;
            _mainWindowViewModel.WelcomeVisibility = System.Windows.Visibility.Collapsed;
            _mainWindowViewModel.SidebarVisibility = System.Windows.Visibility.Visible;
            
            System.Windows.Application.Current.Dispatcher.InvokeAsync(() => 
                _navigationService.Navigate(pageType));
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

        public void DetectAndSuggestMode()
        {
            try
            {
                int printerCount = 0;
                var printServer = new System.Printing.LocalPrintServer();
                var printQueues = printServer.GetPrintQueues(new[]
                {
                    System.Printing.EnumeratedPrintQueueTypes.Local,
                    System.Printing.EnumeratedPrintQueueTypes.Connections
                });

                foreach (var queue in printQueues)
                {
                    var status = queue.QueueStatus;
                    // Consider printer as idle if status has no active errors or busy/offline flags
                    bool isIdle = status == System.Printing.PrintQueueStatus.None ||
                                  status == System.Printing.PrintQueueStatus.PowerSave ||
                                  (!status.HasFlag(System.Printing.PrintQueueStatus.Offline) &&
                                   !status.HasFlag(System.Printing.PrintQueueStatus.Error) &&
                                   !status.HasFlag(System.Printing.PrintQueueStatus.Busy) &&
                                   !status.HasFlag(System.Printing.PrintQueueStatus.Paused) &&
                                   !status.HasFlag(System.Printing.PrintQueueStatus.NotAvailable));

                    if (isIdle)
                    {
                        printerCount++;
                    }
                }

                DetectedPrinterCount = printerCount;

                if (printerCount > 0)
                {
                    IsServerSuggested = true;
                    IsClientSuggested = false;
                    ServerHintText = $"✓ {printerCount} idle printer(s) detected on this PC";
                    ClientHintText = "";
                }
                else
                {
                    IsServerSuggested = false;
                    IsClientSuggested = true;
                    ServerHintText = "";
                    ClientHintText = "No local idle printers - connect to network printers";
                }
            }
            catch
            {
                // Detection failed silently — no suggestions shown
                IsServerSuggested = false;
                IsClientSuggested = false;
                ServerHintText = "";
                ClientHintText = "";
                DetectedPrinterCount = 0;
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
                await System.Threading.Tasks.Task.Delay(300);

                NavigateToMode("Server", typeof(Views.Pages.ServerPage));
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
                await System.Threading.Tasks.Task.Delay(300);

                NavigateToMode("Client", typeof(Views.Pages.ClientPage));
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
                await System.Threading.Tasks.Task.Delay(300);

                NavigateToMode("Monitor", typeof(Views.Pages.MonitorPage));
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
                Models.AppSettings.Current.NetworkChannel = ChannelName;
                Models.AppSettings.Save();
                ShaPrint.Core.Constants.SetNetworkChannel(ChannelName);
            }
            catch (Exception ex)
            {
                System.Diagnostics.Debug.WriteLine($"Failed to save channel: {ex.Message}");
                ShaPrint.Core.Constants.SetNetworkChannel(ChannelName);
            }
        }
    }
}
