<#
.SYNOPSIS
Builds the app and starts it without a console window.

.PARAMETER DataDir
Keeps database, covers, downloads and logins in this folder instead of the user data folder.

.PARAMETER Release
Builds the optimized version, which is faster but takes longer to build.
#>
param(
    [string]$DataDir,
    [switch]$Release
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$profileDir = if ($Release) { "release" } else { "debug" }
$buildArgs = @("build", "-p", "abs-desktop")
if ($Release) { $buildArgs += "--release" }

Push-Location $root
try {
    cargo @buildArgs
    if ($LASTEXITCODE) { throw "cargo build failed" }
} finally {
    Pop-Location
}

if ($DataDir) {
    New-Item -ItemType Directory -Force $DataDir | Out-Null
    $env:ABS_DESKTOP_DATA_DIR = (Resolve-Path $DataDir).Path
}
Start-Process -FilePath (Join-Path $root "target\$profileDir\abs-desktop.exe")
