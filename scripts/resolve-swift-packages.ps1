[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$PackagePath,

    # Test seams: the command that stands in for `xcrun`, and the base of the linear backoff
    # between attempts, so the tests neither run SwiftPM nor sleep. CI leaves both at the defaults.
    [string]$Resolver = "xcrun",

    [ValidateRange(0, 300)]
    [int]$RetryDelaySeconds = 15
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# The macOS jobs of ci.yml, bundle.yml and release.yml run this before their first cargo step.
#
# The browser billing helper's one remote dependency, SweetCookieKit, is otherwise cloned from
# inside a cargo build script, where a transient SwiftPM failure fails the whole build: a Bundle
# run on 2026-09-22 lost SwiftPM's shared repository cache mid-fetch. The build script builds into
# the package's default .build, so resolving here, with retries, leaves the checkout where the
# build finds it and the build script never fetches. A retry starts from an empty .build and
# bypasses the shared cache, since either may hold exactly the state that failed. For the same
# reason the workflows never cache ~/Library/Caches/org.swift.swiftpm.
#
# SideNotch has no remote dependencies, so only BrowserBilling needs this.
if (-not (Test-Path -LiteralPath (Join-Path $PackagePath "Package.swift") -PathType Leaf)) {
    throw "No Swift package at '$PackagePath': Package.swift is missing."
}

$attempts = 3
for ($attempt = 1; $attempt -le $attempts; $attempt++) {
    if ($attempt -eq 1) {
        & $Resolver swift package resolve --package-path $PackagePath
    } else {
        Remove-Item -LiteralPath (Join-Path $PackagePath ".build") -Recurse -Force -ErrorAction Ignore
        & $Resolver swift package resolve --package-path $PackagePath --disable-dependency-cache
    }
    if ($LASTEXITCODE -eq 0) {
        exit 0
    }
    if ($attempt -lt $attempts) {
        $delay = $RetryDelaySeconds * $attempt
        Write-Warning "Swift package resolution failed (attempt $attempt of $attempts); retrying in $delay s."
        Start-Sleep -Seconds $delay
    }
}

throw "Could not resolve the Swift packages in $PackagePath after $attempts attempts."
