[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptUnderTest = Join-Path $PSScriptRoot "install-cargo-about.ps1"
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("on-n-off-cargo-about-test-" + [guid]::NewGuid().ToString("N"))
$powershell = (Get-Process -Id $PID).Path

function Invoke-Installer {
    param([string] $ArchivePath, [string] $GitHubPath)

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $powershell
    foreach ($argument in @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", $scriptUnderTest,
        "-Destination", $fixtureRoot,
        "-ArchivePath", $ArchivePath
    )) {
        $startInfo.ArgumentList.Add($argument)
    }
    $startInfo.Environment["GITHUB_PATH"] = $GitHubPath
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "Failed to start the cargo-about installer"
    }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()
    $result = [pscustomobject]@{
        ExitCode = $process.ExitCode
        Stdout   = $stdoutTask.GetAwaiter().GetResult()
        Stderr   = $stderrTask.GetAwaiter().GetResult()
    }
    $process.Dispose()
    return $result
}

$failures = @()
New-Item -ItemType Directory -Path $fixtureRoot -Force | Out-Null

try {
    $source = [System.IO.File]::ReadAllText($scriptUnderTest)
    $pins = [regex]::Matches($source, '"(?<triple>[a-z0-9_]+-[a-z0-9_-]+)"\s*=\s*"(?<hash>[^"]*)"')
    $triples = @($pins | ForEach-Object { $_.Groups["triple"].Value } | Sort-Object)
    $expectedTriples = @("aarch64-apple-darwin", "x86_64-pc-windows-msvc", "x86_64-unknown-linux-musl")
    if (Compare-Object $expectedTriples $triples) {
        $failures += "Expected pins for $($expectedTriples -join ', '); found $($triples -join ', ')"
    }
    foreach ($pin in $pins) {
        if ($pin.Groups["hash"].Value -cnotmatch '^[0-9a-f]{64}$') {
            $failures += "The pin for $($pin.Groups["triple"].Value) is not a lowercase SHA-256: $($pin.Groups["hash"].Value)"
        }
    }

    $tampered = Join-Path $fixtureRoot "tampered.tar.gz"
    [System.IO.File]::WriteAllText($tampered, "not cargo-about", [System.Text.UTF8Encoding]::new($false))
    $githubPath = Join-Path $fixtureRoot "github-path.txt"
    [System.IO.File]::WriteAllText($githubPath, "", [System.Text.UTF8Encoding]::new($false))

    $refused = Invoke-Installer -ArchivePath $tampered -GitHubPath $githubPath
    if ($refused.ExitCode -eq 0) {
        $failures += "An archive whose SHA-256 is not the pin should be refused"
    } elseif ((($refused.Stderr -replace '\x1b\[[0-9;]*m', '') -replace '[\s|]+', ' ') -notmatch "not the pinned") {
        $failures += "A tampered archive did not report 'not the pinned'. Stderr: $($refused.Stderr)"
    }
    if ([System.IO.File]::ReadAllText($githubPath) -ne "") {
        $failures += "A refused archive must not add anything to GITHUB_PATH"
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

Write-Output "install-cargo-about.ps1: all checks passed"
