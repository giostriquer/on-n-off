[CmdletBinding()]
param(
    [string]$RepositoryRoot,

    [string]$ExpectedTag
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    $RepositoryRoot = Split-Path -Parent $PSScriptRoot
}

function Read-JsonVersion {
    param(
        [Parameter(Mandatory)]
        [string]$Path,

        [Parameter(Mandatory)]
        [string]$SourceName
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$SourceName is missing at $Path"
    }

    $document = Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json
    $versionProperty = $document.PSObject.Properties["version"]
    $version = if ($null -eq $versionProperty) { "" } else { [string]$versionProperty.Value }
    if ([string]::IsNullOrWhiteSpace($version)) {
        throw "$SourceName does not contain a version"
    }
    return $version
}

function Invoke-CargoMetadata {
    param(
        [Parameter(Mandatory)]
        [string]$ManifestPath
    )

    $cargoExecutable = (Get-Command cargo -CommandType Application -ErrorAction Stop).Source
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $cargoExecutable
    $startInfo.Arguments = "metadata --manifest-path `"$ManifestPath`" --no-deps --format-version 1"
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    try {
        if (-not $process.Start()) {
            throw "Failed to start cargo metadata"
        }

        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()

        if ($process.ExitCode -ne 0) {
            throw "cargo metadata failed: $($stderr.Trim())"
        }

        return $stdout
    }
    finally {
        $process.Dispose()
    }
}

try {
    $root = (Resolve-Path -LiteralPath $RepositoryRoot).Path
    $packagePath = Join-Path $root "package.json"
    $tauriPath = Join-Path $root "src-tauri\tauri.conf.json"
    $cargoPath = Join-Path $root "src-tauri\Cargo.toml"

    $packageVersion = Read-JsonVersion -Path $packagePath -SourceName "package.json"
    $tauriVersion = Read-JsonVersion -Path $tauriPath -SourceName "src-tauri/tauri.conf.json"

    if (-not (Test-Path -LiteralPath $cargoPath -PathType Leaf)) {
        throw "src-tauri/Cargo.toml is missing at $cargoPath"
    }

    $metadata = (Invoke-CargoMetadata -ManifestPath $cargoPath) | ConvertFrom-Json
    $resolvedCargoPath = [System.IO.Path]::GetFullPath($cargoPath)
    $cargoPackage = @($metadata.packages) |
        Where-Object { [System.IO.Path]::GetFullPath([string]$_.manifest_path) -eq $resolvedCargoPath } |
        Select-Object -First 1
    if ($null -eq $cargoPackage) {
        throw "cargo metadata did not return the package from src-tauri/Cargo.toml"
    }

    $cargoVersion = [string]$cargoPackage.version
    $versions = [ordered]@{
        "package.json" = $packageVersion
        "src-tauri/tauri.conf.json" = $tauriVersion
        "src-tauri/Cargo.toml" = $cargoVersion
    }

    $invalidVersions = @($versions.GetEnumerator() | Where-Object { $_.Value -notmatch '^\d+\.\d+\.\d+$' })
    if ($invalidVersions.Count -gt 0) {
        $details = $invalidVersions | ForEach-Object { "$($_.Key)=$($_.Value)" }
        throw "Manifest versions must use X.Y.Z format: $($details -join ', ')"
    }

    $mismatches = @($versions.GetEnumerator() | Where-Object { $_.Value -ne $packageVersion })
    if ($mismatches.Count -gt 0) {
        $details = $versions.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }
        throw "Manifest version mismatch: $($details -join ', ')"
    }

    if (-not [string]::IsNullOrWhiteSpace($ExpectedTag)) {
        if ($ExpectedTag -notmatch '^v\d+\.\d+\.\d+$') {
            throw "Expected tag must use vX.Y.Z format; got '$ExpectedTag'"
        }

        $tagVersion = $ExpectedTag.Substring(1)
        if ($tagVersion -ne $packageVersion) {
            throw "Tag '$ExpectedTag' does not match manifest version '$packageVersion'"
        }
    }

    Write-Output "version=$packageVersion"
    exit 0
}
catch {
    [Console]::Error.WriteLine("release version check failed: $($_.Exception.Message)")
    exit 1
}
