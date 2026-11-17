$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot/..").Path
$folder = Join-Path $repo ('target\update-test-' + [guid]::NewGuid().ToString('N'))
$data = Join-Path $folder ('data ' + [char]0x0442 + ' with spaces')
New-Item -ItemType Directory -Path "$data/clips" -Force | Out-Null
$executable = Join-Path $folder 'rustclip.exe'
$package = Join-Path $folder 'download.exe'
Copy-Item -LiteralPath "$env:SystemRoot/System32/where.exe" -Destination $executable
Copy-Item -LiteralPath "$env:SystemRoot/System32/whoami.exe" -Destination $package
Set-Content -LiteralPath "$data/settings.json" -Value 'settings fixture'
Set-Content -LiteralPath "$data/secrets.vault" -Value 'encrypted fixture'
Set-Content -LiteralPath "$data/clips/test.json" -Value 'history fixture'
$writer = Join-Path $folder 'writer.ps1'
@'
param([string]$Data)
Start-Sleep -Milliseconds 1200
Set-Content -LiteralPath (Join-Path $Data 'settings.json') -Value 'settings saved on exit'
'@ | Set-Content -LiteralPath $writer -Encoding ASCII
$application = Start-Process powershell.exe -ArgumentList "-NoProfile -NonInteractive -File `"$writer`" -Data `"$data`"" -WindowStyle Hidden -PassThru
$plan = @{
    executable = $executable; package = $package
    sha256 = (Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash
    data = $data; application = $application.Id; supervisor = $null
    product = $null; downgrade = $false; recovery = $null
}
$planFile = Join-Path $folder 'install.json'
$plan | ConvertTo-Json | Set-Content -LiteralPath $planFile -Encoding UTF8
Set-Content -LiteralPath (Join-Path $folder 'go') -Value 'install'
& powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$repo/src/updates/install.ps1" -PlanFile $planFile
if ($LASTEXITCODE -ne 0) { throw "Portable update failed; see $folder" }
if ((Get-FileHash -LiteralPath $executable).Hash -ne $plan.sha256) { throw 'Portable EXE was not replaced' }
if ((Get-FileHash -LiteralPath "$executable.previous").Hash -ne (Get-FileHash -LiteralPath "$env:SystemRoot/System32/where.exe").Hash) {
    throw 'Previous portable EXE was not preserved'
}
$backups = @(Get-ChildItem -LiteralPath "$data/Backup" -Directory)
if ($backups.Count -ne 1) { throw 'Expected one pre-update backup' }
if ((Get-Content -LiteralPath (Join-Path $backups[0].FullName 'settings.json') -Raw).Trim() -ne 'settings saved on exit') {
    throw 'Updater did not wait for the application to save settings and exit'
}
foreach ($name in @('settings.json', 'secrets.vault', 'clips/test.json')) {
    if ((Get-FileHash -LiteralPath (Join-Path $data $name)).Hash -ne (Get-FileHash -LiteralPath (Join-Path $backups[0].FullName $name)).Hash) {
        throw "Backup mismatch: $name"
    }
}
# Check the elevated portable branch without requesting privileges for this writable fixture.
& powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$repo/src/updates/install.ps1" -PlanFile $planFile -Elevated
if ($LASTEXITCODE -ne 0) { throw 'Elevated portable replacement failed' }
# Simulate a protected folder and verify elevation and the subsequent unelevated restart.
$elevationTest = Join-Path $folder 'test-elevation.ps1'
@'
param([string]$PlanFile, [string]$Helper)
$ErrorActionPreference = 'Stop'
$script:elevated = $false
$script:restarted = $false
function Copy-Item {
    param($LiteralPath, $Destination, [switch]$Force, [switch]$Recurse)
    if ($Destination -eq "$($plan.executable).previous") {
        throw [System.UnauthorizedAccessException]::new('Protected installation fixture')
    }
    Microsoft.PowerShell.Management\Copy-Item @PSBoundParameters
}
function Start-Process {
    param($FilePath, $ArgumentList, $Verb, $WindowStyle, [switch]$Wait, [switch]$PassThru)
    if ($Verb -eq 'RunAs') {
        if ($FilePath -ne 'powershell.exe' -or $ArgumentList -notmatch '-Elevated' -or -not $Wait) {
            throw 'Incorrect elevation request'
        }
        $script:elevated = $true
        return [pscustomobject]@{ ExitCode = 0 }
    }
    if ($Verb -or $FilePath -ne $plan.executable -or -not $script:elevated) { throw 'Incorrect restart context' }
    $script:restarted = $true
}
. $Helper -PlanFile $PlanFile
if (-not $script:elevated -or -not $script:restarted) { throw 'Protected portable update did not elevate and restart' }
'@ | Set-Content -LiteralPath $elevationTest -Encoding ASCII
$plan.application = $null
$plan | ConvertTo-Json | Set-Content -LiteralPath $planFile -Encoding UTF8
& powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $elevationTest -PlanFile $planFile -Helper "$repo/src/updates/install.ps1"
if ($LASTEXITCODE -ne 0) { throw 'Protected portable update regression test failed' }
# Exercise checksum failure without elevation or a real MSI installation.
$plan.sha256 = '0' * 64
$plan | ConvertTo-Json | Set-Content -LiteralPath $planFile -Encoding UTF8
& powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$repo/src/updates/install.ps1" -PlanFile $planFile -Elevated
if ($LASTEXITCODE -ne 1) { throw 'Invalid checksum was accepted' }
if ((Get-Content -LiteralPath "$folder/error.txt" -Raw) -notmatch 'SHA-256 mismatch') { throw 'Expected checksum error' }
Write-Host "Portable replacement, elevation fallback, unelevated restart, backup, and checksum rejection passed: $folder"
exit 0
