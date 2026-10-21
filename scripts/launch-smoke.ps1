[CmdletBinding(DefaultParameterSetName = "Build")]
param(
    [Parameter(Mandatory, ParameterSetName = "Build")] [ValidateSet("nsis", "dmg")] [string] $InstallerKind,
    [Parameter(ParameterSetName = "Build")] [string] $RepositoryRoot = (Split-Path -Parent $PSScriptRoot),
    [Parameter(Mandatory, ParameterSetName = "App")] [string] $App,
    [Parameter(ParameterSetName = "App")] [string] $AppArguments = "",
    [Parameter(ParameterSetName = "App")] [string] $WindowClass,
    [ValidateRange(1, 600)] [int] $Seconds = 20
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($PSCmdlet.ParameterSetName -eq "Build") {
    $releaseRoot = Join-Path $RepositoryRoot "src-tauri" "target" "release"
    if ($InstallerKind -eq "nsis") {
        $App = Join-Path $releaseRoot "on-n-off.exe"
        $WindowClass = "on-n-off Side notch"
    } else {
        $App = Join-Path $releaseRoot "bundle" "macos" "on-n-off.app" "Contents" "MacOS" "on-n-off"
        $WindowClass = $null
    }
}
if (-not (Test-Path -LiteralPath $App -PathType Leaf)) {
    throw "The app to launch does not exist: $App"
}
if ($WindowClass -and -not $IsWindows) {
    throw "-WindowClass is checked only on Windows."
}

if ($WindowClass) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class LaunchSmokeWindows {
    private delegate bool EnumWindowsProc(IntPtr window, IntPtr state);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr state);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetClassName(IntPtr window, StringBuilder name, int capacity);
    public static bool Owns(int processId, string className) {
        bool found = false;
        EnumWindows((window, state) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner == (uint)processId) {
                StringBuilder name = new StringBuilder(256);
                GetClassName(window, name, name.Capacity);
                if (name.ToString() == className) {
                    found = true;
                }
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@
}

$scratchHome = Join-Path ([System.IO.Path]::GetTempPath()) ("on-n-off-launch-smoke-" + [guid]::NewGuid().ToString("N"))
$settingsDirectory = Join-Path $scratchHome ".on-n-off"
New-Item -ItemType Directory -Path $settingsDirectory -Force | Out-Null
[ordered]@{
    enabled      = $true
    edge         = "right"
    size         = "compact"
    show         = "always"
    providers    = @("claude", "codex")
    pullRequests = [ordered]@{ enabled = $true; lists = @("mine") }
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $settingsDirectory "side-notch.json")

$startInfo = [System.Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = $App
$startInfo.Arguments = $AppArguments
$startInfo.UseShellExecute = $false
$startInfo.Environment["ON_N_OFF_HOME"] = $scratchHome
$startInfo.Environment["WEBVIEW2_USER_DATA_FOLDER"] = Join-Path $scratchHome "webview2"
foreach ($token in @("GH_TOKEN", "GITHUB_TOKEN")) {
    $startInfo.Environment.Remove($token) | Out-Null
}

$process = [System.Diagnostics.Process]::new()
$process.StartInfo = $startInfo
$clock = [System.Diagnostics.Stopwatch]::StartNew()
$started = $false
try {
    $started = $process.Start()
    if (-not $started) {
        throw "Failed to start $App"
    }
    $windowSeen = -not $WindowClass
    while ($clock.Elapsed.TotalSeconds -lt $Seconds) {
        if ($process.HasExited) {
            $code = $process.ExitCode
            $hex = "0x{0:X8}" -f [System.BitConverter]::ToUInt32([System.BitConverter]::GetBytes([int] $code), 0)
            throw ("on-n-off exited with code $code ($hex) after {0:N1} s with the side notch on. A crash at launch also stops the in-app updater, so this build must not ship. Run alone on a machine that already runs on-n-off, the single-instance check hands over to that copy and exits." -f $clock.Elapsed.TotalSeconds)
        }
        if (-not $windowSeen -and [LaunchSmokeWindows]::Owns($process.Id, $WindowClass)) {
            $windowSeen = $true
        }
        Start-Sleep -Milliseconds 250
    }
    if (-not $windowSeen) {
        throw "on-n-off kept running for $Seconds s but never opened a '$WindowClass' window, so the side notch path did not run."
    }
    Write-Output ("launched=$App ran {0:N0} s" -f $clock.Elapsed.TotalSeconds)
} finally {
    if ($started -and -not $process.HasExited) {
        $process.Kill($true)
        $process.WaitForExit(10000) | Out-Null
    }
    $process.Dispose()
    for ($attempt = 1; $attempt -le 20 -and (Test-Path -LiteralPath $scratchHome); $attempt++) {
        Remove-Item -LiteralPath $scratchHome -Recurse -Force -ErrorAction SilentlyContinue
        if (Test-Path -LiteralPath $scratchHome) {
            Start-Sleep -Milliseconds 500
        }
    }
}
