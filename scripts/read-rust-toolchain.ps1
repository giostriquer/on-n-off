[CmdletBinding()]
param(
    [string]$RepositoryRoot,

    [string]$ToolchainPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    $RepositoryRoot = Split-Path -Parent $PSScriptRoot
}

if ([string]::IsNullOrWhiteSpace($ToolchainPath)) {
    $ToolchainPath = Join-Path $RepositoryRoot "rust-toolchain.toml"
}

if (-not (Test-Path -LiteralPath $ToolchainPath -PathType Leaf)) {
    throw "rust-toolchain.toml is missing at $ToolchainPath"
}

# Deliberately a narrow reader rather than a TOML parser: the workflows only need `channel`, and
# the pin is meaningless unless it is an exact version. Anything looser (a bare `stable`, a date
# channel) would let the rust-cache environment hash drift again, which is the whole reason the
# file exists, so reject it here instead of discovering it as a cold build weeks later.
$channel = ""
foreach ($line in [System.IO.File]::ReadAllLines($ToolchainPath)) {
    $match = [regex]::Match($line, '^\s*channel\s*=\s*"(?<channel>[^"]*)"\s*(#.*)?$')
    if ($match.Success) {
        if (-not [string]::IsNullOrWhiteSpace($channel)) {
            throw "rust-toolchain.toml declares more than one channel"
        }
        $channel = $match.Groups["channel"].Value
    }
}

if ([string]::IsNullOrWhiteSpace($channel)) {
    throw "rust-toolchain.toml does not declare a channel"
}

if ($channel -notmatch '^\d+\.\d+\.\d+$') {
    throw "rust-toolchain.toml must pin an exact version such as 1.98.0, found '$channel'"
}

if ($env:GITHUB_OUTPUT) {
    "channel=$channel" | Out-File -FilePath $env:GITHUB_OUTPUT -Encoding utf8 -Append
}

Write-Output "channel=$channel"
