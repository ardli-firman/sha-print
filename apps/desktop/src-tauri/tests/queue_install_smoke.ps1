# Issue #35 smoke: confirm the queue ShaPrint installed is in the spooler, then print one page
# through it the way the standard Windows print dialog would. The page is delivered to the fake
# server adapter the Rust smoke test runs, so no paper is used.
param(
    [Parameter(Mandatory = $true)][string]$QueueName
)

$ErrorActionPreference = "Stop"

if (-not (Get-Printer -Name $QueueName -ErrorAction SilentlyContinue)) {
    Write-Error "[SHAPRINT-35] the installed queue '$QueueName' is missing from the spooler"
    exit 1
}

Add-Type -AssemblyName System.Drawing
$document = [System.Drawing.Printing.PrintDocument]::new()
try {
    $document.PrinterSettings.PrinterName = $QueueName
    $document.PrintController = [System.Drawing.Printing.StandardPrintController]::new()
    $document.add_PrintPage({
        param($sender, $eventArgs)
        for ($y = 10; $y -le 120; $y += 10) {
            $eventArgs.Graphics.DrawLine([System.Drawing.Pens]::Black, 10, $y, 200, $y)
        }
        $eventArgs.HasMorePages = $false
    })
    Write-Host "[SHAPRINT-35] Printing through $QueueName..."
    $document.Print()
    Write-Host "[SHAPRINT-35] document.Print() returned."
}
finally {
    $document.Dispose()
}

Write-Host "[SHAPRINT-35] Waiting for the spooler to deliver the print job..."
Start-Sleep -Milliseconds 500
$deadline = [DateTime]::UtcNow.AddSeconds(20)
while ([DateTime]::UtcNow -lt $deadline) {
    $jobs = Get-PrintJob -PrinterName $QueueName -ErrorAction SilentlyContinue
    if ($jobs -and $jobs.Count -gt 0) {
        Start-Sleep -Milliseconds 200
    }
    else {
        Write-Host "[SHAPRINT-35] Spool queue is empty, the print job was dispatched."
        break
    }
}
