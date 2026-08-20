[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# Every case here runs with -DryRun and a canned toolchain list, so the tests never touch the
# real rustup installation on a development machine or a runner.
$scriptUnderTest = Join-Path $PSScriptRoot "prune-rust-toolchains.ps1"
$powershell = (Get-Process -Id $PID).Path

function Invoke-Prune {
    param([string] $Keep, [string] $ToolchainList)

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $powershell
    foreach ($argument in @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", $scriptUnderTest,
        "-Keep", $Keep,
        "-ToolchainList", $ToolchainList,
        "-DryRun"
    )) {
        $startInfo.ArgumentList.Add($argument)
    }
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "Failed to start the toolchain pruner"
    }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()

    return [pscustomobject]@{
        ExitCode = $process.ExitCode
        Stdout   = $stdoutTask.GetAwaiter().GetResult()
        Stderr   = $stderrTask.GetAwaiter().GetResult()
        Removed  = @([regex]::Matches($stdoutTask.GetAwaiter().GetResult(), '(?m)^removing (.+)$') | ForEach-Object { $_.Groups[1].Value.Trim() })
        Kept     = @([regex]::Matches($stdoutTask.GetAwaiter().GetResult(), '(?m)^keeping (.+)$') | ForEach-Object { $_.Groups[1].Value.Trim() })
    }
}

$failures = @()

# The regression this file exists for: on 2026-08-20 the Windows runner carried the image's own
# stable at 1.97.1 alongside the pinned 1.98.0, and rust-cache hashed both, moving the
# environment key from 15b2903b to b6ac5207 and cold-starting the job the pin was meant to keep
# warm. The image's toolchain has to go, and the pinned one has to stay.
$windows = Invoke-Prune -Keep "1.98.0" -ToolchainList @'
1.98.0-x86_64-pc-windows-msvc
stable-x86_64-pc-windows-msvc (default)
'@
if ($windows.ExitCode -ne 0) {
    $failures += "The Windows runner layout should prune cleanly. Stderr: $($windows.Stderr)"
}
if (($windows.Removed -join ',') -ne "stable-x86_64-pc-windows-msvc") {
    $failures += "Expected only the image's stable to be removed, got '$($windows.Removed -join ',')'"
}
if (($windows.Kept -join ',') -ne "1.98.0-x86_64-pc-windows-msvc") {
    $failures += "Expected the pinned toolchain to be kept, got '$($windows.Kept -join ',')'"
}

# The macOS runner is the same shape on a different host triple, and marks the override rather
# than the default.
$macos = Invoke-Prune -Keep "1.98.0" -ToolchainList @'
1.98.0-aarch64-apple-darwin (override)
stable-aarch64-apple-darwin (default)
'@
if (($macos.Removed -join ',') -ne "stable-aarch64-apple-darwin") {
    $failures += "Expected the macOS image stable to be removed, got '$($macos.Removed -join ',')'"
}

# A version that merely shares a prefix must not be mistaken for the pin.
$prefix = Invoke-Prune -Keep "1.9.0" -ToolchainList @'
1.9.0-x86_64-pc-windows-msvc
1.98.0-x86_64-pc-windows-msvc
'@
if (($prefix.Kept -join ',') -ne "1.9.0-x86_64-pc-windows-msvc") {
    $failures += "1.98.0 must not match a 1.9.0 pin, kept '$($prefix.Kept -join ',')'"
}

# Already pruned: nothing to do, and it must stay quiet rather than fail.
$idempotent = Invoke-Prune -Keep "1.98.0" -ToolchainList "1.98.0-x86_64-pc-windows-msvc (default)"
if ($idempotent.ExitCode -ne 0) {
    $failures += "A already-pruned runner should succeed. Stderr: $($idempotent.Stderr)"
}
if ($idempotent.Removed.Count -ne 0) {
    $failures += "Nothing should be removed when only the pin is installed, got '$($idempotent.Removed -join ',')'"
}

# Never strand the runner without a compiler.
$stranded = Invoke-Prune -Keep "1.98.0" -ToolchainList "stable-x86_64-pc-windows-msvc (default)"
if ($stranded.ExitCode -eq 0) {
    $failures += "Pruning every toolchain should have failed but exited 0."
}
if ($stranded.Stderr -notmatch "no installed toolchain matches") {
    $failures += "Expected a refusal naming the missing pin. Stderr: $($stranded.Stderr)"
}

# A floating Keep would defeat the purpose, so it is rejected like it is in the reader.
$floating = Invoke-Prune -Keep "stable" -ToolchainList "stable-x86_64-pc-windows-msvc"
if ($floating.ExitCode -eq 0) {
    $failures += "A floating -Keep should have been rejected."
}

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) {
        Write-Error $failure -ErrorAction Continue
    }
    exit 1
}

Write-Output "prune-rust-toolchains.ps1: all checks passed"
