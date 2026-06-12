using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Client;
using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Controls;
using Wpf.Ui;
using Wpf.Ui.Controls;

namespace ShaPrint.WpfApp.ViewModels.Pages
{
    public partial class ScannerDisplayItem : ObservableObject
    {
        public DiscoveryResponseMessage Server { get; }
        public ScannerInfo Scanner { get; }

        public string DisplayName => $"[{Server.ServerName}] {Scanner.Name}";

        public ScannerDisplayItem(DiscoveryResponseMessage server, ScannerInfo scanner)
        {
            Server = server;
            Scanner = scanner;
        }
    }

    public partial class ScanViewModel : ObservableObject, IDisposable
    {
        private readonly DiscoveryClient _discoveryClient;
        private readonly ScanClientService _scanClientService;
        private readonly ISnackbarService _snackbarService;

        [ObservableProperty]
        private string _targetIp = string.Empty;

        [ObservableProperty]
        private bool _isScanning; // discovery status

        [ObservableProperty]
        [NotifyPropertyChangedFor(nameof(EmptyStateVisibility))]
        [NotifyPropertyChangedFor(nameof(LoadingStateVisibility))]
        private bool _isPerformingScan; // execution status

        [ObservableProperty]
        private string _statusText = "Ready";

        [ObservableProperty]
        private ScannerDisplayItem? _selectedScanner;

        [ObservableProperty]
        private int _selectedDpi = 300;

        [ObservableProperty]
        private int _selectedColorMode = 2; // 2 = Color, 1 = Grayscale, 0 = B&W

        [ObservableProperty]
        private string _selectedFormat = "JPEG";

        [ObservableProperty]
        [NotifyPropertyChangedFor(nameof(ZoomPercentText))]
        [NotifyPropertyChangedFor(nameof(CurrentScale))]
        [NotifyPropertyChangedFor(nameof(ScrollBarVisibilitySetting))]
        private double _zoomLevel = 0.25;

        [ObservableProperty]
        [NotifyPropertyChangedFor(nameof(ZoomPercentText))]
        [NotifyPropertyChangedFor(nameof(CurrentScale))]
        [NotifyPropertyChangedFor(nameof(ScrollBarVisibilitySetting))]
        private bool _isFitMode = true;

        [ObservableProperty]
        [NotifyPropertyChangedFor(nameof(CurrentScale))]
        private double _rotationAngle = 0;

        public string ZoomPercentText => IsFitMode ? "Fit" : $"{Math.Round(ZoomLevel * 100)}%";

        public double CurrentScale => IsFitMode ? 1.0 : ZoomLevel;

        public ScrollBarVisibility ScrollBarVisibilitySetting => 
            IsFitMode ? ScrollBarVisibility.Disabled : ScrollBarVisibility.Auto;

        [ObservableProperty]
        [NotifyPropertyChangedFor(nameof(ImagePreviewVisibility))]
        [NotifyPropertyChangedFor(nameof(PdfPlaceholderVisibility))]
        private BitmapImage? _previewImage;

        [ObservableProperty]
        [NotifyPropertyChangedFor(nameof(EmptyStateVisibility))]
        private bool _hasScannedFile;

        private byte[] _scannedFileBytes = Array.Empty<byte>();
        private string _scannedFileExtension = "jpg";

        public ObservableCollection<ScannerDisplayItem> DiscoveredScanners { get; } = new();
        public List<int> DpiOptions { get; } = new() { 150, 300, 600 };
        public List<string> FormatOptions { get; } = new() { "JPEG", "PNG", "PDF" };

        public ObservableCollection<string> Logs { get; } = new();
        public string LogsText => string.Join(Environment.NewLine, Logs);

        public Visibility ImagePreviewVisibility => (PreviewImage != null && SelectedFormat != "PDF") ? Visibility.Visible : Visibility.Collapsed;
        public Visibility PdfPlaceholderVisibility => (HasScannedFile && SelectedFormat == "PDF") ? Visibility.Visible : Visibility.Collapsed;
        public Visibility EmptyStateVisibility => (!HasScannedFile && !IsPerformingScan) ? Visibility.Visible : Visibility.Collapsed;
        public Visibility LoadingStateVisibility => IsPerformingScan ? Visibility.Visible : Visibility.Collapsed;

        public ScanViewModel(ISnackbarService snackbarService)
        {
            _snackbarService = snackbarService;
            _discoveryClient = new DiscoveryClient();
            _scanClientService = new ScanClientService();

            AppLogger.OnLog += AppLogger_OnLog;
        }

        private void AppLogger_OnLog(string msg)
        {
            if (msg.Contains("[SERVER]", StringComparison.OrdinalIgnoreCase)) return;

            Application.Current.Dispatcher.InvokeAsync(() =>
            {
                Logs.Insert(0, $"[{DateTime.Now:HH:mm:ss}] {msg}");
                if (Logs.Count > 100)
                {
                    Logs.RemoveAt(Logs.Count - 1);
                }
                OnPropertyChanged(nameof(LogsText));
            });
        }

