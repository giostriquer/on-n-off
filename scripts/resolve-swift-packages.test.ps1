[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# Every case runs the resolver in a disposable package directory against a stub that stands in for
# `xcrun` and fails a set number of times, with no delay between attempts. Nothing here touches
# SwiftPM, the network or the repository's own packages, so it runs on both legs.
$scriptUnderTest = Join-Path $PSScriptRoot "resolve-swift-packages.ps1"
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("on-n-off-swift-resolve-test-" + [guid]::NewGuid().ToString("N"))
$powershell = (Get-Process -Id $PID).Path

# The stub logs its arguments and whether .build existed when it was called. Then it leaves a
# half-fetched .build behind, as an interrupted SwiftPM fetch does, and fails until its budget of
# failures is spent.
$stubTemplate = @'
$log = '@LOG@'
$call = @(Get-Content -LiteralPath $log -ErrorAction Ignore).Count + 1
$build = Join-Path $args[[array]::IndexOf($args, '--package-path') + 1] '.build'
Add-Content -LiteralPath $log -Value "$($args -join ' ') | build=$(Test-Path -LiteralPath $build)"
New-Item -ItemType Directory -Path (Join-Path $build 'checkouts') -Force | Out-Null
if ($call -le @FAILURES@) {
    [Console]::Error.WriteLine("error: simulated fetch failure on call $call")
    exit 1
}
exit 0
'@

function Invoke-Resolve {
    param([string] $Name, [int] $Failures, [switch] $NoManifest)

    $case = Join-Path $fixtureRoot $Name
    $package = Join-Path $case "package"
    $build = Join-Path $package ".build"
    $log = Join-Path $case "calls.log"
    $stub = Join-Path $case "xcrun.ps1"

    # What an earlier resolution left in .build: a first attempt that succeeds must keep it.
    New-Item -ItemType Directory -Path $build -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $build "kept") -Value "from an earlier run"
    if (-not $NoManifest) {
        Set-Content -LiteralPath (Join-Path $package "Package.swift") -Value "// swift-tools-version: 6.2"
    }
    Set-Content -LiteralPath $stub -Value $stubTemplate.Replace("@LOG@", $log.Replace("'", "''")).Replace("@FAILURES@", "$Failures")

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $powershell
    foreach ($argument in @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", $scriptUnderTest,
        "-PackagePath", $package,
        "-Resolver", $stub,
        "-RetryDelaySeconds", "0"
    )) {
        $startInfo.ArgumentList.Add($argument)
    }
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "Failed to start the Swift package resolver"
    }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()

    return [pscustomobject]@{
        ExitCode = $process.ExitCode
        Output   = $stdoutTask.GetAwaiter().GetResult() + $stderrTask.GetAwaiter().GetResult()
        Calls    = @(Get-Content -LiteralPath $log -ErrorAction Ignore)
        Kept     = Test-Path -LiteralPath (Join-Path $build "kept")
    }
}

$failures = @()
$retryFlag = "--disable-dependency-cache"
New-Item -ItemType Directory -Path $fixtureRoot -Force | Out-Null

try {
    # The ordinary run: one plain resolve, which reuses whatever .build already holds.
    $first = Invoke-Resolve -Name "first" -Failures 0
    if ($first.ExitCode -ne 0) {
        $failures += "A first-attempt success should exit 0. Output: $($first.Output)"
    }
    if ($first.Calls.Count -ne 1) {
        $failures += "A first-attempt success should resolve once, got $($first.Calls.Count) calls."
    } elseif ($first.Calls[0] -notmatch '^swift package resolve --package-path .+ \| build=True$' -or
        $first.Calls[0] -match $retryFlag) {
        $failures += "The first attempt should be a plain resolve over the existing .build, got '$($first.Calls[0])'"
    }
    if (-not $first.Kept) {
        $failures += "A first-attempt success must leave the existing .build alone."
    }

    # The regression this script exists for: on 2026-09-22 a Bundle run lost SwiftPM's shared
    # repository cache mid-fetch. A retry has to start from an empty .build and bypass that cache.
    $second = Invoke-Resolve -Name "second" -Failures 1
    if ($second.ExitCode -ne 0) {
        $failures += "One failure followed by a success should exit 0. Output: $($second.Output)"
    }
    if ($second.Calls.Count -ne 2) {
        $failures += "One failure should lead to exactly one retry, got $($second.Calls.Count) calls."
    } elseif ($second.Calls[1] -notmatch "$retryFlag \| build=False$") {
        $failures += "A retry should clear .build and bypass the shared cache, got '$($second.Calls[1])'"
    }
    if ($second.Output -notmatch 'attempt 1 of 3') {
        $failures += "A retry should say which attempt failed. Output: $($second.Output)"
    }

    $third = Invoke-Resolve -Name "third" -Failures 2
    if ($third.ExitCode -ne 0) {
        $failures += "Success on the third attempt should exit 0. Output: $($third.Output)"
    }
    if ($third.Calls.Count -ne 3) {
        $failures += "Two failures should lead to three calls, got $($third.Calls.Count)."
    } else {
        foreach ($call in $third.Calls[1..2]) {
            if ($call -notmatch "$retryFlag \| build=False$") {
                $failures += "Every retry should clear .build and bypass the shared cache, got '$call'"
            }
        }
    }

    # Three failures fail the step, with a message naming the package, and never try a fourth time.
    $exhausted = Invoke-Resolve -Name "exhausted" -Failures 3
    if ($exhausted.ExitCode -eq 0) {
        $failures += "Three failures should fail the step but it exited 0."
    }
    if ($exhausted.Calls.Count -ne 3) {
        $failures += "The resolver should stop after three attempts, got $($exhausted.Calls.Count) calls."
    }
    if ($exhausted.Output -notmatch 'after 3 attempts') {
        $failures += "Expected a failure after 3 attempts. Output: $($exhausted.Output)"
    }

    # A wrong -PackagePath fails at once instead of spending three attempts and the backoff on it.
    $missing = Invoke-Resolve -Name "missing" -Failures 0 -NoManifest
    if ($missing.ExitCode -eq 0) {
        $failures += "A directory without Package.swift should be rejected."
    }
    if ($missing.Calls.Count -ne 0) {
        $failures += "A directory without Package.swift should never reach SwiftPM, got $($missing.Calls.Count) calls."
    }
    if ($missing.Output -notmatch 'Package\.swift') {
        $failures += "Expected the rejection to name Package.swift. Output: $($missing.Output)"
    }
} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction Ignore
}

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) {
        Write-Error $failure -ErrorAction Continue
    }
    exit 1
}

Write-Output "resolve-swift-packages.ps1: all checks passed"
