using Avalonia.Media.Imaging;
using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using ShaPrint.Core;
using ShaPrint.Core.Network;
using ShaPrint.UI.Models;
using ShaPrint.UI.Services;
using System.Collections.ObjectModel;

namespace ShaPrint.UI.ViewModels.Pages;

/// <summary>One discovered remote scanner on the Scan page (same shape as WpfApp).</summary>
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

/// <summary>One scanned page. Preview is decoded once into an Avalonia <see cref="Bitmap"/>
/// (WPF <c>BitmapImage</c> replacement).</summary>
public partial class ScannedPageItem : ObservableObject
{
    public byte[] ImageBytes { get; }
    public string Format { get; } // "jpg" or "png"

    [ObservableProperty]
    private double _rotationAngle; // 0, 90, 180, 270

    /// <summary>Decoded preview, or null when the bytes could not be rendered.</summary>
    public Bitmap? Preview { get; }

    public ScannedPageItem(byte[] imageBytes, string format, Bitmap? preview)
    {
        ImageBytes = imageBytes;
        Format = format;
        _rotationAngle = 0;
        Preview = preview;
    }
}

/// <summary>
/// Scan mode page. Migrated from <c>ShaPrint.WpfApp/ViewModels/Pages/ScanViewModel.cs</c>
/// (Task 5), rewired onto the shared Task 4 services:
/// <list type="bullet">
/// <item><c>DiscoveryClient</c> -> <see cref="DiscoveryClientService"/> (real LAN scan);</item>
/// <item><c>ScanClientService</c> (WpfApp) -> the injectable <see cref="Services.ScanClientService"/>
/// which sends <c>ScanRequestPayload</c> to the server's 9877 port — no WIA calls here;</item>
/// <item>WPF imaging (<c>BitmapImage</c>/WIC) -> Avalonia <see cref="Bitmap"/> decode.</item>
/// </list>
///
/// deferred (not stubbed): image re-encoding/rotation for save/PDF export used WPF's
/// <c>BitmapDecoder/TransformedBitmap</c> — an Avalonia/Skia equivalent is not wired yet, so
/// non-JPEG conversions are skipped with a warning; <c>SaveAs</c> falls back to the auto-save
/// folder (the WPF <c>SaveFileDialog</c> needs the Avalonia <c>StorageProvider</c>, a view-layer
/// concern for Task 6).
/// </summary>
public partial class ScanViewModel : ObservableObject, IDisposable
{
    private readonly DiscoveryClientService _discoveryClient;
    private readonly ScanClientService? _scanClientService;

    [ObservableProperty]
    private string _targetIp = string.Empty;

