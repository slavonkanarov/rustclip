param([string]$Wix = 'wix')

# Run on a disposable Windows runner: this installs and removes RustClip.
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') {
    throw 'Run installation/removal tests only on a disposable GitHub Actions runner.'
}
$repo = (Resolve-Path "$PSScriptRoot/..").Path
$package = @(Get-ChildItem "$repo/target/package/*.msi" -File)
if ($package.Count -ne 1) { throw 'Expected exactly one MSI package' }
$msi = $package[0].FullName
$installer = New-Object -ComObject WindowsInstaller.Installer
$database = $installer.OpenDatabase($msi, 0)
$view = $database.OpenView('SELECT `Dialog` FROM `Dialog`')
$view.Execute()
$dialogs = @()
while ($record = $view.Fetch()) { $dialogs += $record.StringData(1) }
$view.Close()
foreach ($expected in @('WelcomeEulaDlg', 'ProgressDlg', 'ExitDialog', 'RemoveDataDlg')) {
    if ($expected -notin $dialogs) { throw "Installer dialog is missing: $expected" }
}
$view = $database.OpenView('SELECT `Value` FROM `Property` WHERE `Property` = ''ProductVersion''')
$view.Execute()
$version = $view.Fetch().StringData(1)
$view.Close()

$testDir = New-Item -ItemType Directory -Force "$repo/target/installer-test"
$previous = Join-Path $testDir 'previous.msi'
& $Wix build -arch x64 -culture en-US -ext WixToolset.UI.wixext/6.0.2 -ext WixToolset.Util.wixext/6.0.2 `
    -d "ProductVersion=$version" -d "BuildDir=$repo/target/x86_64-pc-windows-msvc/release" `
    -d "RepoDir=$repo" -d "LicenseRtf=$repo/target/package/LICENSE.rtf" `
    -o $previous "$repo/installer/RustClip.wxs"
if ($LASTEXITCODE -ne 0) { throw 'Building upgrade test package failed' }

function Invoke-Msi([string]$Arguments, [string]$LogName) {
    $log = Join-Path $testDir "$LogName.log"
    $process = Start-Process msiexec.exe -ArgumentList "$Arguments /qn /norestart /l*v `"$log`"" -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -notin @(0, 3010)) {
        throw "Windows Installer failed with $($process.ExitCode). See $log"
    }
}

$installDir = Join-Path $env:ProgramFiles 'RustClip'
$shortcut = Join-Path ([Environment]::GetFolderPath('CommonPrograms')) 'RustClip.lnk'
$dataDir = Join-Path $env:APPDATA 'RustClip'
if ((Test-Path -LiteralPath $installDir) -or (Test-Path -LiteralPath $dataDir)) {
    throw 'Installer tests require a clean runner without existing RustClip files or data.'
}
New-Item -ItemType Directory -Force "$dataDir/clips", "$dataDir/Backup/nested", "$dataDir/exports" | Out-Null
foreach ($name in @('settings.json', 'secrets.vault', 'clips/test.json', 'Backup/nested/test.json', 'exports/test.txt')) {
    Set-Content -LiteralPath "$dataDir/$name" -Value 'installer test fixture'
}
function Assert-DataPreserved {
    foreach ($name in @('settings.json', 'secrets.vault', 'clips/test.json', 'Backup/nested/test.json', 'exports/test.txt')) {
        if ((Get-Content -LiteralPath "$dataDir/$name" -Raw).Trim() -ne 'installer test fixture') {
            throw "User data changed: $name"
        }
    }
}
try {
    Invoke-Msi "/i `"$previous`"" 'install'
    Invoke-Msi "/i `"$msi`"" 'upgrade'
    Assert-DataPreserved
    foreach ($name in @('rustclip.exe', 'LICENSE.txt')) {
        if (-not (Test-Path -LiteralPath (Join-Path $installDir $name))) {
            throw "Installed file is missing: $name"
        }
    }
    if (-not (Test-Path -LiteralPath $shortcut)) { throw 'Start menu shortcut is missing' }
    $registrations = @(Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue |
        Where-Object DisplayName -eq 'RustClip')
    if ($registrations.Count -ne 1) { throw "Expected one RustClip installation, found $($registrations.Count)" }
} finally {
    Invoke-Msi "/x `"$msi`"" 'uninstall'
}
if (Test-Path -LiteralPath "$installDir/rustclip.exe") { throw 'Executable remains after uninstall' }
if (Test-Path -LiteralPath $shortcut) { throw 'Shortcut remains after uninstall' }
Assert-DataPreserved
Invoke-Msi "/i `"$msi`"" 'reinstall'
Invoke-Msi "/x `"$msi`" DELETEUSERDATA=1" 'uninstall-with-data'
if (Get-ChildItem -LiteralPath $dataDir -File -Recurse -Force -ErrorAction SilentlyContinue) {
    throw 'Explicit data removal left RustClip data behind'
}
