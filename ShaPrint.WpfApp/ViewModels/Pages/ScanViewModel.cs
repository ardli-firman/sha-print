using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.Client;
using ShaPrint.WpfApp.Models;
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
 
    public partial class ScannedPageItem : ObservableObject
    {
        [ObservableProperty]
        private BitmapImage _previewSource;
 
        [ObservableProperty]
        private double _rotationAngle = 0; // 0, 90, 180, 270
 
        public byte[] ImageBytes { get; }
        public string Format { get; } // "jpg" or "png"
 
        public ScannedPageItem(byte[] imageBytes, string format, BitmapImage previewSource)
        {
            ImageBytes = imageBytes;
            Format = format;
            _previewSource = previewSource;
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
        [NotifyPropertyChangedFor(nameof(ImagePreviewVisibility))]
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
        private int _brightness = 0; // -100 to 100
 
        [ObservableProperty]
        private int _contrast = 0;   // -100 to 100
 
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
        [NotifyPropertyChangedFor(nameof(ImagePreviewVisibility))]
        [NotifyPropertyChangedFor(nameof(PreviewImage))]
        [NotifyPropertyChangedFor(nameof(RotationAngle))]
        private ScannedPageItem? _selectedPage;
 
        public BitmapImage? PreviewImage => SelectedPage?.PreviewSource;
 
        public double RotationAngle => SelectedPage?.RotationAngle ?? 0;
 
        partial void OnSelectedPageChanged(ScannedPageItem? value)
        {
            OnPropertyChanged(nameof(PreviewImage));
            OnPropertyChanged(nameof(RotationAngle));
            OnPropertyChanged(nameof(ZoomPercentText));
            OnPropertyChanged(nameof(CurrentScale));
            OnPropertyChanged(nameof(ScrollBarVisibilitySetting));
        }
 
        public string ZoomPercentText => IsFitMode ? "Fit" : $"{Math.Round(ZoomLevel * 100)}%";
 
        public double CurrentScale => IsFitMode ? 1.0 : ZoomLevel;
 
        public ScrollBarVisibility ScrollBarVisibilitySetting => 
            IsFitMode ? ScrollBarVisibility.Disabled : ScrollBarVisibility.Auto;
 
        [ObservableProperty]
        [NotifyPropertyChangedFor(nameof(EmptyStateVisibility))]
        private bool _hasScannedFile;
 
        public ObservableCollection<ScannerDisplayItem> DiscoveredScanners { get; } = new();
        public ObservableCollection<ScannedPageItem> ScannedPages { get; } = new();
        public List<int> DpiOptions { get; } = new() { 150, 300, 600 };
        public List<string> FormatOptions { get; } = new() { "JPEG", "PNG", "PDF" };
 
        public ObservableCollection<string> Logs { get; } = new();
        public string LogsText => string.Join(Environment.NewLine, Logs);
 
        public Visibility ImagePreviewVisibility => (PreviewImage != null && !IsPerformingScan) ? Visibility.Visible : Visibility.Collapsed;
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
                    SelectedFormat,
                    Brightness,
                    Contrast
                );
 
                if (response.Success && response.FileBytes != null && response.FileBytes.Length > 0)
                {
                    byte[] rawImageBytes = response.FileBytes;
                    string formatExt = SelectedFormat.ToLower();
 
                    if (SelectedFormat.Equals("PDF", StringComparison.OrdinalIgnoreCase))
                    {
                        rawImageBytes = ExtractJpegFromPdf(response.FileBytes);
                        formatExt = "jpg";
                    }
 
                    BitmapImage? preview = null;
                    if (rawImageBytes != null && rawImageBytes.Length > 0)
                    {
                        try
                        {
                            preview = new BitmapImage();
                            using (var ms = new MemoryStream(rawImageBytes))
                            {
                                preview.BeginInit();
                                preview.CreateOptions = BitmapCreateOptions.PreservePixelFormat;
                                preview.CacheOption = BitmapCacheOption.OnLoad;
                                preview.StreamSource = ms;
                                preview.EndInit();
                            }
                            preview.Freeze();
                        }
                        catch (Exception ex)
                        {
                            AppLogger.Error("[CLIENT] Failed to render scanned image preview", ex);
                        }
                    }
 
                    if (preview != null)
                    {
                        var newPage = new ScannedPageItem(rawImageBytes!, formatExt, preview);
                        ScannedPages.Add(newPage);
                        SelectedPage = newPage;
                        HasScannedFile = true;
                        IsFitMode = true; // Auto-fit new scan
 
                        StatusText = $"Scan successful! Received {response.FileBytes.Length} bytes.";
 
                        // Auto-Save if enabled
                        var settings = AppSettings.Current;
                        if (settings.AutoSaveScans)
                        {
                            AutoSavePage(newPage);
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
 
        private void AutoSavePage(ScannedPageItem page)
        {
            try
            {
                var settings = AppSettings.Current;
                string dir = string.IsNullOrEmpty(settings.DefaultScansFolder)
                    ? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "ShaPrint Scans")
                    : settings.DefaultScansFolder;
 
                if (!Directory.Exists(dir)) Directory.CreateDirectory(dir);
 
                string ext = page.Format;
                byte[] bytesToSave = page.ImageBytes;
 
                if (page.RotationAngle != 0)
                {
                    bytesToSave = RotateImageBytes(bytesToSave, page.Format, page.RotationAngle);
                }
 
                string fileName = $"Scan_{DateTime.Now:yyyyMMdd_HHmmss}.{ext}";
                string fullPath = Path.Combine(dir, fileName);
 
                File.WriteAllBytes(fullPath, bytesToSave);
                AppLogger.Log($"[CLIENT] Auto-saved scan to {fullPath}");
 
                _snackbarService.Show(
                    "Auto-Saved", 
                    "Scan automatically saved to default folder.", 
                    ControlAppearance.Success, 
                    new SymbolIcon(SymbolRegular.Save24), 
                    TimeSpan.FromSeconds(2.5)
                );
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[CLIENT] Auto-save failed: {ex.Message}");
            }
        }
 
        [RelayCommand]
        private void MovePageUp()
        {
            if (SelectedPage == null) return;
            int idx = ScannedPages.IndexOf(SelectedPage);
            if (idx > 0)
            {
                ScannedPages.Move(idx, idx - 1);
            }
        }
 
        [RelayCommand]
        private void MovePageDown()
        {
            if (SelectedPage == null) return;
            int idx = ScannedPages.IndexOf(SelectedPage);
            if (idx >= 0 && idx < ScannedPages.Count - 1)
            {
                ScannedPages.Move(idx, idx + 1);
            }
        }
 
        [RelayCommand]
        private void DeletePage()
        {
            if (SelectedPage == null) return;
            int idx = ScannedPages.IndexOf(SelectedPage);
            ScannedPages.RemoveAt(idx);
 
            if (ScannedPages.Count > 0)
            {
                int nextIdx = Math.Min(idx, ScannedPages.Count - 1);
                SelectedPage = ScannedPages[nextIdx];
            }
            else
            {
                SelectedPage = null;
                HasScannedFile = false;
            }
        }
 
        [RelayCommand]
        private void ClearQueue()
        {
            ScannedPages.Clear();
            SelectedPage = null;
            HasScannedFile = false;
            StatusText = "Queue cleared. Ready for new scans.";
        }
 
        [RelayCommand]
        private void SaveAs()
        {
            if (ScannedPages.Count == 0) return;
 
            var saveFileDialog = new Microsoft.Win32.SaveFileDialog
            {
                Filter = "PDF Document (*.pdf)|*.pdf|JPEG Image (*.jpg;*.jpeg)|*.jpg;*.jpeg|PNG Image (*.png)|*.png",
                FileName = $"Scan_{DateTime.Now:yyyyMMdd_HHmmss}.pdf"
            };
 
            if (saveFileDialog.ShowDialog() == true)
            {
                try
                {
                    string targetFormat = Path.GetExtension(saveFileDialog.FileName).TrimStart('.').ToUpper();
                    if (targetFormat.Equals("JPEG", StringComparison.OrdinalIgnoreCase)) targetFormat = "JPG";
 
                    byte[] bytesToSave;
 
                    if (targetFormat.Equals("PDF", StringComparison.OrdinalIgnoreCase))
                    {
                        AppLogger.Log($"[CLIENT] Compiling {ScannedPages.Count} pages into a single PDF document...");
                        var processedPages = new List<byte[]>();
                        foreach (var page in ScannedPages)
                        {
                            byte[] imgBytes = page.ImageBytes;
                            if (page.RotationAngle != 0)
                            {
                                imgBytes = RotateImageBytes(imgBytes, page.Format, page.RotationAngle);
                            }
                            if (!page.Format.Equals("jpg", StringComparison.OrdinalIgnoreCase) && 
                                !page.Format.Equals("jpeg", StringComparison.OrdinalIgnoreCase))
                            {
                                imgBytes = ConvertFormat(imgBytes, "JPEG");
                            }
                            processedPages.Add(imgBytes);
                        }
                        bytesToSave = CompileMultiPagePdf(processedPages);
                    }
                    else
                    {
                        if (SelectedPage == null)
                        {
                            System.Windows.MessageBox.Show("Please select a page to save.", "Warning", System.Windows.MessageBoxButton.OK, System.Windows.MessageBoxImage.Warning);
                            return;
                        }
 
                        byte[] imgBytes = SelectedPage.ImageBytes;
                        if (SelectedPage.RotationAngle != 0)
                        {
                            imgBytes = RotateImageBytes(imgBytes, SelectedPage.Format, SelectedPage.RotationAngle);
                        }
 
                        string pageFormat = SelectedPage.Format.ToUpper();
                        if (pageFormat.Equals("JPG")) pageFormat = "JPEG";
 
                        if (!pageFormat.Equals(targetFormat, StringComparison.OrdinalIgnoreCase))
                        {
                            AppLogger.Log($"[CLIENT] Converting page from {pageFormat} to {targetFormat}...");
                            imgBytes = ConvertFormat(imgBytes, targetFormat);
                        }
 
                        bytesToSave = imgBytes;
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
 
        private static byte[] ConvertFormat(byte[] imageBytes, string format)
        {
            try
            {
                using (var ms = new MemoryStream(imageBytes))
                {
                    var decoder = BitmapDecoder.Create(ms, BitmapCreateOptions.None, BitmapCacheOption.OnLoad);
                    if (decoder.Frames.Count == 0) return imageBytes;
 
                    var frame = decoder.Frames[0];
                    using (var outMs = new MemoryStream())
                    {
                        BitmapEncoder encoder;
                        if (format.ToUpper().Equals("PNG"))
                        {
                            encoder = new PngBitmapEncoder();
                        }
                        else
                        {
                            var jpegEncoder = new JpegBitmapEncoder();
                            jpegEncoder.QualityLevel = 100;
                            encoder = jpegEncoder;
                        }
 
                        encoder.Frames.Add(BitmapFrame.Create(frame));
                        encoder.Save(outMs);
                        return outMs.ToArray();
                    }
                }
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[CLIENT] Failed to convert image format: {ex.Message}");
                return imageBytes;
            }
        }
 
        public static byte[] CompileMultiPagePdf(List<byte[]> jpegPages)
        {
            if (jpegPages == null || jpegPages.Count == 0)
                return Array.Empty<byte>();
 
            using (var ms = new MemoryStream())
            using (var sw = new StreamWriter(ms, System.Text.Encoding.ASCII))
            {
                sw.NewLine = "\n";
                sw.Write("%PDF-1.4\n");
                sw.Flush();
 
                int pageCount = jpegPages.Count;
 
                // Catalog is obj 1
                long catalogOffset = ms.Position;
                sw.Write("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
                sw.Flush();
 
                // Pages root is obj 2
                var kidsBuilder = new System.Text.StringBuilder();
                for (int i = 0; i < pageCount; i++)
                {
                    int pageObjId = 3 + i * 3;
                    kidsBuilder.Append($"{pageObjId} 0 R ");
                }
                string kidsStr = kidsBuilder.ToString().Trim();
 
                long pagesOffset = ms.Position;
                sw.Write($"2 0 obj\n<< /Type /Pages /Kids [{kidsStr}] /Count {pageCount} >>\nendobj\n");
                sw.Flush();
 
                var offsets = new List<long>();
                offsets.Add(catalogOffset);
                offsets.Add(pagesOffset);
 
                for (int i = 0; i < pageCount; i++)
                {
                    byte[] jpegBytes = jpegPages[i];
                    int pixelWidth = 0;
                    int pixelHeight = 0;
                    int pointsWidth = 612;
                    int pointsHeight = 792;
                    string colorSpace = "DeviceRGB";
 
                    try
                    {
                        using (var imgMs = new MemoryStream(jpegBytes))
                        {
                            var decoder = BitmapDecoder.Create(
                                imgMs,
                                BitmapCreateOptions.None,
                                BitmapCacheOption.OnLoad);
                            if (decoder.Frames.Count > 0)
                            {
                                var frame = decoder.Frames[0];
                                pixelWidth = frame.PixelWidth;
                                pixelHeight = frame.PixelHeight;

                                double dpiX = frame.DpiX;
                                double dpiY = frame.DpiY;
                                if (dpiX <= 10 || dpiX > 4800) dpiX = 96.0;
                                if (dpiY <= 10 || dpiY > 4800) dpiY = 96.0;

                                pointsWidth = (int)Math.Round(frame.PixelWidth * 72.0 / dpiX);
                                pointsHeight = (int)Math.Round(frame.PixelHeight * 72.0 / dpiY);
 
                                var format = frame.Format;
                                if (format == PixelFormats.Gray8 ||
                                    format == PixelFormats.BlackWhite ||
                                    format == PixelFormats.Gray2 ||
                                    format == PixelFormats.Gray4 ||
                                    format == PixelFormats.Indexed1 ||
                                    format == PixelFormats.Indexed2 ||
                                    format == PixelFormats.Indexed4 ||
                                    format == PixelFormats.Indexed8)
                                {
                                    colorSpace = "DeviceGray";
                                }
                            }
                        }
                    }
                    catch (Exception ex)
                    {
                        AppLogger.Log($"[CLIENT] Warning: Could not parse image bounds for PDF page {i + 1}, using defaults. {ex.Message}");
                    }
 
                    if (pixelWidth <= 0) pixelWidth = pointsWidth;
                    if (pixelHeight <= 0) pixelHeight = pointsHeight;
 
                    int pageObjId = 3 + i * 3;
                    int imageObjId = 4 + i * 3;
                    int contentObjId = 5 + i * 3;
 
                    // Page Object
                    long pageOffset = ms.Position;
                    offsets.Add(pageOffset);
                    sw.Write($"{pageObjId} 0 obj\n<< /Type /Page /Parent 2 0 R /Resources << /XObject << /Im{i + 1} {imageObjId} 0 R >> >> /Contents {contentObjId} 0 R /MediaBox [0 0 {pointsWidth} {pointsHeight}] >>\nendobj\n");
                    sw.Flush();
 
                    // Image Object
                    long imageOffset = ms.Position;
                    offsets.Add(imageOffset);
                    sw.Write($"{imageObjId} 0 obj\n<< /Type /XObject /Subtype /Image /Width {pixelWidth} /Height {pixelHeight} /ColorSpace /{colorSpace} /BitsPerComponent 8 /Filter /DCTDecode /Length {jpegBytes.Length} >>\nstream\n");
                    sw.Flush();
 
                    ms.Write(jpegBytes, 0, jpegBytes.Length);
                    ms.Flush();
 
                    sw.Write("\nendstream\nendobj\n");
                    sw.Flush();
 
                    // Content Stream Object
                    long contentOffset = ms.Position;
                    offsets.Add(contentOffset);
                    string contentStream = $"q\n{pointsWidth} 0 0 {pointsHeight} 0 0 cm\n/Im{i + 1} Do\nQ\n";
                    sw.Write($"{contentObjId} 0 obj\n<< /Length {contentStream.Length} >>\nstream\n{contentStream}endstream\nendobj\n");
                    sw.Flush();
                }
 
                // Xref table
                long xrefOffset = ms.Position;
                int totalObjects = 2 + pageCount * 3;
                sw.Write($"xref\n0 {totalObjects + 1}\n");
                sw.Write("0000000000 65535 f\r\n");
                
                for (int i = 0; i < totalObjects; i++)
                {
                    sw.Write($"{offsets[i]:D10} 00000 n\r\n");
                }
                sw.Flush();
 
                sw.Write($"trailer\n<< /Size {totalObjects + 1} /Root 1 0 R >>\nstartxref\n{xrefOffset}\n%%EOF\n");
                sw.Flush();
 
                return ms.ToArray();
            }
        }
 
        private static byte[] ExtractJpegFromPdf(byte[] pdfBytes)
        {
            if (pdfBytes == null || pdfBytes.Length == 0)
                return Array.Empty<byte>();

            try
            {
                byte[] streamMarker = System.Text.Encoding.ASCII.GetBytes("stream\n");
                byte[] endMarker = System.Text.Encoding.ASCII.GetBytes("\nendstream");

                int startIdx = FindBytes(pdfBytes, streamMarker);
                if (startIdx == -1) return Array.Empty<byte>();

                startIdx += streamMarker.Length;

                int endIdx = FindBytes(pdfBytes, endMarker, startIdx);
                if (endIdx == -1) return Array.Empty<byte>();

                int length = endIdx - startIdx;
                if (length <= 0) return Array.Empty<byte>();

                byte[] jpegBytes = new byte[length];
                Buffer.BlockCopy(pdfBytes, startIdx, jpegBytes, 0, length);
                return jpegBytes;
            }
            catch (Exception ex)
            {
                AppLogger.Error($"[CLIENT] Failed to extract JPEG from PDF: {ex.Message}");
                return Array.Empty<byte>();
            }
        }

        private static int FindBytes(byte[] src, byte[] find, int startSearch = 0)
        {
            for (int i = startSearch; i <= src.Length - find.Length; i++)
            {
                bool match = true;
                for (int j = 0; j < find.Length; j++)
                {
                    if (src[i + j] != find[j])
                    {
                        match = false;
                        break;
                    }
                }
                if (match) return i;
            }
            return -1;
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
            if (SelectedPage != null)
            {
                SelectedPage.RotationAngle = (SelectedPage.RotationAngle + 90) % 360;
                OnPropertyChanged(nameof(RotationAngle));
            }
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
                        BitmapEncoder encoder;
                        if (format.ToUpper().Equals("PNG"))
                        {
                            encoder = new PngBitmapEncoder();
                        }
                        else
                        {
                            var jpegEncoder = new JpegBitmapEncoder();
                            jpegEncoder.QualityLevel = 100;
                            encoder = jpegEncoder;
                        }

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
