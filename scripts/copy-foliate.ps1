<#
.SYNOPSIS
Copies the foliate-js files the reader loads (no tests, build files or source maps) to -Destination.
#>
param([Parameter(Mandatory)][string]$Destination)
$ErrorActionPreference = "Stop"
$source = Resolve-Path (Join-Path $PSScriptRoot "..\vendor\foliate-js")
$skipRoot = @("eslint.config.js", "rollup.config.js", "reader.js")

Remove-Item -Recurse -Force $Destination -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Destination | Out-Null
Get-ChildItem $source -File -Filter *.js | Where-Object { $skipRoot -notcontains $_.Name } |
    Copy-Item -Destination $Destination
Copy-Item (Join-Path $source "LICENSE") $Destination
foreach ($dir in "ui", "vendor") {
    Get-ChildItem (Join-Path $source $dir) -Recurse -File | Where-Object { $_.Extension -ne ".map" } | ForEach-Object {
        $target = Join-Path $Destination $_.FullName.Substring($source.Path.Length + 1)
        New-Item -ItemType Directory -Force (Split-Path $target) | Out-Null
        Copy-Item $_.FullName $target
    }
}
