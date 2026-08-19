[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptUnderTest = Join-Path $PSScriptRoot "build-bundle.ps1"
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("on-n-off-bundle-test-" + [guid]::NewGuid().ToString("N"))
$powershell = (Get-Process -Id $PID).Path

function Invoke-Validator {
    param([string] $Kind, [switch] $Signed, [string] $StageDirectory)
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
    if ($StageDirectory) {
        $startInfo.ArgumentList.Add("-StageDirectory")
        $startInfo.ArgumentList.Add($StageDirectory)
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

function Assert-Match {
    param([string] $Output, [string] $Pattern, [string] $Case)
    if ($Output -notmatch $Pattern) {
        throw "$Case output did not match '$Pattern': $Output"
    }
}

try {
    $bundleRoot = Join-Path $fixtureRoot "src-tauri" "target" "release" "bundle"
    $nsisDirectory = Join-Path $bundleRoot "nsis"
    $dmgDirectory = Join-Path $bundleRoot "dmg"
    $macosDirectory = Join-Path $bundleRoot "macos"
    New-Item -ItemType Directory -Path $nsisDirectory, $dmgDirectory, $macosDirectory -Force | Out-Null
    "installer" | Set-Content -LiteralPath (Join-Path $nsisDirectory "on-n-off_0.2.0_x64-setup.exe") -Encoding UTF8
    "signature" | Set-Content -LiteralPath (Join-Path $nsisDirectory "on-n-off_0.2.0_x64-setup.exe.sig") -Encoding UTF8
    "disk image" | Set-Content -LiteralPath (Join-Path $dmgDirectory "on-n-off_0.2.0_aarch64.dmg") -Encoding UTF8
    "updater bundle" | Set-Content -LiteralPath (Join-Path $macosDirectory "on-n-off.app.tar.gz") -Encoding UTF8
    "signature" | Set-Content -LiteralPath (Join-Path $macosDirectory "on-n-off.app.tar.gz.sig") -Encoding UTF8

    $validNsis = Invoke-Validator -Kind "nsis" -Signed
    Assert-Equal $validNsis.ExitCode 0 "valid signed NSIS"
    Assert-Match $validNsis.Output "on-n-off_0.2.0_x64-setup.exe" "valid NSIS"

    $validDmg = Invoke-Validator -Kind "dmg" -Signed
    Assert-Equal $validDmg.ExitCode 0 "valid signed DMG"
    Assert-Match $validDmg.Output "installer=.*on-n-off_0.2.0_aarch64.dmg" "valid DMG installer"
    Assert-Match $validDmg.Output "updater=.*on-n-off.app.tar.gz" "valid DMG updater"
    Assert-Match $validDmg.Output "signature=.*on-n-off.app.tar.gz.sig" "valid DMG signature"

    $stageDirectory = Join-Path $fixtureRoot "stage"
    $stagedNsis = Invoke-Validator -Kind "nsis" -Signed -StageDirectory $stageDirectory
    Assert-Equal $stagedNsis.ExitCode 0 "staged NSIS"
    $stagedDmg = Invoke-Validator -Kind "dmg" -Signed -StageDirectory $stageDirectory
    Assert-Equal $stagedDmg.ExitCode 0 "staged DMG"
    $stagedNames = @(Get-ChildItem -LiteralPath $stageDirectory -File | Select-Object -ExpandProperty Name | Sort-Object)
    $expectedStaged = @(
        "on-n-off_0.2.0_aarch64.app.tar.gz",
        "on-n-off_0.2.0_aarch64.app.tar.gz.sig",
        "on-n-off_0.2.0_aarch64.dmg",
        "on-n-off_0.2.0_x64-setup.exe",
        "on-n-off_0.2.0_x64-setup.exe.sig"
    ) | Sort-Object
    if (Compare-Object $expectedStaged $stagedNames) {
        throw "staged asset set mismatch: $($stagedNames -join ', ')"
    }

    Remove-Item -LiteralPath (Join-Path $nsisDirectory "on-n-off_0.2.0_x64-setup.exe.sig")
    $missingSignature = Invoke-Validator -Kind "nsis" -Signed
    Assert-Equal $missingSignature.ExitCode 1 "missing NSIS signature"
    Assert-Match $missingSignature.Output "\.sig" "missing NSIS signature"

    Remove-Item -LiteralPath (Join-Path $macosDirectory "on-n-off.app.tar.gz")
    $missingUpdater = Invoke-Validator -Kind "dmg" -Signed
    Assert-Equal $missingUpdater.ExitCode 1 "missing DMG updater bundle"
    Assert-Match $missingUpdater.Output "on-n-off.app.tar.gz" "missing DMG updater bundle"

    $unsignedDmg = Invoke-Validator -Kind "dmg"
    Assert-Equal $unsignedDmg.ExitCode 0 "unsigned DMG ignores updater artifacts"

    "other" | Set-Content -LiteralPath (Join-Path $nsisDirectory "other.exe") -Encoding UTF8
    $extraInstaller = Invoke-Validator -Kind "nsis"
    Assert-Equal $extraInstaller.ExitCode 1 "unexpected NSIS installer"
    Assert-Match $extraInstaller.Output "exactly" "unexpected installer"

    Write-Host "bundle helper tests passed"
}
catch {
    Write-Error $_
    exit 1
}
finally {
    if (Test-Path -LiteralPath $fixtureRoot) {
        $resolvedFixture = (Resolve-Path -LiteralPath $fixtureRoot).Path
        $separator = [System.IO.Path]::DirectorySeparatorChar
        $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd($separator)
        if (-not $resolvedFixture.StartsWith($tempRoot + $separator, [System.StringComparison]::OrdinalIgnoreCase) -or
            -not ([System.IO.Path]::GetFileName($resolvedFixture)).StartsWith("on-n-off-bundle-test-")) {
            throw "Refusing to remove unexpected fixture path: $resolvedFixture"
        }
        Remove-Item -LiteralPath $resolvedFixture -Recurse -Force
    }
}
