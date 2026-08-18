[CmdletBinding()]
param(
    [Parameter(Mandatory)] [ValidateSet("nsis", "msi", "dmg")] [string] $InstallerKind,
    [Parameter(Mandatory)] [string] $ReleaseVersion,
    [string] $RepositoryRoot = (Split-Path -Parent $PSScriptRoot),
    [string] $StageDirectory,
    [switch] $SignedUpdater,
    [switch] $ValidateOnly
)

# Builds (or with -ValidateOnly, only checks) one installer format and, with -StageDirectory,
# copies the release assets under their published names. Runs on Windows and macOS.

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($ReleaseVersion -notmatch '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$') {
    throw "ReleaseVersion '$ReleaseVersion' must use numeric X.Y.Z SemVer."
}
if (-not (Test-Path -LiteralPath $RepositoryRoot -PathType Container)) {
    throw "Repository root does not exist: $RepositoryRoot"
}

# One row per installer format: what `tauri build` is asked for, where the installer people
# download lands, and which artifact (plus its .sig) the in-app updater downloads.
$formats = @{
    nsis = @{
        Bundles = "nsis"
        InstallerDirectory = "nsis"
        InstallerExtension = ".exe"
        InstallerName = "on-n-off_${ReleaseVersion}_x64-setup.exe"
        UpdaterDirectory = "nsis"
        UpdaterName = "on-n-off_${ReleaseVersion}_x64-setup.exe"
        UpdaterStagedName = "on-n-off_${ReleaseVersion}_x64-setup.exe"
    }
    msi = @{
        Bundles = "msi"
        InstallerDirectory = "msi"
        InstallerExtension = ".msi"
        InstallerName = "on-n-off_${ReleaseVersion}_x64_en-US.msi"
        UpdaterDirectory = "msi"
        UpdaterName = "on-n-off_${ReleaseVersion}_x64_en-US.msi"
        UpdaterStagedName = "on-n-off_${ReleaseVersion}_x64_en-US.msi"
    }
    dmg = @{
        Bundles = "app,dmg"
        InstallerDirectory = "dmg"
        InstallerExtension = ".dmg"
        InstallerName = "on-n-off_${ReleaseVersion}_aarch64.dmg"
        UpdaterDirectory = "macos"
        UpdaterName = "on-n-off.app.tar.gz"
        UpdaterStagedName = "on-n-off_${ReleaseVersion}_aarch64.app.tar.gz"
    }
}
$format = $formats[$InstallerKind]
$bundleRoot = Join-Path $RepositoryRoot "src-tauri" "target" "release" "bundle"
$installerDirectory = Join-Path $bundleRoot $format.InstallerDirectory

if (-not $ValidateOnly) {
    $previousKind = [Environment]::GetEnvironmentVariable("ON_N_OFF_INSTALLER_KIND", "Process")
    Push-Location $RepositoryRoot
    try {
        $env:ON_N_OFF_INSTALLER_KIND = $InstallerKind
        $arguments = @("run", "tauri", "--", "build", "--bundles", $format.Bundles)
        if ($SignedUpdater) {
            $arguments += @("--config", "src-tauri/tauri.updater.conf.json")
        }
        & bun @arguments
        if ($LASTEXITCODE -ne 0) {
            throw "$InstallerKind bundle build failed with exit code $LASTEXITCODE."
        }
    }
    finally {
        Pop-Location
        if ($null -eq $previousKind) {
            Remove-Item Env:ON_N_OFF_INSTALLER_KIND -ErrorAction SilentlyContinue
        } else {
            $env:ON_N_OFF_INSTALLER_KIND = $previousKind
        }
    }
}

if (-not (Test-Path -LiteralPath $installerDirectory -PathType Container)) {
    throw "Expected $InstallerKind bundle directory is missing: $installerDirectory"
}
$installers = @(Get-ChildItem -LiteralPath $installerDirectory -File | Where-Object Extension -eq $format.InstallerExtension)
if ($installers.Count -ne 1 -or $installers[0].Name -ne $format.InstallerName) {
    $actualNames = ($installers | Select-Object -ExpandProperty Name) -join ", "
    throw "Expected exactly '$($format.InstallerName)' in '$installerDirectory'; found: $actualNames"
}
$installerPath = $installers[0].FullName
Write-Output "installer=$installerPath"

$updaterPath = $null
$signaturePath = $null
if ($SignedUpdater) {
    $updaterPath = Join-Path $bundleRoot $format.UpdaterDirectory $format.UpdaterName
    $signaturePath = "$updaterPath.sig"
    if (-not (Test-Path -LiteralPath $updaterPath -PathType Leaf)) {
        throw "Expected updater artifact is missing: $updaterPath"
    }
    if (-not (Test-Path -LiteralPath $signaturePath -PathType Leaf)) {
        throw "Expected updater signature is missing: $signaturePath"
    }
    if ([string]::IsNullOrWhiteSpace([System.IO.File]::ReadAllText($signaturePath))) {
        throw "Updater signature is empty: $signaturePath"
    }
    Write-Output "updater=$updaterPath"
    Write-Output "signature=$signaturePath"
}

if ($StageDirectory) {
    New-Item -ItemType Directory -Path $StageDirectory -Force | Out-Null
    Copy-Item -LiteralPath $installerPath -Destination (Join-Path $StageDirectory $format.InstallerName)
    if ($SignedUpdater) {
        if ($format.UpdaterStagedName -ne $format.InstallerName) {
            Copy-Item -LiteralPath $updaterPath -Destination (Join-Path $StageDirectory $format.UpdaterStagedName)
        }
        Copy-Item -LiteralPath $signaturePath -Destination (Join-Path $StageDirectory "$($format.UpdaterStagedName).sig")
    }
    Write-Output "staged=$StageDirectory"
}
