<#
.SYNOPSIS
Verifies a distribution.json against the Hambugui launcher parser with the NeoForge patch applied.

.DESCRIPTION
Copies hambugui-distribution/hambugui-domain out of the (read-only) launcher checkout into a temp
workspace, applies the NeoForge patch there, builds scripts/launcher-contract-harness against the
patched crate, and runs it on the given file. The launcher checkout is never modified.

-Patch defaults to hambugui-neoforge-from-main.patch, which is correct when the launcher checkout is
on main. If it is on branch wip/distribution-neoforge-module-type (419a3af), pass
-Patch docs\launcher\hambugui-neoforge.patch instead. Both bases end up at the same tree.

.EXAMPLE
scripts\verify-launcher-contract.ps1 -Distribution fixtures\launcher\nebula-valid.json -Runtime
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Distribution,
    [switch]$Runtime,
    [string]$Target = 'windows-x86_64',
    [string]$LauncherDir = 'H:\HambuguiLauncherRust',
    [string]$Patch
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if (-not $Patch) { $Patch = Join-Path $repo 'docs\launcher\hambugui-neoforge-from-main.patch' }
$patch = (Resolve-Path $Patch).Path
$distribution = (Resolve-Path $Distribution).Path
foreach ($required in @($patch, (Join-Path $LauncherDir 'crates\hambugui-distribution'))) {
    if (-not (Test-Path $required)) { throw "missing: $required" }
}

$temp = Join-Path ([IO.Path]::GetTempPath()) ('hambugui-contract-' + [Guid]::NewGuid().ToString('N').Substring(0, 8))
try {
    New-Item -ItemType Directory -Path (Join-Path $temp 'crates'), (Join-Path $temp 'docs'), (Join-Path $temp 'harness') | Out-Null
    foreach ($crate in 'hambugui-distribution', 'hambugui-domain') {
        Copy-Item -Recurse (Join-Path $LauncherDir "crates\$crate") (Join-Path $temp 'crates')
    }
    Copy-Item (Join-Path $LauncherDir 'docs\distribution.md') (Join-Path $temp 'docs')
    Copy-Item -Recurse (Join-Path $PSScriptRoot 'launcher-contract-harness\src') (Join-Path $temp 'harness')

    # Same [workspace.dependencies] pins as the launcher root manifest, minus crates we do not build.
    Set-Content -Path (Join-Path $temp 'Cargo.toml') -Encoding utf8 -Value @'
[workspace]
resolver = "3"
members = ["crates/hambugui-domain", "crates/hambugui-distribution", "harness"]

[workspace.package]
version = "0.1.0-rc.1"
edition = "2024"
license = "GPL-3.0-only"
rust-version = "1.97"

[workspace.dependencies]
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.145"
semver = { version = "1.0.27", features = ["serde"] }
url = "2.5.7"
'@
    Set-Content -Path (Join-Path $temp 'harness\Cargo.toml') -Encoding utf8 -Value @'
[package]
name = "contract-harness"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
hambugui-distribution = { path = "../crates/hambugui-distribution" }
'@

    & git -C $temp apply -p1 $patch
    if ($LASTEXITCODE -ne 0) { throw "git apply failed for $patch" }
    Write-Host "patched launcher copy: $temp" -ForegroundColor DarkGray

    $harnessArgs = @($distribution)
    if ($Runtime) {
        $version = (Get-Content -Raw $distribution | ConvertFrom-Json).version
        $harnessArgs += @('--runtime', $version, $Target)
    }
    & cargo run --quiet --manifest-path (Join-Path $temp 'Cargo.toml') -p contract-harness -- @harnessArgs
    exit $LASTEXITCODE
}
finally {
    Remove-Item -Recurse -Force $temp -ErrorAction SilentlyContinue
}
