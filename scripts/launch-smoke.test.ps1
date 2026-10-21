[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptUnderTest = Join-Path $PSScriptRoot "launch-smoke.ps1"
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("on-n-off-launch-smoke-test-" + [guid]::NewGuid().ToString("N"))
$powershell = (Get-Process -Id $PID).Path
$fakeApp = Join-Path $fixtureRoot "fake-app.ps1"

function Invoke-Smoke {
    param([string] $Mode, [int] $Seconds, [string] $WindowClass)

    $dump = Join-Path $fixtureRoot ("dump-" + [guid]::NewGuid().ToString("N") + ".json")
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $powershell
    foreach ($argument in @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", $scriptUnderTest,
        "-App", $powershell,
        "-AppArguments:-NoProfile -ExecutionPolicy Bypass -File `"$fakeApp`" -Mode $Mode -Dump `"$dump`"",
        "-Seconds", $Seconds
    )) {
        $startInfo.ArgumentList.Add([string] $argument)
    }
    if ($WindowClass) {
        $startInfo.ArgumentList.Add("-WindowClass")
        $startInfo.ArgumentList.Add($WindowClass)
    }
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.Environment["GH_TOKEN"] = "launch-smoke-test-token"
    $startInfo.Environment["GITHUB_TOKEN"] = "launch-smoke-test-token"

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "Failed to start the launch smoke"
    }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(120000)) {
        $process.Kill($true)
        throw "The launch smoke did not finish within two minutes"
    }

    $seen = $null
    if (Test-Path -LiteralPath $dump) {
        $seen = Get-Content -LiteralPath $dump -Raw | ConvertFrom-Json
    }
    return [pscustomobject]@{
        ExitCode = $process.ExitCode
        Output   = $stdoutTask.GetAwaiter().GetResult() + $stderrTask.GetAwaiter().GetResult()
        Seen     = $seen
    }
}

function Assert-Failed {
    param($Result, [string] $Pattern, [string] $Case)

    if ($Result.ExitCode -eq 0) {
        throw "${Case}: the smoke should fail. Output: $($Result.Output)"
    }
    if ($Result.Output -notmatch $Pattern) {
        throw "${Case}: the failure should match '$Pattern'. Output: $($Result.Output)"
    }
}

function Assert-Stopped {
    param($Result, [string] $Case)

    if ($null -eq $Result.Seen) {
        throw "${Case}: the fake app never started"
    }
    if (Get-Process -Id $Result.Seen.pid -ErrorAction SilentlyContinue) {
        Stop-Process -Id $Result.Seen.pid -Force
        throw "${Case}: the smoke left the app running"
    }
    if (Test-Path -LiteralPath $Result.Seen.home) {
        throw "${Case}: the smoke left its scratch home behind: $($Result.Seen.home)"
    }
}

New-Item -ItemType Directory -Path $fixtureRoot -Force | Out-Null
try {
    Set-Content -LiteralPath $fakeApp -Value @'
param([string] $Mode, [string] $Dump)
$notch = Join-Path $env:ON_N_OFF_HOME ".on-n-off" "side-notch.json"
[pscustomobject]@{
    pid          = $PID
    home         = $env:ON_N_OFF_HOME
    notch        = if (Test-Path -LiteralPath $notch) { Get-Content -LiteralPath $notch -Raw | ConvertFrom-Json } else { $null }
    webview      = $env:WEBVIEW2_USER_DATA_FOLDER
    ghToken      = $env:GH_TOKEN
    githubToken  = $env:GITHUB_TOKEN
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $Dump
switch ($Mode) {
    "crash" { exit -1073741819 }
    "quit" { exit 0 }
    "window" {
        Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class FakeAppWindow {
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr CreateWindowEx(uint exStyle, string className, string title, int style, int x, int y, int width, int height, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);
}
"@
        $window = [FakeAppWindow]::CreateWindowEx(0, "STATIC", "fake app", 0x80000000, 0, 0, 1, 1, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero)
        if ($window -eq [IntPtr]::Zero) { exit 3 }
        Start-Sleep -Seconds 600
    }
    default { Start-Sleep -Seconds 600 }
}
'@

    $crashed = Invoke-Smoke -Mode "crash" -Seconds 5
    Assert-Failed $crashed "0xC0000005" "an app that crashes at launch"
    Assert-Stopped $crashed "an app that crashes at launch"

    $quit = Invoke-Smoke -Mode "quit" -Seconds 5
    Assert-Failed $quit "exited with code 0 " "an app that quits at launch"
    Assert-Stopped $quit "an app that quits at launch"

    $running = Invoke-Smoke -Mode "stay" -Seconds 3
    if ($running.ExitCode -ne 0) {
        throw "An app that keeps running should pass. Output: $($running.Output)"
    }
    Assert-Stopped $running "an app that keeps running"
    $seen = $running.Seen
    if (-not $seen.notch.enabled -or $seen.notch.show -ne "always" -or -not $seen.notch.pullRequests.enabled) {
        throw "The app should start with the side notch on. Saw: $($seen.notch | ConvertTo-Json -Depth 5)"
    }
    if (@($seen.notch.providers) -join "," -ne "claude,codex") {
        throw "The notch should show both providers. Saw: $(@($seen.notch.providers) -join ',')"
    }
    if (-not $seen.webview -or -not $seen.webview.StartsWith($seen.home)) {
        throw "The WebView2 profile should live in the scratch home. Saw: $($seen.webview)"
    }
    if ($seen.ghToken -or $seen.githubToken) {
        throw "The app must not inherit a GitHub token"
    }

    if ($IsWindows) {
        $noWindow = Invoke-Smoke -Mode "stay" -Seconds 3 -WindowClass "on-n-off launch smoke test class"
        Assert-Failed $noWindow "on-n-off launch smoke test class" "an app without the expected window"
        Assert-Stopped $noWindow "an app without the expected window"

        $window = Invoke-Smoke -Mode "window" -Seconds 10 -WindowClass "Static"
        if ($window.ExitCode -ne 0) {
            throw "An app that opens the expected window should pass. Output: $($window.Output)"
        }
        Assert-Stopped $window "an app that opens the expected window"
    }

    Write-Output "launch-smoke tests passed"
} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}
