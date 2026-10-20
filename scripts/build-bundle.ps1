[CmdletBinding()]
param(
    [Parameter(Mandatory)] [ValidateSet("nsis", "dmg")] [string] $InstallerKind,
    [Parameter(Mandatory)] [string] $ReleaseVersion,
    [string] $RepositoryRoot = (Split-Path -Parent $PSScriptRoot),
    [string] $StageDirectory,
    [switch] $SignedUpdater,
    [switch] $ValidateOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($ReleaseVersion -notmatch '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$') {
    throw "ReleaseVersion '$ReleaseVersion' must use numeric X.Y.Z SemVer."
}
if (-not (Test-Path -LiteralPath $RepositoryRoot -PathType Container)) {
    throw "Repository root does not exist: $RepositoryRoot"
}

$formats = @{
    nsis = @{
        Bundles = "nsis"
        InstallerDirectory = "nsis"
        InstallerExtension = ".exe"
        InstallerName = "on-n-off_${ReleaseVersion}_x64-setup.exe"
        UpdaterDirectory = "nsis"
        UpdaterName = "on-n-off_${ReleaseVersion}_x64-setup.exe"
        UpdaterStagedName = "on-n-off_${ReleaseVersion}_x64-setup.exe"
        Licenses = @("THIRD_PARTY_LICENSES.md")
    }
    dmg = @{
        Bundles = "app,dmg"
        InstallerDirectory = "dmg"
        InstallerExtension = ".dmg"
        InstallerName = "on-n-off_${ReleaseVersion}_aarch64.dmg"
        UpdaterDirectory = "macos"
        UpdaterName = "on-n-off.app.tar.gz"
        UpdaterStagedName = "on-n-off_${ReleaseVersion}_aarch64.app.tar.gz"
        Licenses = @("bundle", "macos", "on-n-off.app", "Contents", "Resources", "THIRD_PARTY_LICENSES.md")
    }
}
$format = $formats[$InstallerKind]
$releaseRoot = Join-Path $RepositoryRoot "src-tauri" "target" "release"
$bundleRoot = Join-Path $releaseRoot "bundle"
$generatedLicenses = Join-Path $RepositoryRoot "src-tauri" "gen" "THIRD_PARTY_LICENSES.md"
$installerDirectory = Join-Path $bundleRoot $format.InstallerDirectory

if (-not $ValidateOnly) {
    $previousKind = [Environment]::GetEnvironmentVariable("ON_N_OFF_INSTALLER_KIND", "Process")
    Push-Location $RepositoryRoot
    try {
        $env:ON_N_OFF_INSTALLER_KIND = $InstallerKind
        & bun "scripts/third-party-licenses.mjs"
        if ($LASTEXITCODE -ne 0) {
            throw "Generating THIRD_PARTY_LICENSES.md failed with exit code $LASTEXITCODE."
        }
        $arguments = @("run", "tauri", "--", "build", "--bundles", $format.Bundles, "--config", "src-tauri/tauri.licenses.conf.json")
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

if (-not (Test-Path -LiteralPath $generatedLicenses -PathType Leaf)) {
    throw "The license notice was not generated: $generatedLicenses"
}
$carriedLicenses = Join-Path $releaseRoot @($format.Licenses)
if (-not (Test-Path -LiteralPath $carriedLicenses -PathType Leaf)) {
    throw "The $InstallerKind build does not carry its license notice: $carriedLicenses"
}
if ((Get-FileHash -LiteralPath $carriedLicenses).Hash -ne (Get-FileHash -LiteralPath $generatedLicenses).Hash) {
    throw "The license notice the $InstallerKind build carries differs from the generated one: $carriedLicenses"
}
Write-Output "licenses=$carriedLicenses"

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
