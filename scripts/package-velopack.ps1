<#
.SYNOPSIS
Builds the self-updating Windows release with Velopack: Setup.exe, Portable.zip and the update packages in dist/velopack.

.NOTES
Needs everything scripts/package-windows.ps1 needs, plus vpk (dotnet tool install -g vpk --version 1.2.161).
The vpk version should match the velopack crate in crates/app/Cargo.toml.
With -Repository it first downloads the newest published release so vpk can build a delta update.
#>
param(
    [string]$Version = "0.0.0-dev",
    [string]$Repository = ""
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$semver = $Version.TrimStart("v")
$output = Join-Path $root "dist\velopack"
$stage = Join-Path $root "dist\abs-desktop-$semver-windows-x64-velopack"

& (Join-Path $PSScriptRoot "package-windows.ps1") -Version $semver -SelfUpdate

Remove-Item -Recurse -Force $output -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $output | Out-Null
if ($Repository) {
    vpk download github --repoUrl $Repository --outputDir $output
    if ($LASTEXITCODE) { Write-Warning "no earlier release to build a delta from" }
}

vpk pack --packId AbsDesktop --packVersion $semver --runtime win-x64 --packDir $stage --mainExe abs-desktop.exe `
    --packTitle "ABS Desktop" --packAuthors "Laurin Sorgend" `
    --icon (Join-Path $root "packaging\icons\abs-desktop.ico") --outputDir $output
if ($LASTEXITCODE) { throw "vpk pack failed" }
Write-Host "packed $output"
