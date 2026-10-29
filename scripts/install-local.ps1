#requires -Version 5.1
param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\RustClip",
    [string]$ShortcutDir = [Environment]::GetFolderPath('Programs'),
    [switch]$Offline
)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot\..").Path
$InstallDir = [IO.Path]::GetFullPath($InstallDir)
$ShortcutDir = [IO.Path]::GetFullPath($ShortcutDir)
$executable = Join-Path $InstallDir 'rustclip.exe'
if (Get-Process rustclip -ErrorAction SilentlyContinue | Where-Object Path -eq $executable) {
    throw 'Exit the installed RustClip from its tray menu before updating.'
}

Push-Location $repo
try {
    $buildArgs = @('build', '--release', '--locked', '--target', 'x86_64-pc-windows-msvc', '--target-dir', "$repo\target")
    if ($Offline) { $buildArgs += '--offline' }
    & cargo +stable-x86_64-pc-windows-msvc @buildArgs
    if ($LASTEXITCODE -ne 0) {
        throw 'Build failed. Rust MSVC and Visual Studio C++ Build Tools with the Windows SDK are required.'
    }

    New-Item -ItemType Directory -Force $InstallDir, $ShortcutDir | Out-Null
    Copy-Item -LiteralPath "$repo\target\x86_64-pc-windows-msvc\release\rustclip.exe", `
        "$repo\assets\icon.ico", "$repo\LICENSE", "$repo\README.md" -Destination $InstallDir -Force

    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut((Join-Path $ShortcutDir 'RustClip.lnk'))
    $shortcut.TargetPath = $executable
    $shortcut.WorkingDirectory = $InstallDir
    $shortcut.IconLocation = "$(Join-Path $InstallDir 'icon.ico'),0"
    $shortcut.Description = 'Clipboard history and encrypted secrets'
    $shortcut.Save()

    Write-Host "Installed: $executable"
    Write-Host "Start menu shortcut: $(Join-Path $ShortcutDir 'RustClip.lnk')"
    Write-Host 'Start RustClip from the shortcut. Existing data in %APPDATA%\RustClip is preserved.'
} finally {
    Pop-Location
}
