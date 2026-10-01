[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Keep,

    [string]$ToolchainList,

    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($Keep -notmatch '^\d+\.\d+\.\d+$') {
    throw "Keep must be an exact version such as 1.98.0, found '$Keep'"
}

if ([string]::IsNullOrWhiteSpace($ToolchainList)) {
    $ToolchainList = (rustup toolchain list | Out-String)
    if ($LASTEXITCODE -ne 0) {
        throw "Could not list rustup toolchains."
    }
}

$kept = @()
$removed = @()

foreach ($line in $ToolchainList -split '[\r\n]+') {
    $trimmed = $line.Trim()
    if ([string]::IsNullOrWhiteSpace($trimmed)) {
        continue
    }

    $name = ($trimmed -split '\s+')[0]
    if ([string]::IsNullOrWhiteSpace($name)) {
        continue
    }

    if ($name -eq $Keep -or $name.StartsWith("$Keep-")) {
        $kept += $name
        continue
    }

    $removed += $name
}

if ($kept.Count -eq 0) {
    throw "Refusing to prune: no installed toolchain matches the pinned version '$Keep'."
}

foreach ($name in $removed) {
    Write-Output "removing $name"
    if (-not $DryRun) {
        rustup toolchain uninstall $name
        if ($LASTEXITCODE -ne 0) {
            throw "Could not uninstall toolchain '$name'."
        }
    }
}

foreach ($name in $kept) {
    Write-Output "keeping $name"
}

if (-not $DryRun -and $removed.Count -gt 0) {
    rustup default $kept[0]
    if ($LASTEXITCODE -ne 0) {
        throw "Could not set '$($kept[0])' as the default toolchain."
    }
}
