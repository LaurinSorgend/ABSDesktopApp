<#
.SYNOPSIS
Builds the release exes and packs them with libmpv, foliate-js and the license files into dist/abs-desktop-<version>-windows-x64.zip.

.DESCRIPTION
The zip has no self-update; winget and Scoop install it. With -SelfUpdate the build has the
self-update feature and the folder is left unzipped for scripts/package-velopack.ps1.

.NOTES
Needs libmpv in vendor/libmpv/x64 (scripts/fetch-libmpv.ps1) and cargo-about (cargo install cargo-about --features cli).
#>
param([string]$Version = "dev", [switch]$SelfUpdate)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$name = if ($SelfUpdate) { "abs-desktop-$Version-windows-x64-velopack" } else { "abs-desktop-$Version-windows-x64" }
$stage = Join-Path $root "dist\$name"
$licenses = Join-Path $stage "licenses"

# A new value makes the build fetch the latest themes.
$env:ABS_THEMES_FETCH = Get-Date -Format o
$features = if ($SelfUpdate) { @("--features", "abs-desktop/self-update") } else { @() }
cargo build --release --locked -p abs-desktop -p abs-reader @features
if ($LASTEXITCODE) { throw "cargo build failed" }

Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $licenses | Out-Null
Copy-Item (Join-Path $root "target\release\abs-desktop.exe") $stage
Copy-Item (Join-Path $root "target\release\abs-reader.exe") $stage
& (Join-Path $PSScriptRoot "copy-foliate.ps1") -Destination (Join-Path $stage "foliate-js")
Copy-Item (Join-Path $root "vendor\libmpv\x64\libmpv-2.dll") $stage
Copy-Item (Join-Path $root "LICENSE") $stage
Copy-Item (Join-Path $root "packaging\README.txt") $stage
Copy-Item (Join-Path $root "packaging\licenses\*") $licenses
Copy-Item (Join-Path $root "crates\app\ui\fonts\licenses\*") $licenses

cargo about generate --locked --workspace -c (Join-Path $root "packaging\about.toml") -m (Join-Path $root "Cargo.toml") `
    (Join-Path $root "packaging\about.hbs") -o (Join-Path $stage "THIRD-PARTY-LICENSES.html")
if ($LASTEXITCODE) { throw "cargo about failed" }

if ($SelfUpdate) {
    Write-Host "staged $stage"
    return
}
$zip = Join-Path $root "dist\$name.zip"
Compress-Archive -Force -Path $stage -DestinationPath $zip
Write-Host "packed $zip"
