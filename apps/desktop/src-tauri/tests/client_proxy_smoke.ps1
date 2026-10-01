param(
    [Parameter(Mandatory = $true)][string]$QueueName,
    [Parameter(Mandatory = $true)][string]$IppUrl
)

$ErrorActionPreference = "Stop"

try {
    Write-Host "[SMOKE-PS] Adding printer $QueueName with URL $IppUrl..."
    Add-Printer -Name $QueueName -IppURL $IppUrl
    Write-Host "[SMOKE-PS] Add-Printer completed successfully."

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
        Write-Host "[SMOKE-PS] Calling document.Print()..."
        $document.Print()
        Write-Host "[SMOKE-PS] document.Print() returned."
    }
    finally {
        $document.Dispose()
    }

    Write-Host "[SMOKE-PS] Waiting for spooler to deliver print job..."
    Start-Sleep -Milliseconds 500
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    while ([DateTime]::UtcNow -lt $deadline) {
        $jobs = Get-PrintJob -PrinterName $QueueName -ErrorAction SilentlyContinue
        if ($jobs -and $jobs.Count -gt 0) {
            Start-Sleep -Milliseconds 200
        } else {
            Write-Host "[SMOKE-PS] Spool queue is empty, print job dispatched."
            break
        }
    }
}
finally {
    if (Get-Printer -Name $QueueName -ErrorAction SilentlyContinue) {
        Write-Host "[SMOKE-PS] Cleaning up printer $QueueName..."
        Remove-Printer -Name $QueueName
        Write-Host "[SMOKE-PS] Cleanup complete."
    }
}