        [RelayCommand]
        private async Task ScanLanAsync()
        {
            string? targetIp = null;
            if (!string.IsNullOrWhiteSpace(TargetIp))
            {
                if (!System.Net.IPAddress.TryParse(TargetIp.Trim(), out _))
                {
                    System.Windows.MessageBox.Show("Invalid IP Address format!", "Error", System.Windows.MessageBoxButton.OK, System.Windows.MessageBoxImage.Error);
                    return;
                }
                targetIp = TargetIp.Trim();
            }

            IsScanning = true;
            StatusText = "Scanning LAN for scanner servers...";
            DiscoveredScanners.Clear();

            try
            {
                var discoveredServers = await _discoveryClient.DiscoverServersAsync(targetIp);
                foreach (var server in discoveredServers)
                {
                    if (server.ExposedScanners != null)
                    {
                        foreach (var scanner in server.ExposedScanners)
                        {
                            DiscoveredScanners.Add(new ScannerDisplayItem(server, scanner));
                        }
                    }
                }
                StatusText = $"Discovery complete. Found {DiscoveredScanners.Count} remote scanner(s).";
            }
            catch (Exception ex)
            {
                StatusText = "Discovery failed.";
                AppLogger.Error("[CLIENT] Failed to discover scanner servers", ex);
            }
            finally
            {
                IsScanning = false;
            }
        }

        [RelayCommand]
        private async Task PerformScanAsync()
        {
            if (SelectedScanner == null)
            {
                System.Windows.MessageBox.Show("Please select a scanner first.", "Warning", System.Windows.MessageBoxButton.OK, System.Windows.MessageBoxImage.Warning);
                return;
            }

            IsPerformingScan = true;
            StatusText = "Initializing scanner...";
            PreviewImage = null;
            HasScannedFile = false;
            _scannedFileBytes = Array.Empty<byte>();
            RotationAngle = 0; // Reset rotation on new scan

            var scanner = SelectedScanner;
            string serverIp = scanner.Server.IpAddress;
            string name = scanner.Scanner.Name;

            AppLogger.Log($"[CLIENT] Initiating scan job for '{name}' at {serverIp}...");

            try
            {
                var response = await _scanClientService.RequestScanAsync(
                    serverIp, 
                    name, 
                    SelectedDpi, 
                    SelectedColorMode, 
                    SelectedFormat
                );

                if (response.Success && response.FileBytes != null && response.FileBytes.Length > 0)
                {
                    _scannedFileBytes = response.FileBytes;
                    _scannedFileExtension = SelectedFormat.ToLower();
                    HasScannedFile = true;
                    StatusText = $"Scan successful! Received {response.FileBytes.Length} bytes.";

                    if (SelectedFormat != "PDF")
                    {
                        try
                        {
                            // Create preview from bytes
                            var image = new BitmapImage();
                            using (var ms = new MemoryStream(_scannedFileBytes))
                            {
                                image.BeginInit();
                                image.CacheOption = BitmapCacheOption.OnLoad;
                                image.StreamSource = ms;
                                image.EndInit();
                            }
                            image.Freeze();
                            PreviewImage = image;
                            IsFitMode = true; // Auto-fit new scan
                        }
                        catch (Exception ex)
                        {
                            AppLogger.Error("[CLIENT] Failed to render scanned image preview", ex);
                        }
                    }

                    _snackbarService.Show(
                        "Scan Complete", 
                        "Successfully received remote scanned document.", 
                        ControlAppearance.Success, 
                        new SymbolIcon(SymbolRegular.DocumentCheckmark24), 
                        TimeSpan.FromSeconds(3)
                    );
                }
                else
                {
                    StatusText = "Scan failed.";
                    System.Windows.MessageBox.Show(
                        $"Scan failed. Details: {response.ErrorMessage}", 
                        "Scan Error", 
                        System.Windows.MessageBoxButton.OK, 
                        System.Windows.MessageBoxImage.Error
                    );
                }
            }
            catch (Exception ex)
            {
                StatusText = "Scan error.";
                System.Windows.MessageBox.Show(
                    $"Error executing scan. Details: {ex.Message}", 
                    "Error", 
                    System.Windows.MessageBoxButton.OK, 
                    System.Windows.MessageBoxImage.Error
                );
            }
            finally
            {
                IsPerformingScan = false;
            }
        }

