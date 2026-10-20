[CmdletBinding()]
param(
    [string]$Destination = $env:RUNNER_TEMP
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$version = "0.9.2"
$pinned = @{
    "aarch64-apple-darwin"   = "ae72f0df0c399a1e96336f696fa55b1b28679fd725632eba8cf8e4568467cc3e"
    "x86_64-pc-windows-msvc" = "1c03e5890238562497c2d89a3b75b02560af349c1fc3e713d3284f532a5cd748"
}

if ([string]::IsNullOrWhiteSpace($Destination)) {
    throw "Pass -Destination, or run where RUNNER_TEMP is set."
}
$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
$triple = if ($IsWindows -and $architecture -eq "X64") {
    "x86_64-pc-windows-msvc"
} elseif ($IsMacOS -and $architecture -eq "Arm64") {
    "aarch64-apple-darwin"
} else {
    throw "No cargo-about build is pinned for this platform ($architecture)."
}

$archive = "cargo-about-$version-$triple"
$archivePath = Join-Path $Destination "$archive.tar.gz"
Invoke-WebRequest -Uri "https://github.com/EmbarkStudios/cargo-about/releases/download/$version/$archive.tar.gz" -OutFile $archivePath
$actual = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -ne $pinned[$triple]) {
    throw "cargo-about $version for $triple has SHA-256 $actual, not the pinned $($pinned[$triple])."
}
tar -xzf $archivePath -C $Destination
if ($LASTEXITCODE -ne 0) {
    throw "Unpacking $archivePath failed with exit code $LASTEXITCODE."
}

$binDirectory = Join-Path $Destination $archive
if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_PATH)) {
    Add-Content -LiteralPath $env:GITHUB_PATH -Value $binDirectory
}
Write-Output "cargo-about=$binDirectory"
