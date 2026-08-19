[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptUnderTest = Join-Path $PSScriptRoot "new-update-feed.ps1"
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("on-n-off-update-feed-test-" + [guid]::NewGuid().ToString("N"))
$assetDirectory = Join-Path $fixtureRoot "assets"
$notesPath = Join-Path $fixtureRoot "notes.md"
$outputPath = Join-Path $fixtureRoot "latest.json"
$powershell = (Get-Process -Id $PID).Path

function Invoke-FeedGenerator {
    param([string] $Tag = "v0.2.0")

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $powershell
    foreach ($argument in @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", $scriptUnderTest,
        "-AssetDirectory", $assetDirectory,
        "-Repository", "giostriquer/on-n-off",
        "-Tag", $Tag,
        "-ReleaseNotesPath", $notesPath,
        "-PublishedAt", "2026-08-15T12:34:56Z",
        "-OutputPath", $outputPath
    )) {
        $startInfo.ArgumentList.Add($argument)
    }
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "Failed to start update feed generator"
    }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()
    $stdout = $stdoutTask.GetAwaiter().GetResult()
    $stderr = $stderrTask.GetAwaiter().GetResult()
    $exitCode = $process.ExitCode
    $process.Dispose()

    [pscustomobject]@{
        ExitCode = $exitCode
        Output = (@($stdout.Trim(), $stderr.Trim()) | Where-Object { $_ }) -join [Environment]::NewLine
    }
}

function Assert-Equal {
    param($Actual, $Expected, [string] $Case)
    if ($Actual -ne $Expected) {
        throw "$Case expected '$Expected', got '$Actual'"
    }
}

try {
    New-Item -ItemType Directory -Path $assetDirectory -Force | Out-Null
    "nsis installer" | Set-Content -LiteralPath (Join-Path $assetDirectory "on-n-off_0.2.0_x64-setup.exe") -Encoding UTF8
    "nsis-signature-value" | Set-Content -LiteralPath (Join-Path $assetDirectory "on-n-off_0.2.0_x64-setup.exe.sig") -Encoding UTF8
    "mac updater bundle" | Set-Content -LiteralPath (Join-Path $assetDirectory "on-n-off_0.2.0_aarch64.app.tar.gz") -Encoding UTF8
    "mac-signature-value" | Set-Content -LiteralPath (Join-Path $assetDirectory "on-n-off_0.2.0_aarch64.app.tar.gz.sig") -Encoding UTF8
    "## Highlights`n`n- Safer updates." | Set-Content -LiteralPath $notesPath -Encoding UTF8

    $valid = Invoke-FeedGenerator
    Assert-Equal $valid.ExitCode 0 "valid feed"
    if (-not (Test-Path -LiteralPath $outputPath -PathType Leaf)) {
        throw "valid feed did not create latest.json"
    }
    $feed = Get-Content -Raw -LiteralPath $outputPath | ConvertFrom-Json -DateKind String
    Assert-Equal $feed.version "0.2.0" "feed version"
    Assert-Equal $feed.pub_date "2026-08-15T12:34:56Z" "publication date"
    Assert-Equal $feed.notes "## Highlights`n`n- Safer updates.`n" "release notes"
    Assert-Equal $feed.platforms.'windows-x86_64-nsis'.signature "nsis-signature-value" "NSIS signature"
    Assert-Equal $feed.platforms.'windows-x86_64-nsis'.url "https://github.com/giostriquer/on-n-off/releases/download/v0.2.0/on-n-off_0.2.0_x64-setup.exe" "NSIS URL"
    Assert-Equal $feed.platforms.'darwin-aarch64'.signature "mac-signature-value" "macOS signature"
    Assert-Equal $feed.platforms.'darwin-aarch64'.url "https://github.com/giostriquer/on-n-off/releases/download/v0.2.0/on-n-off_0.2.0_aarch64.app.tar.gz" "macOS URL"

    Remove-Item -LiteralPath (Join-Path $assetDirectory "on-n-off_0.2.0_aarch64.app.tar.gz.sig")
    $missingMacSignature = Invoke-FeedGenerator
    Assert-Equal $missingMacSignature.ExitCode 1 "missing macOS signature"
    if ($missingMacSignature.Output -notmatch [regex]::Escape("on-n-off_0.2.0_aarch64.app.tar.gz.sig")) {
        throw "missing macOS signature error did not identify the missing file: $($missingMacSignature.Output)"
    }
    "mac-signature-value" | Set-Content -LiteralPath (Join-Path $assetDirectory "on-n-off_0.2.0_aarch64.app.tar.gz.sig") -Encoding UTF8

    Remove-Item -LiteralPath (Join-Path $assetDirectory "on-n-off_0.2.0_x64-setup.exe.sig")
    $missingSignature = Invoke-FeedGenerator
    Assert-Equal $missingSignature.ExitCode 1 "missing NSIS signature"
    if ($missingSignature.Output -notmatch [regex]::Escape("on-n-off_0.2.0_x64-setup.exe.sig")) {
        throw "missing NSIS signature error did not identify the missing file: $($missingSignature.Output)"
    }

    $malformedTag = Invoke-FeedGenerator -Tag "release-0.2.0"
    Assert-Equal $malformedTag.ExitCode 1 "malformed tag"
    if ($malformedTag.Output -notmatch "vX.Y.Z") {
        throw "malformed tag error did not explain the required format: $($malformedTag.Output)"
    }

    Write-Host "update feed tests passed"
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
            -not ([System.IO.Path]::GetFileName($resolvedFixture)).StartsWith("on-n-off-update-feed-test-")) {
            throw "Refusing to remove unexpected fixture path: $resolvedFixture"
        }
        Remove-Item -LiteralPath $resolvedFixture -Recurse -Force
    }
}
