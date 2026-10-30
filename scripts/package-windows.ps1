param(
    [string]$BuildDir = "$PSScriptRoot/../target/x86_64-pc-windows-msvc/release",
    [string]$OutputDir = "$PSScriptRoot/../target/package",
    [string]$Wix = 'wix'
)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot/..").Path
$build = (Resolve-Path $BuildDir).Path
if (-not (Test-Path -LiteralPath "$build/rustclip.exe")) {
    throw 'Build rustclip.exe before packaging.'
}
$metadata = cargo metadata --manifest-path "$repo/Cargo.toml" --no-deps --format-version 1 --locked | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }
$version = ($metadata.packages | Where-Object name -eq 'rustclip').version
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw "MSI requires a three-part version: $version" }
$buildVersion = if ($env:RUSTCLIP_BUILD_VERSION) { $env:RUSTCLIP_BUILD_VERSION } else { $version }
if ($buildVersion -notmatch "^$([regex]::Escape($version))(-(dev|rc)\.(0|[1-9]\d*))?$") {
    throw "Build version must match Cargo.toml: $buildVersion"
}
$parts = $version.Split('.')
if ([int]$parts[0] -gt 255 -or [int]$parts[1] -gt 255 -or [int]$parts[2] -gt 65535) {
    throw "Version exceeds Windows Installer limits: $version"
}
New-Item -ItemType Directory -Force $OutputDir | Out-Null
$output = (Resolve-Path $OutputDir).Path
$license = (Get-Content -LiteralPath "$repo/LICENSE" -Raw).
    Replace('\', '\\').Replace('{', '\{').Replace('}', '\}') -replace '\r?\n', '\par '
Set-Content -LiteralPath "$output/LICENSE.rtf" -Encoding ascii -Value "{\rtf1\ansi\deff0{\fonttbl{\f0 Segoe UI;}}\f0\fs18 $license}"
$msi = "$output/RustClip-$buildVersion-x64.msi"
& $Wix build -arch x64 -culture en-US -ext WixToolset.UI.wixext/6.0.2 -ext WixToolset.Util.wixext/6.0.2 `
    -d "ProductVersion=$version" -d "BuildDir=$build" -d "RepoDir=$repo" `
    -d "LicenseRtf=$output/LICENSE.rtf" -o $msi "$repo/installer/RustClip.wxs"
if ($LASTEXITCODE -ne 0) { throw 'WiX build failed' }
& $Wix msi validate $msi
if ($LASTEXITCODE -ne 0) { throw 'MSI validation failed' }
Copy-Item -LiteralPath "$build/rustclip.exe", "$repo/LICENSE", "$repo/README.md" -Destination $output
$zip = "$output/RustClip-$buildVersion-windows-x64.zip"
Compress-Archive -LiteralPath "$output/rustclip.exe", "$output/LICENSE", "$output/README.md" -DestinationPath $zip -Force
Get-FileHash -Algorithm SHA256 -LiteralPath $msi, $zip, "$output/rustclip.exe" |
    ForEach-Object { '{0}  {1}' -f $_.Hash.ToLowerInvariant(), (Split-Path $_.Path -Leaf) } |
    Set-Content -LiteralPath "$output/SHA256SUMS.txt" -Encoding ascii
Write-Host "Windows packages: $output"
