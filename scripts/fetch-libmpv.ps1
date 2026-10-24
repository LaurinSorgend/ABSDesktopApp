<#
.SYNOPSIS
Downloads a prebuilt libmpv for Windows into vendor/libmpv/x64 and creates the MSVC import library.

.NOTES
Needs gh, 7z and the MSVC tools (lib.exe, dumpbin.exe) on PATH or findable via vswhere.
The shinchiro build is GPL; it is for development only until we ship our own LGPL build.
#>
param(
    [string]$Tag = "20261021",
    [string]$Asset = "mpv-dev-x86_64-20261021-git-e471496c56.7z"
)
$ErrorActionPreference = "Stop"
$root = Join-Path $PSScriptRoot "..\vendor\libmpv"
$out = Join-Path $root "x64"
$download = Join-Path $root "dl"

function Find-MsvcTool([string]$name) {
    $onPath = Get-Command $name -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    $install = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    $found = Get-ChildItem -Recurse -Filter $name (Join-Path $install "VC\Tools\MSVC") |
        Where-Object FullName -like "*\Hostx64\x64\*" | Select-Object -First 1
    if (-not $found) { throw "$name not found" }
    return $found.FullName
}

New-Item -ItemType Directory -Force $download, $out | Out-Null
gh release download $Tag -R shinchiro/mpv-winbuild-cmake -p $Asset -D $download --clobber
7z x -y "-o$out" (Join-Path $download $Asset) | Out-Null

$exports = & (Find-MsvcTool "dumpbin.exe") -nologo -exports (Join-Path $out "libmpv-2.dll")
$names = $exports | ForEach-Object { ($_ -split '\s+')[-1] } | Where-Object { $_ -like "mpv_*" }
$def = Join-Path $out "mpv.def"
@("LIBRARY libmpv-2", "EXPORTS") + $names | Set-Content -Encoding ascii $def
& (Find-MsvcTool "lib.exe") -nologo "-def:$def" -machine:x64 "-out:$(Join-Path $out 'mpv.lib')" | Out-Null
Write-Host "libmpv ready in $out ($($names.Count) exports)"
