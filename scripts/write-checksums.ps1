[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Directory,

    [Parameter(Mandatory = $true)]
    [string]$OutputPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$outputFullPath = $PSCmdlet.GetUnresolvedProviderPathFromPSPath($OutputPath)
$checksumLines = @(Get-ChildItem -LiteralPath $Directory -File |
    Where-Object { $_.FullName -ne $outputFullPath } |
    Sort-Object Name |
    ForEach-Object {
        $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $($_.Name)"
    })

if ($checksumLines.Count -eq 0) {
    throw "No release assets were staged in $Directory."
}

[System.IO.File]::WriteAllText(
    $outputFullPath,
    ($checksumLines -join "`n") + "`n",
    [System.Text.UTF8Encoding]::new($false)
)
Get-Content -LiteralPath $outputFullPath
