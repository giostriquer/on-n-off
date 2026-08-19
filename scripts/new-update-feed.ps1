[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $AssetDirectory,
    [Parameter(Mandatory)] [string] $Repository,
    [Parameter(Mandatory)] [string] $Tag,
    [Parameter(Mandatory)] [string] $ReleaseNotesPath,
    [Parameter(Mandatory)] [string] $PublishedAt,
    [Parameter(Mandatory)] [string] $OutputPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($Tag -notmatch '^v(?<version>(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*))$') {
    throw "Release tag '$Tag' must use vX.Y.Z with numeric SemVer components."
}
$version = $Matches.version

if ($Repository -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') {
    throw "Repository '$Repository' must use owner/name format."
}

$publicationDate = [DateTimeOffset]::MinValue
if (-not [DateTimeOffset]::TryParse(
    $PublishedAt,
    [System.Globalization.CultureInfo]::InvariantCulture,
    [System.Globalization.DateTimeStyles]::AssumeUniversal,
    [ref] $publicationDate
)) {
    throw "PublishedAt '$PublishedAt' must be an RFC-3339 timestamp."
}
$normalizedPublicationDate = $publicationDate.ToUniversalTime().ToString(
    "yyyy-MM-dd'T'HH:mm:ss'Z'",
    [System.Globalization.CultureInfo]::InvariantCulture
)

if (-not (Test-Path -LiteralPath $AssetDirectory -PathType Container)) {
    throw "Asset directory does not exist: $AssetDirectory"
}
if (-not (Test-Path -LiteralPath $ReleaseNotesPath -PathType Leaf)) {
    throw "Release notes file does not exist: $ReleaseNotesPath"
}

$nsisName = "on-n-off_$version`_x64-setup.exe"
$macName = "on-n-off_$version`_aarch64.app.tar.gz"
$expectedNames = @($nsisName, "$nsisName.sig", $macName, "$macName.sig")
foreach ($name in $expectedNames) {
    $path = Join-Path $AssetDirectory $name
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Expected updater asset is missing: $name"
    }
}

$nsisSignature = [System.IO.File]::ReadAllText((Join-Path $AssetDirectory "$nsisName.sig")).Trim()
$macSignature = [System.IO.File]::ReadAllText((Join-Path $AssetDirectory "$macName.sig")).Trim()
if ([string]::IsNullOrWhiteSpace($nsisSignature)) {
    throw "Updater signature is empty: $nsisName.sig"
}
if ([string]::IsNullOrWhiteSpace($macSignature)) {
    throw "Updater signature is empty: $macName.sig"
}

$notes = [System.IO.File]::ReadAllText($ReleaseNotesPath) -replace "`r`n?", "`n"
$downloadRoot = "https://github.com/$Repository/releases/download/$Tag"
$feed = [ordered]@{
    version = $version
    notes = $notes
    pub_date = $normalizedPublicationDate
    platforms = [ordered]@{
        'windows-x86_64-nsis' = [ordered]@{
            url = "$downloadRoot/$nsisName"
            signature = $nsisSignature
        }
        'darwin-aarch64' = [ordered]@{
            url = "$downloadRoot/$macName"
            signature = $macSignature
        }
    }
}

$outputParent = Split-Path -Parent $OutputPath
if ($outputParent) {
    New-Item -ItemType Directory -Path $outputParent -Force | Out-Null
}
$json = $feed | ConvertTo-Json -Depth 6
[System.IO.File]::WriteAllText($OutputPath, $json + "`n", [System.Text.UTF8Encoding]::new($false))
Write-Output "feed=$OutputPath"
