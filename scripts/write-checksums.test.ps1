[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptUnderTest = Join-Path $PSScriptRoot "write-checksums.ps1"
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("on-n-off-checksums-test-" + [guid]::NewGuid().ToString("N"))
$powershell = (Get-Process -Id $PID).Path

function Invoke-Writer {
    param([string] $Directory, [string] $OutputPath)

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $powershell
    foreach ($argument in @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", $scriptUnderTest,
        "-Directory", $Directory,
        "-OutputPath", $OutputPath
    )) {
        $startInfo.ArgumentList.Add($argument)
    }
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "Failed to start the checksum writer"
    }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()

    return [pscustomobject]@{
        ExitCode = $process.ExitCode
        Stdout   = $stdoutTask.GetAwaiter().GetResult()
        Stderr   = $stderrTask.GetAwaiter().GetResult()
    }
}

function Invoke-WriterFrom {
    param([string] $Location)

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $powershell
    $startInfo.WorkingDirectory = $fixtureRoot
    foreach ($argument in @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-Command", "Set-Location -LiteralPath '$Location'; & '$scriptUnderTest' -Directory . -OutputPath SHA256SUMS.txt; exit `$LASTEXITCODE"
    )) {
        $startInfo.ArgumentList.Add($argument)
    }
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "Failed to start the checksum writer"
    }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()

    return [pscustomobject]@{
        ExitCode = $process.ExitCode
        Stdout   = $stdoutTask.GetAwaiter().GetResult()
        Stderr   = $stderrTask.GetAwaiter().GetResult()
    }
}

function Get-Sha256 {
    param([string] $Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

$failures = @()

try {
    $assets = Join-Path $fixtureRoot "assets"
    New-Item -ItemType Directory -Path $assets -Force | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $assets "on-n-off_1.2.3_x64-setup.exe"), "installer", [System.Text.UTF8Encoding]::new($false))
    [System.IO.File]::WriteAllText((Join-Path $assets "latest.json"), "{}", [System.Text.UTF8Encoding]::new($false))
    New-Item -ItemType Directory -Path (Join-Path $assets "nested") -Force | Out-Null
    $sumsPath = Join-Path $assets "SHA256SUMS.txt"

    $written = Invoke-Writer -Directory $assets -OutputPath $sumsPath
    if ($written.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $sumsPath -PathType Leaf)) {
        throw "Writing checksums should succeed. Stderr: $($written.Stderr)"
    }

    $bytes = [System.IO.File]::ReadAllBytes($sumsPath)
    if ($bytes -contains 13) {
        $failures += "SHA256SUMS.txt must end its lines with LF alone, so shasum -c on macOS and Linux reads it"
    }
    if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
        $failures += "SHA256SUMS.txt must not start with a byte order mark"
    }

    $expected = (@(
        "$(Get-Sha256 (Join-Path $assets 'latest.json'))  latest.json",
        "$(Get-Sha256 (Join-Path $assets 'on-n-off_1.2.3_x64-setup.exe'))  on-n-off_1.2.3_x64-setup.exe"
    ) -join "`n") + "`n"
    $actual = [System.Text.Encoding]::UTF8.GetString($bytes)
    if ($actual -cne $expected) {
        $failures += "Expected one '<sha256>  <name>' line per asset, sorted, each ending in LF. Got: $actual"
    }

    $rerun = Invoke-Writer -Directory $assets -OutputPath $sumsPath
    $rerunText = [System.IO.File]::ReadAllText($sumsPath)
    if ($rerun.ExitCode -ne 0 -or $rerunText -match "SHA256SUMS\.txt") {
        $failures += "A second run must not list the checksum file itself. Got: $rerunText"
    }
    if ($rerunText -cne $actual) {
        $failures += "A second run must write the same bytes as the first. Got: $rerunText"
    }

    $shasum = Get-Command shasum -ErrorAction SilentlyContinue
    if ($null -eq $shasum) {
        Write-Output "shasum is not installed here; skipped the shasum -a 256 -c check"
    } else {
        Push-Location $assets
        try {
            $verify = & $shasum.Source -a 256 -c "SHA256SUMS.txt" 2>&1
            if ($LASTEXITCODE -ne 0) {
                $failures += "shasum -a 256 -c rejected the written file: $verify"
            }
        } finally {
            Pop-Location
        }
    }

    $relative = Join-Path $fixtureRoot "relative"
    New-Item -ItemType Directory -Path $relative -Force | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $relative "on-n-off_1.2.3_aarch64.dmg"), "dmg", [System.Text.UTF8Encoding]::new($false))
    $fromLocation = Invoke-WriterFrom -Location $relative
    if ($fromLocation.ExitCode -ne 0) {
        $failures += "A relative output path should succeed. Stderr: $($fromLocation.Stderr)"
    }
    if (-not (Test-Path -LiteralPath (Join-Path $relative "SHA256SUMS.txt") -PathType Leaf)) {
        $failures += "A relative -OutputPath must resolve against PowerShell's location, the asset folder"
    }
    if (Test-Path -LiteralPath (Join-Path $fixtureRoot "SHA256SUMS.txt")) {
        $failures += "A relative -OutputPath was written against the process directory instead of PowerShell's location"
    }

    $empty = Join-Path $fixtureRoot "empty"
    New-Item -ItemType Directory -Path $empty -Force | Out-Null
    $none = Invoke-Writer -Directory $empty -OutputPath (Join-Path $empty "SHA256SUMS.txt")
    if ($none.ExitCode -eq 0) {
        $failures += "An empty asset directory should fail"
    } elseif ($none.Stderr -notmatch [regex]::Escape("No release assets")) {
        $failures += "An empty asset directory did not report 'No release assets'. Stderr: $($none.Stderr)"
    }
} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) {
        Write-Error $failure -ErrorAction Continue
    }
    exit 1
}

Write-Output "write-checksums.ps1: all checks passed"