        [RelayCommand]
        private void SaveAs()
        {
            if (!HasScannedFile || _scannedFileBytes.Length == 0) return;

            var saveFileDialog = new Microsoft.Win32.SaveFileDialog
            {
                Filter = SelectedFormat switch
                {
                    "PNG" => "PNG Image (*.png)|*.png",
                    "PDF" => "PDF Document (*.pdf)|*.pdf",
                    _ => "JPEG Image (*.jpg;*.jpeg)|*.jpg;*.jpeg"
                },
                FileName = $"Scan_{DateTime.Now:yyyyMMdd_HHmmss}.{_scannedFileExtension}"
            };

            if (saveFileDialog.ShowDialog() == true)
            {
                try
                {
                    byte[] bytesToSave = _scannedFileBytes;

                    if (SelectedFormat != "PDF" && RotationAngle != 0)
                    {
                        AppLogger.Log($"[CLIENT] Rotating saved image file by {RotationAngle} degrees...");
                        bytesToSave = RotateImageBytes(_scannedFileBytes, SelectedFormat, RotationAngle);
                    }

                    File.WriteAllBytes(saveFileDialog.FileName, bytesToSave);
                    _snackbarService.Show(
                        "File Saved", 
                        "Scanned document saved successfully.", 
                        ControlAppearance.Success, 
                        new SymbolIcon(SymbolRegular.Save24), 
                        TimeSpan.FromSeconds(2)
                    );
                }
                catch (Exception ex)
                {
                    System.Windows.MessageBox.Show($"Failed to save file. Details: {ex.Message}", "Error", System.Windows.MessageBoxButton.OK, System.Windows.MessageBoxImage.Error);
                }
            }
        }

        [RelayCommand]
        private void ZoomIn(object? parameter)
        {
            var scrollViewer = parameter as ScrollViewer;
            double currentScale = ZoomLevel;

            if (IsFitMode)
            {
                currentScale = CalculateFitScale(scrollViewer);
                IsFitMode = false;
            }

            if (currentScale < 3.0)
                ZoomLevel = Math.Min(3.0, currentScale + 0.1);
        }

        [RelayCommand]
        private void ZoomOut(object? parameter)
        {
            var scrollViewer = parameter as ScrollViewer;
            double currentScale = ZoomLevel;

            if (IsFitMode)
            {
                currentScale = CalculateFitScale(scrollViewer);
                IsFitMode = false;
            }

            if (currentScale > 0.05)
                ZoomLevel = Math.Max(0.05, currentScale - 0.1);
        }

        [RelayCommand]
        private void ZoomFit()
        {
            IsFitMode = true;
        }

        [RelayCommand]
        private void Rotate()
        {
            RotationAngle = (RotationAngle + 90) % 360;
        }

        private byte[] RotateImageBytes(byte[] imageBytes, string format, double angle)
        {
            if (angle == 0 || angle == 360)
                return imageBytes;

            try
            {
                using (var ms = new MemoryStream(imageBytes))
                {
                    var decoder = BitmapDecoder.Create(ms, BitmapCreateOptions.None, BitmapCacheOption.OnLoad);
                    if (decoder.Frames.Count == 0) return imageBytes;

                    var bitmapSource = decoder.Frames[0];

                    var rotation = angle switch
                    {
                        90 => Rotation.Rotate90,
                        180 => Rotation.Rotate180,
                        270 => Rotation.Rotate270,
                        _ => Rotation.Rotate0
                    };

                    if (rotation == Rotation.Rotate0)
                        return imageBytes;

                    var rotatedBitmap = new TransformedBitmap(bitmapSource, new RotateTransform(angle));

                    using (var outMs = new MemoryStream())
                    {
                        BitmapEncoder encoder = format.ToUpper() switch
                        {
                            "PNG" => new PngBitmapEncoder(),
                            _ => new JpegBitmapEncoder()
                        };

                        encoder.Frames.Add(BitmapFrame.Create(rotatedBitmap));
                        encoder.Save(outMs);
                        return outMs.ToArray();
                    }
                }
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[CLIENT] Failed to rotate image bytes before saving: {ex.Message}");
                return imageBytes;
            }
        }

        private double CalculateFitScale(ScrollViewer? scrollViewer)
        {
            if (scrollViewer == null || PreviewImage == null)
                return 0.25; // fallback

            double viewportWidth = scrollViewer.ViewportWidth;
            double viewportHeight = scrollViewer.ViewportHeight;

            // If layout hasn't computed viewport yet, fallback to actual size
            if (viewportWidth <= 0) viewportWidth = scrollViewer.ActualWidth;
            if (viewportHeight <= 0) viewportHeight = scrollViewer.ActualHeight;

            if (viewportWidth <= 0 || viewportHeight <= 0)
                return 0.25;

            double imgWidth = PreviewImage.Width;
            double imgHeight = PreviewImage.Height;

            // Fallback to pixel dimensions if Width/Height is NaN or 0
            if (double.IsNaN(imgWidth) || imgWidth <= 0) imgWidth = PreviewImage.PixelWidth;
            if (double.IsNaN(imgHeight) || imgHeight <= 0) imgHeight = PreviewImage.PixelHeight;

            if (imgWidth <= 0 || imgHeight <= 0)
                return 0.25;

            // Swap dimensions if rotated 90 or 270 degrees
            if (Math.Abs(RotationAngle % 180) != 0)
            {
                double temp = imgWidth;
                imgWidth = imgHeight;
                imgHeight = temp;
            }

            // We subtract a small margin (e.g. 16px) for scrollbar/borders
            double fitX = (viewportWidth - 16) / imgWidth;
            double fitY = (viewportHeight - 16) / imgHeight;

            return Math.Min(fitX, fitY);
        }

        public void Dispose()
        {
            AppLogger.OnLog -= AppLogger_OnLog;
        }
    }
}
