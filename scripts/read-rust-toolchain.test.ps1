[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptUnderTest = Join-Path $PSScriptRoot "read-rust-toolchain.ps1"
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("on-n-off-rust-toolchain-test-" + [guid]::NewGuid().ToString("N"))
$powershell = (Get-Process -Id $PID).Path

function Invoke-Reader {
    param([string] $Content, [switch] $Missing)

    $toolchainPath = Join-Path $fixtureRoot "rust-toolchain.toml"
    if ($Missing) {
        if (Test-Path -LiteralPath $toolchainPath) {
            Remove-Item -LiteralPath $toolchainPath
        }
    } else {
        [System.IO.File]::WriteAllText($toolchainPath, $Content, [System.Text.UTF8Encoding]::new($false))
    }

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $powershell
    foreach ($argument in @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", $scriptUnderTest,
        "-ToolchainPath", $toolchainPath
    )) {
        $startInfo.ArgumentList.Add($argument)
    }
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "Failed to start the toolchain reader"
    }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()

    return [pscustomobject]@{
        ExitCode = $process.ExitCode
        Stdout   = $stdoutTask.GetAwaiter().GetResult()
        Stderr   = $stderrTask.GetAwaiter().GetResult()
    }
}

function Assert-Rejected {
    param([string] $Name, [object] $Result, [string] $ExpectedMessage)

    if ($Result.ExitCode -eq 0) {
        throw "$Name should have failed but exited 0. Output: $($Result.Stdout)"
    }
    if ($Result.Stderr -notmatch [regex]::Escape($ExpectedMessage)) {
        throw "$Name did not report '$ExpectedMessage'. Stderr: $($Result.Stderr)"
    }
}

$failures = @()
New-Item -ItemType Directory -Path $fixtureRoot -Force | Out-Null

try {
    $accepted = Invoke-Reader -Content @'
[toolchain]
channel = "1.98.0"
components = ["rustfmt", "clippy"]
profile = "minimal"
'@
    if ($accepted.ExitCode -ne 0) {
        $failures += "A pinned channel should be accepted. Stderr: $($accepted.Stderr)"
    }
    if ($accepted.Stdout.Trim() -ne "channel=1.98.0") {
        $failures += "Expected 'channel=1.98.0', got '$($accepted.Stdout.Trim())'"
    }

    # A trailing comment is ordinary TOML and must not break the reader.
    $commented = Invoke-Reader -Content @'
[toolchain]
channel = "1.98.0" # bump deliberately
'@
    if ($commented.Stdout.Trim() -ne "channel=1.98.0") {
        $failures += "A trailing comment should be tolerated, got '$($commented.Stdout.Trim())'"
    }

    # The whole point of the file is a stable rust-cache environment hash, so a floating
    # channel has to be rejected rather than silently reintroducing the cold-build problem.
    Assert-Rejected -Name "A floating channel" -ExpectedMessage "must pin an exact version" -Result (Invoke-Reader -Content @'
[toolchain]
channel = "stable"
'@)

    Assert-Rejected -Name "A channel-less file" -ExpectedMessage "does not declare a channel" -Result (Invoke-Reader -Content @'
[toolchain]
components = ["clippy"]
'@)

    Assert-Rejected -Name "A duplicated channel" -ExpectedMessage "more than one channel" -Result (Invoke-Reader -Content @'
[toolchain]
channel = "1.98.0"
channel = "1.97.1"
'@)

    Assert-Rejected -Name "A missing file" -ExpectedMessage "is missing at" -Result (Invoke-Reader -Missing)

    # The committed file must satisfy the same contract the workflows rely on.
    $committed = Invoke-Reader -Content ([System.IO.File]::ReadAllText((Join-Path $repositoryRoot "rust-toolchain.toml")))
    if ($committed.ExitCode -ne 0) {
        $failures += "The committed rust-toolchain.toml was rejected. Stderr: $($committed.Stderr)"
    }
} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) {
        Write-Error $failure -ErrorAction Continue
    }
    exit 1
}

Write-Output "read-rust-toolchain.ps1: all checks passed"
