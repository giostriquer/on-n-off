[CmdletBinding()]
param(
    [Parameter(Mandatory)] [ValidateSet("nsis", "msi")] [string] $InstallerKind,
    [Parameter(Mandatory)] [string] $ReleaseVersion,
    [string] $RepositoryRoot = (Split-Path -Parent $PSScriptRoot),
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

$bundleDirectory = Join-Path $RepositoryRoot "src-tauri\target\release\bundle\$InstallerKind"
$expectedName = if ($InstallerKind -eq "nsis") {
    "on-n-off_$ReleaseVersion`_x64-setup.exe"
} else {
    "on-n-off_$ReleaseVersion`_x64_en-US.msi"
}
$installerExtension = if ($InstallerKind -eq "nsis") { ".exe" } else { ".msi" }

if (-not $ValidateOnly) {
    $previousKind = [Environment]::GetEnvironmentVariable("ON_N_OFF_INSTALLER_KIND", "Process")
    Push-Location $RepositoryRoot
    try {
        $env:ON_N_OFF_INSTALLER_KIND = $InstallerKind
        $arguments = @("run", "tauri", "--", "build", "--bundles", $InstallerKind)
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

if (-not (Test-Path -LiteralPath $bundleDirectory -PathType Container)) {
    throw "Expected $InstallerKind bundle directory is missing: $bundleDirectory"
}
$installers = @(Get-ChildItem -LiteralPath $bundleDirectory -File | Where-Object Extension -eq $installerExtension)
if ($installers.Count -ne 1 -or $installers[0].Name -ne $expectedName) {
    $actualNames = ($installers | Select-Object -ExpandProperty Name) -join ", "
    throw "Expected exactly '$expectedName' in '$bundleDirectory'; found: $actualNames"
}

$installerPath = $installers[0].FullName
Write-Output "installer=$installerPath"
if ($SignedUpdater) {
    $signaturePath = "$installerPath.sig"
    if (-not (Test-Path -LiteralPath $signaturePath -PathType Leaf)) {
        throw "Expected updater signature is missing: $signaturePath"
    }
    if ([string]::IsNullOrWhiteSpace([System.IO.File]::ReadAllText($signaturePath))) {
        throw "Updater signature is empty: $signaturePath"
    }
    Write-Output "signature=$signaturePath"
}
