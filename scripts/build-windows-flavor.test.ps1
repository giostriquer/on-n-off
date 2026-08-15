[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptUnderTest = Join-Path $PSScriptRoot "build-windows-flavor.ps1"
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("on-n-off-flavor-test-" + [guid]::NewGuid().ToString("N"))
$powershell = (Get-Process -Id $PID).Path

function Invoke-Validator {
    param([string] $Kind, [switch] $Signed)
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $powershell
    foreach ($argument in @(
        "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $scriptUnderTest,
        "-InstallerKind", $Kind, "-ReleaseVersion", "0.2.0",
        "-RepositoryRoot", $fixtureRoot, "-ValidateOnly"
    )) {
        $startInfo.ArgumentList.Add($argument)
    }
    if ($Signed) {
        $startInfo.ArgumentList.Add("-SignedUpdater")
    }
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    [void] $process.Start()
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()
    $result = [pscustomobject]@{
        ExitCode = $process.ExitCode
        Output = (@($stdoutTask.GetAwaiter().GetResult().Trim(), $stderrTask.GetAwaiter().GetResult().Trim()) |
            Where-Object { $_ }) -join [Environment]::NewLine
    }
    $process.Dispose()
    $result
}

function Assert-Equal {
    param($Actual, $Expected, [string] $Case)
    if ($Actual -ne $Expected) {
        throw "$Case expected '$Expected', got '$Actual'"
    }
}

try {
    $nsisDirectory = Join-Path $fixtureRoot "src-tauri\target\release\bundle\nsis"
    $msiDirectory = Join-Path $fixtureRoot "src-tauri\target\release\bundle\msi"
    New-Item -ItemType Directory -Path $nsisDirectory, $msiDirectory -Force | Out-Null
    "installer" | Set-Content -LiteralPath (Join-Path $nsisDirectory "on-n-off_0.2.0_x64-setup.exe") -Encoding UTF8
    "signature" | Set-Content -LiteralPath (Join-Path $nsisDirectory "on-n-off_0.2.0_x64-setup.exe.sig") -Encoding UTF8
    "installer" | Set-Content -LiteralPath (Join-Path $msiDirectory "on-n-off_0.2.0_x64_en-US.msi") -Encoding UTF8
    "signature" | Set-Content -LiteralPath (Join-Path $msiDirectory "on-n-off_0.2.0_x64_en-US.msi.sig") -Encoding UTF8

    $validNsis = Invoke-Validator -Kind "nsis" -Signed
    Assert-Equal $validNsis.ExitCode 0 "valid signed NSIS"
    if ($validNsis.Output -notmatch "on-n-off_0.2.0_x64-setup.exe") {
        throw "valid NSIS output did not report its installer: $($validNsis.Output)"
    }

    Remove-Item -LiteralPath (Join-Path $msiDirectory "on-n-off_0.2.0_x64_en-US.msi.sig")
    $missingSignature = Invoke-Validator -Kind "msi" -Signed
    Assert-Equal $missingSignature.ExitCode 1 "missing MSI signature"
    if ($missingSignature.Output -notmatch "\.sig") {
        throw "missing MSI signature error did not identify the signature: $($missingSignature.Output)"
    }

    "other" | Set-Content -LiteralPath (Join-Path $nsisDirectory "other.exe") -Encoding UTF8
    $extraInstaller = Invoke-Validator -Kind "nsis"
    Assert-Equal $extraInstaller.ExitCode 1 "unexpected NSIS installer"
    if ($extraInstaller.Output -notmatch "exactly") {
        throw "unexpected installer error did not explain the exact-set rule: $($extraInstaller.Output)"
    }

    Write-Host "Windows flavor helper tests passed"
}
catch {
    Write-Error $_
    exit 1
}
finally {
    if (Test-Path -LiteralPath $fixtureRoot) {
        $resolvedFixture = (Resolve-Path -LiteralPath $fixtureRoot).Path
        $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd('\')
        if (-not $resolvedFixture.StartsWith($tempRoot + '\', [System.StringComparison]::OrdinalIgnoreCase) -or
            -not ([System.IO.Path]::GetFileName($resolvedFixture)).StartsWith("on-n-off-flavor-test-")) {
            throw "Refusing to remove unexpected fixture path: $resolvedFixture"
        }
        Remove-Item -LiteralPath $resolvedFixture -Recurse -Force
    }
}
