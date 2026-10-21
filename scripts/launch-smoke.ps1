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
    $targets = @{
        nsis = @{ App = Join-Path $releaseRoot "on-n-off.exe"; WindowClass = "on-n-off Side notch" }
        dmg  = @{ App = Join-Path $releaseRoot "bundle" "macos" "on-n-off.app" "Contents" "MacOS" "on-n-off"; WindowClass = $null }
    }
    $App = $targets[$InstallerKind].App
    $WindowClass = $targets[$InstallerKind].WindowClass
}
if (-not (Test-Path -LiteralPath $App -PathType Leaf)) {
    throw "The app to launch does not exist: $App"
}

if ($WindowClass) {
    Add-Type -Namespace LaunchSmoke -Name Windows -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)]
public static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string className, string title);
[DllImport("user32.dll")]
public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
'@
}

function Test-OwnsWindow {
    param([int] $ProcessId, [string] $ClassName)

    $window = [IntPtr]::Zero
    while (($window = [LaunchSmoke.Windows]::FindWindowEx([IntPtr]::Zero, $window, $ClassName, [NullString]::Value)) -ne [IntPtr]::Zero) {
        $owner = [uint32] 0
        [LaunchSmoke.Windows]::GetWindowThreadProcessId($window, [ref] $owner) | Out-Null
        if ($owner -eq $ProcessId) {
            return $true
        }
    }
    return $false
}

$scratchHome = Join-Path ([System.IO.Path]::GetTempPath()) ("on-n-off-launch-smoke-" + [guid]::NewGuid().ToString("N"))
$settingsDirectory = Join-Path $scratchHome ".on-n-off"
New-Item -ItemType Directory -Path $settingsDirectory -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $PSScriptRoot "fixtures" "launch-smoke-side-notch.json") -Destination (Join-Path $settingsDirectory "side-notch.json")

$startInfo = [System.Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = $App
$startInfo.Arguments = $AppArguments
$startInfo.UseShellExecute = $false
$startInfo.Environment["ON_N_OFF_HOME"] = $scratchHome
$startInfo.Environment["WEBVIEW2_USER_DATA_FOLDER"] = Join-Path $scratchHome "webview2"
$startInfo.Environment["GH_CONFIG_DIR"] = Join-Path $scratchHome "gh"
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
            $code = if ($IsWindows) { "{0} (0x{0:X8})" -f $process.ExitCode } else { "$($process.ExitCode)" }
            throw ("on-n-off exited with code {0} after {1:N1} s with the side notch on. See OS.md, Packaging and updates." -f $code, $clock.Elapsed.TotalSeconds)
        }
        if (-not $windowSeen -and (Test-OwnsWindow -ProcessId $process.Id -ClassName $WindowClass)) {
            $windowSeen = $true
        }
        Start-Sleep -Milliseconds 250
    }
    if (-not $windowSeen) {
        throw "on-n-off kept running for $Seconds s but never opened a '$WindowClass' window, so the side notch path did not run."
    }
    Write-Output ("launched={0} ran {1:N0} s" -f $App, $clock.Elapsed.TotalSeconds)
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
