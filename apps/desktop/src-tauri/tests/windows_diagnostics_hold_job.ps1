# Fixture-only work on a paused temporary queue. Diagnostics itself never submits a job.
$ErrorActionPreference = 'Stop'
$name = $env:SHAPRINT_DIAGNOSTICS_QUEUE
$printer = Get-CimInstance -ClassName Win32_Printer | Where-Object { $_.Name -eq $name }
if ($null -eq $printer) { throw 'temporary queue is missing' }
$paused = Invoke-CimMethod -InputObject $printer -MethodName Pause
if ($paused.ReturnValue -ne 0) { throw 'could not pause temporary queue' }

Add-Type -AssemblyName System.Drawing
$document = [System.Drawing.Printing.PrintDocument]::new()
try {
    $document.PrinterSettings.PrinterName = $name
    $document.PrintController = [System.Drawing.Printing.StandardPrintController]::new()
    $document.add_PrintPage({
        param($sender, $eventArgs)
        # Generate fixture content at runtime; do not keep document fixtures or output them.
        $eventArgs.Graphics.DrawString([System.Guid]::NewGuid().ToString(),
            [System.Drawing.SystemFonts]::DefaultFont, [System.Drawing.Brushes]::Black, 10, 10)
        $eventArgs.HasMorePages = $false
    })
    $document.Print()
}
finally { $document.Dispose() }

if (@(Get-PrintJob -PrinterName $name).Count -eq 0) { throw 'temporary queue has no held work' }
