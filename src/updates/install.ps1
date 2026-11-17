param([Parameter(Mandatory)][string]$PlanFile, [switch]$Elevated)
$ErrorActionPreference = 'Stop'
$plan = Get-Content -LiteralPath $PlanFile -Raw -Encoding UTF8 | ConvertFrom-Json
$folder = Split-Path -Parent $PlanFile

function Assert-Package {
    if ((Get-FileHash -LiteralPath $plan.package -Algorithm SHA256).Hash -ne $plan.sha256) {
        throw 'Update package SHA-256 mismatch'
    }
}

function Invoke-Installer([string]$Arguments, [string]$LogName) {
    $log = Join-Path $folder $LogName
    $process = Start-Process msiexec.exe -ArgumentList "$Arguments /qn /norestart /l*v `"$log`"" -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -notin @(0, 3010)) { throw "Windows Installer failed: $($process.ExitCode). See $log" }
}

function Install-Portable {
    $previous = "$($plan.executable).previous"
    Copy-Item -LiteralPath $plan.executable -Destination $previous -Force
    try {
        Copy-Item -LiteralPath $plan.package -Destination $plan.executable -Force
    } catch {
        try { Copy-Item -LiteralPath $previous -Destination $plan.executable -Force } catch { }
        throw
    }
}

function Invoke-ElevatedUpdate {
    $arguments = "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$PSCommandPath`" -PlanFile `"$PlanFile`" -Elevated"
    $installer = Start-Process powershell.exe -ArgumentList $arguments -Verb RunAs -WindowStyle Hidden -Wait -PassThru
    if ($installer.ExitCode -ne 0) { throw "Installation failed. See $folder\error.txt" }
}

if ($Elevated) {
    try {
        Assert-Package
        if (-not $plan.product) {
            Install-Portable
            exit 0
        }
        if ($plan.product -notmatch '^\{[0-9A-Fa-f-]{36}\}$') { throw 'Invalid MSI product code' }
        $rollback = $null
        if ($plan.downgrade) {
            # Download and verify the complete current MSI before removing anything.
            if (-not $plan.recovery -or (Get-FileHash -LiteralPath $plan.recovery.package -Algorithm SHA256).Hash -ne $plan.recovery.sha256) {
                throw 'Verified recovery MSI is required for downgrade'
            }
            $rollback = $plan.recovery.package
            Invoke-Installer "/x $($plan.product)" 'uninstall.log'
        }
        try {
            Invoke-Installer "/i `"$($plan.package)`"" 'install.log'
        } catch {
            if ($rollback) { Invoke-Installer "/i `"$rollback`"" 'rollback.log' }
            throw
        }
        exit 0
    } catch {
        $_ | Out-String | Set-Content -LiteralPath (Join-Path $folder 'error.txt') -Encoding UTF8
        exit 1
    }
}

try {
    # Capture handles before signaling readiness; this also avoids PID reuse while waiting.
    $parents = @($plan.application, $plan.supervisor) | Where-Object { $_ } | ForEach-Object {
        try {
            $process = [System.Diagnostics.Process]::GetProcessById($_)
            $null = $process.Handle
            $process
        } catch [System.ArgumentException] { }
    }
    Set-Content -LiteralPath (Join-Path $folder 'ready') -Value 'ready'
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path -LiteralPath (Join-Path $folder 'go'))) {
        if ([DateTime]::UtcNow -gt $deadline) { exit 1 }
        Start-Sleep -Milliseconds 25
    }
    foreach ($process in $parents) {
        if (-not $process.WaitForExit(60000)) { throw 'RustClip did not close within 60 seconds' }
    }
    Assert-Package
    $backup = Join-Path $plan.data ('Backup\before-update-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $backup | Out-Null
    foreach ($name in @('clips', 'settings.json', 'secrets.vault')) {
        $source = Join-Path $plan.data $name
        if (Test-Path -LiteralPath $source) { Copy-Item -LiteralPath $source -Destination $backup -Recurse }
    }
    if ($plan.product) {
        Invoke-ElevatedUpdate
    } else {
        try {
            Install-Portable
        } catch [System.UnauthorizedAccessException] {
            # Local installations and portable EXEs can live in protected folders too.
            Invoke-ElevatedUpdate
        }
    }
    $env:RUSTCLIP_DATA_DIR = $plan.data
    Start-Process -FilePath $plan.executable -WindowStyle Hidden
} catch {
    $failure = $_ | Out-String
    $failure | Set-Content -LiteralPath (Join-Path $folder 'error.txt') -Encoding UTF8
    # Restart without elevation after cancellation or a failed installation.
    if (Test-Path -LiteralPath $plan.executable) {
        $env:RUSTCLIP_DATA_DIR = $plan.data
        Start-Process -FilePath $plan.executable -WindowStyle Hidden
    }
    $shell = New-Object -ComObject WScript.Shell
    $null = $shell.Popup("RustClip update failed. $failure", 20, 'RustClip', 16)
    exit 1
}
