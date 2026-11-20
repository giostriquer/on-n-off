[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptUnderTest = Join-Path $PSScriptRoot "launch-smoke.ps1"
$notchFixture = Join-Path $PSScriptRoot "fixtures" "launch-smoke-side-notch.json"
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("on-n-off-launch-smoke-test-" + [guid]::NewGuid().ToString("N"))
$powershell = (Get-Process -Id $PID).Path
$fakeApp = Join-Path $fixtureRoot "fake-app.ps1"
$crashCode = if ($IsWindows) { "-1073741819 \(0xC0000005\)" } else { "139 " }

function Invoke-SmokeWith {
    param([string[]] $Arguments)

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $powershell
    foreach ($argument in @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $scriptUnderTest) + $Arguments) {
        $startInfo.ArgumentList.Add([string] $argument)
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
    return [pscustomobject]@{
        ExitCode = $process.ExitCode
        Output   = $stdoutTask.GetAwaiter().GetResult() + $stderrTask.GetAwaiter().GetResult()
    }
}

function Invoke-Smoke {
    param([string] $Mode, [int] $Seconds, [string] $WindowClass, [int] $WindowSeconds)

    $dump = Join-Path $fixtureRoot ("dump-" + [guid]::NewGuid().ToString("N") + ".json")
    $arguments = @(
        "-App", $powershell,
        "-AppArguments:-NoProfile -ExecutionPolicy Bypass -File `"$fakeApp`" -Mode $Mode -Dump `"$dump`"",
        "-Seconds", $Seconds
    )
    if ($WindowClass) {
        $arguments += @("-WindowClass", $WindowClass)
    }
    if ($WindowSeconds) {
        $arguments += @("-WindowSeconds", $WindowSeconds)
    }
    $result = Invoke-SmokeWith $arguments
    $seen = $null
    if (Test-Path -LiteralPath $dump) {
        $seen = Get-Content -LiteralPath $dump -Raw | ConvertFrom-Json
    }
    $result | Add-Member -NotePropertyName Seen -NotePropertyValue $seen
    return $result
}

function New-StandInBuild {
    param([string] $Name, [string] $Main)

    $root = Join-Path $fixtureRoot $Name
    $app = Join-Path $root "src-tauri" "target" "release" "on-n-off.exe"
    New-Item -ItemType Directory -Path (Split-Path -Parent $app) -Force | Out-Null
    & powershell.exe -NoProfile -NonInteractive -Command "Add-Type -OutputType ConsoleApplication -OutputAssembly '$app' -TypeDefinition 'public static class StandIn { $Main }'"
    if (-not (Test-Path -LiteralPath $app)) {
        throw "Could not build the stand-in app $Name"
    }
    return [pscustomobject]@{ Root = $root; App = $app }
}

function Get-Plain {
    param([string] $Text)

    ($Text -replace "\e\[[0-9;]*m", "") -replace "\s*\r?\n\s*(\|\s*)?", " "
}

function Assert-Failed {
    param($Result, [string] $Pattern, [string] $Case)

    if ($Result.ExitCode -eq 0) {
        throw "${Case}: the smoke should fail. Output: $($Result.Output)"
    }
    if ((Get-Plain $Result.Output) -notmatch $Pattern) {
        throw "${Case}: the failure should match '$Pattern'. Output: $($Result.Output)"
    }
}

function Assert-Passed {
    param($Result, [string] $Case)

    if ($Result.ExitCode -ne 0) {
        throw "${Case}: the smoke should pass. Output: $($Result.Output)"
    }
}

function Assert-Stopped {
    param($Result, [string] $Case)

    if ($null -eq $Result.Seen) {
        throw "${Case}: the fake app never started"
    }
    $survivor = Get-Process -Id $Result.Seen.pid -ErrorAction SilentlyContinue |
        Where-Object { $_.StartTime.ToFileTimeUtc() -eq $Result.Seen.started }
    if ($survivor) {
        Stop-Process -InputObject $survivor -Force
        throw "${Case}: the smoke left the app running"
    }
    if (Test-Path -LiteralPath $Result.Seen.home) {
        throw "${Case}: the smoke left its scratch home behind: $($Result.Seen.home)"
    }
}

$windowType = @'
[DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
public static extern IntPtr CreateWindowEx(uint exStyle, string className, string title, int style, int x, int y, int width, int height, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);
[DllImport("user32.dll")]
public static extern bool DestroyWindow(IntPtr window);
'@

New-Item -ItemType Directory -Path $fixtureRoot -Force | Out-Null
try {
    Set-Content -LiteralPath $fakeApp -Value (@'
param([string] $Mode, [string] $Dump)
$notch = Join-Path $env:ON_N_OFF_HOME ".on-n-off" "side-notch.json"
[pscustomobject]@{
    pid         = $PID
    started     = (Get-Process -Id $PID).StartTime.ToFileTimeUtc()
    home        = $env:ON_N_OFF_HOME
    notch       = if (Test-Path -LiteralPath $notch) { Get-Content -LiteralPath $notch -Raw } else { $null }
    webview     = $env:WEBVIEW2_USER_DATA_FOLDER
    ghConfig    = $env:GH_CONFIG_DIR
    ghToken     = $env:GH_TOKEN
    githubToken = $env:GITHUB_TOKEN
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $Dump
$crash = if ($IsWindows) { -1073741819 } else { 139 }
function Open-Window {
    Add-Type -Namespace FakeApp -Name Windows -MemberDefinition @"
WINDOW_TYPE
"@
    if ([FakeApp.Windows]::CreateWindowEx(0, "STATIC", "fake app", 0x80000000, 0, 0, 1, 1, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero) -eq [IntPtr]::Zero) {
        exit 3
    }
}
switch ($Mode) {
    "crash" { exit $crash }
    "quit" { exit 0 }
    "window" { Open-Window; Start-Sleep -Seconds 600 }
    "slow" { Start-Sleep -Seconds 5; Open-Window; Start-Sleep -Seconds 600 }
    "slowlate" { Start-Sleep -Seconds 5; Open-Window; Start-Sleep -Seconds 2; exit $crash }
    "dies" { Start-Sleep -Seconds 5; exit $crash }
    "late" { if ($IsWindows) { Open-Window }; Start-Sleep -Seconds 2; exit $crash }
    default { Start-Sleep -Seconds 600 }
}
'@).Replace("WINDOW_TYPE", $windowType)

    $crashed = Invoke-Smoke -Mode "crash" -Seconds 5
    Assert-Failed $crashed "exited with code $crashCode" "an app that crashes at launch"
    Assert-Stopped $crashed "an app that crashes at launch"

    $quit = Invoke-Smoke -Mode "quit" -Seconds 5
    Assert-Failed $quit "exited with code 0 " "an app that quits at launch"
    Assert-Stopped $quit "an app that quits at launch"

    $late = Invoke-Smoke -Mode "late" -Seconds 6 -WindowClass $(if ($IsWindows) { "Static" } else { $null })
    Assert-Failed $late "exited with code $crashCode" "an app that crashes after its window opens"
    Assert-Stopped $late "an app that crashes after its window opens"

    $running = Invoke-Smoke -Mode "stay" -Seconds 3
    Assert-Passed $running "an app that keeps running"
    Assert-Stopped $running "an app that keeps running"
    $seen = $running.Seen
    if ($seen.notch -ne (Get-Content -LiteralPath $notchFixture -Raw)) {
        throw "The app should start with the shared side notch fixture. Saw: $($seen.notch)"
    }
    foreach ($private in @($seen.webview, $seen.ghConfig)) {
        if (-not $private -or -not $private.StartsWith($seen.home)) {
            throw "The WebView2 profile and the gh config should live in the scratch home. Saw: $($seen.webview), $($seen.ghConfig)"
        }
    }
    if ($seen.ghToken -or $seen.githubToken) {
        throw "The app must not inherit a GitHub token"
    }

    $emptyRoot = Join-Path $fixtureRoot "empty-repository"
    New-Item -ItemType Directory -Path $emptyRoot -Force | Out-Null
    foreach ($target in @(
        @{ Kind = "nsis"; Path = Join-Path $emptyRoot "src-tauri" "target" "release" "on-n-off.exe" },
        @{ Kind = "dmg"; Path = Join-Path $emptyRoot "src-tauri" "target" "release" "bundle" "macos" "on-n-off.app" "Contents" "MacOS" "on-n-off" }
    )) {
        $missing = Invoke-SmokeWith @("-InstallerKind", $target.Kind, "-RepositoryRoot", $emptyRoot, "-Seconds", 2)
        Assert-Failed $missing "The app to launch does not exist" "the $($target.Kind) build"
        if (-not ((Get-Plain $missing.Output) -replace "\s", "").Contains(($target.Path -replace "\s", ""))) {
            throw "The $($target.Kind) build should launch $($target.Path). Output: $($missing.Output)"
        }
    }

    if ($IsWindows) {
        $noWindow = Invoke-Smoke -Mode "stay" -Seconds 3 -WindowSeconds 3 -WindowClass "on-n-off launch smoke test class"
        Assert-Failed $noWindow "kept running for 3 s but never opened a 'on-n-off launch smoke test class' window" "an app without the expected window"
        Assert-Stopped $noWindow "an app without the expected window"

        Add-Type -Namespace LaunchSmokeTest -Name Windows -MemberDefinition $windowType
        $foreign = [LaunchSmokeTest.Windows]::CreateWindowEx(0, "STATIC", "another process", 0x80000000, 0, 0, 1, 1, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero)
        if ($foreign -eq [IntPtr]::Zero) {
            throw "Could not open a window for another process"
        }
        try {
            $notOwned = Invoke-Smoke -Mode "stay" -Seconds 3 -WindowSeconds 3 -WindowClass "Static"
            Assert-Failed $notOwned "kept running for 3 s but never opened a 'Static' window" "a window of another process"
            Assert-Stopped $notOwned "a window of another process"
        } finally {
            [LaunchSmokeTest.Windows]::DestroyWindow($foreign) | Out-Null
        }

        $window = Invoke-Smoke -Mode "window" -Seconds 10 -WindowClass "Static"
        Assert-Passed $window "an app that opens the expected window"
        Assert-Stopped $window "an app that opens the expected window"

        $slow = Invoke-Smoke -Mode "slow" -Seconds 2 -WindowSeconds 90 -WindowClass "Static"
        Assert-Passed $slow "an app whose window opens after the run time, within the window wait"
        Assert-Stopped $slow "an app whose window opens after the run time, within the window wait"
        if ((Get-Plain $slow.Output) -notmatch "ran (\d+) s, 'Static' window after (\d+)\.\d s") {
            throw "The smoke should say how long it ran and when the window opened. Output: $($slow.Output)"
        }
        $ran = [int] $Matches[1]
        $openedAfter = [int] $Matches[2]
        if ($openedAfter -lt 5) {
            throw "The smoke should say the window opened no sooner than the app opened it. Output: $($slow.Output)"
        }
        if ($ran -lt $openedAfter + 2 -or $ran -gt $openedAfter + 10) {
            throw "The smoke should end 2 s after the window opened, not at the end of the window wait. Output: $($slow.Output)"
        }

        $lateCrash = Invoke-Smoke -Mode "slowlate" -Seconds 3 -WindowSeconds 90 -WindowClass "Static"
        Assert-Failed $lateCrash "exited with code $crashCode" "an app that crashes after a window that opened past the run time"
        Assert-Stopped $lateCrash "an app that crashes after a window that opened past the run time"

        $unopened = Invoke-Smoke -Mode "stay" -Seconds 2 -WindowSeconds 6 -WindowClass "on-n-off launch smoke test class"
        Assert-Failed $unopened "kept running for 6 s but never opened a 'on-n-off launch smoke test class' window" "an app without the expected window by the end of the window wait"
        Assert-Stopped $unopened "an app without the expected window by the end of the window wait"

        $died = Invoke-Smoke -Mode "dies" -Seconds 2 -WindowSeconds 90 -WindowClass "on-n-off launch smoke test class"
        Assert-Failed $died "exited with code $crashCode" "an app that crashes while its window is awaited"
        Assert-Stopped $died "an app that crashes while its window is awaited"

        $standIn = New-StandInBuild -Name "stand-in-repository" -Main "public static void Main() { System.Threading.Thread.Sleep(600000); }"
        $withoutNotch = Invoke-SmokeWith @("-InstallerKind", "nsis", "-RepositoryRoot", $standIn.Root, "-Seconds", 2, "-WindowSeconds", 4)
        Assert-Failed $withoutNotch "kept running for 4 s but never opened a 'on-n-off Side notch' window" "an nsis build without the notch window"
        if (Get-Process | Where-Object { $_.Path -eq $standIn.App }) {
            throw "The smoke left the nsis stand-in running"
        }

        $shortLived = New-StandInBuild -Name "short-lived-repository" -Main "public static int Main() { System.Threading.Thread.Sleep(6000); return 7; }"
        $stillAwaited = Invoke-SmokeWith @("-InstallerKind", "nsis", "-RepositoryRoot", $shortLived.Root, "-Seconds", 2)
        Assert-Failed $stillAwaited "exited with code 7 \(0x00000007\)" "an nsis build, whose notch window is awaited past its run time"
    }

    Write-Output "launch-smoke tests passed"
} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}
