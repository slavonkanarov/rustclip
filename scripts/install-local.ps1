#requires -Version 5.1
param([switch]$Offline, [switch]$SystemOnly)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot\..").Path
$installDir = Join-Path $env:ProgramFiles 'RustClip'
$oldInstallDir = Join-Path $env:LOCALAPPDATA 'Programs\RustClip'
$executable = Join-Path $installDir 'rustclip.exe'
$oldExecutable = Join-Path $oldInstallDir 'rustclip.exe'
if (Get-Process rustclip -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -eq $executable -or $_.Path -eq $oldExecutable }) {
    throw 'Exit the installed RustClip from its tray menu before updating.'
}

function Update-UserInstallation {
    $userShortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'RustClip.lnk'
    if (Test-Path -LiteralPath $userShortcut) {
        $shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($userShortcut)
        $shortcut.TargetPath = $executable
        $shortcut.WorkingDirectory = $installDir
        $shortcut.IconLocation = "$(Join-Path $installDir 'icon.ico'),0"
        $shortcut.Description = 'Clipboard history and encrypted secrets'
        $shortcut.Save()
    }
    $runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
    if ((Get-ItemProperty -LiteralPath $runKey -Name RustClip -ErrorAction SilentlyContinue).RustClip) {
        Set-ItemProperty -LiteralPath $runKey -Name RustClip -Value "`"$executable`""
    }
    foreach ($name in @('rustclip.exe', 'icon.ico', 'LICENSE', 'README.md')) {
        $path = Join-Path $oldInstallDir $name
        if (Test-Path -LiteralPath $path -PathType Leaf) { Remove-Item -LiteralPath $path -Force }
    }
    if ((Test-Path -LiteralPath $oldInstallDir -PathType Container) -and
        -not (Get-ChildItem -LiteralPath $oldInstallDir -Force)) {
        Remove-Item -LiteralPath $oldInstallDir
    }
    Write-Host "Installed: $executable"
    Write-Host "Start menu shortcut: $(Join-Path ([Environment]::GetFolderPath('CommonPrograms')) 'RustClip.lnk')"
    Write-Host 'Existing data in %APPDATA%\RustClip is preserved.'
}

Push-Location $repo
try {
    if (-not $SystemOnly) {
        $buildArgs = @('build', '--release', '--locked', '--target', 'x86_64-pc-windows-msvc', '--target-dir', "$repo\target")
        if ($Offline) { $buildArgs += '--offline' }
        & cargo +stable-x86_64-pc-windows-msvc @buildArgs
        if ($LASTEXITCODE -ne 0) {
            throw 'Build failed. Rust MSVC and Visual Studio C++ Build Tools with the Windows SDK are required.'
        }
    }

    $build = Join-Path $repo 'target\x86_64-pc-windows-msvc\release\rustclip.exe'
    if (-not (Test-Path -LiteralPath $build -PathType Leaf)) { throw 'Build did not produce rustclip.exe' }

    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        if ($SystemOnly) { throw 'Administrator rights are required to install RustClip.' }
        $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"")
        if ($Offline) { $arguments += '-Offline' }
        $arguments += '-SystemOnly'
        try {
            $elevated = Start-Process -FilePath (Get-Process -Id $PID).Path -ArgumentList $arguments `
                -Verb RunAs -WorkingDirectory $repo -WindowStyle Hidden -Wait -PassThru
        } catch {
            throw "Administrator permission was not granted: $($_.Exception.Message)"
        }
        if ($elevated.ExitCode -ne 0) {
            throw "Elevated installation failed with exit code $($elevated.ExitCode). Run PowerShell as Administrator to see the error."
        }
        Update-UserInstallation
        return
    }

    $msiInstallations = @(Get-ItemProperty `
        'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*', `
        'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue |
        Where-Object { $_.DisplayName -eq 'RustClip' -and $_.WindowsInstaller -eq 1 })
    foreach ($msi in $msiInstallations) {
        if ($msi.PSChildName -notmatch '^\{[0-9A-Fa-f-]{36}\}$') {
            throw "Cannot identify the old RustClip MSI: $($msi.PSChildName)"
        }
        $process = Start-Process msiexec.exe -ArgumentList @('/x', $msi.PSChildName, '/qn', '/norestart') `
            -WindowStyle Hidden -Wait -PassThru
        if ($process.ExitCode -eq 3010) {
            throw 'The old RustClip MSI was removed. Restart Windows, then run this script again.'
        }
        if ($process.ExitCode -ne 0) {
            throw "Removing the old RustClip MSI failed with exit code $($process.ExitCode)."
        }
    }

    $shortcutDir = [Environment]::GetFolderPath('CommonPrograms')
    New-Item -ItemType Directory -Force $installDir, $shortcutDir | Out-Null
    Copy-Item -LiteralPath $build, "$repo\assets\icon.ico", "$repo\LICENSE", "$repo\README.md" `
        -Destination $installDir -Force
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $build).Hash -ne
        (Get-FileHash -Algorithm SHA256 -LiteralPath $executable).Hash) {
        throw 'Installed rustclip.exe does not match the current build'
    }

    $shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path $shortcutDir 'RustClip.lnk'))
    $shortcut.TargetPath = $executable
    $shortcut.WorkingDirectory = $installDir
    $shortcut.IconLocation = "$(Join-Path $installDir 'icon.ico'),0"
    $shortcut.Description = 'Clipboard history and encrypted secrets'
    $shortcut.Save()
    if (-not $SystemOnly) { Update-UserInstallation }
} finally {
    Pop-Location
}
