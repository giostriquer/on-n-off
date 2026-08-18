[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptUnderTest = Join-Path $PSScriptRoot "check-release-version.ps1"
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("on-n-off-release-version-test-" + [guid]::NewGuid().ToString("N"))
$powershell = (Get-Process -Id $PID).Path

function Set-FixtureVersion {
    param(
        [Parameter(Mandatory)]
        [string]$PackageVersion,

        [Parameter(Mandatory)]
        [string]$TauriVersion,

        [Parameter(Mandatory)]
        [string]$CargoVersion
    )

    @{ version = $PackageVersion } |
        ConvertTo-Json |
        Set-Content -LiteralPath (Join-Path $fixtureRoot "package.json") -Encoding UTF8

    @{ version = $TauriVersion } |
        ConvertTo-Json |
        Set-Content -LiteralPath (Join-Path $fixtureRoot "src-tauri" "tauri.conf.json") -Encoding UTF8

    @"
[package]
name = "release-version-fixture"
version = "$CargoVersion"
edition = "2021"
"@ | Set-Content -LiteralPath (Join-Path $fixtureRoot "src-tauri" "Cargo.toml") -Encoding UTF8
}

function Invoke-Checker {
    param(
        [Parameter(Mandatory)]
        [string]$ExpectedTag,

        [switch]$UseDefaultRepositoryRoot
    )

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $powershell
    $rootArgument = if ($UseDefaultRepositoryRoot) { "" } else { " -RepositoryRoot `"$fixtureRoot`"" }
    $startInfo.Arguments = "-NoProfile -ExecutionPolicy Bypass -File `"$scriptUnderTest`"$rootArgument -ExpectedTag `"$ExpectedTag`""
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "Failed to start version checker process"
    }

    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()
    $stdout = $stdoutTask.GetAwaiter().GetResult()
    $stderr = $stderrTask.GetAwaiter().GetResult()
    $exitCode = $process.ExitCode
    $process.Dispose()
    $output = @($stdout.Trim(), $stderr.Trim()) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }

    [pscustomobject]@{
        ExitCode = $exitCode
        Output = ($output -join [Environment]::NewLine)
    }
}

function Assert-ExitCode {
    param(
        [Parameter(Mandatory)]
        $Result,

        [Parameter(Mandatory)]
        [int]$Expected,

        [Parameter(Mandatory)]
        [string]$Case
    )

    if ($Result.ExitCode -ne $Expected) {
        throw "$Case expected exit code $Expected, got $($Result.ExitCode). Output: $($Result.Output)"
    }
}

function Assert-OutputContains {
    param(
        [Parameter(Mandatory)]
        $Result,

        [Parameter(Mandatory)]
        [string]$ExpectedText,

        [Parameter(Mandatory)]
        [string]$Case
    )

    if ($Result.Output -notmatch [regex]::Escape($ExpectedText)) {
        throw "$Case expected output containing '$ExpectedText'. Output: $($Result.Output)"
    }
}

try {
    New-Item -ItemType Directory -Path (Join-Path $fixtureRoot "src-tauri" "src") -Force | Out-Null
    "fn main() {}" | Set-Content -LiteralPath (Join-Path $fixtureRoot "src-tauri" "src" "main.rs") -Encoding UTF8

    Set-FixtureVersion -PackageVersion "0.1.0" -TauriVersion "0.1.0" -CargoVersion "0.1.0"
    $matching = Invoke-Checker -ExpectedTag "v0.1.0"
    Assert-ExitCode -Result $matching -Expected 0 -Case "matching versions"
    Assert-OutputContains -Result $matching -ExpectedText "version=0.1.0" -Case "matching versions"

    $repositoryVersion = [string]((Get-Content -Raw -LiteralPath (Join-Path (Split-Path -Parent $PSScriptRoot) "package.json") | ConvertFrom-Json).version)
    $defaultRoot = Invoke-Checker -ExpectedTag "v$repositoryVersion" -UseDefaultRepositoryRoot
    Assert-ExitCode -Result $defaultRoot -Expected 0 -Case "default repository root"
    Assert-OutputContains -Result $defaultRoot -ExpectedText "version=$repositoryVersion" -Case "default repository root"

    $malformedTag = Invoke-Checker -ExpectedTag "release-0.1.0"
    Assert-ExitCode -Result $malformedTag -Expected 1 -Case "malformed tag"
    Assert-OutputContains -Result $malformedTag -ExpectedText "vX.Y.Z" -Case "malformed tag"

    $wrongTag = Invoke-Checker -ExpectedTag "v0.2.0"
    Assert-ExitCode -Result $wrongTag -Expected 1 -Case "tag mismatch"
    Assert-OutputContains -Result $wrongTag -ExpectedText "does not match" -Case "tag mismatch"

    Set-FixtureVersion -PackageVersion "0.2.0" -TauriVersion "0.1.0" -CargoVersion "0.1.0"
    $manifestMismatch = Invoke-Checker -ExpectedTag "v0.1.0"
    Assert-ExitCode -Result $manifestMismatch -Expected 1 -Case "manifest mismatch"
    Assert-OutputContains -Result $manifestMismatch -ExpectedText "package.json" -Case "manifest mismatch"

    '{}' | Set-Content -LiteralPath (Join-Path $fixtureRoot "package.json") -Encoding UTF8
    $missingJsonVersion = Invoke-Checker -ExpectedTag "v0.1.0"
    Assert-ExitCode -Result $missingJsonVersion -Expected 1 -Case "missing JSON version"
    Assert-OutputContains -Result $missingJsonVersion -ExpectedText "package.json does not contain a version" -Case "missing JSON version"

    Write-Host "release version contract tests passed"
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
            -not ([System.IO.Path]::GetFileName($resolvedFixture)).StartsWith("on-n-off-release-version-test-")) {
            throw "Refusing to remove unexpected fixture path: $resolvedFixture"
        }
        Remove-Item -LiteralPath $resolvedFixture -Recurse -Force
    }
}
