#requires -Version 5.1
param(
    [string]$Executable = "$PSScriptRoot\..\target\x86_64-pc-windows-msvc\release\rustclip.exe"
)

$ErrorActionPreference = 'Stop'
$executablePath = (Resolve-Path -LiteralPath $Executable).Path
$data = Join-Path ([IO.Path]::GetTempPath()) "rustclip-recovery-$([Guid]::NewGuid().ToString('N'))"
$previousData = $env:RUSTCLIP_DATA_DIR
$supervisor = $null
$children = [Collections.Generic.HashSet[int]]::new()

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class RustClipRecoveryTest {
    public delegate bool EnumCallback(IntPtr hwnd, IntPtr param);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumCallback callback, IntPtr param);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int size);
    [DllImport("user32.dll", SetLastError = true)] public static extern IntPtr SendMessageTimeout(IntPtr hwnd, uint message, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
}
'@

function Wait-Application([int]$ParentId, [int]$PreviousId = 0) {
    $deadline = (Get-Date).AddSeconds(20)
    while ((Get-Date) -lt $deadline) {
        $processes = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $ParentId" |
            Where-Object { $_.ExecutablePath -eq $executablePath -and $_.ProcessId -ne $PreviousId })
        foreach ($candidate in $processes) {
            $candidateId = [int]$candidate.ProcessId
            [void]$children.Add($candidateId)
            $script:appWindow = [IntPtr]::Zero
            [RustClipRecoveryTest]::EnumWindows({
                param($handle, $unused)
                $owner = 0u
                [void][RustClipRecoveryTest]::GetWindowThreadProcessId($handle, [ref]$owner)
                $title = [Text.StringBuilder]::new(512)
                [void][RustClipRecoveryTest]::GetWindowText($handle, $title, 512)
                if ($owner -eq $candidateId -and $title.ToString() -like 'RustClip (*') {
                    $script:appWindow = $handle
                }
                return $true
            }, [IntPtr]::Zero) | Out-Null
            if ($script:appWindow -ne [IntPtr]::Zero) {
                $result = [IntPtr]::Zero
                if ([RustClipRecoveryTest]::SendMessageTimeout(
                    $script:appWindow, 0, [IntPtr]::Zero, [IntPtr]::Zero, 2, 2000, [ref]$result
                ) -ne [IntPtr]::Zero) {
                    return $candidate
                }
            }
        }
        Start-Sleep -Milliseconds 100
    }
    throw 'Application did not create a responsive window'
}

try {
    $env:RUSTCLIP_DATA_DIR = $data
    New-Item -ItemType Directory -Path (Join-Path $data 'clips') | Out-Null
    [IO.File]::WriteAllText((Join-Path $data 'clips\1.json'), '{broken')
    $supervisor = Start-Process -FilePath $executablePath -ArgumentList '--demo' -PassThru
    $first = Wait-Application $supervisor.Id
    Stop-Process -Id ([int]$first.ProcessId) -Force
    $restarted = Wait-Application $supervisor.Id ([int]$first.ProcessId)
    if ($restarted.CommandLine -notmatch '--safe-mode') { throw 'Restart did not enable safe mode' }
    $log = Get-Content -LiteralPath (Join-Path $data 'runtime.log') -Raw
    foreach ($expected in @('Application crashed:', 'Restarting in safe mode', 'safe_mode=true', '1.json')) {
        if (-not $log.Contains($expected)) { throw "Missing diagnostic record: $expected" }
    }
    if (-not (Test-Path -LiteralPath (Join-Path $data 'clips\1.json'))) {
        throw 'Corrupt history file was deleted'
    }
    Write-Host 'PASS: native process termination logged; responsive UI restarted in safe mode; corrupt clip preserved.'
} finally {
    if ($supervisor) {
        # Stop the supervisor first so test cleanup cannot trigger another restart.
        Get-CimInstance Win32_Process -Filter "ParentProcessId = $($supervisor.Id)" |
            Where-Object { $_.ExecutablePath -eq $executablePath } |
            ForEach-Object { [void]$children.Add([int]$_.ProcessId) }
        Stop-Process -Id $supervisor.Id -Force -ErrorAction SilentlyContinue
        foreach ($childId in $children) {
            $child = Get-CimInstance Win32_Process -Filter "ProcessId = $childId"
            if ($child -and $child.ExecutablePath -eq $executablePath) {
                Stop-Process -Id $childId -Force -ErrorAction SilentlyContinue
            }
        }
    }
    $env:RUSTCLIP_DATA_DIR = $previousData
    if (Test-Path -LiteralPath $data) {
        Get-ChildItem -LiteralPath $data -File -Recurse | ForEach-Object {
            Remove-Item -LiteralPath $_.FullName -Force
        }
        Get-ChildItem -LiteralPath $data -Directory -Recurse |
            Sort-Object { $_.FullName.Length } -Descending | ForEach-Object {
                Remove-Item -LiteralPath $_.FullName
            }
        Remove-Item -LiteralPath $data
    }
}