    [ObservableProperty]
    private bool _isScanning; // discovery status

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsPreviewVisible))]
    [NotifyPropertyChangedFor(nameof(IsEmptyStateVisible))]
    [NotifyPropertyChangedFor(nameof(IsLoadingVisible))]
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
    private double _zoomLevel = 0.25;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ZoomPercentText))]
    private bool _isFitMode = true;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(Preview))]
    [NotifyPropertyChangedFor(nameof(RotationAngle))]
    [NotifyPropertyChangedFor(nameof(ZoomPercentText))]
    [NotifyPropertyChangedFor(nameof(IsPreviewVisible))]
    private ScannedPageItem? _selectedPage;

    public Bitmap? Preview => SelectedPage?.Preview;

    public double RotationAngle => SelectedPage?.RotationAngle ?? 0;

    partial void OnSelectedPageChanged(ScannedPageItem? value)
    {
        OnPropertyChanged(nameof(Preview));
        OnPropertyChanged(nameof(RotationAngle));
        OnPropertyChanged(nameof(ZoomPercentText));
        OnPropertyChanged(nameof(IsPreviewVisible));
    }

    public string ZoomPercentText => IsFitMode ? "Fit" : $"{Math.Round(ZoomLevel * 100)}%";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsEmptyStateVisible))]
    private bool _hasScannedFile;

    public ObservableCollection<ScannerDisplayItem> DiscoveredScanners { get; } = new();
    public ObservableCollection<ScannedPageItem> ScannedPages { get; } = new();
    public List<int> DpiOptions { get; } = new() { 150, 300, 600 };
    public List<string> FormatOptions { get; } = new() { "JPEG", "PNG", "PDF" };

    public ObservableCollection<string> Logs { get; } = new();
    public string LogsText => string.Join(Environment.NewLine, Logs);

    // WPF Visibility -> bool state (view picks the visual, Task 6).
    public bool IsPreviewVisible => Preview != null && !IsPerformingScan;
    public bool IsEmptyStateVisible => !HasScannedFile && !IsPerformingScan;
    public bool IsLoadingVisible => IsPerformingScan;

    public ScanViewModel(DiscoveryClientService discoveryClient, IServiceProvider serviceProvider)
    {
        _discoveryClient = discoveryClient;
        _scanClientService = ViewModelSupport.Resolve<ScanClientService>(serviceProvider);

        AppLogger.OnLog += AppLogger_OnLog;
    }

    private void AppLogger_OnLog(string msg)
    {
        if (msg.Contains("[SERVER]", StringComparison.OrdinalIgnoreCase)) return;

        void append() => AppendLog(msg);
        if (Avalonia.Application.Current is not null)
            Dispatcher.UIThread.Post(append);
        else
            append();
    }

    private void AppendLog(string msg)
    {
        Logs.Insert(0, msg);
        if (Logs.Count > 100)
        {
            Logs.RemoveAt(Logs.Count - 1);
        }
        OnPropertyChanged(nameof(LogsText));
    }

    [RelayCommand]
    private async Task ScanLanAsync()
    {
        string? targetIp = null;
        if (!string.IsNullOrWhiteSpace(TargetIp))
        {
            if (!System.Net.IPAddress.TryParse(TargetIp.Trim(), out _))
            {
                StatusText = "Invalid IP Address format!";
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
            StatusText = "Please select a scanner first.";
            return;
        }

        if (_scanClientService == null)
        {
            StatusText = "Scanner backend unavailable on this platform.";
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
                SelectedFormat
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

                Bitmap? preview = null;
                if (rawImageBytes != null && rawImageBytes.Length > 0)
                {
                    try
                    {
                        using var ms = new MemoryStream(rawImageBytes);
                        preview = new Bitmap(ms);
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
                    if (AppSettings.Current.AutoSaveScans)
                    {
                        AutoSavePage(newPage);
                    }
                }
                else
                {
                    StatusText = "Scan finished but the image could not be rendered.";
                }
            }
            else
            {
                StatusText = "Scan failed.";
                AppLogger.Error($"[CLIENT] Scan failed: {response.ErrorMessage}");
            }
        }
        catch (Exception ex)
        {
            StatusText = "Scan error.";
            AppLogger.Error($"[CLIENT] Error executing scan: {ex.Message}", ex);
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
            string dir = string.IsNullOrEmpty(AppSettings.Current.DefaultScansFolder)
                ? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "ShaPrint Scans")
                : AppSettings.Current.DefaultScansFolder;

            if (!Directory.Exists(dir)) Directory.CreateDirectory(dir);

            byte[] bytesToSave = page.ImageBytes;

            // deferred: RotateImageBytes used WPF TransformedBitmap — see class doc.
            if (page.RotationAngle != 0)
            {
                AppLogger.Log($"[CLIENT] Rotation skipped for auto-save (Avalonia imaging pending).");
            }

            string fileName = $"Scan_{DateTime.Now:yyyyMMdd_HHmmss}.{page.Format}";
            string fullPath = Path.Combine(dir, fileName);

            File.WriteAllBytes(fullPath, bytesToSave);
            AppLogger.Log($"[CLIENT] Auto-saved scan to {fullPath}");
            StatusText = $"Auto-saved scan to {fullPath}";
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

        // deferred: the WPF SaveFileDialog is replaced by a write to the default scans folder;
        // an Avalonia StorageProvider-driven picker belongs to the view layer (Task 6).
        StatusText = "Saving to default scans folder (file dialog pending)...";

        if (SelectedPage != null)
        {
            AutoSavePage(SelectedPage);
        }
        else
        {
            StatusText = "Please select a page to save.";
        }
    }

    /// <summary>Pure bytes: compiles JPEG pages into a single PDF (unchanged from WpfApp).</summary>
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
                        var bitmap = new Bitmap(imgMs);
                        pixelWidth = bitmap.PixelSize.Width;
                        pixelHeight = bitmap.PixelSize.Height;

                        double dpiX = bitmap.Dpi.X;
                        double dpiY = bitmap.Dpi.Y;
                        if (dpiX <= 10 || dpiX > 4800) dpiX = 96.0;
                        if (dpiY <= 10 || dpiY > 4800) dpiY = 96.0;

                        pointsWidth = (int)Math.Round(pixelWidth * 72.0 / dpiX);
                        pointsHeight = (int)Math.Round(pixelHeight * 72.0 / dpiY);
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
    private void ZoomIn()
    {
        double currentScale = ZoomLevel;
        if (IsFitMode)
        {
            // deferred: CalculateFitScale needs the ScrollViewer viewport — Task 6 view wiring.
            IsFitMode = false;
        }

        if (currentScale < 3.0)
            ZoomLevel = Math.Min(3.0, currentScale + 0.1);
    }

    [RelayCommand]
    private void ZoomOut()
    {
        double currentScale = ZoomLevel;
        if (IsFitMode)
        {
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

    public void Dispose()
    {
        AppLogger.OnLog -= AppLogger_OnLog;
    }
}