[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Keep,

    # Test seam: canned `rustup toolchain list` output, so the tests never touch a real rustup
    # installation. Left empty in CI, where the real command runs.
    [string]$ToolchainList,

    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($Keep -notmatch '^\d+\.\d+\.\d+$') {
    throw "Keep must be an exact version such as 1.98.0, found '$Keep'"
}

# Swatinem/rust-cache builds the environment portion of its cache key by running
# `rustup toolchain list` and hashing the rustc version of *every* installed toolchain, not just
# the active one. The runner images ship their own `stable`, so pinning the toolchain is not
# enough on its own: the pinned toolchain plus the image's stable is a two-element set, and it
# changes shape every time the image's stable moves, rotating the key exactly as an unpinned
# `stable` did. Reducing the set to the pinned toolchain makes the hash depend only on
# rust-toolchain.toml, which is the whole point of the pin.
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

    # Lines look like `1.98.0-x86_64-pc-windows-msvc (default)` or `stable-aarch64-apple-darwin`.
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
    # The image's stable was very likely rustup's default; make the pin the default so any
    # invocation from outside the repository still resolves to a toolchain that exists.
    rustup default $kept[0]
    if ($LASTEXITCODE -ne 0) {
        throw "Could not set '$($kept[0])' as the default toolchain."
    }
}
