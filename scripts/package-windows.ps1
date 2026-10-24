<#
.SYNOPSIS
Builds the release exe and packs it with libmpv and the license files into dist/abs-desktop-<version>-windows-x64.zip.

.NOTES
Needs libmpv in vendor/libmpv/x64 (scripts/fetch-libmpv.ps1) and cargo-about (cargo install cargo-about --features cli).
#>
param([string]$Version = "dev")
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$name = "abs-desktop-$Version-windows-x64"
$stage = Join-Path $root "dist\$name"
$licenses = Join-Path $stage "licenses"

cargo build --release --locked -p abs-desktop
if ($LASTEXITCODE) { throw "cargo build failed" }

Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $licenses | Out-Null
Copy-Item (Join-Path $root "target\release\abs-desktop.exe") $stage
Copy-Item (Join-Path $root "vendor\libmpv\x64\libmpv-2.dll") $stage
Copy-Item (Join-Path $root "LICENSE") $stage
Copy-Item (Join-Path $root "packaging\README.txt") $stage
Copy-Item (Join-Path $root "packaging\licenses\*") $licenses
Copy-Item (Join-Path $root "crates\app\ui\fonts\licenses\*") $licenses

cargo about generate --locked -c (Join-Path $root "packaging\about.toml") -m (Join-Path $root "crates\app\Cargo.toml") `
    (Join-Path $root "packaging\about.hbs") -o (Join-Path $stage "THIRD-PARTY-LICENSES.html")
if ($LASTEXITCODE) { throw "cargo about failed" }

$zip = Join-Path $root "dist\$name.zip"
Compress-Archive -Force -Path $stage -DestinationPath $zip
Write-Host "packed $zip"
